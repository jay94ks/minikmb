use super::*;
fn uid(n: u8) -> Uid {
    Uid::from_bytes(&[n; 12]).unwrap()
}
#[test]
fn card_order_persists_hidden_channels_and_preserves_other_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let file = path(&dir);
    std::fs::write(&file, br#"{"version":1,"revision":0,"channels":[]}"#).unwrap();
    let store = Store::load(file.clone());
    assert!(store.status().blocked.is_none());
    store.network(&uid(1), Config::default()).unwrap();
    store
        .move_channel(&uid(3), &uid(1), false, &[uid(1), uid(2), uid(3)])
        .unwrap();
    assert_eq!(store.snapshot().channel_order, vec![uid(3), uid(1), uid(2)]);
    store
        .move_channel(&uid(1), &uid(3), false, &[uid(3), uid(1)])
        .unwrap();
    let reopened = Store::load(file);
    assert_eq!(
        reopened.snapshot().channel_order,
        vec![uid(1), uid(3), uid(2)]
    );
    assert_eq!(reopened.channel(&uid(1)).config(), Config::default());
    reopened.video(&uid(2), CaptureMode::default()).unwrap();
    assert_eq!(
        reopened.snapshot().channel_order,
        vec![uid(1), uid(3), uid(2)]
    );
    reopened
        .move_channel(&uid(4), &uid(2), true, &[uid(2), uid(4), uid(3), uid(1)])
        .unwrap();
    assert_eq!(
        reopened.snapshot().channel_order,
        vec![uid(1), uid(3), uid(2), uid(4)]
    );
}
#[test]
fn stale_drag_or_external_write_never_overwrites_order_or_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let file = path(&dir);
    let a = Store::load(file.clone());
    let b = Store::load(file.clone());
    a.move_channel(&uid(2), &uid(1), false, &[uid(1), uid(2)])
        .unwrap();
    let saved = std::fs::read(&file).unwrap();
    assert!(a
        .move_channel(&uid(3), &uid(1), false, &[uid(1), uid(2)])
        .is_err());
    assert!(a
        .move_channel(&uid(2), &uid(1), false, &[uid(1), uid(1), uid(2)])
        .is_err());
    assert!(b
        .move_channel(&uid(1), &uid(2), false, &[uid(1), uid(2)])
        .is_err());
    assert_eq!(std::fs::read(&file).unwrap(), saved);
}
#[tokio::test]
async fn displayed_and_served_order_match_without_selecting_or_renumbering_channels() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    let active = app.active.state();
    app.preferences
        .move_channel(&uid(2), &uid(1), false, &[uid(1), uid(2)])
        .unwrap();
    let snapshot = app.snapshot();
    assert_eq!(
        snapshot
            .inventory
            .channels
            .iter()
            .map(|c| c.uid.clone())
            .collect::<Vec<_>>(),
        vec![uid(2), uid(1)]
    );
    assert_eq!(snapshot.inventory.channels[0].slot, Some(1));
    assert_eq!(app.active.state(), active);
    let served = app
        .execute("channels_list", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(served["channels"][0]["uid"], serde_json::json!(uid(2)));
    assert_eq!(app.inventory.read().channels[0].uid, uid(1));
    assert!(wire.calls.lock().is_empty());
}
fn path(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("preferences.json")
}
#[test]
fn roundtrip_is_atomic_bounded_and_does_not_store_runtime_activation_flags() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::load(path(&dir));
    assert!(!path(&dir).exists());
    let config = Config {
        server_ip: "10.20.30.1".into(),
        ..Default::default()
    };
    store.network(&uid(1), config.clone()).unwrap();
    store
        .video(
            &uid(2),
            CaptureMode {
                width: 1920,
                height: 1080,
                fps: 59.94,
            },
        )
        .unwrap();
    let reopened = Store::load(path(&dir));
    assert_eq!(reopened.channel(&uid(1)).config(), config);
    assert_eq!(reopened.channel(&uid(2)).video.unwrap().fps, 59.94);
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path(&dir)).unwrap()).unwrap();
    assert_eq!(value["channels"].as_array().unwrap().len(), 2);
    for name in [
        "active_uid",
        "network_enabled",
        "rest_enabled",
        "mcp_enabled",
        "special_keys",
    ] {
        assert!(value.get(name).is_none());
    }
    let before = std::fs::read(path(&dir)).unwrap();
    assert!(store
        .network(
            &uid(1),
            Config {
                server_ip: "8.8.8.1".into(),
                ..config
            }
        )
        .is_err());
    assert_eq!(std::fs::read(path(&dir)).unwrap(), before);
}
#[test]
fn corruption_and_future_schema_are_preserved_until_confirmed_recovery() {
    for bytes in [
        b"broken json".as_slice(),
        br#"{"version":2,"revision":0,"channels":[]}"#.as_slice(),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(path(&dir), bytes).unwrap();
        let store = Store::load(path(&dir));
        assert!(store.status().blocked.is_some());
        assert!(store.network(&uid(1), Config::default()).is_err());
        assert_eq!(std::fs::read(path(&dir)).unwrap(), bytes);
        assert!(store.recover_invalid(false).is_err());
        store.recover_invalid(true).unwrap();
        assert!(store.status().blocked.is_none());
        let backup = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(".invalid-")
            })
            .unwrap();
        assert_eq!(std::fs::read(backup).unwrap(), bytes);
        assert!(Store::load(path(&dir)).status().blocked.is_none());
    }
}
#[test]
fn stale_instances_and_process_lock_never_overwrite_another_commit() {
    let dir = tempfile::tempdir().unwrap();
    let a = Store::load(path(&dir));
    let b = Store::load(path(&dir));
    a.network(&uid(1), Config::default()).unwrap();
    assert_eq!(
        b.network(&uid(2), Config::default()).unwrap_err().code,
        "PREFERENCES_CHANGED"
    );
    b.reload().unwrap();
    b.network(&uid(2), Config::default()).unwrap();
    assert_eq!(Store::load(path(&dir)).status().channels, 2);
    let lock = a.lock_file().unwrap();
    b.reload().unwrap();
    assert_eq!(
        b.network(&uid(3), Config::default()).unwrap_err().code,
        "PREFERENCES_BUSY"
    );
    drop(lock);
    assert_eq!(Store::load(path(&dir)).status().channels, 2);
}
#[test]
fn size_profile_and_duplicate_uid_limits_fail_without_truncating_existing_data() {
    let store = Store::memory();
    for n in 0..64 {
        store.network(&uid(n), Config::default()).unwrap();
    }
    assert!(store.network(&uid(64), Config::default()).is_err());
    assert_eq!(store.status().error.unwrap().code, "PREFERENCES_LIMIT");
    assert_eq!(store.status().channels, 64);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(path(&dir), vec![b' '; MAX_BYTES as usize + 1]).unwrap();
    assert!(Store::load(path(&dir)).status().blocked.is_some());
    let duplicate = Document {
        channels: vec![
            Row {
                uid: uid(1),
                preferences: Default::default(),
            },
            Row {
                uid: uid(1),
                preferences: Default::default(),
            },
        ],
        ..Default::default()
    };
    std::fs::write(path(&dir), serde_json::to_vec(&duplicate).unwrap()).unwrap();
    assert!(Store::load(path(&dir)).status().blocked.is_some());
}

