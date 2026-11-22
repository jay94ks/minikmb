//! Bounded operation metadata only. No arbitrary text/payload/path logging API.
use crate::model::{now_ms, AppError, Uid};
use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
#[cfg(test)]
mod tests;
pub const RECENT: usize = 300;
pub const PENDING: usize = 1024;
pub const FILE_BYTES: u64 = 4 * 1024 * 1024;
pub const FILES: usize = 5;
#[derive(Default)]
pub struct CaptureTracker {
    last: Option<(Uid, u64, String, &'static str)>,
}
impl CaptureTracker {
    pub fn observe(
        &mut self,
        journal: &Journal,
        binding: &crate::active::Binding,
        status: &crate::video::VideoStatus,
        now: u64,
    ) {
        let code = if !status.error.is_empty() {
            "CAPTURE_FAILED"
        } else if !status.running {
            "CAPTURE_STOPPED"
        } else if status.frame.is_none() {
            "WAITING_FRAME"
        } else if status
            .frame
            .as_ref()
            .is_some_and(|frame| now < frame.received_at || now - frame.received_at > 1000)
        {
            "FRAME_STALE"
        } else if !status.notice.is_empty() {
            "CAPTURE_RESTORED"
        } else {
            "FRAME_READY"
        };
        let current = (
            binding.uid.clone(),
            binding.generation,
            status
                .frame
                .as_ref()
                .map(|f| f.stream_id.clone())
                .unwrap_or_default(),
            code,
        );
        if self.last.as_ref() == Some(&current) {
            return;
        }
        self.last = Some(current);
        journal.emit(
            if ["CAPTURE_FAILED", "FRAME_STALE"].contains(&code) {
                Level::Warn
            } else {
                Level::Info
            },
            Event::CaptureChanged,
            Fields {
                uid: Some(binding.uid.clone()),
                generation: Some(binding.generation),
                code: Some(code),
                success: Some(["FRAME_READY", "CAPTURE_RESTORED"].contains(&code)),
                ..Default::default()
            },
        );
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}
#[derive(Clone, Copy, Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    KeyboardTransfer,
    KeyboardCaptureChanged,
    FirmwareFinished,
    AppStarted,
    AppStopped,
    Command,
    ChannelChanged,
    DeviceConnected,
    DeviceDisconnected,
    TransportFailed,
    NetworkChanged,
    CaptureChanged,
    ServerChanged,
    StorageJobChanged,
    InputState,
    LoggingConfigured,
}
/// Every string is either a parsed UID or a source-code constant; user values cannot enter.
#[derive(Clone, Default, Debug, Serialize, schemars::JsonSchema)]
pub struct Fields {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opcode: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hid_ticket: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_thread_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub received_events: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued_events: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejected_events: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub helper_pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swallowed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_callbacks: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_swallowed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reentrant_callbacks: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_error: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<Uid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uncertain: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbose: Option<bool>,
}
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Entry {
    pub id: u64,
    pub time_ms: u64,
    pub level: Level,
    pub event: Event,
    pub fields: Fields,
}
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Status {
    pub enabled: bool,
    pub verbose: bool,
    pub dropped: u64,
    pub pending: usize,
    pub write_error: Option<AppError>,
    pub recent: Vec<Entry>,
}
#[derive(Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub verbose: bool,
}
struct State {
    config: Config,
    sequence: u64,
    recent: VecDeque<Entry>,
    queue: VecDeque<Entry>,
    dropped: u64,
    error: Option<AppError>,
    closed: bool,
}
struct Inner {
    state: Mutex<State>,
    wake: Condvar,
    flush: Mutex<()>,
    directory: Option<PathBuf>,
    limit: u64,
    files: usize,
}
pub struct Journal {
    inner: Arc<Inner>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}
