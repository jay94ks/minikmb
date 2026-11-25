//! Remote input leases. IDs never replace API authentication or select a UID.
use crate::{
    active::{ActiveCoordinator, Binding},
    input::Owner,
    model::{now_ms, AppError, Uid},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const TTL: Duration = Duration::from_secs(30);
#[derive(Clone)]
pub struct Principal {
    pub key_id: String,
    pub client_id: String,
    pub cancelled: CancellationToken,
    pub ended: CancellationToken,
}
impl Principal {
    pub fn rest(auth: &crate::auth::AuthLease) -> Self {
        Self {
            key_id: auth.info.id.clone(),
            client_id: "rest".into(),
            cancelled: auth.revoked.clone(),
            ended: CancellationToken::new(),
        }
    }
}
struct Lease {
    key: String,
    client: String,
    binding: Binding,
    owner: Owner,
    deadline: Instant,
    automatic: bool,
    ended: CancellationToken,
}
#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
pub struct SessionInfo {
    pub input_session: String,
    pub uid: Uid,
    pub generation: u64,
    pub expires_at: u64,
}
#[derive(Default)]
pub struct InputSessions {
    leases: Mutex<HashMap<String, Lease>>,
}
impl InputSessions {
    pub fn open(
        &self,
        active: &ActiveCoordinator,
        principal: &Principal,
    ) -> Result<SessionInfo, AppError> {
        let binding = active.bind()?;
        let mut leases = self.leases.lock();
        self.insert(&mut leases, principal, binding, false)
    }
    fn insert(
        &self,
        leases: &mut HashMap<String, Lease>,
        p: &Principal,
        b: Binding,
        automatic: bool,
    ) -> Result<SessionInfo, AppError> {
        if p.cancelled.is_cancelled() || p.ended.is_cancelled() {
            return Err(AppError::new(
                "UNAUTHORIZED",
                "인증 또는 연결이 종료됐습니다.",
            ));
        }
        if leases.len() >= 64 {
            return Err(AppError::new(
                "INPUT_SESSION_LIMIT",
                "사용하지 않는 입력 세션을 먼저 닫으세요.",
            ));
        }
        let mut random = [0; 16];
        getrandom::fill(&mut random)
            .map_err(|_| AppError::new("RANDOM_FAILED", "입력 세션 난수 생성에 실패했습니다."))?;
        let id = URL_SAFE_NO_PAD.encode(random);
        let info = SessionInfo {
            input_session: id.clone(),
            uid: b.uid.clone(),
            generation: b.generation,
            expires_at: now_ms() + TTL.as_millis() as u64,
        };
        leases.insert(
            id.clone(),
            Lease {
                key: p.key_id.clone(),
                client: p.client_id.clone(),
                binding: b,
                owner: Owner {
                    pin: Default::default(),
                    id: format!("remote:{id}"),
                    cancelled: p.cancelled.child_token(),
                    command_guard: None,
                },
                deadline: Instant::now() + TTL,
                automatic,
                ended: p.ended.clone(),
            },
        );
        Ok(info)
    }
    fn verify(lease: &Lease, p: &Principal) -> Result<(), AppError> {
        if lease.key != p.key_id || lease.client != p.client_id {
            return Err(AppError::new(
                "FORBIDDEN",
                "다른 클라이언트의 입력 세션입니다.",
            ));
        }
        if p.cancelled.is_cancelled()
            || p.ended.is_cancelled()
            || lease.ended.is_cancelled()
            || lease.owner.cancelled.is_cancelled()
        {
            return Err(AppError::new(
                "INPUT_SESSION_EXPIRED",
                "입력 세션이 종료됐습니다.",
            ));
        }
        if Instant::now() >= lease.deadline {
            lease.owner.cancelled.cancel();
            return Err(AppError::new(
                "INPUT_SESSION_EXPIRED",
                "입력 세션이 만료됐습니다.",
            ));
        }
        Ok(())
    }
    pub fn resolve(
        &self,
        active: &ActiveCoordinator,
        p: &Principal,
        id: Option<&str>,
    ) -> Result<(Owner, Binding), AppError> {
        let current = active.bind()?;
        let mut leases = self.leases.lock();
        let id = if let Some(id) = id {
            if id.len() > 64 {
                return Err(AppError::new(
                    "INVALID_ARGUMENTS",
                    "입력 세션 번호가 잘못됐습니다.",
                ));
            }
            id.to_owned()
        } else if let Some((id, _)) = leases
            .iter()
            .find(|(_, l)| l.automatic && l.key == p.key_id && l.client == p.client_id)
        {
            id.clone()
        } else {
            self.insert(&mut leases, p, current.clone(), true)?
                .input_session
        };
        let lease = leases.get_mut(&id).ok_or_else(|| {
            AppError::new("INPUT_SESSION_NOT_FOUND", "입력 세션을 찾을 수 없습니다.")
        })?;
        Self::verify(lease, p).map_err(|e| {
            if lease.key == p.key_id && lease.client == p.client_id {
                e.at(&lease.binding)
            } else {
                e
            }
        })?;
        if lease.binding != current {
            lease.owner.cancelled.cancel();
            return Err(
                AppError::new("STALE_OPERATION", "채널 전환 전 입력 세션입니다.")
                    .at(&lease.binding),
            );
        }
        lease.deadline = Instant::now() + TTL;
        Ok((lease.owner.clone(), lease.binding.clone()))
    }
    pub fn heartbeat(
        &self,
        active: &ActiveCoordinator,
        p: &Principal,
        id: &str,
    ) -> Result<SessionInfo, AppError> {
        let _ = self.resolve(active, p, Some(id))?;
        let leases = self.leases.lock();
        let lease = leases
            .get(id)
            .ok_or_else(|| AppError::new("INPUT_SESSION_EXPIRED", "입력 세션이 종료됐습니다."))?;
        Ok(SessionInfo {
            input_session: id.into(),
            uid: lease.binding.uid.clone(),
            generation: lease.binding.generation,
            expires_at: now_ms() + TTL.as_millis() as u64,
        })
    }
    pub fn close(&self, p: &Principal, id: &str) -> Result<(String, Binding), AppError> {
        let mut leases = self.leases.lock();
        let lease = leases.get(id).ok_or_else(|| {
            AppError::new("INPUT_SESSION_NOT_FOUND", "입력 세션을 찾을 수 없습니다.")
        })?;
        // Expired leases remain closable by their original authenticated client.
        if lease.key != p.key_id || lease.client != p.client_id {
            return Err(AppError::new(
                "FORBIDDEN",
                "다른 클라이언트의 입력 세션입니다.",
            ));
        }
        let lease = leases.remove(id).unwrap();
        lease.owner.cancelled.cancel();
        Ok((lease.owner.id, lease.binding))
    }
    pub fn reap(&self, active: &ActiveCoordinator) -> Vec<String> {
        self.remove_where(|l| {
            l.owner.cancelled.is_cancelled()
                || l.ended.is_cancelled()
                || Instant::now() >= l.deadline
                || active.validate(&l.binding).is_err()
        })
    }
    pub fn cancel_client(&self, key: &str, client: &str) -> Vec<String> {
        self.remove_where(|l| l.key == key && l.client == client)
    }
    pub fn cancel_network_clients(&self) -> Vec<String> {
        self.remove_where(|lease| lease.client == "rest" || lease.client.starts_with("mcp:"))
    }
    pub fn cancel_all(&self) -> Vec<String> {
        self.remove_where(|_| true)
    }
    pub fn cancel_mcp(&self) -> Vec<String> {
        self.remove_where(|l| l.client.starts_with("mcp:"))
    }
    fn remove_where(&self, test: impl Fn(&Lease) -> bool) -> Vec<String> {
        let mut leases = self.leases.lock();
        let ids: Vec<_> = leases
            .iter()
            .filter(|(_, l)| test(l))
            .map(|(id, _)| id.clone())
            .collect();
        ids.into_iter()
            .filter_map(|id| leases.remove(&id))
            .map(|l| {
                l.owner.cancelled.cancel();
                l.owner.id
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn principal(key: &str, client: &str) -> Principal {
        Principal {
            key_id: key.into(),
            client_id: client.into(),
            cancelled: CancellationToken::new(),
            ended: CancellationToken::new(),
        }
    }
    #[test]
    fn disabling_network_servers_preserves_local_ipc_input() {
        let active = ActiveCoordinator::default();
        active.begin().unwrap();
        active.finish(Some(Uid::from_bytes(&[1; 12]).unwrap()));
        let sessions = InputSessions::default();
        let ipc = principal("ipc:local", "ipc");
        let local = sessions.open(&active, &ipc).unwrap();
        sessions.open(&active, &principal("A", "rest")).unwrap();
        sessions.open(&active, &principal("A", "mcp:1")).unwrap();
        assert_eq!(sessions.cancel_network_clients().len(), 2);
        assert!(sessions
            .resolve(&active, &ipc, Some(&local.input_session))
            .is_ok());
        assert_eq!(sessions.cancel_client("ipc:local", "ipc").len(), 1);
    }
    #[test]
    fn leases_bind_key_client_generation_and_revocation() {
        let active = Arc::new(ActiveCoordinator::default());
        active.begin().unwrap();
        active.finish(Some(Uid::from_bytes(&[1; 12]).unwrap()));
        let sessions = InputSessions::default();
        let a = principal("A", "mcp:1");
        let other = principal("A", "mcp:2");
        let b = principal("B", "rest");
        let info = sessions.open(&active, &a).unwrap();
        assert_eq!(
            sessions
                .resolve(&active, &other, Some(&info.input_session))
                .err()
                .unwrap()
                .code,
            "FORBIDDEN"
        );
        assert!(sessions.close(&b, &info.input_session).is_err());
        let (owner, _) = sessions
            .resolve(&active, &a, Some(&info.input_session))
            .unwrap();
        a.cancelled.cancel();
        assert!(owner.cancelled.is_cancelled());
        assert_eq!(sessions.reap(&active), vec![owner.id]);
        let info = sessions.open(&active, &b).unwrap();
        active.begin().unwrap();
        active.finish(Some(Uid::from_bytes(&[2; 12]).unwrap()));
        assert_eq!(
            sessions
                .resolve(&active, &b, Some(&info.input_session))
                .err()
                .unwrap()
                .code,
            "STALE_OPERATION"
        );
    }
    #[test]
    fn expiry_and_client_close_cancel_only_owned_leases() {
        let active = ActiveCoordinator::default();
        active.begin().unwrap();
        active.finish(Some(Uid::from_bytes(&[1; 12]).unwrap()));
        let sessions = InputSessions::default();
        let p = principal("A", "rest");
        let other = principal("B", "rest");
        let one = sessions.open(&active, &p).unwrap();
        let two = sessions.open(&active, &other).unwrap();
        let (owner, _) = sessions
            .resolve(&active, &p, Some(&one.input_session))
            .unwrap();
        sessions
            .leases
            .lock()
            .get_mut(&one.input_session)
            .unwrap()
            .deadline = Instant::now() - TTL;
        assert_eq!(
            sessions
                .resolve(&active, &p, Some(&one.input_session))
                .err()
                .unwrap()
                .code,
            "INPUT_SESSION_EXPIRED"
        );
        assert!(owner.cancelled.is_cancelled());
        sessions.reap(&active);
        assert!(sessions
            .heartbeat(&active, &other, &two.input_session)
            .is_ok());
        assert_eq!(sessions.cancel_client("A", "rest").len(), 0);
        assert_eq!(sessions.cancel_client("B", "rest").len(), 1);
    }
}
