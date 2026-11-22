use crate::{
    active::Binding,
    firmware::{
        manager::{check_owner, Start},
        sources, Target,
    },
    input_sessions::Principal,
    model::AppError,
    service::AppService,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct List {
    pub target: Target,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    pub target: Target,
    pub programmer_id: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub job_id: String,
}
pub fn is_command(name: &str) -> bool {
    name.starts_with("firmware_")
}
pub fn schema(name: &str) -> Option<Value> {
    Some(match name {
        "firmware_images" | "firmware_get_status" => {
            serde_json::to_value(schemars::schema_for!(Empty)).unwrap()
        }
        "firmware_programmers" => serde_json::to_value(schemars::schema_for!(List)).unwrap(),
        "firmware_inspect" => serde_json::to_value(schemars::schema_for!(Inspect)).unwrap(),
        "firmware_upload_prepare" => {
            serde_json::to_value(schemars::schema_for!(sources::Spec)).unwrap()
        }
        "firmware_start" => serde_json::to_value(schemars::schema_for!(Start)).unwrap(),
        "firmware_job_get" | "firmware_job_cancel" => {
            serde_json::to_value(schemars::schema_for!(Job)).unwrap()
        }
        _ => return None,
    })
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, AppError> {
    serde_json::from_value(value).map_err(|_| {
        AppError::new(
            "INVALID_ARGUMENTS",
            "펌웨어 인자를 확인하세요. 일반 작업에 UID/로컬 경로를 지정하지 않습니다.",
        )
    })
}
impl AppService {
    pub async fn execute_firmware(
        self: &Arc<Self>,
        binding: Option<Binding>,
        owner: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        let original = binding.or_else(|| self.active.bind().ok());
        let result = async {
            check_owner(owner)?;
            match name {
                "firmware_images" => {
                    let _: Empty = parse(args)?;
                    Ok(json!({"images":sources::images()}))
                }
                "firmware_get_status" => {
                    let _: Empty = parse(args)?;
                    Ok(json!({"job":self.firmware.current()}))
                }
                "firmware_programmers" => {
                    let a: List = parse(args)?;
                    Ok(json!({"programmers":self.firmware.list(a.target, owner).await?}))
                }
                "firmware_inspect" => {
                    let a: Inspect = parse(args)?;
                    let b = None::<&Binding>;
                    Ok(json!(
                        self.firmware
                            .inspect(self, b, owner, a.target, a.programmer_id)
                            .await?
                    ))
                }
                "firmware_upload_prepare" => {
                    let a: sources::Spec = parse(args)?;
                    let b = None::<&Binding>;
                    let address =
                        self.remote.read().rest.ok_or_else(|| {
                            AppError::new("SERVER_OFF", "REST 서버를 먼저 켜세요.")
                        })?;
                    Ok(json!(self.firmware.sources.upload(
                        b.cloned(),
                        owner.clone(),
                        a,
                        address
                    )?))
                }
                "firmware_start" => {
                    let a: Start = parse(args)?;
                    if !a.confirmed || a.image_id.is_some() == a.source_id.is_some() {
                        return Err(AppError::new(
                            "INVALID_ARGUMENTS",
                            "펌웨어 하나와 confirmed=true가 필요합니다.",
                        ));
                    }
                    let b = None::<&Binding>;
                    let image = if let Some(id) = a.image_id {
                        let target = a.target;
                        tokio::task::spawn_blocking(move || sources::bundled(&id, target))
                            .await
                            .map_err(|_| {
                                AppError::new(
                                    "FIRMWARE_IO_FAILED",
                                    "동봉 펌웨어를 준비하지 못했습니다.",
                                )
                            })??
                    } else {
                        self.firmware
                            .sources
                            .take(b, owner, &a.source_id.unwrap(), a.target)?
                    };
                    Ok(json!(self.firmware.start(
                        self.clone(),
                        b.cloned(),
                        owner.clone(),
                        a.target,
                        a.programmer_id,
                        image
                    )?))
                }
                "firmware_job_get" => {
                    let a: Job = parse(args)?;
                    Ok(json!(self.firmware.job(owner, &a.job_id)?))
                }
                "firmware_job_cancel" => {
                    let a: Job = parse(args)?;
                    Ok(json!(self.firmware.cancel(owner, &a.job_id)?))
                }
                _ => Err(AppError::new(
                    "NOT_IMPLEMENTED",
                    "펌웨어 명령을 찾을 수 없습니다.",
                )),
            }
        }
        .await;
        self.audit(name, original.as_ref(), result)
    }
}
