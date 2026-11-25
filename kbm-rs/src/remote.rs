use crate::{
    model::AppError,
    service::{AppService, COMMANDS},
};
use axum::{
    extract::{OriginalUri, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, put},
    Extension, Json, Router,
};
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerConfig, Tool,
    },
    service::{RequestContext, RoleServer},
    ErrorData, ServerHandler,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};
use tokio_util::sync::CancellationToken;
#[cfg(test)]
mod event_tests;
mod events;
mod ipc;
pub(crate) use ipc::{ipc_router, upload_address, upload_response};

#[derive(Clone, Debug, Default, Serialize, schemars::JsonSchema)]
pub struct RemoteStatus {
    pub rest: Option<SocketAddr>,
    pub mcp: Option<SocketAddr>,
    pub error: String,
}
struct Running {
    token: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    address: SocketAddr,
}
#[derive(Default)]
struct Servers {
    rest: Option<Running>,
    mcp: Option<Running>,
}
pub struct RemoteServers {
    app: Arc<AppService>,
    servers: tokio::sync::Mutex<Servers>,
}
impl RemoteServers {
    pub fn new(app: Arc<AppService>) -> Arc<Self> {
        Arc::new(Self {
            app,
            servers: tokio::sync::Mutex::new(Servers::default()),
        })
    }
    fn publish(&self, s: &Servers, error: String) {
        self.app.logs.emit(
            if error.is_empty() {
                crate::logging::Level::Info
            } else {
                crate::logging::Level::Warn
            },
            crate::logging::Event::ServerChanged,
            crate::logging::Fields {
                count: Some(u64::from(s.rest.is_some()) + u64::from(s.mcp.is_some())),
                success: Some(error.is_empty()),
                ..Default::default()
            },
        );
        *self.app.remote.write() = RemoteStatus {
            rest: s.rest.as_ref().map(|r| r.address),
            mcp: s.mcp.as_ref().map(|r| r.address),
            error,
        };
    }
    async fn start(&self, mcp: bool, port: u16) -> Result<Running, AppError> {
        if self.app.keys.list().is_empty() {
            return Err(AppError::new("KEY_REQUIRED", "먼저 API 키를 만드세요."));
        }
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|_| AppError::new("BIND_FAILED", "서버 포트를 열 수 없습니다."))?;
        let address = listener
            .local_addr()
            .map_err(|_| AppError::new("BIND_FAILED", "서버 주소를 확인할 수 없습니다."))?;
        let token = CancellationToken::new();
        let router = if mcp {
            mcp_router(self.app.clone(), token.child_token())
        } else {
            rest_router(self.app.clone())
        };
        let router = router
            .layer(middleware::from_fn_with_state(
                Auth {
                    app: self.app.clone(),
                    address,
                },
                authenticate,
            ))
            .layer(Extension(ServerLifetime(token.clone())));
        let stop = token.clone();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await;
        });
        Ok(Running {
            token,
            task,
            address,
        })
    }
    async fn stop_one(r: Option<Running>) {
        if let Some(mut r) = r {
            r.token.cancel();
            if tokio::time::timeout(std::time::Duration::from_secs(2), &mut r.task)
                .await
                .is_err()
            {
                r.task.abort();
            }
        }
    }
    pub async fn rest(&self, on: bool, port: u16) -> Result<(), AppError> {
        let mut s = self.servers.lock().await;
        let result = if on {
            if s.rest.is_none() {
                self.start(false, port).await.map(|r| s.rest = Some(r))
            } else {
                Ok(())
            }
        } else {
            if let Some(r) = &s.rest {
                r.token.cancel();
            }
            if let Some(r) = &s.mcp {
                r.token.cancel();
            }
            self.app.close_remote_inputs().await;
            Self::stop_one(s.mcp.take()).await;
            Self::stop_one(s.rest.take()).await;
            Ok(())
        };
        self.publish(
            &s,
            result
                .as_ref()
                .err()
                .map(|e| e.message.clone())
                .unwrap_or_default(),
        );
        result
    }
    pub async fn mcp(&self, on: bool, rest_port: u16, mcp_port: u16) -> Result<(), AppError> {
        let mut s = self.servers.lock().await;
        if !on {
            if let Some(r) = &s.mcp {
                r.token.cancel();
            }
            for id in self.app.input_sessions.cancel_mcp() {
                let _ = self.app.input.release_owner(id).await;
            }
            Self::stop_one(s.mcp.take()).await;
            self.publish(&s, String::new());
            return Ok(());
        }
        let added = s.rest.is_none();
        if added {
            match self.start(false, rest_port).await {
                Ok(r) => s.rest = Some(r),
                Err(e) => {
                    self.publish(&s, e.message.clone());
                    return Err(e);
                }
            }
        }
        if s.mcp.is_none() {
            match self.start(true, mcp_port).await {
                Ok(r) => s.mcp = Some(r),
                Err(e) => {
                    if added {
                        Self::stop_one(s.rest.take()).await;
                    }
                    self.publish(&s, e.message.clone());
                    return Err(e);
                }
            }
        }
        self.publish(&s, String::new());
        Ok(())
    }
    pub async fn shutdown(&self) {
        let _ = self.rest(false, 0).await;
    }
}
impl Drop for RemoteServers {
    fn drop(&mut self) {
        if let Ok(mut s) = self.servers.try_lock() {
            for r in [s.mcp.take(), s.rest.take()].into_iter().flatten() {
                r.token.cancel();
                r.task.abort();
            }
        }
    }
}

#[derive(Clone)]
struct Auth {
    app: Arc<AppService>,
    address: SocketAddr,
}
#[derive(Clone)]
struct ServerLifetime(CancellationToken);
async fn authenticate(
    State(auth): State<Auth>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let context = crate::remote_error::Context::new();
    let rest = !request.uri().path().starts_with("/mcp");
    request.extensions_mut().insert(context.clone());
    let response = authenticate_inner(auth, request, next, &context).await;
    context.finish(response, rest)
}
async fn authenticate_inner(
    auth: Auth,
    mut request: Request<axum::body::Body>,
    next: Next,
    context: &crate::remote_error::Context,
) -> Response {
    let host = request.headers().get("host").and_then(|s| s.to_str().ok());
    let expected = auth.address.to_string();
    if host != Some(expected.as_str()) {
        return AppError::new("HOST_NOT_ALLOWED", "요청 Host를 허용하지 않습니다.").into_response();
    }
    if let Some(origin) = request.headers().get("origin") {
        if origin.to_str().ok() != Some(format!("http://{expected}").as_str()) {
            return AppError::new("ORIGIN_NOT_ALLOWED", "요청 Origin을 허용하지 않습니다.")
                .into_response();
        }
    }
    let token = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let Some(lease) = token.and_then(|t| auth.app.keys.authorize(t)) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let path = request.uri().path();
    let active = COMMANDS
        .iter()
        .any(|c| c.path == path && c.method == request.method().as_str() && c.active_required)
        || path.starts_with("/api/v1/active/storage/uploads/")
        || path.starts_with("/api/v1/firmware/uploads/");
    if active {
        if let Ok(b) = auth.app.active.bind() {
            context.observe(b);
        }
    }
    let revoked = lease.revoked.clone();
    context.authorize(revoked.clone());
    let mut principal = crate::input_sessions::Principal::rest(&lease);
    if let Some(life) = request.extensions().get::<ServerLifetime>() {
        principal.ended = life.0.clone();
    }
    if principal.ended.is_cancelled() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    request.extensions_mut().insert(principal);
    request.extensions_mut().insert(lease);
    let response = tokio::select! {
        biased;
        _=revoked.cancelled()=>return StatusCode::UNAUTHORIZED.into_response(),
        response=next.run(request)=>response
    };
    if revoked.is_cancelled() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    response
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = crate::remote_error::status(&self);
        let mut response = (status, Json(&self)).into_response();
        response.extensions_mut().insert(self);
        response
    }
}

