use super::*;
use serde_json::{json, Value};
fn principal(key: &str) -> Principal {
    Principal {
        key_id: key.into(),
        client_id: "fixture".into(),
        cancelled: Default::default(),
        ended: Default::default(),
    }
}
async fn next(sub: &mut Subscription) -> Arc<Frame> {
    tokio::time::timeout(Duration::from_secs(2), sub.next())
        .await
        .unwrap()
        .unwrap()
}
#[tokio::test]
async fn cached_metadata_matches_ui_and_never_starts_device_features() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    let mut sub = app.events.subscribe(&app, &principal("A")).unwrap();
    let frame = next(&mut sub).await;
    let data: Value = serde_json::from_str(&frame.json).unwrap();
    assert_eq!(data["type"], "state");
    assert_eq!(data["snapshot"]["state"], json!(app.snapshot()));
    assert_eq!(
        data["snapshot"]["active_metadata"]["binding"],
        json!(app.active.bind().unwrap())
    );
    let schema = schemars::generate::SchemaSettings::default()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<Envelope>();
    let validator = jsonschema::validator_for(&serde_json::to_value(schema).unwrap()).unwrap();
    assert!(validator.is_valid(&data));
    assert!(wire.calls.lock().is_empty());
    assert!(app.network.resource().is_none());
    assert!(app.storage.resources_info().is_none());
    assert!(app.tty.cached(&app.active.bind().unwrap()).is_none());
    assert!(!app.events.inner.state.lock().previous.is_empty());
    assert!(tokio::time::timeout(Duration::from_millis(70), sub.next())
        .await
        .is_err());
    drop(sub);
    app.events.shutdown().await;
}
#[tokio::test]
async fn slow_consumers_get_latest_and_switch_never_reuses_old_metadata() {
    let (_dir, app, wire) = crate::input_api::tests::setup();
    let mut sub = app.events.subscribe(&app, &principal("A")).unwrap();
    let first = next(&mut sub).await;
    let a: Value = serde_json::from_str(&first.json).unwrap();
    for i in 0..100 {
        app.inventory.write().channels[0].name = format!("name-{i}");
        app.events.inner.publish(&app);
    }
    let b: Value = serde_json::from_str(&next(&mut sub).await.json).unwrap();
    assert_eq!(
        b["snapshot"]["state"]["inventory"]["channels"][0]["name"],
        "name-99"
    );
    assert!(b["sequence"].as_u64().unwrap() > a["sequence"].as_u64().unwrap() + 1);
    let other = app.inventory.read().channels[1].uid.clone();
    app.active.begin().unwrap();
    app.active.finish(Some(other.clone()));
    app.events.inner.publish(&app);
    let c: Value = serde_json::from_str(&next(&mut sub).await.json).unwrap();
    assert_eq!(
        c["snapshot"]["active_metadata"]["binding"]["uid"],
        json!(other)
    );
    assert_eq!(
        c["snapshot"]["active_metadata"]["binding"]["generation"],
        c["snapshot"]["state"]["active"]["generation"]
    );
    assert!(wire.calls.lock().is_empty());
    app.events.shutdown().await;
}
#[tokio::test]
async fn quotas_drop_revocation_stop_and_idle_collector_are_bounded() {
    let (_dir, app, _) = crate::input_api::tests::setup();
    let owner = principal("A");
    let mut subs = (0..MAX_PER_KEY)
        .map(|_| app.events.subscribe(&app, &owner).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        app.events.subscribe(&app, &owner).err().unwrap().code,
        "EVENT_SUBSCRIPTION_LIMIT"
    );
    for k in 0..3 {
        for _ in 0..MAX_PER_KEY {
            subs.push(
                app.events
                    .subscribe(&app, &principal(&format!("B{k}")))
                    .unwrap(),
            );
        }
    }
    assert_eq!(app.events.status().subscriptions, MAX_CLIENTS);
    assert_eq!(
        app.events
            .subscribe(&app, &principal("C"))
            .err()
            .unwrap()
            .code,
        "EVENT_SUBSCRIPTION_LIMIT"
    );
    owner.cancelled.cancel();
    assert!(subs[0].next().await.is_none());
    let newer = app.events.subscribe(&app, &principal("C")).unwrap();
    assert_eq!(
        app.events.status().subscriptions,
        MAX_CLIENTS - MAX_PER_KEY + 1
    );
    drop(subs);
    drop(newer);
    assert_eq!(app.events.status().subscriptions, 0);
    let mut again = app.events.subscribe(&app, &principal("C")).unwrap();
    assert_eq!(next(&mut again).await.event, "state");
    app.stop();
    assert!(again.next().await.is_none());
    assert_eq!(
        app.events
            .subscribe(&app, &principal("D"))
            .err()
            .unwrap()
            .code,
        "APP_STOPPING"
    );
    app.events.shutdown().await;
    assert!(app.events.task.lock().is_none());
    assert_eq!(app.events.status().subscriptions, 0);
}
#[tokio::test]
async fn oversized_state_signals_resync_without_silently_truncating_inventory() {
    let (_dir, app, _) = crate::input_api::tests::setup();
    app.inventory.write().channels[0].name = "x".repeat(MAX_PAYLOAD + 1);
    let mut sub = app.events.subscribe(&app, &principal("A")).unwrap();
    let frame = next(&mut sub).await;
    assert_eq!(frame.event, "resync");
    assert!(frame.json.len() < 1024);
    let value: Value = serde_json::from_str(&frame.json).unwrap();
    assert_eq!(value["snapshot_url"], SNAPSHOT_PATH);
    let full = json!(snapshot(&app));
    assert_eq!(
        full["state"]["inventory"]["channels"][0]["name"]
            .as_str()
            .unwrap()
            .len(),
        MAX_PAYLOAD + 1
    );
    app.inventory.write().channels[0].name = "normal".into();
    app.events.inner.publish(&app);
    assert_eq!(next(&mut sub).await.event, "state");
    app.events.shutdown().await;
}
#[test]
fn job_progress_contract_excludes_results_and_payloads() {
    let j = crate::storage::JobInfo {
        job_id: "fixture".into(),
        uid: crate::model::Uid::from_bytes(&[1; 12]).unwrap(),
        generation: 1,
        kind: "fixture",
        state: crate::storage::JobState::Completed,
        phase: "done".into(),
        done: 1,
        total: 1,
        can_cancel: false,
        side_effects_possible: false,
        mcu_job: None,
        error: None,
        cleanup_error: None,
        result: Some(json!({"bytes":"PRIVATE_BYTES"})),
        created_at: 0,
        finished_at: Some(1),
        transfer: None,
    };
    let value = json!(JobProgress::from(j));
    assert!(value.get("result").is_none());
    assert!(!value.to_string().contains("PRIVATE_BYTES"));
    assert_eq!(value["state"], "completed");
}
#[tokio::test]
async fn dropping_app_does_not_leave_a_strong_collector_reference() {
    let (_dir, app, _) = crate::input_api::tests::setup();
    let weak = Arc::downgrade(&app);
    let mut sub = app.events.subscribe(&app, &principal("A")).unwrap();
    next(&mut sub).await;
    drop(app);
    tokio::time::timeout(Duration::from_secs(2), async {
        while weak.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(sub.next().await.is_none());
}
