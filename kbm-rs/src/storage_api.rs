use crate::{
    active::Binding, input_sessions::Principal, model::AppError, service::AppService,
    storage::Action,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobArgs {
    pub job_id: String,
}
pub fn is_command(name: &str) -> bool {
    name.starts_with("storage_")
}
pub fn schema(name: &str) -> Option<Value> {
    Some(match name {
        "storage_action" => serde_json::to_value(schemars::schema_for!(Action)).unwrap(),
        "storage_upload_prepare" => {
            serde_json::to_value(schemars::schema_for!(crate::storage::import::Spec)).unwrap()
        }
        "storage_job_get" | "storage_job_cancel" => {
            serde_json::to_value(schemars::schema_for!(JobArgs)).unwrap()
        }
        "storage_get_status" => serde_json::to_value(schemars::schema_for!(Empty)).unwrap(),
        _ => return None,
    })
}
fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, AppError> {
    serde_json::from_value(v).map_err(|_| {
        AppError::new(
            "INVALID_ARGUMENTS",
            "저장소 인자를 확인하세요. 일반 작업에는 UID를 지정하지 않습니다.",
        )
    })
}
impl AppService {
    /// binding is an internal GUI pin, never a public routing argument.
    pub fn execute_storage(
        self: &Arc<Self>,
        binding: Option<Binding>,
        owner: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        let binding = if name.starts_with("storage_job_") {
            binding
        } else {
            binding.or_else(|| self.active.bind().ok())
        };
        let original = if name.starts_with("storage_job_") {
            None
        } else {
            binding.clone().or_else(|| self.active.bind().ok())
        };
        let result = self.execute_storage_inner(binding, owner, name, args);
        let source = result.as_ref().ok().and_then(|v| {
            Some(Binding {
                uid: crate::model::Uid::try_from(v["uid"].as_str()?.to_owned()).ok()?,
                generation: v["generation"].as_u64()?,
            })
        });
        self.audit(name, source.as_ref().or(original.as_ref()), result)
    }
    fn execute_storage_inner(
        self: &Arc<Self>,
        binding: Option<Binding>,
        owner: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
            return Err(AppError::new(
                "UNAUTHORIZED",
                "저장소 제어 연결이 종료됐습니다.",
            ));
        }
        match name {
            "storage_upload_prepare" => {
                let spec: crate::storage::import::Spec = parse(args)?;
                let b = binding.map(Ok).unwrap_or_else(|| self.active.bind())?;
                let address = self
                    .remote
                    .read()
                    .rest
                    .ok_or_else(|| AppError::new("SERVER_OFF", "REST 서버를 먼저 켜세요."))?;
                self.storage
                    .prepare_upload(b.clone(), owner.clone(), spec, address)
                    .map_err(|e| e.at(&b))
            }
            "storage_job_get" => {
                let args: JobArgs = parse(args)?;
                Ok(json!(self.storage.job(owner, &args.job_id)?))
            }
            "storage_job_cancel" => {
                let args: JobArgs = parse(args)?;
                Ok(json!(self.storage.cancel(owner, &args.job_id)?))
            }
            "storage_action" => {
                let action: Action = parse(args)?;
                let b = binding.map(Ok).unwrap_or_else(|| self.active.bind())?;
                self.active.validate(&b)?;
                Ok(json!(self
                    .storage
                    .start(b.clone(), owner.clone(), action)
                    .map_err(|e| e.at(&b))?))
            }
            "storage_get_status" => {
                let _: Empty = parse(args)?;
                let b = binding.map(Ok).unwrap_or_else(|| self.active.bind())?;
                self.active.validate(&b)?;
                let value = json!({"uid":b.uid,"generation":b.generation,"snapshot":self.storage.cached(&b),"job":self.storage.current(&b),"resources":self.storage.resources_info(),"cleanup_error":self.storage.media_error(),"card_format":self.storage.card_format_info(),"folder_sources":self.storage.folder_sources(&b)});
                self.active.validate(&b)?;
                Ok(value)
            }
            _ => Err(AppError::new(
                "NOT_IMPLEMENTED",
                "저장소 명령을 찾을 수 없습니다.",
            )),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_actions_reject_routing_overrides() {
        for action in [
            "refresh",
            "card_info",
            "card_format_status",
            "name_get",
            "user_read",
        ] {
            assert!(serde_json::from_value::<Action>(json!({"action":action})).is_ok());
            for field in ["uid", "uids", "generation"] {
                let mut args = json!({"action":action});
                args[field] = json!([]);
                assert!(
                    serde_json::from_value::<Action>(args).is_err(),
                    "{action}/{field}"
                );
            }
        }
        assert!(serde_json::from_value::<Action>(
            json!({"action":"name_set","name":"a","uid":"b"})
        )
        .is_err());
        assert!(serde_json::from_value::<Action>(
            json!({"action":"user_write","bytes":[],"uid":"b"})
        )
        .is_err());
        for value in [
            json!({"action":"folder_share","source_id":"01".repeat(32),"confirmed":true}),
            json!({"action":"folder_forget","source_id":"01".repeat(32)}),
            json!({"action":"card_format","format":2,"confirmed":true}),
            json!({"action":"optical_set","enabled":false}),
            json!({"action":"optical_mount","id":1,"mounted":true}),
            json!({"action":"disk_open","id":2,"path":"direct","writable":true}),
            json!({"action":"disk_close","id":2}),
            json!({"action":"disk_sync","id":2}),
            json!({"action":"disk_read","id":2,"lba":0}),
            json!({"action":"disk_write","id":2,"lba":0,"bytes":vec![0;512],"confirmed":true}),
        ] {
            assert!(serde_json::from_value::<Action>(value.clone()).is_ok());
            for field in ["uid", "uids", "generation"] {
                let mut invalid = value.clone();
                invalid[field] = json!([]);
                assert!(serde_json::from_value::<Action>(invalid).is_err());
            }
        }
    }
}
