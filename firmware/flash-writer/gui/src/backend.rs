use crate::{model::*, native, protocol};
use anyhow::{ensure, Context, Result};
use serde_json::json;
use std::{fs::{self, OpenOptions}, io::Write, path::{Path, PathBuf},
    sync::{atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, SyncSender}, Arc, Mutex},
    thread, time::{Duration, SystemTime, UNIX_EPOCH}};

pub type DemoFlash = Arc<Mutex<Vec<u8>>>;
pub fn demo_flash() -> DemoFlash { demo_with_capacity(DEMO_CAPACITY) }
pub fn demo_with_capacity(capacity: usize) -> DemoFlash {
    Arc::new(Mutex::new((0..capacity).map(|i| ((i*31 + i/257) % 256) as u8).collect()))
}

pub enum Event {
    Log(String),
    Progress { phase: String, fraction: Option<f32> },
    CommitStarted,
    Done { chip: Chip, read: Option<ReadData>, message: String, folder: PathBuf },
    Saved { path: PathBuf },
    SaveFailed { message: String },
    Failed { message: String, folder: PathBuf },
}
pub struct Worker { pub rx: Receiver<Event>, pub cancel: Arc<AtomicBool> }

fn stamp() -> u128 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() }
fn log(tx: &SyncSender<Event>, message: impl Into<String>) { let _ = tx.try_send(Event::Log(message.into())); }
fn progress(tx: &SyncSender<Event>, phase: &str, fraction: Option<f32>) {
    let _ = tx.try_send(Event::Progress { phase: phase.into(), fraction });
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "작업이 취소되었습니다. 변경 시작 전 중단했습니다.");
    Ok(())
}
fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)
        .with_context(|| format!("새 파일 저장 실패(기존 파일 덮어쓰기 금지): {}", path.display()))?;
    file.write_all(data)?; file.sync_all()?; Ok(())
}
fn save_output(path: &Path, data: &[u8], cancel: &AtomicBool, tx: &SyncSender<Event>) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "파일 저장을 취소했습니다.");
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    for (i, chunk) in data.chunks(65536).enumerate() {
        ensure!(!cancel.load(Ordering::Relaxed), "파일 저장을 취소했습니다.");
        staged.write_all(chunk)?;
        progress(tx, "메모리 버퍼 저장", Some(((i*65536 + chunk.len()) as f32 / data.len().max(1) as f32).min(1.0)));
    }
    staged.as_file().sync_all()?;
    ensure!(!cancel.load(Ordering::Relaxed), "파일 저장을 취소했습니다.");
    staged.persist_noclobber(path).with_context(|| format!("새 파일 저장 실패: {}", path.display()))?;
    Ok(())
}

/// Save the captured bytes; no device or programmer session is opened here.
pub fn start_save(data: ReadData, output: PathBuf) -> Worker {
    let (tx, rx) = mpsc::sync_channel(256);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    thread::spawn(move || {
        let result = save_output(&output, &data.data, &worker_cancel, &tx);
        let _ = tx.send(match result {
            Ok(()) => Event::Saved { path: output },
            Err(e) => Event::SaveFailed { message: format!("저장 실패: {e:#}\n읽어둔 메모리 버퍼는 유지됩니다.") },
        });
    });
    Worker { rx, cancel }
}

pub fn start(request: Request, demo: DemoFlash) -> Worker {
    let (tx, rx) = mpsc::sync_channel(256);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    thread::spawn(move || {
        let folder = request.data_dir.join("jobs").join(format!("{}-{}", stamp(), std::process::id()));
        if let Err(e) = fs::create_dir_all(&folder) {
            let _ = tx.send(Event::Failed { message: format!("작업 폴더 생성 실패: {e}"), folder }); return;
        }
        let mut report = json!({"started_unix_ns": stamp(), "device": request.device, "model": request.model,
            "spi_requested_hz": request.hz, "action": request.action, "backup_requested": request.backup,
            "engine": "embedded libflashrom 1.8.0 / serprog only", "mode": if request.demo {"demo"} else {"hardware"}});
        let result = execute(&request, &folder, &worker_cancel, &tx, &demo, &mut report);
        report["finished_unix_ns"] = json!(stamp());
        report["status"] = json!(if result.is_ok() {"passed"} else {"failed"});
        if let Err(e) = &result { report["error"] = json!(format!("{e:#}")); }
        if let Err(e) = write_new(&folder.join("report.json"), &serde_json::to_vec_pretty(&report).unwrap()) {
            log(&tx, format!("보고서 저장 실패: {e:#}"));
        }
        let _ = tx.send(match result {
            Ok((chip, read, message)) => Event::Done { chip, read, message, folder },
            Err(e) => Event::Failed { message: format!("{e:#}\n작업 로그: {}", folder.display()), folder },
        });
    });
    Worker { rx, cancel }
}

