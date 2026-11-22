//! Bounded, owner-scoped storage operations; MCU job acceptance is not completion.
use crate::{
    active::{ActiveCoordinator, Binding},
    input_sessions::Principal,
    model::{now_ms, AppError, Uid},
    storage_wire::{self as abi, Backend, Card, Cleanup, Image, McuJob, Snapshot, Wire},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
#[derive(Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Refresh {},
    CardInfo {},
    CardFormatStatus {},
    CardFormat {
        format: u8,
        confirmed: bool,
        expected_card_uuid: Option<String>,
    },
    NameGet {},
    NameSet {
        name: String,
    },
    UserRead {},
    UserWrite {
        #[schemars(length(max = 4096))]
        bytes: Vec<u8>,
    },
    OpticalSet {
        enabled: bool,
    },
    OpticalMount {
        id: u32,
        mounted: bool,
        expected_card_uuid: Option<String>,
    },
    DiskOpen {
        id: u32,
        path: media::Path,
        writable: bool,
        expected_card_uuid: Option<String>,
    },
    DiskClose {
        id: u32,
    },
    DiskSync {
        id: u32,
    },
    DiskCreate {
        format: u8,
        size_mib: u32,
        confirmed: bool,
        expected_card_uuid: Option<String>,
    },
    FolderShare {
        source_id: String,
        confirmed: bool,
        expected_card_uuid: Option<String>,
    },
    FolderForget {
        source_id: String,
    },
    DiskFormat {
        id: u32,
        format: u8,
        confirmed: bool,
        expected_size: Option<u64>,
        expected_card_uuid: Option<String>,
    },
    DiskRead {
        id: u32,
        lba: u32,
        expected_card_uuid: Option<String>,
    },
    DiskWrite {
        id: u32,
        lba: u32,
        #[schemars(length(min = 512, max = 512))]
        bytes: Vec<u8>,
        confirmed: bool,
        expected_lease: Option<u32>,
        expected_card_uuid: Option<String>,
    },
}
pub mod card_format;
pub mod folder;
pub mod folder_image;
pub mod format;
pub mod import;
pub mod media;
pub mod optical;
#[cfg(test)]
pub(crate) mod tests;
pub mod transfer;
pub mod upload;
impl Action {
    fn label(&self) -> &'static str {
        match self {
            Self::Refresh {} => "refresh",
            Self::CardInfo {} => "card_info",
            Self::CardFormatStatus {} => "card_format_status",
            Self::CardFormat { .. } => "card_format",
            Self::NameGet {} => "name_get",
            Self::NameSet { .. } => "name_set",
            Self::UserRead {} => "user_read",
            Self::UserWrite { .. } => "user_write",
            Self::OpticalSet { .. } => "optical_set",
            Self::OpticalMount { .. } => "optical_mount",
            Self::DiskOpen { .. } => "disk_open",
            Self::DiskClose { .. } => "disk_close",
            Self::DiskSync { .. } => "disk_sync",
            Self::DiskCreate { .. } => "disk_create",
            Self::FolderShare { .. } => "folder_share",
            Self::FolderForget { .. } => "folder_forget",
            Self::DiskFormat { .. } => "disk_format",
            Self::DiskRead { .. } => "disk_read",
            Self::DiskWrite { .. } => "disk_write",
        }
    }
    fn normalize(self) -> Result<Self, AppError> {
        Ok(match self {
            Self::FolderForget { source_id } => {
                if source_id.len() != 64 || !source_id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(AppError::new(
                        "INVALID_ARGUMENTS",
                        "올바른 폴더 source_id가 필요합니다.",
                    ));
                }
                Self::FolderForget { source_id }
            }
            Self::FolderShare {
                source_id,
                confirmed,
                expected_card_uuid,
            } => {
                if !confirmed
                    || source_id.len() != 64
                    || !source_id.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err(AppError::new(
                        "INVALID_ARGUMENTS",
                        "앱에서 선택한 폴더 source_id와 confirmed:true가 필요합니다.",
                    ));
                }
                Self::FolderShare {
                    source_id,
                    confirmed,
                    expected_card_uuid,
                }
            }
            a @ Self::CardFormat {
                format, confirmed, ..
            } => {
                if !confirmed || ![1, 2].contains(&format) {
                    return Err(AppError::new(
                        "INVALID_ARGUMENTS",
                        "전체 카드 포맷은 확인된 FAT32/exFAT만 지원합니다.",
                    ));
                }
                a
            }
            Self::NameSet { name } => Self::NameSet {
                name: abi::channel_name(&name)?,
            },
            Self::UserWrite { bytes } if bytes.len() > 4096 => {
                return Err(AppError::new(
                    "INVALID_ARGUMENTS",
                    "사용자 값은 최대4096바이트입니다.",
                ))
            }
            a @ Self::DiskCreate {
                format,
                size_mib,
                confirmed,
                ..
            } => {
                if !confirmed {
                    return Err(AppError::new(
                        "INVALID_ARGUMENTS",
                        "새 이미지 생성을 확인해야 합니다.",
                    ));
                }
                format::Request {
                    format,
                    bytes: u64::from(size_mib) * 1048576,
                    existing: false,
                }
                .validate()?;
                a
            }
            a @ Self::DiskFormat {
                id,
                format,
                confirmed,
                ..
            } => {
                if !confirmed || !(1..=0xfffffff).contains(&id) || ![1, 2, 3].contains(&format) {
                    return Err(AppError::new(
                        "INVALID_ARGUMENTS",
                        "포맷할 이미지 ID·종류·확인 여부를 확인하세요.",
                    ));
                }
                a
            }
            Self::DiskWrite {
                ref bytes,
                confirmed,
                ..
            } if bytes.len() != 512 || !confirmed => {
                return Err(AppError::new(
                    "INVALID_ARGUMENTS",
                    "확인된512바이트 섹터 데이터가 필요합니다.",
                ))
            }
            a @ (Self::OpticalMount { id, .. }
            | Self::DiskOpen { id, .. }
            | Self::DiskClose { id }
            | Self::DiskSync { id }
            | Self::DiskRead { id, .. }
            | Self::DiskWrite { id, .. }) => {
                if id == 0 || id > 0x0fffffff {
                    return Err(AppError::new(
                        "INVALID_ARGUMENTS",
                        "이미지 ID 범위를 확인하세요.",
                    ));
                }
                a
            }
            a => a,
        })
    }
    fn has_mcu_job(&self) -> bool {
        !matches!(
            self,
            Self::CardInfo {}
                | Self::CardFormatStatus {}
                | Self::NameGet {}
                | Self::UserRead {}
                | Self::FolderForget { .. }
        )
    }
}
#[derive(Clone, Serialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}
#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct JobInfo {
    pub job_id: String,
    pub uid: Uid,
    pub generation: u64,
    pub kind: &'static str,
    pub state: JobState,
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub can_cancel: bool,
    pub side_effects_possible: bool,
    pub mcu_job: Option<McuJob>,
    pub error: Option<AppError>,
    pub cleanup_error: Option<AppError>,
    pub result: Option<Value>,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    pub transfer: Option<transfer::Progress>,
}
struct Job {
    binding: Binding,
    owner: Principal,
    cancel: CancellationToken,
    info: Mutex<JobInfo>,
    finished: watch::Sender<bool>,
    meter: Mutex<Option<transfer::Meter>>,
}
impl Job {
    fn snapshot(&self) -> JobInfo {
        let mut info = self.info.lock().clone();
        if info.finished_at.is_none() {
            info.transfer = self
                .meter
                .lock()
                .as_mut()
                .map(|m| m.progress(Instant::now()));
        }
        info
    }
    async fn wait(&self) {
        let mut rx = self.finished.subscribe();
        if !*rx.borrow() {
            let _ = rx.changed().await;
        }
    }
}
#[derive(Default)]
struct State {
    jobs: VecDeque<Arc<Job>>,
    current: Option<Arc<Job>>,
    suspended: bool,
    stopped: bool,
    uncertain: HashMap<Uid, u64>,
    cache: HashMap<Uid, Snapshot>,
    media: Option<media::Held>,
    media_error: Option<AppError>,
    card_format: Option<card_format::Hold>,
}
pub struct Manager {
    logger: Mutex<Option<std::sync::Weak<crate::logging::Journal>>>,
    active: Arc<ActiveCoordinator>,
    backend: Arc<dyn Backend>,
    state: Mutex<State>,
    uploads: upload::Tickets,
    formatter: Arc<dyn format::Engine>,
    folder_sources: Mutex<HashMap<String, folder::Source>>,
}
pub struct Transition {
    manager: Arc<Manager>,
}
impl Drop for Transition {
    fn drop(&mut self) {
        self.manager.state.lock().suspended = false;
    }
}
impl Manager {
    pub fn set_logger(&self, logger: &Arc<crate::logging::Journal>) {
        *self.logger.lock() = Some(Arc::downgrade(logger));
    }
    pub fn import_session(&self, binding: &Binding) -> Result<u64, AppError> {
        self.active.validate(binding)?;
        let wire = self.backend.connect(binding)?;
        if !wire.live() {
            return Err(AppError::new("STALE_SESSION", "MCU 연결이 종료됐습니다."));
        }
        self.active.validate(binding)?;
        Ok(wire.session())
    }
    pub fn new(active: Arc<ActiveCoordinator>, backend: Arc<dyn Backend>) -> Arc<Self> {
        Arc::new(Self {
            logger: Default::default(),
            active,
            backend,
            state: Default::default(),
            uploads: Default::default(),
            formatter: Arc::new(format::Native),
            folder_sources: Default::default(),
        })
    }
    pub fn start(
        self: &Arc<Self>,
        binding: Binding,
        owner: Principal,
        action: Action,
    ) -> Result<JobInfo, AppError> {
        let action = action.normalize()?;
        self.start_operation(binding, owner, Operation::Action(action))
    }
    pub fn start_import(
        self: &Arc<Self>,
        binding: Binding,
        owner: Principal,
        spec: import::Spec,
        file: std::fs::File,
        session: u64,
    ) -> Result<JobInfo, AppError> {
        spec.validate()?;
        self.start_operation(binding, owner, Operation::Import(spec, file, session))
    }
    fn start_operation(
        self: &Arc<Self>,
        binding: Binding,
        owner: Principal,
        operation: Operation,
    ) -> Result<JobInfo, AppError> {
        self.active.validate(&binding)?;
        check_owner(&owner)?;
        let wire = self.backend.connect(&binding)?;
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)
            .map_err(|_| AppError::new("RANDOM_FAILED", "저장소 작업 ID를 만들지 못했습니다."))?;
        let id: String = random.iter().map(|n| format!("{n:02x}")).collect();
        let info = JobInfo {
            job_id: id,
            uid: binding.uid.clone(),
            generation: binding.generation,
            kind: operation.label(),
            state: JobState::Queued,
            phase: String::new(),
            done: 0,
            total: 0,
            can_cancel: true,
            side_effects_possible: false,
            mcu_job: None,
            error: None,
            cleanup_error: None,
            result: None,
            created_at: now_ms(),
            finished_at: None,
            transfer: None,
        };
        let job = Arc::new(Job {
            binding: binding.clone(),
            cancel: owner.cancelled.child_token(),
            owner,
            info: Mutex::new(info.clone()),
            finished: watch::channel(false).0,
            meter: Default::default(),
        });
        {
            let mut s = self.state.lock();
            if s.stopped || s.suspended {
                return Err(AppError::new(
                    "CHANNEL_SWITCHING",
                    "저장소를 정리하거나 채널을 전환 중입니다.",
                ));
            }
            if s.current.is_some() {
                return Err(AppError::new(
                    "STORAGE_BUSY",
                    "다른 저장소 작업이 진행 중입니다.",
                ));
            }
            if s.card_format.as_ref().is_some_and(|h| {
                h.info.uid != binding.uid
                    || !matches!(
                        &operation,
                        Operation::Action(Action::CardFormatStatus {} | Action::CardInfo {})
                    )
            }) {
                return Err(card_format::pending());
            }
            if let Operation::Import(_, _, session) = &operation {
                if *session != wire.session() {
                    return Err(AppError::new(
                        "STALE_SESSION",
                        "업로드를 준비한 MCU 연결이 변경됐습니다.",
                    ));
                }
            }
            if s.uncertain.get(&binding.uid) == Some(&wire.session()) && operation.has_mcu_job() {
                return Err(AppError::new(
                    "STORAGE_UNCERTAIN",
                    "이전 MCU 저장소 작업의 종료를 확인하지 못했습니다. 관리 USB를 재연결하세요.",
                ));
            }
            if s.uncertain
                .get(&binding.uid)
                .is_some_and(|session| *session != wire.session())
            {
                s.uncertain.remove(&binding.uid);
            }
            while s.jobs.len() >= 64 {
                s.jobs.pop_back();
            }
            s.jobs.push_front(job.clone());
            s.current = Some(job.clone());
        }
        let manager = self.clone();
        tokio::spawn(async move {
            use futures_util::FutureExt;
            job.info.lock().state = JobState::Running;
            let mut work = Work {
                manager: manager.clone(),
                job: job.clone(),
                wire,
                mcu: None,
                user_transfer: None,
                starting_job: false,
                starting_user: false,
                claimed_disk: false,
                claimed_optical: false,
                upload: None,
                host_cleanup: None,
            };
            let result = std::panic::AssertUnwindSafe(async {
                match operation {
                    Operation::Action(action) => work.run(action).await,
                    Operation::Import(spec, file, _) => work.import_image(spec, file).await,
                }
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                Err(AppError::uncertain(
                    "STORAGE_FAILED",
                    "저장소 작업 처리 중 오류가 발생했습니다.",
                ))
            });
            let wire_cleanup = if result.is_err() {
                work.cleanup().await.err()
            } else {
                None
            };
            let cleanup = match (wire_cleanup.clone(), work.host_cleanup.take()) {
                (Some(mut wire), Some(host)) => {
                    wire.message.push_str(&format!(" {}", host.message));
                    Some(wire)
                }
                (Some(wire), None) => Some(wire),
                (None, host) => host,
            };
            let mut s = manager.state.lock();
            if wire_cleanup.is_some() && work.wire.live() {
                s.uncertain
                    .insert(job.binding.uid.clone(), work.wire.session());
            }
            if result.is_err() {
                if let Some(snapshot) = s.cache.get_mut(&job.binding.uid) {
                    snapshot.stale = true;
                }
            }
            {
                let mut info = job.info.lock();
                info.can_cancel = false;
                info.finished_at = Some(now_ms());
                info.cleanup_error = cleanup;
                match result {
                    Ok(value) => {
                        info.state = JobState::Completed;
                        info.phase = "완료".into();
                        info.result = Some(value);
                    }
                    Err(e) => {
                        info.state = if ["OPERATION_CANCELLED", "UNAUTHORIZED", "STALE_OPERATION"]
                            .contains(&e.code)
                        {
                            JobState::Cancelled
                        } else {
                            JobState::Failed
                        };
                        info.error = Some(e);
                    }
                }
                if let Some(logger) = manager
                    .logger
                    .lock()
                    .as_ref()
                    .and_then(std::sync::Weak::upgrade)
                {
                    let success = info.state == JobState::Completed && info.cleanup_error.is_none();
                    logger.emit(
                        if success {
                            crate::logging::Level::Info
                        } else {
                            crate::logging::Level::Warn
                        },
                        crate::logging::Event::StorageJobChanged,
                        crate::logging::Fields {
                            uid: Some(info.uid.clone()),
                            generation: Some(info.generation),
                            command: Some(info.kind),
                            success: Some(success),
                            bytes: Some(info.done),
                            code: info
                                .error
                                .as_ref()
                                .or(info.cleanup_error.as_ref())
                                .map(|e| e.code),
                            uncertain: Some(!success && info.side_effects_possible),
                            ..Default::default()
                        },
                    );
                }
            }
            if s.current
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &job))
            {
                s.current = None;
            }
            drop(s);
            job.finished.send_replace(true);
        });
        Ok(info)
    }
    pub fn job(&self, owner: &Principal, id: &str) -> Result<JobInfo, AppError> {
        check_owner(owner)?;
        let s = self.state.lock();
        let j = s
            .jobs
            .iter()
            .find(|j| j.info.lock().job_id == id)
            .ok_or_else(|| AppError::new("JOB_NOT_FOUND", "저장소 작업을 찾을 수 없습니다."))?;
        authorize(&j.owner, owner)?;
        let info = j.snapshot();
        Ok(info)
    }
    pub fn cancel(&self, owner: &Principal, id: &str) -> Result<JobInfo, AppError> {
        check_owner(owner)?;
        let s = self.state.lock();
        let j = s
            .jobs
            .iter()
            .find(|j| j.info.lock().job_id == id)
            .ok_or_else(|| AppError::new("JOB_NOT_FOUND", "저장소 작업을 찾을 수 없습니다."))?;
        authorize(&j.owner, owner)?;
        let info = j.info.lock().clone();
        if info.can_cancel {
            j.cancel.cancel();
        } else if info.finished_at.is_none() {
            return Err(AppError::new(
                "JOB_NOT_CANCELLABLE",
                "수락된 전체 카드 포맷은 취소할 수 없습니다.",
            )
            .at_job(&j.binding, &info.job_id));
        }
        Ok(info)
    }
    pub fn cached(&self, b: &Binding) -> Option<Snapshot> {
        self.state.lock().cache.get(&b.uid).map(|s| {
            let mut s = s.clone();
            s.stale |= s.generation != b.generation;
            s
        })
    }
    pub fn current(&self, b: &Binding) -> Option<JobInfo> {
        self.state
            .lock()
            .current
            .as_ref()
            .filter(|j| j.binding == *b)
            .map(|j| j.snapshot())
    }
    // Reserve before active.begin(): a rejected irreversible operation must not
    // invalidate the active generation, video or input sessions.
    pub fn reserve_switch(self: &Arc<Self>, target: Option<&Uid>) -> Result<Transition, AppError> {
        let mut s = self.state.lock();
        if s.suspended || s.stopped {
            return Err(AppError::new(
                "STORAGE_BUSY",
                "다른 저장소 정리가 진행 중입니다.",
            ));
        }
        if s.card_format
            .as_ref()
            .is_some_and(|h| target != Some(&h.info.uid))
        {
            return Err(card_format::pending());
        }
        if let Some(j) = &s.current {
            let info = j.info.lock();
            if info.finished_at.is_none() && !info.can_cancel {
                return Err(card_format::pending());
            }
            j.cancel.cancel();
        }
        s.suspended = true;
        Ok(Transition {
            manager: self.clone(),
        })
    }
    pub async fn prepare_switch(
        self: &Arc<Self>,
        transition: &Transition,
        b: Option<&Binding>,
        target: Option<&Uid>,
    ) -> Result<(), AppError> {
        if !Arc::ptr_eq(self, &transition.manager) {
            return Err(AppError::new("STORAGE_BUSY", "잘못된 저장소 전환입니다."));
        }
        let (current, uncertain) = {
            let s = self.state.lock();
            (
                s.current.clone(),
                b.and_then(|b| s.uncertain.get(&b.uid).copied()),
            )
        };
        if let (Some(b), Some(session)) = (b, uncertain) {
            if self
                .backend
                .connect(b)
                .is_ok_and(|w| w.live() && w.session() == session)
            {
                return Err(AppError::new(
                    "STORAGE_UNCERTAIN",
                    "현재 채널의 저장소 종료 상태를 먼저 확인해야 합니다.",
                ));
            }
        }
        if let Some(j) = current {
            if j.info.lock().finished_at.is_none() && !j.info.lock().can_cancel {
                return Err(AppError::new(
                    "STORAGE_BUSY",
                    "수락된 취소 불가 저장소 작업을 기다리세요.",
                ));
            }
            j.cancel.cancel();
            tokio::time::timeout(Duration::from_secs(10), j.wait())
                .await
                .map_err(|_| {
                    AppError::new("STORAGE_BUSY", "저장소 작업 정리를 기다리고 있습니다.")
                })?;
            if j.info.lock().cleanup_error.is_some() {
                return Err(AppError::new(
                    "STORAGE_UNCERTAIN",
                    "원래 MCU의 저장소 작업 종료를 확인하지 못했습니다.",
                ));
            }
        }
        self.release_media(target).await?;
        Ok(())
    }
    pub async fn shutdown(&self) {
        let j = {
            let mut s = self.state.lock();
            s.stopped = true;
            s.current.clone()
        };
        if let Some(j) = j {
            if !j.info.lock().can_cancel {
                // Firmware continues an accepted format after management USB
                // closes. Do not cancel it or issue media cleanup concurrently.
                return;
            }
            j.cancel.cancel();
            if tokio::time::timeout(Duration::from_secs(10), j.wait())
                .await
                .is_err()
            {
                self.state.lock().media_error = Some(AppError::new(
                    "MEDIA_CLEANUP_FAILED",
                    "저장소 작업 종료를 확인하지 못해 매체 정리를 중단했습니다.",
                ));
                return;
            }
        }
        if self.state.lock().card_format.is_none() {
            let _ = self.release_media(None).await;
        }
    }
    pub fn stop(&self) {
        let mut s = self.state.lock();
        s.stopped = true;
        if let Some(j) = &s.current {
            if j.info.lock().can_cancel {
                j.cancel.cancel();
            }
        }
        drop(s);
        self.clear_uploads();
        self.folder_sources.lock().clear();
    }
}
fn check_owner(p: &Principal) -> Result<(), AppError> {
    if p.cancelled.is_cancelled() || p.ended.is_cancelled() {
        Err(AppError::new(
            "UNAUTHORIZED",
            "저장소 제어 연결이 종료됐습니다.",
        ))
    } else {
        Ok(())
    }
}
fn authorize(a: &Principal, b: &Principal) -> Result<(), AppError> {
    if a.key_id != b.key_id {
        Err(AppError::new(
            "FORBIDDEN",
            "다른 클라이언트의 저장소 작업입니다.",
        ))
    } else {
        Ok(())
    }
}
struct Work {
    manager: Arc<Manager>,
    job: Arc<Job>,
    wire: Arc<dyn Wire>,
    mcu: Option<(u32, u8)>,
    user_transfer: Option<u32>,
    starting_job: bool,
    starting_user: bool,
    claimed_disk: bool,
    claimed_optical: bool,
    upload: Option<u32>,
    host_cleanup: Option<AppError>,
}
enum Operation {
    Action(Action),
    Import(import::Spec, std::fs::File, u64),
}
impl Operation {
    fn label(&self) -> &'static str {
        match self {
            Self::Action(a) => a.label(),
            Self::Import(spec, ..) => spec.label(),
        }
    }
    fn has_mcu_job(&self) -> bool {
        match self {
            Self::Action(a) => a.has_mcu_job(),
            Self::Import(..) => true,
        }
    }
}
impl Work {
    fn check(&self) -> Result<(), AppError> {
        if self.job.cancel.is_cancelled() {
            return Err(AppError::new(
                "OPERATION_CANCELLED",
                "저장소 작업을 취소했습니다.",
            ));
        }
        check_owner(&self.job.owner)?;
        self.manager.active.validate(&self.job.binding)?;
        if !self.wire.live() {
            return Err(AppError::new(
                "DEVICE_DISCONNECTED",
                "저장소 MCU 연결이 종료됐습니다.",
            ));
        }
        Ok(())
    }
    fn phase(&self, text: &str) {
        self.job.info.lock().phase = text.into();
    }
    async fn command(&self, ch: u8, op: u16, payload: Vec<u8>) -> Result<Vec<u8>, AppError> {
        self.check()?;
        self.wire
            .send(
                self.job.binding.clone(),
                self.job.owner.clone(),
                self.job.cancel.clone(),
                ch,
                op,
                payload,
            )
            .await
    }
    async fn optional(&self, op: u16) -> Result<Option<Vec<u8>>, AppError> {
        match self.command(3, op, vec![]).await {
            Ok(v) => Ok(Some(v)),
            Err(e) if [Some(3), Some(17)].contains(&e.status) => Ok(None),
            Err(e) => Err(e),
        }
    }
    async fn mcu_job(&mut self, op: u16, kind: u8, payload: Vec<u8>) -> Result<McuJob, AppError> {
        self.check()?;
        self.starting_job = true;
        let mutating = [1, 2, 3, 4, 6, 7, 8, 9, 10, 12, 13, 16, 17].contains(&kind);
        let prior_effects = self.job.info.lock().side_effects_possible;
        if mutating {
            self.job.info.lock().side_effects_possible = true;
        }
        let reply = match self.command(3, op, payload).await {
            Ok(v) => v,
            Err(e) => {
                if !e.uncertain {
                    self.starting_job = false;
                    if mutating {
                        self.job.info.lock().side_effects_possible = prior_effects;
                    }
                }
                return Err(e);
            }
        };
        if reply.len() != 4 || abi::u32at(&reply, 0) == 0 {
            return Err(abi::invalid());
        }
        let id = abi::u32at(&reply, 0);
        self.mcu = Some((id, kind));
        if kind == 1 {
            self.upload = Some(id);
        }
        self.starting_job = false;
        let deadline = Instant::now()
            + Duration::from_secs(if [1, 2, 7, 8].contains(&kind) {
                3600
            } else {
                30
            });
        loop {
            self.check()?;
            let reply = self.command(0, 7, abi::words(&[id])).await?;
            let status = McuJob::decode(&reply, id, kind)?;
            {
                let mut info = self.job.info.lock();
                if info.transfer.is_none() {
                    info.done = status.done;
                    info.total = status.total;
                }
                info.mcu_job = Some(status.clone());
            }
            if status.state != 0 {
                self.mcu = None;
                if status.state != 1 || status.result != 0 {
                    let mut error = AppError::new(
                        "MCU_JOB_FAILED",
                        format!(
                            "MCU 저장소 작업 실패: 상태 {}, 오류 {}, 상세 {}",
                            status.state, status.result, status.detail
                        ),
                    );
                    error.status = (status.result != 0).then_some(status.result);
                    return Err(error);
                }
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(AppError::new(
                    "JOB_TIMEOUT",
                    "MCU 저장소 작업 완료를 기다리는 시간이 초과됐습니다.",
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    async fn cleanup(&mut self) -> Result<(), AppError> {
        if !self.wire.live() {
            return Ok(());
        }
        if let Some((id, kind)) = self.mcu {
            self.wire.cleanup(Cleanup::Job(id), 8).await?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let b = self.wire.cleanup(Cleanup::Job(id), 7).await?;
                let status = McuJob::decode(&b, id, kind)?;
                self.job.info.lock().mcu_job = Some(status.clone());
                if status.state != 0 {
                    self.mcu = None;
                    break;
                }
                if Instant::now() > deadline {
                    return Err(AppError::new(
                        "CLEANUP_FAILED",
                        "MCU 저장소 작업의 취소 완료를 확인하지 못했습니다.",
                    ));
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        if let Some(id) = self.user_transfer.take() {
            self.wire.cleanup(Cleanup::User(id), 0x30e).await?;
        }
        self.cleanup_upload().await?;
        if self.claimed_disk || self.claimed_optical {
            self.manager
                .release_media_parts(self.claimed_disk, self.claimed_optical, None)
                .await?;
        }
        if self.starting_job || self.starting_user {
            return Err(AppError::new(
                "CLEANUP_FAILED",
                "MCU가 수락한 작업/전송 ID를 확인하지 못했습니다. 관리 USB를 재연결하세요.",
            ));
        }
        Ok(())
    }
    async fn card(&self) -> Result<Card, AppError> {
        let card = Card::decode(
            &self.command(3, 0x308, vec![]).await?,
            &self.job.binding.uid,
        )?;
        self.check()?;
        self.cache_card(&card);
        Ok(card)
    }
    fn cache_card(&self, card: &Card) {
        let b = &self.job.binding;
        self.manager.observe_media_card(&b.uid, card);
        let mut state = self.manager.state.lock();
        if !state.cache.contains_key(&b.uid) && state.cache.len() >= 64 {
            if let Some(oldest) = state
                .cache
                .values()
                .min_by_key(|s| s.observed_at)
                .map(|s| s.uid.clone())
            {
                state.cache.remove(&oldest);
            }
        }
        let snapshot = state
            .cache
            .entry(b.uid.clone())
            .or_insert_with(|| Snapshot {
                uid: b.uid.clone(),
                generation: b.generation,
                observed_at: now_ms(),
                stale: true,
                card: None,
                optical: None,
                disk: None,
                capabilities: 0,
                optical_enabled: false,
                optical_applied: false,
                images: vec![],
            });
        if snapshot
            .card
            .as_ref()
            .is_some_and(|old| old.uuid != card.uuid || old.present != card.present)
        {
            snapshot.images.clear();
            snapshot.optical = None;
            snapshot.disk = None;
            snapshot.stale = true;
        }
        snapshot.stale |= snapshot.generation != b.generation;
        snapshot.generation = b.generation;
        snapshot.observed_at = now_ms();
        snapshot.card = Some(card.clone());
    }
    async fn user_read(&self, card: &Card) -> Result<Vec<u8>, AppError> {
        if !card.user_available {
            return Err(AppError::new(
                "CARD_NOT_READY",
                "카드 사용자 값을 읽을 수 없습니다.",
            ));
        }
        let mut data = Vec::with_capacity(usize::from(card.user_length));
        while data.len() < usize::from(card.user_length) {
            let count = (usize::from(card.user_length) - data.len()).min(256);
            let mut request = abi::words(&[card.revision]);
            request.extend_from_slice(&(data.len() as u16).to_le_bytes());
            request.extend_from_slice(&(count as u16).to_le_bytes());
            let part = self.command(3, 0x30a, request).await?;
            if part.len() != count {
                return Err(abi::invalid());
            }
            data.extend(part);
        }
        let after = self.card().await?;
        if after.uuid != card.uuid
            || after.revision != card.revision
            || after.user_length != card.user_length
        {
            return Err(AppError::new(
                "CARD_CHANGED",
                "사용자 값 읽기 중 카드나 revision이 변경됐습니다.",
            ));
        }
        Ok(data)
    }
    async fn refresh(&mut self) -> Result<Snapshot, AppError> {
        self.observe_card_format().await?;
        self.phase("SD 상태와 이미지 목록 읽기");
        let optical = self
            .optional(0x305)
            .await?
            .map(|v| abi::Optical::decode(&v))
            .transpose()?
            .flatten();
        let disk = self
            .optional(0x318)
            .await?
            .map(|v| abi::Disk::decode(&v))
            .transpose()?
            .flatten();
        let card = self
            .optional(0x308)
            .await?
            .map(|v| Card::decode(&v, &self.job.binding.uid))
            .transpose()?;
        let caps = match self.command(3, 0x319, vec![]).await {
            Ok(v) => abi::capabilities(&v)?,
            Err(e) if e.status == Some(1) => (0, false, false),
            Err(e) => return Err(e),
        };
        let mut images = Vec::new();
        if let Some(image) = &optical {
            images.push(Image {
                id: image.id,
                kind: "optical".into(),
                name: image.name.clone(),
                size: image.size,
                format: 0,
                mounted: image.mounted,
                path: if image.mounted { 1 } else { 0 },
                writable: false,
            });
        }
        if caps.0 & 2 != 0 && card.as_ref().is_some_and(|c| c.present) {
            let mut cursor = 0;
            let mut seen = HashSet::new();
            let mut pages = 0;
            loop {
                pages += 1;
                if pages > 1024 {
                    return Err(abi::invalid());
                }
                let job = self.mcu_job(0x31b, 15, abi::words(&[cursor])).await?;
                let page = self.command(3, 0x31c, abi::words(&[job.id])).await?;
                if page.len() < 8
                    || page[4] > 4
                    || page.len() != 8 + usize::from(page[4]) * 12
                    || page[5..8] != [0, 0, 0]
                {
                    return Err(abi::invalid());
                }
                let next = abi::u32at(&page, 0);
                if next != 0 && next <= cursor {
                    return Err(abi::invalid());
                }
                for item in page[8..].chunks_exact(12) {
                    let id = abi::u32at(item, 0);
                    let sectors = abi::u32at(item, 4);
                    let format = item[8];
                    if id == 0
                        || id > 0x0fffffff
                        || sectors == 0
                        || !(1..=3).contains(&format)
                        || item[9..12] != [0, 0, 0]
                        || !seen.insert(id)
                        || images.len() >= 1024
                    {
                        return Err(abi::invalid());
                    }
                    let opened = disk.as_ref().filter(|d| d.id == id);
                    images.push(Image {
                        id,
                        kind: "disk".into(),
                        name: format!("D{id:07X}.IMG"),
                        size: u64::from(sectors) * 512,
                        format,
                        mounted: opened.is_some(),
                        path: opened
                            .map(|d| match d.state {
                                4 => 1,
                                5 => 2,
                                _ => 0,
                            })
                            .unwrap_or(0),
                        writable: opened.is_some_and(|d| d.writable),
                    });
                }
                if next == 0 {
                    break;
                }
                cursor = next;
            }
        }
        self.check()?;
        if let Some(before) = &card {
            let after = self.card().await?;
            if before.uuid != after.uuid
                || before.revision != after.revision
                || before.present != after.present
            {
                return Err(AppError::new(
                    "CARD_CHANGED",
                    "목록 조회 중 카드나 사용자 revision이 변경됐습니다.",
                ));
            }
        }
        let snapshot = Snapshot {
            uid: self.job.binding.uid.clone(),
            generation: self.job.binding.generation,
            observed_at: now_ms(),
            stale: false,
            card,
            optical,
            disk,
            capabilities: caps.0,
            optical_enabled: caps.1,
            optical_applied: caps.2,
            images,
        };
        let mut state = self.manager.state.lock();
        if state.cache.len() >= 64 && !state.cache.contains_key(&snapshot.uid) {
            if let Some(oldest) = state
                .cache
                .values()
                .min_by_key(|v| v.observed_at)
                .map(|v| v.uid.clone())
            {
                state.cache.remove(&oldest);
            }
        }
        state.cache.insert(snapshot.uid.clone(), snapshot.clone());
        Ok(snapshot)
    }
    async fn run(&mut self, action: Action) -> Result<Value, AppError> {
        if let Action::FolderShare {
            source_id,
            expected_card_uuid,
            ..
        } = action
        {
            return self.share_folder(source_id, expected_card_uuid).await;
        }
        if matches!(
            action,
            Action::DiskCreate { .. } | Action::DiskFormat { .. }
        ) {
            return self.format_image(action).await;
        }
        if matches!(
            action,
            Action::OpticalSet { .. }
                | Action::OpticalMount { .. }
                | Action::DiskOpen { .. }
                | Action::DiskClose { .. }
                | Action::DiskSync { .. }
                | Action::DiskRead { .. }
                | Action::DiskWrite { .. }
        ) {
            return self.media_action(action).await;
        }
        match action {
            Action::FolderForget { source_id } => {
                self.check()?;
                self.manager.forget_folder(&self.job.binding, &source_id)?;
                Ok(json!({"forgotten":true}))
            }
            Action::CardFormat {
                format,
                expected_card_uuid,
                ..
            } => self.format_card(format, expected_card_uuid).await,
            Action::CardFormatStatus {} => self.card_format_status().await,
            Action::Refresh {} => Ok(json!(self.refresh().await?)),
            Action::CardInfo {} => {
                self.observe_card_format().await?;
                Ok(json!(self.card().await?))
            }
            Action::NameGet {} => {
                self.phase("채널 이름 읽기");
                Ok(json!({"name":abi::name_result(&self.command(3,0x31e,vec![]).await?)?}))
            }
            Action::NameSet { name } => {
                self.phase("채널 이름 저장");
                let card = self.card().await?;
                card.writable_metadata()?;
                let mut payload = (name.len() as u16).to_le_bytes().to_vec();
                payload.extend_from_slice(name.as_bytes());
                self.mcu_job(0x31f, 17, payload).await?;
                let read = abi::name_result(&self.command(3, 0x31e, vec![]).await?)?;
                if read != name {
                    return Err(AppError::new(
                        "VERIFY_FAILED",
                        "저장 뒤 읽은 채널 이름이 다릅니다.",
                    ));
                }
                let after = self.card().await?;
                if after.uuid != card.uuid {
                    return Err(AppError::new(
                        "CARD_CHANGED",
                        "이름 저장 중 SD 카드가 변경됐습니다.",
                    ));
                }
                self.check()?;
                self.wire.verified_name(&read)?;
                Ok(json!({"name":read,"verified":true,"card":after}))
            }
            Action::UserRead {} => {
                self.phase("사용자 값 읽기");
                let card = self.card().await?;
                let bytes = self.user_read(&card).await?;
                Ok(json!({"bytes":bytes,"revision":card.revision,"uuid":card.uuid,"card":card}))
            }
            Action::UserWrite { bytes } => {
                self.phase("사용자 값 저장");
                let card = self.card().await?;
                card.writable_metadata()?;
                self.check()?;
                self.starting_user = true;
                let begin = match self
                    .command(3, 0x30b, (bytes.len() as u16).to_le_bytes().to_vec())
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        if !e.uncertain {
                            self.starting_user = false;
                        }
                        return Err(e);
                    }
                };
                if begin.len() != 4 || abi::u32at(&begin, 0) == 0 {
                    return Err(abi::invalid());
                }
                let transfer = abi::u32at(&begin, 0);
                self.user_transfer = Some(transfer);
                self.starting_user = false;
                let mut at = 0;
                for chunk in bytes.chunks(1008) {
                    let mut payload = abi::words(&[transfer]);
                    payload.extend_from_slice(&(at as u16).to_le_bytes());
                    payload.extend_from_slice(chunk);
                    let ack = self.command(3, 0x30c, payload).await?;
                    at += chunk.len();
                    if ack.len() != 2 || usize::from(abi::u16at(&ack, 0)) != at {
                        return Err(abi::invalid());
                    }
                }
                self.mcu_job(0x30d, 6, abi::words(&[transfer])).await?;
                self.user_transfer = None;
                let after = self.card().await?;
                if after.uuid != card.uuid {
                    return Err(AppError::new(
                        "CARD_CHANGED",
                        "사용자 값 저장 중 카드가 변경됐습니다.",
                    ));
                }
                let read = self.user_read(&after).await?;
                if read != bytes {
                    return Err(AppError::new(
                        "VERIFY_FAILED",
                        "저장 뒤 읽은 사용자 값이 다릅니다.",
                    ));
                }
                Ok(
                    json!({"bytes_written":bytes.len(),"revision":after.revision,"verified":true,"card":after}),
                )
            }
            _ => unreachable!("media actions dispatched above"),
        }
    }
}
