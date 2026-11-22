use super::*;
use tower::ServiceExt;
async fn body(response: Response) -> Value {
    let id = response.headers()["x-minikmb-request-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let value: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 16384)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value["request_id"], id);
    let schema = crate::api_docs::error_schema();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(
        validator.is_valid(&value),
        "{}",
        validator
            .iter_errors(&value)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    );
    value
}
#[tokio::test]
async fn rest_auth_parser_method_and_body_limit_errors_are_typed_correlated_and_redacted() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    let key = app.keys.create("error-test").unwrap();
    let router = rest_router(app.clone()).layer(middleware::from_fn_with_state(
        Auth {
            app: app.clone(),
            address: "127.0.0.1:23980".parse().unwrap(),
        },
        authenticate,
    ));
    let request = |method: &str, path: &str, data: Vec<u8>, mime: bool, authorized: bool| {
        let mut r = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "127.0.0.1:23980")
            .header("x-minikmb-request-id", "SHOULD_NOT_ECHO");
        if mime {
            r = r.header("content-type", "application/json");
        }
        if authorized {
            r = r.header("authorization", format!("Bearer {}", key.token.expose()));
        }
        r.body(axum::body::Body::from(data)).unwrap()
    };
    let mut seen = std::collections::HashSet::new();
    for (request, status, code) in [
        (
            request("GET", "/api/v1/active", vec![], false, false),
            StatusCode::UNAUTHORIZED,
            "UNAUTHORIZED",
        ),
        (
            request(
                "POST",
                "/api/v1/active/hid/keys/down",
                b"{PRIVATE_SECRET".to_vec(),
                true,
                true,
            ),
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENTS",
        ),
        (
            request(
                "POST",
                "/api/v1/active/hid/keys/down",
                br#"{"usage":"PRIVATE_SECRET"}"#.to_vec(),
                true,
                true,
            ),
            StatusCode::BAD_REQUEST,
            "INVALID_ARGUMENTS",
        ),
        (
            request(
                "POST",
                "/api/v1/active/hid/keys/down",
                b"PRIVATE_SECRET".to_vec(),
                false,
                true,
            ),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "INVALID_CONTENT_TYPE",
        ),
        (
            request(
                "POST",
                "/api/v1/active/hid/keys/down",
                vec![b'x'; 2 * 1024 * 1024 + 1],
                true,
                true,
            ),
            StatusCode::PAYLOAD_TOO_LARGE,
            "PAYLOAD_TOO_LARGE",
        ),
        (
            request("GET", "/api/v1/missing", vec![], false, true),
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
        ),
        (
            request("GET", "/api/v1/active/hid/keys/down", vec![], false, true),
            StatusCode::METHOD_NOT_ALLOWED,
            "METHOD_NOT_ALLOWED",
        ),
    ] {
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), status, "{code}");
        if status == StatusCode::UNAUTHORIZED {
            assert_eq!(
                response.headers()["www-authenticate"],
                "Bearer realm=\"MiniKMB\""
            );
        }
        if status == StatusCode::METHOD_NOT_ALLOWED {
            assert!(response.headers()["allow"]
                .to_str()
                .unwrap()
                .contains("POST"));
        }
        let value = body(response).await;
        assert_eq!(value["code"], code);
        assert!(
            !value.to_string().contains("PRIVATE_SECRET")
                && !value.to_string().contains("SHOULD_NOT_ECHO")
        );
        if status == StatusCode::UNAUTHORIZED {
            assert!(value["channel_uid"].is_null() && value["generation"].is_null());
        }
        assert!(seen.insert(value["request_id"].as_str().unwrap().to_owned()));
    }
    for (name, value, code) in [
        ("host", "PRIVATE_SECRET", "HOST_NOT_ALLOWED"),
        ("origin", "http://PRIVATE_SECRET", "ORIGIN_NOT_ALLOWED"),
    ] {
        let mut req = request("GET", "/api/v1/active", vec![], false, true);
        req.headers_mut()
            .insert(name, axum::http::HeaderValue::from_static(value));
        let response = router.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let value = body(response).await;
        assert_eq!(value["code"], code);
        assert!(value["channel_uid"].is_null());
        assert!(!value.to_string().contains("PRIVATE_SECRET"));
    }
    app.keys.revoke(&key.info.id).unwrap();
    let response = router
        .oneshot(request("GET", "/api/v1/active", vec![], false, true))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(body(response).await["channel_uid"].is_null());
    assert!(wire.calls.lock().is_empty());
    app.shutdown().await;
}
#[tokio::test]
async fn stale_lease_error_keeps_original_channel_and_generation_after_a_switch() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    let original = app.active.bind().unwrap();
    let key = app.keys.create("lease-error").unwrap();
    let router = rest_router(app.clone()).layer(middleware::from_fn_with_state(
        Auth {
            app: app.clone(),
            address: "127.0.0.1:23980".parse().unwrap(),
        },
        authenticate,
    ));
    let post = |path: &str, args: Value| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "127.0.0.1:23980")
            .header("authorization", format!("Bearer {}", key.token.expose()))
            .header("content-type", "application/json")
            .body(axum::body::Body::from(args.to_string()))
            .unwrap()
    };
    let response = router
        .clone()
        .oneshot(post("/api/v1/input-sessions", json!({})))
        .await
        .unwrap();
    let opened: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 16384)
            .await
            .unwrap(),
    )
    .unwrap();
    app.active.begin().unwrap();
    app.active
        .finish(Some(crate::model::Uid::from_bytes(&[2; 12]).unwrap()));
    let response = router
        .oneshot(post(
            "/api/v1/input-sessions/heartbeat",
            json!({"input_session":opened["input_session"]}),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let failure = body(response).await;
    assert_eq!(failure["code"], "STALE_OPERATION");
    assert_eq!(failure["channel_uid"], json!(original.uid));
    assert_eq!(failure["generation"], original.generation);
    assert!(wire.calls.lock().is_empty());
    app.shutdown().await;
}
#[tokio::test]
async fn sdk_tool_and_resource_errors_share_the_http_request_id_and_keep_success_unchanged() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    app.active.begin().unwrap();
    app.active.finish(None);
    let key = app.keys.create("mcp-error").unwrap();
    let stop = CancellationToken::new();
    let router = mcp_router(app.clone(), stop.clone()).layer(middleware::from_fn_with_state(
        Auth {
            app: app.clone(),
            address: "127.0.0.1:23981".parse().unwrap(),
        },
        authenticate,
    ));
    let req = |args: Value, sid: Option<&str>| {
        let mut r = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "127.0.0.1:23981")
            .header("authorization", format!("Bearer {}", key.token.expose()))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(sid) = sid {
            r = r
                .header("mcp-session-id", sid)
                .header("mcp-protocol-version", "2025-11-25");
        }
        r.body(axum::body::Body::from(args.to_string())).unwrap()
    };
    let response=router.clone().oneshot(req(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"error-fixture","version":"1"}}}),None)).await.unwrap();
    let sid = response.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned();
    axum::body::to_bytes(response.into_body(), 65536)
        .await
        .unwrap();
    router
        .clone()
        .oneshot(req(
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            Some(&sid),
        ))
        .await
        .unwrap();
    let mut seen = std::collections::HashSet::new();
    for (method, params, resource) in [
        (
            "tools/call",
            json!({"name":"hid_get_status","arguments":{}}),
            false,
        ),
        (
            "resources/read",
            json!({"uri":"minikmb://active/channel"}),
            true,
        ),
        (
            "resources/read",
            json!({"uri":"file:///PRIVATE_SECRET"}),
            true,
        ),
        ("resources/list", json!({"cursor":"PRIVATE_SECRET"}), true),
    ] {
        let response = router
            .clone()
            .oneshot(req(
                json!({"jsonrpc":"2.0","id":2,"method":method,"params":params}),
                Some(&sid),
            ))
            .await
            .unwrap();
        let id = response.headers()["x-minikmb-request-id"]
            .to_str()
            .unwrap()
            .to_owned();
        let value = tests::mcp_payload(
            &axum::body::to_bytes(response.into_body(), 65536)
                .await
                .unwrap(),
        );
        let failure = if resource {
            &value["error"]["data"]
        } else {
            assert_eq!(value["result"]["isError"], true);
            &value["result"]["structuredContent"]
        };
        assert_eq!(failure["request_id"], id);
        assert!(failure["channel_uid"].is_null());
        assert!(!failure.to_string().contains("PRIVATE_SECRET"));
        let validator = jsonschema::validator_for(&crate::api_docs::error_schema()).unwrap();
        assert!(validator.is_valid(failure));
        assert!(seen.insert(id));
    }
    let response=router.oneshot(req(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"channels_list","arguments":{}}}),Some(&sid))).await.unwrap();
    let value = tests::mcp_payload(
        &axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap(),
    );
    assert_eq!(
        value["result"]["structuredContent"],
        json!(app.inventory.read().clone())
    );
    assert!(wire.calls.lock().is_empty());
    stop.cancel();
    app.shutdown().await;
}
