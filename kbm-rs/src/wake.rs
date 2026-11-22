//! Explicit host LAN WOL, independent of USB/NCM and never automatic.
use crate::{
    active::Binding, input_sessions::Principal, model::AppError, preferences::Wol,
    service::AppService,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    net::{Ipv4Addr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Adapter {
    pub name: String,
    pub address: String,
    pub broadcast: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[repr(u16)]
pub enum Code {
    SentUnconfirmed = 128,
    IgnoredDisabled = 129,
    IgnoredUnsupported = 130,
}
impl Code {
    pub fn message(self) -> &'static str {
        match self {
            Self::SentUnconfirmed => "WOL 패킷 발신 (지원 여부·기상 확인 불가)",
            Self::IgnoredDisabled => "무시: WOL 비활성",
            Self::IgnoredUnsupported => "무시: 이 시스템의 UDP 브로드캐스트 미지원",
        }
    }
}
pub fn normalize_mac(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    let valid = if value.len() == 12 {
        value.bytes().all(|b| b.is_ascii_hexdigit())
    } else if value.len() == 17 {
        let bytes = value.as_bytes();
        let separator = bytes[2];
        matches!(separator, b':' | b'-')
            && bytes.iter().enumerate().all(|(i, b)| {
                if i % 3 == 2 {
                    *b == separator
                } else {
                    b.is_ascii_hexdigit()
                }
            })
    } else {
        false
    };
    if !valid {
        return Err(invalid_mac());
    }
    let raw = value.replace([':', '-'], "").to_ascii_lowercase();
    if u8::from_str_radix(&raw[..2], 16).map_err(|_| invalid_mac())? & 1 != 0
        || raw == "000000000000"
    {
        return Err(invalid_mac());
    }
    Ok((0..6)
        .map(|i| raw[i * 2..i * 2 + 2].to_owned())
        .collect::<Vec<_>>()
        .join(":"))
}
fn invalid_mac() -> AppError {
    AppError::new(
        "INVALID_MAC",
        "대상 PC의 유니캐스트 유선 LAN MAC 주소를 확인하세요.",
    )
}
pub fn magic_packet(mac: &str) -> Result<[u8; 102], AppError> {
    let mac = normalize_mac(mac)?;
    let bytes = mac
        .split(':')
        .map(|p| u8::from_str_radix(p, 16).map_err(|_| invalid_mac()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut packet = [255; 102];
    for chunk in packet[6..].chunks_exact_mut(6) {
        chunk.copy_from_slice(&bytes);
    }
    Ok(packet)
}
pub fn directed_broadcast(ip: Ipv4Addr, prefix: u8) -> Option<Ipv4Addr> {
    if !(1..=30).contains(&prefix)
        || ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || ip.is_broadcast()
    {
        return None;
    }
    let host = (1u32 << (32 - prefix)) - 1;
    Some(Ipv4Addr::from(u32::from(ip) | host))
}
pub trait Backend: Send + Sync {
    fn adapters(&self) -> Result<Vec<Adapter>, AppError>;
    fn send(
        &self,
        adapter: &Adapter,
        packet: &[u8; 102],
        guard: &dyn Fn() -> Result<(), AppError>,
    ) -> Result<Code, AppError>;
}
struct Native;
impl Backend for Native {
    fn adapters(&self) -> Result<Vec<Adapter>, AppError> {
        #[cfg(windows)]
        {
            windows::adapters()
        }
        #[cfg(not(windows))]
        {
            Ok(vec![])
        }
    }
    fn send(
        &self,
        adapter: &Adapter,
        packet: &[u8; 102],
        guard: &dyn Fn() -> Result<(), AppError>,
    ) -> Result<Code, AppError> {
        let send = || -> Result<(), AppError> {
            let local = adapter
                .address
                .parse::<Ipv4Addr>()
                .map_err(|_| io_error())?;
            let remote = adapter
                .broadcast
                .parse::<Ipv4Addr>()
                .map_err(|_| io_error())?;
            let socket =
                std::net::UdpSocket::bind(SocketAddrV4::new(local, 0)).map_err(socket_error)?;
            socket
                .set_write_timeout(Some(Duration::from_secs(2)))
                .map_err(socket_error)?;
            socket.set_broadcast(true).map_err(socket_error)?;
            guard()?;
            let n = socket
                .send_to(packet, SocketAddrV4::new(remote, 9))
                .map_err(socket_error)?;
            if n != packet.len() {
                return Err(io_error());
            }
            Ok(())
        };
        match send() {
            Ok(()) => Ok(Code::SentUnconfirmed),
            Err(e) if e.code == "WOL_UNSUPPORTED" => Ok(Code::IgnoredUnsupported),
            Err(e) => Err(e),
        }
    }
}
fn socket_error(error: std::io::Error) -> AppError {
    if error.kind() == std::io::ErrorKind::Unsupported
        || matches!(error.raw_os_error(), Some(10045 | 10047))
    {
        AppError::new("WOL_UNSUPPORTED", "UDP 브로드캐스트를 지원하지 않습니다.")
    } else {
        io_error()
    }
}
pub(super) fn io_error() -> AppError {
    AppError::new(
        "WOL_IO",
        "LAN 어댑터를 조회하거나 WOL을 발신하지 못했습니다. 선택한 연결을 확인하세요.",
    )
}
pub struct Service {
    backend: Arc<dyn Backend>,
    slots: Arc<tokio::sync::Semaphore>,
}
impl Default for Service {
    fn default() -> Self {
        Self::new(Arc::new(Native))
    }
}
impl Service {
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self {
            backend,
            slots: Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Empty {}
pub fn is_command(name: &str) -> bool {
    name.starts_with("wake_")
}
pub fn schema(name: &str) -> Option<Value> {
    Some(match name {
        "wake_configure" => serde_json::to_value(schemars::schema_for!(Wol)).unwrap(),
        "wake_get_status" | "wake_list_adapters" | "wake_send" => {
            serde_json::to_value(schemars::schema_for!(Empty)).unwrap()
        }
        _ => return None,
    })
}
fn check(app: &AppService, b: &Binding, owner: &Principal) -> Result<(), AppError> {
    if !app.is_running() {
        return Err(AppError::new("OPERATION_CANCELLED", "앱이 종료 중입니다."));
    }
    if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
        return Err(AppError::new(
            "UNAUTHORIZED",
            "인증 또는 연결이 종료됐습니다.",
        ));
    }
    app.active.validate(b)
}
impl AppService {
    pub async fn execute_wake(
        self: &Arc<Self>,
        b: Binding,
        owner: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        let binding = b.clone();
        let result = self.execute_wake_inner(b, owner, name, args).await;
        self.audit(name, Some(&binding), result)
    }
    async fn execute_wake_inner(
        self: &Arc<Self>,
        b: Binding,
        owner: &Principal,
        name: &str,
        args: Value,
    ) -> Result<Value, AppError> {
        check(self, &b, owner)?;
        let mut config = if name == "wake_configure" {
            Some(serde_json::from_value::<Wol>(args).map_err(|_| {
                AppError::new(
                    "INVALID_ARGUMENTS",
                    "WOL 설정을 확인하세요. UID·목적지·포트 인자는 받지 않습니다.",
                )
            })?)
        } else {
            let _: Empty = serde_json::from_value(args).map_err(|_| {
                AppError::new("INVALID_ARGUMENTS", "WOL 일반 명령은 빈 인자를 사용합니다.")
            })?;
            None
        };
        if let Some(config) = &mut config {
            if !config.mac.is_empty() {
                config.mac = normalize_mac(&config.mac)?;
            }
            config.validate()?;
        }
        let permit = self
            .wake
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::new("QUEUE_FULL", "WOL 작업이 진행 중입니다."))?;
        let app = self.clone();
        let owner = owner.clone();
        let name = name.to_owned();
        tokio::task::spawn_blocking(move||{
            let _permit=permit;let guard=||check(&app,&b,&owner);guard()?;
            match name.as_str(){
                "wake_get_status"=>Ok(json!({"uid":b.uid,"generation":b.generation,"config":app.preferences.channel(&b.uid).wol.unwrap_or_default(),"usb_wake_supported":false,"system_control_supported":false})),
                "wake_configure"=>{let config=config.unwrap();app.preferences.wol(&b.uid,config.clone())?;guard()?;Ok(json!({"config":config}))},
                "wake_list_adapters"=>{let adapters=app.wake.backend.adapters()?;guard()?;Ok(json!({"adapters":adapters}))},
                "wake_send"=>{
                    let config=app.preferences.channel(&b.uid).wol.unwrap_or_default();config.validate()?;
                    let code=if !config.enabled {Code::IgnoredDisabled}else{
                        let packet=magic_packet(&config.mac)?;
                        let matches=app.wake.backend.adapters()?.into_iter().filter(|a|a.address==config.local_address).collect::<Vec<_>>();
                        if matches.len()!=1{return Err(AppError::new("WOL_ADAPTER_MISSING","선택한 LAN IPv4 주소가 없거나 모호합니다. 어댑터를 다시 선택하세요."));}
                        guard()?;app.wake.backend.send(&matches[0],&packet,&guard)?
                    };
                    Ok(json!({"uid":b.uid,"generation":b.generation,"code":code as u16,"message":code.message(),"wake_confirmed":false}))
                },
                _=>Err(AppError::new("NOT_IMPLEMENTED","WOL 명령을 찾을 수 없습니다.")),
            }
        }).await.map_err(|_|AppError::uncertain("WOL_FAILED","WOL 작업 결과를 확인하지 못했습니다. 자동 재발신하지 않습니다."))?
    }
}
