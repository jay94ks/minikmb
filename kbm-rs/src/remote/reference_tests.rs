use super::*;
use tower::ServiceExt;
// The generated OpenAPI resource is JSON encoded inside JSON-RPC text content.
// This bounds the fixture reader, not the product's request/body limits.
const LIMIT: usize = 8 * 1024 * 1024;
#[tokio::test]
async fn authenticated_sdk_resources_and_output_contracts_match_the_current_app() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    app.remote.write().rest = Some("127.0.0.1:22980".parse().unwrap());
    let key = app.keys.create("reference-test").unwrap();
    let stop = CancellationToken::new();
    let router = mcp_router(app.clone(), stop.clone()).layer(middleware::from_fn_with_state(
        Auth {
            app: app.clone(),
            address: "127.0.0.1:22981".parse().unwrap(),
        },
        authenticate,
    ));
    let req = |body: Value, sid: Option<&str>| {
        let mut r = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "127.0.0.1:22981")
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
    let response=router.clone().oneshot(req(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"reference-fixture","version":"1"}}}),None)).await.unwrap();
    let sid = response.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned();
    let info = tests::mcp_payload(
        &axum::body::to_bytes(response.into_body(), LIMIT)
            .await
            .unwrap(),
    );
    assert!(info["result"]["capabilities"]["resources"].is_object());
    assert_eq!(
        info["result"]["capabilities"]["resources"]["subscribe"],
        true
    );
    router
        .clone()
        .oneshot(req(
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            Some(&sid),
        ))
        .await
        .unwrap();
    for name in ["firmware_images", "firmware_get_status"] {
        let response=router.clone().oneshot(req(json!({"jsonrpc":"2.0","id":20,"method":"tools/call","params":{"name":name,"arguments":{}}}),Some(&sid))).await.unwrap();
        let value = tests::mcp_payload(
            &axum::body::to_bytes(response.into_body(), LIMIT)
                .await
                .unwrap(),
        );
        assert_ne!(value["result"]["isError"], true, "{name}: {value}");
        let output = &value["result"]["structuredContent"];
        jsonschema::validator_for(&crate::api_docs::response_schema(name).unwrap())
            .unwrap()
            .validate(output)
            .unwrap();
    }
    let response = router
        .clone()
        .oneshot(req(
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            Some(&sid),
        ))
        .await
        .unwrap();
    let tools = tests::mcp_payload(
        &axum::body::to_bytes(response.into_body(), LIMIT)
            .await
            .unwrap(),
    );
    for tool in tools["result"]["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        assert_eq!(
            tool["outputSchema"],
            crate::api_docs::response_schema(name).unwrap(),
            "{name}"
        );
    }
    let response = router
        .clone()
        .oneshot(req(
            json!({"jsonrpc":"2.0","id":3,"method":"resources/list"}),
            Some(&sid),
        ))
        .await
        .unwrap();
    let listed = tests::mcp_payload(
        &axum::body::to_bytes(response.into_body(), LIMIT)
            .await
            .unwrap(),
    );
    assert_eq!(
        listed["result"]["resources"].as_array().unwrap().len(),
        crate::remote_resources::RESOURCES.len()
    );
    for r in crate::remote_resources::RESOURCES {
        let response = router
            .clone()
            .oneshot(req(
                json!({"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":r.uri}}),
                Some(&sid),
            ))
            .await
            .unwrap();
        let value = tests::mcp_payload(
            &axum::body::to_bytes(response.into_body(), LIMIT)
                .await
                .unwrap(),
        );
        assert!(value["error"].is_null(), "{value}");
        let content = &value["result"]["contents"][0];
        assert_eq!(content["uri"], r.uri);
        assert_eq!(content["mimeType"], r.mime);
        let text = content["text"].as_str().unwrap();
        assert!(!text.contains(key.token.expose()));
        if r.uri == "minikmb://channels" {
            assert_eq!(
                serde_json::from_str::<Value>(text).unwrap(),
                json!(app.inventory.read().clone())
            );
        }
        if r.uri == "minikmb://active/channel" {
            assert_eq!(
                serde_json::from_str::<Value>(text).unwrap()["channel"]["uid"],
                json!(app.active.bind().unwrap().uid)
            );
        }
        if r.uri == "minikmb://reference/openapi" {
            assert_eq!(
                serde_json::from_str::<Value>(text).unwrap()["servers"][0]["url"],
                "http://127.0.0.1:22980"
            );
        }
    }
    app.active.begin().unwrap();
    app.active.finish(None);
    let response=router.clone().oneshot(req(json!({"jsonrpc":"2.0","id":5,"method":"resources/read","params":{"uri":"minikmb://active/channel"}}),Some(&sid))).await.unwrap();
    let value = tests::mcp_payload(
        &axum::body::to_bytes(response.into_body(), LIMIT)
            .await
            .unwrap(),
    );
    assert_eq!(value["error"]["data"]["code"], "NO_ACTIVE_CHANNEL");
    for uri in [
        "minikmb://channels?uid=other",
        "file:///private",
        "minikmb://active/channel/0x0101",
    ] {
        let response = router
            .clone()
            .oneshot(req(
                json!({"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":uri}}),
                Some(&sid),
            ))
            .await
            .unwrap();
        let value = tests::mcp_payload(
            &axum::body::to_bytes(response.into_body(), LIMIT)
                .await
                .unwrap(),
        );
        assert_eq!(value["error"]["code"], -32002);
    }
    assert!(wire.calls.lock().is_empty(), "resource reading emitted HID");
    app.keys.revoke(&key.info.id).unwrap();
    let response = router
        .oneshot(req(
            json!({"jsonrpc":"2.0","id":7,"method":"resources/list"}),
            Some(&sid),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    stop.cancel();
    app.shutdown().await;
}
#[tokio::test]
async fn rest_openapi_serves_the_bound_address_without_device_effects() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    app.remote.write().rest = Some("127.0.0.1:22980".parse().unwrap());
    let key = app.keys.create("reference").unwrap();
    let router = rest_router(app.clone()).layer(middleware::from_fn_with_state(
        Auth {
            app: app.clone(),
            address: "127.0.0.1:22980".parse().unwrap(),
        },
        authenticate,
    ));
    let request = Request::builder()
        .uri("/api/v1/openapi.json")
        .header("host", "127.0.0.1:22980")
        .header("authorization", format!("Bearer {}", key.token.expose()))
        .body(axum::body::Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), LIMIT)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(value, openapi_for(app.remote.read().rest));
    assert!(wire.calls.lock().is_empty());
    app.shutdown().await;
}
