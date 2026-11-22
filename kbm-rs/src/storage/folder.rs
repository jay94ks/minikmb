//! App-approved directory capabilities. Remote clients never supply host paths.
use super::*;
#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct SourceInfo {
    pub source_id: String,
    pub name: String,
    pub expires_at: u64,
}
#[derive(Clone)]
pub(super) struct Source {
    info: SourceInfo,
    root: folder_image::Root,
    binding: Binding,
    session: u64,
    wire: Arc<dyn Wire>,
    deadline: Instant,
}
fn missing() -> AppError {
    AppError::new("FOLDER_SOURCE_EXPIRED", "앱에서 이 채널에 공유할 폴더를 다시 선택하세요. 폴더 식별자가 만료되었거나 원래 연결이 변경됐습니다.")
}
impl Manager {
    /// Only the native GUI chooser/tests call this; no REST/MCP route accepts a Root/path.
    pub fn approve_folder(
        &self,
        binding: Binding,
        root: folder_image::Root,
        session: u64,
    ) -> Result<SourceInfo, AppError> {
        if self.import_session(&binding)? != session {
            return Err(missing());
        }
        let wire = self.backend.connect(&binding)?;
        if !wire.live() || wire.session() != session {
            return Err(missing());
        }
        if self.state.lock().stopped {
            return Err(missing());
        }
        let mut random = [0; 32];
        getrandom::fill(&mut random)
            .map_err(|_| AppError::new("RANDOM_FAILED", "폴더 식별자를 만들지 못했습니다."))?;
        let id: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let mut sources = self.folder_sources.lock();
        sources.retain(|_, s| {
            s.deadline > Instant::now() && s.binding == binding && s.session == session
        });
        if sources.len() >= 16 {
            return Err(AppError::new(
                "QUEUE_FULL",
                "선택한 폴더가 너무 많습니다. 사용하지 않는 폴더를 해제하세요.",
            ));
        }
        let info = SourceInfo {
            source_id: id.clone(),
            name: root.display_name(),
            expires_at: now_ms() + 3600000,
        };
        sources.insert(
            id,
            Source {
                info: info.clone(),
                root,
                binding,
                session,
                wire,
                deadline: Instant::now() + Duration::from_secs(3600),
            },
        );
        Ok(info)
    }
    pub fn folder_sources(&self, binding: &Binding) -> Vec<SourceInfo> {
        let mut sources = self.folder_sources.lock();
        sources.retain(|_, s| s.deadline > Instant::now() && s.wire.live());
        let mut result: Vec<_> = sources
            .values()
            .filter(|s| s.binding == *binding)
            .map(|s| s.info.clone())
            .collect();
        result.sort_by(|a, b| a.name.cmp(&b.name).then(a.source_id.cmp(&b.source_id)));
        result
    }
    pub fn forget_folder(&self, binding: &Binding, source_id: &str) -> Result<(), AppError> {
        self.active.validate(binding)?;
        let mut sources = self.folder_sources.lock();
        if sources.get(source_id).is_none_or(|s| s.binding != *binding) {
            return Err(missing());
        }
        sources.remove(source_id);
        Ok(())
    }
}
impl Work {
    pub(super) async fn share_folder(
        &mut self,
        source_id: String,
        expected: Option<String>,
    ) -> Result<Value, AppError> {
        self.check()?;
        let source = self
            .manager
            .folder_sources
            .lock()
            .get(&source_id)
            .cloned()
            .ok_or_else(missing)?;
        if source.binding != self.job.binding
            || source.session != self.wire.session()
            || source.deadline <= Instant::now()
        {
            return Err(missing());
        }
        let card = self.media_card(expected.as_deref()).await?;
        match self.command(3, 0x318, vec![]).await {
            Err(e) if e.status == Some(17) => {}
            Err(e) => return Err(e),
            Ok(_) => {
                return Err(AppError::new(
                    "MEDIA_CONFLICT",
                    "폴더 공유 전에 열린 디스크 경로를 닫으세요.",
                ))
            }
        }
        self.phase("폴더 읽기 전용 스냅샷 준비");
        let job = self.job.clone();
        let manager = self.manager.clone();
        let wire = self.wire.clone();
        let guard: format::Guard = Arc::new(move || {
            if job.cancel.is_cancelled() {
                return Err(AppError::new(
                    "OPERATION_CANCELLED",
                    "폴더 스냅샷을 취소했습니다.",
                ));
            }
            check_owner(&job.owner)?;
            manager.active.validate(&job.binding)?;
            if !wire.live() {
                return Err(missing());
            }
            Ok(())
        });
        let snapshot = tokio::task::spawn_blocking(move || folder_image::build(source.root, guard))
            .await
            .map_err(|_| {
                AppError::new(
                    "FOLDER_SNAPSHOT_FAILED",
                    "폴더 스냅샷을 준비하지 못했습니다.",
                )
            })??;
        self.check()?;
        let summary = snapshot.summary;
        let uploaded = self
            .import_image_prefix(
                import::Spec::Disk {
                    size: u64::from(summary.sectors) * 512,
                    format: 1,
                    replace_id: None,
                    confirmed: true,
                    expected_card_uuid: Some(card.uuid.clone()),
                },
                snapshot.file,
                Some(summary.populated_sectors),
            )
            .await?;
        let id = uploaded["image"]["id"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(abi::invalid)?;
        self.phase("타깃에 읽기 전용 폴더 이미지 연결");
        let mounted = self
            .media_action(Action::DiskOpen {
                id,
                path: media::Path::Target,
                writable: false,
                expected_card_uuid: Some(card.uuid),
            })
            .await?;
        Ok(
            json!({"image":uploaded["image"],"snapshot":mounted["snapshot"],"folder":summary,"source_id":source_id,"readonly":true}),
        )
    }
}