fn mcp_router(app: Arc<AppService>, token: CancellationToken) -> Router {
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };
    let manager = Arc::new(LocalSessionManager::default());
    let guard = McpSessions {
        app: app.clone(),
        manager: manager.clone(),
        owners: Default::default(),
        slots: Arc::new(tokio::sync::Semaphore::new(64)),
        stop: token.clone(),
    };
    let service: StreamableHttpService<Mcp, LocalSessionManager> = StreamableHttpService::new(
        move || {
            Ok(Mcp {
                app: app.clone(),
                subscriptions: Default::default(),
            })
        },
        manager,
        StreamableHttpServerConfig::default()
            .with_json_response(true)
            .with_cancellation_token(token),
    );
    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(guard, mcp_session_guard))
}
#[derive(Clone)]
struct McpSessions {
    app: Arc<AppService>,
    manager: Arc<rmcp::transport::streamable_http_server::session::local::LocalSessionManager>,
    owners: Arc<parking_lot::Mutex<std::collections::HashMap<String, McpOwner>>>,
    slots: Arc<tokio::sync::Semaphore>,
    stop: CancellationToken,
}
struct McpOwner {
    key: String,
    end: CancellationToken,
    life: CancellationToken,
}
async fn mcp_session_guard(
    State(state): State<McpSessions>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    use rmcp::transport::streamable_http_server::session::SessionManager;
    let Some(lease) = request
        .extensions()
        .get::<crate::auth::AuthLease>()
        .cloned()
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let sid = request
        .headers()
        .get("mcp-session-id")
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let remove = request.method() == axum::http::Method::DELETE;
    let mut principal = crate::input_sessions::Principal::rest(&lease);
    principal.client_id = "mcp:stateless".into();
    principal.ended = state.stop.clone();
    let permit = if let Some(sid) = &sid {
        let owners = state.owners.lock();
        match owners.get(sid) {
            Some(owner) if owner.key == lease.info.id => {
                principal.client_id = format!("mcp:{sid}");
                principal.ended = owner.life.clone();
            }
            Some(_) => return StatusCode::FORBIDDEN.into_response(),
            None => return StatusCode::NOT_FOUND.into_response(),
        }
        None
    } else {
        match state.slots.clone().try_acquire_owned() {
            Ok(p) => Some(p),
            Err(_) => return StatusCode::TOO_MANY_REQUESTS.into_response(),
        }
    };
    request.extensions_mut().insert(principal);
    let response = next.run(request).await;
    if remove && response.status().is_success() {
        if let Some(owner) = sid.and_then(|s| state.owners.lock().remove(&s)) {
            owner.life.cancel();
            owner.end.cancel();
        }
    }
    if let Some(permit) = permit {
        if let Some(id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|h| h.to_str().ok())
            .map(str::to_owned)
        {
            let end = state.stop.child_token();
            let life = lease.revoked.child_token();
            state.owners.lock().insert(
                id.clone(),
                McpOwner {
                    key: lease.info.id.clone(),
                    end: end.clone(),
                    life: life.clone(),
                },
            );
            tokio::spawn(async move {
                let _permit = permit;
                tokio::select! {
                    _=lease.revoked.cancelled()=>{},
                    _=end.cancelled()=>{},
                    _=tokio::time::sleep(std::time::Duration::from_secs(3600))=>{}
                }
                life.cancel();
                state.owners.lock().remove(&id);
                state
                    .app
                    .close_client_inputs(&lease.info.id, &format!("mcp:{id}"))
                    .await;
                let session: Arc<str> = id.into();
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    state.manager.close_session(&session),
                )
                .await;
            });
        }
    }
    response
}
fn rest_router(app: Arc<AppService>) -> Router {
    let mut router = Router::new();
    for doc in COMMANDS.iter().filter(|c| {
        (crate::input_api::is_command(c.name)
            || crate::tty_api::is_command(c.name)
            || crate::storage_api::is_command(c.name)
            || crate::firmware_api::is_command(c.name)
            || crate::wake::is_command(c.name)
            || crate::observability::is_command(c.name)
            || crate::network_api::is_command(c.name))
            && c.method == "POST"
    }) {
        router = router.route(doc.path, axum::routing::post(rest_input_command));
    }
    router
        .route(
            "/api/v1/firmware/uploads/{upload_id}",
            axum::routing::post(rest_firmware_upload)
                .layer(axum::extract::DefaultBodyLimit::disable()),
        )
        .route(
            "/api/v1/active/storage/uploads/{upload_id}",
            axum::routing::post(rest_storage_upload)
                .layer(axum::extract::DefaultBodyLimit::disable()),
        )
        .route("/api/v1/active/hid", get(rest_input_status))
        .route("/api/v1/active/tty", get(rest_tty_status))
        .route("/api/v1/active/storage", get(rest_storage_status))
        .route("/api/v1/active/network", get(rest_network_status))
        .route("/api/v1/active/wake", get(rest_wake_status))
        .route("/api/v1/active/wake/adapters", get(rest_wake_status))
        .route("/api/v1/logs", get(rest_observability))
        .route("/api/v1/active/diagnostics", get(rest_observability))
        .route("/api/v1/state", get(rest_state))
        .route("/api/v1/events", get(events::stream))
        .route("/api/v1/events/snapshot", get(events::snapshot))
        .route("/api/v1/channels", get(rest_channels))
        .route("/api/v1/active", get(rest_active))
        .route(
            "/api/v1/active/video",
            get(rest_video_status)
                .post(rest_video_open)
                .delete(rest_video_close),
        )
        .route("/api/v1/active/video/snapshot", get(rest_video_snapshot))
        .route(
            "/api/v1/active-channel",
            put(rest_select).delete(rest_clear),
        )
        .route("/api/v1/openapi.json", get(rest_openapi))
        .with_state(app)
}
async fn rest_openapi(State(app): State<Arc<AppService>>) -> Json<Value> {
    Json(openapi_for(app.remote.read().rest))
}
async fn rest_firmware_upload(
    State(app): State<Arc<AppService>>,
    Extension(owner): Extension<crate::input_sessions::Principal>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> Result<Json<Value>, AppError> {
    if query.is_some_and(|q| !q.is_empty())
        || headers.contains_key(axum::http::header::TRANSFER_ENCODING)
        || headers
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.split(';').next())
            .is_none_or(|s| !s.trim().eq_ignore_ascii_case("application/octet-stream"))
    {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "query/chunked 없이 application/octet-stream으로 업로드하세요.",
        ));
    }
    let binding = None::<crate::active::Binding>;
    let expected = app
        .firmware
        .sources
        .expected(binding.as_ref(), &owner, &id)?;
    if headers
        .get_all(axum::http::header::CONTENT_LENGTH)
        .iter()
        .count()
        != 1
        || headers
            .get(axum::http::header::CONTENT_LENGTH)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            != Some(expected)
    {
        return Err(AppError::new(
            "FIRMWARE_SIZE_MISMATCH",
            "Content-Length가 준비한 크기와 다릅니다.",
        ));
    }
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        axum::body::to_bytes(body, expected as usize),
    )
    .await
    .map_err(|_| AppError::new("UPLOAD_TIMEOUT", "펌웨어 업로드 시간이 초과됐습니다."))?
    .map_err(|_| AppError::new("FIRMWARE_SIZE_MISMATCH", "업로드 본문 크기가 다릅니다."))?;
    Ok(Json(json!(app.firmware.sources.receive(
        binding.as_ref(),
        &owner,
        &id,
        &bytes
    )?)))
}
async fn rest_storage_upload(
    State(app): State<Arc<AppService>>,
    Extension(owner): Extension<crate::input_sessions::Principal>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> Result<Json<Value>, AppError> {
    if query.is_some_and(|q| !q.is_empty())
        || headers.contains_key(axum::http::header::TRANSFER_ENCODING)
    {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "업로드에는 query나 chunked 전송을 사용하지 않습니다.",
        ));
    }
    if !headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("application/octet-stream")
        })
    {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "application/octet-stream 파일 본문이 필요합니다.",
        ));
    }
    let lengths: Vec<_> = headers
        .get_all(axum::http::header::CONTENT_LENGTH)
        .iter()
        .collect();
    let length = if lengths.len() == 1 {
        lengths[0].to_str().ok().and_then(|v| v.parse().ok())
    } else {
        None
    };
    Ok(Json(json!(
        app.storage.receive_upload(owner, &id, length, body).await?
    )))
}
async fn rest_input_command(
    State(app): State<Arc<AppService>>,
    Extension(principal): Extension<crate::input_sessions::Principal>,
    OriginalUri(uri): OriginalUri,
    Json(args): Json<Value>,
) -> Result<Json<Value>, AppError> {
    if uri.query().is_some_and(|q| !q.is_empty()) {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "이 명령에는 query 인자를 지정하지 않습니다.",
        ));
    }
    let command = COMMANDS
        .iter()
        .find(|c| c.path == uri.path() && c.method == "POST")
        .ok_or_else(|| AppError::new("NOT_FOUND", "명령을 찾을 수 없습니다."))?;
    app.execute_remote(&principal, command.name, args)
        .await
        .map(Json)
}
async fn rest_input_status(
    State(app): State<Arc<AppService>>,
    Extension(principal): Extension<crate::input_sessions::Principal>,
) -> Result<Json<Value>, AppError> {
    app.execute_remote(&principal, "hid_get_status", json!({}))
        .await
        .map(Json)
}
async fn rest_wake_status(
    State(app): State<Arc<AppService>>,
    Extension(principal): Extension<crate::input_sessions::Principal>,
    OriginalUri(uri): OriginalUri,
) -> Result<Json<Value>, AppError> {
    if uri.query().is_some_and(|query| !query.is_empty()) {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "WOL 조회는 UID나 query 인자를 받지 않습니다.",
        ));
    }
    let command = COMMANDS
        .iter()
        .find(|doc| doc.method == "GET" && doc.path == uri.path())
        .ok_or_else(|| AppError::new("NOT_FOUND", "명령을 찾을 수 없습니다."))?;
    app.execute_remote(&principal, command.name, json!({}))
        .await
        .map(Json)
}
async fn rest_observability(
    State(app): State<Arc<AppService>>,
    Extension(principal): Extension<crate::input_sessions::Principal>,
    OriginalUri(uri): OriginalUri,
) -> Result<Json<Value>, AppError> {
    if uri.query().is_some_and(|query| !query.is_empty()) {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "로그·진단 조회에는 추가 query 인자를 받지 않습니다.",
        ));
    }
    let doc = COMMANDS
        .iter()
        .find(|doc| doc.method == "GET" && doc.path == uri.path())
        .ok_or_else(|| AppError::new("NOT_FOUND", "명령을 찾을 수 없습니다."))?;
    app.execute_observability(&principal, doc.name, json!({}))
        .await
        .map(Json)
}
async fn rest_tty_status(
    State(app): State<Arc<AppService>>,
    Extension(principal): Extension<crate::input_sessions::Principal>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<Json<Value>, AppError> {
    if query.is_some_and(|q| !q.is_empty()) {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "시리얼 상태 조회에는 UID나 추가 인자를 지정하지 않습니다.",
        ));
    }
    app.execute_remote(&principal, "tty_get_status", json!({}))
        .await
        .map(Json)
}
async fn rest_storage_status(
    State(app): State<Arc<AppService>>,
    Extension(principal): Extension<crate::input_sessions::Principal>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<Json<Value>, AppError> {
    if query.is_some_and(|q| !q.is_empty()) {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "저장소 상태에는 추가 query 인자를 지정하지 않습니다.",
        ));
    }
    app.execute_storage(None, &principal, "storage_get_status", json!({}))
        .map(Json)
}
async fn rest_network_status(
    State(app): State<Arc<AppService>>,
    Extension(principal): Extension<crate::input_sessions::Principal>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<Json<Value>, AppError> {
    if query.is_some_and(|q| !q.is_empty()) {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "네트워크 상태에는 UID나 query 인자를 지정하지 않습니다.",
        ));
    }
    app.execute_remote(&principal, "network_get_status", json!({}))
        .await
        .map(Json)
}
async fn rest_state(State(app): State<Arc<AppService>>) -> Result<Json<Value>, AppError> {
    app.execute("app_get_state", json!({})).await.map(Json)
}
async fn rest_channels(State(app): State<Arc<AppService>>) -> Result<Json<Value>, AppError> {
    app.execute("channels_list", json!({})).await.map(Json)
}
async fn rest_active(State(app): State<Arc<AppService>>) -> Result<Json<Value>, AppError> {
    app.execute("channel_get_active", json!({})).await.map(Json)
}
async fn rest_select(
    State(app): State<Arc<AppService>>,
    Json(args): Json<Value>,
) -> Result<Json<Value>, AppError> {
    app.execute("channel_activate", args).await.map(Json)
}
async fn rest_clear(State(app): State<Arc<AppService>>) -> Result<Json<Value>, AppError> {
    app.execute("channel_deactivate", json!({})).await.map(Json)
}
async fn rest_video_status(State(app): State<Arc<AppService>>) -> Result<Json<Value>, AppError> {
    app.execute("video_get_status", json!({})).await.map(Json)
}
async fn rest_video_open(
    State(app): State<Arc<AppService>>,
    Json(args): Json<Value>,
) -> Result<Json<Value>, AppError> {
    app.execute("video_open", args).await.map(Json)
}
async fn rest_video_close(State(app): State<Arc<AppService>>) -> Result<Json<Value>, AppError> {
    app.execute("video_close", json!({})).await.map(Json)
}
async fn rest_video_snapshot(
    State(app): State<Arc<AppService>>,
    Query(args): Query<crate::video::SnapshotArgs>,
) -> Result<Response, AppError> {
    let image = app.snapshot_image(args.format).await?;
    let info = &image.info;
    Response::builder()
        .header("content-type", image.format.mime())
        .header("cache-control", "no-store")
        .header("x-minikmb-uid", info.uid.display())
        .header("x-minikmb-generation", info.generation)
        .header("x-minikmb-frame", &info.frame_id)
        .header("x-minikmb-stream", &info.stream_id)
        .header("x-minikmb-width", info.width)
        .header("x-minikmb-height", info.height)
        .header("x-minikmb-received-at", info.received_at)
        .body(axum::body::Body::from(image.bytes))
        .map_err(|_| AppError::new("ENCODE_FAILED", "이미지 응답 생성에 실패했습니다."))
}
pub fn input_schema(name: &str) -> Value {
    if let Some(schema) = crate::firmware_api::schema(name) {
        return schema;
    }
    if let Some(schema) = crate::observability::schema(name) {
        return schema;
    }
    if let Some(schema) = crate::wake::schema(name) {
        return schema;
    }
    if let Some(schema) = crate::network_api::schema(name) {
        return schema;
    }
    if let Some(schema) = crate::storage_api::schema(name) {
        return schema;
    }
    if let Some(schema) = crate::tty_api::schema(name) {
        return schema;
    }
    if let Some(schema) = crate::input_api::schema(name) {
        return schema;
    }
    if name == "video_open" {
        serde_json::to_value(schemars::schema_for!(crate::media::CaptureMode)).unwrap()
    } else if name == "video_snapshot" {
        serde_json::to_value(schemars::schema_for!(crate::video::SnapshotArgs)).unwrap()
    } else if name == "channel_activate" {
        json!({"type":"object","properties":{"uid":{"type":"string","description":"0x + 24 hex digits"}},"required":["uid"],"additionalProperties":false})
    } else {
        json!({"type":"object","additionalProperties":false})
    }
}
pub fn request_example(
    command: &crate::service::CommandDoc,
    address: Option<SocketAddr>,
) -> String {
    let address = address.unwrap_or_else(|| "127.0.0.1:18180".parse().unwrap());
    let mut request = format!(
        "curl.exe -X {} -H 'Authorization: Bearer <API_KEY>'",
        command.method
    );
    if ["POST", "PUT"].contains(&command.method) {
        let body = crate::api_docs::example_args(command.name);
        request.push_str(&format!(
            " -H 'Content-Type: application/json' --data '{body}'"
        ));
    }
    request.push_str(&format!(" http://{address}{}", command.path));
    if command.name == "video_snapshot" {
        request.push_str("?format=jpeg --output snapshot.jpg");
    }
    request
}
pub fn openapi() -> Value {
    crate::api_docs::openapi(None)
}
pub fn openapi_for(address: Option<SocketAddr>) -> Value {
    crate::api_docs::openapi(address)
}
#[derive(Clone)]
struct Mcp {
    app: Arc<AppService>,
    subscriptions: Arc<events::Subscriptions>,
}
fn error_context(context: &RequestContext<RoleServer>) -> crate::remote_error::Context {
    context
        .extensions
        .get::<axum::http::request::Parts>()
        .and_then(|p| p.extensions.get::<crate::remote_error::Context>())
        .cloned()
        .unwrap_or_default()
}
impl ServerHandler for Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_resources().enable_resources_subscribe().build()).with_instructions("MiniKMB: one globally active channel across GUI/REST/MCP. Read channels_list, select one with channel_activate; ordinary controls take no UID. Mouse coordinates are capture pixels. Read minikmb://reference/guide and minikmb://reference/openapi. Subscribe to minikmb://events/snapshot for metadata invalidations, then read it again. For large files call storage_upload_prepare, POST bytes to its REST url using the same Bearer key, then poll storage_job_get; never inline file bytes in MCP.")
    }
    async fn subscribe(
        &self,
        request: rmcp::model::SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.subscribe_events(request.uri, context).await
    }
    async fn unsubscribe(
        &self,
        request: rmcp::model::UnsubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.unsubscribe_events(request.uri, context).await
    }
    fn accepted_subscription_filter(
        &self,
        requested: &rmcp::model::SubscriptionFilter,
    ) -> Option<rmcp::model::SubscriptionFilter> {
        let mut filter = rmcp::model::SubscriptionFilter::new();
        filter.resource_subscriptions = requested.resource_subscriptions.as_ref().map(|uris| {
            let mut accepted = uris
                .iter()
                .filter(|uri| events::live(uri))
                .cloned()
                .collect::<Vec<_>>();
            accepted.sort();
            accepted.dedup();
            accepted
        });
        Some(filter)
    }
    async fn listen(&self, context: rmcp::service::SubscriptionContext) -> Result<(), ErrorData> {
        self.listen_events(context).await
    }
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: COMMANDS
                .iter()
                .map(|c| {
                    let mut tool = Tool::new(
                        c.name,
                        c.description,
                        input_schema(c.name).as_object().unwrap().clone(),
                    );
                    tool.title = Some(c.title.into());
                    tool.output_schema = crate::api_docs::response_schema(c.name)
                        .map(|v| Arc::new(v.as_object().unwrap().clone()));
                    tool
                })
                .collect(),
            ..Default::default()
        })
    }
    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ListResourcesResult, ErrorData> {
        if request.is_some_and(|p| p.cursor.is_some()) {
            return Err(ErrorData::invalid_params(
                "리소스 목록에는 다음 페이지가 없습니다.",
                Some(json!(error_context(&context).failure(AppError::new(
                    "INVALID_ARGUMENTS",
                    "리소스 목록에는 다음 페이지가 없습니다."
                )))),
            ));
        }
        Ok(rmcp::model::ListResourcesResult {
            resources: crate::remote_resources::RESOURCES
                .iter()
                .map(|r| {
                    rmcp::model::Resource::new(r.uri, r.name)
                        .with_title(r.name)
                        .with_description(r.description)
                        .with_mime_type(r.mime)
                })
                .collect(),
            ..Default::default()
        })
    }
    async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::ReadResourceResponse, ErrorData> {
        let failure = error_context(&context);
        if request.uri.starts_with("minikmb://active/") {
            if let Ok(b) = self.app.active.bind() {
                failure.observe(b);
            }
        }
        let owner = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|p| p.extensions.get::<crate::input_sessions::Principal>())
            .ok_or_else(|| {
                ErrorData::invalid_request(
                    "인증된 연결이 필요합니다.",
                    Some(json!(failure.failure(AppError::new(
                        "UNAUTHORIZED",
                        "인증된 연결이 필요합니다."
                    )))),
                )
            })?;
        let resource = crate::remote_resources::RESOURCES
            .iter()
            .find(|r| r.uri == request.uri)
            .ok_or_else(|| {
                ErrorData::resource_not_found(
                    "등록된 리소스가 아닙니다.",
                    Some(json!(failure.failure(AppError::new(
                        "RESOURCE_NOT_FOUND",
                        "등록된 리소스가 아닙니다."
                    )))),
                )
            })?;
        let text = crate::remote_resources::read(&self.app, owner, &request.uri)
            .await
            .map_err(|e| {
                ErrorData::resource_not_found(e.message.clone(), Some(json!(failure.failure(e))))
            })?;
        Ok(
            rmcp::model::ReadResourceResult::new(vec![rmcp::model::ResourceContents::text(
                text,
                request.uri,
            )
            .with_mime_type(resource.mime)])
            .with_ttl_ms(0)
            .with_cache_scope(rmcp::model::CacheScope::Private)
            .into(),
        )
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let failure = error_context(&context);
        if COMMANDS
            .iter()
            .any(|c| c.name == request.name && c.active_required)
        {
            if let Ok(b) = self.app.active.bind() {
                failure.observe(b);
            }
        }
        let principal = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|p| p.extensions.get::<crate::input_sessions::Principal>())
            .cloned();
        let Some(principal) =
            principal.filter(|p| !p.cancelled.is_cancelled() && !p.ended.is_cancelled())
        else {
            return Ok(rmcp::model::CallToolResult::structured_error(json!(
                failure.failure(AppError::new("UNAUTHORIZED", "인증된 연결이 필요합니다."))
            ))
            .into());
        };
        let args = Value::Object(request.arguments.unwrap_or_default());
        if request.name == "video_snapshot" {
            let result = match serde_json::from_value::<crate::video::SnapshotArgs>(args) {
                Ok(args) => self.app.snapshot_image(args.format).await,
                Err(_) => Err(AppError::new(
                    "INVALID_ARGUMENTS",
                    "format은 jpeg/png이며 UID는 지정하지 않습니다.",
                )),
            };
            return Ok(match result {
                Ok(image) => {
                    use base64::Engine;
                    let mut result = rmcp::model::CallToolResult::success(vec![
                        rmcp::model::ContentBlock::image(
                            base64::engine::general_purpose::STANDARD.encode(&image.bytes),
                            image.format.mime(),
                        ),
                    ]);
                    result.structured_content = Some(json!(image.info));
                    result.into()
                }
                Err(e) => {
                    rmcp::model::CallToolResult::structured_error(json!(failure.failure(e))).into()
                }
            });
        }
        let result = self
            .app
            .execute_remote(&principal, &request.name, args)
            .await;
        Ok(match result {
            Ok(value) => rmcp::model::CallToolResult::structured(value).into(),
            Err(e) => {
                rmcp::model::CallToolResult::structured_error(json!(failure.failure(e))).into()
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn authenticated_rest_and_mcp_input_share_owner_and_cleanup() {
        let (_dir, mut app, wire) = crate::input_api::tests::setup();
        crate::tty::tests::install(&mut app);
        let storage_mock = crate::storage::tests::install(&mut app);
        crate::network::manager::tests::install(&mut app);
        app.remote.write().rest = Some("127.0.0.1:18180".parse().unwrap());
        let key = app.keys.create("input-A").unwrap();
        let other = app.keys.create("input-B").unwrap();
        let rest = rest_router(app.clone()).layer(middleware::from_fn_with_state(
            Auth {
                app: app.clone(),
                address: "127.0.0.1:18180".parse().unwrap(),
            },
            authenticate,
        ));
        let post = |path: &str, token: &str, args: Value| {
            Request::builder()
                .method("POST")
                .uri(path)
                .header("host", "127.0.0.1:18180")
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", "application/json")
                .body(axum::body::Body::from(args.to_string()))
                .unwrap()
        };
        let storage_owner = crate::input_sessions::Principal {
            key_id: key.info.id.clone(),
            client_id: "rest".into(),
            cancelled: Default::default(),
            ended: Default::default(),
        };
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/hid/keys/sequence",
                key.token.expose(),
                json!(["a", "b", "c"]),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let sequence: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(sequence["completed_keys"], 3);
        assert_eq!(sequence["packets"], 6);
        assert_eq!(
            wire.calls.lock().iter().map(|c| c.1).collect::<Vec<_>>(),
            vec![0x101, 0x102, 0x101, 0x102, 0x101, 0x102]
        );
        let before = wire.calls.lock().len();
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/hid/keys/sequence",
                key.token.expose(),
                json!({"keys":["a","unknown-key"]}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(wire.calls.lock().len(), before);
        for (path, body, expected) in [
            (
                "/api/v1/active/hid/click",
                json!({"x":0,"y":0,"hold_ms":0}),
                vec![0x103, 0x104, 0x105],
            ),
            (
                "/api/v1/active/hid/click",
                json!({"x":0,"y":0,"count":2,"hold_ms":0}),
                vec![0x103, 0x104, 0x105, 0x104, 0x105],
            ),
            (
                "/api/v1/active/hid/double-click",
                json!({"x":1919,"y":1079,"hold_ms":0}),
                vec![0x103, 0x104, 0x105, 0x104, 0x105],
            ),
        ] {
            app.test_video_geometry(1920, 1080);
            let before = wire.calls.lock().len();
            assert_eq!(
                rest.clone()
                    .oneshot(post(path, key.token.expose(), body))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
            assert_eq!(
                wire.calls.lock()[before..]
                    .iter()
                    .map(|c| c.1)
                    .collect::<Vec<_>>(),
                expected
            );
        }
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/wake/configure",
                key.token.expose(),
                json!({"enabled":false,"mac":"","local_address":""}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/wake/send",
                key.token.expose(),
                json!({}),
            ))
            .await
            .unwrap();
        let disabled: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(disabled["code"], 129);
        for (path, expected) in [
            ("/api/v1/active/wake", StatusCode::OK),
            ("/api/v1/logs", StatusCode::OK),
            ("/api/v1/active/diagnostics", StatusCode::OK),
            (
                "/api/v1/active/diagnostics?uid=other",
                StatusCode::BAD_REQUEST,
            ),
            (
                "/api/v1/active/wake/adapters?uid=other",
                StatusCode::BAD_REQUEST,
            ),
        ] {
            let response = rest
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("host", "127.0.0.1:18180")
                        .header("authorization", format!("Bearer {}", key.token.expose()))
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
        }
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/storage/actions",
                key.token.expose(),
                json!({"action":"name_set","name":"mock channel"}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let accepted: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        let storage_id = accepted["job_id"].as_str().unwrap();
        crate::storage::tests::wait(&app, &storage_owner, storage_id).await;
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/storage/jobs/get",
                key.token.expose(),
                json!({"job_id":storage_id}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let completed: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(completed["state"], "completed");
        assert_eq!(completed["result"]["verified"], true);
        assert_eq!(
            rest.clone()
                .oneshot(post(
                    "/api/v1/storage/jobs/get",
                    other.token.expose(),
                    json!({"job_id":storage_id})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            rest.clone()
                .oneshot(post(
                    "/api/v1/active/storage/actions",
                    key.token.expose(),
                    json!({"action":"name_get","uids":[]})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        for (path, args) in [
            ("/api/v1/active/tty/open", json!({})),
            ("/api/v1/active/tty/write", json!({"bytes":[65,13,10]})),
            ("/api/v1/active/tty/read", json!({})),
            ("/api/v1/active/tty/close", json!({})),
        ] {
            let response = rest
                .clone()
                .oneshot(post(path, key.token.expose(), args))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let value: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap(),
            )
            .unwrap();
            if path.ends_with("/write") {
                assert_eq!(value["complete"], true);
                assert_eq!(value["accepted"], 3);
            }
        }
        assert_eq!(
            rest.clone()
                .oneshot(post(
                    "/api/v1/active/tty/write?uid=other",
                    key.token.expose(),
                    json!({"bytes":[1]})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/input-sessions",
                key.token.expose(),
                json!({}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let opened: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        let id = opened["input_session"].as_str().unwrap();
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/hid/keys/down",
                key.token.expose(),
                json!({"usage":4,"input_session":id}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let result: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["usb_completed"], true);
        assert_eq!(result["uid"], opened["uid"]);
        let count = wire.calls.lock().len();
        assert_eq!(
            rest.clone()
                .oneshot(post(
                    "/api/v1/active/hid/keys/up",
                    other.token.expose(),
                    json!({"usage":4,"input_session":id})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            rest.clone()
                .oneshot(post(
                    "/api/v1/active/hid/keys/down",
                    key.token.expose(),
                    json!({"usage":5,"uids":[]})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(wire.calls.lock().len(), count);
        assert_eq!(
            rest.clone()
                .oneshot(post(
                    "/api/v1/input-sessions/close",
                    key.token.expose(),
                    json!({"input_session":id})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        assert_eq!(wire.calls.lock().last().unwrap().1, 0x107);
        let stop = CancellationToken::new();
        let mcp = mcp_router(app.clone(), stop.clone()).layer(middleware::from_fn_with_state(
            Auth {
                app: app.clone(),
                address: "127.0.0.1:18181".parse().unwrap(),
            },
            authenticate,
        ));
        let req = |body: Value, sid: Option<&str>| {
            let mut r = Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("host", "127.0.0.1:18181")
                .header("authorization", format!("Bearer {}", key.token.expose()))
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream");
            if let Some(id) = sid {
                r = r
                    .header("mcp-session-id", id)
                    .header("mcp-protocol-version", "2025-11-25");
            }
            r.body(axum::body::Body::from(body.to_string())).unwrap()
        };
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"mock-input-client","version":"1"}}}),None)).await.unwrap();
        let sid = response.headers()["mcp-session-id"]
            .to_str()
            .unwrap()
            .to_owned();
        let _ = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let _ = mcp
            .clone()
            .oneshot(req(
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                Some(&sid),
            ))
            .await
            .unwrap();
        for (name, args) in [
            ("logs_get_state", json!({})),
            ("diagnostics_get", json!({})),
            ("wake_get_status", json!({})),
            ("wake_send", json!({})),
            ("tty_open", json!({})),
            ("tty_write", json!({"bytes":[66,13,10]})),
            ("tty_get_status", json!({})),
        ] {
            let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":name,"arguments":args}}),Some(&sid))).await.unwrap();
            let value = mcp_payload(
                &axum::body::to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap(),
            );
            assert_ne!(value["result"]["isError"], true, "{value}");
            if name == "wake_send" {
                assert_eq!(value["result"]["structuredContent"]["code"], 129);
                assert_eq!(
                    value["result"]["structuredContent"]["wake_confirmed"],
                    false
                );
            }
            if name == "tty_get_status" {
                assert_eq!(value["result"]["structuredContent"]["bytes_accepted"], 6);
            }
        }
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":21,"method":"tools/call","params":{"name":"storage_action","arguments":{"action":"card_info"}}}),Some(&sid))).await.unwrap();
        let accepted = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        let mcp_storage_id = accepted["result"]["structuredContent"]["job_id"]
            .as_str()
            .unwrap();
        crate::storage::tests::wait(&app, &storage_owner, mcp_storage_id).await;
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/storage/jobs/get",
                key.token.expose(),
                json!({"job_id":mcp_storage_id}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let completed: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(completed["state"], "completed");
        assert_eq!(
            completed["result"]["current_uid"],
            "0x010101010101010101010101"
        );
        // MCP opens the single active disk; REST writes/reads/closes the same original lease.
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":22,"method":"tools/call","params":{"name":"storage_action","arguments":{"action":"disk_open","id":2,"path":"direct","writable":true}}}),Some(&sid))).await.unwrap();
        let accepted = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        let id = accepted["result"]["structuredContent"]["job_id"]
            .as_str()
            .unwrap();
        assert!(
            crate::storage::tests::wait(&app, &storage_owner, id)
                .await
                .state
                == crate::storage::JobState::Completed
        );
        for args in [
            json!({"action":"disk_write","id":2,"lba":7,"bytes":vec![239;512],"confirmed":true}),
            json!({"action":"disk_read","id":2,"lba":7}),
            json!({"action":"disk_close","id":2}),
        ] {
            let response = rest
                .clone()
                .oneshot(post(
                    "/api/v1/active/storage/actions",
                    key.token.expose(),
                    args.clone(),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let accepted: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap(),
            )
            .unwrap();
            let result = crate::storage::tests::wait(
                &app,
                &storage_owner,
                accepted["job_id"].as_str().unwrap(),
            )
            .await;
            assert!(
                result.state == crate::storage::JobState::Completed,
                "{}",
                serde_json::to_string(&result).unwrap()
            );
            if args["action"] == "disk_read" {
                assert_eq!(
                    result.result.unwrap()["sector"]["bytes"],
                    json!(vec![239; 512])
                );
            }
        }
        assert!(app.storage.resources_info().is_none());
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":23,"method":"tools/call","params":{"name":"storage_upload_prepare","arguments":{"kind":"optical","name":"boot.iso","size":81920,"confirmed":true}}}),Some(&sid))).await.unwrap();
        let prepared = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        let ticket = &prepared["result"]["structuredContent"];
        let upload_url = ticket["url"]
            .as_str()
            .expect("MCP returns a REST upload URL");
        let path = upload_url.strip_prefix("http://127.0.0.1:18180").unwrap();
        let bytes = crate::storage::tests::import_cases::iso();
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "127.0.0.1:18180")
            .header("authorization", format!("Bearer {}", key.token.expose()))
            .header("content-type", "application/octet-stream")
            .header("content-length", bytes.len())
            .body(axum::body::Body::from(bytes))
            .unwrap();
        let response = rest.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let accepted: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            crate::storage::tests::wait(&app, &storage_owner, accepted["job_id"].as_str().unwrap())
                .await
                .state
                == crate::storage::JobState::Completed
        );
        storage_mock.seed_disk(2, 18433, 1);
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/storage/actions",
                key.token.expose(),
                json!({"action":"disk_create","format":2,"size_mib":64,"confirmed":true}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let created: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            crate::storage::tests::wait(&app, &storage_owner, created["job_id"].as_str().unwrap())
                .await
                .state
                == crate::storage::JobState::Completed
        );
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":24,"method":"tools/call","params":{"name":"storage_action","arguments":{"action":"disk_format","id":2,"format":2,"confirmed":true,"expected_size":9437696}}}),Some(&sid))).await.unwrap();
        let formatted = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        let job = crate::storage::tests::wait(
            &app,
            &storage_owner,
            formatted["result"]["structuredContent"]["job_id"]
                .as_str()
                .unwrap(),
        )
        .await;
        assert!(
            job.state == crate::storage::JobState::Completed,
            "{}",
            serde_json::to_string(&job).unwrap()
        );
        assert_eq!(job.result.unwrap()["image"]["size"], 9437696u64);
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/storage/actions",
                key.token.expose(),
                json!({"action":"card_format","format":2,"confirmed":true}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let accepted: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        let formatted =
            crate::storage::tests::wait(&app, &storage_owner, accepted["job_id"].as_str().unwrap())
                .await;
        assert!(formatted.state == crate::storage::JobState::Completed);
        assert_eq!(formatted.result.unwrap()["formatted"], true);
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":25,"method":"tools/call","params":{"name":"storage_action","arguments":{"action":"card_format_status"}}}),Some(&sid))).await.unwrap();
        let checked = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        let checked = crate::storage::tests::wait(
            &app,
            &storage_owner,
            checked["result"]["structuredContent"]["job_id"]
                .as_str()
                .unwrap(),
        )
        .await;
        assert_eq!(checked.result.unwrap()["outcome"], "idle");
        let response=rest.clone().oneshot(post("/api/v1/active/network/configure",key.token.expose(),json!({"server_ip":"10.20.30.1","pool_start":100,"pool_end":199,"lease_seconds":3600}))).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":27,"method":"tools/call","params":{"name":"network_set_mode","arguments":{"mode":"emulation"}}}),Some(&sid))).await.unwrap();
        let mode = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        assert_eq!(mode["result"]["structuredContent"]["mode"], "emulation");
        let response = rest
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/active/network")
                    .header("host", "127.0.0.1:18180")
                    .header("authorization", format!("Bearer {}", key.token.expose()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let status: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(status["config"]["server_ip"], "10.20.30.1");
        assert_eq!(status["mcu"]["enabled"], true);
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/network/scan/start",
                key.token.expose(),
                json!({"target":"10.20.30.100","start":22,"end":24,"timeout_ms":250}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let scan: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(scan["running"], true);
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":28,"method":"tools/call","params":{"name":"network_scan_stop","arguments":{}}}),Some(&sid))).await.unwrap();
        let stopped = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        assert_eq!(stopped["result"]["structuredContent"]["phase"], "stopped");
        let reserved = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let forward_port = reserved.local_addr().unwrap().port();
        drop(reserved);
        let rule = json!({"id":"auth-ssh","protocol":"tcp","listen_port":forward_port,"target_ip":"10.20.30.100","target_port":22});
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/network/forward/start",
                key.token.expose(),
                json!({"rules":[rule]}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let forward: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(forward["running"], true);
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":29,"method":"tools/call","params":{"name":"network_forward_stop","arguments":{}}}),Some(&sid))).await.unwrap();
        let stopped = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        assert_eq!(
            stopped["result"]["structuredContent"]["cleanup_required"],
            false
        );
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, forward_port))
            .await
            .unwrap();
        let response = rest
            .clone()
            .oneshot(post(
                "/api/v1/active/network/mode",
                key.token.expose(),
                json!({"mode":"off"}),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let source_dir = tempfile::tempdir().unwrap();
        std::fs::write(source_dir.path().join("fixture-key.txt"), b"test data only").unwrap();
        let binding = app.active.bind().unwrap();
        let source = app
            .storage
            .approve_folder(
                binding.clone(),
                crate::storage::folder_image::Root::approve(source_dir.path().to_path_buf())
                    .unwrap(),
                app.storage.import_session(&binding).unwrap(),
            )
            .unwrap();
        let response = rest
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/active/storage")
                    .header("host", "127.0.0.1:18180")
                    .header("authorization", format!("Bearer {}", key.token.expose()))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let source_status: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            source_status["folder_sources"][0]["source_id"],
            source.source_id
        );
        assert!(source_status["folder_sources"][0].get("path").is_none());
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":26,"method":"tools/call","params":{"name":"storage_action","arguments":{"action":"folder_share","source_id":source.source_id,"confirmed":true}}}),Some(&sid))).await.unwrap();
        let shared = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        let share_id = shared["result"]["structuredContent"]["job_id"]
            .as_str()
            .unwrap();
        let shared = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            loop {
                let info = app.storage.job(&storage_owner, share_id).unwrap();
                if info.finished_at.is_some() {
                    break info;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            shared.state == crate::storage::JobState::Completed,
            "{}",
            serde_json::to_string(&shared).unwrap()
        );
        assert_eq!(shared.result.unwrap()["readonly"], true);
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":101,"method":"tools/call","params":{"name":"hid_key_sequence","arguments":{"keys":["a","b","c"]}}}),Some(&sid))).await.unwrap();
        let sequence = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        assert_eq!(
            sequence["result"]["structuredContent"]["completed_keys"], 3,
            "{sequence}"
        );
        app.test_video_geometry(1920, 1080);
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":102,"method":"tools/call","params":{"name":"hid_double_click","arguments":{"x":0,"y":0,"hold_ms":0}}}),Some(&sid))).await.unwrap();
        let clicked = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        assert_eq!(
            clicked["result"]["structuredContent"]["usb_completed"], true,
            "{clicked}"
        );
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"input_open","arguments":{}}}),Some(&sid))).await.unwrap();
        let opened = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        let input = opened["result"]["structuredContent"]["input_session"]
            .as_str()
            .unwrap();
        let response=mcp.clone().oneshot(req(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"hid_key_down","arguments":{"usage":5,"input_session":input}}}),Some(&sid))).await.unwrap();
        let result = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        assert_eq!(
            result["result"]["structuredContent"]["usb_completed"], true,
            "{result}"
        );
        let count = wire.calls.lock().len();
        let delete = Request::builder()
            .method("DELETE")
            .uri("/mcp")
            .header("host", "127.0.0.1:18181")
            .header("authorization", format!("Bearer {}", key.token.expose()))
            .header("mcp-session-id", &sid)
            .header("mcp-protocol-version", "2025-11-25")
            .body(axum::body::Body::empty())
            .unwrap();
        assert!(mcp.oneshot(delete).await.unwrap().status().is_success());
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while wire.calls.lock().len() <= count {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(wire.calls.lock().last().unwrap().1, 0x107);
        stop.cancel();
        tokio::time::sleep(std::time::Duration::from_millis(70)).await;
        assert!(
            !app.tty
                .cached(&app.active.bind().unwrap())
                .unwrap()
                .receiving
        );
        app.tty.shutdown().await;
    }
    pub(super) fn mcp_payload(bytes: &[u8]) -> Value {
        if let Ok(v) = serde_json::from_slice(bytes) {
            return v;
        }
        std::str::from_utf8(bytes)
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter_map(|s| serde_json::from_str::<Value>(s.trim()).ok())
            .rfind(|v| v.get("id").is_some())
            .expect("JSON-RPC response in JSON or SSE")
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "Read-only hardware test: exact MINIKMB_VIDEO_TEST_UID and MINIKMB_VIDEO_TEST_OUTPUT required; close Electron first"]
    async fn hardware_video_rest_and_mcp_share_one_capture() {
        let uid = crate::model::Uid::try_from(
            std::env::var("MINIKMB_VIDEO_TEST_UID").expect("exact UID required"),
        )
        .unwrap();
        let output = std::path::PathBuf::from(
            std::env::var("MINIKMB_VIDEO_TEST_OUTPUT").expect("artifact directory required"),
        );
        assert!(output.is_dir());
        let mut devices = crate::device::DeviceManager::default();
        let inventory = devices.refresh(true);
        assert!(
            inventory
                .channels
                .iter()
                .any(|c| c.uid == uid && c.connected && c.capture_id.is_some()),
            "exact MCU/capture pair missing"
        );
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        *app.inventory.write() = inventory;
        let key = app.keys.create("temporary hardware test").unwrap();
        app.select(Some(uid.clone())).await.unwrap();
        app.open_video(crate::media::CaptureMode {
            width: 1920,
            height: 1080,
            fps: 30.0,
        })
        .await
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while app.current_frame().is_err() {
            assert!(
                std::time::Instant::now() < deadline,
                "first frame timeout: {:?}",
                app.snapshot().video
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let first = app.current_frame().unwrap().info;
        assert_eq!((first.width, first.height), (1920, 1080));
        let rate_started = std::time::Instant::now();
        let rate_first = app.current_frame().unwrap().data.id;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let observed_fps = (app.current_frame().unwrap().data.id - rate_first) as f64
            / rate_started.elapsed().as_secs_f64();
        let rest = rest_router(app.clone()).layer(middleware::from_fn_with_state(
            Auth {
                app: app.clone(),
                address: "127.0.0.1:18180".parse().unwrap(),
            },
            authenticate,
        ));
        let mut artifacts = Vec::new();
        for (format, mime, ext) in [("jpeg", "image/jpeg", "jpg"), ("png", "image/png", "png")] {
            let response = rest
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/v1/active/video/snapshot?format={format}"))
                        .header("host", "127.0.0.1:18180")
                        .header("authorization", format!("Bearer {}", key.token.expose()))
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["content-type"], mime);
            assert_eq!(response.headers()["cache-control"], "no-store");
            assert_eq!(response.headers()["x-minikmb-uid"], uid.display());
            assert_eq!(response.headers()["x-minikmb-stream"], first.stream_id);
            let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
                .await
                .unwrap();
            let image = image::load_from_memory(&bytes).unwrap();
            assert_eq!((image.width(), image.height()), (1920, 1080));
            std::fs::write(output.join(format!("native-rest-capture.{ext}")), &bytes).unwrap();
            artifacts.push(json!({"format":format,"bytes":bytes.len(),"width":image.width(),"height":image.height()}));
        }
        let token = CancellationToken::new();
        let mcp = mcp_router(app.clone(), token.clone()).layer(middleware::from_fn_with_state(
            Auth {
                app: app.clone(),
                address: "127.0.0.1:18181".parse().unwrap(),
            },
            authenticate,
        ));
        let request = |body: Value, sid: Option<&str>| {
            let mut r = Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("host", "127.0.0.1:18181")
                .header("authorization", format!("Bearer {}", key.token.expose()))
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream");
            if let Some(sid) = sid {
                r = r
                    .header("mcp-session-id", sid)
                    .header("mcp-protocol-version", "2025-11-25");
            }
            r.body(axum::body::Body::from(body.to_string())).unwrap()
        };
        let response=mcp.clone().oneshot(request(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"readonly-hardware-video-test","version":"0"}}}),None)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let sid = response.headers()["mcp-session-id"]
            .to_str()
            .unwrap()
            .to_owned();
        let _ = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let _ = mcp
            .clone()
            .oneshot(request(
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                Some(&sid),
            ))
            .await
            .unwrap();
        let response=mcp.clone().oneshot(request(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"video_snapshot","arguments":{"format":"jpeg"}}}),Some(&sid))).await.unwrap();
        let result = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
                .await
                .unwrap(),
        );
        assert_eq!(result["result"]["isError"], false, "{result}");
        assert_eq!(result["result"]["content"][0]["type"], "image");
        assert_eq!(result["result"]["content"][0]["mimeType"], "image/jpeg");
        assert_eq!(result["result"]["structuredContent"]["uid"], uid.display());
        assert_eq!(
            result["result"]["structuredContent"]["stream_id"],
            first.stream_id
        );
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(result["result"]["content"][0]["data"].as_str().unwrap())
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (1920, 1080));
        let response=mcp.clone().oneshot(request(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"video_snapshot","arguments":{"format":"png"}}}),Some(&sid))).await.unwrap();
        let png = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
                .await
                .unwrap(),
        );
        assert_eq!(png["result"]["isError"], false);
        assert_eq!(png["result"]["content"][0]["mimeType"], "image/png");
        assert_eq!(
            png["result"]["structuredContent"]["stream_id"],
            first.stream_id
        );
        let png_bytes = base64::engine::general_purpose::STANDARD
            .decode(png["result"]["content"][0]["data"].as_str().unwrap())
            .unwrap();
        let png_image = image::load_from_memory(&png_bytes).unwrap();
        assert_eq!((png_image.width(), png_image.height()), (1920, 1080));
        let later = app.current_frame().unwrap().info;
        app.open_video(crate::media::CaptureMode {
            width: 1,
            height: 1,
            fps: 30.0,
        })
        .await
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        loop {
            let status = app.snapshot().video;
            if status.frame.is_some() && !status.notice.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "previous mode restoration failed: {status:?}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let restored = app.current_frame().unwrap().info;
        assert_ne!(restored.stream_id, first.stream_id);
        assert_eq!((restored.width, restored.height), (1920, 1080));
        app.select(None).await.unwrap();
        assert_eq!(app.current_frame().err().unwrap().code, "NO_ACTIVE_CHANNEL");
        token.cancel();
        app.shutdown().await;
        let evidence = json!({"uid":uid,"first":first,"last":later,"observed_decode_fps":observed_fps,"restored_after_invalid_mode":restored,"rest":artifacts,"mcp":{"jpeg_bytes":bytes.len(),"png_bytes":png_bytes.len(),"same_stream":true},"after_deactivate":"NO_ACTIVE_CHANNEL","hid_sent":false,"sd_writes":false});
        std::fs::write(
            output.join("native-video-readonly-validation.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
        println!("{}", serde_json::to_string(&evidence).unwrap());
    }
    #[tokio::test]
    async fn actual_mcp_handshake_tools_and_revoked_session() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        let key = app.keys.create("mcp").unwrap();
        let ct = CancellationToken::new();
        let router = mcp_router(app.clone(), ct.clone()).layer(middleware::from_fn_with_state(
            Auth {
                app: app.clone(),
                address: "127.0.0.1:18181".parse().unwrap(),
            },
            authenticate,
        ));
        let req = |body: Value, sid: Option<&str>| {
            let mut r = Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("host", "127.0.0.1:18181")
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .header("authorization", format!("Bearer {}", key.token.expose()));
            if let Some(sid) = sid {
                r = r
                    .header("mcp-session-id", sid)
                    .header("mcp-protocol-version", "2025-11-25");
            }
            r.body(axum::body::Body::from(body.to_string())).unwrap()
        };
        let response=router.clone().oneshot(req(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"native-unit-test","version":"0"}}}),None)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let sid = response
            .headers()
            .get("mcp-session-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let body = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let value = mcp_payload(&body);
        assert!(value["result"]["capabilities"]["tools"].is_object());
        let _ = router
            .clone()
            .oneshot(req(
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                Some(&sid),
            ))
            .await
            .unwrap();
        let response = router
            .clone()
            .oneshot(req(
                json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
                Some(&sid),
            ))
            .await
            .unwrap();
        let value = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
                .await
                .unwrap(),
        );
        assert_eq!(
            value["result"]["tools"].as_array().unwrap().len(),
            COMMANDS.len()
        );
        let response=router.clone().oneshot(req(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"channel_get_active","arguments":{}}}),Some(&sid))).await.unwrap();
        let value = mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        assert_eq!(
            value["result"]["structuredContent"]["code"],
            "NO_ACTIVE_CHANNEL"
        );
        let other = app.keys.create("other client").unwrap();
        let stolen = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "127.0.0.1:18181")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("authorization", format!("Bearer {}", other.token.expose()))
            .header("mcp-session-id", &sid)
            .header("mcp-protocol-version", "2025-11-25")
            .body(axum::body::Body::from(
                json!({"jsonrpc":"2.0","id":7,"method":"tools/list"}).to_string(),
            ))
            .unwrap();
        assert_eq!(
            router.clone().oneshot(stolen).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        let get = Request::builder()
            .uri("/mcp")
            .header("host", "127.0.0.1:18181")
            .header("accept", "text/event-stream")
            .header("authorization", format!("Bearer {}", key.token.expose()))
            .header("mcp-session-id", &sid)
            .header("mcp-protocol-version", "2025-11-25")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = router.clone().oneshot(get).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let idle =
            tokio::spawn(async move { axum::body::to_bytes(response.into_body(), 65536).await });
        app.keys.revoke(&key.info.id).unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), idle)
                .await
                .unwrap()
                .unwrap()
                .is_ok(),
            "revoked idle SSE must close immediately"
        );
        assert!(app.keys.authenticate(other.token.expose()).is_some());
        assert_eq!(
            router
                .oneshot(req(
                    json!({"jsonrpc":"2.0","id":4,"method":"tools/list"}),
                    Some(&sid)
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        ct.cancel();
    }
    #[tokio::test]
    async fn auth_revocation_origin_and_uid_free_rest() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        let a = app.keys.create("A").unwrap();
        let b = app.keys.create("B").unwrap();
        let router = rest_router(app.clone()).layer(middleware::from_fn_with_state(
            Auth {
                app: app.clone(),
                address: "127.0.0.1:18180".parse().unwrap(),
            },
            authenticate,
        ));
        let request = |path: &str, token: Option<&str>, origin: Option<&str>| {
            let mut r = Request::builder()
                .uri(path)
                .header("host", "127.0.0.1:18180");
            if let Some(token) = token {
                r = r.header("authorization", format!("Bearer {token}"));
            }
            if let Some(origin) = origin {
                r = r.header("origin", origin);
            }
            r.body(axum::body::Body::empty()).unwrap()
        };
        assert_eq!(
            router
                .clone()
                .oneshot(request("/api/v1/state", None, None))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            router
                .clone()
                .oneshot(request(
                    "/api/v1/state",
                    Some(a.token.expose()),
                    Some("https://example.invalid")
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            router
                .clone()
                .oneshot(request("/api/v1/state", Some(a.token.expose()), None))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let response = router
            .clone()
            .oneshot(request("/api/v1/active", Some(a.token.expose()), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["code"],
            "NO_ACTIVE_CHANNEL"
        );
        app.keys.revoke(&a.info.id).unwrap();
        assert_eq!(
            router
                .clone()
                .oneshot(request("/api/v1/state", Some(a.token.expose()), None))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            router
                .oneshot(request("/api/v1/state", Some(b.token.expose()), None))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    #[tokio::test]
    async fn lifecycle_dependency_and_rollback_without_hardware() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        let servers = RemoteServers::new(app.clone());
        assert!(servers.mcp(true, 0, 0).await.is_err());
        let _key = app.keys.create("test").unwrap();
        servers.mcp(true, 0, 0).await.unwrap();
        assert!(app.remote.read().rest.is_some() && app.remote.read().mcp.is_some());
        servers.mcp(false, 0, 0).await.unwrap();
        assert!(app.remote.read().rest.is_some() && app.remote.read().mcp.is_none());
        servers.rest(false, 0).await.unwrap();
        assert!(app.remote.read().rest.is_none());
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = occupied.local_addr().unwrap().port();
        assert!(servers.mcp(true, 0, port).await.is_err());
        assert!(app.remote.read().rest.is_none());
        servers.rest(true, 0).await.unwrap();
        assert!(servers.mcp(true, 0, port).await.is_err());
        assert!(app.remote.read().rest.is_some());
        servers.shutdown().await;
    }
}

#[cfg(test)]
mod error_tests;
#[cfg(test)]
mod firmware_tests;
#[cfg(test)]
mod reference_tests;
