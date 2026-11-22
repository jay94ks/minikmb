//! Bounded, atomic preferences. This data cannot activate devices or services.
use crate::{
    media::CaptureMode,
    model::{AppError, Uid},
    network::{
        forwarding::{self, Rule},
        Config,
    },
};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
pub mod legacy;
#[cfg(test)]
mod tests;
pub const MAX_BYTES: u64 = 1024 * 1024;
pub const MAX_CHANNELS: usize = 64;
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Wol {
    pub enabled: bool,
    pub mac: String,
    pub local_address: String,
}
impl Wol {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.mac.len() > 17 || self.local_address.len() > 15 {
            return Err(invalid());
        }
        if !self.mac.is_empty() {
            crate::wake::normalize_mac(&self.mac)?;
        }
        if !self.local_address.is_empty() {
            let ip = self
                .local_address
                .parse::<std::net::Ipv4Addr>()
                .map_err(|_| invalid())?;
            if ip.to_string() != self.local_address
                || ip.is_unspecified()
                || ip.is_loopback()
                || ip.is_multicast()
                || ip.is_broadcast()
            {
                return Err(invalid());
            }
        }
        if self.enabled && (self.mac.is_empty() || self.local_address.is_empty()) {
            return Err(invalid());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Channel {
    pub video: Option<CaptureMode>,
    pub network: Option<Config>,
    pub forward_rules: Option<Vec<Rule>>,
    pub wol: Option<Wol>,
}
impl Channel {
    pub fn config(&self) -> Config {
        self.network.clone().unwrap_or_default()
    }
    pub fn rules(&self) -> Vec<Rule> {
        self.forward_rules.clone().unwrap_or_default()
    }
    pub fn validate(&self) -> Result<(), AppError> {
        if let Some(mode) = self.video {
            crate::video::validate_mode(mode)?;
        }
        if let Some(config) = &self.network {
            config.validate()?;
        }
        if let Some(rules) = &self.forward_rules {
            validate_draft(rules)?;
        }
        if let Some(wol) = &self.wol {
            wol.validate()?;
        }
        Ok(())
    }
}
/// A saved draft may retain its old subnet after the user changes network settings.
/// Starting forwarding still checks every rule against the *running* NCM config.
pub fn validate_draft(rules: &[Rule]) -> Result<(), AppError> {
    if rules.len() > 32 {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut binds = HashSet::new();
    for rule in rules {
        let mut ip = rule
            .target_ip
            .parse::<std::net::Ipv4Addr>()
            .map_err(|_| invalid())?
            .octets();
        ip[3] = 1;
        forwarding::validate(
            std::slice::from_ref(rule),
            &Config {
                server_ip: std::net::Ipv4Addr::from(ip).to_string(),
                ..Default::default()
            },
        )?;
        if !ids.insert(&rule.id) || !binds.insert((rule.protocol, rule.listen_port)) {
            return Err(invalid());
        }
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub uid: Uid,
    pub preferences: Channel,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub version: u32,
    pub revision: u64,
    pub channels: Vec<Row>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channel_order: Vec<Uid>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            channels: vec![],
            channel_order: vec![],
        }
    }
}
impl Document {
    fn validate(&self) -> Result<(), AppError> {
        if self.version != 1
            || self.channels.len() > MAX_CHANNELS
            || self.channel_order.len() > MAX_CHANNELS
        {
            return Err(invalid());
        }
        let mut ids = HashSet::new();
        for uid in &self.channel_order {
            if !ids.insert(uid) {
                return Err(invalid());
            }
        }
        ids.clear();
        for row in &self.channels {
            if !ids.insert(&row.uid) {
                return Err(invalid());
            }
            row.preferences.validate()?;
        }
        Ok(())
    }
    pub fn channel(&self, uid: &Uid) -> Channel {
        self.channels
            .iter()
            .find(|row| &row.uid == uid)
            .map(|row| row.preferences.clone())
            .unwrap_or_default()
    }
    pub fn channel_rank(&self, uid: &Uid) -> usize {
        self.channel_order
            .iter()
            .position(|item| item == uid)
            .unwrap_or(usize::MAX)
    }
    fn entry(&mut self, uid: &Uid) -> Result<&mut Channel, AppError> {
        let index = if let Some(index) = self.channels.iter().position(|row| &row.uid == uid) {
            index
        } else {
            if self.channels.len() >= MAX_CHANNELS {
                return Err(AppError::new(
                    "PREFERENCES_LIMIT",
                    "설정은 최대64개 채널을 보관합니다.",
                ));
            }
            self.channels.push(Row {
                uid: uid.clone(),
                preferences: Default::default(),
            });
            self.channels.len() - 1
        };
        Ok(&mut self.channels[index].preferences)
    }
}
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Status {
    pub revision: u64,
    pub channels: usize,
    pub blocked: Option<AppError>,
    pub error: Option<AppError>,
}
struct State {
    document: Document,
    stamp: Option<Vec<u8>>,
    blocked: Option<AppError>,
}
pub struct Store {
    path: Option<PathBuf>,
    state: RwLock<State>,
    commit: Mutex<()>,
    error: RwLock<Option<AppError>>,
}
pub fn invalid() -> AppError {
    AppError::new(
        "PREFERENCES_INVALID",
        "설정의 버전·채널·값을 확인하지 못했습니다. 원본 파일은 보존했습니다.",
    )
}
fn io_error() -> AppError {
    AppError::new(
        "PREFERENCES_IO",
        "설정을 저장하거나 읽을 수 없습니다. 기존 설정은 보존했습니다.",
    )
}
fn changed() -> AppError {
    AppError::new(
        "PREFERENCES_CHANGED",
        "다른 작업에서 설정이 바뀌었습니다. 설정을 다시 불러온 뒤 시도하세요.",
    )
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(io_error()),
    };
    let meta = file.metadata().map_err(|_| io_error())?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err(invalid());
    }
    let mut bytes = vec![];
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| io_error())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    Ok(Some(bytes))
}
fn decode(bytes: &Option<Vec<u8>>) -> Result<Document, AppError> {
    let document = match bytes {
        Some(bytes) => serde_json::from_slice::<Document>(bytes).map_err(|_| invalid())?,
        None => Document::default(),
    };
    document.validate()?;
    Ok(document)
}
impl Store {
    pub fn memory() -> Arc<Self> {
        Arc::new(Self {
            path: None,
            state: RwLock::new(State {
                document: Default::default(),
                stamp: None,
                blocked: None,
            }),
            commit: Mutex::new(()),
            error: RwLock::new(None),
        })
    }
    pub fn load(path: PathBuf) -> Arc<Self> {
        let stamp = read(&path);
        let decoded = stamp.as_ref().map_err(Clone::clone).and_then(decode);
        let (document, blocked) = match decoded {
            Ok(document) => (document, None),
            Err(error) => (Document::default(), Some(error)),
        };
        Arc::new(Self {
            path: Some(path),
            state: RwLock::new(State {
                document,
                stamp: stamp.ok().flatten(),
                blocked,
            }),
            commit: Mutex::new(()),
            error: RwLock::new(None),
        })
    }
    pub fn snapshot(&self) -> Document {
        self.state.read().document.clone()
    }
    pub fn channel(&self, uid: &Uid) -> Channel {
        self.state.read().document.channel(uid)
    }
    pub fn sort_channels(&self, channels: &mut [crate::model::Channel]) {
        let state = self.state.read();
        // Stable sort leaves newly discovered, unsaved channels in discovery order.
        channels.sort_by_key(|channel| state.document.channel_rank(&channel.uid));
    }
    pub fn move_channel(
        &self,
        source: &Uid,
        target: &Uid,
        after: bool,
        observed: &[Uid],
    ) -> Result<(), AppError> {
        let _commit = self.commit.lock();
        let result = (|| {
            if observed.len() > MAX_CHANNELS
                || observed.iter().collect::<HashSet<_>>().len() != observed.len()
                || !observed.contains(source)
                || !observed.contains(target)
            {
                return Err(AppError::new(
                    "CHANNEL_ORDER_CHANGED",
                    "채널 목록이 변경됐습니다. 다시 드래그하세요.",
                ));
            }
            if source == target {
                return Ok(());
            }
            let mut document = self.snapshot();
            let mut order = document.channel_order.clone();
            // Keep hidden/disconnected UID positions so reconnect restores order.
            for uid in observed {
                if !order.contains(uid) {
                    order.push(uid.clone());
                }
            }
            if order.len() > MAX_CHANNELS {
                return Err(AppError::new(
                    "PREFERENCES_LIMIT",
                    "카드 정렬은 최대64개 채널을 보관합니다.",
                ));
            }
            order.retain(|uid| uid != source);
            let at = order
                .iter()
                .position(|uid| uid == target)
                .ok_or_else(invalid)?
                + usize::from(after);
            order.insert(at, source.clone());
            if order == document.channel_order {
                return Ok(());
            }
            document.channel_order = order;
            self.write(document)
        })();
        self.record(result)
    }
    pub fn status(&self) -> Status {
        let state = self.state.read();
        Status {
            revision: state.document.revision,
            channels: state.document.channels.len(),
            blocked: state.blocked.clone(),
            error: self.error.read().clone(),
        }
    }
    pub fn reload(&self) -> Result<(), AppError> {
        let _commit = self.commit.lock();
        let Some(path) = &self.path else {
            return Ok(());
        };
        let result: Result<(), AppError> = (|| {
            let stamp = read(path)?;
            let document = decode(&stamp)?;
            *self.state.write() = State {
                document,
                stamp,
                blocked: None,
            };
            Ok(())
        })();
        if let Err(error) = &result {
            // Preserve the last usable values while exposing explicit recovery.
            self.state.write().blocked = Some(error.clone());
        }
        self.record(result)
    }
    fn record<T>(&self, result: Result<T, AppError>) -> Result<T, AppError> {
        *self.error.write() = result.as_ref().err().cloned();
        result
    }
    fn lock_file(&self) -> Result<Option<File>, AppError> {
        let Some(path) = &self.path else {
            return Ok(None);
        };
        let parent = path.parent().ok_or_else(io_error)?;
        std::fs::create_dir_all(parent).map_err(|_| io_error())?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .map_err(|_| io_error())?;
        file.try_lock().map_err(|_| {
            AppError::new(
                "PREFERENCES_BUSY",
                "다른 MiniKMB 프로세스가 설정을 저장하고 있습니다.",
            )
        })?;
        Ok(Some(file))
    }
    fn write(&self, document: Document) -> Result<(), AppError> {
        let _lock = self.lock_file()?;
        self.write_locked(document)
    }
    fn write_locked(&self, mut document: Document) -> Result<(), AppError> {
        document.validate()?;
        let (revision, stamp, blocked) = {
            let state = self.state.read();
            (
                state.document.revision,
                state.stamp.clone(),
                state.blocked.clone(),
            )
        };
        if let Some(error) = blocked {
            return Err(error);
        }
        document.revision = revision.checked_add(1).ok_or_else(invalid)?;
        let bytes = serde_json::to_vec_pretty(&document).map_err(|_| invalid())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid());
        }
        if let Some(path) = &self.path {
            if read(path)? != stamp {
                return Err(changed());
            }
            let mut temp = tempfile::NamedTempFile::new_in(path.parent().ok_or_else(io_error)?)
                .map_err(|_| io_error())?;
            temp.write_all(&bytes).map_err(|_| io_error())?;
            temp.as_file().sync_all().map_err(|_| io_error())?;
            temp.persist(path).map_err(|_| io_error())?;
        }
        *self.state.write() = State {
            document,
            stamp: Some(bytes),
            blocked: None,
        };
        Ok(())
    }
    fn update(&self, uid: &Uid, edit: impl FnOnce(&mut Channel)) -> Result<(), AppError> {
        let _commit = self.commit.lock();
        let result = (|| {
            let mut document = self.snapshot();
            let channel = document.entry(uid)?;
            edit(channel);
            self.write(document)
        })();
        self.record(result)
    }
    pub fn network(&self, uid: &Uid, config: Config) -> Result<(), AppError> {
        self.update(uid, |channel| channel.network = Some(config))
    }
    pub fn forwards(&self, uid: &Uid, rules: Vec<Rule>) -> Result<(), AppError> {
        self.update(uid, |channel| channel.forward_rules = Some(rules))
    }
    pub fn video(&self, uid: &Uid, mode: CaptureMode) -> Result<(), AppError> {
        if self.channel(uid).video == Some(mode) {
            return Ok(());
        }
        self.update(uid, |channel| channel.video = Some(mode))
    }
    pub fn wol(&self, uid: &Uid, config: Wol) -> Result<(), AppError> {
        self.update(uid, |channel| channel.wol = Some(config))
    }
    pub fn recover_invalid(&self, confirmed: bool) -> Result<(), AppError> {
        if !confirmed {
            return Err(AppError::new(
                "CONFIRMATION_REQUIRED",
                "원본 보관 후 설정 초기화를 확인하세요.",
            ));
        }
        let _commit = self.commit.lock();
        let _lock = self.lock_file()?;
        let Some(path) = &self.path else {
            return Err(invalid());
        };
        if self.state.read().blocked.is_none() {
            return Err(changed());
        }
        if read(path).and_then(|bytes| decode(&bytes)).is_ok() {
            return Err(changed());
        }
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(|_| io_error())?;
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let backup = path.with_extension(format!("invalid-{suffix}.json"));
        std::fs::rename(path, &backup).map_err(|_| io_error())?;
        *self.state.write() = State {
            document: Default::default(),
            stamp: None,
            blocked: None,
        };
        if let Err(error) = self.write_locked(Document::default()) {
            let _ = std::fs::rename(backup, path);
            let stamp = read(path).ok().flatten();
            self.state.write().stamp = stamp;
            self.state.write().blocked = Some(error.clone());
            return self.record(Err(error));
        }
        self.record(Ok(()))
    }
}
