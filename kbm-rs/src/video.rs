//! One capture and one latest frame for GUI, REST and MCP together.
use crate::{
    active::Binding,
    media::{Capture, CaptureMode, VideoFrame},
    model::{now_ms, AppError, Uid},
};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct FrameInfo {
    pub uid: Uid,
    pub generation: u64,
    pub frame_id: String,
    pub stream_id: String,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub received_at: u64,
}
#[derive(Clone, Debug, Default, Serialize, schemars::JsonSchema)]
pub struct VideoStatus {
    pub running: bool,
    pub requested: Option<CaptureMode>,
    pub effective_mode: Option<CaptureMode>,
    pub notice: String,
    pub frame: Option<FrameInfo>,
    pub error: String,
}
struct Session {
    binding: Binding,
    epoch: u64,
    mode: CaptureMode,
    capture: Capture,
    life: tokio_util::sync::CancellationToken,
}
#[derive(Default)]
pub struct VideoHub {
    session: Mutex<Option<Session>>,
    sequence: AtomicU64,
}
pub struct CurrentFrame {
    pub info: FrameInfo,
    pub data: Arc<VideoFrame>,
    pub life: tokio_util::sync::CancellationToken,
}
#[derive(Clone, Copy, Default, serde::Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotFormat {
    #[default]
    Jpeg,
    Png,
}
impl SnapshotFormat {
    pub fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}
