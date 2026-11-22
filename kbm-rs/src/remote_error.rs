//! Remote error envelope and request correlation. Never reflects request bodies,
//! auth headers, query strings, caller-provided request IDs or parser diagnostics.
use crate::{
    active::Binding,
    model::{AppError, Uid},
};
use axum::{
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};

static SEQUENCE: AtomicU64 = AtomicU64::new(1);
static PREFIX: OnceLock<String> = OnceLock::new();
#[derive(Clone)]
pub struct Context {
    pub request_id: String,
    observed: Arc<parking_lot::Mutex<Option<Binding>>>,
    revoked: Arc<parking_lot::Mutex<Option<tokio_util::sync::CancellationToken>>>,
}
#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct Failure {
    #[serde(flatten)]
    pub error: AppError,
    pub request_id: String,
    pub channel_uid: Option<Uid>,
    pub generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
}
impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
impl Context {
    pub fn new() -> Self {
        // Correlation IDs are not credentials. A process-start stamp/PID and a
        // non-wrapping counter remain usable even if OS random generation fails.
        let prefix = PREFIX.get_or_init(|| {
            format!(
                "{:x}-{:x}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )
        });
        let seq = SEQUENCE
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("request ID exhausted");
        Self {
            request_id: format!("mkr-{prefix}-{seq:016x}"),
            observed: Default::default(),
            revoked: Default::default(),
        }
    }
    pub fn observe(&self, binding: Binding) {
        *self.observed.lock() = Some(binding);
    }
    pub fn authorize(&self, revoked: tokio_util::sync::CancellationToken) {
        *self.revoked.lock() = Some(revoked);
    }
    pub fn failure(&self, error: AppError) -> Failure {
        let authorized = error.code != "UNAUTHORIZED";
        let source = authorized.then_some(error.origin.as_ref()).flatten();
        let observed = self.observed.lock();
        let binding = source
            .map(|s| &s.binding)
            .or_else(|| authorized.then_some(observed.as_ref()).flatten());
        Failure {
            request_id: self.request_id.clone(),
            channel_uid: binding.map(|b| b.uid.clone()),
            generation: binding.map(|b| b.generation),
            job_id: source.and_then(|s| s.job_id.clone()),
            error,
        }
    }
    pub fn finish(&self, mut response: Response, rest: bool) -> Response {
        let revoked = self.revoked.lock().clone();
        if revoked.as_ref().is_some_and(|t| t.is_cancelled()) {
            response = AppError::uncertain(
                "UNAUTHORIZED",
                "처리 중 API 키가 폐기됐습니다. 이미 수락한 동작의 결과를 확인하세요.",
            )
            .into_response();
        }
        let status = response.status();
        if status.is_client_error() || status.is_server_error() {
            let allow = response.headers().get(axum::http::header::ALLOW).cloned();
            let error = response.extensions_mut().remove::<AppError>();
            // Keep SDK JSON-RPC transport errors intact. Known app/auth/session
            // failures and every REST rejection use the same typed envelope.
            if error.is_some()
                || rest
                || matches!(
                    status,
                    StatusCode::UNAUTHORIZED
                        | StatusCode::FORBIDDEN
                        | StatusCode::NOT_FOUND
                        | StatusCode::TOO_MANY_REQUESTS
                        | StatusCode::SERVICE_UNAVAILABLE
                )
            {
                let error = error.unwrap_or_else(|| generic(status));
                response = (status, Json(self.failure(error))).into_response();
                if let Some(allow) = allow {
                    response
                        .headers_mut()
                        .insert(axum::http::header::ALLOW, allow);
                }
            }
        }
        response.headers_mut().insert(
            "x-minikmb-request-id",
            HeaderValue::from_str(&self.request_id).expect("generated ASCII ID"),
        );
        response
            .headers_mut()
            .insert("cache-control", HeaderValue::from_static("no-store"));
        if response.status() == StatusCode::UNAUTHORIZED {
            response.headers_mut().insert(
                axum::http::header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer realm=\"MiniKMB\""),
            );
        }
        if response.status() != StatusCode::UNAUTHORIZED {
            if let Some(token) = revoked {
                use futures_util::StreamExt;
                let (parts, body) = response.into_parts();
                return Response::from_parts(
                    parts,
                    axum::body::Body::from_stream(
                        body.into_data_stream().take_until(token.cancelled_owned()),
                    ),
                );
            }
        }
        response
    }
}
fn generic(status: StatusCode) -> AppError {
    let (code, message) = match status {
        StatusCode::UNAUTHORIZED => ("UNAUTHORIZED", "유효한 API 키가 필요합니다."),
        StatusCode::FORBIDDEN => ("FORBIDDEN", "이 요청을 허용하지 않습니다."),
        StatusCode::NOT_FOUND => ("NOT_FOUND", "요청한 경로나 세션을 찾을 수 없습니다."),
        StatusCode::METHOD_NOT_ALLOWED => {
            ("METHOD_NOT_ALLOWED", "이 HTTP 메서드는 지원하지 않습니다.")
        }
        StatusCode::PAYLOAD_TOO_LARGE => {
            ("PAYLOAD_TOO_LARGE", "요청 본문이 허용 크기를 넘었습니다.")
        }
        StatusCode::UNSUPPORTED_MEDIA_TYPE => {
            ("INVALID_CONTENT_TYPE", "요청 Content-Type을 확인하세요.")
        }
        StatusCode::TOO_MANY_REQUESTS => ("QUEUE_FULL", "허용한 대기 요청 수를 넘었습니다."),
        StatusCode::SERVICE_UNAVAILABLE => {
            ("SERVER_OFF", "서버가 종료 중이거나 사용할 수 없습니다.")
        }
        s if s.is_server_error() => ("INTERNAL_ERROR", "요청을 처리하지 못했습니다."),
        _ => (
            "INVALID_ARGUMENTS",
            "요청 JSON·본문·쿼리 형식을 확인하세요.",
        ),
    };
    AppError::new(code, message)
}
pub fn status(error: &AppError) -> StatusCode {
    match error.code {
        "UNAUTHORIZED" => StatusCode::UNAUTHORIZED,
        "FORBIDDEN" | "HOST_NOT_ALLOWED" | "ORIGIN_NOT_ALLOWED" => StatusCode::FORBIDDEN,
        "CHANNEL_NOT_FOUND"
        | "FIRMWARE_NOT_FOUND"
        | "FIRMWARE_SOURCE_NOT_FOUND"
        | "JOB_NOT_FOUND"
        | "UPLOAD_NOT_FOUND"
        | "INPUT_SESSION_NOT_FOUND"
        | "RESOURCE_NOT_FOUND" => StatusCode::NOT_FOUND,
        "NOT_IMPLEMENTED" | "UNSUPPORTED" | "HID_UNSUPPORTED" | "UNSUPPORTED_PLATFORM" => {
            StatusCode::NOT_IMPLEMENTED
        }
        "SERVER_OFF" | "APP_STOPPING" => StatusCode::SERVICE_UNAVAILABLE,
        "CAPTURE_BUSY"
        | "QUEUE_FULL"
        | "EVENT_SUBSCRIPTION_LIMIT"
        | "INPUT_SESSION_LIMIT"
        | "INPUT_STATE_LIMIT"
        | "INPUT_QUEUE_FULL" => StatusCode::TOO_MANY_REQUESTS,
        "NO_ACTIVE_CHANNEL"
        | "FIRMWARE_BUSY"
        | "FIRMWARE_UID_MISMATCH"
        | "PROGRAMMER_TARGET_CHANGED"
        | "CHANNEL_SWITCHING"
        | "STALE_OPERATION"
        | "INPUT_BUSY"
        | "INPUT_CANCELLED"
        | "INPUT_SESSION_EXPIRED"
        | "STALE_FRAME_REFERENCE"
        | "FRAME_GEOMETRY_CHANGED"
        | "STORAGE_BUSY"
        | "STORAGE_UNCERTAIN"
        | "STALE_SESSION"
        | "MEDIA_CONFLICT"
        | "MEDIA_CLEANUP_FAILED"
        | "CARD_CHANGED"
        | "CARD_NOT_READY"
        | "CARD_UID_MISMATCH"
        | "CARD_FORMAT_PENDING"
        | "JOB_NOT_CANCELLABLE"
        | "HID_RELEASE_UNCONFIRMED"
        | "HID_SEQUENCE_FAILED"
        | "NETWORK_BUSY"
        | "TARGET_NOT_READY"
        | "DEVICE_DISCONNECTED" => StatusCode::CONFLICT,
        _ => StatusCode::BAD_REQUEST,
    }
}

#[cfg(test)]
mod tests;
