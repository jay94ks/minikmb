use super::*;
fn request() -> Action {
    Action::CardFormat {
        format: 2,
        confirmed: true,
        expected_card_uuid: Some("09".repeat(16)),
    }
}
async fn started(app: &AppService) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while app.storage.card_format_info().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn no_cancel(fake: &Fake) {
    assert!(!fake.data.lock().calls.iter().any(|c| c.op == 8));
}
#[tokio::test]
async fn whole_card_format_verifies_profile_and_erases_names_values_catalog() {
    let (_dir, app, fake, p) = setup();
    fake.data.lock().name = "old name".into();
    fake.data.lock().user = vec![5; 4096];
    let info = action(&app, &p, request()).await;
    assert!(
        info.state == JobState::Completed,
        "{}",
        serde_json::to_string(&info).unwrap()
    );
    assert_eq!(info.result.as_ref().unwrap()["verified"], true);
    assert_eq!(fake.data.lock().verified.last().unwrap(), "");
    assert!(app.storage.card_format_info().is_none());
    let snapshot = action(&app, &p, Action::Refresh {}).await.result.unwrap();
    assert_eq!(snapshot["images"], json!([]));
    assert_eq!(snapshot["card"]["uuid"], "0a".repeat(16));
    no_cancel(&fake);
}
#[tokio::test]
async fn acceptance_seal_blocks_cancel_switch_without_invalidating_generation() {
    let (_dir, app, fake, p) = setup();
    fake.hold.store(true, Ordering::Release);
    fake.hold_start.store(true, Ordering::Release);
    let b = app.active.bind().unwrap();
    let job = app.storage.start(b.clone(), p.clone(), request()).unwrap();
    started(&app).await;
    let error = app.storage.cancel(&p, &job.job_id).err().unwrap();
    assert_eq!(error.code, "JOB_NOT_CANCELLABLE");
    let context = crate::remote_error::Context::new();
    context.observe(Binding {
        uid: Uid::from_bytes(&[2; 12]).unwrap(),
        generation: b.generation + 1,
    });
    let failure = context.failure(error);
    assert_eq!(failure.channel_uid, Some(b.uid.clone()));
    assert_eq!(failure.generation, Some(b.generation));
    assert_eq!(failure.job_id, Some(job.job_id.clone()));
    assert_eq!(
        app.select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
            .await
            .unwrap_err()
            .code,
        "CARD_FORMAT_PENDING"
    );
    assert_eq!(app.active.bind().unwrap(), b);
    assert!(!app.active.state().switching);
    fake.hold.store(false, Ordering::Release);
    fake.hold_start.store(false, Ordering::Release);
    fake.resume.notify_one();
    assert!(wait(&app, &p, &job.job_id).await.state == JobState::Completed);
    no_cancel(&fake);
}
#[tokio::test]
async fn revoked_owner_does_not_cancel_accepted_format_readonly_monitor() {
    for revoke_key in [true, false] {
        let (_dir, app, fake, p) = setup();
        fake.hold.store(true, Ordering::Release);
        let job = app
            .storage
            .start(app.active.bind().unwrap(), p.clone(), request())
            .unwrap();
        started(&app).await;
        let internal = app.storage.state.lock().current.clone().unwrap();
        if revoke_key {
            p.cancelled.cancel();
        } else {
            p.ended.cancel();
        }
        fake.hold.store(false, Ordering::Release);
        tokio::time::timeout(Duration::from_secs(3), internal.wait())
            .await
            .unwrap();
        assert!(
            internal.snapshot().state == JobState::Completed,
            "{}",
            job.job_id
        );
        no_cancel(&fake);
        assert!(fake
            .data
            .lock()
            .calls
            .iter()
            .filter(|c| c.cleanup)
            .all(|c| [7, 0x308, 0x31e].contains(&c.op)));
    }
}
#[tokio::test]
async fn lost_ack_is_not_retried_or_cancelled_and_readonly_recovery_clears_hold() {
    let (_dir, app, fake, p) = setup();
    fake.bad_start.store(true, Ordering::Release);
    let job = action(&app, &p, request()).await;
    assert_eq!(job.error.as_ref().unwrap().code, "CARD_FORMAT_UNCONFIRMED");
    assert!(job.error.unwrap().uncertain);
    assert!(app.storage.card_format_info().unwrap().job_id.is_none());
    assert_eq!(
        app.storage
            .start(
                app.active.bind().unwrap(),
                p.clone(),
                Action::NameSet { name: "x".into() }
            )
            .err()
            .unwrap()
            .code,
        "CARD_FORMAT_PENDING"
    );
    assert!(app.select(None).await.is_err());
    let recovery = action(&app, &p, Action::CardFormatStatus {}).await;
    assert!(recovery.state == JobState::Completed);
    assert!(app.storage.card_format_info().is_none());
    assert_eq!(recovery.result.unwrap()["outcome"], "observed_terminal");
    assert_eq!(
        fake.data
            .lock()
            .calls
            .iter()
            .filter(|c| c.op == 0x30f)
            .count(),
        1
    );
    no_cancel(&fake);
}
#[tokio::test]
async fn disconnect_preserves_hold_across_session_reconnect_and_shutdown_never_cancels() {
    let (_dir, app, fake, p) = setup();
    fake.hold.store(true, Ordering::Release);
    let job = app
        .storage
        .start(app.active.bind().unwrap(), p.clone(), request())
        .unwrap();
    started(&app).await;
    // Wait for the accepted ID, not just pre-dispatch sealing.
    while app.storage.card_format_info().unwrap().job_id.is_none() {
        tokio::task::yield_now().await;
    }
    fake.session.store(2, Ordering::Release);
    let failed = wait(&app, &p, &job.job_id).await;
    assert!(failed.state == JobState::Failed);
    assert!(app.storage.card_format_info().is_some());
    assert!(app
        .select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
        .await
        .is_err());
    fake.hold.store(false, Ordering::Release);
    let recovered = action(&app, &p, Action::CardFormatStatus {})
        .await
        .result
        .unwrap();
    assert_eq!(recovered["same_session"], false);
    assert_eq!(recovered["outcome"], "observed_terminal");
    app.storage.shutdown().await;
    no_cancel(&fake);
}
#[tokio::test]
async fn running_format_survives_app_stop_and_restart_refresh_adopts_it() {
    let (_dir, app, fake, p) = setup();
    fake.hold.store(true, Ordering::Release);
    let job = app
        .storage
        .start(app.active.bind().unwrap(), p.clone(), request())
        .unwrap();
    started(&app).await;
    app.storage.stop();
    app.storage.shutdown().await;
    let internal = app.storage.state.lock().jobs.front().unwrap().clone();
    tokio::time::timeout(Duration::from_secs(3), internal.wait())
        .await
        .unwrap();
    assert!(
        internal.snapshot().state == JobState::Failed,
        "{}",
        job.job_id
    );
    no_cancel(&fake);
    // New manager, same physical fake, no retained RAM job/hold.
    let manager = Manager::new(
        app.active.clone(),
        Arc::new(FakeBackend {
            fake: fake.clone(),
            active: app.active.clone(),
        }),
    );
    let info = manager
        .start(app.active.bind().unwrap(), p.clone(), Action::Refresh {})
        .unwrap();
    let internal = manager.state.lock().jobs.front().unwrap().clone();
    internal.wait().await;
    assert!(manager.card_format_info().is_some(), "{}", info.job_id);
    assert!(!fake.data.lock().calls.iter().any(|c| c.op == 0x31b));
    no_cancel(&fake);
}
#[test]
fn restricted_scope_and_request_validation() {
    for format in [0, 3, 255] {
        assert!(Action::CardFormat {
            format,
            confirmed: true,
            expected_card_uuid: None
        }
        .normalize()
        .is_err());
    }
    assert!(Action::CardFormat {
        format: 1,
        confirmed: false,
        expected_card_uuid: None
    }
    .normalize()
    .is_err());
    let scope = Cleanup::CardFormat(42);
    assert!(scope.matches(0, 7, &42u32.to_le_bytes()));
    assert!(!scope.matches(0, 8, &42u32.to_le_bytes()));
    assert!(!scope.matches(0, 7, &43u32.to_le_bytes()));
    assert!(scope.matches(3, 0x308, &[]));
    assert!(!scope.matches(3, 0x30f, &[2, 0, 0, 0]));
}

