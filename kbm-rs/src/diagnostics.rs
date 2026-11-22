//! Cached v1 STATUS64, never a feature-enable or raw terminal read.
use crate::{
    active::Binding,
    input_sessions::Principal,
    model::{now_ms, AppError},
    service::AppService,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct McuStatus {
    pub observed_at: u64,
    pub uptime_ms: u64,
    pub features: u32,
    pub target_state: u8,
    pub storage_state: u8,
    pub management_cdc_open: bool,
    pub network_up: bool,
    pub storage_job: u32,
    pub image_id: u32,
    pub upload_id: u32,
    pub log_dropped: u32,
    pub crc_errors: u32,
    pub framing_errors: u32,
    pub storage_received: u64,
    pub storage_total: u64,
    pub storage_error: u16,
    pub event_dropped: u32,
}
impl McuStatus {
    pub fn parse(bytes: &[u8], observed_at: u64) -> Result<Self, AppError> {
        if bytes.len() != 64 {
            return Err(AppError::new(
                "INVALID_RESPONSE",
                "MCU 상태 응답 길이를 확인할 수 없습니다.",
            ));
        }
        let u32at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let u64at = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        Ok(Self {
            observed_at,
            uptime_ms: u64at(0),
            features: u32at(8),
            target_state: bytes[12],
            storage_state: bytes[13],
            management_cdc_open: bytes[14] != 0,
            network_up: bytes[15] != 0,
            storage_job: u32at(16),
            image_id: u32at(20),
            upload_id: u32at(24),
            log_dropped: u32at(28),
            crc_errors: u32at(32),
            framing_errors: u32at(36),
            storage_received: u64at(40),
            storage_total: u64at(48),
            storage_error: u16::from_le_bytes(bytes[56..58].try_into().unwrap()),
            event_dropped: u32at(60),
        })
    }
    pub fn fresh_at(&self, now: u64) -> bool {
        now >= self.observed_at && now - self.observed_at <= 1500
    }
}
#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct Transport {
    pub session: u64,
    pub event_gaps: u64,
    pub crc_rejected: u64,
    pub faulted: bool,
    pub mcu: Option<McuStatus>,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Empty {}
pub fn schema(name: &str) -> Option<Value> {
    (name == "diagnostics_get").then(|| serde_json::to_value(schemars::schema_for!(Empty)).unwrap())
}
impl AppService {
    pub fn diagnostics(&self, b: &Binding) -> Result<Value, AppError> {
        self.active.validate(b)?;
        let transport = self
            .devices
            .handle(&b.uid)
            .ok()
            .map(|handle| handle.diagnostics());
        let fresh = transport
            .as_ref()
            .and_then(|t| t.mcu.as_ref())
            .is_some_and(|mcu| mcu.fresh_at(now_ms()));
        let input = self.input.snapshot(b);
        let tty = self.tty.cached(b);
        let network = self.network.status(b);
        let job = self.storage.current(b);
        let channel=self.inventory.read().channels.iter().find(|c|c.uid==b.uid).map(|c|json!({"connected":c.connected,"features":c.features,"firmware":c.firmware,"slot":c.slot,"capture_paired":c.capture_id.is_some(),"target_state":c.target_state}));
        let result = json!({"uid":b.uid,"generation":b.generation,"observed_at":now_ms(),"channel":channel,"mcu_fresh":fresh,"transport":transport,
            "input":{"ready":input.ready,"keys_held":input.keys_held,"buttons_held":input.buttons_held,"release_unconfirmed":input.release_unconfirmed,"error_present":!input.error.is_empty()},
            "video":self.video_status(b),
            "tty":tty.map(|s|json!({"receiving":s.receiving,"bytes_received":s.bytes_received,"bytes_retained":s.bytes_retained,"bytes_overwritten":s.bytes_overwritten,"bytes_accepted":s.bytes_accepted,"tx_discarded":s.line.tx_discarded,"rx_discarded":s.line.rx_discarded,"error_present":!s.error.is_empty()})),
            "network":{"mode":network.mode,"mcu":network.mcu,"cleanup_required":network.cleanup_required,"error_code":network.error.map(|e|e.code)},
            "storage_job":job.map(|j|json!({"kind":j.kind,"state":j.state,"done":j.done,"total":j.total,"can_cancel":j.can_cancel,"error_code":j.error.map(|e|e.code),"cleanup_error_code":j.cleanup_error.map(|e|e.code)})),
            "logging":{"dropped":self.logs.status().dropped,"write_error":self.logs.status().write_error.map(|e|e.code)}});
        self.active.validate(b)?;
        Ok(result)
    }
    pub async fn execute_diagnostics(
        self: &Arc<Self>,
        owner: &Principal,
        args: Value,
    ) -> Result<Value, AppError> {
        let _: Empty = serde_json::from_value(args).map_err(|_| {
            AppError::new(
                "INVALID_ARGUMENTS",
                "진단 조회는 UID나 추가 인자를 받지 않습니다.",
            )
        })?;
        if owner.cancelled.is_cancelled() || owner.ended.is_cancelled() {
            return Err(AppError::new(
                "UNAUTHORIZED",
                "인증 또는 연결이 종료됐습니다.",
            ));
        }
        self.diagnostics(&self.active.bind()?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status64_offsets_and_freshness_are_exact() {
        let mut bytes = [0u8; 64];
        bytes[..8].copy_from_slice(&123456u64.to_le_bytes());
        bytes[8..12].copy_from_slice(&0x77u32.to_le_bytes());
        bytes[12] = 2;
        bytes[13] = 3;
        bytes[14] = 1;
        bytes[15] = 1;
        for (i, offset) in [16, 20, 24, 28, 32, 36, 60].iter().enumerate() {
            bytes[*offset..*offset + 4].copy_from_slice(&((i + 1) as u32).to_le_bytes());
        }
        bytes[40..48].copy_from_slice(&4000000000u64.to_le_bytes());
        bytes[48..56].copy_from_slice(&5000000000u64.to_le_bytes());
        bytes[56..58].copy_from_slice(&123u16.to_le_bytes());
        let s = McuStatus::parse(&bytes, 10000).unwrap();
        assert_eq!(
            (s.uptime_ms, s.features, s.target_state, s.storage_state),
            (123456, 0x77, 2, 3)
        );
        assert_eq!(
            (
                s.log_dropped,
                s.crc_errors,
                s.framing_errors,
                s.event_dropped
            ),
            (4, 5, 6, 7)
        );
        assert_eq!(
            (s.storage_received, s.storage_total, s.storage_error),
            (4000000000, 5000000000, 123)
        );
        assert!(s.fresh_at(11500));
        assert!(!s.fresh_at(11501));
        assert!(!s.fresh_at(9999));
        assert!(McuStatus::parse(&bytes[..63], 0).is_err());
    }
    #[tokio::test]
    async fn cached_read_never_enables_features_or_exposes_terminal_payload() {
        let (_dir, app, wire) = crate::input_api::tests::setup();
        let b = app.active.bind().unwrap();
        let before = wire.calls.lock().len();
        let d = app.diagnostics(&b).unwrap();
        assert_eq!(wire.calls.lock().len(), before);
        assert!(d["transport"].is_null());
        assert_eq!(d["mcu_fresh"], false);
        assert!(d["tty"].is_null());
        assert_eq!(d["uid"], json!(b.uid));
        app.active.begin().unwrap();
        app.active.finish(None);
        assert_eq!(app.diagnostics(&b).unwrap_err().code, "STALE_OPERATION");
        app.shutdown().await;
    }
}
