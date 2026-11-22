use crate::model::{AppError, Uid};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, PartialEq, schemars::JsonSchema)]
pub struct ActiveState {
    pub uid: Option<Uid>,
    pub generation: u64,
    pub switching: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Binding {
    pub uid: Uid,
    pub generation: u64,
}
#[derive(Default)]
pub struct ActiveCoordinator {
    state: Mutex<ActiveState>,
    life: Mutex<tokio_util::sync::CancellationToken>,
}
impl ActiveCoordinator {
    /// Resolve once outside time-sensitive native callbacks. Channel changes
    /// cancel this lease; checking it in a Windows hook never takes our mutex.
    pub fn guard(
        &self,
        binding: &Binding,
    ) -> Result<tokio_util::sync::CancellationToken, AppError> {
        let s = self.state.lock();
        if s.switching || s.generation != binding.generation || s.uid.as_ref() != Some(&binding.uid)
        {
            return Err(AppError::new("STALE_OPERATION", "활성 채널이 변경됐습니다.").at(binding));
        }
        Ok(self.life.lock().clone())
    }
    pub fn state(&self) -> ActiveState {
        self.state.lock().clone()
    }
    pub(crate) fn inspect<T>(&self, read: impl FnOnce(&ActiveState) -> T) -> T {
        read(&self.state.lock())
    }
    pub fn bind(&self) -> Result<Binding, AppError> {
        let s = self.state.lock();
        if s.switching {
            return Err(AppError::new(
                "CHANNEL_SWITCHING",
                "채널을 전환하고 있습니다.",
            ));
        }
        Ok(Binding {
            uid: s
                .uid
                .clone()
                .ok_or_else(|| AppError::new("NO_ACTIVE_CHANNEL", "활성 채널이 없습니다."))?,
            generation: s.generation,
        })
    }
    pub fn validate(&self, b: &Binding) -> Result<(), AppError> {
        let s = self.state.lock();
        if s.switching || s.generation != b.generation || s.uid.as_ref() != Some(&b.uid) {
            Err(AppError::new("STALE_OPERATION", "채널 전환으로 이전 요청을 취소했습니다.").at(b))
        } else {
            Ok(())
        }
    }
    /// Reserve a switch; device cleanup must succeed before commit.
    pub(crate) fn begin(&self) -> Result<ActiveState, AppError> {
        let mut s = self.state.lock();
        if s.switching {
            return Err(AppError::new(
                "CHANNEL_SWITCHING",
                "다른 채널 전환이 진행 중입니다.",
            ));
        }
        let previous = s.clone();
        self.life.lock().cancel();
        s.switching = true;
        s.generation += 1;
        Ok(previous)
    }
    pub(crate) fn finish(&self, next: Option<Uid>) {
        let mut s = self.state.lock();
        let mut life = self.life.lock();
        life.cancel();
        *life = tokio_util::sync::CancellationToken::new();
        s.uid = next;
        s.switching = false;
    }
    pub fn disconnected(&self, uid: &Uid) {
        let mut s = self.state.lock();
        if s.uid.as_ref() == Some(uid) {
            self.life.lock().cancel();
            s.uid = None;
            s.generation += 1;
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PixelPoint {
    pub x: f64,
    pub y: f64,
}
impl PixelPoint {
    pub fn hid(self, width: u32, height: u32) -> Result<[u16; 2], AppError> {
        if width == 0 || height == 0 {
            return Err(AppError::new("FRAME_NOT_READY", "현재 영상이 없습니다."));
        }
        if !self.x.is_finite()
            || !self.y.is_finite()
            || self.x < 0.0
            || self.y < 0.0
            || self.x > f64::from(width - 1)
            || self.y > f64::from(height - 1)
        {
            return Err(AppError::new(
                "INVALID_COORDINATES",
                "캡처 영상 범위 안의 픽셀 좌표를 입력하세요.",
            ));
        }
        let axis = |n: f64, size| {
            if size <= 1 {
                0
            } else {
                (n / f64::from(size - 1) * 32767.0)
                    .round()
                    .clamp(0.0, 32767.0) as u16
            }
        };
        Ok([axis(self.x, width), axis(self.y, height)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn uid(n: u8) -> Uid {
        Uid::from_bytes(&[n; 12]).unwrap()
    }
    #[test]
    fn one_target_and_old_operations_never_retarget() {
        let a = ActiveCoordinator::default();
        assert_eq!(a.bind().unwrap_err().code, "NO_ACTIVE_CHANNEL");
        a.begin().unwrap();
        a.finish(Some(uid(1)));
        let old = a.bind().unwrap();
        a.begin().unwrap();
        assert!(a.begin().is_err());
        assert!(a.bind().is_err());
        a.finish(Some(uid(2)));
        assert!(a.validate(&old).is_err());
        let new = a.bind().unwrap();
        assert_eq!(new.uid, uid(2));
        a.disconnected(&uid(1));
        assert!(a.validate(&new).is_ok());
        a.disconnected(&uid(2));
        assert!(a.bind().is_err());
    }
    #[test]
    fn capture_pixels_not_window_or_hid_units() {
        assert_eq!(
            PixelPoint {
                x: 1919.0,
                y: 1079.0
            }
            .hid(1920, 1080)
            .unwrap(),
            [32767, 32767]
        );
        assert_eq!(
            PixelPoint { x: 960.0, y: 540.0 }.hid(1920, 1080).unwrap(),
            [16392, 16399]
        );
        assert_eq!(PixelPoint { x: 0.0, y: 0.0 }.hid(1, 1).unwrap(), [0, 0]);
        assert!(PixelPoint {
            x: f64::NAN,
            y: 1.0
        }
        .hid(1920, 1080)
        .is_err());
        assert!(PixelPoint { x: 1920.0, y: 1.0 }.hid(1920, 1080).is_err());
        assert!(serde_json::from_str::<PixelPoint>(r#"{"x":1,"y":2,"uids":["x"]}"#).is_err());
    }
}
