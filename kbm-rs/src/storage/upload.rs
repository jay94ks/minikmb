//! Single-use REST staging. Only app-owned temporary files reach import jobs.
use super::*;
use axum::body::Body;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
const TTL: Duration = Duration::from_secs(600);
struct Ticket {
    binding: Binding,
    owner: Principal,
    spec: import::Spec,
    wire: Arc<dyn Wire>,
    deadline: Instant,
}
pub struct Tickets {
    pending: Mutex<HashMap<String, Ticket>>,
    slot: Arc<tokio::sync::Semaphore>,
}
impl Default for Tickets {
    fn default() -> Self {
        Self {
            pending: Default::default(),
            slot: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}
impl Ticket {
    fn check(&self, manager: &Manager) -> Result<(), AppError> {
        check_owner(&self.owner)?;
        manager.active.validate(&self.binding)?;
        if manager.state.lock().stopped {
            return Err(AppError::new("OPERATION_CANCELLED", "앱이 종료 중입니다."));
        }
        if !self.wire.live() {
            return Err(AppError::new(
                "STALE_SESSION",
                "업로드 주소를 발급한 MCU 연결이 종료됐습니다.",
            ));
        }
        Ok(())
    }
}
impl Manager {
    pub(super) fn clear_uploads(&self) {
        self.uploads.pending.lock().clear();
    }
    pub fn reap_uploads(&self) {
        self.uploads
            .pending
            .lock()
            .retain(|_, ticket| ticket.deadline > Instant::now() && ticket.check(self).is_ok());
    }
    pub fn prepare_upload(
        &self,
        binding: Binding,
        owner: Principal,
        spec: import::Spec,
        address: std::net::SocketAddr,
    ) -> Result<Value, AppError> {
        spec.validate()?;
        self.active.validate(&binding)?;
        check_owner(&owner)?;
        {
            let s = self.state.lock();
            if s.stopped || s.suspended {
                return Err(AppError::new(
                    "CHANNEL_SWITCHING",
                    "저장소를 정리 중입니다.",
                ));
            }
        }
        self.reap_uploads();
        let wire = self.backend.connect(&binding)?;
        let mut b = [0; 32];
        getrandom::fill(&mut b)
            .map_err(|_| AppError::new("RANDOM_FAILED", "업로드 주소 생성 실패"))?;
        let id: String = b.iter().map(|n| format!("{n:02x}")).collect();
        let size = spec.size();
        let mut pending = self.uploads.pending.lock();
        if pending.len() >= 32 {
            return Err(AppError::new(
                "QUEUE_FULL",
                "사용하지 않은 업로드 주소가 너무 많습니다.",
            ));
        }
        pending.insert(
            id.clone(),
            Ticket {
                binding,
                owner,
                spec,
                wire,
                deadline: Instant::now() + TTL,
            },
        );
        let url = format!("http://{address}/api/v1/active/storage/uploads/{id}");
        Ok(
            json!({"upload_id":id,"url":url,"method":"POST","content_type":"application/octet-stream","content_length":size,"expires_at":now_ms()+TTL.as_millis()as u64,"single_use":true,"instructions":"같은 API 키의 Bearer 인증으로 파일 원문을 POST하세요. MCP 메시지에는 파일 본문을 넣지 마세요. 응답 job_id를 storage_job_get으로 조회해 SD 검증 완료를 확인하세요.","example":format!("curl.exe -X POST -H 'Authorization: Bearer <API_KEY>' -H 'Content-Type: application/octet-stream' --data-binary '@<FILE_PATH>' '{url}'")}),
        )
    }
    pub async fn receive_upload(
        self: &Arc<Self>,
        owner: Principal,
        id: &str,
        length: Option<u64>,
        body: Body,
    ) -> Result<JobInfo, AppError> {
        check_owner(&owner)?;
        let _slot = self
            .uploads
            .slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::new("QUEUE_FULL", "다른 파일을 수신 중입니다."))?;
        let ticket = {
            let mut pending = self.uploads.pending.lock();
            let ticket = pending.get(id).ok_or_else(|| {
                AppError::new("UPLOAD_NOT_FOUND", "사용했거나 만료된 업로드 주소입니다.")
            })?;
            authorize(&ticket.owner, &owner)?;
            if ticket.deadline <= Instant::now() {
                pending.remove(id);
                return Err(AppError::new(
                    "UPLOAD_NOT_FOUND",
                    "업로드 주소가 만료됐습니다.",
                ));
            }
            ticket.check(self)?;
            if length != Some(ticket.spec.size()) {
                return Err(AppError::new(
                    "UPLOAD_LENGTH_MISMATCH",
                    "준비한 크기와 같은 Content-Length가 필요합니다.",
                ));
            }
            pending.remove(id).unwrap()
        };
        let file = tokio::task::spawn_blocking(tempfile::tempfile)
            .await
            .map_err(|_| import::file_error())?
            .map_err(|_| import::file_error())?;
        let mut out = tokio::fs::File::from_std(file);
        let mut stream = body.into_data_stream();
        let mut done = 0u64;
        let mut last_progress = Instant::now();
        loop {
            ticket.check(self)?;
            check_owner(&owner)?;
            if last_progress.elapsed() > Duration::from_secs(30) {
                return Err(AppError::new(
                    "UPLOAD_TIMEOUT",
                    "파일 수신이30초 동안 진행되지 않았습니다.",
                ));
            }
            let next = tokio::select! {biased;
            _=ticket.owner.cancelled.cancelled()=>return Err(AppError::new("UNAUTHORIZED","업로드 키가 폐기됐습니다.")),
            _=ticket.owner.ended.cancelled()=>return Err(AppError::new("UNAUTHORIZED","업로드 생성 연결이 종료됐습니다.")),
            _=owner.ended.cancelled()=>return Err(AppError::new("UNAUTHORIZED","REST 서버가 종료됐습니다.")),
            next=tokio::time::timeout(Duration::from_millis(250),stream.next())=>match next{Ok(value)=>value,Err(_)=>continue},};
            let Some(bytes) = next else {
                break;
            };
            let bytes = bytes.map_err(|_| {
                AppError::new("UPLOAD_INTERRUPTED", "파일 수신 연결이 종료됐습니다.")
            })?;
            if bytes.len() as u64 > ticket.spec.size() - done {
                return Err(AppError::new(
                    "UPLOAD_LENGTH_MISMATCH",
                    "수신 파일이 준비한 크기보다 큽니다.",
                ));
            }
            for part in bytes.chunks(65536) {
                ticket.check(self)?;
                check_owner(&owner)?;
                out.write_all(part)
                    .await
                    .map_err(|_| import::file_error())?;
                done += part.len() as u64;
                last_progress = Instant::now();
            }
        }
        if done != ticket.spec.size() {
            return Err(AppError::new(
                "UPLOAD_LENGTH_MISMATCH",
                "수신 파일이 준비한 크기보다 작습니다.",
            ));
        }
        out.flush().await.map_err(|_| import::file_error())?;
        ticket.check(self)?;
        check_owner(&owner)?;
        let file = out.into_std().await;
        self.start_import(
            ticket.binding,
            ticket.owner,
            ticket.spec,
            file,
            ticket.wire.session(),
        )
    }
}
