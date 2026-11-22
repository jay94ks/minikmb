use super::*;
#[test]
fn capture_changes_log_first_frame_failure_stale_and_recovery_without_frame_content() {
    let journal = Journal::memory();
    let b = crate::active::Binding {
        uid: Uid::from_bytes(&[0x12; 12]).unwrap(),
        generation: 4,
    };
    let mut tracker = CaptureTracker::default();
    let mut status = crate::video::VideoStatus {
        running: true,
        ..Default::default()
    };
    tracker.observe(&journal, &b, &status, 2000);
    assert_eq!(
        journal.status().recent.last().unwrap().fields.code,
        Some("WAITING_FRAME")
    );
    status.frame = Some(crate::video::FrameInfo {
        uid: b.uid.clone(),
        generation: b.generation,
        frame_id: "4-1-1".into(),
        stream_id: "4-1".into(),
        width: 1920,
        height: 1080,
        fps: 30.0,
        received_at: 2000,
    });
    tracker.observe(&journal, &b, &status, 2000);
    tracker.observe(&journal, &b, &status, 2001);
    assert_eq!(journal.status().recent.len(), 2);
    tracker.observe(&journal, &b, &status, 3001);
    assert_eq!(
        journal.status().recent.last().unwrap().fields.code,
        Some("FRAME_STALE")
    );
    status.error = "private decoder details and source path must not be logged".into();
    tracker.observe(&journal, &b, &status, 3001);
    assert_eq!(
        journal.status().recent.last().unwrap().fields.code,
        Some("CAPTURE_FAILED")
    );
    status.error.clear();
    status.notice = "restored".into();
    tracker.observe(&journal, &b, &status, 2000);
    assert_eq!(
        journal.status().recent.last().unwrap().fields.code,
        Some("CAPTURE_RESTORED")
    );
    let text = serde_json::to_string(&journal.status()).unwrap();
    assert!(!text.contains("private decoder"));
    assert!(!text.contains("source path"));
    assert!(!text.contains("frame_id"));
}
fn fields() -> Fields {
    Fields {
        uid: Some(Uid::from_bytes(&[0x12; 12]).unwrap()),
        command: Some("wake_send"),
        success: Some(true),
        ..Default::default()
    }
}
#[test]
fn enabled_verbose_and_recent_pending_limits_preserve_control_progress() {
    let journal = Journal::memory();
    journal.emit(Level::Debug, Event::Command, fields());
    assert!(journal.status().recent.is_empty());
    for _ in 0..(PENDING + 2000) {
        journal.emit(Level::Info, Event::Command, fields());
    }
    let status = journal.status();
    assert_eq!(status.recent.len(), RECENT);
    assert_eq!(status.pending, PENDING);
    assert_eq!(status.dropped, 2000);
    assert!(status.recent.windows(2).all(|p| p[1].id > p[0].id));
    journal.flush().unwrap();
    assert_eq!(journal.status().pending, 0);
    journal.configure(Config {
        enabled: true,
        verbose: true,
    });
    journal.emit(Level::Debug, Event::Command, fields());
    assert_eq!(journal.status().recent.last().unwrap().level, Level::Debug);
    journal.configure(Config {
        enabled: false,
        verbose: true,
    });
    let last = journal.status().recent.last().unwrap().id;
    journal.emit(Level::Error, Event::Command, fields());
    assert_eq!(journal.status().recent.last().unwrap().id, last);
}
#[test]
fn jsonl_rotation_is_bounded_and_preserves_unrelated_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("keep.txt"), b"owned elsewhere").unwrap();
    let journal = Journal::make(Some(dir.path().to_owned()), 1024, 5, false);
    for _ in 0..100 {
        journal.emit(Level::Info, Event::Command, fields());
    }
    journal.flush().unwrap();
    let mut files = 0;
    let mut ids = vec![];
    for n in (0..5).rev() {
        let path = file(dir.path(), n);
        if path.exists() {
            files += 1;
            let bytes = std::fs::read(path).unwrap();
            assert!(bytes.len() <= 1024);
            for line in bytes.split(|b| *b == b'\n').filter(|line| !line.is_empty()) {
                let v: serde_json::Value = serde_json::from_slice(line).unwrap();
                ids.push(v["id"].as_u64().unwrap());
                assert_eq!(v["fields"]["command"], "wake_send");
            }
        }
    }
    assert_eq!(files, 5);
    assert!(ids.windows(2).all(|p| p[0] < p[1]));
    assert_eq!(*ids.last().unwrap(), 100);
    assert_eq!(
        std::fs::read(dir.path().join("keep.txt")).unwrap(),
        b"owned elsewhere"
    );
    assert!(!file(dir.path(), 5).exists());
}
#[test]
fn write_failure_reports_redacted_error_without_retrying_or_discarding_recent() {
    let dir = tempfile::tempdir().unwrap();
    let private = dir.path().join("do-not-expose-user-name");
    std::fs::write(&private, b"not a directory").unwrap();
    let journal = Journal::make(Some(private.clone()), FILE_BYTES, FILES, false);
    journal.emit(Level::Info, Event::AppStarted, fields());
    assert_eq!(journal.flush().unwrap_err().code, "LOG_IO");
    let status = journal.status();
    assert_eq!(status.dropped, 1);
    assert_eq!(status.pending, 0);
    assert_eq!(status.recent.len(), 1);
    assert!(!serde_json::to_string(&status)
        .unwrap()
        .contains("do-not-expose-user-name"));
    assert_eq!(std::fs::read(private).unwrap(), b"not a directory");
}
#[test]
fn other_instance_lock_drops_batch_without_overwriting_existing_log() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::make(Some(dir.path().to_owned()), FILE_BYTES, FILES, false);
    journal.emit(Level::Info, Event::AppStarted, fields());
    journal.flush().unwrap();
    let original = std::fs::read(file(dir.path(), 0)).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join("operations.lock"))
        .unwrap();
    lock.lock().unwrap();
    journal.emit(Level::Info, Event::AppStopped, fields());
    assert!(journal.flush().is_err());
    assert_eq!(std::fs::read(file(dir.path(), 0)).unwrap(), original);
    drop(lock);
    journal.emit(Level::Info, Event::AppStopped, fields());
    journal.flush().unwrap();
    assert!(journal.status().write_error.is_none());
    assert_eq!(journal.status().dropped, 1);
}
#[test]
fn worker_shutdown_flushes_once_and_rejects_new_events() {
    let dir = tempfile::tempdir().unwrap();
    let journal = Journal::new(dir.path().to_owned());
    for _ in 0..50 {
        journal.emit(Level::Info, Event::AppStarted, Fields::default());
    }
    journal.shutdown();
    let bytes = std::fs::read(file(dir.path(), 0)).unwrap();
    assert_eq!(
        bytes
            .split(|b| *b == b'\n')
            .filter(|b| !b.is_empty())
            .count(),
        50
    );
    journal.emit(Level::Info, Event::AppStopped, Fields::default());
    journal.shutdown();
    assert_eq!(std::fs::read(file(dir.path(), 0)).unwrap(), bytes);
}
#[test]
fn quiet_preview_and_successful_input_metadata_do_not_write_info_events() {
    let journal = Journal::memory();
    journal.command("hid_key_down", None, &Ok::<_, AppError>(()));
    assert!(journal.status().recent.is_empty());
    journal.command(
        "hid_key_down",
        None,
        &Err::<(), _>(AppError::uncertain(
            "HID_TIMEOUT",
            "private path and key usage ignored",
        )),
    );
    let text = serde_json::to_string(&journal.status()).unwrap();
    assert!(text.contains("HID_TIMEOUT"));
    assert!(!text.contains("private path"));
    assert!(!text.contains("key usage"));
    journal.quiet();
    let before = journal.status().recent.len();
    journal.emit(Level::Info, Event::AppStopped, Fields::default());
    assert_eq!(journal.status().recent.len(), before);
}