enum Engine<'a> { Demo(&'a DemoFlash), Native(native::Session) }
impl Engine<'_> {
    fn read(&mut self, region: Region, cancel: Option<&AtomicBool>, tx: &SyncSender<Event>) -> Result<Vec<u8>> {
        match self {
            Self::Native(session) => session.read(region, cancel),
            Self::Demo(flash) => {
                for i in 0..=10 {
                    if let Some(flag) = cancel { cancelled(flag)?; }
                    progress(tx, "데모 읽기", Some(i as f32/10.0)); thread::sleep(Duration::from_millis(3));
                }
                Ok(flash.lock().unwrap()[region.start..region.end()].to_vec())
            }
        }
    }
    fn write(&mut self, target: &mut [u8]) -> Result<()> {
        match self { Self::Demo(flash) => { *flash.lock().unwrap() = target.to_vec(); Ok(()) },
            Self::Native(session) => session.write(target) }
    }
    fn erase_all(&mut self, value: u8) -> Result<()> {
        match self { Self::Demo(flash) => { flash.lock().unwrap().fill(value); Ok(()) },
            Self::Native(session) => session.erase_all() }
    }
}

fn guard_target(expected: Option<&Chip>, actual: &Chip) -> Result<()> {
    if let Some(expected) = expected {
        ensure!(actual.device == expected.device && actual.name == expected.name && actual.capacity == expected.capacity,
            "확인 후 대상 또는 칩 정의가 바뀌었습니다. 다시 식별하세요.");
        if expected.jedec.len() == 8 && actual.jedec.len() == 8 {
            ensure!(actual.jedec == expected.jedec, "확인 후 JEDEC ID가 바뀌었습니다.");
        }
    }
    Ok(())
}

