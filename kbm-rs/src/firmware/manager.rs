//! One owner-scoped firmware job. Original UID/epoch and programmer remain pinned
//! even when programming intentionally disconnects the management USB.
use super::{
    backend::{Backend, Control, Inspection, Notify, Programmed},
    image, Target,
};
use crate::{
    active::Binding,
    input_sessions::Principal,
    model::{now_ms, AppError, Uid},
    service::AppService,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::{watch, Semaphore};
use tokio_util::sync::CancellationToken;
#[cfg(test)]
pub(crate) mod tests;

#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Info {
    pub job_id: String,
    pub uid: Option<Uid>,
    pub generation: Option<u64>,
    pub target: Target,
    pub programmer_id: String,
    pub state: State,
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub can_cancel: bool,
    pub firmware_bytes: u64,
    pub sha256: String,
    pub result: Option<Programmed>,
    pub error: Option<AppError>,
    pub created_at: u64,
    pub finished_at: Option<u64>,
}
impl Info {
    pub fn terminal(&self) -> bool {
        matches!(
            self.state,
            State::Completed | State::Failed | State::Cancelled
        )
    }
}
struct Job {
    binding: Option<Binding>,
    owner: Principal,
    cancel: CancellationToken,
    info: Mutex<Info>,
    finished: watch::Sender<bool>,
}
#[derive(Default)]
struct Shared {
    stopped: bool,
    current: Option<Arc<Job>>,
    jobs: VecDeque<Arc<Job>>,
}
pub struct Manager {
    pub sources: super::sources::Pool,
    pub(crate) backend: Arc<dyn Backend>,
    shared: Mutex<Shared>,
    slot: Arc<Semaphore>,
}
#[derive(Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub target: Target,
    pub programmer_id: String,
    pub image_id: Option<String>,
    pub source_id: Option<String>,
    pub confirmed: bool,
}

