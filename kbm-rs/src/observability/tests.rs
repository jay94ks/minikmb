use super::*;
use tokio_util::sync::CancellationToken;
#[tokio::test]
async fn folder_open_accepts_only_fixed_app_directory_and_never_returns_host_paths() {
    #[derive(Default)]
    struct Open(parking_lot::Mutex<Vec<std::path::PathBuf>>);
    impl crate::logging::DirectoryOpener for Open {
        fn open(
            &self,
            path: &std::path::Path,
            guard: &dyn Fn() -> Result<(), AppError>,
        ) -> Result<(), AppError> {
            guard()?;
            self.0.lock().push(path.to_owned());
            Ok(())
        }
    }
    let (dir, mut app, _wire) = crate::input_api::tests::setup();
    let opened = Arc::new(Open::default());
    Arc::get_mut(&mut app).unwrap().log_opener = opened.clone();
    let user = owner();
    app.active.begin().unwrap();
    app.active.finish(None);
    assert!(app
        .execute_remote(&user, "logs_open_folder", json!({"path":"elsewhere"}))
        .await
        .is_err());
    let result = app
        .execute_remote(&user, "logs_open_folder", json!({}))
        .await
        .unwrap();
    assert_eq!(result, json!({"requested":true,"confirmed":false}));
    assert_eq!(&*opened.0.lock(), &[dir.path().join("logs")]);
    user.cancelled.cancel();
    assert!(app
        .execute_remote(&user, "logs_open_folder", json!({}))
        .await
        .is_err());
    assert_eq!(opened.0.lock().len(), 1);
    app.shutdown().await;
}
#[tokio::test]
async fn shared_input_and_finished_storage_jobs_emit_only_metadata() {
    let (_dir, mut app, _wire) = crate::input_api::tests::setup();
    crate::storage::tests::install(&mut app);
    app.logs.configure(crate::logging::Config {
        enabled: true,
        verbose: true,
    });
    let user = owner();
    app.execute_remote(&user, "hid_chord", json!({"usages":[224,4],"hold_ms":1}))
        .await
        .unwrap();
    let accepted = app
        .execute_remote(
            &user,
            "storage_action",
            json!({"action":"name_set","name":"private channel name"}),
        )
        .await
        .unwrap();
    crate::storage::tests::wait(&app, &user, accepted["job_id"].as_str().unwrap()).await;
    let status = app.logs.status();
    assert!(status.recent.iter().any(|entry| matches!(
        entry.event,
        crate::logging::Event::StorageJobChanged
    ) && entry.fields.command == Some("name_set")));
    assert!(status.recent.iter().any(
        |entry| entry.fields.command == Some("hid_chord") && entry.fields.success == Some(true)
    ));
    let text = serde_json::to_string(&status).unwrap();
    assert!(!text.contains("private channel name"));
    assert!(!text.contains("usages"));
    assert!(!text.contains("payload"));
    app.shutdown().await;
}
fn owner() -> Principal {
    Principal {
        key_id: "mock".into(),
        client_id: "mock".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    }
}
#[tokio::test]
async fn log_controls_work_without_active_but_diagnostics_remains_channel_bound() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    let user = owner();
    let before = wire.calls.lock().len();
    let d = app
        .execute_remote(&user, "diagnostics_get", json!({}))
        .await
        .unwrap();
    assert_eq!(d["mcu_fresh"], false);
    assert!(d["transport"].is_null());
    assert_eq!(wire.calls.lock().len(), before);
    app.active.begin().unwrap();
    app.active.finish(None);
    assert_eq!(
        app.execute_remote(&user, "diagnostics_get", json!({}))
            .await
            .unwrap_err()
            .code,
        "NO_ACTIVE_CHANNEL"
    );
    let value = app
        .execute_remote(
            &user,
            "logs_configure",
            json!({"enabled":true,"verbose":true}),
        )
        .await
        .unwrap();
    assert_eq!(value["enabled"], true);
    assert_eq!(value["verbose"], true);
    assert!(value.get("directory").is_none());
    assert!(app
        .execute_remote(&user, "logs_flush", json!({}))
        .await
        .is_ok());
    for name in ["logs_get_state", "diagnostics_get", "logs_flush"] {
        assert_eq!(
            app.execute_remote(&user, name, json!({"uid":"other"}))
                .await
                .unwrap_err()
                .code,
            "INVALID_ARGUMENTS"
        );
    }
    user.cancelled.cancel();
    assert_eq!(
        app.execute_remote(&user, "logs_get_state", json!({}))
            .await
            .unwrap_err()
            .code,
        "UNAUTHORIZED"
    );
    app.shutdown().await;
}
#[tokio::test]
async fn common_command_logging_never_records_names_paths_or_user_bytes() {
    let (dir, app, _wire) = crate::input_api::tests::setup();
    let user = owner();
    let secret = "never-log-this-api-key-or-filename";
    let result = app
        .execute_remote(
            &user,
            "wake_configure",
            json!({"enabled":false,"mac":"","local_address":"","filename":secret}),
        )
        .await;
    assert!(result.is_err());
    app.logs.flush().unwrap();
    let data = std::fs::read_to_string(dir.path().join("logs/operations.jsonl")).unwrap();
    assert!(data.contains("wake_configure"));
    assert!(data.contains("INVALID_ARGUMENTS"));
    assert!(!data.contains(secret));
    assert!(!data.contains("filename"));
    assert!(!data.contains("directory"));
    app.shutdown().await;
}
