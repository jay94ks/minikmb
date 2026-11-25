use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::time::Duration;

async fn send(
    server: &Server,
    method: &str,
    path: &str,
    value: Option<Value>,
) -> axum::http::Response<hyper::body::Incoming> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "minikmb.local");
    let body = if let Some(value) = value {
        builder = builder.header("content-type", "application/json");
        Body::from(value.to_string())
    } else {
        Body::empty()
    };
    client::request(
        &server.endpoint,
        builder.body(body).unwrap(),
        Duration::from_secs(2),
    )
    .await
    .unwrap()
}
async fn value(
    server: &Server,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let response = send(server, method, path, body).await;
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap())
}
#[tokio::test]
async fn actual_pipe_shares_catalog_and_rest_without_enabling_tcp() {
    let dir = tempfile::tempdir().unwrap();
    let app = crate::service::AppService::new(dir.path().into()).unwrap();
    let server = Server::start(app.clone(), dir.path()).await.unwrap();
    assert_eq!(endpoint(dir.path()).unwrap(), server.endpoint);
    assert!(Server::start(app.clone(), dir.path()).await.is_err());
    let (status, catalog) = value(&server, "GET", "/ipc/v1/commands", None).await;
    assert_eq!(status, StatusCode::OK);
    let commands = catalog["commands"].as_array().unwrap();
    assert_eq!(commands.len(), crate::service::COMMANDS.len());
    for doc in crate::service::COMMANDS {
        let found = commands.iter().find(|c| c["name"] == doc.name).unwrap();
        assert_eq!(found["path"], doc.path);
        assert!(found["input_schema"].is_object());
    }
    assert_eq!(
        value(&server, "GET", "/api/v1/state", None).await.0,
        StatusCode::OK
    );
    assert_eq!(
        value(&server, "GET", "/api/v1/openapi.json", None).await.0,
        StatusCode::OK
    );
    let error = value(&server, "GET", "/api/v1/active/hid", None).await;
    assert!(!error.0.is_success());
    assert!(
        error.1.to_string().contains("NO_ACTIVE_CHANNEL"),
        "{}",
        error.1
    );
    assert!(app.remote.read().rest.is_none());
    let mut events = send(&server, "GET", "/api/v1/events", None)
        .await
        .into_body();
    let first = tokio::time::timeout(Duration::from_secs(2), events.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(first.data_ref().unwrap()).contains("data:"));
    drop(events);
    server.shutdown().await;
    let server = Server::start(app, dir.path()).await.unwrap();
    server.shutdown().await;
}
#[tokio::test]
async fn binary_upload_crosses_separate_connections_without_tcp_or_programming() {
    let dir = tempfile::tempdir().unwrap();
    let app = crate::service::AppService::new(dir.path().into()).unwrap();
    let server = Server::start(app.clone(), dir.path()).await.unwrap();
    let ticket = value(
        &server,
        "POST",
        "/api/v1/firmware/upload-prepare",
        Some(json!({"target":"ms2130","name":"test.bin","size":32})),
    )
    .await;
    assert_eq!(ticket.0, StatusCode::OK, "{}", ticket.1);
    let path = ticket.1["url"]
        .as_str()
        .unwrap()
        .strip_prefix("ipc:")
        .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "minikmb.local")
        .header("content-type", "application/octet-stream")
        .header("content-length", "32")
        .body(Body::from(vec![0x55; 32]))
        .unwrap();
    let result = client::request(&server.endpoint, request, Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let source: Value =
        serde_json::from_slice(&result.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(source["source_id"].is_string());
    assert!(app.remote.read().rest.is_none());
    assert!(app.active.state().uid.is_none());
    server.shutdown().await;
}

#[tokio::test]
async fn storage_upload_uses_ipc_ticket_and_the_existing_job_pipeline() {
    let (dir, mut app, _) = crate::input_api::tests::setup();
    let _storage = crate::storage::tests::install(&mut app);
    let server = Server::start(app.clone(), dir.path()).await.unwrap();
    let image = crate::storage::tests::import_cases::iso();
    let prepared = value(
        &server,
        "POST",
        "/api/v1/active/storage/uploads/prepare",
        Some(json!({"kind":"optical","name":"boot.iso","size":image.len(),"confirmed":true})),
    )
    .await;
    assert_eq!(prepared.0, StatusCode::OK, "{}", prepared.1);
    let path = prepared.1["url"]
        .as_str()
        .unwrap()
        .strip_prefix("ipc:")
        .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "minikmb.local")
        .header("content-type", "application/octet-stream")
        .header("content-length", image.len())
        .body(Body::from(image))
        .unwrap();
    let uploaded = client::request(&server.endpoint, request, Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(uploaded.status(), StatusCode::OK);
    let accepted: Value =
        serde_json::from_slice(&uploaded.into_body().collect().await.unwrap().to_bytes()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let job = value(
                &server,
                "POST",
                "/api/v1/storage/jobs/get",
                Some(json!({"job_id":accepted["job_id"]})),
            )
            .await;
            assert_eq!(job.0, StatusCode::OK, "{}", job.1);
            if job.1["state"] == "completed" {
                break;
            }
            assert_ne!(job.1["state"], "failed", "{}", job.1);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(app.remote.read().rest.is_none());
    server.shutdown().await;
}
#[test]
fn endpoint_is_scoped_by_profile_and_cannot_address_remote_machine() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    assert_ne!(endpoint(a.path()).unwrap(), endpoint(b.path()).unwrap());
    assert!(validate_endpoint(r"\\remote\pipe\MiniKMB.Native.test").is_err());
    assert!(validate_endpoint(r"\\.\pipe\MiniKMB.Native.").is_err());
}

#[tokio::test]
async fn input_session_survives_cli_connections_and_rest_off_but_not_channel_switch() {
    let (dir, app, wire) = crate::input_api::tests::setup();
    let server = Server::start(app.clone(), dir.path()).await.unwrap();
    let opened = value(&server, "POST", "/api/v1/input-sessions", Some(json!({}))).await;
    assert_eq!(opened.0, StatusCode::OK, "{}", opened.1);
    let id = opened.1["input_session"].as_str().unwrap();
    assert_eq!(
        value(
            &server,
            "POST",
            "/api/v1/active/hid/keys/down",
            Some(json!({"usage":4,"input_session":id}))
        )
        .await
        .0,
        StatusCode::OK
    );
    app.close_remote_inputs().await;
    assert_eq!(
        value(
            &server,
            "POST",
            "/api/v1/active/hid/keys/up",
            Some(json!({"usage":4,"input_session":id}))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(!wire.calls.lock().is_empty());
    app.test_video_pixels();
    let frame = send(
        &server,
        "GET",
        "/api/v1/active/video/snapshot?format=png",
        None,
    )
    .await;
    assert_eq!(frame.status(), StatusCode::OK);
    assert_eq!(frame.headers()["content-type"], "image/png");
    let image =
        image::load_from_memory(&frame.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!((image.width(), image.height()), (1920, 1080));
    let uid = crate::model::Uid::from_bytes(&[2; 12]).unwrap();
    let selected = value(
        &server,
        "PUT",
        "/api/v1/active-channel",
        Some(json!({"uid":uid.display()})),
    )
    .await;
    assert_eq!(selected.0, StatusCode::OK, "{}", selected.1);
    assert_eq!(app.active.state().uid, Some(uid));
    assert!(!value(
        &server,
        "POST",
        "/api/v1/active/hid/keys/up",
        Some(json!({"usage":4,"input_session":id}))
    )
    .await
    .0
    .is_success());
    server.shutdown().await;
}