#[derive(Default, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotArgs {
    #[serde(default)]
    pub format: SnapshotFormat,
}
pub struct SnapshotImage {
    pub info: FrameInfo,
    pub format: SnapshotFormat,
    pub bytes: Vec<u8>,
}
pub fn encode_snapshot(
    frame: CurrentFrame,
    format: SnapshotFormat,
) -> Result<SnapshotImage, AppError> {
    let bytes = match format {
        SnapshotFormat::Jpeg => frame.data.jpeg.clone(),
        SnapshotFormat::Png => {
            use image::ImageEncoder;
            let mut out = Vec::new();
            image::codecs::png::PngEncoder::new(&mut out)
                .write_image(
                    &frame.data.rgba,
                    frame.info.width,
                    frame.info.height,
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|_| AppError::new("ENCODE_FAILED", "PNG 생성에 실패했습니다."))?;
            out
        }
    };
    Ok(SnapshotImage {
        info: frame.info,
        format,
        bytes,
    })
}
impl Session {
    fn info(&self, f: &VideoFrame) -> FrameInfo {
        FrameInfo {
            uid: self.binding.uid.clone(),
            generation: self.binding.generation,
            frame_id: format!("{}-{}-{}", self.binding.generation, self.epoch, f.id),
            stream_id: format!("{}-{}", self.binding.generation, self.epoch),
            width: f.width,
            height: f.height,
            fps: f.fps,
            received_at: f.received_at,
        }
    }
}
impl VideoHub {
    #[cfg(test)]
    pub(crate) fn test_pixels(&self) {
        let session = self.session.lock();
        let state = &session.as_ref().unwrap().capture.state;
        let mut state = state.lock();
        let frame = state.frame.as_mut().unwrap();
        let frame = Arc::get_mut(frame).unwrap();
        frame.rgba = vec![127; (frame.width * frame.height * 4) as usize].into();
        frame.received_at = now_ms();
    }
    #[cfg(test)]
    pub(crate) fn test_geometry(&self, binding: Binding, width: u32, height: u32) {
        let epoch = self.sequence.fetch_add(1, Ordering::Relaxed);
        let frame = VideoFrame {
            id: 1,
            width,
            height,
            fps: 30.0,
            received_at: now_ms(),
            jpeg: Vec::new(),
            rgba: Arc::from([]),
        };
        *self.session.lock() = Some(Session {
            binding,
            epoch,
            mode: CaptureMode::default(),
            capture: Capture::test_frame(frame),
            life: tokio_util::sync::CancellationToken::new(),
        });
    }
    pub fn validate_reference(&self, binding: &Binding, id: &str) -> Result<(), AppError> {
        let slot = self.session.lock();
        let s = slot
            .as_ref()
            .filter(|s| &s.binding == binding)
            .ok_or_else(|| AppError::new("FRAME_NOT_READY", "활성 영상이 없습니다."))?;
        let prefix = format!("{}-{}-", binding.generation, s.epoch);
        let number = id
            .strip_prefix(&prefix)
            .and_then(|n| n.parse::<u64>().ok())
            .ok_or_else(|| {
                AppError::new(
                    "STALE_FRAME_REFERENCE",
                    "다른 채널/스트림의 영상 참조입니다.",
                )
            })?;
        let state = s.capture.state.lock();
        let f = state
            .frame
            .as_ref()
            .ok_or_else(|| AppError::new("FRAME_NOT_READY", "현재 영상이 없습니다."))?;
        let (_, w, h, received) = state
            .history
            .iter()
            .find(|(n, _, _, _)| *n == number)
            .ok_or_else(|| AppError::new("STALE_FRAME_REFERENCE", "영상 참조가 만료됐습니다."))?;
        if (*w, *h) != (f.width, f.height) {
            return Err(AppError::new(
                "FRAME_GEOMETRY_CHANGED",
                "영상 크기가 변경됐습니다.",
            ));
        }
        ensure_fresh(*received, now_ms())
            .map_err(|_| AppError::new("STALE_FRAME_REFERENCE", "영상 참조가 만료됐습니다."))
    }
    pub fn cancel(&self) {
        if let Some(s) = self.session.lock().as_ref() {
            s.life.cancel();
            s.capture.cancel();
        }
    }
    pub async fn stop(&self) -> Result<(), AppError> {
        let old = self.session.lock().take();
        if let Some(mut s) = old {
            s.life.cancel();
            s.capture.cancel();
            tokio::task::spawn_blocking(move || s.capture.stop())
                .await
                .map_err(|_| {
                    AppError::new("CAPTURE_STOP_FAILED", "영상 종료 작업이 실패했습니다.")
                })?;
        }
        Ok(())
    }
    /// Caller serializes starts/stops and validates the active binding.
    pub fn start(
        &self,
        binding: Binding,
        root: String,
        mode: CaptureMode,
        fallback: Option<CaptureMode>,
    ) -> Result<(), AppError> {
        validate_mode(mode)?;
        let mut slot = self.session.lock();
        if slot.is_some() {
            return Err(AppError::new(
                "CAPTURE_BUSY",
                "이전 영상을 먼저 종료해야 합니다.",
            ));
        }
        *slot = Some(Session {
            binding,
            epoch: self.sequence.fetch_add(1, Ordering::Relaxed),
            mode,
            capture: Capture::start_with_fallback(root, mode, fallback),
            life: tokio_util::sync::CancellationToken::new(),
        });
        Ok(())
    }
    pub fn status(&self, binding: &Binding) -> VideoStatus {
        let slot = self.session.lock();
        let Some(s) = slot.as_ref().filter(|s| &s.binding == binding) else {
            return VideoStatus::default();
        };
        let state = s.capture.state.lock();
        VideoStatus {
            running: state.running && !s.life.is_cancelled(),
            requested: Some(s.mode),
            effective_mode: Some(state.effective_mode),
            notice: state.notice.clone(),
            frame: state.frame.as_ref().map(|f| s.info(f)),
            error: state.error.clone(),
        }
    }
    pub fn frame(&self, binding: &Binding) -> Result<CurrentFrame, AppError> {
        let slot = self.session.lock();
        let s = slot
            .as_ref()
            .filter(|s| &s.binding == binding)
            .ok_or_else(|| {
                AppError::new("FRAME_NOT_READY", "활성 채널의 영상이 열려 있지 않습니다.")
            })?;
        let state = s.capture.state.lock();
        if !state.running || !state.error.is_empty() || s.life.is_cancelled() {
            return Err(AppError::new(
                "CAPTURE_STOPPED",
                "영상 수신이 중단됐습니다.",
            ));
        }
        let f = state.frame.as_ref().ok_or_else(|| {
            AppError::new("FRAME_NOT_READY", "첫 영상 프레임을 기다리고 있습니다.")
        })?;
        ensure_fresh(f.received_at, now_ms())?;
        Ok(CurrentFrame {
            info: s.info(f),
            data: f.clone(),
            life: s.life.clone(),
        })
    }
}
pub fn validate_mode(mode: CaptureMode) -> Result<(), AppError> {
    let auto = mode == CaptureMode::default();
    let fixed = (1..=8192).contains(&mode.width)
        && (1..=8192).contains(&mode.height)
        && mode.fps.is_finite()
        && (0.0..=240.0).contains(&mode.fps);
    if auto || fixed {
        Ok(())
    } else {
        Err(AppError::new(
            "INVALID_MODE",
            "자동 모드 또는 올바른 영상 크기·주사율을 지정하세요.",
        ))
    }
}
fn ensure_fresh(received: u64, now: u64) -> Result<(), AppError> {
    if now < received || now - received > 1000 {
        Err(AppError::new(
            "STALE_FRAME",
            "최근 1초 이내의 영상 프레임이 없습니다.",
        ))
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_jpeg_preserves_source_and_png_preserves_decoded_pixels() {
        let uid = Uid::from_bytes(&[0x12; 12]).unwrap();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 95)
            .encode(
                &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255],
                2,
                2,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        let rgba = crate::media::decode_jpeg(&jpeg).unwrap();
        let info = FrameInfo {
            uid,
            generation: 3,
            frame_id: "3-1-2".into(),
            stream_id: "3-1".into(),
            width: 2,
            height: 2,
            fps: 30.0,
            received_at: 123,
        };
        let frame = Arc::new(VideoFrame {
            id: 2,
            width: 2,
            height: 2,
            fps: 30.0,
            received_at: 123,
            jpeg: jpeg.clone(),
            rgba: rgba.clone().into(),
        });
        let jpg = encode_snapshot(
            CurrentFrame {
                info: info.clone(),
                data: frame.clone(),
                life: tokio_util::sync::CancellationToken::new(),
            },
            SnapshotFormat::Jpeg,
        )
        .unwrap();
        assert_eq!(jpg.bytes, jpeg);
        let png = encode_snapshot(
            CurrentFrame {
                info,
                data: frame,
                life: tokio_util::sync::CancellationToken::new(),
            },
            SnapshotFormat::Png,
        )
        .unwrap();
        let decoded = image::load_from_memory_with_format(&png.bytes, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert_eq!(decoded.into_raw(), rgba);
    }
    #[test]
    fn stale_frames_and_invalid_modes_never_become_current() {
        assert!(ensure_fresh(100, 1100).is_ok());
        assert!(ensure_fresh(100, 1101).is_err());
        assert!(ensure_fresh(100, 99).is_err());
        for mode in [
            CaptureMode {
                width: 0,
                height: 1080,
                fps: 30.0,
            },
            CaptureMode {
                width: 1920,
                height: 1080,
                fps: f64::NAN,
            },
        ] {
            assert!(validate_mode(mode).is_err());
        }
        assert!(validate_mode(CaptureMode::default()).is_ok());
        let modes = crate::media::mode_candidates(CaptureMode::default());
        assert_eq!(modes.len(), 10);
        assert_eq!(
            modes[0],
            CaptureMode {
                width: 1920,
                height: 1080,
                fps: 30.0
            }
        );
        assert_eq!(modes[1].fps, 24.0);
        assert_eq!(modes[2].fps, 0.0);
        assert_eq!(modes[3].width, 1280);
        assert_eq!(*modes.last().unwrap(), CaptureMode::default());
    }
}