pub fn check_owner(owner: &Principal) -> Result<(), AppError> {
    if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
        Err(AppError::new(
            "UNAUTHORIZED",
            "펌웨어 작업 연결이 종료됐습니다.",
        ))
    } else {
        Ok(())
    }
}
fn authorize(job: &Job, owner: &Principal) -> Result<(), AppError> {
    check_owner(owner)?;
    if job.owner.key_id != owner.key_id {
        return Err(AppError::new(
            "FORBIDDEN",
            "다른 API 키의 펌웨어 작업입니다.",
        ));
    }
    Ok(())
}
impl Manager {
    pub fn new(backend: Arc<dyn Backend>) -> Arc<Self> {
        Arc::new(Self {
            sources: Default::default(),
            backend,
            shared: Default::default(),
            slot: Arc::new(Semaphore::new(1)),
        })
    }
    pub fn current(&self) -> Option<Info> {
        self.shared
            .lock()
            .current
            .as_ref()
            .map(|j| j.info.lock().clone())
    }
    pub fn busy(&self) -> bool {
        self.shared.lock().current.is_some()
    }
    fn available(&self) -> Result<(), AppError> {
        let s = self.shared.lock();
        if s.stopped {
            return Err(AppError::new("APP_STOPPING", "앱 종료 중입니다."));
        }
        if s.current.is_some() {
            return Err(AppError::new(
                "FIRMWARE_BUSY",
                "현재 펌웨어 작업이 끝날 때까지 기다려주세요.",
            ));
        }
        Ok(())
    }
    pub async fn list(
        &self,
        target: Target,
        owner: &Principal,
    ) -> Result<Vec<super::backend::Programmer>, AppError> {
        check_owner(owner)?;
        self.available()?;
        let _permit = self
            .slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::new("FIRMWARE_BUSY", "프로그래머 확인 중입니다."))?;
        let list = self
            .backend
            .list(target, owner.cancelled.child_token())
            .await?;
        check_owner(owner)?;
        Ok(list)
    }
    pub async fn inspect(
        &self,
        app: &AppService,
        binding: Option<&Binding>,
        owner: &Principal,
        target: Target,
        id: String,
    ) -> Result<Inspection, AppError> {
        check_owner(owner)?;
        self.available()?;
        if let Some(binding) = binding {
            app.active.validate(binding)?;
        }
        let _permit = self
            .slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::new("FIRMWARE_BUSY", "프로그래머 확인 중입니다."))?;
        let inspected = self
            .backend
            .inspect(target, id, owner.cancelled.child_token())
            .await?;
        check_owner(owner)?;
        if let Some(binding) = binding {
            app.active.validate(binding)?;
        }
        Ok(inspected)
    }
    pub fn start(
        self: &Arc<Self>,
        app: Arc<AppService>,
        binding: impl Into<Option<Binding>>,
        owner: Principal,
        target: Target,
        programmer_id: String,
        image: image::Prepared,
    ) -> Result<Info, AppError> {
        let binding = binding.into();
        check_owner(&owner)?;
        if let Some(binding) = &binding {
            app.active.validate(binding)?;
        }
        self.available()?;
        let permit = self
            .slot
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::new("FIRMWARE_BUSY", "프로그래머 확인/기록 중입니다."))?;
        let mut id = [0u8; 16];
        getrandom::fill(&mut id)
            .map_err(|_| AppError::new("RANDOM_FAILED", "펌웨어 작업 ID를 만들지 못했습니다."))?;
        let info = Info {
            job_id: id.iter().map(|b| format!("{b:02x}")).collect(),
            uid: binding.as_ref().map(|b| b.uid.clone()),
            generation: binding.as_ref().map(|b| b.generation),
            target,
            programmer_id: programmer_id.clone(),
            state: State::Queued,
            phase: "기록 전 확인".into(),
            done: 0,
            total: 0,
            can_cancel: true,
            firmware_bytes: image.bytes.len() as u64,
            sha256: image.sha256.clone(),
            result: None,
            error: None,
            created_at: now_ms(),
            finished_at: None,
        };
        let job = Arc::new(Job {
            binding: binding.clone(),
            cancel: owner.cancelled.child_token(),
            owner,
            info: Mutex::new(info.clone()),
            finished: watch::channel(false).0,
        });
        {
            let mut state = self.shared.lock();
            if state.stopped || state.current.is_some() {
                return Err(AppError::new(
                    "FIRMWARE_BUSY",
                    "다른 펌웨어 작업이 진행 중입니다.",
                ));
            }
            if let Some(binding) = &binding {
                app.active.validate(binding)?;
            }
            state.current = Some(job.clone());
            while state.jobs.len() >= 32 {
                state.jobs.pop_back();
            }
            state.jobs.push_front(job.clone());
        }
        let this = self.clone();
        tokio::spawn(async move {
            use futures_util::FutureExt;
            let work = std::panic::AssertUnwindSafe(async {
                job.info.lock().state = State::Running;
                check_owner(&job.owner)?;
                if let Some(binding) = &job.binding {
                    app.active.validate(binding)?;
                }
                let inspection = this
                    .backend
                    .inspect(target, programmer_id, job.cancel.clone())
                    .await?;
                check_owner(&job.owner)?;
                if let Some(binding) = &job.binding {
                    app.active.validate(binding)?;
                }
                if inspection.target == Target::Stm32 {
                    job.info.lock().uid = inspection.uid.clone();
                }
                super::ms2130::require_capacity(image.bytes.len() as u64, inspection.capacity)?;
                if job.cancel.is_cancelled() {
                    return Err(AppError::new(
                        "OPERATION_CANCELLED",
                        "펌웨어 작업을 취소했습니다.",
                    ));
                }
                app.prepare_firmware(job.binding.as_ref()).await?;
                let commit_job = job.clone();
                let commit_app = app.clone();
                let weak = Arc::downgrade(&this);
                let control = Control {
                    cancel: job.cancel.clone(),
                    commit: Arc::new(move || {
                        if weak.upgrade().is_none_or(|m| m.shared.lock().stopped) {
                            return Err(AppError::new(
                                "APP_STOPPING",
                                "기록 전에 앱이 종료됐습니다.",
                            ));
                        }
                        check_owner(&commit_job.owner)?;
                        if commit_job.cancel.is_cancelled() {
                            return Err(AppError::new(
                                "OPERATION_CANCELLED",
                                "기록 전에 취소했습니다.",
                            ));
                        }
                        if commit_app.active.state().uid.is_some() {
                            return Err(AppError::new(
                                "STALE_OPERATION",
                                "플래싱 중 다른 채널이 활성화됐습니다.",
                            ));
                        }
                        let mut info = commit_job.info.lock();
                        if commit_job.cancel.is_cancelled() {
                            return Err(AppError::new(
                                "OPERATION_CANCELLED",
                                "기록 전에 취소했습니다.",
                            ));
                        }
                        info.can_cancel = false;
                        info.phase = "펌웨어 기록·검증".into();
                        Ok(())
                    }),
                };
                let progress_job = job.clone();
                let notify: Notify = Arc::new(move |p| {
                    let mut i = progress_job.info.lock();
                    i.phase = p.phase.into();
                    i.done = p.done;
                    i.total = p.total;
                });
                let result = this
                    .backend
                    .program(inspection, image, control, notify)
                    .await?;
                if !result.verified
                    || result.target != target
                    || result.bytes != job.info.lock().firmware_bytes
                {
                    return Err(AppError::uncertain(
                        "FLASH_VERIFY_FAILED",
                        "펌웨어 기록 결과가 요청과 다릅니다.",
                    ));
                }
                Ok::<_, AppError>(result)
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                Err(AppError::uncertain(
                    "FIRMWARE_FAILED",
                    "펌웨어 작업 완료를 확인하지 못했습니다.",
                ))
            });
            {
                let mut i = job.info.lock();
                let was_cancellable = i.can_cancel;
                i.can_cancel = false;
                i.finished_at = Some(now_ms());
                match work {
                    Ok(result) => {
                        i.state = State::Completed;
                        i.phase = "완료".into();
                        i.done = i.firmware_bytes;
                        i.total = i.firmware_bytes;
                        i.result = Some(result);
                    }
                    Err(error) => {
                        i.state = if was_cancellable
                            && (job.cancel.is_cancelled()
                                || matches!(error.code, "OPERATION_CANCELLED" | "UNAUTHORIZED"))
                        {
                            State::Cancelled
                        } else {
                            State::Failed
                        };
                        i.error = Some(if let Some(binding) = &job.binding {
                            error.at(binding)
                        } else {
                            error
                        });
                    }
                }
            }
            let completed = job.info.lock().clone();
            app.logs.emit(
                crate::logging::Level::Info,
                crate::logging::Event::FirmwareFinished,
                crate::logging::Fields {
                    uid: completed.uid.clone(),
                    generation: completed.generation,
                    success: Some(completed.result.is_some()),
                    code: completed.error.as_ref().map(|e| e.code),
                    count: Some(completed.firmware_bytes),
                    ..Default::default()
                },
            );
            {
                let mut state = this.shared.lock();
                if state.current.as_ref().is_some_and(|j| Arc::ptr_eq(j, &job)) {
                    state.current = None;
                }
            }
            job.finished.send_replace(true);
            drop(permit);
        });
        Ok(info)
    }
    pub fn job(&self, owner: &Principal, id: &str) -> Result<Info, AppError> {
        let state = self.shared.lock();
        let job = state
            .jobs
            .iter()
            .find(|j| j.info.lock().job_id == id)
            .ok_or_else(|| AppError::new("JOB_NOT_FOUND", "펌웨어 작업을 찾을 수 없습니다."))?;
        authorize(job, owner)?;
        let info = job.info.lock().clone();
        Ok(info)
    }
    pub fn cancel(&self, owner: &Principal, id: &str) -> Result<Info, AppError> {
        let state = self.shared.lock();
        let job = state
            .jobs
            .iter()
            .find(|j| j.info.lock().job_id == id)
            .ok_or_else(|| AppError::new("JOB_NOT_FOUND", "펌웨어 작업을 찾을 수 없습니다."))?;
        authorize(job, owner)?;
        let info = job.info.lock();
        if info.can_cancel && !info.terminal() {
            job.cancel.cancel();
        }
        Ok(info.clone())
    }
    pub fn stop(&self) {
        let mut s = self.shared.lock();
        s.stopped = true;
        if let Some(j) = &s.current {
            if j.info.lock().can_cancel {
                j.cancel.cancel();
            }
        }
    }
    pub async fn shutdown(&self) {
        self.stop();
        let job = self.shared.lock().current.clone();
        if let Some(j) = job {
            let mut done = j.finished.subscribe();
            while !*done.borrow() {
                if done.changed().await.is_err() {
                    break;
                }
            }
        }
    }
}