#[test]
fn reload_corruption_preserves_live_values_and_enables_explicit_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::load(path(&dir));
    let config = Config {
        server_ip: "10.20.30.1".into(),
        ..Default::default()
    };
    store.network(&uid(1), config.clone()).unwrap();
    std::fs::write(path(&dir), b"damaged after startup").unwrap();
    assert!(store.reload().is_err());
    assert!(store.status().blocked.is_some());
    assert_eq!(store.channel(&uid(1)).config(), config);
    assert!(store.video(&uid(1), CaptureMode::default()).is_err());
    assert_eq!(std::fs::read(path(&dir)).unwrap(), b"damaged after startup");
    // An external repair wins over a previously offered reset.
    std::fs::write(path(&dir), serde_json::to_vec(&store.snapshot()).unwrap()).unwrap();
    assert_eq!(
        store.recover_invalid(true).unwrap_err().code,
        "PREFERENCES_CHANGED"
    );
    store.reload().unwrap();
    assert!(store.status().blocked.is_none());
    assert_eq!(store.channel(&uid(1)).config(), config);
}

#[test]
fn import_rejects_duplicate_keys_aliases_and_unknown_runtime_fields() {
    let store = Store::memory();
    let key = format!("minikmb.video.{}", uid(1).raw());
    let raw = format!(
        r#"{{"format":"minikmb-electron-settings/v1","entries":{{"{key}":"\"720\"","{key}":"\"1080\""}}}}"#
    );
    assert!(store.prepare_import(raw.as_bytes()).is_err());
    let alias = format!("minikmb.video.{}", uid(1).display().to_lowercase());
    let bytes = export(serde_json::json!({key:"\"720\"",alias:"\"1080\""}));
    assert_eq!(
        store.prepare_import(&bytes).err().unwrap().code,
        "IMPORT_AMBIGUOUS"
    );
    let dir = tempfile::tempdir().unwrap();
    let mut doc = serde_json::to_value(Document::default()).unwrap();
    doc["network_enabled"] = true.into();
    let bytes = serde_json::to_vec(&doc).unwrap();
    std::fs::write(path(&dir), &bytes).unwrap();
    let loaded = Store::load(path(&dir));
    assert!(loaded.status().blocked.is_some());
    assert!(loaded.network(&uid(1), Config::default()).is_err());
    assert_eq!(std::fs::read(path(&dir)).unwrap(), bytes);
    assert_eq!(store.status().channels, 0);
}

