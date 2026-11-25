#![cfg(windows)]
//! The real console process talks to the real pipe, with no physical devices.
use minikmb_native::{ipc::Server, service::AppService};
use serde_json::{json, Value};
use std::{path::Path, process::Output};

async fn cli(profile: &Path, args: Vec<String>) -> Output {
    let mut args = args;
    args.push("--json".into());
    plain_cli(profile, args).await
}
async fn plain_cli(profile: &Path, args: Vec<String>) -> Output {
    let profile = profile.to_owned();
    tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_minikmb-cli"))
            .arg("--profile")
            .arg(profile)
            .args(args)
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}
fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}
fn success(result: &Output) -> Value {
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn console_covers_catalog_raw_json_binary_upload_and_error_exit() {
    let dir = tempfile::tempdir().unwrap();
    let app = AppService::new(dir.path().into()).unwrap();
    let server = Server::start(app.clone(), dir.path()).await.unwrap();
    let human = plain_cli(dir.path(), args(&["commands"])).await;
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains("NAME"));
    assert!(!human.contains("\"commands\""));
    let prepared_flags = success(
        &cli(
            dir.path(),
            args(&[
                "firmware_upload_prepare",
                "--target",
                "ms2130",
                "--name",
                "flags.bin",
                "--size",
                "32",
            ]),
        )
        .await,
    );
    assert!(prepared_flags["url"].as_str().unwrap().starts_with("ipc:"));
    let help = plain_cli(dir.path(), args(&["schema", "hid_key_down"])).await;
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--usage"));
    let catalog = success(&cli(dir.path(), args(&["commands"])).await);
    assert_eq!(
        catalog["commands"].as_array().unwrap().len(),
        minikmb_native::service::COMMANDS.len()
    );
    success(&cli(dir.path(), args(&["state", "--pretty"])).await);
    success(&cli(dir.path(), args(&["request", "GET", "/api/v1/state"])).await);
    assert_eq!(
        success(&cli(dir.path(), args(&["schema", "hid_key_sequence"])).await)["method"],
        "POST"
    );
    let body = json!({"target":"ms2130","name":"test.bin","size":32}).to_string();
    let prepared = success(
        &cli(
            dir.path(),
            vec!["firmware_upload_prepare".into(), "--json".into(), body],
        )
        .await,
    );
    let file = dir.path().join("input.bin");
    std::fs::write(&file, [0xAB; 32]).unwrap();
    let uploaded = success(
        &cli(
            dir.path(),
            vec![
                "upload".into(),
                prepared["url"].as_str().unwrap().into(),
                file.display().to_string(),
            ],
        )
        .await,
    );
    assert!(uploaded["source_id"].is_string());
    let destination = dir.path().join("existing.png");
    std::fs::write(&destination, b"keep").unwrap();
    let failed = cli(
        dir.path(),
        vec![
            "capture".into(),
            "--output".into(),
            destination.display().to_string(),
        ],
    )
    .await;
    assert_eq!(failed.status.code(), Some(3));
    assert_eq!(std::fs::read(&destination).unwrap(), b"keep");
    let unknown = cli(dir.path(), args(&["unknown_command"])).await;
    assert_eq!(unknown.status.code(), Some(2));
    assert!(app.remote.read().rest.is_none());
    server.shutdown().await;
    let offline = cli(dir.path(), args(&["state"])).await;
    assert_eq!(offline.status.code(), Some(2));
}
