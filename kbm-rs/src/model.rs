use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "String", into = "String")]
pub struct Uid(String);
impl TryFrom<String> for Uid {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let raw = value.strip_prefix("0x").unwrap_or(&value);
        if raw.len() != 24 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("UID must contain 12 hexadecimal bytes");
        }
        Ok(Self(raw.to_ascii_uppercase()))
    }
}
impl From<Uid> for String {
    fn from(value: Uid) -> Self {
        format!("0x{}", value.0)
    }
}
impl Uid {
    pub fn raw(&self) -> &str {
        &self.0
    }
    pub fn display(&self) -> String {
        format!("0x{}", self.0)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() != 12 {
            return Err("UID must contain 12 bytes");
        }
        Self::try_from(bytes.iter().map(|b| format!("{b:02X}")).collect::<String>())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Channel {
    pub uid: Uid,
    pub name: String,
    pub port: String,
    pub connected: bool,
    pub features: u32,
    pub target_state: u8,
    pub firmware: [u16; 3],
    pub board_id: String,
    pub usb_port: u32,
    pub slot: Option<u8>,
    pub capture_id: Option<String>,
    pub pairing_error: String,
    pub observed_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Inventory {
    pub revision: u64,
    pub observed_at: u64,
    pub channels: Vec<Channel>,
    pub error: String,
}

#[derive(Clone, Debug, thiserror::Error, Serialize, schemars::JsonSchema)]
#[error("{message}")]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub uncertain: bool,
    #[serde(skip)]
    pub(crate) origin: Option<ErrorSource>,
}
#[derive(Clone, Debug)]
pub(crate) struct ErrorSource {
    pub binding: crate::active::Binding,
    pub job_id: Option<String>,
}
impl AppError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            status: None,
            uncertain: false,
            origin: None,
        }
    }
    pub fn mcu(status: u16) -> Self {
        Self {
            code: "MCU_REJECTED",
            message: format!("MCU가 요청을 거부했습니다. (상태 {status})"),
            status: Some(status),
            uncertain: false,
            origin: None,
        }
    }
    pub fn uncertain(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            uncertain: true,
            ..Self::new(code, message)
        }
    }
    pub(crate) fn at(mut self, binding: &crate::active::Binding) -> Self {
        if self.origin.is_none() {
            self.origin = Some(ErrorSource {
                binding: binding.clone(),
                job_id: None,
            });
        }
        self
    }
    pub(crate) fn at_job(mut self, binding: &crate::active::Binding, job_id: &str) -> Self {
        self = self.at(binding);
        if let Some(source) = self.origin.as_mut().filter(|s| s.binding == *binding) {
            source.job_id = Some(job_id.into());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uid_roundtrip_preserves_bytes_and_leading_zeroes() {
        let uid = Uid::from_bytes(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xab, 0xff]).unwrap();
        assert_eq!(uid.display(), "0x00010203040506070809ABFF");
        assert_eq!(
            serde_json::from_str::<Uid>(&serde_json::to_string(&uid).unwrap()).unwrap(),
            uid
        );
        assert!(Uid::try_from("1234".to_owned()).is_err());
    }
}
