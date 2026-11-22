//! Media Foundation capture. COM objects stay on their creating MTA worker;
//! callbacks publish owned JPEG bytes, never COM pointers, across threads.
use parking_lot::{Condvar, Mutex};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use windows::{
    core::{implement, Ref, HRESULT, PWSTR},
    Win32::{Media::MediaFoundation::*, System::Com::*},
};

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureMode {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct CaptureDevice {
    pub root: String,
    pub link: String,
}
pub struct VideoFrame {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub received_at: u64,
    pub jpeg: Vec<u8>,
    pub rgba: Arc<[u8]>,
}
#[derive(Default)]
pub struct CaptureState {
    pub frame: Option<Arc<VideoFrame>>,
    pub error: String,
    pub running: bool,
    pub effective_mode: CaptureMode,
    pub notice: String,
    pub history: std::collections::VecDeque<(u64, u32, u32, u64)>,
}
pub struct Capture {
    pub state: Arc<Mutex<CaptureState>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Capture {
    #[cfg(test)]
    pub(crate) fn test_frame(frame: VideoFrame) -> Self {
        let mut state = CaptureState {
            running: true,
            ..Default::default()
        };
        state
            .history
            .push_back((frame.id, frame.width, frame.height, frame.received_at));
        state.frame = Some(Arc::new(frame));
        Self {
            state: Arc::new(Mutex::new(state)),
            stop: Default::default(),
            worker: None,
        }
    }
    pub fn start(root: String, mode: CaptureMode) -> Self {
        Self::start_with_fallback(root, mode, None)
    }
    pub fn start_with_fallback(
        root: String,
        mode: CaptureMode,
        fallback: Option<CaptureMode>,
    ) -> Self {
        let state = Arc::new(Mutex::new(CaptureState {
            running: true,
            effective_mode: mode,
            ..Default::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let shared = state.clone();
        let halt = stop.clone();
        let worker = std::thread::spawn(move || {
            let mut result = run(&root, mode, &shared, &halt);
            if result.is_err() && !halt.load(Ordering::Acquire) && shared.lock().frame.is_none() {
                if let Some(previous) = fallback.filter(|p| *p != mode) {
                    {
                        let mut s = shared.lock();
                        s.effective_mode = previous;
                        s.notice = "요청한 영상 모드를 열지 못해 이전 모드로 복원했습니다.".into();
                    }
                    result = run(&root, previous, &shared, &halt);
                }
            }
            let mut s = shared.lock();
            s.running = false;
            if let Err(e) = result {
                if !halt.load(Ordering::Acquire) {
                    s.error = e.to_string();
                }
            }
        });
        Self {
            state,
            stop,
            worker: Some(worker),
        }
    }
    pub fn stop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
    pub fn cancel(&self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.stop();
    }
}

struct MediaContext;
impl MediaContext {
    fn new() -> windows::core::Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                CoUninitialize();
                return Err(e);
            }
        }
        Ok(Self)
    }
}
impl Drop for MediaContext {
    fn drop(&mut self) {
        unsafe {
            let _ = MFShutdown();
            CoUninitialize();
        }
    }
}
struct Device {
    public: CaptureDevice,
    activation: IMFActivate,
}
fn enumerate() -> windows::core::Result<Vec<Device>> {
    unsafe {
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1)?;
        let attributes = attributes.unwrap();
        attributes.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )?;
        let mut list = std::ptr::null_mut();
        let mut count = 0;
        MFEnumDeviceSources(&attributes, &mut list, &mut count)?;
        if count == 0 {
            CoTaskMemFree(Some(list.cast()));
            return Ok(Vec::new());
        }
        let activations = std::slice::from_raw_parts_mut(list, count as usize)
            .iter_mut()
            .filter_map(Option::take)
            .collect::<Vec<_>>();
        CoTaskMemFree(Some(list.cast()));
        let mut result = Vec::new();
        for activation in activations {
            let mut raw = PWSTR::null();
            let mut length = 0;
            if activation
                .GetAllocatedString(
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                    &mut raw,
                    &mut length,
                )
                .is_err()
            {
                continue;
            }
            let link = raw.to_string();
            CoTaskMemFree(Some(raw.0.cast()));
            let Ok(link) = link else {
                continue;
            };
            let lower = link.to_ascii_lowercase();
            if ![
                "usb#vid_345f&pid_2130&mi_00#",
                "usb#vid_534d&pid_2130&mi_00#",
            ]
            .iter()
            .any(|p| lower.contains(p))
            {
                continue;
            }
            let Some(path) = link.strip_prefix("\\\\?\\") else {
                continue;
            };
            let instance = path.split('#').take(3).collect::<Vec<_>>().join("\\");
            let Ok(root) = crate::platform::device_parent(&instance) else {
                continue;
            };
            result.push(Device {
                public: CaptureDevice { root, link },
                activation,
            });
        }
        Ok(result)
    }
}
pub fn devices() -> windows::core::Result<Vec<CaptureDevice>> {
    let _context = MediaContext::new()?;
    Ok(enumerate()?.into_iter().map(|d| d.public).collect())
}

