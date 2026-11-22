use super::*;
use http_body_util::BodyExt;
use std::time::Duration;
use tower::ServiceExt;

async fn json_body(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    tests::mcp_payload(&bytes)
}
async fn event(body: &mut axum::body::Body) -> String {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let frame = body.frame().await.unwrap().unwrap();
            if let Ok(bytes) = frame.into_data() {
                let text = String::from_utf8(bytes.to_vec()).unwrap();
                if text.lines().any(|line| {
                    line.strip_prefix("data:")
                        .is_some_and(|s| !s.trim().is_empty())
                }) {
                    return text;
                }
            }
        }
    })
    .await
    .unwrap()
}
fn request(method: &str, path: &str, token: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:24980")
        .header("authorization", format!("Bearer {token}"))
}
#[tokio::test]
async fn rest_sse_auth_snapshot_updates_drop_limits_and_key_revocation() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    let key = app.keys.create("events-fixture").unwrap();
    let token = key.token.expose();
    let life = CancellationToken::new();
    let router = rest_router(app.clone())
        .layer(middleware::from_fn_with_state(
            Auth {
                app: app.clone(),
                address: "127.0.0.1:24980".parse().unwrap(),
            },
            authenticate,
        ))
        .layer(Extension(ServerLifetime(life.clone())));
    for (path, token, status) in [
        ("/api/v1/events", "bad", StatusCode::UNAUTHORIZED),
        ("/api/v1/events?uid=private", token, StatusCode::BAD_REQUEST),
        (
            "/api/v1/events/snapshot?uid=private",
            token,
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                request("GET", path, token)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(app.events.status().subscriptions, 0);
    }
    let response = router
        .clone()
        .oneshot(
            request("GET", "/api/v1/events/snapshot", token)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let snapshot = json_body(response).await;
    let schema = crate::api_docs::response_schema("events_get_snapshot").unwrap();
    assert!(jsonschema::validator_for(&schema)
        .unwrap()
        .is_valid(&snapshot));
    assert_eq!(app.events.status().subscriptions, 0);
    let response = router
        .clone()
        .oneshot(
            request("GET", "/api/v1/events", token)
                .header("last-event-id", "unrelated-old-history")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    assert_eq!(response.headers()["cache-control"], "no-store");
    let mut body = response.into_body();
    let text = event(&mut body).await;
    assert!(text.contains("event: state"));
    assert!(!text.contains("unrelated-old-history"));
    app.inventory.write().channels[0].name = "updated-name".into();
    assert!(event(&mut body).await.contains("updated-name"));
    let mut bodies = vec![];
    for _ in 1..crate::events::MAX_PER_KEY {
        let r = router
            .clone()
            .oneshot(
                request("GET", "/api/v1/events", token)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        bodies.push(r.into_body());
    }
    let r = router
        .clone()
        .oneshot(
            request("GET", "/api/v1/events", token)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(json_body(r).await["code"], "EVENT_SUBSCRIPTION_LIMIT");
    drop(bodies);
    assert_eq!(app.events.status().subscriptions, 1);
    app.keys.revoke(&key.info.id).unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .unwrap()
        .is_none());
    drop(body);
    assert_eq!(app.events.status().subscriptions, 0);
    let other = app.keys.create("other").unwrap();
    let r = router
        .clone()
        .oneshot(
            request("GET", "/api/v1/events", other.token.expose())
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = r.into_body();
    event(&mut body).await;
    life.cancel();
    assert!(tokio::time::timeout(Duration::from_secs(1), body.frame())
        .await
        .unwrap()
        .is_none());
    drop(body);
    assert!(wire.calls.lock().is_empty());
    app.events.shutdown().await;
}

#[tokio::test]
async fn mcp_legacy_subscription_is_session_scoped_idempotent_and_stops_on_revoke() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    let key = app.keys.create("events-mcp").unwrap();
    let stop = CancellationToken::new();
    let router = mcp_router(app.clone(), stop.clone()).layer(middleware::from_fn_with_state(
        Auth {
            app: app.clone(),
            address: "127.0.0.1:24980".parse().unwrap(),
        },
        authenticate,
    ));
    let post = |value: Value, sid: Option<&str>| {
        let mut r = request("POST", "/mcp", key.token.expose())
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(sid) = sid {
            r = r
                .header("mcp-session-id", sid)
                .header("mcp-protocol-version", "2025-11-25");
        }
        r.body(axum::body::Body::from(value.to_string())).unwrap()
    };
    let response=router.clone().oneshot(post(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"event-fixture","version":"1"}}}),None)).await.unwrap();
    let sid = response.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        json_body(response).await["result"]["capabilities"]["resources"]["subscribe"],
        true
    );
    router
        .clone()
        .oneshot(post(
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            Some(&sid),
        ))
        .await
        .unwrap();
    let response = router
        .clone()
        .oneshot(
            request("GET", "/mcp", key.token.expose())
                .header("accept", "text/event-stream")
                .header("mcp-session-id", &sid)
                .header("mcp-protocol-version", "2025-11-25")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body();
    for _ in 0..2 {
        let response=router.clone().oneshot(post(json!({"jsonrpc":"2.0","id":2,"method":"resources/subscribe","params":{"uri":"minikmb://events/snapshot"}}),Some(&sid))).await.unwrap();
        assert!(json_body(response).await["error"].is_null());
        assert_eq!(app.events.status().subscriptions, 1);
    }
    let notification = event(&mut stream).await;
    assert!(
        notification.contains("notifications/resources/updated"),
        "{notification}"
    );
    app.inventory.write().channels[0].connected = false;
    assert!(event(&mut stream)
        .await
        .contains("minikmb://events/snapshot"));
    let response=router.clone().oneshot(post(json!({"jsonrpc":"2.0","id":3,"method":"resources/subscribe","params":{"uri":"file:///private"}}),Some(&sid))).await.unwrap();
    assert_eq!(
        json_body(response).await["error"]["data"]["code"],
        "INVALID_ARGUMENTS"
    );
    let response=router.clone().oneshot(post(json!({"jsonrpc":"2.0","id":4,"method":"resources/unsubscribe","params":{"uri":"minikmb://events/snapshot"}}),Some(&sid))).await.unwrap();
    assert!(json_body(response).await["error"].is_null());
    assert_eq!(app.events.status().subscriptions, 0);
    let response=router.clone().oneshot(post(json!({"jsonrpc":"2.0","id":5,"method":"resources/subscribe","params":{"uri":"minikmb://channels"}}),Some(&sid))).await.unwrap();
    assert!(json_body(response).await["error"].is_null());
    app.keys.revoke(&key.info.id).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while app.events.status().subscriptions != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    drop(stream);
    stop.cancel();
    app.events.shutdown().await;
    assert!(wire.calls.lock().is_empty());
}