#[tokio::test]
async fn preflight_cancel_card_guard_and_open_path_never_send_format() {
    let (_dir, app, fake, p) = setup();
    let job = app
        .storage
        .start(app.active.bind().unwrap(), p.clone(), request())
        .unwrap();
    app.storage.cancel(&p, &job.job_id).unwrap();
    assert!(wait(&app, &p, &job.job_id).await.state == JobState::Cancelled);
    let wrong = action(
        &app,
        &p,
        Action::CardFormat {
            format: 2,
            confirmed: true,
            expected_card_uuid: Some("ff".repeat(16)),
        },
    )
    .await;
    assert_eq!(wrong.error.unwrap().code, "CARD_CHANGED");
    let open = action(
        &app,
        &p,
        Action::DiskOpen {
            id: 2,
            path: media::Path::Direct,
            writable: true,
            expected_card_uuid: None,
        },
    )
    .await;
    assert!(open.state == JobState::Completed);
    let denied = action(&app, &p, request()).await;
    assert_eq!(denied.error.unwrap().code, "MEDIA_CONFLICT");
    assert!(!fake.data.lock().calls.iter().any(|c| c.op == 0x30f));
}

#[tokio::test]
async fn unconfirmed_ack_then_reboot_reports_unknown_without_inventing_success() {
    let (_dir, app, fake, p) = setup();
    fake.bad_start.store(true, Ordering::Release);
    let original = action(&app, &p, request()).await;
    assert!(original.state == JobState::Failed);
    fake.session.store(2, Ordering::Release);
    fake.data.lock().job = None;
    let recovered = action(&app, &p, Action::CardFormatStatus {}).await;
    assert_eq!(recovered.result.unwrap()["outcome"], "unknown");
    assert!(app.storage.job(&p, &original.job_id).unwrap().state == JobState::Failed);
    assert!(app.storage.card_format_info().is_none());
    no_cancel(&fake);
}
