use super::*;
use crate::storage::format::{Artifact, Engine, Guard, Request};
#[derive(Clone, Default)]
pub(super) struct FakeFormat {
    pub fail: Arc<AtomicBool>,
    pub hold: Arc<AtomicBool>,
    pub requests: Arc<Mutex<Vec<Request>>>,
}
impl Engine for FakeFormat {
    fn generate(
        &self,
        request: Request,
        guard: Guard,
    ) -> BoxFuture<'static, Result<Artifact, AppError>> {
        let this = self.clone();
        Box::pin(async move {
            guard()?;
            request.validate()?;
            this.requests.lock().push(request);
            while this.hold.load(Ordering::Acquire) {
                guard()?;
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            guard()?;
            if this.fail.load(Ordering::Acquire) {
                return Err(AppError::new("FORMAT_TOOL_FAILED", "mock mkfs failure"));
            }
            Artifact::mock(request)
        })
    }
}
fn setup_format() -> (
    tempfile::TempDir,
    Arc<AppService>,
    Arc<Fake>,
    Principal,
    FakeFormat,
) {
    let (dir, mut app, fake, p) = setup();
    let formatter = FakeFormat::default();
    Arc::get_mut(&mut Arc::get_mut(&mut app).unwrap().storage)
        .unwrap()
        .formatter = Arc::new(formatter.clone());
    (dir, app, fake, p, formatter)
}
async fn run(app: &Arc<AppService>, p: &Principal, action: Action) -> JobInfo {
    let job = app
        .storage
        .start(app.active.bind().unwrap(), p.clone(), action)
        .unwrap();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let info = app.storage.job(p, &job.job_id).unwrap();
            if info.finished_at.is_some() {
                break info;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn generation_failure_or_cancel_never_deletes_or_unmounts_existing_media() {
    let (_dir, app, fake, p, formatter) = setup_format();
    fake.data.lock().catalog_rows = Some(vec![(2, 131072, 1)]);
    fake.data.lock().optical = Some(abi::Optical {
        id: 100,
        size: 8192,
        mounted: true,
        name: "boot.iso".into(),
        crc: 1,
    });
    formatter.fail.store(true, Ordering::Release);
    let bad = run(
        &app,
        &p,
        Action::DiskFormat {
            id: 2,
            format: 1,
            confirmed: true,
            expected_size: Some(67108864),
            expected_card_uuid: None,
        },
    )
    .await;
    assert!(bad.state == JobState::Failed);
    assert_eq!(bad.error.unwrap().code, "FORMAT_TOOL_FAILED");
    assert!(!bad.side_effects_possible);
    assert!(fake.data.lock().optical.as_ref().unwrap().mounted);
    assert!(!fake
        .data
        .lock()
        .calls
        .iter()
        .any(|c| [0x307, 0x310, 0x31d].contains(&c.op)));
    formatter.fail.store(false, Ordering::Release);
    formatter.hold.store(true, Ordering::Release);
    let accepted = app
        .storage
        .start(
            app.active.bind().unwrap(),
            p.clone(),
            Action::DiskCreate {
                format: 2,
                size_mib: 64,
                confirmed: true,
                expected_card_uuid: None,
            },
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while formatter.requests.lock().len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    app.select(None).await.unwrap();
    assert!(wait(&app, &p, &accepted.job_id).await.state == JobState::Cancelled);
    assert!(fake.data.lock().deleted.is_empty());
}
#[tokio::test]
async fn selected_format_preserves_exact_capacity_and_deletes_only_after_preparation() {
    let (_dir, app, fake, p, formatter) = setup_format();
    let bytes = 9 * 1048576 + 512;
    fake.data.lock().catalog_rows = Some(vec![(2, (bytes / 512) as u32, 1)]);
    let info = run(
        &app,
        &p,
        Action::DiskFormat {
            id: 2,
            format: 2,
            confirmed: true,
            expected_size: Some(bytes),
            expected_card_uuid: Some("09".repeat(16)),
        },
    )
    .await;
    assert!(
        info.state == JobState::Completed,
        "{}",
        serde_json::to_string(&info).unwrap()
    );
    assert_eq!(info.result.unwrap()["image"]["size"], bytes);
    let requests = formatter.requests.lock();
    assert_eq!(requests[0].bytes, bytes);
    assert!(requests[0].existing);
    drop(requests);
    assert_eq!(fake.data.lock().deleted, [2]);
    assert_eq!(
        fake.data.lock().catalog_rows.as_ref().unwrap(),
        &[(2, (bytes / 512) as u32, 2)]
    );
    assert!(app.storage.resources_info().is_none());
}
#[tokio::test]
async fn new_image_uses_same_job_path_and_invalid_existing_sizes_fail_before_delete() {
    let (_dir, app, fake, p, formatter) = setup_format();
    fake.data.lock().catalog_rows = Some(vec![(2, 128, 1)]);
    let bad = run(
        &app,
        &p,
        Action::DiskFormat {
            id: 2,
            format: 1,
            confirmed: true,
            expected_size: None,
            expected_card_uuid: None,
        },
    )
    .await;
    assert!(bad.state == JobState::Failed);
    assert!(formatter.requests.lock().is_empty());
    assert!(fake.data.lock().deleted.is_empty());
    let bad = run(
        &app,
        &p,
        Action::DiskFormat {
            id: 2,
            format: 2,
            confirmed: true,
            expected_size: Some(4096),
            expected_card_uuid: None,
        },
    )
    .await;
    assert_eq!(bad.error.unwrap().code, "IMAGE_CHANGED");
    let info = run(
        &app,
        &p,
        Action::DiskCreate {
            format: 3,
            size_mib: 64,
            confirmed: true,
            expected_card_uuid: None,
        },
    )
    .await;
    assert!(
        info.state == JobState::Completed,
        "{}",
        serde_json::to_string(&info).unwrap()
    );
    assert_eq!(formatter.requests.lock()[0].bytes, 67108864);
    assert!(fake.data.lock().deleted.is_empty());
}
#[test]
fn format_arguments_reject_paths_routing_bad_sizes_and_missing_confirmation() {
    for value in [
        json!({"action":"disk_create","format":1,"size_mib":64,"confirmed":true}),
        json!({"action":"disk_format","id":2,"format":2,"confirmed":true}),
    ] {
        serde_json::from_value::<Action>(value.clone())
            .unwrap()
            .normalize()
            .unwrap();
        for field in ["uid", "uids", "generation", "path", "filename"] {
            let mut invalid = value.clone();
            invalid[field] = json!("no");
            assert!(serde_json::from_value::<Action>(invalid).is_err());
        }
        let mut invalid = value;
        invalid["confirmed"] = json!(false);
        assert!(serde_json::from_value::<Action>(invalid)
            .unwrap()
            .normalize()
            .is_err());
    }
    for size_mib in [0, 1, 63, 32769, u32::MAX] {
        assert!(Action::DiskCreate {
            format: 1,
            size_mib,
            confirmed: true,
            expected_card_uuid: None
        }
        .normalize()
        .is_err());
    }
}
