//! Private local transport adapter; the TCP REST authentication path is unchanged.
use super::*;

tokio::task_local! { static LOCAL_REQUEST: (); }

pub(crate) fn upload_address(rest: Option<SocketAddr>) -> Result<SocketAddr, AppError> {
    if LOCAL_REQUEST.try_with(|_| ()).is_ok() {
        Ok(([127, 0, 0, 1], 0).into())
    } else {
        rest.ok_or_else(|| AppError::new("SERVER_OFF", "REST 서버를 먼저 켜세요."))
    }
}

pub(crate) fn upload_response(mut value: Value) -> Value {
    if LOCAL_REQUEST.try_with(|_| ()).is_ok() {
        if let Some(url) = value.get("url").and_then(Value::as_str) {
            if let Ok(uri) = url.parse::<axum::http::Uri>() {
                value["url"] = json!(format!("ipc:{}", uri.path()));
                value["instructions"] = json!("Upload the original bytes with minikmb-cli upload URL FILE. This URL uses the existing local IPC connection, not a TCP server.");
            }
        }
    }
    value
}

#[derive(Clone)]
struct LocalAuth {
    app: Arc<AppService>,
    principal: crate::input_sessions::Principal,
}

pub(crate) fn ipc_router(
    app: Arc<AppService>,
    principal: crate::input_sessions::Principal,
) -> Router {
    rest_router(app.clone())
        .route("/ipc/v1/commands", get(catalog))
        .layer(middleware::from_fn_with_state(
            LocalAuth { app, principal },
            local_request,
        ))
}

async fn catalog() -> Json<Value> {
    Json(json!({
        "protocol": 1,
        "commands": COMMANDS.iter().map(|doc| json!({
            "name": doc.name, "method": doc.method, "path": doc.path,
            "title": doc.title, "description": doc.description,
            "active_required": doc.active_required, "input_schema": input_schema(doc.name)
        })).collect::<Vec<_>>(),
        "additional_routes": [
            {"method":"GET","path":"/api/v1/openapi.json"},
            {"method":"GET","path":"/api/v1/events"},
            {"method":"POST","path":"/api/v1/firmware/uploads/{upload_id}"},
            {"method":"POST","path":"/api/v1/active/storage/uploads/{upload_id}"}
        ]
    }))
}

async fn local_request(
    State(auth): State<LocalAuth>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let context = crate::remote_error::Context::new();
    context.authorize(auth.principal.cancelled.clone());
    if let Ok(binding) = auth.app.active.bind() {
        context.observe(binding);
    }
    request.extensions_mut().insert(context.clone());
    request.extensions_mut().insert(auth.principal.clone());
    let response = if auth.principal.ended.is_cancelled() {
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    } else {
        tokio::select! {
            biased;
            _=auth.principal.ended.cancelled()=>StatusCode::SERVICE_UNAVAILABLE.into_response(),
            response=LOCAL_REQUEST.scope((), next.run(request))=>response,
        }
    };
    let mut response = context.finish(response, true);
    response
        .headers_mut()
        .insert("x-minikmb-ipc", axum::http::HeaderValue::from_static("1"));
    response
}
