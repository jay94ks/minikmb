use super::{image::Prepared, Target};
use crate::model::{AppError, Uid};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Programmer {
    pub id: String,
    pub label: String,
    pub target: Target,
    pub selectable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Inspection {
    pub target: Target,
    pub programmer_id: String,
    pub uid: Option<Uid>,
    pub capacity: u64,
}
#[derive(Clone, Debug)]
pub struct Progress {
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
}
pub type Notify = Arc<dyn Fn(Progress) + Send + Sync>;
pub struct Control {
    pub cancel: CancellationToken,
    pub commit: Arc<dyn Fn() -> Result<(), AppError> + Send + Sync>,
}
impl Control {
    pub fn check(&self) -> Result<(), AppError> {
        if self.cancel.is_cancelled() {
            Err(AppError::new(
                "OPERATION_CANCELLED",
                "펌웨어 작업을 취소했습니다.",
            ))
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Programmed {
    pub target: Target,
    pub programmer_id: String,
    pub bytes: u64,
    pub verified_bytes: u64,
    pub verified: bool,
    pub reset: bool,
    pub tail_preserved: bool,
}
pub trait Backend: Send + Sync {
    fn list(
        &self,
        target: Target,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<Vec<Programmer>, AppError>>;
    fn inspect(
        &self,
        target: Target,
        programmer: String,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<Inspection, AppError>>;
    fn program(
        &self,
        expected: Inspection,
        image: Prepared,
        control: Control,
        notify: Notify,
    ) -> BoxFuture<'static, Result<Programmed, AppError>>;
}
