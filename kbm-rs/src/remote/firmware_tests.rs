use super::*;
use tower::ServiceExt;
fn request(path: &str, token: &str, args: Value) -> Request<axum::body::Body> {
    Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "127.0.0.1:27980")
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(args.to_string()))
        .unwrap()
}
async fn value(router: &Router, path: &str, token: &str, args: Value) -> (StatusCode, Value) {
    let r = router
        .clone()
        .oneshot(request(path, token, args))
        .await
        .unwrap();
    let status = r.status();
    (
        status,
        serde_json::from_slice(
            &axum::body::to_bytes(r.into_body(), 4 * 1024 * 1024)
                .await
                .unwrap(),
        )
        .unwrap(),
    )
}
#[tokio::test]
async fn firmware_rest_upload_and_job_share_the_pinned_service_and_never_write_on_receipt() {
    let (_dir, app, fake) = crate::firmware::manager::tests::setup(64, None);
    app.remote.write().rest = Some("127.0.0.1:27980".parse().unwrap());
    app.select(None).await.unwrap();
    app.inventory.write().channels.clear();
    let key = app.keys.create("A").unwrap();
    let other = app.keys.create("B").unwrap();
    let router = rest_router(app.clone()).layer(middleware::from_fn_with_state(
        Auth {
            app: app.clone(),
            address: "127.0.0.1:27980".parse().unwrap(),
        },
        authenticate,
    ));
    let ticket = value(
        &router,
        "/api/v1/firmware/upload-prepare",
        key.token.expose(),
        json!({"target":"ms2130","name":"rom.bin","size":32}),
    )
    .await;
    assert_eq!(ticket.0, StatusCode::OK);
    let url = ticket.1["url"].as_str().unwrap();
    let path = url.strip_prefix("http://127.0.0.1:27980").unwrap();
    let upload = |token: &str| {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "127.0.0.1:27980")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/octet-stream")
            .header("content-length", "32")
            .body(axum::body::Body::from(vec![0x55; 32]))
            .unwrap()
    };
    assert_eq!(
        router
            .clone()
            .oneshot(upload(other.token.expose()))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let response = router
        .clone()
        .oneshot(upload(key.token.expose()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let source: Value = serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(fake.writes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_ne!(
        router
            .clone()
            .oneshot(upload(key.token.expose()))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let mut args = json!({"target":"ms2130","programmer_id":"test","source_id":source["source_id"],"confirmed":false});
    assert_eq!(
        value(
            &router,
            "/api/v1/firmware/start",
            key.token.expose(),
            args.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    args["confirmed"] = json!(true);
    let (started, job) = value(&router, "/api/v1/firmware/start", key.token.expose(), args).await;
    assert_eq!(started, StatusCode::OK);
    let id = job["job_id"].as_str().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), fake.entered.notified())
        .await
        .unwrap();
    assert!(app.active.state().uid.is_none());
    assert_eq!(
        value(
            &router,
            "/api/v1/firmware/job",
            other.token.expose(),
            json!({"job_id":id})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        value(
            &router,
            "/api/v1/firmware/cancel",
            key.token.expose(),
            json!({"job_id":id})
        )
        .await
        .1["can_cancel"],
        false
    );
    fake.finish.notify_one();
    let completed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let (status, v) = value(
                &router,
                "/api/v1/firmware/job",
                key.token.expose(),
                json!({"job_id":id}),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            if v["state"] == "completed" {
                break v;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(completed["uid"].is_null());
    assert!(completed["generation"].is_null());
    assert_eq!(completed["result"]["verified"], true);
    assert_eq!(fake.writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    jsonschema::validator_for(&crate::api_docs::response_schema("firmware_job_get").unwrap())
        .unwrap()
        .validate(&completed)
        .unwrap();
}