fn io_error() -> AppError {
    AppError::new(
        "LOG_IO",
        "로그를 파일에 기록하지 못했습니다. USB와 제어 기능은 계속 사용할 수 있습니다.",
    )
}
pub trait DirectoryOpener: Send + Sync {
    fn open(&self, path: &Path, guard: &dyn Fn() -> Result<(), AppError>) -> Result<(), AppError>;
}
pub struct NativeOpener;
impl DirectoryOpener for NativeOpener {
    fn open(&self, path: &Path, guard: &dyn Fn() -> Result<(), AppError>) -> Result<(), AppError> {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::{
                core::PCWSTR,
                Win32::{
                    System::Com::{
                        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
                        COINIT_DISABLE_OLE1DDE,
                    },
                    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
                },
            };
            let error = || {
                AppError::new(
                    "LOG_DIRECTORY",
                    "로그 폴더 열기 요청을 처리하지 못했습니다.",
                )
            };
            std::fs::create_dir_all(path).map_err(|_| error())?;
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
                .ok()
                .map_err(|_| error())?;
            struct Com;
            impl Drop for Com {
                fn drop(&mut self) {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
            let _com = Com;
            let path = path
                .as_os_str()
                .encode_wide()
                .chain([0])
                .collect::<Vec<_>>();
            let operation = "open\0".encode_utf16().collect::<Vec<_>>();
            guard()?;
            let result = unsafe {
                ShellExecuteW(
                    None,
                    PCWSTR(operation.as_ptr()),
                    PCWSTR(path.as_ptr()),
                    None,
                    None,
                    SW_SHOWNORMAL,
                )
            };
            if result.0 as isize <= 32 {
                return Err(error());
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = (path, guard);
            Err(AppError::new(
                "UNSUPPORTED",
                "이 시스템의 폴더 열기를 지원하지 않습니다.",
            ))
        }
    }
}
impl Journal {
    fn make(directory: Option<PathBuf>, limit: u64, files: usize, worker: bool) -> Arc<Self> {
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                config: Config {
                    enabled: true,
                    verbose: false,
                },
                sequence: 0,
                recent: VecDeque::with_capacity(RECENT),
                queue: VecDeque::with_capacity(PENDING),
                dropped: 0,
                error: None,
                closed: false,
            }),
            wake: Condvar::new(),
            flush: Mutex::new(()),
            directory,
            limit,
            files,
        });
        let journal = Arc::new(Self {
            inner: inner.clone(),
            worker: Default::default(),
        });
        if worker {
            let handle = std::thread::Builder::new()
                .name("MiniKMB logs".into())
                .stack_size(128 * 1024)
                .spawn(move || loop {
                    let mut state = inner.state.lock();
                    if !state.closed {
                        inner.wake.wait_for(&mut state, Duration::from_millis(250));
                    }
                    let closed = state.closed;
                    drop(state);
                    flush(&inner);
                    if closed {
                        break;
                    }
                });
            match handle {
                Ok(handle) => *journal.worker.lock() = Some(handle),
                Err(_) => journal.inner.state.lock().error = Some(io_error()),
            }
        }
        journal
    }
    pub fn new(directory: PathBuf) -> Arc<Self> {
        Self::make(Some(directory), FILE_BYTES, FILES, true)
    }
    pub fn memory() -> Arc<Self> {
        Self::make(None, FILE_BYTES, FILES, false)
    }
    /// QA previews have no events yet; changing this flag causes no file activity.
    pub fn quiet(&self) {
        self.inner.state.lock().config.enabled = false;
    }
    pub fn emit(&self, level: Level, event: Event, fields: Fields) {
        let mut state = self.inner.state.lock();
        if state.closed || !state.config.enabled || (level == Level::Debug && !state.config.verbose)
        {
            return;
        }
        state.sequence = state.sequence.saturating_add(1);
        let entry = Entry {
            id: state.sequence,
            time_ms: now_ms(),
            level,
            event,
            fields,
        };
        if state.recent.len() == RECENT {
            state.recent.pop_front();
        }
        state.recent.push_back(entry.clone());
        if state.queue.len() == PENDING {
            state.dropped = state.dropped.saturating_add(1);
        } else {
            state.queue.push_back(entry);
        }
    }
    pub fn status(&self) -> Status {
        let state = self.inner.state.lock();
        Status {
            enabled: state.config.enabled,
            verbose: state.config.verbose,
            dropped: state.dropped,
            pending: state.queue.len(),
            write_error: state.error.clone(),
            recent: state.recent.iter().cloned().collect(),
        }
    }
    pub fn configure(&self, config: Config) {
        let fields = Fields {
            enabled: Some(config.enabled),
            verbose: Some(config.verbose),
            ..Default::default()
        };
        if !config.enabled {
            self.emit(Level::Info, Event::LoggingConfigured, fields.clone());
        }
        self.inner.state.lock().config = config;
        if config.enabled {
            self.emit(Level::Info, Event::LoggingConfigured, fields);
        }
    }
    pub fn flush(&self) -> Result<(), AppError> {
        flush(&self.inner);
        self.inner.state.lock().error.clone().map_or(Ok(()), Err)
    }
    pub fn shutdown(&self) {
        self.inner.state.lock().closed = true;
        self.inner.wake.notify_all();
        if let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
        } else {
            flush(&self.inner);
        }
    }
    pub fn directory(&self) -> Option<&Path> {
        self.inner.directory.as_deref()
    }
    pub fn command<T>(
        &self,
        name: &'static str,
        binding: Option<&crate::active::Binding>,
        result: &Result<T, AppError>,
    ) {
        // Input and streaming chunks are metadata-only debug, preventing info floods.
        let frequent = name.starts_with("hid_")
            || name.starts_with("tty_")
            || name == "storage_io"
            || name == "storage_job_get"
            || name.ends_with("get_status");
        let level = if result.is_err() {
            Level::Warn
        } else if frequent {
            Level::Debug
        } else {
            Level::Info
        };
        self.emit(
            level,
            Event::Command,
            Fields {
                uid: binding.map(|b| b.uid.clone()),
                generation: binding.map(|b| b.generation),
                command: Some(name),
                success: Some(result.is_ok()),
                code: result.as_ref().err().map(|e| e.code),
                uncertain: result.as_ref().err().map(|e| e.uncertain),
                ..Default::default()
            },
        );
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn file(directory: &Path, index: usize) -> PathBuf {
    directory.join(if index == 0 {
        "operations.jsonl".into()
    } else {
        format!("operations.jsonl.{index}")
    })
}
fn regular(path: &Path) -> Result<Option<u64>, AppError> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(Some(meta.len())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        _ => Err(io_error()),
    }
}
fn persist(inner: &Inner, entries: &[Entry]) -> Result<(), AppError> {
    let Some(directory) = &inner.directory else {
        return Ok(());
    };
    std::fs::create_dir_all(directory).map_err(|_| io_error())?;
    let lock_path = directory.join("operations.lock");
    regular(&lock_path)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|_| io_error())?;
    lock.try_lock().map_err(|_| io_error())?;
    let mut size = regular(&file(directory, 0))?.unwrap_or(0);
    let mut output: Option<File> = None;
    for entry in entries {
        let mut line = serde_json::to_vec(entry).map_err(|_| io_error())?;
        line.push(b'\n');
        if line.len() > 2048 || line.len() as u64 > inner.limit {
            return Err(io_error());
        }
        if size > 0 && size + line.len() as u64 > inner.limit {
            if let Some(handle) = output.take() {
                handle.sync_data().map_err(|_| io_error())?;
            }
            if regular(&file(directory, inner.files - 1))?.is_some() {
                std::fs::remove_file(file(directory, inner.files - 1)).map_err(|_| io_error())?;
            }
            for index in (1..inner.files).rev() {
                if regular(&file(directory, index - 1))?.is_some() {
                    std::fs::rename(file(directory, index - 1), file(directory, index))
                        .map_err(|_| io_error())?;
                }
            }
            size = 0;
        }
        if output.is_none() {
            output = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(file(directory, 0))
                    .map_err(|_| io_error())?,
            );
        }
        output
            .as_mut()
            .unwrap()
            .write_all(&line)
            .map_err(|_| io_error())?;
        size += line.len() as u64;
    }
    if let Some(handle) = output {
        handle.sync_data().map_err(|_| io_error())?;
    }
    Ok(())
}
fn flush(inner: &Inner) {
    let _gate = inner.flush.lock();
    let entries = {
        let mut state = inner.state.lock();
        state.queue.drain(..).collect::<Vec<_>>()
    };
    if entries.is_empty() {
        return;
    }
    let result = persist(inner, &entries);
    let mut state = inner.state.lock();
    if result.is_err() {
        state.dropped = state.dropped.saturating_add(entries.len() as u64);
    }
    state.error = result.err();
}
