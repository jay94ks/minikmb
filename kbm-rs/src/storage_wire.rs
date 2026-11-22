//! Existing v1.0 storage ABI, separated from GUI and operation lifetime.
use crate::{
    active::Binding,
    actor::{DeviceHub, Scope},
    input_sessions::Principal,
    model::{AppError, Uid},
    transport::Completion,
};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
#[derive(Clone, Copy)]
pub enum Cleanup {
    Job(u32),
    User(u32),
    MediaInfo,
    Disk(u32),
    Optical(u32),
    Upload(u32),
    UploadStatus,
    CardFormat(u32),
}
impl Cleanup {
    pub fn matches(self, ch: u8, op: u16, payload: &[u8]) -> bool {
        match self {
            Self::Job(id) => {
                id != 0 && ch == 0 && [7, 8].contains(&op) && payload == id.to_le_bytes()
            }
            Self::User(id) => id != 0 && ch == 3 && op == 0x30e && payload == id.to_le_bytes(),
            Self::MediaInfo => ch == 3 && [0x305, 0x308, 0x318].contains(&op) && payload.is_empty(),
            Self::Disk(lease) => {
                lease != 0 && ch == 3 && op == 0x313 && payload == lease.to_le_bytes()
            }
            Self::Optical(id) => id != 0 && ch == 3 && op == 0x307 && payload == id.to_le_bytes(),
            Self::Upload(id) => id != 0 && ch == 0 && op == 9 && payload == id.to_le_bytes(),
            Self::UploadStatus => ch == 0 && op == 4 && payload.is_empty(),
            Self::CardFormat(id) => {
                (id != 0 && ch == 0 && op == 7 && payload == id.to_le_bytes())
                    || (payload.is_empty()
                        && ((ch == 0 && op == 4) || (ch == 3 && [0x308, 0x31e].contains(&op))))
            }
        }
    }
}
pub trait Wire: Send + Sync {
    fn session(&self) -> u64;
    fn live(&self) -> bool;
    fn send(
        &self,
        binding: Binding,
        owner: Principal,
        cancel: CancellationToken,
        ch: u8,
        op: u16,
        payload: Vec<u8>,
    ) -> BoxFuture<'static, Result<Vec<u8>, AppError>>;
    fn cleanup(&self, kind: Cleanup, op: u16) -> BoxFuture<'static, Result<Vec<u8>, AppError>>;
    fn verified_name(&self, _name: &str) -> Result<(), AppError> {
        Ok(())
    }
}
pub trait Backend: Send + Sync {
    fn connect(&self, binding: &Binding) -> Result<Arc<dyn Wire>, AppError>;
    fn prepare_target_change(&self, _binding: Binding) -> BoxFuture<'static, Result<(), AppError>> {
        Box::pin(async { Ok(()) })
    }
}
pub struct NativeBackend {
    pub devices: Arc<DeviceHub>,
    pub input: Arc<crate::input::InputManager>,
}
struct NativeWire {
    handle: Arc<crate::actor::DeviceHandle>,
    uid: Uid,
    session: u64,
}
impl Wire for NativeWire {
    fn session(&self) -> u64 {
        self.session
    }
    fn live(&self) -> bool {
        self.handle.session() == self.session && self.session != 0
    }
    fn send(
        &self,
        binding: Binding,
        owner: Principal,
        cancel: CancellationToken,
        ch: u8,
        op: u16,
        payload: Vec<u8>,
    ) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
        let handle = self.handle.clone();
        let session = self.session;
        Box::pin(async move {
            if handle.session() != session {
                return Err(AppError::new(
                    "STALE_SESSION",
                    "저장소 MCU 세션이 변경됐습니다.",
                ));
            }
            handle
                .request(
                    Scope::StorageActive {
                        binding,
                        cancel,
                        key: owner.cancelled,
                        client: owner.ended,
                        session,
                    },
                    ch,
                    op,
                    payload,
                    Completion::Ack,
                )
                .await
        })
    }
    fn cleanup(&self, kind: Cleanup, op: u16) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
        let handle = self.handle.clone();
        let uid = self.uid.clone();
        let session = self.session;
        Box::pin(async move {
            if handle.session() != session {
                return Err(AppError::new(
                    "STALE_SESSION",
                    "이전 저장소 세션이 종료됐습니다.",
                ));
            }
            let (ch, payload) = match kind {
                Cleanup::Job(id) | Cleanup::Upload(id) => (0, id.to_le_bytes().to_vec()),
                Cleanup::User(id) | Cleanup::Disk(id) | Cleanup::Optical(id) => {
                    (3, id.to_le_bytes().to_vec())
                }
                Cleanup::MediaInfo => (3, vec![]),
                Cleanup::UploadStatus => (0, vec![]),
                Cleanup::CardFormat(id) => match op {
                    7 => (0, id.to_le_bytes().to_vec()),
                    4 => (0, vec![]),
                    _ => (3, vec![]),
                },
            };
            handle
                .request(
                    Scope::StorageCleanup { uid, kind, session },
                    ch,
                    op,
                    payload,
                    Completion::Ack,
                )
                .await
        })
    }
    fn verified_name(&self, name: &str) -> Result<(), AppError> {
        self.handle.verified_name(self.session, name)
    }
}
impl Backend for NativeBackend {
    fn prepare_target_change(&self, binding: Binding) -> BoxFuture<'static, Result<(), AppError>> {
        let input = self.input.clone();
        Box::pin(async move { input.release_channel(&binding.uid).await })
    }
    fn connect(&self, b: &Binding) -> Result<Arc<dyn Wire>, AppError> {
        let handle = self.devices.handle(&b.uid)?;
        if handle.channel().is_none_or(|c| c.features & 4 == 0) {
            return Err(AppError::new(
                "UNSUPPORTED",
                "현재 MCU는 SD 저장소를 지원하지 않습니다.",
            ));
        }
        let session = handle.session();
        Ok(Arc::new(NativeWire {
            handle,
            uid: b.uid.clone(),
            session,
        }))
    }
}
pub fn invalid() -> AppError {
    AppError::new(
        "INVALID_RESPONSE",
        "저장소 응답의 길이/필드/순서를 확인하지 못했습니다.",
    )
}
pub fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|n| n.to_le_bytes()).collect()
}
pub fn u16at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}
pub fn u32at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
pub fn u64at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Card {
    pub present: bool,
    pub initialized: bool,
    pub user_available: bool,
    pub uuid: String,
    pub user_length: u16,
    pub revision: u32,
    pub stored_uid: Option<Uid>,
    pub current_uid: Uid,
    pub profile_status: u16,
    pub user_status: u16,
}
impl Card {
    pub fn decode(b: &[u8], uid: &Uid) -> Result<Self, AppError> {
        if b.len() != 52 || b[0] & !7 != 0 || b[1] != 0 || u16at(b, 2) > 4096 {
            return Err(invalid());
        }
        let current_uid = Uid::from_bytes(&b[36..48]).map_err(|_| invalid())?;
        if &current_uid != uid {
            return Err(invalid());
        }
        Ok(Self {
            present: b[0] & 1 != 0,
            initialized: b[0] & 2 != 0,
            user_available: b[0] & 4 != 0,
            uuid: b[8..24].iter().map(|n| format!("{n:02x}")).collect(),
            user_length: u16at(b, 2),
            revision: u32at(b, 4),
            stored_uid: (b[0] & 2 != 0).then(|| Uid::from_bytes(&b[24..36]).unwrap()),
            current_uid,
            profile_status: u16at(b, 48),
            user_status: u16at(b, 50),
        })
    }
    pub fn writable_metadata(&self) -> Result<(), AppError> {
        if !self.present {
            return Err(AppError::new("CARD_MISSING", "SD 카드가 없습니다."));
        }
        if !self.initialized {
            return Err(AppError::new(
                "CARD_NOT_READY",
                "SD 내부 식별정보를 준비 중입니다. 잠시 후 다시 시도하세요.",
            ));
        }
        if self.stored_uid.as_ref() != Some(&self.current_uid) {
            return Err(AppError::new(
                "CARD_UID_MISMATCH",
                "카드에 등록된 MCU UID가 현재 채널과 다릅니다.",
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Optical {
    pub id: u32,
    pub size: u64,
    pub mounted: bool,
    pub name: String,
    pub crc: u32,
}
impl Optical {
    pub fn decode(b: &[u8]) -> Result<Option<Self>, AppError> {
        if b.len() < 48 || b.len() != 48 + usize::from(u16at(b, 14)) || b[13] > 1 {
            return Err(invalid());
        }
        if u32at(b, 0) == 0 {
            return Ok(None);
        }
        Ok(Some(Self {
            id: u32at(b, 0),
            size: u64at(b, 4),
            mounted: b[13] != 0,
            name: String::from_utf8(b[48..].to_vec()).map_err(|_| invalid())?,
            crc: u32at(b, 16),
        }))
    }
}
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Disk {
    pub id: u32,
    pub sectors: u32,
    pub lease: u32,
    pub format: u8,
    pub state: u8,
    pub writable: bool,
}
impl Disk {
    pub fn decode(b: &[u8]) -> Result<Option<Self>, AppError> {
        if b.len() != 16 || b[15] != 0 || b[14] > 1 || b[13] > 5 {
            return Err(invalid());
        }
        if u32at(b, 8) == 0 {
            return Ok(None);
        }
        if u32at(b, 0) == 0 || u32at(b, 4) == 0 || !(1..=3).contains(&b[12]) {
            return Err(invalid());
        }
        Ok(Some(Self {
            id: u32at(b, 0),
            sectors: u32at(b, 4),
            lease: u32at(b, 8),
            format: b[12],
            state: b[13],
            writable: b[14] != 0,
        }))
    }
}
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Image {
    pub id: u32,
    pub kind: String,
    pub name: String,
    pub size: u64,
    pub format: u8,
    pub mounted: bool,
    pub path: u8,
    pub writable: bool,
}
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Snapshot {
    pub uid: Uid,
    pub generation: u64,
    pub observed_at: u64,
    pub stale: bool,
    pub card: Option<Card>,
    pub optical: Option<Optical>,
    pub disk: Option<Disk>,
    pub capabilities: u32,
    pub optical_enabled: bool,
    pub optical_applied: bool,
    pub images: Vec<Image>,
}
pub fn capabilities(b: &[u8]) -> Result<(u32, bool, bool), AppError> {
    if b.len() != 8 || b[4] > 1 || b[5] > 1 || b[6..8] != [0, 0] {
        return Err(invalid());
    }
    Ok((u32at(b, 0), b[4] != 0, b[5] != 0))
}
pub fn name_result(b: &[u8]) -> Result<String, AppError> {
    if b.len() < 4 || b[2..4] != [0, 0] || b.len() != 4 + usize::from(u16at(b, 0)) || b.len() > 68 {
        return Err(invalid());
    }
    String::from_utf8(b[4..].to_vec()).map_err(|_| invalid())
}
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct McuJob {
    pub id: u32,
    pub kind: u8,
    pub state: u8,
    pub result: u16,
    pub detail: u32,
    pub object: u32,
    pub done: u64,
    pub total: u64,
}
impl McuJob {
    pub fn decode(b: &[u8], id: u32, kind: u8) -> Result<Self, AppError> {
        if b.len() != 32 || u32at(b, 0) != id || b[5] != kind || b[4] > 3 {
            return Err(invalid());
        }
        Ok(Self {
            id,
            kind,
            state: b[4],
            result: u16at(b, 6),
            detail: u32at(b, 8),
            object: u32at(b, 12),
            done: u64at(b, 16),
            total: u64at(b, 24),
        })
    }
}
pub fn channel_name(input: &str) -> Result<String, AppError> {
    let trimmed = input.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    #[cfg(windows)]
    let name = {
        use windows::Win32::Globalization::{NormalizationC, NormalizeString};
        let source: Vec<_> = trimmed.encode_utf16().collect();
        let count = unsafe { NormalizeString(NormalizationC, &source, None) };
        if count <= 0 || count > 1024 {
            return Err(AppError::new(
                "INVALID_ARGUMENTS",
                "채널 이름을 정규화하지 못했습니다.",
            ));
        }
        let mut out = vec![0u16; count as usize];
        let count = unsafe { NormalizeString(NormalizationC, &source, Some(&mut out)) };
        if count <= 0 {
            return Err(invalid());
        }
        String::from_utf16(&out[..count as usize]).map_err(|_| invalid())?
    };
    #[cfg(not(windows))]
    let name = trimmed.to_owned();
    if name.len() > 64
        || name
            .chars()
            .any(|c| c.is_control() || c == '\u{2028}' || c == '\u{2029}')
    {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "채널 이름은 제어문자 없이 UTF-8 64바이트까지 입력하세요.",
        ));
    }
    Ok(name)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf8_names_and_cleanup_are_strict() {
        assert_eq!(channel_name(" \u{1100}\u{1161} ").unwrap(), "가");
        assert!(channel_name(&"가".repeat(22)).is_err());
        assert!(channel_name("line\nname").is_err());
        assert_eq!(channel_name("\u{feff} name \u{feff}").unwrap(), "name");
        assert!(Cleanup::Job(3).matches(0, 8, &words(&[3])));
        assert!(!Cleanup::Job(3).matches(3, 0x30f, &words(&[3])));
        assert!(!Cleanup::Job(3).matches(0, 8, &words(&[4])));
        assert!(!Cleanup::User(4).matches(3, 0x30d, &words(&[4])));
        assert!(Cleanup::Disk(10).matches(3, 0x313, &words(&[10])));
        assert!(!Cleanup::Disk(10).matches(3, 0x315, &words(&[10])));
        assert!(!Cleanup::Optical(2).matches(3, 0x306, &words(&[2])));
        assert!(!Cleanup::MediaInfo.matches(3, 0x308, &words(&[1])));
        assert!(Cleanup::MediaInfo.matches(3, 0x308, &[]));
        assert!(name_result(&[2, 0, 0, 0, b'a']).is_err());
        assert!(capabilities(&[1, 0, 0, 0, 1, 1, 1, 0]).is_err());
    }
}
