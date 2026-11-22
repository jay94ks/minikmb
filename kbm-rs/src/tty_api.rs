//! The same strict TTY command types are used by GUI, REST and MCP.
use crate::{active::Binding, input_sessions::Principal, model::AppError, service::AppService};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadArgs {
    pub stream_id: Option<String>,
    pub cursor: Option<u64>,
    #[serde(default = "read_limit")]
    #[schemars(range(min = 1, max = 4096))]
    pub max_bytes: usize,
}
fn read_limit() -> usize {
    4096
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteArgs {
    #[schemars(length(min = 1, max = 65536))]
    pub bytes: Vec<u8>,
}
pub fn is_command(name: &str) -> bool {
    name.starts_with("tty_")
}
pub fn schema(name: &str) -> Option<Value> {
    Some(match name {
        "tty_open" | "tty_close" | "tty_get_status" => {
            serde_json::to_value(schemars::schema_for!(Empty)).unwrap()
        }
        "tty_read" => serde_json::to_value(schemars::schema_for!(ReadArgs)).unwrap(),
        "tty_write" => serde_json::to_value(schemars::schema_for!(WriteArgs)).unwrap(),
        _ => return None,
    })
}
fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, AppError> {
    serde_json::from_value(v).map_err(|_| {
        AppError::new(
            "INVALID_ARGUMENTS",
            "시리얼 인자를 확인하세요. UID는 지정하지 않습니다.",
        )
    })
}
impl AppService {
    pub async fn execute_tty(
        self: &Arc<Self>,
        b: Binding,
        principal: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        let original = b.clone();
        let result = self.execute_tty_inner(b, principal, name, args).await;
        self.audit(name, Some(&original), result)
    }
    async fn execute_tty_inner(
        self: &Arc<Self>,
        b: Binding,
        principal: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        if principal.cancelled.is_cancelled() || principal.ended.is_cancelled() {
            return Err(AppError::new(
                "UNAUTHORIZED",
                "인증 또는 연결이 종료됐습니다.",
            ));
        }
        self.active.validate(&b)?;
        match name {
            "tty_open" => {
                let _: Empty = parse(args)?;
                Ok(json!(self.tty.open(b, principal.clone()).await?))
            }
            "tty_close" => {
                let _: Empty = parse(args)?;
                self.tty.close(&b)?;
                Ok(json!({"receiving":false,"uid":b.uid,"generation":b.generation}))
            }
            "tty_get_status" => {
                let _: Empty = parse(args)?;
                Ok(json!(self.tty.status(b, principal.clone()).await?))
            }
            "tty_read" => {
                let a: ReadArgs = parse(args)?;
                Ok(json!(self.tty.read(
                    &b,
                    a.stream_id.as_deref(),
                    a.cursor,
                    a.max_bytes
                )?))
            }
            "tty_write" => {
                let a: WriteArgs = parse(args)?;
                Ok(json!(self.tty.write(b, principal.clone(), a.bytes).await?))
            }
            _ => Err(AppError::new(
                "NOT_IMPLEMENTED",
                "시리얼 명령을 찾을 수 없습니다.",
            )),
        }
    }
}
