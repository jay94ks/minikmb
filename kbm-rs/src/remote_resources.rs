//! Fixed, read-only MCP resources. URIs never contain a caller-selected UID/path.
use crate::{input_sessions::Principal, model::AppError, service::AppService};
use std::sync::Arc;
pub struct ResourceDoc {
    pub uri: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub mime: &'static str,
}
pub const RESOURCES: &[ResourceDoc] = &[
    ResourceDoc {
        uri: "minikmb://events/snapshot",
        name: "상태 이벤트",
        description: "변경 알림과 같은 캐시 메타데이터 전체입니다. 기능을 시작하지 않습니다.",
        mime: "application/json",
    },
    ResourceDoc {
        uri: "minikmb://app/state",
        name: "앱 상태",
        description: "UI와 같은 채널/활성/서버 상태. 기능을 시작하지 않습니다.",
        mime: "application/json",
    },
    ResourceDoc {
        uri: "minikmb://channels",
        name: "채널 목록",
        description: "허브 매핑과 MCU UID를 포함한 수동 조회 목록입니다.",
        mime: "application/json",
    },
    ResourceDoc {
        uri: "minikmb://active/channel",
        name: "현재 활성 채널",
        description: "현재 활성 채널 하나. 없으면 NO_ACTIVE_CHANNEL 오류입니다.",
        mime: "application/json",
    },
    ResourceDoc {
        uri: "minikmb://active/diagnostics",
        name: "현재 채널 진단",
        description: "캐시된 상태/신선도/카운터. 입력이나 파일 내용을 읽지 않습니다.",
        mime: "application/json",
    },
    ResourceDoc {
        uri: "minikmb://reference/openapi",
        name: "REST API 계약",
        description: "현재 REST 주소와 요청/응답/오류 스키마를 포함한 OpenAPI 문서입니다.",
        mime: "application/json",
    },
    ResourceDoc {
        uri: "minikmb://reference/guide",
        name: "MiniKMB 연동 안내",
        description: "단일 활성 채널·입력·파일 업로드/jobs·전체 도구 사용 안내입니다.",
        mime: "text/markdown",
    },
];
pub async fn read(app: &Arc<AppService>, owner: &Principal, uri: &str) -> Result<String, AppError> {
    if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
        return Err(AppError::new(
            "UNAUTHORIZED",
            "인증 또는 연결이 종료됐습니다.",
        ));
    }
    if !RESOURCES.iter().any(|r| r.uri == uri) {
        return Err(AppError::new(
            "RESOURCE_NOT_FOUND",
            "등록된 MiniKMB 리소스가 아닙니다.",
        ));
    }
    let text = match uri {
        "minikmb://reference/openapi" => {
            serde_json::to_string_pretty(&crate::remote::openapi_for(app.remote.read().rest))
                .unwrap()
        }
        "minikmb://reference/guide" => {
            let servers = app.remote.read();
            crate::api_docs::mcp_guide(servers.rest, servers.mcp)
        }
        _ => {
            let command = match uri {
                "minikmb://events/snapshot" => "events_get_snapshot",
                "minikmb://app/state" => "app_get_state",
                "minikmb://channels" => "channels_list",
                "minikmb://active/channel" => "channel_get_active",
                _ => "diagnostics_get",
            };
            serde_json::to_string_pretty(
                &app.execute_remote(owner, command, serde_json::json!({}))
                    .await?,
            )
            .unwrap()
        }
    };
    if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
        return Err(AppError::new(
            "UNAUTHORIZED",
            "인증 또는 연결이 종료됐습니다.",
        ));
    }
    Ok(text)
}
