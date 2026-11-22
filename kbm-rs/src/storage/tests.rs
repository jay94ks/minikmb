use super::*;
use crate::{
    service::AppService,
    storage_wire::{self as abi, Backend, Cleanup, Wire},
};
use futures_util::future::BoxFuture;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::Notify;
mod card_format_cases;
mod folder_cases;
mod format_cases;
pub(crate) mod import_cases;
mod media_cases;
#[derive(Clone)]
struct Call {
    uid: Uid,
    op: u16,
    bytes: Vec<u8>,
    cleanup: bool,
}
struct Mcu {
    id: u32,
    kind: u8,
    state: u8,
    payload: Vec<u8>,
    object: u32,
}
#[derive(Default)]
struct Data {
    calls: Vec<Call>,
    name: String,
    user: Vec<u8>,
    staged: Vec<u8>,
    revision: u32,
    job: Option<Mcu>,
    next: u32,
    catalog_cursor: u32,
    verified: Vec<String>,
    disk: Option<abi::Disk>,
    optical: Option<abi::Optical>,
    optical_off: bool,
    sectors: HashMap<u32, Vec<u8>>,
    read: Vec<u8>,
    read_job: u32,
    card_epoch: u8,
    upload_id: u32,
    upload_begin: Vec<u8>,
    uploaded: Vec<u8>,
    catalog_rows: Option<Vec<(u32, u32, u8)>>,
    deleted: Vec<u32>,
}
pub(crate) struct Fake {
    data: Mutex<Data>,
    session: AtomicU64,
    hold: AtomicBool,
    hold_start: AtomicBool,
    resume: Notify,
    bad_name: AtomicBool,
    bad_offset: AtomicBool,
    bad_cursor: AtomicBool,
    bad_start: AtomicBool,
    deny_cleanup: AtomicBool,
    uid_mismatch: AtomicBool,
    caps_unsupported: AtomicBool,
}
impl Default for Fake {
    fn default() -> Self {
        Self {
            data: Default::default(),
            session: AtomicU64::new(1),
            hold: AtomicBool::new(false),
            hold_start: AtomicBool::new(false),
            resume: Default::default(),
            bad_name: AtomicBool::new(false),
            bad_offset: AtomicBool::new(false),
            bad_cursor: AtomicBool::new(false),
            bad_start: AtomicBool::new(false),
            deny_cleanup: AtomicBool::new(false),
            uid_mismatch: AtomicBool::new(false),
            caps_unsupported: AtomicBool::new(false),
        }
    }
}
struct FakeWire {
    fake: Arc<Fake>,
    uid: Uid,
    session: u64,
    active: Arc<ActiveCoordinator>,
}
struct FakeBackend {
    fake: Arc<Fake>,
    active: Arc<ActiveCoordinator>,
}
impl Backend for FakeBackend {
    fn prepare_target_change(&self, binding: Binding) -> BoxFuture<'static, Result<(), AppError>> {
        let fake = self.fake.clone();
        Box::pin(async move {
            fake.data.lock().calls.push(Call {
                uid: binding.uid,
                op: 0xffff,
                bytes: vec![],
                cleanup: false,
            });
            Ok(())
        })
    }
    fn connect(&self, b: &Binding) -> Result<Arc<dyn Wire>, AppError> {
        Ok(Arc::new(FakeWire {
            fake: self.fake.clone(),
            uid: b.uid.clone(),
            session: self.fake.session.load(Ordering::Acquire),
            active: self.active.clone(),
        }))
    }
}
impl Fake {
    pub(crate) fn seed_disk(&self, id: u32, sectors: u32, format: u8) {
        self.data.lock().catalog_rows = Some(vec![(id, sectors, format)]);
    }
    fn command(
        &self,
        uid: Uid,
        op: u16,
        payload: Vec<u8>,
        cleanup: bool,
    ) -> Result<Vec<u8>, AppError> {
        let mut d = self.data.lock();
        d.calls.push(Call {
            uid: uid.clone(),
            op,
            bytes: payload.clone(),
            cleanup,
        });
        match op {
            0x308 => {
                let mut b = vec![0u8; 52];
                b[0] = 7;
                b[2..4].copy_from_slice(&(d.user.len() as u16).to_le_bytes());
                b[4..8].copy_from_slice(&d.revision.to_le_bytes());
                b[8..24].fill(9 + d.card_epoch);
                let bytes: Vec<_> = uid
                    .raw()
                    .as_bytes()
                    .chunks_exact(2)
                    .map(|h| u8::from_str_radix(std::str::from_utf8(h).unwrap(), 16).unwrap())
                    .collect();
                b[24..36].copy_from_slice(&bytes);
                b[36..48].copy_from_slice(&bytes);
                if self.uid_mismatch.load(Ordering::Acquire) {
                    b[24] ^= 1;
                }
                Ok(b)
            }
            0x31e => {
                let name = if self.bad_name.load(Ordering::Acquire) {
                    "different"
                } else {
                    &d.name
                };
                let mut b = (name.len() as u16).to_le_bytes().to_vec();
                b.extend_from_slice(&[0, 0]);
                b.extend_from_slice(name.as_bytes());
                Ok(b)
            }
            0x305 => {
                let o = d.optical.as_ref().ok_or_else(|| AppError::mcu(17))?;
                let mut b = vec![0; 48];
                b[0..4].copy_from_slice(&o.id.to_le_bytes());
                b[4..12].copy_from_slice(&o.size.to_le_bytes());
                b[13] = u8::from(o.mounted);
                b[14..16].copy_from_slice(&(o.name.len() as u16).to_le_bytes());
                b[16..20].copy_from_slice(&o.crc.to_le_bytes());
                b.extend_from_slice(o.name.as_bytes());
                Ok(b)
            }
            0x318 => {
                let disk = d.disk.as_ref().ok_or_else(|| AppError::mcu(17))?;
                let mut b = abi::words(&[disk.id, disk.sectors, disk.lease]);
                b.extend_from_slice(&[disk.format, disk.state, u8::from(disk.writable), 0]);
                Ok(b)
            }
            0x319 => {
                if self.caps_unsupported.load(Ordering::Acquire) {
                    Err(AppError::mcu(1))
                } else {
                    let enabled = u8::from(!d.optical_off);
                    Ok(vec![7, 0, 0, 0, enabled, enabled, 0, 0])
                }
            }
            0x31a => {
                assert!(d.disk.is_none());
                assert!(d.optical.as_ref().is_none_or(|o| !o.mounted));
                assert_eq!(payload.len(), 1);
                d.optical_off = payload[0] == 0;
                Ok(vec![])
            }
            4 => {
                let mut b = vec![0; 64];
                b[13] = if d.upload_id == 0 { 1 } else { 3 };
                if let Some(job) = d.job.as_ref().filter(|j| j.kind == 14 && j.state == 0) {
                    b[13] = 10;
                    b[16..20].copy_from_slice(&job.id.to_le_bytes());
                }
                b[24..28].copy_from_slice(&d.upload_id.to_le_bytes());
                Ok(b)
            }
            9 => {
                assert!(cleanup);
                assert_eq!(abi::u32at(&payload, 0), d.upload_id);
                d.upload_id = 0;
                d.uploaded.clear();
                Ok(vec![])
            }
            0x302 => {
                assert_eq!(abi::u32at(&payload, 0), d.upload_id);
                assert_eq!(abi::u64at(&payload, 4), d.uploaded.len() as u64);
                let size = abi::u64at(&d.upload_begin, 0);
                assert_eq!(
                    payload.len() - 12,
                    ((size - d.uploaded.len() as u64).min(1008)) as usize
                );
                d.uploaded.extend_from_slice(&payload[12..]);
                let at = d.uploaded.len() as u64;
                Ok(if self.bad_offset.load(Ordering::Acquire) {
                    at - 1
                } else {
                    at
                }
                .to_le_bytes()
                .to_vec())
            }
            0x316 => {
                assert_eq!(payload.len(), 9);
                assert_eq!(abi::u32at(&payload, 0), d.disk.as_ref().unwrap().lease);
                assert_eq!(abi::u32at(&payload, 4), d.read_job);
                let at = usize::from(payload[8]) * 256;
                Ok(d.read[at..at + 256].to_vec())
            }
            0x31f
            | 0x31b
            | 0x30d
            | 0x30f
            | 0x301
            | 0x303
            | 0x306
            | 0x307
            | 0x310..=0x315
            | 0x317
            | 0x31d => {
                if cleanup && self.deny_cleanup.load(Ordering::Acquire) {
                    return Err(AppError::mcu(3));
                }
                if d.job.as_ref().is_some_and(|j| j.state == 0) {
                    return Err(AppError::mcu(4));
                }
                d.next += 1;
                let id = d.next;
                let kind = match op {
                    0x31f => 17,
                    0x31b => 15,
                    0x30f => 14,
                    0x306 => 3,
                    0x307 => 4,
                    0x301 => 1,
                    0x303 => 2,
                    0x310 => 7,
                    0x311 => 8,
                    0x31d => 16,
                    0x312 => 9,
                    0x313 => 10,
                    0x314 => 11,
                    0x315 => 12,
                    0x317 => 13,
                    _ => 6,
                };
                let object = match kind {
                    1 => id,
                    2 => 200 + id,
                    7 | 9 => 1000 + id,
                    _ => 0,
                };
                if kind == 15 {
                    d.catalog_cursor = abi::u32at(&payload, 0);
                }
                d.job = Some(Mcu {
                    id,
                    kind,
                    state: 0,
                    payload,
                    object,
                });
                if self.bad_start.load(Ordering::Acquire) {
                    Ok(vec![])
                } else {
                    Ok(id.to_le_bytes().to_vec())
                }
            }
            7 => {
                let mut job = d.job.take().ok_or_else(|| AppError::mcu(17))?;
                assert_eq!(job.id, abi::u32at(&payload, 0));
                if job.state == 0 && !self.hold.load(Ordering::Acquire) {
                    match job.kind {
                        14 => {
                            assert_eq!(job.payload.len(), 4);
                            assert!([1, 2].contains(&job.payload[0]));
                            assert_eq!(job.payload[1..], [0, 0, 0]);
                            assert!(d.disk.is_none());
                            assert!(d.optical.as_ref().is_none_or(|o| !o.mounted));
                            d.name.clear();
                            d.user.clear();
                            d.card_epoch += 1;
                            d.optical = None;
                            d.catalog_rows = Some(vec![]);
                            d.sectors.clear();
                            job.object = job.id;
                        }
                        1 => {
                            d.upload_id = job.id;
                            d.upload_begin = job.payload.clone();
                            d.uploaded.clear();
                            d.optical = None;
                        }
                        2 => {
                            assert_eq!(d.uploaded.len() as u64, abi::u64at(&d.upload_begin, 0));
                            assert_eq!(
                                crc32fast::hash(&d.uploaded),
                                abi::u32at(&d.upload_begin, 8)
                            );
                            d.optical = Some(abi::Optical {
                                id: job.object,
                                size: d.uploaded.len() as u64,
                                mounted: false,
                                name: String::from_utf8(d.upload_begin[44..].to_vec()).unwrap(),
                                crc: abi::u32at(&d.upload_begin, 8),
                            });
                            d.upload_id = 0;
                        }
                        7 => {
                            if d.catalog_rows.is_none() {
                                d.catalog_rows = Some((1..=6).map(|id| (id, 128, 1)).collect());
                            }
                            d.sectors.clear();
                            d.disk = Some(abi::Disk {
                                id: abi::u32at(&job.payload, 0),
                                sectors: abi::u32at(&job.payload, 4),
                                lease: job.object,
                                format: job.payload[8],
                                state: 2,
                                writable: true,
                            });
                        }
                        8 => {
                            let disk = d.disk.as_ref().unwrap();
                            assert_eq!(disk.lease, abi::u32at(&job.payload, 0));
                            let mut checksum = crc32fast::Hasher::new();
                            for lba in 0..disk.sectors {
                                checksum.update(
                                    d.sectors
                                        .get(&lba)
                                        .map(|v| v.as_slice())
                                        .unwrap_or(&[0; 512]),
                                );
                            }
                            assert_eq!(checksum.finalize(), abi::u32at(&job.payload, 4));
                            let row = (disk.id, disk.sectors, disk.format);
                            if let Some(rows) = d.catalog_rows.as_mut() {
                                rows.retain(|r| r.0 != row.0);
                                rows.push(row);
                            }
                            d.disk = None;
                        }
                        17 => d.name = String::from_utf8(job.payload[2..].to_vec()).unwrap(),
                        6 => {
                            d.user = d.staged.clone();
                            d.revision += 1;
                        }
                        3 | 4 => {
                            let optical = d.optical.as_mut().unwrap();
                            assert_eq!(abi::u32at(&job.payload, 0), optical.id);
                            optical.mounted = job.kind == 3;
                        }
                        9 => {
                            assert!(d.disk.is_none());
                            assert_eq!(job.payload.len(), 8);
                            let id = abi::u32at(&job.payload, 0);
                            let row = d
                                .catalog_rows
                                .as_ref()
                                .and_then(|rows| rows.iter().find(|r| r.0 == id))
                                .copied()
                                .unwrap_or((id, 128, 1));
                            d.disk = Some(abi::Disk {
                                id,
                                sectors: row.1,
                                lease: job.object,
                                format: row.2,
                                state: 3 + job.payload[5],
                                writable: job.payload[4] != 0,
                            });
                        }
                        10..=13 => {
                            let disk = d.disk.as_ref().unwrap();
                            assert_eq!(abi::u32at(&job.payload, 0), disk.lease);
                            if job.kind != 10 {
                                assert!(disk.state == 3 || job.kind == 12 && disk.state == 2);
                            }
                            match job.kind {
                                10 => d.disk = None,
                                11 => {
                                    let lba = abi::u32at(&job.payload, 4);
                                    assert!(lba < disk.sectors);
                                    d.read = d
                                        .sectors
                                        .get(&lba)
                                        .cloned()
                                        .unwrap_or_else(|| vec![0; 512]);
                                    d.read_job = job.id;
                                }
                                12 => {
                                    assert!(disk.writable);
                                    assert_eq!(job.payload.len(), 520);
                                    let lba = abi::u32at(&job.payload, 4);
                                    assert!(lba < disk.sectors);
                                    d.sectors.insert(lba, job.payload[8..].to_vec());
                                }
                                _ => {}
                            }
                        }
                        16 => {
                            let id = abi::u32at(&job.payload, 0);
                            d.deleted.push(id);
                            if let Some(rows) = &mut d.catalog_rows {
                                rows.retain(|r| r.0 != id);
                            }
                        }
                        _ => {}
                    }
                    job.state = 1;
                }
                let mut b = vec![0u8; 32];
                b[..4].copy_from_slice(&job.id.to_le_bytes());
                b[4] = job.state;
                b[5] = job.kind;
                b[12..16].copy_from_slice(&job.object.to_le_bytes());
                d.job = Some(job);
                Ok(b)
            }
            8 => {
                assert!(cleanup);
                if self.deny_cleanup.load(Ordering::Acquire) {
                    return Err(AppError::mcu(3));
                }
                let j = d.job.as_mut().unwrap();
                assert_ne!(
                    j.kind, 14,
                    "accepted card format must never receive SYS_CANCEL"
                );
                assert_eq!(abi::u32at(&payload, 0), j.id);
                j.state = 3;
                Ok(vec![])
            }
            0x31c => {
                if let Some(rows) = &d.catalog_rows {
                    let at = d.catalog_cursor as usize;
                    let count = (rows.len().saturating_sub(at)).min(4);
                    let next = if at + count < rows.len() {
                        (at + count) as u32
                    } else {
                        0
                    };
                    let mut b = abi::words(&[next]);
                    b.extend_from_slice(&[count as u8, 0, 0, 0]);
                    for &(id, sectors, format) in rows.iter().skip(at).take(count) {
                        b.extend(abi::words(&[id, sectors]));
                        b.extend_from_slice(&[format, 0, 0, 0]);
                    }
                    return Ok(b);
                }
                let cursor = d.catalog_cursor;
                let count = if cursor == 0 { 4 } else { 2 };
                let next = if cursor == 0 {
                    4
                } else if self.bad_cursor.load(Ordering::Acquire) {
                    1
                } else {
                    0
                };
                let mut b = abi::words(&[next]);
                b.extend_from_slice(&[count, 0, 0, 0]);
                for n in 0..u32::from(count) {
                    b.extend(abi::words(&[cursor + n + 1, 128]));
                    b.extend_from_slice(&[1, 0, 0, 0]);
                }
                Ok(b)
            }
            0x30a => {
                assert_eq!(abi::u32at(&payload, 0), d.revision);
                let at = usize::from(abi::u16at(&payload, 4));
                let count = usize::from(abi::u16at(&payload, 6));
                assert!((1..=256).contains(&count));
                Ok(d.user[at..at + count].to_vec())
            }
            0x30b => {
                assert!(abi::u16at(&payload, 0) <= 4096);
                d.staged.clear();
                Ok(77u32.to_le_bytes().to_vec())
            }
            0x30c => {
                assert_eq!(abi::u32at(&payload, 0), 77);
                assert_eq!(usize::from(abi::u16at(&payload, 4)), d.staged.len());
                assert!(payload.len() <= 1014);
                d.staged.extend_from_slice(&payload[6..]);
                let at = d.staged.len() as u16;
                Ok(if self.bad_offset.load(Ordering::Acquire) {
                    at - 1
                } else {
                    at
                }
                .to_le_bytes()
                .to_vec())
            }
            0x30e => {
                assert!(cleanup);
                assert_eq!(abi::u32at(&payload, 0), 77);
                d.staged.clear();
                Ok(vec![])
            }
            _ => panic!("unimplemented fake opcode {op:x}"),
        }
    }
}
impl Wire for FakeWire {
    fn session(&self) -> u64 {
        self.session
    }
    fn live(&self) -> bool {
        self.fake.session.load(Ordering::Acquire) == self.session
    }
    fn send(
        &self,
        b: Binding,
        p: Principal,
        cancel: CancellationToken,
        _ch: u8,
        op: u16,
        payload: Vec<u8>,
    ) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
        let fake = self.fake.clone();
        let uid = self.uid.clone();
        let active = self.active.clone();
        let session = self.session;
        Box::pin(async move {
            active.validate(&b)?;
            check_owner(&p)?;
            if cancel.is_cancelled() {
                return Err(AppError::new("OPERATION_CANCELLED", "cancelled"));
            }
            if fake.session.load(Ordering::Acquire) != session {
                return Err(AppError::new("STALE_SESSION", "new session"));
            }
            let result = fake.command(uid, op, payload, false);
            if [
                0x31f, 0x31b, 0x30d, 0x30f, 0x306, 0x307, 0x312, 0x313, 0x314, 0x315, 0x317,
            ]
            .contains(&op)
                && fake.hold_start.load(Ordering::Acquire)
            {
                fake.resume.notified().await;
            }
            result
        })
    }
    fn cleanup(&self, kind: Cleanup, op: u16) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
        let fake = self.fake.clone();
        let uid = self.uid.clone();
        let session = self.session;
        Box::pin(async move {
            if fake.session.load(Ordering::Acquire) != session {
                return Err(AppError::new("STALE_SESSION", "new session"));
            }
            let (ch, payload) = match kind {
                Cleanup::Job(id) | Cleanup::Upload(id) => (0, abi::words(&[id])),
                Cleanup::User(id) | Cleanup::Disk(id) | Cleanup::Optical(id) => {
                    (3, abi::words(&[id]))
                }
                Cleanup::MediaInfo => (3, vec![]),
                Cleanup::UploadStatus => (0, vec![]),
                Cleanup::CardFormat(id) => match op {
                    7 => (0, abi::words(&[id])),
                    4 => (0, vec![]),
                    _ => (3, vec![]),
                },
            };
            assert!(kind.matches(ch, op, &payload));
            fake.command(uid, op, payload, true)
        })
    }
    fn verified_name(&self, name: &str) -> Result<(), AppError> {
        self.fake.data.lock().verified.push(name.into());
        Ok(())
    }
}
pub(crate) fn install(app: &mut Arc<AppService>) -> Arc<Fake> {
    let fake = Arc::new(Fake::default());
    let mut manager = Manager::new(
        app.active.clone(),
        Arc::new(FakeBackend {
            fake: fake.clone(),
            active: app.active.clone(),
        }),
    );
    Arc::get_mut(&mut manager).unwrap().formatter = Arc::new(format_cases::FakeFormat::default());
    manager.set_logger(&app.logs);
    Arc::get_mut(app).unwrap().storage = manager;
    fake
}
fn setup() -> (tempfile::TempDir, Arc<AppService>, Arc<Fake>, Principal) {
    let (dir, mut app, _) = crate::input_api::tests::setup();
    let fake = install(&mut app);
    let p = Principal {
        key_id: "key-A".into(),
        client_id: "rest".into(),
        cancelled: Default::default(),
        ended: Default::default(),
    };
    (dir, app, fake, p)
}
pub(crate) async fn wait(app: &AppService, p: &Principal, id: &str) -> JobInfo {
    app.storage.job(p, id).unwrap();
    let job = app
        .storage
        .state
        .lock()
        .jobs
        .iter()
        .find(|job| job.info.lock().job_id == id)
        .unwrap()
        .clone();
    // Observe the real terminal signal instead of spin-polling on every executor turn.
    // Cleanup itself may take five seconds; a three-second test deadline was shorter.
    if tokio::time::timeout(Duration::from_secs(10), job.wait())
        .await
        .is_err()
    {
        panic!(
            "storage job did not finish: {}",
            serde_json::to_string(&job.snapshot()).unwrap()
        );
    }
    app.storage.job(p, id).unwrap()
}
async fn action(app: &Arc<AppService>, p: &Principal, action: Action) -> JobInfo {
    let id = app
        .storage
        .start(app.active.bind().unwrap(), p.clone(), action)
        .unwrap()
        .job_id;
    wait(app, p, &id).await
}
#[tokio::test]
async fn catalog_paging_bounds_optional_slots_and_no_internal_files() {
    let (_dir, app, fake, p) = setup();
    let info = action(&app, &p, Action::Refresh {}).await;
    assert!(info.state == JobState::Completed);
    let snapshot = app.storage.cached(&app.active.bind().unwrap()).unwrap();
    assert_eq!(snapshot.images.len(), 6);
    assert_eq!(snapshot.images[5].id, 6);
    assert!(snapshot
        .images
        .iter()
        .all(|i| i.kind == "disk" && i.name.ends_with(".IMG") && !i.name.contains("BOARD")));
    assert_eq!(
        fake.data
            .lock()
            .calls
            .iter()
            .filter(|c| c.op == 0x31b)
            .count(),
        2
    );
    fake.bad_cursor.store(true, Ordering::Release);
    let bad = action(&app, &p, Action::Refresh {}).await;
    assert!(bad.state == JobState::Failed);
    assert_eq!(bad.error.unwrap().code, "INVALID_RESPONSE");
    assert!(
        app.storage
            .cached(&app.active.bind().unwrap())
            .unwrap()
            .stale
    );
    fake.caps_unsupported.store(true, Ordering::Release);
    let recovered = action(&app, &p, Action::Refresh {}).await;
    assert!(recovered.state == JobState::Completed);
    assert!(recovered.result.unwrap()["images"]
        .as_array()
        .unwrap()
        .is_empty());
}
#[tokio::test]
async fn names_are_nfc_verified_and_do_not_query_or_unmount_images() {
    let (_dir, app, fake, p) = setup();
    let info = action(
        &app,
        &p,
        Action::NameSet {
            name: " \u{1100}\u{1161} 채널 ".into(),
        },
    )
    .await;
    assert!(
        info.state == JobState::Completed,
        "{}",
        serde_json::to_string(&info).unwrap()
    );
    assert_eq!(info.result.unwrap()["name"], "가 채널");
    assert_eq!(fake.data.lock().verified, vec!["가 채널"]);
    assert!(fake
        .data
        .lock()
        .calls
        .iter()
        .all(|c| [0x308, 0x31f, 7, 0x31e].contains(&c.op)));
    fake.bad_name.store(true, Ordering::Release);
    let bad = action(&app, &p, Action::NameSet { name: "new".into() }).await;
    assert!(bad.state == JobState::Failed);
    assert_eq!(bad.error.unwrap().code, "VERIFY_FAILED");
    assert_eq!(fake.data.lock().verified.len(), 1);
    fake.uid_mismatch.store(true, Ordering::Release);
    let count = fake.data.lock().calls.len();
    let bad = action(
        &app,
        &p,
        Action::NameSet {
            name: "other".into(),
        },
    )
    .await;
    assert_eq!(bad.error.unwrap().code, "CARD_UID_MISMATCH");
    assert!(fake.data.lock().calls[count..]
        .iter()
        .all(|c| c.op == 0x308));
}
#[tokio::test]
async fn four_kib_values_and_empty_write_roundtrip_with_checked_offsets() {
    let (_dir, app, fake, p) = setup();
    let bytes: Vec<_> = (0..4096).map(|n| (n % 256) as u8).collect();
    let info = action(
        &app,
        &p,
        Action::UserWrite {
            bytes: bytes.clone(),
        },
    )
    .await;
    assert!(
        info.state == JobState::Completed,
        "{}",
        serde_json::to_string(&info).unwrap()
    );
    assert_eq!(info.result.unwrap()["verified"], true);
    let read = action(&app, &p, Action::UserRead {}).await;
    let card = app
        .storage
        .cached(&app.active.bind().unwrap())
        .unwrap()
        .card
        .unwrap();
    assert_eq!(card.revision, 1);
    assert_eq!(card.user_length, 4096);
    assert_eq!(
        serde_json::from_value::<Vec<u8>>(read.result.unwrap()["bytes"].clone()).unwrap(),
        bytes
    );
    assert_eq!(
        fake.data
            .lock()
            .calls
            .iter()
            .filter(|c| c.op == 0x30c)
            .map(|c| c.bytes.len() - 6)
            .collect::<Vec<_>>(),
        vec![1008, 1008, 1008, 1008, 64]
    );
    let cleared = action(&app, &p, Action::UserWrite { bytes: vec![] }).await;
    assert!(cleared.state == JobState::Completed);
    assert!(fake.data.lock().user.is_empty());
    fake.bad_offset.store(true, Ordering::Release);
    let bad = action(
        &app,
        &p,
        Action::UserWrite {
            bytes: vec![99; 300],
        },
    )
    .await;
    assert!(bad.state == JobState::Failed);
    assert!(fake
        .data
        .lock()
        .calls
        .iter()
        .any(|c| c.op == 0x30e && c.cleanup));
    assert!(fake.data.lock().user.is_empty());
}
#[tokio::test]
async fn cancel_during_acceptance_cleans_original_job_before_channel_switch() {
    let (_dir, app, fake, p) = setup();
    fake.hold_start.store(true, Ordering::Release);
    fake.hold.store(true, Ordering::Release);
    let job = app
        .storage
        .start(
            app.active.bind().unwrap(),
            p.clone(),
            Action::NameSet {
                name: "pending".into(),
            },
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while fake.data.lock().job.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let app2 = app.clone();
    let switch =
        tokio::spawn(async move { app2.select(Some(Uid::from_bytes(&[2; 12]).unwrap())).await });
    tokio::time::timeout(Duration::from_secs(2), async {
        while !app
            .storage
            .state
            .lock()
            .current
            .as_ref()
            .unwrap()
            .cancel
            .is_cancelled()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        app.active.state().uid,
        Some(Uid::from_bytes(&[1; 12]).unwrap())
    );
    assert!(app.active.state().switching);
    fake.hold_start.store(false, Ordering::Release);
    fake.resume.notify_waiters();
    switch.await.unwrap().unwrap();
    let result = wait(&app, &p, &job.job_id).await;
    assert!(result.state == JobState::Cancelled);
    assert!(result.cleanup_error.is_none());
    let calls = fake.data.lock();
    assert!(calls
        .calls
        .iter()
        .filter(|c| c.cleanup)
        .all(|c| c.uid == Uid::from_bytes(&[1; 12]).unwrap()));
    assert!(calls.calls.iter().any(|c| c.cleanup && c.op == 8));
    assert!(calls.name.is_empty());
}
#[tokio::test]
async fn unknown_job_ack_quarantines_only_same_session_and_retains_error() {
    let (_dir, app, fake, p) = setup();
    fake.bad_start.store(true, Ordering::Release);
    let bad = action(
        &app,
        &p,
        Action::NameSet {
            name: "unknown".into(),
        },
    )
    .await;
    assert!(bad.cleanup_error.is_some());
    assert!(bad.side_effects_possible);
    assert_eq!(
        app.storage
            .start(app.active.bind().unwrap(), p.clone(), Action::Refresh {})
            .err()
            .unwrap()
            .code,
        "STORAGE_UNCERTAIN"
    );
    assert!(action(&app, &p, Action::CardInfo {}).await.state == JobState::Completed);
    assert_eq!(
        app.select(Some(Uid::from_bytes(&[2; 12]).unwrap()))
            .await
            .err()
            .unwrap()
            .code,
        "STORAGE_UNCERTAIN"
    );
    assert_eq!(
        app.active.bind().unwrap().uid,
        Uid::from_bytes(&[1; 12]).unwrap()
    );
    fake.session.store(2, Ordering::Release);
    fake.data.lock().job = None;
    fake.bad_start.store(false, Ordering::Release);
    assert!(action(&app, &p, Action::Refresh {}).await.state == JobState::Completed);
}
#[tokio::test]
async fn bounded_history_key_scope_and_busy_admission() {
    let (_dir, app, fake, p) = setup();
    fake.hold.store(true, Ordering::Release);
    let pending = app
        .storage
        .start(
            app.active.bind().unwrap(),
            p.clone(),
            Action::NameSet {
                name: "busy".into(),
            },
        )
        .unwrap();
    assert_eq!(
        app.storage
            .start(app.active.bind().unwrap(), p.clone(), Action::CardInfo {})
            .err()
            .unwrap()
            .code,
        "STORAGE_BUSY"
    );
    fake.hold.store(false, Ordering::Release);
    wait(&app, &p, &pending.job_id).await;
    let mut mcp = p.clone();
    mcp.client_id = "mcp:session".into();
    assert!(app.storage.job(&mcp, &pending.job_id).is_ok());
    let mut other = p.clone();
    other.key_id = "key-B".into();
    assert_eq!(
        app.storage.job(&other, &pending.job_id).err().unwrap().code,
        "FORBIDDEN"
    );
    for _ in 0..66 {
        assert!(action(&app, &p, Action::CardInfo {}).await.state == JobState::Completed);
    }
    assert_eq!(app.storage.state.lock().jobs.len(), 64);
    assert_eq!(
        app.storage.job(&p, &pending.job_id).err().unwrap().code,
        "JOB_NOT_FOUND"
    );
}
