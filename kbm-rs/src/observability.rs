//! App-global log controls and active-channel cached diagnostics.
use crate::{input_sessions::Principal, model::AppError, service::AppService};
use serde_json::{json, Value};
use std::sync::Arc;
#[cfg(test)]
mod tests;
pub fn is_command(name: &str) -> bool {
    name.starts_with("logs_") || name == "diagnostics_get"
}
pub fn schema(name: &str) -> Option<Value> {
    if let Some(schema) = crate::diagnostics::schema(name) {
        return Some(schema);
    }
    Some(match name {
        "logs_configure" => {
            serde_json::to_value(schemars::schema_for!(crate::logging::Config)).unwrap()
        }
        "logs_get_state" | "logs_flush" | "logs_open_folder" => {
            serde_json::to_value(schemars::schema_for!(crate::diagnostics::Empty)).unwrap()
        }
        _ => return None,
    })
}
impl AppService {
    pub async fn execute_observability(
        self: &Arc<Self>,
        owner: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
            return Err(AppError::new(
                "UNAUTHORIZED",
                "인증 또는 연결이 종료됐습니다.",
            ));
        }
        if name == "diagnostics_get" {
            return self.execute_diagnostics(owner, args).await;
        }
        let config = if name == "logs_configure" {
            Some(
                serde_json::from_value::<crate::logging::Config>(args).map_err(|_| {
                    AppError::new("INVALID_ARGUMENTS", "enabled/verbose 불리언을 지정하세요.")
                })?,
            )
        } else {
            let _: crate::diagnostics::Empty = serde_json::from_value(args).map_err(|_| {
                AppError::new(
                    "INVALID_ARGUMENTS",
                    "로그 조회는 UID나 추가 인자를 받지 않습니다.",
                )
            })?;
            None
        };
        match name {
            "logs_get_state" => Ok(json!(self.logs.status())),
            "logs_configure" | "logs_flush" | "logs_open_folder" => {
                let slot =
                    self.log_controls.clone().try_acquire_owned().map_err(|_| {
                        AppError::new("QUEUE_FULL", "로그 제어 작업이 진행 중입니다.")
                    })?;
                let app = self.clone();
                let owner = owner.clone();
                let open_folder = name == "logs_open_folder";
                tokio::task::spawn_blocking(move || {
                    let _slot = slot;
                    if !app.is_running()
                        || owner.cancelled.is_cancelled()
                        || owner.ended.is_cancelled()
                    {
                        return Err(AppError::new(
                            "OPERATION_CANCELLED",
                            "로그 제어 연결이 종료됐습니다.",
                        ));
                    }
                    if let Some(config) = config {
                        app.logs.configure(config);
                    }
                    if open_folder {
                        let path = app.logs.directory().ok_or_else(|| {
                            AppError::new("LOG_DIRECTORY", "로그 폴더가 지정되지 않았습니다.")
                        })?;
                        app.log_opener.open(path, &|| {
                            if app.is_running()
                                && !owner.cancelled.is_cancelled()
                                && !owner.ended.is_cancelled()
                            {
                                Ok(())
                            } else {
                                Err(AppError::new(
                                    "OPERATION_CANCELLED",
                                    "폴더 열기 연결이 종료됐습니다.",
                                ))
                            }
                        })?;
                        return Ok(json!({"requested":true,"confirmed":false}));
                    }
                    app.logs.flush()?;
                    Ok(json!(app.logs.status()))
                })
                .await
                .map_err(|_| AppError::new("LOG_IO", "로그 제어 작업이 종료됐습니다."))?
            }
            _ => Err(AppError::new(
                "NOT_IMPLEMENTED",
                "로그 명령을 찾을 수 없습니다.",
            )),
        }
    }
}