fn execute(request: &Request, folder: &Path, cancel: &AtomicBool, tx: &SyncSender<Event>, demo: &DemoFlash,
    report: &mut serde_json::Value) -> Result<(Chip, Option<ReadData>, String)> {
    ensure!((100000..=12000000).contains(&request.hz), "SPI 요청은 100 kHz~12 MHz 범위여야 합니다.");
    cancelled(cancel)?;
    progress(tx, "프로그래머 연결 / 칩 탐색", None);
    let (mut engine, chip) = if request.demo {
        let capacity = demo.lock().unwrap().len();
        let definition = Definition { vendor: "Demo".into(), name: format!("Generic SPI {capacity} B"), capacity,
            erased_value: 255, can_read: true, can_write: true, can_erase: true };
        (Engine::Demo(demo), Chip { device: request.device.clone(), name: definition.name.clone(), capacity,
            jedec: "모의 ID".into(), actual_hz: request.hz, sfdp: "없음 (DB 기반 데모)".into(),
            definition: Some(definition), candidates: vec![] })
    } else {
        let info = protocol::probe(&request.device, request.hz)?;
        cancelled(cancel)?;
        protocol::identity(&request.device)?;
        let mut session = native::Session::open(&request.device, request.hz, folder, tx)?;
        let (candidates, definition) = session.detect(request.model.as_deref())?;
        let chip = Chip { device: request.device.clone(), name: definition.as_ref().map_or("모델 선택 필요".into(), |d| d.name.clone()),
            capacity: definition.as_ref().map_or(0, |d| d.capacity), definition, candidates,
            jedec: info.jedec, actual_hz: info.actual_hz, sfdp: info.sfdp };
        (Engine::Native(session), chip)
    };
    report["chip"] = serde_json::to_value(&chip)?;
    if matches!(request.action, Action::Connect) {
        let message = if chip.ready() { format!("{} · {} bytes 연결 완료. SFDP는 필수 조건이 아닙니다.", chip.name, chip.capacity) }
            else if chip.candidates.is_empty() { "프로그래머 연결 완료. 자동 탐색 일치 없음: DB에서 칩 모델을 선택하고 다시 식별하세요.".into() }
            else { "프로그래머 연결 완료. 탐색 후보에서 정확한 칩 모델을 선택하고 다시 식별하세요.".into() };
        return Ok((chip, None, message));
    }
    guard_target(request.expected.as_ref(), &chip)?;
    let definition = chip.require_definition()?.clone();
    ensure!(definition.can_read, "이 칩 정의는 읽기를 지원하지 않습니다.");
    let capacity = chip.capacity;
    let whole = Region::new(0, capacity, capacity)?;
    let read_region = match &request.action {
        Action::Dump => Some(whole),
        Action::Read { region } => Some(Region::new(region.start, region.len, capacity)?),
        _ => None,
    };
    if let Some(region) = read_region {
        let data = engine.read(region, Some(cancel), tx)?; cancelled(cancel)?;
        let hash = sha256(&data);
        report["buffer"] = json!({"storage": "memory", "bytes": data.len(), "offset": region.start, "sha256": hash});
        let read = ReadData { source: chip.device.clone(), chip_name: chip.name.clone(), offset: region.start,
            data: Arc::new(data), sha256: hash, saved: None };
        return Ok((chip, Some(read), "메모리 읽기 완료. 저장 버튼으로 파일에 저장할 수 있습니다.".into()));
    }
    let image = match &request.action {
        Action::Write { image, expected_sha256, .. } | Action::Verify { image, expected_sha256, .. } => {
            ensure!(fs::metadata(image)?.len() <= capacity as u64, "입력 파일이 현재 칩 용량보다 큽니다.");
            let bytes = fs::read(image)?;
            ensure!(!bytes.is_empty() && sha256(&bytes) == *expected_sha256, "입력 파일이 비었거나 선택 후 변경됐습니다. 다시 선택하세요.");
            Some(bytes)
        }
        _ => None,
    };
    if let Action::Verify { start, .. } = request.action {
        let image = image.unwrap(); let region = Region::new(start, image.len(), capacity)?;
        let actual = engine.read(region, Some(cancel), tx)?; cancelled(cancel)?;
        equal(&actual, &image, start)?;
        report["verified_sha256"] = json!(sha256(&actual));
        return Ok((chip, None, format!("{} bytes가 입력 파일과 일치합니다.", image.len())));
    }
    let region = match &request.action {
        Action::Write { start, .. } => Region::new(*start, image.as_ref().unwrap().len(), capacity)?,
        Action::Erase { region } => Region::new(region.start, region.len, capacity)?,
        _ => unreachable!(),
    };
    let full_erase = matches!(request.action, Action::Erase {..}) && region == whole;
    ensure!(definition.can_erase, "이 칩 정의는 지우기를 지원하지 않습니다.");
    ensure!(full_erase || definition.can_write, "이 칩 정의는 기록/범위 지우기를 지원하지 않습니다.");
    // Original data is read in RAM for preservation. It is saved ONLY when requested.
    let needs_original = request.backup || !full_erase;
    let original = if needs_original {
        progress(tx, if request.backup {"선택한 전체 백업 읽기"} else {"주변 데이터 보존을 위한 메모리 읽기 (백업 저장 안 함)"}, None);
        Some(engine.read(whole, Some(cancel), tx)?)
    } else { None };
    if request.backup {
        let original = original.as_ref().unwrap();
        let backup = folder.join("backup-original.bin");
        write_new(&backup, original)?;
        report["backup"] = json!({"path": backup, "bytes": original.len(), "sha256": sha256(original)});
        log(tx, format!("사용자 선택 전체 백업 저장: {}", backup.display()));
    } else {
        report["backup"] = json!(null);
        log(tx, "백업 저장 안 함 (사용자 선택)");
    }
    let mut target = match &request.action {
        Action::Write { start, .. } => compose_write(original.as_ref().unwrap(), image.as_ref().unwrap(), *start)?,
        Action::Erase { .. } if full_erase => { let mut data = Vec::new(); data.try_reserve_exact(capacity)?; data.resize(capacity, definition.erased_value); data },
        Action::Erase { region } => compose_erase(original.as_ref().unwrap(), *region, definition.erased_value)?,
        _ => unreachable!(),
    };
    let target_hash = sha256(&target);
    report["target_sha256"] = json!(target_hash);
    cancelled(cancel)?;
    if !request.demo { protocol::identity(&request.device)?; }
    let _ = tx.send(Event::CommitStarted);
    if full_erase { engine.erase_all(definition.erased_value)?; } else { engine.write(&mut target)?; }
    // Native write may reuse the supplied buffer; compare against the frozen target hash.
    let actual = engine.read(whole, None, tx)?;
    ensure!(sha256(&actual) == target_hash, "변경 후 전체 칩 검증 불일치. 작업 로그와 선택한 백업을 확인하세요.");
    report["final_sha256"] = json!(sha256(&actual)); report["whole_chip_verified"] = json!(true);
    Ok((chip, None, format!("변경과 전체 {capacity} bytes 검증 완료. 백업 저장: {}", if request.backup {"사용"} else {"안 함"})))
}

pub fn execute_cli(request: Request, demo: DemoFlash) -> Result<PathBuf> {
    execute_cli_with_save(request, demo, None)
}

