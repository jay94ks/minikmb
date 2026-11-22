//! Selected-channel NCM service. No host NIC/TAP/route/ICS configuration.
pub mod dhcp;
pub mod flow_budget;
pub mod forwarding;
pub mod framing;
pub mod gateway;
#[cfg(windows)]
mod host_dns;
pub mod host_io;
pub mod ipv4;
pub mod manager;
pub mod scanner;
pub mod stack;
#[cfg(test)]
mod tests;
pub mod wire;
use crate::model::AppError;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server_ip: String,
    pub pool_start: u8,
    pub pool_end: u8,
    pub lease_seconds: u32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            server_ip: "192.168.127.1".into(),
            pool_start: 100,
            pool_end: 199,
            lease_seconds: 3600,
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<Ipv4Addr, AppError> {
        let ip = self.server_ip.parse::<Ipv4Addr>().map_err(|_| invalid())?;
        if ip.to_string() != self.server_ip
            || !ip.is_private()
            || ip.octets()[3] != 1
            || self.pool_start < 2
            || self.pool_start >= self.pool_end
            || self.pool_end > 254
            || !(60..=86400).contains(&self.lease_seconds)
        {
            return Err(invalid());
        }
        Ok(ip)
    }
    pub fn target(&self, ip: Ipv4Addr) -> Result<(), AppError> {
        let server = self.validate()?;
        if ip.octets()[..3] != server.octets()[..3] || [0, 1, 255].contains(&ip.octets()[3]) {
            return Err(AppError::new(
                "INVALID_ARGUMENTS",
                "타깃은 선택 채널의 /24 주소이며 서버/네트워크/브로드캐스트 주소일 수 없습니다.",
            ));
        }
        Ok(())
    }
}
fn invalid() -> AppError {
    AppError::new("INVALID_ARGUMENTS","서버는 사설 IPv4 서브넷의 .1, DHCP 범위는2..254의 최소2개 주소, 임대 시간은60..86400초여야 합니다.")
}
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    Emulation,
    Host,
}