#[tokio::test]
async fn capture_monitor_saves_only_after_first_frame_and_never_activates_on_restart() {
    let (dir, app, _wire) = crate::input_api::tests::setup();
    let binding = app.active.bind().unwrap();
    app.close_pinned_video(binding.clone()).await.unwrap();
    let monitor = app.start_preference_monitor();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(app.preferences.status().channels, 0);
    app.test_video_geometry(1920, 1080);
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while app.preferences.channel(&binding.uid).video.is_none() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let revision = app.preferences.status().revision;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(app.preferences.status().revision, revision);
    app.stop();
    monitor.join().unwrap();
    let restarted = crate::service::AppService::new(dir.path().to_owned()).unwrap();
    assert!(restarted.active.bind().is_err());
    assert!(!restarted.snapshot().video.running);
    assert!(restarted.network.resource().is_none());
    assert_eq!(
        restarted.preferences.channel(&binding.uid).video,
        Some(CaptureMode::default())
    );
    app.shutdown().await;
    restarted.shutdown().await;
}
fn export(entries: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(
        &serde_json::json!({"format":"minikmb-electron-settings/v1","entries":entries}),
    )
    .unwrap()
}
#[test]
fn legacy_conversion_preserves_native_fields_and_never_imports_credentials_or_activation() {
    let store = Store::memory();
    store
        .network(
            &uid(1),
            Config {
                server_ip: "10.20.30.1".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let old=serde_json::json!({"config":{"serverIp":"192.168.127.1","poolStart":100,"poolEnd":199,"leaseSeconds":3600},"rules":[{"id":"ssh","port":0,"protocol":"tcp","listenPort":18022,"targetIp":"192.168.127.100","targetPort":22}],"wol":{"enabled":true,"mac":"AA-BB-CC-DD-EE-F0","localAddress":"192.168.1.4"}}).to_string();
    let bytes = export(
        serde_json::json!({format!("minikmb.settings.{}",uid(1).raw()):old,format!("minikmb.video.{}",uid(1).raw()):"\"1080\"","api-keys.json":"super-secret","activeUid":"other"}),
    );
    let plan = store.prepare_import(&bytes).unwrap();
    assert_eq!(plan.report.preserved_fields, 1);
    assert_eq!(plan.report.imported_fields, 3);
    assert_eq!(plan.report.ignored_keys, 2);
    store.apply_import(plan).unwrap();
    let channel = store.channel(&uid(1));
    assert_eq!(channel.config().server_ip, "10.20.30.1");
    assert_eq!(channel.video.unwrap().width, 1920);
    assert_eq!(channel.wol.as_ref().unwrap().mac, "aa:bb:cc:dd:ee:f0");
    assert_eq!(channel.rules()[0].listen_port, 18022);
    assert!(!serde_json::to_string(&store.snapshot())
        .unwrap()
        .contains("super-secret"));
    let repeat = store.prepare_import(&bytes).unwrap();
    assert_eq!(repeat.report.imported_fields, 0);
}
#[test]
fn import_preview_is_revision_pinned_and_reports_invalid_legacy_routes() {
    let store = Store::memory();
    let bytes = export(
        serde_json::json!({format!("minikmb.video.{}",uid(1).raw()):"{\"width\":1280,\"height\":720,\"frameRate\":59.94}"}),
    );
    let plan = store.prepare_import(&bytes).unwrap();
    store.network(&uid(2), Config::default()).unwrap();
    assert_eq!(
        store.apply_import(plan).unwrap_err().code,
        "PREFERENCES_CHANGED"
    );
    assert!(store.channel(&uid(1)).video.is_none());
    let old=serde_json::json!({"rules":[{"id":"wrong-link","port":1,"protocol":"tcp","listenPort":18022,"targetIp":"192.168.127.100","targetPort":22}]}).to_string();
    let bytes = export(serde_json::json!({format!("minikmb.settings.{}",uid(1).raw()):old}));
    let plan = store.prepare_import(&bytes).unwrap();
    assert_eq!(plan.report.rejected[0].code, "LEGACY_ROUTE_UNSUPPORTED");
    assert_eq!(plan.report.imported_fields, 0);
}
