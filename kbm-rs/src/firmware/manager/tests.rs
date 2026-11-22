use super::super::backend::{Programmer, Progress};
use super::*;
use futures_util::future::BoxFuture;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify as Signal;
pub(crate) struct Fake {
    capacity: u64,
    uid: Option<Uid>,
    pub(crate) writes: Arc<AtomicUsize>,
    pub(crate) entered: Arc<Signal>,
    pub(crate) finish: Arc<Signal>,
}
impl Backend for Fake {
    fn list(
        &self,
        target: Target,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<Vec<Programmer>, AppError>> {
        Box::pin(async move {
            Ok(vec![Programmer {
                id: "test".into(),
                label: "test".into(),
                target,
                selectable: true,
            }])
        })
    }
    fn inspect(
        &self,
        target: Target,
        id: String,
        _: CancellationToken,
    ) -> BoxFuture<'static, Result<Inspection, AppError>> {
        let capacity = self.capacity;
        let uid = self.uid.clone();
        Box::pin(async move {
            Ok(Inspection {
                target,
                programmer_id: id,
                uid,
                capacity,
            })
        })
    }
    fn program(
        &self,
        expected: Inspection,
        image: image::Prepared,
        control: Control,
        notify: Notify,
    ) -> BoxFuture<'static, Result<Programmed, AppError>> {
        let writes = self.writes.clone();
        let entered = self.entered.clone();
        let finish = self.finish.clone();
        Box::pin(async move {
            (control.commit)()?;
            writes.fetch_add(1, Ordering::SeqCst);
            entered.notify_one();
            finish.notified().await;
            let bytes = image.bytes.len() as u64;
            notify(Progress {
                phase: "verified",
                done: bytes,
                total: bytes,
            });
            Ok(Programmed {
                target: expected.target,
                programmer_id: expected.programmer_id,
                bytes,
                verified_bytes: expected.capacity,
                verified: true,
                reset: false,
                tail_preserved: true,
            })
        })
    }
}
fn owner(key: &str) -> Principal {
    Principal {
        key_id: key.into(),
        client_id: "test".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    }
}
pub(crate) fn setup(
    capacity: u64,
    uid: Option<Uid>,
) -> (tempfile::TempDir, Arc<AppService>, Arc<Fake>) {
    let (dir, mut app, _) = crate::input_api::tests::setup();
    let fake = Arc::new(Fake {
        capacity,
        uid,
        writes: Default::default(),
        entered: Default::default(),
        finish: Default::default(),
    });
    Arc::get_mut(&mut app).unwrap().firmware = Manager::new(fake.clone());
    (dir, app, fake)
}
async fn terminal(app: &AppService, p: &Principal, id: &str) -> Info {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let job = app.firmware.job(p, id).unwrap();
            if job.terminal() {
                break job;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn too_small_capacity_fails_before_channel_cleanup_or_programming() {
    for target in [Target::Ms2130, Target::Stm32] {
        let (_dir, app, fake) = setup(4, None);
        let b = app.active.bind().unwrap();
        let p = owner("A");
        let job = app
            .firmware
            .start(
                app.clone(),
                b.clone(),
                p.clone(),
                target,
                "test".into(),
                image::prepare(Target::Ms2130, &[0x55; 8]).unwrap(),
            )
            .unwrap();
        let result = terminal(&app, &p, &job.job_id).await;
        assert_eq!(result.error.unwrap().code, "FLASH_TOO_SMALL");
        assert_eq!(fake.writes.load(Ordering::SeqCst), 0);
        app.active.validate(&b).unwrap();
    }
}
#[tokio::test]
async fn both_programmers_work_without_any_channel_or_management_firmware() {
    for target in [Target::Stm32, Target::Ms2130] {
        let swd_uid = (target == Target::Stm32).then(|| Uid::from_bytes(&[9; 12]).unwrap());
        let (_dir, app, fake) = setup(2097152, swd_uid.clone());
        app.select(None).await.unwrap();
        app.inventory.write().channels.clear();
        let p = owner("A");
        let inspection = app
            .firmware
            .inspect(&app, None, &p, target, "test".into())
            .await
            .unwrap();
        assert_eq!(inspection.uid, swd_uid);
        let bytes = if target == Target::Stm32 {
            let mut bytes = vec![0; 16];
            bytes[..4].copy_from_slice(&0x24080000u32.to_le_bytes());
            bytes[4..8].copy_from_slice(&0x08000009u32.to_le_bytes());
            bytes
        } else {
            vec![0x55; 16]
        };
        let source = app
            .firmware
            .sources
            .stage(
                None,
                p.clone(),
                super::super::sources::Spec {
                    target,
                    name: "new-board.bin".into(),
                    size: bytes.len() as u64,
                },
                &bytes,
            )
            .unwrap();
        let started = app.execute_firmware(None, &p, "firmware_start", serde_json::json!({
            "target": target, "programmer_id":"test", "source_id":source.source_id, "confirmed":true,
        })).await.unwrap();
        let id = started["job_id"].as_str().unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), fake.entered.notified())
            .await
            .unwrap();
        assert!(app.active.state().uid.is_none());
        fake.finish.notify_one();
        let result = terminal(&app, &p, id).await;
        assert!(matches!(result.state, State::Completed));
        assert_eq!(result.uid, swd_uid);
        assert_eq!(result.generation, None);
        assert_eq!(fake.writes.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn committed_job_is_pinned_blocks_switches_and_finishes_despite_cancel_or_key_revoke() {
    let (_dir, app, fake) = setup(64, None);
    let b = app.active.bind().unwrap();
    let p = owner("A");
    let job = app
        .firmware
        .start(
            app.clone(),
            b.clone(),
            p.clone(),
            Target::Ms2130,
            "test".into(),
            image::prepare(Target::Ms2130, &[0x55; 32]).unwrap(),
        )
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), fake.entered.notified())
        .await
        .unwrap();
    assert!(app.active.state().uid.is_none());
    assert!(!app.firmware.cancel(&p, &job.job_id).unwrap().can_cancel);
    assert_eq!(
        app.select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
            .await
            .unwrap_err()
            .code,
        "FIRMWARE_BUSY"
    );
    assert_eq!(
        app.firmware.job(&owner("B"), &job.job_id).unwrap_err().code,
        "FORBIDDEN"
    );
    p.cancelled.cancel();
    app.firmware.stop();
    assert!(app.firmware.busy());
    fake.finish.notify_one();
    let result = terminal(&app, &owner("A"), &job.job_id).await;
    assert!(matches!(result.state, State::Completed));
    assert_eq!(result.uid, Some(b.uid));
    assert_eq!(result.generation, Some(b.generation));
    assert_eq!(fake.writes.load(Ordering::SeqCst), 1);
    assert!(result.result.unwrap().verified);
}
#[tokio::test]
async fn cancellation_before_inspection_never_programs() {
    let (_dir, app, fake) = setup(64, None);
    let p = owner("A");
    let job = app
        .firmware
        .start(
            app.clone(),
            app.active.bind().unwrap(),
            p.clone(),
            Target::Ms2130,
            "test".into(),
            image::prepare(Target::Ms2130, &[1; 8]).unwrap(),
        )
        .unwrap();
    app.firmware.cancel(&p, &job.job_id).unwrap();
    let result = terminal(&app, &p, &job.job_id).await;
    assert!(matches!(result.state, State::Cancelled));
    assert_eq!(fake.writes.load(Ordering::SeqCst), 0);
}
