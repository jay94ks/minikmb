//! GUI/REST/MCP share the same UID-free selected-channel NCM commands.
use crate::{
    active::Binding,
    input_sessions::Principal,
    model::AppError,
    network::{Config, Mode},
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
pub struct ModeArgs {
    pub mode: Mode,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForwardArgs {
    pub rules: Vec<crate::network::forwarding::Rule>,
}
pub fn is_command(name: &str) -> bool {
    name.starts_with("network_")
}
pub fn schema(name: &str) -> Option<Value> {
    Some(match name {
        "network_get_status" => serde_json::to_value(schemars::schema_for!(Empty)).unwrap(),
        "network_configure" => serde_json::to_value(schemars::schema_for!(Config)).unwrap(),
        "network_set_mode" => serde_json::to_value(schemars::schema_for!(ModeArgs)).unwrap(),
        "network_scan_start" => {
            serde_json::to_value(schemars::schema_for!(crate::network::scanner::Request)).unwrap()
        }
        "network_scan_stop" => serde_json::to_value(schemars::schema_for!(Empty)).unwrap(),
        "network_forward_configure" | "network_forward_start" => {
            serde_json::to_value(schemars::schema_for!(ForwardArgs)).unwrap()
        }
        "network_forward_stop" => serde_json::to_value(schemars::schema_for!(Empty)).unwrap(),
        _ => return None,
    })
}
fn parse<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, AppError> {
    serde_json::from_value(args).map_err(|_| {
        AppError::new(
            "INVALID_ARGUMENTS",
            "네트워크 인자를 확인하세요. UID·포트 경로는 지정하지 않습니다.",
        )
    })
}
impl AppService {
    pub async fn execute_network(
        self: &Arc<Self>,
        binding: Binding,
        owner: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        let original = binding.clone();
        let result = self.execute_network_inner(binding, owner, name, args).await;
        self.audit(name, Some(&original), result)
    }
    async fn execute_network_inner(
        self: &Arc<Self>,
        binding: Binding,
        owner: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
            return Err(AppError::new(
                "UNAUTHORIZED",
                "네트워크 제어 연결이 종료됐습니다.",
            ));
        }
        self.active.validate(&binding)?;
        match name {
            "network_forward_configure" => {
                let args: ForwardArgs = parse(args)?;
                let network = self.network.clone();
                let owner = owner.clone();
                Ok(json!(tokio::task::spawn_blocking(
                    move || network.configure_forward(&binding, &owner, args.rules)
                )
                .await
                .map_err(|_| AppError::new(
                    "PREFERENCES_IO",
                    "설정 저장 작업이 종료됐습니다."
                ))??))
            }
            "network_forward_start" => {
                let args: ForwardArgs = parse(args)?;
                Ok(json!(
                    self.network
                        .forward(binding, owner.clone(), Some(args.rules))
                        .await?
                ))
            }
            "network_forward_stop" => {
                let _: Empty = parse(args)?;
                Ok(json!(
                    self.network.forward(binding, owner.clone(), None).await?
                ))
            }
            "network_scan_start" => {
                let request = parse(args)?;
                Ok(json!(
                    self.network.scan(binding, owner, Some(request)).await?
                ))
            }
            "network_scan_stop" => {
                let _: Empty = parse(args)?;
                Ok(json!(self.network.scan(binding, owner, None).await?))
            }
            "network_get_status" => {
                let _: Empty = parse(args)?;
                let status = json!(self.network.status(&binding));
                self.active.validate(&binding)?;
                Ok(status)
            }
            "network_configure" => {
                let config: Config = parse(args)?;
                let network = self.network.clone();
                let user = owner.clone();
                let target = binding.clone();
                let value =
                    tokio::task::spawn_blocking(move || network.configure(&target, &user, config))
                        .await
                        .map_err(|_| {
                            AppError::new("PREFERENCES_IO", "설정 저장 작업이 종료됐습니다.")
                        })??;
                self.active.validate(&binding)?;
                Ok(json!(value))
            }
            "network_set_mode" => {
                let args: ModeArgs = parse(args)?;
                self.logs.emit(
                    crate::logging::Level::Info,
                    crate::logging::Event::NetworkChanged,
                    crate::logging::Fields {
                        uid: Some(binding.uid.clone()),
                        generation: Some(binding.generation),
                        code: Some(match args.mode {
                            Mode::Off => "NETWORK_OFF_REQUESTED",
                            Mode::Emulation => "NETWORK_EMULATION_REQUESTED",
                            Mode::Host => "NETWORK_HOST_REQUESTED",
                        }),
                        ..Default::default()
                    },
                );
                Ok(json!(
                    self.network
                        .set_mode(binding, owner.clone(), args.mode)
                        .await?
                ))
            }
            _ => Err(AppError::new(
                "NOT_IMPLEMENTED",
                "네트워크 명령을 찾을 수 없습니다.",
            )),
        }
    }
}
#[cfg(test)]
mod tests;