struct Packet {
    bytes: Vec<u8>,
    flags: u32,
    received_at: u64,
}
type Pending = Arc<(Mutex<Option<Result<Packet, HRESULT>>>, Condvar)>;
#[implement(IMFSourceReaderCallback)]
struct ReaderCallback {
    pending: Pending,
}
impl IMFSourceReaderCallback_Impl for ReaderCallback_Impl {
    fn OnReadSample(
        &self,
        status: HRESULT,
        _stream: u32,
        flags: u32,
        _timestamp: i64,
        sample: Ref<IMFSample>,
    ) -> windows::core::Result<()> {
        let result = (|| -> windows::core::Result<Packet> {
            status.ok()?;
            let mut bytes = Vec::new();
            if let Some(sample) = sample.as_ref() {
                unsafe {
                    let buffer = sample.ConvertToContiguousBuffer()?;
                    let mut data = std::ptr::null_mut();
                    let mut length = 0;
                    buffer.Lock(&mut data, None, Some(&mut length))?;
                    if !data.is_null() && length > 0 && length <= 8 * 1024 * 1024 {
                        bytes.extend_from_slice(std::slice::from_raw_parts(data, length as usize));
                    }
                    let _ = buffer.Unlock();
                    if length > 8 * 1024 * 1024 {
                        return Err(fail("영상 프레임이 허용 크기를 초과했습니다."));
                    }
                }
            }
            Ok(Packet {
                bytes,
                flags,
                received_at: crate::model::now_ms(),
            })
        })()
        .map_err(|e| e.code());
        *self.pending.0.lock() = Some(result);
        self.pending.1.notify_one();
        Ok(())
    }
    fn OnFlush(&self, _stream: u32) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnEvent(&self, _stream: u32, _event: Ref<IMFMediaEvent>) -> windows::core::Result<()> {
        Ok(())
    }
}
fn fail(text: &str) -> windows::core::Error {
    windows::core::Error::new(windows::Win32::Foundation::E_FAIL, text)
}
fn run(
    root: &str,
    mode: CaptureMode,
    state: &Arc<Mutex<CaptureState>>,
    stop: &AtomicBool,
) -> windows::core::Result<()> {
    let _context = MediaContext::new()?;
    let candidates = mode_candidates(mode);
    let mut last_error = fail("지원하는 영상 모드가 없습니다.");
    for candidate in candidates {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        // Each attempt releases the entire source before opening the next one.
        // Once a real frame arrives, a later stream failure is reported, not
        // silently replaced with a different mode or capture device.
        match run_one(root, candidate, state, stop) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if state.lock().frame.is_some() {
                    return Err(e);
                }
                last_error = e;
            }
        }
    }
    Err(last_error)
}
pub fn mode_candidates(mode: CaptureMode) -> Vec<CaptureMode> {
    if mode.width != 0 {
        return vec![mode];
    }
    let mut modes = Vec::new();
    for (width, height) in [(1920, 1080), (1280, 720), (640, 480)] {
        for fps in [30.0, 24.0, 0.0] {
            modes.push(CaptureMode { width, height, fps });
        }
    }
    modes.push(CaptureMode::default());
    modes
}
fn run_one(
    root: &str,
    mode: CaptureMode,
    state: &Arc<Mutex<CaptureState>>,
    stop: &AtomicBool,
) -> windows::core::Result<()> {
    let mut devices: Vec<_> = enumerate()?
        .into_iter()
        .filter(|d| d.public.root.eq_ignore_ascii_case(root))
        .collect();
    if devices.len() != 1 {
        return Err(fail("정확히 대응하는 MS2130 장치가 없습니다."));
    }
    let device = devices.remove(0);
    let source: IMFMediaSource = unsafe { device.activation.ActivateObject()? };
    let result = capture(&source, mode, state, stop);
    unsafe {
        let _ = source.Shutdown();
        let _ = device.activation.ShutdownObject();
    }
    result
}
fn capture(
    source: &IMFMediaSource,
    mode: CaptureMode,
    state: &Arc<Mutex<CaptureState>>,
    stop: &AtomicBool,
) -> windows::core::Result<()> {
    unsafe {
        let pending: Pending = Arc::new((Mutex::new(None), Condvar::new()));
        let callback: IMFSourceReaderCallback = ReaderCallback {
            pending: pending.clone(),
        }
        .into();
        let mut attributes = None;
        MFCreateAttributes(&mut attributes, 1)?;
        let attributes = attributes.unwrap();
        attributes.SetUnknown(&MF_SOURCE_READER_ASYNC_CALLBACK, &callback)?;
        let reader = MFCreateSourceReaderFromMediaSource(source, &attributes)?;
        let video = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(video, true)?;
        let mut chosen = None;
        for index in 0..512 {
            let Ok(ty) = reader.GetNativeMediaType(video, index) else {
                break;
            };
            if ty.GetGUID(&MF_MT_SUBTYPE)? != MFVideoFormat_MJPG {
                continue;
            }
            let wh = ty.GetUINT64(&MF_MT_FRAME_SIZE)?;
            let rate = ty.GetUINT64(&MF_MT_FRAME_RATE)?;
            let (width, height) = (wh >> 32, wh & 0xffff_ffff);
            let (n, d) = (rate >> 32, rate & 0xffff_ffff);
            if d == 0 {
                continue;
            }
            let fps = n as f64 / d as f64;
            if (mode.width > 0 && u64::from(mode.width) != width)
                || (mode.height > 0 && u64::from(mode.height) != height)
                || (mode.fps > 0.0 && (mode.fps - fps).abs() > 0.01)
            {
                continue;
            }
            if width == 0 || height == 0 || width > 8192 || height > 8192 {
                continue;
            }
            reader.SetCurrentMediaType(video, None, &ty)?;
            let checked = reader.GetCurrentMediaType(video)?;
            if checked.GetGUID(&MF_MT_SUBTYPE)? == MFVideoFormat_MJPG
                && checked.GetUINT64(&MF_MT_FRAME_SIZE)? == wh
                && checked.GetUINT64(&MF_MT_FRAME_RATE)? == rate
            {
                chosen = Some((width as u32, height as u32, fps));
            }
            break;
        }
        let Some((width, height, fps)) = chosen else {
            return Err(fail("요청한 영상 모드를 지원하지 않습니다."));
        };
        let result = (|| {
            let mut deadline = Instant::now() + Duration::from_secs(5);
            let mut id = 0;
            reader.ReadSample(video, 0, None, None, None, None)?;
            while !stop.load(Ordering::Acquire) {
                let mut next = pending.0.lock();
                if next.is_none() {
                    pending.1.wait_for(&mut next, Duration::from_millis(100));
                }
                let Some(packet) = next.take() else {
                    if Instant::now() > deadline {
                        return Err(fail(if id == 0 {
                            "첫 영상 프레임을 받지 못했습니다."
                        } else {
                            "영상 프레임 수신이 중단됐습니다."
                        }));
                    }
                    continue;
                };
                drop(next);
                let packet = packet.map_err(windows::core::Error::from)?;
                if packet.flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                    return Err(fail("영상 스트림 오류"));
                }
                if packet.flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    return Err(fail("영상 스트림이 종료됐습니다."));
                }
                if !packet.bytes.is_empty() {
                    let dimensions = image::ImageReader::with_format(
                        std::io::Cursor::new(&packet.bytes),
                        image::ImageFormat::Jpeg,
                    )
                    .into_dimensions()
                    .map_err(|_| fail("JPEG 영상 형식 오류"))?;
                    if dimensions != (width, height) {
                        return Err(fail("실제 프레임 크기가 협상된 영상과 다릅니다."));
                    }
                    let rgba = decode_jpeg(&packet.bytes).map_err(|_| fail("JPEG 디코딩 실패"))?;
                    id += 1;
                    deadline = Instant::now() + Duration::from_secs(5);
                    let mut state = state.lock();
                    if state.history.len() == 128 {
                        state.history.pop_front();
                    }
                    state
                        .history
                        .push_back((id, width, height, packet.received_at));
                    state.frame = Some(Arc::new(VideoFrame {
                        id,
                        width,
                        height,
                        fps,
                        received_at: packet.received_at,
                        jpeg: packet.bytes,
                        rgba: rgba.into(),
                    }));
                }
                reader.ReadSample(video, 0, None, None, None, None)?;
            }
            Ok(())
        })();
        let _ = reader.Flush(video);
        result
    }
}

pub fn decode_jpeg(bytes: &[u8]) -> image::ImageResult<Vec<u8>> {
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Jpeg);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?.to_rgba8().into_raw())
}