pub fn execute_cli_with_save(request: Request, demo: DemoFlash, save: Option<&Path>) -> Result<PathBuf> {
    ensure!(save.is_none() || matches!(request.action, Action::Dump | Action::Read {..}), "--save는 전체 덤프/범위 읽기에서만 사용할 수 있습니다.");
    let worker = start(request, demo);
    loop { match worker.rx.recv()? {
        Event::Log(line) => eprintln!("{line}"),
        Event::Progress { phase, fraction } => eprintln!("{phase}: {fraction:?}"),
        Event::Done { chip, read, message, folder } => {
            if let Some(path) = save {
                let buffer = read.as_ref().context("저장할 메모리 버퍼가 없습니다.")?;
                save_output(path, &buffer.data, &AtomicBool::new(false), &mpsc::sync_channel(256).0)?;
            }
            let buffer = read.as_ref().map(|r| json!({"offset": r.offset, "bytes": r.data.len(), "sha256": r.sha256, "saved": save}));
            println!("{}", serde_json::to_string_pretty(&json!({"chip":chip,"message":message,"folder":folder,"buffer":buffer}))?); return Ok(folder);
        }
        Event::Failed { message, .. } | Event::SaveFailed { message } => anyhow::bail!(message),
        Event::CommitStarted | Event::Saved {..} => (),
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(action: Action, dir: &Path, backup: bool) -> Request {
        Request { device: Device { port: "DEMO".into(), serial: "SIMULATED".into() }, hz: 1_000_000,
            model: None, expected: None, backup, data_dir: dir.into(), action, demo: true }
    }
    fn memory_read(action: Action, dir: &Path, flash: &DemoFlash) -> (ReadData, PathBuf) {
        let worker = start(request(action, dir, false), flash.clone());
        loop { match worker.rx.recv().unwrap() {
            Event::Done { read: Some(read), folder, .. } => return (read, folder),
            Event::Failed { message, .. } => panic!("{message}"), _ => (),
        }}
    }
    fn save_buffer(data: ReadData, output: PathBuf) -> Result<PathBuf> {
        let worker = start_save(data, output);
        loop { match worker.rx.recv()? {
            Event::Saved { path } => return Ok(path),
            Event::SaveFailed { message } => anyhow::bail!(message), _ => (),
        }}
    }
    #[test]
    fn multiple_capacities_optional_backup_write_read_dump_verify_and_erase() {
        for (capacity, backup) in [(65536, false), (2*1024*1024, true), (32*1024*1024, false)] {
            let dir = tempfile::tempdir().unwrap(); let flash = demo_with_capacity(capacity);
            let original = flash.lock().unwrap().clone(); let start = capacity-8193;
            let image = vec![0x3c; 4099]; let image_path = dir.path().join("image.bin"); fs::write(&image_path, &image).unwrap();
            let folder = execute_cli(request(Action::Write { image: image_path.clone(), start, expected_sha256: sha256(&image) }, dir.path(), backup), flash.clone()).unwrap();
            assert_eq!(folder.join("backup-original.bin").exists(), backup);
            if backup { assert_eq!(fs::read(folder.join("backup-original.bin")).unwrap(), original); }
            assert_eq!(fs::read_dir(&folder).unwrap().filter_map(|p| p.ok()).filter(|p| p.path().extension().is_some_and(|e| e == "bin")).count(), usize::from(backup));
            let expected = compose_write(&original, &image, start).unwrap(); assert_eq!(*flash.lock().unwrap(), expected);
            execute_cli(request(Action::Verify { image: image_path, start, expected_sha256: sha256(&image) }, dir.path(), false), flash.clone()).unwrap();
            let (range, _) = memory_read(Action::Read { region: Region { start, len: image.len() } }, dir.path(), &flash);
            assert_eq!(range.data.as_slice(), image);
            let (dump, _) = memory_read(Action::Dump, dir.path(), &flash);
            assert_eq!(dump.data.as_slice(), expected);
            let region = Region { start: start+3, len: 201 };
            execute_cli(request(Action::Erase { region }, dir.path(), backup), flash.clone()).unwrap();
            assert_eq!(*flash.lock().unwrap(), compose_erase(&expected, region, 255).unwrap());
            execute_cli(request(Action::Erase { region: Region { start: 0, len: capacity } }, dir.path(), false), flash.clone()).unwrap();
            assert!(flash.lock().unwrap().iter().all(|b| *b == 255));
        }
    }
    #[test]
    fn changed_input_invalid_range_and_existing_output_cannot_modify_flash() {
        let dir = tempfile::tempdir().unwrap(); let flash = demo_with_capacity(65536); let original = flash.lock().unwrap().clone();
        let image = dir.path().join("changed.bin"); fs::write(&image, [1,2,3]).unwrap();
        for (start, hash) in [(0, sha256(&[9,9,9])), (65535, sha256(&[1,2,3]))] {
            assert!(execute_cli(request(Action::Write { image: image.clone(), start, expected_sha256: hash }, dir.path(), false), flash.clone()).is_err());
            assert_eq!(*flash.lock().unwrap(), original);
        }
        let output = dir.path().join("keep.bin"); fs::write(&output, b"keep").unwrap();
        assert!(execute_cli_with_save(request(Action::Dump, dir.path(), false), flash.clone(), Some(&output)).is_err());
        assert_eq!(fs::read(output).unwrap(), b"keep");
    }
    #[test]
    fn cancellation_before_read_and_after_commit() {
        let dir = tempfile::tempdir().unwrap(); let flash = demo_with_capacity(65536);
        let (tx, _) = mpsc::sync_channel(256);
        assert!(Engine::Demo(&flash).read(Region { start: 0, len: 10 }, Some(&AtomicBool::new(true)), &tx).is_err());
        let worker = start(request(Action::Erase { region: Region { start: 0, len: 65536 } }, dir.path(), false), flash.clone());
        let folder = loop { match worker.rx.recv().unwrap() {
            Event::CommitStarted => worker.cancel.store(true, Ordering::Relaxed),
            Event::Done { folder, .. } => break folder,
            Event::Failed { message, .. } => panic!("{message}"), _ => (),
        }};
        let report: serde_json::Value = serde_json::from_slice(&fs::read(folder.join("report.json")).unwrap()).unwrap();
        assert_eq!(report["whole_chip_verified"], true); assert!(report["backup"].is_null());
    }

    #[test]
    fn reads_are_memory_only_and_explicit_save_uses_snapshot_without_device() {
        let dir = tempfile::tempdir().unwrap(); let flash = demo_with_capacity(65536);
        let original = flash.lock().unwrap().clone();
        let (dump, dump_job) = memory_read(Action::Dump, dir.path(), &flash);
        let (range, range_job) = memory_read(Action::Read { region: Region { start: 4093, len: 4099 } }, dir.path(), &flash);
        assert_eq!(dump.data.as_slice(), original); assert_eq!(range.data.as_slice(), &original[4093..8192]);
        assert!(dump.saved.is_none() && range.saved.is_none());
        assert_eq!(range.offset, 4093); assert_eq!(range.source.serial, "SIMULATED");
        for folder in [dump_job, range_job] {
            let names: Vec<_> = fs::read_dir(folder).unwrap().map(|p| p.unwrap().file_name()).collect();
            assert_eq!(names, [std::ffi::OsString::from("report.json")]);
        }
        flash.lock().unwrap().fill(0); drop(flash); // saving must not reread a programmer
        let path = dir.path().join("explicit-range.bin");
        save_buffer(range.clone(), path.clone()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original[4093..8192]);
        assert_eq!(range.sha256, sha256(&fs::read(path).unwrap()));
        let path = dir.path().join("explicit-dump.bin");
        save_buffer(dump, path.clone()).unwrap(); assert_eq!(fs::read(path).unwrap(), original);
    }

    #[test]
    fn failed_or_cancelled_save_preserves_buffer_and_can_retry() {
        let dir = tempfile::tempdir().unwrap(); let bytes = vec![0xa5; 70001];
        let read = ReadData { source: Device { port: "DISCONNECTED".into(), serial: "OLD-UID".into() },
            chip_name: "Captured chip".into(), offset: 4093, data: Arc::new(bytes.clone()), sha256: sha256(&bytes), saved: None };
        let (tx, _) = mpsc::sync_channel(256);
        let output = dir.path().join("cancelled.bin");
        assert!(save_output(&output, &read.data, &AtomicBool::new(true), &tx).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        let existing = dir.path().join("existing.bin"); fs::write(&existing, b"keep").unwrap();
        assert!(save_buffer(read.clone(), existing.clone()).is_err());
        assert_eq!(fs::read(existing).unwrap(), b"keep");
        assert!(save_buffer(read.clone(), dir.path().join("missing-parent/failed.bin")).is_err());
        assert_eq!(read.data.as_slice(), bytes); assert_eq!(read.sha256, sha256(&bytes));
        let retry = dir.path().join("retry.bin"); save_buffer(read.clone(), retry.clone()).unwrap();
        assert_eq!(fs::read(retry).unwrap(), bytes); assert!(read.saved.is_none());
    }
}
