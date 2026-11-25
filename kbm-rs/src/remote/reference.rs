//! Same-origin, self-contained API tester. No app state or credentials in HTML.
use super::*;

fn render(spec: Value) -> String {
    let data = serde_json::to_string(&spec)
        .unwrap()
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    include_str!("../../scripts/api-browser.html").replace("__OPENAPI_JSON__", &data)
}
pub(super) async fn page() -> Response {
    let mut response = axum::response::Html(render(openapi())).into_response();
    for (name, value) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(name),
            axum::http::HeaderValue::from_static(value),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;
    #[tokio::test]
    async fn tester_is_public_only_after_origin_checks_and_controls_still_require_auth() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        let router = rest_router(app.clone()).layer(middleware::from_fn_with_state(
            Auth {
                app: app.clone(),
                address: "127.0.0.1:27980".parse().unwrap(),
            },
            authenticate,
        ));
        let req = |path: &str, host: &str, origin: Option<&str>| {
            let mut r = Request::builder().uri(path).header("host", host);
            if let Some(origin) = origin {
                r = r.header("origin", origin);
            }
            r.body(Body::empty()).unwrap()
        };
        let response = router
            .clone()
            .oneshot(req("/api/v1/reference", "127.0.0.1:27980", None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        let html = String::from_utf8(
            axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("MiniKMB REST API Console"));
        assert!(!html.contains("__OPENAPI_JSON__"));
        {
            let origin = Some("https://other.invalid");
            assert_eq!(
                router
                    .clone()
                    .oneshot(req("/api/v1/reference", "127.0.0.1:27980", origin))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            router
                .clone()
                .oneshot(req("/api/v1/reference", "other.invalid", None))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            router
                .clone()
                .oneshot(req("/api/v1/state", "127.0.0.1:27980", None))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let key = app.keys.create("tester").unwrap();
        let authorized = Request::builder()
            .uri("/api/v1/state")
            .header("host", "127.0.0.1:27980")
            .header("authorization", format!("Bearer {}", key.token.expose()))
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.oneshot(authorized).await.unwrap().status(),
            StatusCode::OK
        );
        assert!(app.active.state().uid.is_none());
    }
    #[test]
    fn embedded_json_cannot_end_its_script_data_element() {
        let rendered = render(json!({"description":"</script><script>alert(1)</script>\u{2028}"}));
        assert!(!rendered.contains("<script>alert(1)</script>"));
        assert!(rendered.contains("\\u003c/script\\u003e"));
    }
}
