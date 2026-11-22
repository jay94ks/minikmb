use super::*;
use crate::storage::media::Path;

fn open(path: Path, writable: bool) -> Action {
    Action::DiskOpen {
        id: 2,
        path,
        writable,
        expected_card_uuid: Some("09".repeat(16)),
    }
}
fn optical(fake: &Fake) {
    fake.data.lock().optical = Some(abi::Optical {
        id: 100,
        size: 8192,
        mounted: false,
        name: "boot.iso".into(),
        crc: 1,
    });
}
fn completed(job: &JobInfo) {
    assert!(
        job.state == JobState::Completed,
        "{}",
        serde_json::to_string(job).unwrap()
    );
}
#[tokio::test]
async fn direct_sector_roundtrip_exact_parts_and_guards() {
    let (_dir, app, fake, p) = setup();
    completed(&action(&app, &p, open(Path::Direct, true)).await);
    let before = fake.data.lock().calls.len();
    let stale = action(
        &app,
        &p,
        Action::DiskWrite {
            id: 2,
            lba: 0,
            bytes: vec![0; 512],
            confirmed: true,
            expected_lease: Some(999),
            expected_card_uuid: None,
        },
    )
    .await;
    assert!(stale.state == JobState::Failed);
    assert!(!fake.data.lock().calls[before..]
        .iter()
        .any(|c| c.op == 0x315));
    let bytes: Vec<_> = (0..512).map(|n| (n % 251) as u8).collect();
    let write = Action::DiskWrite {
        id: 2,
        lba: 127,
        bytes: bytes.clone(),
        confirmed: true,
        expected_lease: None,
        expected_card_uuid: Some("09".repeat(16)),
    };
    completed(&action(&app, &p, write.clone()).await);
    let read = action(
        &app,
        &p,
        Action::DiskRead {
            id: 2,
            lba: 127,
            expected_card_uuid: None,
        },
    )
    .await;
    completed(&read);
    assert_eq!(read.result.unwrap()["sector"]["bytes"], json!(bytes));
    let calls = fake.data.lock().calls.clone();
    let parts: Vec<_> = calls
        .iter()
        .filter(|c| c.op == 0x316)
        .map(|c| c.bytes[8])
        .collect();
    assert_eq!(parts, [0, 1]);
    assert!(calls.iter().any(|c| c.op == 0x317));
    for invalid in [
        Action::DiskRead {
            id: 3,
            lba: 0,
            expected_card_uuid: None,
        },
        Action::DiskRead {
            id: 2,
            lba: 128,
            expected_card_uuid: None,
        },
        Action::DiskRead {
            id: 2,
            lba: 0,
            expected_card_uuid: Some("stale-card".into()),
        },
    ] {
        let count = fake.data.lock().calls.len();
        let bad = action(&app, &p, invalid).await;
        assert!(bad.state == JobState::Failed);
        assert!(!fake.data.lock().calls[count..]
            .iter()
            .any(|c| [0x314, 0x315].contains(&c.op)));
    }
    for invalid in [
        Action::DiskWrite {
            id: 2,
            lba: 0,
            bytes: vec![0; 512],
            confirmed: false,
            expected_lease: None,
            expected_card_uuid: None,
        },
        Action::DiskWrite {
            id: 2,
            lba: 0,
            bytes: vec![0; 511],
            confirmed: true,
            expected_lease: None,
            expected_card_uuid: None,
        },
    ] {
        assert!(app
            .storage
            .start(app.active.bind().unwrap(), p.clone(), invalid)
            .is_err());
    }
    completed(&action(&app, &p, Action::DiskClose { id: 2 }).await);
    assert!(app.storage.resources_info().is_none());
}
#[tokio::test]
async fn exported_and_readonly_disks_reject_direct_write_and_sync() {
    let (_dir, app, fake, p) = setup();
    for path in [Path::Target, Path::Management, Path::Direct] {
        completed(&action(&app, &p, open(path, false)).await);
        let before = fake.data.lock().calls.len();
        let write = action(
            &app,
            &p,
            Action::DiskWrite {
                id: 2,
                lba: 0,
                bytes: vec![1; 512],
                confirmed: true,
                expected_lease: None,
                expected_card_uuid: None,
            },
        )
        .await;
        assert!(write.state == JobState::Failed);
        if path != Path::Direct {
            assert!(
                action(
                    &app,
                    &p,
                    Action::DiskRead {
                        id: 2,
                        lba: 0,
                        expected_card_uuid: None
                    }
                )
                .await
                .state
                    == JobState::Failed
            );
            assert!(action(&app, &p, Action::DiskSync { id: 2 }).await.state == JobState::Failed);
        }
        assert!(!fake.data.lock().calls[before..]
            .iter()
            .any(|c| [0x314, 0x315, 0x317].contains(&c.op)));
        completed(&action(&app, &p, Action::DiskClose { id: 2 }).await);
    }
}
#[tokio::test]
async fn optical_disable_ejects_first_and_mount_lives_after_dialog_owner() {
    let (_dir, app, fake, p) = setup();
    optical(&fake);
    completed(
        &action(
            &app,
            &p,
            Action::OpticalMount {
                id: 100,
                mounted: true,
                expected_card_uuid: None,
            },
        )
        .await,
    );
    p.ended.cancel();
    assert_eq!(app.storage.resources_info().unwrap().optical_id, Some(100));
    assert!(fake.data.lock().optical.as_ref().unwrap().mounted);
    let mut other = p.clone();
    other.ended = Default::default();
    completed(&action(&app, &other, Action::OpticalSet { enabled: false }).await);
    assert!(app.storage.resources_info().is_none());
    let d = fake.data.lock();
    let eject = d.calls.iter().position(|c| c.op == 0x307).unwrap();
    let release = d.calls.iter().position(|c| c.op == 0xffff).unwrap();
    let off = d.calls.iter().position(|c| c.op == 0x31a).unwrap();
    assert!(eject < release && release < off);
    assert!(d.optical_off);
    assert!(!d.calls.iter().any(|c| c.op == 0x313));
}
#[tokio::test]
async fn switch_cleans_original_wire_and_failure_retains_claim() {
    let (_dir, app, fake, p) = setup();
    let original = app.active.bind().unwrap();
    completed(&action(&app, &p, open(Path::Target, true)).await);
    fake.deny_cleanup.store(true, Ordering::Release);
    assert!(app.select(None).await.is_err());
    assert_eq!(app.active.state().uid, Some(original.uid.clone()));
    assert!(app.storage.resources_info().is_some());
    assert!(app.storage.media_error().is_some());
    fake.deny_cleanup.store(false, Ordering::Release);
    app.select(None).await.unwrap();
    assert!(app.storage.resources_info().is_none());
    assert!(fake.data.lock().disk.is_none());
    assert!(fake
        .data
        .lock()
        .calls
        .iter()
        .filter(|c| c.cleanup)
        .all(|c| c.uid == original.uid));
}
#[tokio::test]
async fn lost_session_requires_source_recovery_and_does_not_replay_old_lease() {
    let (_dir, app, fake, p) = setup();
    let original = app.active.bind().unwrap();
    completed(&action(&app, &p, open(Path::Target, true)).await);
    fake.session.fetch_add(1, Ordering::AcqRel);
    app.active.finish(None);
    assert!(app.storage.resources_info().unwrap().connection_lost);
    let count = fake.data.lock().calls.len();
    let other = app
        .inventory
        .read()
        .channels
        .iter()
        .find(|c| c.uid != original.uid)
        .unwrap()
        .uid
        .clone();
    assert!(app.select(Some(other)).await.is_err());
    assert_eq!(fake.data.lock().calls.len(), count);
    app.select(Some(original.uid)).await.unwrap();
    completed(&action(&app, &p, Action::DiskClose { id: 2 }).await);
    assert!(app.storage.resources_info().is_none());
    assert!(!fake.data.lock().calls[count..]
        .iter()
        .any(|c| c.cleanup && c.op == 0x313));
}
#[tokio::test]
async fn changed_card_cleanup_does_not_eject_new_cards_reused_image_id() {
    let (_dir, app, fake, p) = setup();
    optical(&fake);
    completed(
        &action(
            &app,
            &p,
            Action::OpticalMount {
                id: 100,
                mounted: true,
                expected_card_uuid: None,
            },
        )
        .await,
    );
    fake.data.lock().card_epoch = 1;
    app.select(None).await.unwrap();
    assert!(fake.data.lock().optical.as_ref().unwrap().mounted);
    assert!(!fake.data.lock().calls.iter().any(|c| c.op == 0x307));
    assert!(app.storage.resources_info().is_none());
}
#[tokio::test]
async fn cancel_open_during_acceptance_clears_pending_claim_after_job_cancel() {
    let (_dir, app, fake, p) = setup();
    fake.hold_start.store(true, Ordering::Release);
    fake.hold.store(true, Ordering::Release);
    let accepted = app
        .storage
        .start(
            app.active.bind().unwrap(),
            p.clone(),
            open(Path::Target, true),
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while fake.data.lock().job.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(app.storage.resources_info().unwrap().disk_id, Some(2));
    app.storage.cancel(&p, &accepted.job_id).unwrap();
    fake.hold_start.store(false, Ordering::Release);
    fake.resume.notify_waiters();
    let result = wait(&app, &p, &accepted.job_id).await;
    assert!(result.state == JobState::Cancelled);
    assert!(result.cleanup_error.is_none());
    assert!(app.storage.resources_info().is_none());
    assert!(fake.data.lock().disk.is_none());
    assert!(fake
        .data
        .lock()
        .calls
        .iter()
        .any(|c| c.op == 8 && c.cleanup));
}
#[tokio::test]
async fn refresh_card_releases_old_claim_and_shutdown_closes_both_owned_media() {
    let (_dir, app, fake, p) = setup();
    optical(&fake);
    completed(
        &action(
            &app,
            &p,
            Action::OpticalMount {
                id: 100,
                mounted: true,
                expected_card_uuid: None,
            },
        )
        .await,
    );
    completed(&action(&app, &p, open(Path::Target, true)).await);
    app.storage.shutdown().await;
    assert!(fake.data.lock().disk.is_none());
    assert!(!fake.data.lock().optical.as_ref().unwrap().mounted);
    assert!(app.storage.resources_info().is_none());
    let (_dir, app, fake, p) = setup();
    optical(&fake);
    completed(
        &action(
            &app,
            &p,
            Action::OpticalMount {
                id: 100,
                mounted: true,
                expected_card_uuid: None,
            },
        )
        .await,
    );
    fake.data.lock().card_epoch = 1;
    completed(&action(&app, &p, Action::CardInfo {}).await);
    assert!(app.storage.resources_info().is_none());
    assert!(fake.data.lock().optical.as_ref().unwrap().mounted);
    assert!(!fake.data.lock().calls.iter().any(|c| c.op == 0x307));
}
