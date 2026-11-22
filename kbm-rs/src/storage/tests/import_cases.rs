use super::*;
use crate::storage::import::Spec;
use std::io::Write;
pub(crate) fn iso() -> Vec<u8> {
    let mut b = vec![0; 40 * 2048];
    let p = &mut b[16 * 2048..17 * 2048];
    p[0] = 1;
    p[1..6].copy_from_slice(b"CD001");
    p[6] = 1;
    p[80..84].copy_from_slice(&40u32.to_le_bytes());
    p[84..88].copy_from_slice(&40u32.to_be_bytes());
    p[128..130].copy_from_slice(&2048u16.to_le_bytes());
    p[130..132].copy_from_slice(&2048u16.to_be_bytes());
    let p = &mut b[17 * 2048..18 * 2048];
    p[1..6].copy_from_slice(b"CD001");
    p[6] = 1;
    p[7..30].copy_from_slice(b"EL TORITO SPECIFICATION");
    p[71..75].copy_from_slice(&20u32.to_le_bytes());
    let p = &mut b[18 * 2048..19 * 2048];
    p[0] = 255;
    p[1..6].copy_from_slice(b"CD001");
    p[6] = 1;
    let p = &mut b[20 * 2048..21 * 2048];
    p[0] = 1;
    p[30] = 0x55;
    p[31] = 0xaa;
    p[28..30].copy_from_slice(&0x55aau16.to_le_bytes());
    p[32] = 0x88;
    p[38..40].copy_from_slice(&1u16.to_le_bytes());
    p[40..44].copy_from_slice(&21u32.to_le_bytes());
    b
}
fn optical_spec() -> Spec {
    Spec::Optical {
        name: "boot.iso".into(),
        size: 81920,
        confirmed: true,
        expected_card_uuid: Some("09".repeat(16)),
    }
}
fn file(bytes: &[u8]) -> std::fs::File {
    let mut f = tempfile::tempfile().unwrap();
    f.write_all(bytes).unwrap();
    f
}
async fn import(
    app: &Arc<AppService>,
    p: &Principal,
    fake: &Fake,
    spec: Spec,
    bytes: &[u8],
) -> JobInfo {
    let accepted = app
        .storage
        .start_import(
            app.active.bind().unwrap(),
            p.clone(),
            spec,
            file(bytes),
            fake.session.load(Ordering::Acquire),
        )
        .unwrap();
    wait(app, p, &accepted.job_id).await
}
#[test]
fn optical_profile_matches_v1_and_rejects_nonbootable_or_out_of_range_catalogs() {
    let good = iso();
    let inspect = |b: &[u8]| {
        crate::storage::optical::inspect(b.len() as u64, |lba| {
            Ok(b[lba as usize * 2048..(lba as usize + 1) * 2048]
                .try_into()
                .unwrap())
        })
    };
    inspect(&good).unwrap();
    for (at, value) in [
        (20 * 2048 + 32, 0),
        (20 * 2048 + 30, 0),
        (16 * 2048 + 84, 1),
        (18 * 2048, 0),
        (20 * 2048 + 40, 99),
    ] {
        let mut bad = good.clone();
        bad[at] = value;
        assert!(inspect(&bad).is_err(), "{at}");
    }
    let mut udf = vec![0; 600 * 2048];
    udf[19 * 2048..20 * 2048].copy_from_slice(&good[17 * 2048..18 * 2048]);
    udf[20 * 2048..21 * 2048].copy_from_slice(&good[20 * 2048..21 * 2048]);
    for (lba, id) in [(16, b"BEA01"), (17, b"NSR03"), (18, b"TEA01")] {
        udf[lba * 2048 + 1..lba * 2048 + 6].copy_from_slice(id);
        udf[lba * 2048 + 6] = 1;
    }
    let tag = &mut udf[256 * 2048..256 * 2048 + 16];
    tag[..2].copy_from_slice(&2u16.to_le_bytes());
    tag[12..16].copy_from_slice(&256u32.to_le_bytes());
    tag[4] = tag.iter().fold(0u8, |s, b| s.wrapping_add(*b));
    inspect(&udf).unwrap();
    udf[256 * 2048 + 4] ^= 1;
    assert!(inspect(&udf).is_err());
    assert!(crate::storage::optical::inspect(81921, |_| panic!()).is_err());
}
#[tokio::test]
async fn optical_import_checks_before_mutation_and_verifies_commit() {
    let (_dir, app, fake, p) = setup();
    let mut invalid = iso();
    invalid[20 * 2048 + 32] = 0;
    let bad = import(&app, &p, &fake, optical_spec(), &invalid).await;
    assert!(bad.state == JobState::Failed);
    assert!(fake.data.lock().calls.is_empty());
    assert!(!bad.side_effects_possible);
    let good = import(&app, &p, &fake, optical_spec(), &iso()).await;
    assert!(
        good.state == JobState::Completed,
        "{}",
        serde_json::to_string(&good).unwrap()
    );
    assert_eq!(
        good.result.unwrap()["image"]["crc32"],
        crc32fast::hash(&iso())
    );
    let d = fake.data.lock();
    assert_eq!(d.uploaded, iso());
    assert_eq!(d.upload_id, 0);
    assert!(d
        .calls
        .iter()
        .filter(|c| c.op == 0x302)
        .all(|c| c.bytes.len() <= 1020));
}
#[tokio::test]
async fn offset_failure_cancels_upload_without_committing() {
    let (_dir, app, fake, p) = setup();
    fake.bad_offset.store(true, Ordering::Release);
    let bad = import(&app, &p, &fake, optical_spec(), &iso()).await;
    assert!(bad.state == JobState::Failed);
    assert!(
        bad.cleanup_error.is_none(),
        "{}",
        serde_json::to_string(&bad).unwrap()
    );
    assert!(bad.side_effects_possible);
    let d = fake.data.lock();
    assert!(d.calls.iter().any(|c| c.op == 9 && c.cleanup));
    assert!(!d.calls.iter().any(|c| c.op == 0x303));
    assert_eq!(d.upload_id, 0);
}
#[tokio::test]
async fn sparse_disk_import_keeps_zero_tail_in_crc_and_closes_lease() {
    let (_dir, app, fake, p) = setup();
    let mut bytes = vec![0; 8 * 512];
    bytes[512] = 9;
    bytes[6 * 512 + 511] = 17;
    let spec = Spec::Disk {
        size: 4096,
        format: 2,
        replace_id: None,
        confirmed: true,
        expected_card_uuid: None,
    };
    let result = import(&app, &p, &fake, spec, &bytes).await;
    assert!(
        result.state == JobState::Completed,
        "{}",
        serde_json::to_string(&result).unwrap()
    );
    assert_eq!(
        fake.data
            .lock()
            .calls
            .iter()
            .filter(|c| c.op == 0x315)
            .count(),
        2
    );
    assert!(fake.data.lock().disk.is_none());
    assert!(app.storage.resources_info().is_none());
}
#[tokio::test]
async fn upload_tickets_enforce_key_size_single_use_session_and_stream_bounds() {
    let (_dir, app, fake, p) = setup();
    let prepared = app
        .storage
        .prepare_upload(
            app.active.bind().unwrap(),
            p.clone(),
            optical_spec(),
            "127.0.0.1:18180".parse().unwrap(),
        )
        .unwrap();
    let id = prepared["upload_id"].as_str().unwrap();
    let mut other = p.clone();
    other.key_id = "key-B".into();
    assert_eq!(
        app.storage
            .receive_upload(other, id, Some(81920), axum::body::Body::empty())
            .await
            .err()
            .unwrap()
            .code,
        "FORBIDDEN"
    );
    assert_eq!(
        app.storage
            .receive_upload(p.clone(), id, Some(12), axum::body::Body::empty())
            .await
            .err()
            .unwrap()
            .code,
        "UPLOAD_LENGTH_MISMATCH"
    );
    let accepted = app
        .storage
        .receive_upload(p.clone(), id, Some(81920), axum::body::Body::from(iso()))
        .await
        .unwrap();
    assert!(wait(&app, &p, &accepted.job_id).await.state == JobState::Completed);
    assert_eq!(
        app.storage
            .receive_upload(p.clone(), id, Some(81920), axum::body::Body::empty())
            .await
            .err()
            .unwrap()
            .code,
        "UPLOAD_NOT_FOUND"
    );
    for bytes in [vec![0; 81919], vec![0; 81921]] {
        let prepared = app
            .storage
            .prepare_upload(
                app.active.bind().unwrap(),
                p.clone(),
                optical_spec(),
                "127.0.0.1:18180".parse().unwrap(),
            )
            .unwrap();
        let before = fake.data.lock().calls.len();
        assert_eq!(
            app.storage
                .receive_upload(
                    p.clone(),
                    prepared["upload_id"].as_str().unwrap(),
                    Some(81920),
                    axum::body::Body::from(bytes)
                )
                .await
                .err()
                .unwrap()
                .code,
            "UPLOAD_LENGTH_MISMATCH"
        );
        assert_eq!(fake.data.lock().calls.len(), before);
    }
    let prepared = app
        .storage
        .prepare_upload(
            app.active.bind().unwrap(),
            p.clone(),
            optical_spec(),
            "127.0.0.1:18180".parse().unwrap(),
        )
        .unwrap();
    fake.session.fetch_add(1, Ordering::AcqRel);
    assert_eq!(
        app.storage
            .receive_upload(
                p,
                prepared["upload_id"].as_str().unwrap(),
                Some(81920),
                axum::body::Body::from(iso())
            )
            .await
            .err()
            .unwrap()
            .code,
        "STALE_SESSION"
    );
}
#[tokio::test]
async fn pending_rest_body_stops_on_key_revocation_before_any_device_write() {
    let (_dir, app, fake, p) = setup();
    let prepared = app
        .storage
        .prepare_upload(
            app.active.bind().unwrap(),
            p.clone(),
            optical_spec(),
            "127.0.0.1:18180".parse().unwrap(),
        )
        .unwrap();
    let id = prepared["upload_id"].as_str().unwrap().to_owned();
    let polled = Arc::new(Notify::new());
    let signal = polled.clone();
    let stream = futures_util::stream::poll_fn(move |_| {
        signal.notify_one();
        std::task::Poll::Pending::<Option<Result<axum::body::Bytes, std::io::Error>>>
    });
    let manager = app.storage.clone();
    let owner = p.clone();
    let task = tokio::spawn(async move {
        manager
            .receive_upload(
                owner,
                &id,
                Some(81920),
                axum::body::Body::from_stream(stream),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), polled.notified())
        .await
        .unwrap();
    p.cancelled.cancel();
    let error = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .err()
        .unwrap();
    assert_eq!(error.code, "UNAUTHORIZED");
    assert!(fake.data.lock().calls.is_empty());
}
#[test]
fn import_specs_never_accept_host_paths_uid_overrides_or_unconfirmed_mutations() {
    for mut value in [
        json!({"kind":"optical","name":"boot.iso","size":81920,"confirmed":true}),
        json!({"kind":"disk","format":1,"size":4096,"confirmed":true}),
    ] {
        serde_json::from_value::<Spec>(value.clone())
            .unwrap()
            .validate()
            .unwrap();
        for field in ["uid", "uids", "generation", "path", "filename"] {
            let mut invalid = value.clone();
            invalid[field] = json!("no");
            assert!(serde_json::from_value::<Spec>(invalid).is_err());
        }
        value["confirmed"] = json!(false);
        assert!(serde_json::from_value::<Spec>(value)
            .unwrap()
            .validate()
            .is_err());
    }
}
