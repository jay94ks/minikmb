use crate::{
    active::{ActiveCoordinator, ActiveState},
    auth::KeyStore,
    model::{AppError, Inventory, Uid},
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Snapshot {
    pub preferences: crate::preferences::Status,
    pub version: &'static str,
    pub inventory: Inventory,
    pub active: ActiveState,
    pub video: crate::video::VideoStatus,
    pub storage_resources: Option<crate::storage::media::ResourceInfo>,
    pub storage_cleanup_error: Option<AppError>,
    pub storage_card_format: Option<crate::storage::card_format::Info>,
    pub network_resource: Option<crate::network::manager::Status>,
    pub firmware_job: Option<crate::firmware::manager::Info>,
    pub keyboard_capture: Option<crate::keyboard_capture::Status>,
}
pub struct AppService {
    pub log_opener: Arc<dyn crate::logging::DirectoryOpener>,
    pub(crate) log_controls: Arc<tokio::sync::Semaphore>,
    pub logs: Arc<crate::logging::Journal>,
    pub events: crate::events::Hub,
    pub wake: crate::wake::Service,
    pub preferences: Arc<crate::preferences::Store>,
    pub active: Arc<ActiveCoordinator>,
    pub keys: KeyStore,
    pub inventory: RwLock<Inventory>,
    pub remote: RwLock<crate::remote::RemoteStatus>,
    pub devices: Arc<crate::actor::DeviceHub>,
    pub input: Arc<crate::input::InputManager>,
    pub input_sessions: crate::input_sessions::InputSessions,
    pub tty: Arc<crate::tty::TtyManager>,
    pub storage: Arc<crate::storage::Manager>,
    pub network: Arc<crate::network::manager::Manager>,
    pub firmware: Arc<crate::firmware::manager::Manager>,
    pub keyboard_capture: RwLock<Option<crate::keyboard_capture::Status>>,
    video: crate::video::VideoHub,
    snapshot_slot: Arc<tokio::sync::Semaphore>,
    gate: tokio::sync::Mutex<()>,
    alive: Arc<AtomicBool>,
    rescan: AtomicBool,
}
#[derive(Deserialize, rmcp::schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectChannel {
    pub uid: String,
}
impl AppService {
    pub fn new(data: PathBuf) -> Result<Arc<Self>, AppError> {
        Self::construct(data, None)
    }
    #[cfg(test)]
    pub(crate) fn with_test_input(
        data: PathBuf,
        backend: Arc<dyn crate::input::Backend>,
    ) -> Result<Arc<Self>, AppError> {
        Self::construct(data, Some(backend))
    }
    fn construct(
        data: PathBuf,
        backend: Option<Arc<dyn crate::input::Backend>>,
    ) -> Result<Arc<Self>, AppError> {
        let active = Arc::new(ActiveCoordinator::default());
        let logs = crate::logging::Journal::new(data.join("logs"));
        let devices = Arc::new(crate::actor::DeviceHub::with_logger(
            active.clone(),
            logs.clone(),
        ));
        let input = crate::input::InputManager::new(
            active.clone(),
            backend.unwrap_or_else(|| {
                Arc::new(crate::input::NativeBackend {
                    devices: devices.clone(),
                    logs: logs.clone(),
                })
            }),
        );
        let tty = crate::tty::TtyManager::new(
            active.clone(),
            Arc::new(crate::tty::NativeBackend {
                devices: devices.clone(),
            }),
        );
        let storage = crate::storage::Manager::new(
            active.clone(),
            Arc::new(crate::storage_wire::NativeBackend {
                devices: devices.clone(),
                input: input.clone(),
            }),
        );
        let preferences = crate::preferences::Store::load(data.join("preferences.json"));
        let network = crate::network::manager::Manager::with_preferences(
            active.clone(),
            Arc::new(crate::network::wire::NativeBackend {
                devices: devices.clone(),
                input: input.clone(),
            }),
            preferences.clone(),
        );
        input.set_logger(&logs);
        storage.set_logger(&logs);
        Ok(Arc::new(Self {
            log_opener: Arc::new(crate::logging::NativeOpener),
            log_controls: Arc::new(tokio::sync::Semaphore::new(2)),
            logs,
            events: Default::default(),
            wake: Default::default(),
            preferences,
            active,
            devices,
            input,
            tty,
            storage,
            network,
            firmware: crate::firmware::manager::Manager::new(Arc::new(
                crate::firmware::native::Native::default(),
            )),
            keyboard_capture: RwLock::new(None),
            input_sessions: Default::default(),
            keys: KeyStore::load(data.join("api-keys.json"))?,
            inventory: RwLock::new(Inventory::default()),
            remote: RwLock::new(crate::remote::RemoteStatus::default()),
            video: crate::video::VideoHub::default(),
            snapshot_slot: Arc::new(tokio::sync::Semaphore::new(1)),
            gate: tokio::sync::Mutex::new(()),
            alive: Arc::new(AtomicBool::new(true)),
            rescan: AtomicBool::new(true),
        }))
    }
    pub fn snapshot(&self) -> Snapshot {
        let firmware_job = self.firmware.current();
        let inventory = self.inventory.read();
        let mut ordered_inventory = inventory.clone();
        self.preferences
            .sort_channels(&mut ordered_inventory.channels);
        self.active.inspect(|active| {
            let video = if active.switching {
                Default::default()
            } else {
                active
                    .uid
                    .as_ref()
                    .map(|uid| {
                        self.video_status(&crate::active::Binding {
                            uid: uid.clone(),
                            generation: active.generation,
                        })
                    })
                    .unwrap_or_default()
            };
            Snapshot {
                preferences: self.preferences.status(),
                version: env!("CARGO_PKG_VERSION"),
                inventory: ordered_inventory,
                active: active.clone(),
                video,
                storage_resources: self.storage.resources_info(),
                storage_cleanup_error: self.storage.media_error(),
                storage_card_format: self.storage.card_format_info(),
                network_resource: self.network.resource(),
                firmware_job,
                keyboard_capture: self.keyboard_capture.read().clone(),
            }
        })
    }
    pub fn request_rescan(&self) {
        self.rescan.store(true, Ordering::Release);
    }
    pub fn video_status(&self, binding: &crate::active::Binding) -> crate::video::VideoStatus {
        let mut status = self.video.status(binding);
        if status.requested.is_none() {
            status.requested = self.preferences.channel(&binding.uid).video;
        }
        status
    }
    pub fn start_preference_monitor(self: &Arc<Self>) -> std::thread::JoinHandle<()> {
        let weak = Arc::downgrade(self);
        let alive = self.alive.clone();
        std::thread::spawn(move || {
            let mut seen = String::new();
            let mut capture_log = crate::logging::CaptureTracker::default();
            while alive.load(Ordering::Acquire) {
                let Some(app) = weak.upgrade() else {
                    break;
                };
                if let Ok(binding) = app.active.bind() {
                    let status = app.video.status(&binding);
                    capture_log.observe(&app.logs, &binding, &status, crate::model::now_ms());
                    if status.running && status.error.is_empty() && status.notice.is_empty() {
                        if let (Some(frame), Some(mode)) = (status.frame, status.requested) {
                            if frame.stream_id != seen && app.active.validate(&binding).is_ok() {
                                seen = frame.stream_id;
                                let _ = app.preferences.video(&binding.uid, mode);
                            }
                        }
                    }
                }
                drop(app);
                std::thread::sleep(Duration::from_millis(250));
            }
        })
    }
    pub fn start_discovery(self: &Arc<Self>) -> std::thread::JoinHandle<()> {
        let weak = Arc::downgrade(self);
        let alive = self.alive.clone();
        std::thread::spawn(move || {
            let mut ticks = 0;
            while alive.load(Ordering::Acquire) {
                let Some(app) = weak.upgrade() else {
                    break;
                };
                let full = app.rescan.swap(false, Ordering::AcqRel) || ticks % 50 == 0;
                if full {
                    app.devices.scan();
                }
                let next = app.devices.inventory();
                let mut inventory = app.inventory.write();
                let old = inventory
                    .channels
                    .iter()
                    .filter(|c| c.connected)
                    .map(|c| c.uid.clone())
                    .collect::<std::collections::HashSet<_>>();
                let new = next
                    .channels
                    .iter()
                    .filter(|c| c.connected)
                    .map(|c| c.uid.clone())
                    .collect::<std::collections::HashSet<_>>();
                for uid in new.difference(&old) {
                    app.logs.emit(
                        crate::logging::Level::Info,
                        crate::logging::Event::DeviceConnected,
                        crate::logging::Fields {
                            uid: Some(uid.clone()),
                            ..Default::default()
                        },
                    );
                }
                for uid in old.difference(&new) {
                    app.logs.emit(
                        crate::logging::Level::Warn,
                        crate::logging::Event::DeviceDisconnected,
                        crate::logging::Fields {
                            uid: Some(uid.clone()),
                            ..Default::default()
                        },
                    );
                }
                *inventory = next;
                let active = app.active.state();
                if let Some(uid) = active.uid {
                    if !inventory
                        .channels
                        .iter()
                        .any(|c| c.uid == uid && c.connected)
                    {
                        app.active.disconnected(&uid);
                        app.input.disconnected(&uid);
                        app.video.cancel();
                    }
                }
                drop(inventory);
                drop(app);
                ticks += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    }
    pub async fn select(self: &Arc<Self>, uid: Option<Uid>) -> Result<ActiveState, AppError> {
        // Accepted transitions finish even when an HTTP client disconnects.
        let app = self.clone();
        tokio::spawn(async move {
            let previous = app.active.state();
            let command = if uid.is_some() {
                "channel_activate"
            } else {
                "channel_deactivate"
            };
            let result = app.select_inner(uid).await;
            if let Ok(active) = &result {
                if active != &previous {
                    app.logs.emit(
                        crate::logging::Level::Info,
                        crate::logging::Event::ChannelChanged,
                        crate::logging::Fields {
                            uid: active.uid.clone(),
                            generation: Some(active.generation),
                            ..Default::default()
                        },
                    );
                }
            }
            app.audit(command, None, result)
        })
        .await
        .map_err(|_| AppError::new("TRANSITION_FAILED", "채널 전환 작업이 실패했습니다."))?
    }
    async fn select_inner(&self, uid: Option<Uid>) -> Result<ActiveState, AppError> {
        self.transition(uid, None, false, false).await
    }
    /// Naming never silently switches away from another live channel. The card
    /// dialog offers an explicit selection action if that is what the user wants.
    pub async fn bind_for_rename(&self, uid: Uid) -> Result<crate::active::Binding, AppError> {
        let state = self
            .transition(Some(uid.clone()), None, true, false)
            .await?;
        Ok(crate::active::Binding {
            uid,
            generation: state.generation,
        })
    }
    pub(crate) async fn prepare_firmware(
        &self,
        binding: Option<&crate::active::Binding>,
    ) -> Result<(), AppError> {
        self.transition(None, binding, false, true)
            .await
            .map(|_| ())
    }
    async fn transition(
        &self,
        uid: Option<Uid>,
        firmware: Option<&crate::active::Binding>,
        naming: bool,
        programming: bool,
    ) -> Result<ActiveState, AppError> {
        let _gate = self.gate.lock().await;
        if naming
            && self
                .active
                .state()
                .uid
                .as_ref()
                .is_some_and(|active| Some(active) != uid.as_ref())
        {
            return Err(AppError::new(
                "CHANNEL_SELECTION_REQUIRED",
                "다른 채널이 사용 중입니다. 먼저 이 채널을 명시적으로 선택하세요.",
            ));
        }
        if let Some(binding) = firmware {
            self.active.validate(binding)?;
        } else if !programming && self.firmware.busy() {
            return Err(AppError::new(
                "FIRMWARE_BUSY",
                "펌웨어 작업이 끝난 후 채널을 변경하세요.",
            ));
        }
        let (previous, _storage_transition) = {
            let inventory = self.inventory.read();
            if let Some(next) = &uid {
                if !inventory
                    .channels
                    .iter()
                    .any(|c| &c.uid == next && c.connected)
                {
                    return Err(AppError::new(
                        "CHANNEL_NOT_FOUND",
                        "연결된 채널을 찾을 수 없습니다.",
                    ));
                }
            }
            if self.active.state().uid == uid {
                return Ok(self.active.state());
            }
            let transition = self.storage.reserve_switch(uid.as_ref())?;
            self.network.quiesce();
            (self.active.begin()?, transition)
        };
        let original = previous.uid.as_ref().map(|uid| crate::active::Binding {
            uid: uid.clone(),
            generation: previous.generation,
        });
        if let Err(error) = self
            .storage
            .prepare_switch(&_storage_transition, original.as_ref(), uid.as_ref())
            .await
        {
            self.tty.close_all();
            if let Some(old) = &previous.uid {
                let _ = self.input.release_channel(old).await;
            }
            let _ = self.video.stop().await;
            let inventory = self.inventory.read();
            self.active.finish(previous.uid.filter(|uid| {
                inventory
                    .channels
                    .iter()
                    .any(|c| &c.uid == uid && c.connected)
            }));
            return Err(error);
        }
        self.tty.close_all();
        if let Some(old) = &previous.uid {
            if let Err(e) = self.input.release_channel(old).await {
                // User policy: best-effort original-MCU cleanup must not trap
                // channel selection on a manual release-confirmation step.
                // Keep the actual failure in the journal; never report it as
                // a confirmed USB completion or retarget its cleanup.
                self.logs.command(
                    "hid_release_during_switch",
                    original.as_ref(),
                    &Err::<(), _>(e),
                );
            }
        }
        if let Err(error) = self.network.prepare_switch(uid.as_ref()).await {
            let _ = self.video.stop().await;
            let inventory = self.inventory.read();
            self.active.finish(previous.uid.filter(|old| {
                inventory
                    .channels
                    .iter()
                    .any(|c| &c.uid == old && c.connected)
            }));
            return Err(error);
        }
        if let Err(e) = self.video.stop().await {
            self.active.finish(None);
            return Err(e);
        }
        let inventory = self.inventory.read();
        if uid.as_ref().is_some_and(|u| {
            !inventory
                .channels
                .iter()
                .any(|c| &c.uid == u && c.connected)
        }) {
            self.active.finish(None);
            return Err(AppError::new(
                "CHANNEL_NOT_FOUND",
                "전환 중 채널 연결이 끊겼습니다.",
            ));
        }
        // NCM/SD adapters join this same transaction as they are ported.
        self.active.finish(uid);
        Ok(self.active.state())
    }
    pub async fn open_video(
        self: &Arc<Self>,
        mode: crate::media::CaptureMode,
    ) -> Result<crate::video::VideoStatus, AppError> {
        crate::video::validate_mode(mode)?;
        let binding = self.active.bind()?;
        self.open_pinned_video(binding, mode).await
    }
    /// Internal UI queue binding, never a public REST/MCP routing argument.
    pub async fn open_pinned_video(
        self: &Arc<Self>,
        binding: crate::active::Binding,
        mode: crate::media::CaptureMode,
    ) -> Result<crate::video::VideoStatus, AppError> {
        crate::video::validate_mode(mode).map_err(|e| e.at(&binding))?;
        let original = binding.clone();
        let app = self.clone();
        tokio::spawn(async move {
            let _gate = app.gate.lock().await;
            app.active.validate(&binding)?;
            let root = {
                let inventory = app.inventory.read();
                inventory
                    .channels
                    .iter()
                    .find(|c| c.uid == binding.uid && c.connected)
                    .and_then(|c| c.capture_id.clone())
                    .ok_or_else(|| {
                        AppError::new(
                            "CAPTURE_NOT_FOUND",
                            "활성 채널에 대응하는 캡처 장치가 없습니다.",
                        )
                    })?
            };
            let previous = app.video.status(&binding);
            let fallback =
                if previous.running && previous.frame.is_some() && previous.error.is_empty() {
                    previous.effective_mode
                } else {
                    None
                };
            app.video.stop().await?;
            let inventory = app.inventory.read();
            app.active.validate(&binding)?;
            if !inventory.channels.iter().any(|c| {
                c.uid == binding.uid && c.connected && c.capture_id.as_deref() == Some(&root)
            }) {
                return Err(AppError::new(
                    "CAPTURE_NOT_FOUND",
                    "캡처 장치 연결이 변경됐습니다.",
                ));
            }
            app.video.start(binding.clone(), root, mode, fallback)?;
            Ok(app.video.status(&binding))
        })
        .await
        .map_err(|_| {
            AppError::new("CAPTURE_FAILED", "영상 시작 작업이 실패했습니다.").at(&original)
        })?
        .map_err(|e| e.at(&original))
    }
    pub async fn close_video(self: &Arc<Self>) -> Result<(), AppError> {
        let binding = self.active.bind()?;
        self.close_pinned_video(binding).await
    }
    pub async fn close_pinned_video(
        self: &Arc<Self>,
        binding: crate::active::Binding,
    ) -> Result<(), AppError> {
        let app = self.clone();
        tokio::spawn(async move {
            let _gate = app.gate.lock().await;
            app.active.validate(&binding)?;
            app.video.stop().await
        })
        .await
        .map_err(|_| AppError::new("CAPTURE_STOP_FAILED", "영상 종료 작업이 실패했습니다."))?
    }
    pub fn current_frame(&self) -> Result<crate::video::CurrentFrame, AppError> {
        let binding = self.active.bind()?;
        let frame = self.video.frame(&binding)?;
        self.active.validate(&binding)?;
        Ok(frame)
    }
    #[cfg(test)]
    pub(crate) fn test_video_geometry(&self, width: u32, height: u32) {
        self.video
            .test_geometry(self.active.bind().unwrap(), width, height);
    }
    pub fn validate_frame_reference(
        &self,
        binding: &crate::active::Binding,
        id: &str,
    ) -> Result<(), AppError> {
        self.video.validate_reference(binding, id)
    }
    pub async fn close_remote_inputs(&self) {
        for id in self.input_sessions.cancel_all() {
            let _ = self.input.release_owner(id).await;
        }
    }
    pub async fn close_client_inputs(&self, key: &str, client: &str) {
        for id in self.input_sessions.cancel_client(key, client) {
            let _ = self.input.release_owner(id).await;
        }
    }
    pub async fn snapshot_image(
        &self,
        format: crate::video::SnapshotFormat,
    ) -> Result<crate::video::SnapshotImage, AppError> {
        let binding = self.active.bind()?;
        let result = async {
            let slot = self
                .snapshot_slot
                .clone()
                .try_acquire_owned()
                .map_err(|_| AppError::new("CAPTURE_BUSY", "다른 장면 캡처가 진행 중입니다."))?;
            let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
            let frame = loop {
                self.active.validate(&binding)?;
                match self.video.frame(&binding) {
                    Ok(frame) => break frame,
                    Err(e)
                        if ["FRAME_NOT_READY", "STALE_FRAME"].contains(&e.code)
                            && tokio::time::Instant::now() < deadline =>
                    {
                        tokio::time::sleep(Duration::from_millis(20)).await
                    }
                    Err(e) => return Err(e),
                }
            };
            let image = tokio::task::spawn_blocking(move || {
                let _slot = slot;
                crate::video::encode_snapshot(frame, format)
            })
            .await
            .map_err(|_| AppError::new("ENCODE_FAILED", "장면 캡처 작업이 실패했습니다."))??;
            self.active.validate(&binding)?;
            let current = self.video.frame(&binding)?;
            if current.info.stream_id != image.info.stream_id {
                return Err(AppError::new("STALE_FRAME", "영상 스트림이 변경됐습니다."));
            }
            Ok(image)
        }
        .await;
        result.map_err(|e: AppError| e.at(&binding))
    }
    pub async fn shutdown(&self) {
        self.stop();
        self.firmware.shutdown().await;
        self.events.shutdown().await;
        let _gate = self.gate.lock().await;
        self.storage.shutdown().await;
        self.network.shutdown().await;
        let previous = self.active.begin().ok().and_then(|s| s.uid);
        if let Some(uid) = previous {
            let _ = self.input.release_channel(&uid).await;
        }
        let _ = self.video.stop().await;
        self.tty.shutdown().await;
        self.devices.stop();
        let devices = self.devices.clone();
        let _ = tokio::task::spawn_blocking(move || devices.join()).await;
        self.active.finish(None);
        self.logs.emit(
            crate::logging::Level::Info,
            crate::logging::Event::AppStopped,
            Default::default(),
        );
        let logs = self.logs.clone();
        let _ = tokio::task::spawn_blocking(move || logs.shutdown()).await;
    }
    pub(crate) fn audit<T>(
        &self,
        name: &str,
        binding: Option<&crate::active::Binding>,
        result: Result<T, AppError>,
    ) -> Result<T, AppError> {
        let result = result.map_err(|error| match binding {
            Some(b) => error.at(b),
            None => error,
        });
        if let Some(doc) = COMMANDS.iter().find(|doc| doc.name == name) {
            self.logs.command(doc.name, binding, &result);
        }
        result
    }
    pub fn stop(&self) {
        self.firmware.stop();
        self.alive.store(false, Ordering::Release);
        self.events.stop();
        self.video.cancel();
        self.input.stop();
        self.storage.stop();
        self.network.stop();
        self.tty.close_all();
    }
    pub(crate) fn is_running(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }
    pub fn start_input_monitor(
        self: &Arc<Self>,
        runtime: &tokio::runtime::Handle,
    ) -> tokio::task::JoinHandle<()> {
        let weak = Arc::downgrade(self);
        let alive = self.alive.clone();
        runtime.spawn(async move {
            while alive.load(Ordering::Acquire) {
                if let Some(app) = weak.upgrade() {
                    for id in app.input_sessions.reap(&app.active) {
                        let _ = app.input.release_owner(id).await;
                    }
                    app.input.expire_cancelled().await;
                    app.storage.reap_uploads();
                } else {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
    }
    pub async fn execute(
        self: &Arc<Self>,
        name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, AppError> {
        let bad = || {
            AppError::new(
                "INVALID_ARGUMENTS",
                "요청 인자를 확인하세요. 일반 제어에는 UID를 지정하지 않습니다.",
            )
        };
        if !["channel_activate", "video_open", "video_snapshot"].contains(&name)
            && args.as_object().is_none_or(|o| !o.is_empty())
        {
            return Err(bad());
        }
        match name {
            "events_get_snapshot" => Ok(serde_json::json!(crate::events::snapshot(self))),
            "app_get_state" => Ok(
                serde_json::json!({"state":self.snapshot(),"servers":self.remote.read().clone()}),
            ),
            "channels_list" => {
                let mut inventory = self.inventory.read().clone();
                self.preferences.sort_channels(&mut inventory.channels);
                Ok(serde_json::json!(inventory))
            }
            "channel_get_active" => {
                let snapshot = self.snapshot();
                let uid = snapshot
                    .active
                    .uid
                    .as_ref()
                    .filter(|_| !snapshot.active.switching)
                    .ok_or_else(|| AppError::new("NO_ACTIVE_CHANNEL", "활성 채널이 없습니다."))?;
                let c = snapshot
                    .inventory
                    .channels
                    .iter()
                    .find(|c| &c.uid == uid)
                    .ok_or_else(|| AppError::new("NO_ACTIVE_CHANNEL", "활성 채널이 없습니다."))?;
                Ok(serde_json::json!({"channel":c,"generation":snapshot.active.generation}))
            }
            "channel_activate" => {
                let select: SelectChannel = serde_json::from_value(args).map_err(|_| bad())?;
                let uid = Uid::try_from(select.uid).map_err(|_| bad())?;
                Ok(serde_json::json!(self.select(Some(uid)).await?))
            }
            "channel_deactivate" => Ok(serde_json::json!(self.select(None).await?)),
            "video_open" => {
                let mode = serde_json::from_value(args).map_err(|_| bad())?;
                Ok(serde_json::json!(self.open_video(mode).await?))
            }
            "video_close" => {
                self.close_video().await?;
                Ok(serde_json::json!({"running":false}))
            }
            "video_get_status" => {
                let b = self.active.bind()?;
                let status = self.video_status(&b);
                self.active.validate(&b)?;
                Ok(serde_json::json!(status))
            }
            _ => Err(AppError::new(
                "NOT_IMPLEMENTED",
                "이 명령은 아직 지원하지 않습니다.",
            )),
        }
    }
}
impl Drop for AppService {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Serialize, schemars::JsonSchema)]
pub struct CommandDoc {
    pub name: &'static str,
    pub method: &'static str,
    pub path: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub active_required: bool,
}
pub const COMMANDS: &[CommandDoc] = &[
    CommandDoc{name:"firmware_images",method:"POST",path:"/api/v1/firmware/images",title:"동봉 펌웨어",description:"바로 사용할 동봉 STM32/ROM의 크기와 SHA256을 조회합니다.",active_required:false},
    CommandDoc{name:"firmware_programmers",method:"POST",path:"/api/v1/firmware/programmers",title:"프로그래머 검색",description:"stm32의 ST-Link 또는 ms2130의 RP2040 Writer를 검색합니다. 기록하지 않습니다.",active_required:false},
    CommandDoc{name:"firmware_inspect",method:"POST",path:"/api/v1/firmware/inspect",title:"기록 대상 확인",description:"선택한 ST-Link/Writer의 플래시 용량을 읽습니다. 채널 선택이나 MCU UID 대조는 필요하지 않습니다. 기록하지 않습니다.",active_required:false},
    CommandDoc{name:"firmware_upload_prepare",method:"POST",path:"/api/v1/firmware/upload-prepare",title:"펌웨어 업로드 준비",description:"같은 API 키로 원본 파일을 POST할 단회 REST 주소를 발급합니다. 수신만으로 기록하지 않습니다. 반환된 source_id로 firmware_start를 호출하세요.",active_required:false},
    CommandDoc{name:"firmware_start",method:"POST",path:"/api/v1/firmware/start",title:"펌웨어 기록",description:"image_id 또는 업로드 source_id 하나와 confirmed=true로 기록합니다. 용량 부족이면 지우기 전에 오류. 채널 자원을 정리하고 기록·검증하며, 기록 시작 후에는 강제 취소하지 않습니다. STM32는 리셋, MS2130은 다음 보드 재기동에 적용됩니다.",active_required:false},
    CommandDoc{name:"firmware_job_get",method:"POST",path:"/api/v1/firmware/job",title:"펌웨어 작업 조회",description:"같은 API 키의 원래 채널에 고정된 작업과 검증 결과를 조회합니다. 활성 채널이 없어도 됩니다.",active_required:false},
    CommandDoc{name:"firmware_job_cancel",method:"POST",path:"/api/v1/firmware/cancel",title:"기록 전 취소",description:"지우기·기록 전이면 취소합니다. 기록 시작 후에는 can_cancel=false이며 검증 완료까지 기다립니다.",active_required:false},
    CommandDoc{name:"firmware_get_status",method:"POST",path:"/api/v1/firmware/status",title:"펌웨어 진행 상태",description:"앱의 진행 중인 펌웨어 작업 메타데이터를 조회합니다.",active_required:false},
    CommandDoc { name:"events_get_snapshot", method:"GET", path:"/api/v1/events/snapshot", title:"이벤트 상태 재조회", description:"이벤트와 같은 캐시 메타데이터 전체를 읽습니다. 장치 기능을 시작하지 않습니다.", active_required:false },
    CommandDoc{name:"logs_open_folder",method:"POST",path:"/api/v1/logs/open-folder",title:"로그 폴더 열기",description:"앱에 고정된 로그 폴더를 호스트의 기본 파일 관리자로 여는 명시적 요청입니다. 빈 인자만 받으며 임의 경로/URL은 받지 않고 호스트 경로를 응답에 노출하지 않습니다. requested는 OS 요청 수락이며 실제 창 표시를 확인한 결과가 아닙니다.",active_required:false},
    CommandDoc{name:"logs_get_state",method:"GET",path:"/api/v1/logs",title:"동작 로그 조회",description:"현재 실행의 기록 설정·최근300건·유한 큐/누락·기록 오류를 조회합니다. 앱 전역 정보이며 active가 없어도 조회합니다. 키 입력값·파일 본문·키 원문·호스트 경로는 반환하지 않습니다.",active_required:false},
    CommandDoc{name:"logs_configure",method:"POST",path:"/api/v1/logs/configure",title:"로그 설정",description:"enabled/verbose 불리언으로 이번 실행의 기록·상세 메타데이터 기록을 설정하고 파일을 flush합니다. 기본ON/상세OFF, JSONL 4MiB×최대5개, 최근300건·대기1024건입니다. 이 설정으로 장치 기능을 켜지 않습니다.",active_required:false},
    CommandDoc{name:"logs_flush",method:"POST",path:"/api/v1/logs/flush",title:"대기 로그 기록",description:"빈 인자로 대기 중인 메타데이터를 앱 로그에 기록합니다. 임의 경로를 받지 않으며 파일 기록 실패가 USB 제어를 중지시키지 않습니다.",active_required:false},
    CommandDoc{name:"diagnostics_get",method:"GET",path:"/api/v1/active/diagnostics",title:"채널 진단",description:"활성 채널의 캐시된 MCU STATUS64·신선도·호스트 transport/영상/HID/TTY·NCM·저장소 작업 카운터를 조회합니다. 조회가 기능을 켜거나 키/파일/패킷 본문을 읽지 않습니다. 아직 수신하지 않은 MCU 상태는 null이며 정상으로 추정하지 않습니다.",active_required:true},
    CommandDoc{name:"wake_get_status",method:"GET",path:"/api/v1/active/wake",title:"PC 깨우기 설정",description:"현재 활성 채널의 저장된 WOL 설정과 USB wake/System Control 지원 여부를 조회합니다. 조회나 enabled 저장은 패킷을 발신하지 않습니다.",active_required:true},
    CommandDoc{name:"wake_list_adapters",method:"GET",path:"/api/v1/active/wake/adapters",title:"WOL LAN 어댑터",description:"현재 PC에서 연결된 브로드캐스트 가능 IPv4 LAN 주소와 broadcast를 조회합니다. USB/NCM이나 NIC·route 설정을 변경하지 않습니다.",active_required:true},
    CommandDoc{name:"wake_configure",method:"POST",path:"/api/v1/active/wake/configure",title:"WOL 설정 저장",description:"enabled/mac/local_address를 현재 활성 채널의 UID에 저장합니다. MAC은 유니캐스트 주소로 정규화하고 실제 발신 때 현재 LAN 주소를 다시 확인합니다. 저장은 패킷을 발신하지 않습니다.",active_required:true},
    CommandDoc{name:"wake_send",method:"POST",path:"/api/v1/active/wake/send",title:"WOL 발신",description:"빈 인자로 현재 채널에 저장된 WOL 설정을 사용해 명시적 발신합니다. 선택한 PC LAN IPv4에 bind하고 해당 subnet의 broadcast UDP9로 102B Magic Packet을 한 번 보냅니다. code128은 발신/기상 미확인,129는 비활성,130은 미지원입니다. UID/호스트 경로/임의 목적지·포트는 받지 않습니다.",active_required:true},
    CommandDoc{name:"network_forward_configure",method:"POST",path:"/api/v1/active/network/forward/configure",title:"포워딩 규칙 저장",description:"현재 채널의 중지된 포워딩 규칙을 설정합니다. rules 각 항목은 id/protocol(tcp/udp)/listen_port/target_ip/target_port입니다. 로컬 수신은127.0.0.1로 고정되며 타깃은 NCM /24입니다. 빈 배열은 목록을 지우며 저장만으로 포트를 열지 않습니다. UID별 설정 파일에 원자 저장하며 앱 재실행 시 자동으로 시작하지 않습니다.",active_required:true},
    CommandDoc{name:"network_forward_start",method:"POST",path:"/api/v1/active/network/forward/start",title:"포워딩 시작",description:"rules1..32개로 새 loopback TCP/UDP 수신 포트를 열어 현재 NCM 타깃으로 연결합니다. UID/인터페이스를 받지 않으며 호스트 공유와 합쳐 최대32연결입니다. 유효한 시작 요청은 기존 포워딩을 중지한 뒤 준비하고 실패 시 새 포트를 모두 닫습니다. 성공한 자원은 창/클라이언트 종료 후에도 유지합니다.",active_required:true},
    CommandDoc{name:"network_forward_stop",method:"POST",path:"/api/v1/active/network/forward/stop",title:"포워딩 중지",description:"현재 채널의 새 loopback 수신 포트와 프록시 연결을 닫고 대기 전송을 취소합니다. 소켓 정리 후 응답하며 NCM/호스트 공유는 유지합니다.",active_required:true},
    CommandDoc{name:"network_get_status",method:"GET",path:"/api/v1/active/network",title:"네트워크 상태",description:"현재 채널의 NCM 모드·연결 상태·통계·DHCP 임대·정리 필요 여부를 조회합니다. UID나 query 인자는 없습니다. 완료된 네트워크 자원은 제어 창/클라이언트 종료 후에도 유지됩니다.",active_required:true},
    CommandDoc{name:"network_configure",method:"POST",path:"/api/v1/active/network/configure",title:"네트워크 설정",description:"NCM OFF 상태에서 사설 .1 서버 IPv4·DHCP 범위·임대 시간을 설정합니다. 현재 PC의 NIC/라우트/방화벽은 변경하지 않습니다.",active_required:true},
    CommandDoc{name:"network_set_mode",method:"POST",path:"/api/v1/active/network/mode",title:"NCM 모드 변경",description:"off/emulation/host 중 하나를 지정합니다. host는 현재 PC 경로로 TCP/UDP·DNS를 중계합니다. USB 재연결 전에 원래 HID를 해제하며 수락된 작업은 연결 종료 후에도 정리까지 진행합니다. 결과 불명 시 원래 채널에서 off로 복구하세요.",active_required:true},
    CommandDoc{name:"network_scan_start",method:"POST",path:"/api/v1/active/network/scan/start",title:"타깃 포트 스캔",description:"NCM이 연결된 현재 채널의 /24 타깃 하나를 명시적으로 SYN 스캔합니다. target/start/end/timeout_ms를 지정하고 network_get_status.scan에서 진행·열림/닫힘/응답 없음을 확인합니다. 최대8개 동시 프로브이며 키/클라이언트 종료·채널 전환·NCM OFF에서 취소합니다. PC 네트워크는 스캔하지 않습니다.",active_required:true},
    CommandDoc{name:"network_scan_stop",method:"POST",path:"/api/v1/active/network/scan/stop",title:"포트 스캔 중지",description:"현재 채널의 스캔을 중지하고 대기 프로브를 취소합니다. 이미 전송된 패킷은 되돌리지 않으며 남은 포트를 응답 없음으로 합산하지 않습니다. NCM 링크와 호스트 공유는 유지합니다.",active_required:true},
    CommandDoc{name:"storage_upload_prepare",method:"POST",path:"/api/v1/active/storage/uploads/prepare",title:"이미지 업로드 주소",description:"광학 또는 디스크 파일의 이름·크기·확인 여부로10분 유효한 단회 REST 업로드 URL을 발급합니다. 같은 키의 Bearer 인증과 application/octet-stream 파일 본문을 사용하세요. MCP에 파일을 넣지 마세요. 수신 후 job_id로 SD 검증 완료를 확인합니다.",active_required:true},
    CommandDoc{name:"storage_action",method:"POST",path:"/api/v1/active/storage/actions",title:"SD 저장소 작업",description:"SD 목록·메타데이터·광학/raw 매체·섹터 I/O·이미지 생성/포맷·전체 카드 포맷・폴더 공유를 관리합니다. disk_create는64..32768MiB, disk_format은 기존 용량을 보존합니다. card_format은FAT32/exFAT·수락 후 취소 불가이며 결과 불명 시 card_format_status로 확인합니다. folder_share는 앱에서 선택한 source_id로 새 RO 스냅샷을 만들어 타깃에 연결하며, folder_forget은 선택 목록에서만 해제합니다. 호스트 경로·UID는 받지 않습니다. confirmed:true와 expected_card_uuid/expected_size/expected_lease로 고정하고 job_id로 완료를 확인하세요. 섹터 쓰기는512바이트입니다.",active_required:true},
    CommandDoc{name:"storage_get_status",method:"GET",path:"/api/v1/active/storage",title:"SD 저장소 상태",description:"공통 스냅샷·작업·소유 매체·정리 오류·card_format 보류·앱에서 선택한 folder_sources를 반환합니다. 폴더에는 source_id/이름/만료 시각만 포함되며 경로는 공개하지 않습니다. stale이면 목록을 갱신하세요. active가 없으면 앱 상태의 storage_resources/storage_cleanup_error/storage_card_format을 조회하세요.",active_required:true},
    CommandDoc{name:"storage_job_get",method:"POST",path:"/api/v1/storage/jobs/get",title:"SD 작업 결과",description:"같은 API 키가 시작한 job의 진행률·완료·검증 결과를 조회합니다. 원래 채널 정보가 응답에 포함됩니다.",active_required:false},
    CommandDoc{name:"storage_job_cancel",method:"POST",path:"/api/v1/storage/jobs/cancel",title:"SD 작업 취소",description:"같은 키의 원래 MCU 작업을 취소합니다. 취소 요청 수락과 MCU 종료 확인은 다르므로 결과를 다시 조회하세요.",active_required:false},
    CommandDoc{name:"tty_open",method:"POST",path:"/api/v1/active/tty/open",title:"시리얼 수신 시작",description:"현재 타깃 CDC의 수신을 시작합니다. 64KiB 유한 버퍼와 누적 credit을 사용합니다.",active_required:true},
    CommandDoc{name:"tty_close",method:"POST",path:"/api/v1/active/tty/close",title:"시리얼 수신 중지",description:"추가 수신 credit을 중지합니다. 이미 허용한 최대32KiB는 폐기될 수 있습니다. 타깃 CDC 자체를 닫지는 않습니다.",active_required:true},
    CommandDoc{name:"tty_get_status",method:"GET",path:"/api/v1/active/tty",title:"시리얼 상태",description:"타깃 line coding, TX/RX 큐·폐기 카운터와 앱 수신 상태를 조회합니다. 관리 USB baud를 변경하지 않습니다.",active_required:true},
    CommandDoc{name:"tty_read",method:"POST",path:"/api/v1/active/tty/read",title:"시리얼 수신 읽기",description:"최대4096바이트와 cursor/stream_id를 반환합니다. 버퍼 덮어쓰기는 skipped로 알리며 다른 클라이언트의 cursor를 소비하지 않습니다.",active_required:true},
    CommandDoc{name:"tty_write",method:"POST",path:"/api/v1/active/tty/write",title:"시리얼 송신",description:"1~65536바이트를 순서대로256바이트씩 전송합니다. complete와 accepted를 확인하세요. 오류/불확실 수락은 재시도하지 않습니다.",active_required:true},
    CommandDoc{name:"input_open",method:"POST",path:"/api/v1/input-sessions",title:"입력 세션 열기",description:"현재 활성 채널에 30초 입력 lease를 엽니다. UID 인자는 없습니다.",active_required:true},
    CommandDoc{name:"input_heartbeat",method:"POST",path:"/api/v1/input-sessions/heartbeat",title:"입력 세션 유지",description:"원래 키·클라이언트의 입력 세션만 갱신합니다.",active_required:true},
    CommandDoc{name:"input_close",method:"POST",path:"/api/v1/input-sessions/close",title:"입력 세션 닫기",description:"해당 소유자의 입력을 원래 MCU에서 해제하고 세션을 닫습니다.",active_required:false},
    CommandDoc{name:"hid_get_status",method:"GET",path:"/api/v1/active/hid",title:"입력 상태",description:"눌림 개수와 미확인 해제 상태를 조회합니다. 키 내용은 제공하지 않습니다.",active_required:true},
    CommandDoc{name:"hid_key_down",method:"POST",path:"/api/v1/active/hid/keys/down",title:"키 누르기",description:"USB HID usage를 누릅니다. 기본 lease 또는 지정한 입력 세션을 사용합니다.",active_required:true},
    CommandDoc{name:"hid_key_up",method:"POST",path:"/api/v1/active/hid/keys/up",title:"키 떼기",description:"해당 소유자가 누른 usage를 해제합니다.",active_required:true},
    CommandDoc{name:"hid_button_down",method:"POST",path:"/api/v1/active/hid/buttons/down",title:"마우스 버튼 누르기",description:"버튼1~5를 누릅니다. 드래그는 같은 입력 세션에서 이동·UP으로 조합합니다.",active_required:true},
    CommandDoc{name:"hid_button_up",method:"POST",path:"/api/v1/active/hid/buttons/up",title:"마우스 버튼 떼기",description:"해당 소유자의 마우스 버튼을 해제합니다.",active_required:true},
    CommandDoc{name:"hid_mouse_move",method:"POST",path:"/api/v1/active/hid/mouse/move",title:"마우스 이동",description:"캡처 영상의 픽셀 x/y를 내부 절대좌표로 변환합니다. frame_id로 오래된 geometry를 거부할 수 있습니다.",active_required:true},
    CommandDoc{name:"hid_wheel",method:"POST",path:"/api/v1/active/hid/wheel",title:"마우스 휠",description:"수직·수평 휠을 각축 -127~127 범위로 전송합니다.",active_required:true},
    CommandDoc{name:"hid_chord",method:"POST",path:"/api/v1/active/hid/chord",title:"키 조합",description:"usage 목록을 순서대로 누르고 역순으로 뗍니다. USB report 완료와 타깃 앱 처리는 다릅니다.",active_required:true},
    CommandDoc{name:"hid_click",method:"POST",path:"/api/v1/active/hid/click",title:"클릭·더블클릭",description:"영상 픽셀 좌표로 이동 후 같은 입력 트랜잭션에서 클릭합니다. count는1~2입니다.",active_required:true},
    CommandDoc{name:"hid_double_click",method:"POST",path:"/api/v1/active/hid/double-click",title:"더블클릭",description:"활성 캡처 영상 픽셀 x/y에서 DOWN/UP 두 쌍을 한 요청으로 실행합니다. 기존 hid_click/count 동작은 유지합니다.",active_required:true},
    CommandDoc{name:"hid_key_sequence",method:"POST",path:"/api/v1/active/hid/keys/sequence",title:"키 배열 순차 입력",description:"keys:[\"a\",\"b\",\"c\"]를 a DOWN/UP→b DOWN/UP→c DOWN/UP으로 처리합니다. 1~128개 키 이름/ASCII 문자, US 물리 키 매핑. REST는 문자열 배열 본문도 지원합니다. 한 활성 채널/세션에 고정하고 짝지은 UP을 미리 큐에 넣습니다. 펌웨어 변경 및 실시간 간격 보장은 없습니다. 부분 실패 시 전체를 재시도하지 마세요.",active_required:true},
    CommandDoc{name:"hid_release",method:"POST",path:"/api/v1/active/hid/release",title:"소유 입력 해제",description:"기본 lease 또는 지정한 입력 세션 소유의 키·버튼만 해제합니다.",active_required:true},
    CommandDoc{name:"hid_recover",method:"POST",path:"/api/v1/active/hid/recover",title:"입력 해제 복구",description:"활성 MCU에 RELEASE_ALL을 보내 미확인 해제를 복구합니다.",active_required:true},
    CommandDoc {name:"video_open",method:"POST",path:"/api/v1/active/video",title:"영상 열기",description:"활성 채널의 캡처를 엽니다. 빈 객체는 자동 모드, 수동은 width/height/fps입니다. 첫 프레임 준비는 영상 상태로 확인합니다.",active_required:true},
    CommandDoc {name:"video_close",method:"DELETE",path:"/api/v1/active/video",title:"영상 닫기",description:"활성 채널의 캡처를 닫습니다. 다른 스트림으로 대체하지 않습니다.",active_required:true},
    CommandDoc {name:"video_get_status",method:"GET",path:"/api/v1/active/video",title:"영상 상태",description:"UI와 동일한 캡처 상태·협상된 크기·주사율·프레임 시각을 조회합니다.",active_required:true},
    CommandDoc {name:"video_snapshot",method:"GET",path:"/api/v1/active/video/snapshot",title:"현재 장면 캡처",description:"활성 HDMI의 최근 1초 이내 프레임을 JPEG 또는 PNG로 반환합니다. format은 jpeg/png, 기본 jpeg입니다.",active_required:true},
    CommandDoc {
        name: "app_get_state",
        method: "GET",
        path: "/api/v1/state",
        title: "앱 상태",
        description: "UI와 같은 채널 목록·연결 상태·활성 채널을 조회합니다.",
        active_required: false,
    },
    CommandDoc {
        name: "channels_list",
        method: "GET",
        path: "/api/v1/channels",
        title: "채널 목록",
        description: "발견된 모든 채널의 상태 스냅샷을 조회합니다. 제어는 실행하지 않습니다.",
        active_required: false,
    },
    CommandDoc {
        name: "channel_get_active",
        method: "GET",
        path: "/api/v1/active",
        title: "활성 채널",
        description: "현재 활성 채널 하나를 조회합니다. UID를 입력하지 않습니다.",
        active_required: true,
    },
    CommandDoc {
        name: "channel_activate",
        method: "PUT",
        path: "/api/v1/active-channel",
        title: "채널 선택",
        description: "선택한 UID 하나를 활성화합니다. 여러 대상을 지정할 수 없습니다.",
        active_required: false,
    },
    CommandDoc {
        name: "channel_deactivate",
        method: "DELETE",
        path: "/api/v1/active-channel",
        title: "채널 해제",
        description: "현재 활성 채널을 해제합니다.",
        active_required: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn failed_original_release_does_not_keep_old_video_or_require_manual_ack_to_switch() {
        let (_dir, app, wire) = crate::input_api::tests::setup();
        let old = app.active.bind().unwrap();
        let owner = crate::input::Owner {
            pin: Default::default(),
            id: "ui-test".into(),
            cancelled: tokio_util::sync::CancellationToken::new(),
            command_guard: None,
        };
        app.input
            .apply(
                owner.clone(),
                old.clone(),
                crate::input::Action::Key {
                    down: true,
                    usage: 4,
                },
            )
            .await
            .unwrap();
        wire.fail_release.store(true, Ordering::Release);
        let next = Uid::from_bytes(&[2; 12]).unwrap();
        app.select(Some(next.clone())).await.unwrap();
        assert_eq!(app.active.bind().unwrap().uid, next);
        assert!(!app.snapshot().video.running);
        assert!(app.current_frame().is_err());
        assert!(app.input.snapshot(&old).release_unconfirmed);
        let sent = wire.calls.lock().clone();
        assert_eq!(sent.iter().filter(|c| c.1 == 0x107).count(), 1);
        assert_eq!(sent.last().unwrap().0, old.uid);
        assert!(sent.last().unwrap().3);
        let fresh = crate::input::Owner {
            pin: Default::default(),
            ..owner
        };
        app.input
            .apply(
                fresh,
                app.active.bind().unwrap(),
                crate::input::Action::Key {
                    down: true,
                    usage: 5,
                },
            )
            .await
            .unwrap();
        assert_eq!(wire.calls.lock().last().unwrap().0, next);
        app.shutdown().await;
    }
    #[tokio::test]
    async fn naming_binds_before_control_without_silently_switching_another_channel() {
        let (_dir, app, wire) = crate::input_api::tests::setup();
        let original = app.active.bind().unwrap();
        let other = Uid::from_bytes(&[2; 12]).unwrap();
        assert_eq!(
            app.bind_for_rename(other.clone()).await.unwrap_err().code,
            "CHANNEL_SELECTION_REQUIRED"
        );
        app.active.validate(&original).unwrap();
        assert!(wire.calls.lock().is_empty());
        assert_eq!(
            app.bind_for_rename(original.uid.clone()).await.unwrap(),
            original
        );
        app.select(None).await.unwrap();
        let selected = app.bind_for_rename(other.clone()).await.unwrap();
        assert_eq!(selected.uid, other);
        assert!(!app.snapshot().video.running);
    }
    #[tokio::test]
    async fn native_capture_guard_is_invalidated_before_channel_switch_finishes() {
        let (_dir, app, _) = crate::input_api::tests::setup();
        let b = app.active.bind().unwrap();
        let life = app.active.guard(&b).unwrap();
        assert!(!life.is_cancelled());
        let old = app.active.begin().unwrap();
        assert!(life.is_cancelled());
        assert!(app.active.guard(&b).is_err());
        app.active.finish(old.uid);
        let next = app.active.bind().unwrap();
        assert!(app.active.guard(&b).is_err());
        assert!(!app.active.guard(&next).unwrap().is_cancelled());
    }
    fn channel(n: u8) -> crate::model::Channel {
        crate::model::Channel {
            uid: Uid::from_bytes(&[n; 12]).unwrap(),
            name: String::new(),
            port: format!("test-{n}"),
            connected: true,
            features: 0x77,
            target_state: 1,
            firmware: [1, 0, 0],
            board_id: "test-board".into(),
            usb_port: 3,
            slot: Some(0),
            capture_id: Some("test-only-root-never-open".into()),
            pairing_error: String::new(),
            observed_at: 0,
        }
    }
    #[tokio::test]
    async fn delayed_ui_video_operations_cannot_retarget_after_switch() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        let a = channel(1);
        let b = channel(2);
        app.inventory.write().channels = vec![a.clone(), b.clone()];
        app.select(Some(a.uid)).await.unwrap();
        let old = app.active.bind().unwrap();
        app.select(Some(b.uid.clone())).await.unwrap();
        assert_eq!(
            app.open_pinned_video(old.clone(), Default::default())
                .await
                .unwrap_err()
                .code,
            "STALE_OPERATION"
        );
        assert_eq!(
            app.close_pinned_video(old).await.unwrap_err().code,
            "STALE_OPERATION"
        );
        assert_eq!(app.active.bind().unwrap().uid, b.uid);
        assert!(!app.snapshot().video.running);
    }
    #[tokio::test]
    async fn accepted_transition_survives_disconnected_requester() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        let target = channel(1);
        app.inventory.write().channels = vec![target.clone()];
        let gate = app.gate.lock().await;
        let client = app.clone();
        let uid = target.uid.clone();
        let request = tokio::spawn(async move { client.select(Some(uid)).await });
        tokio::task::yield_now().await;
        request.abort();
        drop(gate);
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while app.active.state().uid.as_ref() != Some(&target.uid) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!app.active.state().switching);
    }
    #[tokio::test]
    async fn no_active_and_selection_requires_connected_identity() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().into()).unwrap();
        assert_eq!(app.active.bind().unwrap_err().code, "NO_ACTIVE_CHANNEL");
        assert!(app
            .select(Some(Uid::from_bytes(&[1; 12]).unwrap()))
            .await
            .is_err());
        assert!(serde_json::from_str::<SelectChannel>(r#"{"uid":"x","uids":["y"]}"#).is_err());
        for name in ["video_open", "video_close", "video_get_status"] {
            assert_eq!(
                app.execute(name, serde_json::json!({}))
                    .await
                    .unwrap_err()
                    .code,
                "NO_ACTIVE_CHANNEL"
            );
            assert_eq!(
                app.execute(
                    name,
                    serde_json::json!({"uid":"0x000102030405060708090A0B"})
                )
                .await
                .unwrap_err()
                .code,
                "INVALID_ARGUMENTS"
            );
        }
        assert_eq!(
            app.snapshot_image(crate::video::SnapshotFormat::Jpeg)
                .await
                .err()
                .unwrap()
                .code,
            "NO_ACTIVE_CHANNEL"
        );
    }
}
