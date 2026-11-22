use super::*;
fn approve(app: &AppService, dir: &tempfile::TempDir) -> folder::SourceInfo {
    let b = app.active.bind().unwrap();
    let session = app.storage.import_session(&b).unwrap();
    app.storage
        .approve_folder(
            b,
            folder_image::Root::approve(dir.path().to_path_buf()).unwrap(),
            session,
        )
        .unwrap()
}
fn share(id: &str) -> Action {
    Action::FolderShare {
        source_id: id.into(),
        confirmed: true,
        expected_card_uuid: Some("09".repeat(16)),
    }
}
async fn run(app: &Arc<AppService>, p: &Principal, a: Action) -> JobInfo {
    let job = app
        .storage
        .start(app.active.bind().unwrap(), p.clone(), a)
        .unwrap();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let result = app.storage.job(p, &job.job_id).unwrap();
            if result.finished_at.is_some() {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn folder_share_commits_full_zero_tail_crc_and_mounts_target_readonly() {
    let (_dir, app, fake, p) = setup();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("key.txt"), b"fixture only").unwrap();
    let info = approve(&app, &source);
    let finished = run(&app, &p, share(&info.source_id)).await;
    assert!(
        finished.state == JobState::Completed,
        "{}",
        serde_json::to_string(&finished).unwrap()
    );
    let result = finished.result.unwrap();
    assert_eq!(result["readonly"], true);
    assert_eq!(result["folder"]["files"], 1);
    let id = result["image"]["id"].as_u64().unwrap() as u32;
    {
        let d = fake.data.lock();
        let disk = d.disk.as_ref().unwrap();
        assert_eq!(disk.id, id);
        assert_eq!(disk.state, 4);
        assert!(!disk.writable);
        assert!(disk.sectors > 65536 * 8);
        assert_eq!(d.calls.iter().filter(|c| c.op == 0x312).count(), 1);
        assert!(d
            .calls
            .iter()
            .filter(|c| c.op == 0x312)
            .all(|c| c.bytes[4..] == [0, 1, 0, 0]));
        assert!(d
            .calls
            .iter()
            .filter(|c| c.op == 0x315)
            .all(|c| abi::u32at(&c.bytes, 4)
                < result["folder"]["populated_sectors"].as_u64().unwrap() as u32));
    }
    assert_eq!(
        std::fs::read(source.path().join("key.txt")).unwrap(),
        b"fixture only"
    );
    // Closing the originating UI/client keeps the successfully exported image.
    p.ended.cancel();
    assert!(app.storage.resources_info().is_some());
    app.select(None).await.unwrap();
    assert!(fake.data.lock().disk.is_none());
}
#[tokio::test]
async fn preparation_failures_and_guard_rejection_leave_existing_media_intact() {
    let (_dir, app, fake, p) = setup();
    let source = tempfile::tempdir().unwrap();
    std::fs::File::create(source.path().join("oversize.img"))
        .unwrap()
        .set_len(1u64 << 32)
        .unwrap();
    let info = approve(&app, &source);
    fake.data.lock().optical = Some(abi::Optical {
        id: 50,
        size: 8192,
        mounted: true,
        name: "keep.iso".into(),
        crc: 0,
    });
    let fail = run(&app, &p, share(&info.source_id)).await;
    assert_eq!(fail.error.unwrap().code, "FOLDER_LIMIT");
    assert!(!fail.side_effects_possible);
    assert!(fake.data.lock().optical.as_ref().unwrap().mounted);
    assert!(!fake
        .data
        .lock()
        .calls
        .iter()
        .any(|c| [0x310, 0x307].contains(&c.op)));
    let mut wrong = share(&info.source_id);
    if let Action::FolderShare {
        expected_card_uuid, ..
    } = &mut wrong
    {
        *expected_card_uuid = Some("ff".repeat(16));
    }
    assert_eq!(
        run(&app, &p, wrong).await.error.unwrap().code,
        "CARD_CHANGED"
    );
}
#[tokio::test]
async fn source_capabilities_are_bounded_revocable_and_pinned_to_active_and_session() {
    let (_dir, app, fake, p) = setup();
    let source = tempfile::tempdir().unwrap();
    let info = approve(&app, &source);
    let public =
        serde_json::to_string(&app.storage.folder_sources(&app.active.bind().unwrap())).unwrap();
    assert!(!public.contains(&source.path().to_string_lossy().to_string()));
    assert!(!public.contains("path"));
    fake.session.store(2, Ordering::Release);
    assert_eq!(
        run(&app, &p, share(&info.source_id))
            .await
            .error
            .unwrap()
            .code,
        "FOLDER_SOURCE_EXPIRED"
    );
    assert!(!fake.data.lock().calls.iter().any(|c| c.op == 0x310));
    let info = approve(&app, &source);
    let forgotten = run(
        &app,
        &p,
        Action::FolderForget {
            source_id: info.source_id.clone(),
        },
    )
    .await;
    assert!(forgotten.state == JobState::Completed);
    assert_eq!(
        run(&app, &p, share(&info.source_id))
            .await
            .error
            .unwrap()
            .code,
        "FOLDER_SOURCE_EXPIRED"
    );
    let old = app.active.bind().unwrap();
    for _ in 0..16 {
        approve(&app, &source);
    }
    let b = app.active.bind().unwrap();
    assert_eq!(
        app.storage
            .approve_folder(
                b,
                folder_image::Root::approve(source.path().to_path_buf()).unwrap(),
                2
            )
            .err()
            .unwrap()
            .code,
        "QUEUE_FULL"
    );
    app.select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
        .await
        .unwrap();
    assert!(app
        .storage
        .folder_sources(&app.active.bind().unwrap())
        .is_empty());
    assert_eq!(
        run(&app, &p, share(&info.source_id))
            .await
            .error
            .unwrap()
            .code,
        "FOLDER_SOURCE_EXPIRED"
    );
    assert!(app.storage.forget_folder(&old, &info.source_id).is_err());
}
#[test]
fn folder_actions_never_accept_a_host_path_routing_override_or_missing_confirmation() {
    let value = json!({"action":"folder_share","source_id":"01".repeat(32),"confirmed":true});
    for field in ["path", "folder", "uid", "uids", "generation"] {
        let mut v = value.clone();
        v[field] = json!("C:/");
        assert!(serde_json::from_value::<Action>(v).is_err());
    }
    assert!(Action::FolderShare {
        source_id: "C:/".into(),
        confirmed: true,
        expected_card_uuid: None
    }
    .normalize()
    .is_err());
    assert!(Action::FolderShare {
        source_id: "01".repeat(32),
        confirmed: false,
        expected_card_uuid: None
    }
    .normalize()
    .is_err());
}
