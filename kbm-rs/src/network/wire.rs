//! Session-pinned NCM backend. No arbitrary NIC or CDC handle may be selected.
use crate::{
    active::Binding,
    actor::{DeviceEvent, DeviceHub, Scope},
    input_sessions::Principal,
    model::{AppError, Uid},
    transport::Completion,
};
use futures_util::future::BoxFuture;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
pub fn allowed(ch: u8, op: u16, p: &[u8]) -> bool {
    (ch == 4
        && ((op == 0x401 && p.len() == 1 && p[0] <= 1)
            || (op == 0x402 && (17..=1024).contains(&p.len()) && p[12..16] == [0; 4])
            || (op == 0x403 && p.is_empty())))
        || (ch == 0 && op == 6 && p.len() == 12 && p[..4] == [4, 0, 0, 0])
}
pub trait Wire: Send + Sync {
    fn live(&self) -> bool;
    fn send(
        &self,
        b: Binding,
        owner: Principal,
        life: CancellationToken,
        ch: u8,
        op: u16,
        p: Vec<u8>,
    ) -> BoxFuture<'static, Result<Vec<u8>, AppError>>;
    fn cleanup(&self, op: u16) -> BoxFuture<'static, Result<Vec<u8>, AppError>>;
}
pub struct Connection {
    pub session: u64,
    pub wire: Arc<dyn Wire>,
    pub events: broadcast::Receiver<DeviceEvent>,
}
pub trait Backend: Send + Sync {
    fn connect(&self, b: &Binding) -> Result<Connection, AppError>;
    fn host(&self) -> BoxFuture<'static, Result<super::host_io::Host, AppError>> {
        Box::pin(async {
            Err(AppError::new(
                "DNS_UNAVAILABLE",
                "호스트 네트워크 공유를 준비하지 못했습니다.",
            ))
        })
    }
    fn prepare_target_change(&self, _b: Binding) -> BoxFuture<'static, Result<(), AppError>> {
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
fn disconnected() -> AppError {
    AppError::new("STALE_SESSION", "원래 NCM 관리 USB 세션이 종료됐습니다.")
}
impl Backend for NativeBackend {
    fn host(&self) -> BoxFuture<'static, Result<super::host_io::Host, AppError>> {
        Box::pin(super::host_io::Host::native())
    }
    fn prepare_target_change(&self, b: Binding) -> BoxFuture<'static, Result<(), AppError>> {
        let input = self.input.clone();
        Box::pin(async move { input.release_channel(&b.uid).await })
    }
    fn connect(&self, b: &Binding) -> Result<Connection, AppError> {
        let handle = self.devices.handle(&b.uid)?;
        if handle.channel().is_none_or(|c| c.features & 32 == 0) {
            return Err(AppError::new(
                "UNSUPPORTED",
                "현재 MCU는 NCM을 지원하지 않습니다.",
            ));
        }
        let session = handle.session();
        if session == 0 {
            return Err(disconnected());
        }
        let events = handle.subscribe_channel(4)?;
        Ok(Connection {
            session,
            events,
            wire: Arc::new(NativeWire {
                handle,
                uid: b.uid.clone(),
                session,
            }),
        })
    }
}
impl Wire for NativeWire {
    fn live(&self) -> bool {
        self.session != 0 && self.handle.session() == self.session
    }
    fn send(
        &self,
        b: Binding,
        owner: Principal,
        life: CancellationToken,
        ch: u8,
        op: u16,
        p: Vec<u8>,
    ) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
        let handle = self.handle.clone();
        let session = self.session;
        Box::pin(async move {
            if handle.session() != session {
                return Err(disconnected());
            }
            if !allowed(ch, op, &p) {
                return Err(AppError::new(
                    "INVALID_COMMAND",
                    "NCM 요청 범위가 아닙니다.",
                ));
            }
            handle
                .request(
                    Scope::NetworkActive {
                        binding: b,
                        life,
                        key: owner.cancelled,
                        client: owner.ended,
                        session,
                    },
                    ch,
                    op,
                    p,
                    Completion::Ack,
                )
                .await
        })
    }
    fn cleanup(&self, op: u16) -> BoxFuture<'static, Result<Vec<u8>, AppError>> {
        let handle = self.handle.clone();
        let session = self.session;
        let uid = self.uid.clone();
        Box::pin(async move {
            if ![0x401, 0x403].contains(&op) {
                return Err(AppError::new(
                    "INVALID_COMMAND",
                    "NCM 정리는 OFF·상태 읽기만 허용합니다.",
                ));
            }
            if handle.session() != session {
                return Err(disconnected());
            }
            handle
                .request(
                    Scope::NetworkCleanup { uid, session },
                    4,
                    op,
                    if op == 0x401 { vec![0] } else { vec![] },
                    Completion::Ack,
                )
                .await
        })
    }
}
