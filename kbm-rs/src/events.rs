//! Cached metadata only. One shared latest value, never a packet/file/input log.
use crate::{active::Binding, input_sessions::Principal, model::AppError, service::AppService};
use parking_lot::Mutex;
use schemars::JsonSchema;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::Write,
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::{
    sync::{watch, Notify},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

pub const MAX_CLIENTS: usize = 32;
pub const MAX_PER_KEY: usize = 8;
pub const MAX_PAYLOAD: usize = 1024 * 1024;
pub const SNAPSHOT_PATH: &str = "/api/v1/events/snapshot";

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Status {
    pub subscriptions: usize,
    pub limit: usize,
    pub per_key_limit: usize,
    pub collecting: bool,
}
#[derive(Serialize, JsonSchema)]
pub struct JobProgress {
    pub job_id: String,
    pub kind: &'static str,
    pub state: crate::storage::JobState,
    pub phase: String,
    pub done: u64,
    pub total: u64,
    pub can_cancel: bool,
    pub side_effects_possible: bool,
    pub error: Option<AppError>,
    pub cleanup_error: Option<AppError>,
    pub transfer: Option<crate::storage::transfer::Progress>,
}
impl From<crate::storage::JobInfo> for JobProgress {
    fn from(j: crate::storage::JobInfo) -> Self {
        Self {
            job_id: j.job_id,
            kind: j.kind,
            state: j.state,
            phase: j.phase,
            done: j.done,
            total: j.total,
            can_cancel: j.can_cancel,
            side_effects_possible: j.side_effects_possible,
            error: j.error,
            cleanup_error: j.cleanup_error,
            transfer: j.transfer,
        }
    }
}
#[derive(Serialize, JsonSchema)]
pub struct ActiveMetadata {
    pub binding: Binding,
    pub input: crate::input::InputStatus,
    pub tty: Option<crate::tty::Status>,
    pub storage: Option<crate::storage_wire::Snapshot>,
    pub job: Option<JobProgress>,
}
#[derive(Serialize, JsonSchema)]
pub struct Report {
    pub state: crate::service::Snapshot,
    pub servers: crate::remote::RemoteStatus,
    pub active_metadata: Option<ActiveMetadata>,
    pub streams: Status,
}
pub fn snapshot(app: &AppService) -> Report {
    let mut state = app.snapshot();
    let mut active_metadata = state
        .active
        .uid
        .as_ref()
        .filter(|_| !state.active.switching)
        .map(|uid| {
            let binding = Binding {
                uid: uid.clone(),
                generation: state.active.generation,
            };
            ActiveMetadata {
                input: app.input.snapshot(&binding),
                tty: app.tty.cached(&binding),
                storage: app.storage.cached(&binding),
                job: app.storage.current(&binding).map(Into::into),
                binding,
            }
        });
    // A switch during collection must never attach the new channel's metadata
    // to an old state. A subsequent tick sends the coherent new generation.
    if active_metadata
        .as_ref()
        .is_some_and(|m| app.active.validate(&m.binding).is_err())
    {
        state = app.snapshot();
        active_metadata = None;
    }
    Report {
        state,
        servers: app.remote.read().clone(),
        active_metadata,
        streams: app.events.status(),
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Update {
    State {
        snapshot: Box<Report>,
    },
    Resync {
        reason: String,
        snapshot_url: String,
    },
}
#[derive(Serialize, JsonSchema)]
pub struct Envelope {
    pub epoch: String,
    pub sequence: u64,
    pub observed_at: u64,
    #[serde(flatten)]
    pub update: Update,
}
pub struct Frame {
    pub id: String,
    pub event: &'static str,
    pub json: String,
}
struct Client {
    key: String,
    cancelled: CancellationToken,
    ended: CancellationToken,
}
#[derive(Default)]
struct State {
    clients: BTreeMap<u64, Client>,
    next_client: u64,
    sequence: u64,
    previous: Vec<u8>,
}
struct Inner {
    state: Mutex<State>,
    sample: Mutex<()>,
    value: watch::Sender<Option<Arc<Frame>>>,
    wake: Notify,
    stop: CancellationToken,
    epoch: String,
}
pub struct Hub {
    inner: Arc<Inner>,
    task: Mutex<Option<JoinHandle<()>>>,
}
impl Default for Hub {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Default::default(),
                sample: Mutex::new(()),
                value: watch::channel(None).0,
                wake: Notify::new(),
                stop: CancellationToken::new(),
                epoch: crate::remote_error::Context::default().request_id,
            }),
            task: Mutex::new(None),
        }
    }
}
impl Hub {
    pub fn status(&self) -> Status {
        let n = self.inner.state.lock().clients.len();
        Status {
            subscriptions: n,
            limit: MAX_CLIENTS,
            per_key_limit: MAX_PER_KEY,
            collecting: n > 0 && !self.inner.stop.is_cancelled(),
        }
    }
    pub fn subscribe(
        &self,
        app: &Arc<AppService>,
        p: &Principal,
    ) -> Result<Subscription, AppError> {
        if self.inner.stop.is_cancelled() || !app.is_running() {
            return Err(AppError::new("APP_STOPPING", "앱이 종료 중입니다."));
        }
        if p.cancelled.is_cancelled() || p.ended.is_cancelled() {
            return Err(AppError::new(
                "UNAUTHORIZED",
                "인증 또는 연결이 종료됐습니다.",
            ));
        }
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| AppError::new("SERVER_OFF", "이벤트 실행기가 없습니다."))?;
        let id = {
            let mut s = self.inner.state.lock();
            s.clients
                .retain(|_, c| !c.cancelled.is_cancelled() && !c.ended.is_cancelled());
            if s.clients.len() >= MAX_CLIENTS
                || s.clients.values().filter(|c| c.key == p.key_id).count() >= MAX_PER_KEY
            {
                return Err(AppError::new(
                    "EVENT_SUBSCRIPTION_LIMIT",
                    "이벤트 구독 한도를 초과했습니다.",
                ));
            }
            s.next_client = s.next_client.checked_add(1).ok_or_else(|| {
                AppError::new("EVENT_SUBSCRIPTION_LIMIT", "이벤트 구독 ID가 소진됐습니다.")
            })?;
            let id = s.next_client;
            s.clients.insert(
                id,
                Client {
                    key: p.key_id.clone(),
                    cancelled: p.cancelled.clone(),
                    ended: p.ended.clone(),
                },
            );
            id
        };
        let sub = Subscription {
            inner: self.inner.clone(),
            id,
            rx: self.inner.value.subscribe(),
            first: true,
            cancelled: p.cancelled.clone(),
            ended: p.ended.clone(),
        };
        // Serialize initial sampling with the collector so a stale captured value
        // cannot overwrite a newer one when a second client connects.
        self.inner.publish(app);
        let mut task = self.task.lock();
        if task.is_none() {
            let inner = self.inner.clone();
            let app = Arc::downgrade(app);
            *task = Some(runtime.spawn(async move { collect(inner, app).await }));
        }
        self.inner.wake.notify_one();
        Ok(sub)
    }
    pub fn stop(&self) {
        self.inner.stop.cancel();
    }
    pub async fn shutdown(&self) {
        self.stop();
        let task = self.task.lock().take();
        if let Some(mut task) = task {
            if tokio::time::timeout(Duration::from_secs(1), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }
}
impl Drop for Hub {
    fn drop(&mut self) {
        self.stop();
        if let Some(task) = self.task.get_mut().take() {
            task.abort();
        }
    }
}
impl Inner {
    fn publish(&self, app: &AppService) {
        let _sample = self.sample.lock();
        let mut encoded = Capped(Vec::new());
        let report = snapshot(app);
        let update = if serde_json::to_writer(&mut encoded, &report).is_ok() {
            Update::State {
                snapshot: Box::new(report),
            }
        } else {
            encoded.0 = b"payload_limit".to_vec();
            Update::Resync {
                reason: "payload_limit".into(),
                snapshot_url: SNAPSHOT_PATH.into(),
            }
        };
        let mut s = self.state.lock();
        if self.stop.is_cancelled() || s.clients.is_empty() {
            return;
        }
        if s.previous == encoded.0 {
            return;
        }
        let Some(seq) = s.sequence.checked_add(1) else {
            self.stop.cancel();
            return;
        };
        s.sequence = seq;
        s.previous = encoded.0;
        let event = match update {
            Update::State { .. } => "state",
            Update::Resync { .. } => "resync",
        };
        let frame = Frame {
            id: format!("{}:{seq}", self.epoch),
            event,
            json: serde_json::to_string(&Envelope {
                epoch: self.epoch.clone(),
                sequence: seq,
                observed_at: crate::model::now_ms(),
                update,
            })
            .expect("metadata is JSON serializable"),
        };
        self.value.send_replace(Some(Arc::new(frame)));
    }
}
// Stops allocation at the limit, including when an unusually large SD catalog
// or inventory already exists in the authoritative application cache.
struct Capped(Vec<u8>);
impl Write for Capped {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(buf.len()) > MAX_PAYLOAD {
            return Err(std::io::Error::other("event payload limit"));
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
async fn collect(inner: Arc<Inner>, app: Weak<AppService>) {
    loop {
        let collecting = {
            let mut s = inner.state.lock();
            s.clients
                .retain(|_, c| !c.cancelled.is_cancelled() && !c.ended.is_cancelled());
            if s.clients.is_empty() {
                s.previous.clear();
                inner.value.send_replace(None);
            }
            !s.clients.is_empty()
        };
        tokio::select! {
            biased;
            _=inner.stop.cancelled()=>break,
            _=inner.wake.notified()=>{},
            _=tokio::time::sleep(Duration::from_millis(250)), if collecting=>{},
        }
        let Some(app) = app.upgrade() else {
            break;
        };
        if collecting {
            inner.publish(&app);
        }
    }
    inner.state.lock().clients.clear();
    inner.value.send_replace(None);
}
pub struct Subscription {
    inner: Arc<Inner>,
    id: u64,
    rx: watch::Receiver<Option<Arc<Frame>>>,
    first: bool,
    cancelled: CancellationToken,
    ended: CancellationToken,
}
impl Subscription {
    pub async fn next(&mut self) -> Option<Arc<Frame>> {
        loop {
            if self.inner.stop.is_cancelled()
                || self.cancelled.is_cancelled()
                || self.ended.is_cancelled()
            {
                return None;
            }
            if self.first {
                self.first = false;
                let frame = self.rx.borrow_and_update().clone();
                if frame.is_some() {
                    return frame;
                }
            }
            tokio::select! {
                biased;
                _=self.inner.stop.cancelled()=>return None,
                _=self.cancelled.cancelled()=>return None,
                _=self.ended.cancelled()=>return None,
                r=self.rx.changed()=>{ if r.is_err() { return None; } self.first=true; }
            }
        }
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.inner.state.lock().clients.remove(&self.id);
        self.inner.wake.notify_one();
    }
}

#[cfg(test)]
mod tests;
