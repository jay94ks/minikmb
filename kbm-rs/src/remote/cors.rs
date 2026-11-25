//! Standalone file:// console support. Actual REST controls still require Bearer auth.
use super::*;

pub(super) fn local_file(auth: &Auth, request: &Request<axum::body::Body>) -> bool {
    request.headers().get("host").and_then(|v| v.to_str().ok())
        == Some(auth.address.to_string().as_str())
        && request
            .headers()
            .get("origin")
            .and_then(|v| v.to_str().ok())
            == Some("null")
        && request.uri().path().starts_with("/api/v1/")
}

pub(super) fn preflight(request: &Request<axum::body::Body>) -> Response {
    let method = request
        .headers()
        .get("access-control-request-method")
        .and_then(|v| v.to_str().ok());
    let headers = request
        .headers()
        .get("access-control-request-headers")
        .and_then(|v| v.to_str().ok());
    let allowed = headers.is_none_or(|value| {
        value.split(',').all(|name| {
            matches!(
                name.trim().to_ascii_lowercase().as_str(),
                "authorization" | "content-type" | "last-event-id"
            )
        })
    });
    if !matches!(method, Some("GET" | "POST" | "PUT" | "DELETE" | "HEAD")) || !allowed {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    let h = response.headers_mut();
    h.insert(
        "access-control-allow-methods",
        "GET, POST, PUT, DELETE, HEAD".parse().unwrap(),
    );
    h.insert(
        "access-control-allow-headers",
        "authorization, content-type, last-event-id"
            .parse()
            .unwrap(),
    );
    // Chromium's local-network preflight may add this. The listener is loopback-only.
    if request
        .headers()
        .get("access-control-request-private-network")
        .is_some_and(|v| v == "true")
    {
        h.insert(
            "access-control-allow-private-network",
            "true".parse().unwrap(),
        );
    }
    response
}

pub(super) fn expose(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert("access-control-allow-origin", "null".parse().unwrap());
    headers.append("vary", "Origin".parse().unwrap());
    headers.insert("access-control-expose-headers", "content-type, content-disposition, x-minikmb-request-id, x-minikmb-uid, x-minikmb-generation, x-minikmb-frame, x-minikmb-stream, x-minikmb-width, x-minikmb-height, x-minikmb-received-at".parse().unwrap());
    // No cookie/credential sharing and no wildcard origin.
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;

    fn router(app: Arc<AppService>) -> Router {
        rest_router(app.clone()).layer(middleware::from_fn_with_state(
            Auth {
                app,
                address: "127.0.0.1:27980".parse().unwrap(),
            },
            authenticate,
        ))
    }
    fn request(method: &str, path: &str) -> axum::http::request::Builder {
        Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:27980")
            .header("origin", "null")
    }

    #[tokio::test]
    async fn standalone_preflight_is_rest_only_and_does_not_authorize_a_control_request() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        let router = router(app.clone());
        let preflight = request("OPTIONS", "/api/v1/state")
            .header("access-control-request-method", "GET")
            .header(
                "access-control-request-headers",
                "Authorization, Content-Type",
            )
            .header("access-control-request-private-network", "true")
            .body(Body::empty())
            .unwrap();
        let response = router.clone().oneshot(preflight).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(response.headers()["access-control-allow-origin"], "null");
        assert_eq!(
            response.headers()["access-control-allow-private-network"],
            "true"
        );
        assert!(!response
            .headers()
            .contains_key("access-control-allow-credentials"));
        let response = router
            .clone()
            .oneshot(request("GET", "/api/v1/state").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()["access-control-allow-origin"], "null");
        for (path, method, headers) in [
            ("/mcp", "POST", "authorization"),
            ("/api/v1/state", "TRACE", "authorization"),
            ("/api/v1/state", "GET", "authorization, x-arbitrary-header"),
        ] {
            let response = router
                .clone()
                .oneshot(
                    request("OPTIONS", path)
                        .header("access-control-request-method", method)
                        .header("access-control-request-headers", headers)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert!(!response
                .headers()
                .contains_key("access-control-allow-methods"));
        }
        assert!(app.active.state().uid.is_none());
    }

    #[tokio::test]
    async fn standalone_json_binary_and_revocation_keep_bearer_and_host_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        app.remote.write().rest = Some("127.0.0.1:27980".parse().unwrap());
        let key = app.keys.create("standalone fixture").unwrap();
        let auth = format!("Bearer {}", key.token.expose());
        let router = router(app.clone());
        let response = router
            .clone()
            .oneshot(
                request("GET", "/api/v1/state")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["access-control-allow-origin"], "null");
        assert!(response.headers()["access-control-expose-headers"]
            .to_str()
            .unwrap()
            .contains("x-minikmb-width"));
        let response = router
            .clone()
            .oneshot(
                request("POST", "/api/v1/firmware/upload-prepare")
                    .header("authorization", &auth)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name":"fixture.bin","size":32,"target":"ms2130"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap();
        let path = format!(
            "/api/v1/firmware/uploads/{}",
            body["upload_id"].as_str().unwrap()
        );
        let response = router
            .clone()
            .oneshot(
                request("POST", &path)
                    .header("authorization", &auth)
                    .header("content-type", "application/octet-stream")
                    .header("content-length", "32")
                    .body(Body::from(vec![0u8; 32]))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["access-control-allow-origin"], "null");
        for (name, value) in [
            ("host", "other.invalid"),
            ("origin", "https://other.invalid"),
        ] {
            let mut invalid = request("GET", "/api/v1/state")
                .header("authorization", &auth)
                .body(Body::empty())
                .unwrap();
            invalid.headers_mut().insert(
                axum::http::HeaderName::from_static(name),
                value.parse().unwrap(),
            );
            let response = router.clone().oneshot(invalid).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert!(!response
                .headers()
                .contains_key("access-control-allow-origin"));
        }
        app.keys.revoke(&key.info.id).unwrap();
        let response = router
            .oneshot(
                request("GET", "/api/v1/state")
                    .header("authorization", &auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(app.active.state().uid.is_none());
    }
}
