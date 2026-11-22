//! Reads the exact MiniKMB v1 localStorage keys; never opens Chromium/LevelDB.
use super::*;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    format: String,
    #[serde(deserialize_with = "entries")]
    entries: BTreeMap<String, String>,
}
fn entries<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    struct Unique;
    impl<'de> serde::de::Visitor<'de> for Unique {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("unique MiniKMB storage keys")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut source: M,
        ) -> Result<Self::Value, M::Error> {
            let mut map = BTreeMap::new();
            while let Some((key, value)) = source.next_entry::<String, String>()? {
                if map.len() >= MAX_CHANNELS * 2 || map.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom(
                        "duplicate or excessive storage keys",
                    ));
                }
            }
            Ok(map)
        }
    }
    deserializer.deserialize_map(Unique)
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub added_channels: usize,
    pub imported_fields: usize,
    pub preserved_fields: usize,
    pub ignored_keys: usize,
    pub rejected: Vec<Issue>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Issue {
    pub uid: Option<Uid>,
    pub field: String,
    pub code: String,
}
pub struct Plan {
    revision: u64,
    next: Document,
    pub report: Report,
}
impl Plan {
    pub fn revision(&self) -> u64 {
        self.revision
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyConfig {
    server_ip: String,
    pool_start: u8,
    pool_end: u8,
    lease_seconds: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyRule {
    id: String,
    port: u8,
    protocol: forwarding::Protocol,
    listen_port: u16,
    target_ip: String,
    target_port: u16,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyWol {
    enabled: bool,
    mac: String,
    local_address: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacySettings {
    config: Option<Value>,
    rules: Option<Value>,
    wol: Option<Value>,
}
fn video(value: Value) -> Result<CaptureMode, AppError> {
    let mode = match value {
        Value::String(name) => match name.as_str() {
            "auto" => CaptureMode::default(),
            "1080" => CaptureMode {
                width: 1920,
                height: 1080,
                fps: 30.0,
            },
            "720" => CaptureMode {
                width: 1280,
                height: 720,
                fps: 30.0,
            },
            "480" => CaptureMode {
                width: 640,
                height: 480,
                fps: 30.0,
            },
            _ => return Err(invalid()),
        },
        value => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct Mode {
                width: u32,
                height: u32,
                frame_rate: f64,
            }
            let mode: Mode = serde_json::from_value(value).map_err(|_| invalid())?;
            if mode.width < 2 || mode.height < 2 || mode.frame_rate < 1.0 {
                return Err(invalid());
            }
            CaptureMode {
                width: mode.width,
                height: mode.height,
                fps: mode.frame_rate,
            }
        }
    };
    crate::video::validate_mode(mode)?;
    Ok(mode)
}
fn config(value: Value) -> Result<Config, AppError> {
    let old: LegacyConfig = serde_json::from_value(value).map_err(|_| invalid())?;
    let config = Config {
        server_ip: old.server_ip,
        pool_start: old.pool_start,
        pool_end: old.pool_end,
        lease_seconds: old.lease_seconds,
    };
    config.validate()?;
    Ok(config)
}
fn rules(value: Value) -> Result<Vec<Rule>, AppError> {
    let old: Vec<LegacyRule> = serde_json::from_value(value).map_err(|_| invalid())?;
    let mut rules = vec![];
    for rule in old {
        if rule.port != 0 {
            return Err(AppError::new(
                "LEGACY_ROUTE_UNSUPPORTED",
                "MiniKMB 단일 UID 설정의 port0 규칙만 이관합니다.",
            ));
        }
        rules.push(Rule {
            id: rule.id,
            protocol: rule.protocol,
            listen_port: rule.listen_port,
            target_ip: rule.target_ip,
            target_port: rule.target_port,
        });
    }
    validate_draft(&rules)?;
    Ok(rules)
}
fn wol(value: Value) -> Result<Wol, AppError> {
    let old: LegacyWol = serde_json::from_value(value).map_err(|_| invalid())?;
    let mut wol = Wol {
        enabled: old.enabled,
        mac: old.mac.trim().to_ascii_lowercase(),
        local_address: old.local_address,
    };
    if !wol.mac.is_empty() {
        wol.mac = crate::wake::normalize_mac(&wol.mac)?;
    }
    wol.validate()?;
    Ok(wol)
}
fn reject(report: &mut Report, uid: &Uid, field: &str, error: AppError) {
    report.rejected.push(Issue {
        uid: Some(uid.clone()),
        field: field.into(),
        code: error.code.into(),
    });
}
fn parsed(bytes: &[u8]) -> Result<(Vec<Row>, Report), AppError> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    let export: Export = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if export.format != "minikmb-electron-settings/v1" || export.entries.len() > MAX_CHANNELS * 2 {
        return Err(invalid());
    }
    let mut seen = HashSet::new();
    let mut channels: HashMap<Uid, Channel> = HashMap::new();
    let mut report = Report::default();
    for (key, text) in export.entries {
        let (kind, raw) = if let Some(raw) = key.strip_prefix("minikmb.settings.") {
            ("settings", raw)
        } else if let Some(raw) = key.strip_prefix("minikmb.video.") {
            ("video", raw)
        } else {
            report.ignored_keys += 1;
            continue;
        };
        if key.len() > 96 || text.len() > 65536 {
            return Err(invalid());
        }
        let uid = Uid::try_from(raw.to_owned()).map_err(|_| invalid())?;
        if !seen.insert((uid.clone(), kind)) {
            return Err(AppError::new(
                "IMPORT_AMBIGUOUS",
                "같은 UID의 중복 설정 키가 있습니다.",
            ));
        }
        let channel = channels.entry(uid.clone()).or_default();
        if kind == "video" {
            match serde_json::from_str(&text)
                .map_err(|_| invalid())
                .and_then(video)
            {
                Ok(mode) => channel.video = Some(mode),
                Err(error) => reject(&mut report, &uid, "video", error),
            };
            continue;
        }
        let settings = match serde_json::from_str::<LegacySettings>(&text) {
            Ok(settings) => settings,
            Err(_) => {
                reject(&mut report, &uid, "settings", invalid());
                continue;
            }
        };
        if let Some(value) = settings.config {
            match config(value) {
                Ok(config) => channel.network = Some(config),
                Err(error) => reject(&mut report, &uid, "network", error),
            }
        }
        if let Some(value) = settings.rules {
            match rules(value) {
                Ok(rules) => channel.forward_rules = Some(rules),
                Err(error) => reject(&mut report, &uid, "rules", error),
            }
        }
        if let Some(value) = settings.wol {
            match wol(value) {
                Ok(wol) => channel.wol = Some(wol),
                Err(error) => reject(&mut report, &uid, "wol", error),
            }
        }
    }
    if channels.len() > MAX_CHANNELS {
        return Err(invalid());
    }
    let mut rows = channels
        .into_iter()
        .map(|(uid, preferences)| Row { uid, preferences })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.uid.raw().cmp(b.uid.raw()));
    Ok((rows, report))
}
fn field<T>(native: &mut Option<T>, old: Option<T>, report: &mut Report) {
    if let Some(value) = old {
        if native.is_some() {
            report.preserved_fields += 1;
        } else {
            *native = Some(value);
            report.imported_fields += 1;
        }
    }
}
impl Store {
    pub fn prepare_import(&self, bytes: &[u8]) -> Result<Plan, AppError> {
        let (rows, mut report) = parsed(bytes)?;
        let mut next = self.snapshot();
        let revision = next.revision;
        for row in rows {
            if row.preferences.video.is_none()
                && row.preferences.network.is_none()
                && row.preferences.forward_rules.is_none()
                && row.preferences.wol.is_none()
            {
                continue;
            }
            if !next.channels.iter().any(|native| native.uid == row.uid) {
                report.added_channels += 1;
            }
            let native = next.entry(&row.uid)?;
            let old = row.preferences;
            field(&mut native.video, old.video, &mut report);
            field(&mut native.network, old.network, &mut report);
            field(&mut native.forward_rules, old.forward_rules, &mut report);
            field(&mut native.wol, old.wol, &mut report);
        }
        next.validate()?;
        Ok(Plan {
            revision,
            next,
            report,
        })
    }
    pub fn apply_import(&self, plan: Plan) -> Result<Report, AppError> {
        let _commit = self.commit.lock();
        if self.state.read().document.revision != plan.revision {
            return Err(changed());
        }
        if plan.report.imported_fields > 0 {
            self.record(self.write(plan.next))?;
        }
        Ok(plan.report)
    }
}