#[tokio::test]
async fn mcp_current_listen_filters_exact_uris_and_closes_on_disconnect_or_revoke() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    let key = app.keys.create("events-current").unwrap();
    let stop = CancellationToken::new();
    let router = mcp_router(app.clone(), stop.clone()).layer(middleware::from_fn_with_state(
        Auth {
            app: app.clone(),
            address: "127.0.0.1:24980".parse().unwrap(),
        },
        authenticate,
    ));
    // 2026-07-28 removed initialize/session headers. Negotiation is per-request.
    let post = || {
        request("POST", "/mcp", key.token.expose())
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "subscriptions/listen")
        .body(axum::body::Body::from(json!({"jsonrpc":"2.0","id":2,"method":"subscriptions/listen","params":{
            "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}},
            "notifications":{"resourceSubscriptions":["file:///private","minikmb://events/snapshot","minikmb://events/snapshot"],"toolsListChanged":true}
        }}).to_string())).unwrap()
    };
    for revoked in [false, true] {
        let response = router.clone().oneshot(post()).await.unwrap();
        if response.status() != StatusCode::OK {
            panic!("listen failed: {}", json_body(response).await);
        }
        assert!(response.headers().get("mcp-session-id").is_none());
        let mut body = response.into_body();
        let text = event(&mut body).await;
        assert!(text.contains("subscriptions/acknowledged"), "{text}");
        assert!(!text.contains("file:///private"), "{text}");
        let text = event(&mut body).await;
        assert!(text.contains("notifications/resources/updated"), "{text}");
        assert!(text.contains("minikmb://events/snapshot"));
        assert_eq!(app.events.status().subscriptions, 1);
        if revoked {
            app.keys.revoke(&key.info.id).unwrap();
        }
        drop(body);
        tokio::time::timeout(Duration::from_secs(2), async {
            while app.events.status().subscriptions != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
    stop.cancel();
    app.events.shutdown().await;
    assert!(wire.calls.lock().is_empty());
}
