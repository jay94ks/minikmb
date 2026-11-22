//! Bounded, owner/epoch-pinned staging. Receiving bytes never programs a device.
use super::{
    image::{self, Prepared},
    manager::check_owner,
    Target,
};
use crate::{
    active::Binding,
    input_sessions::Principal,
    model::{now_ms, AppError},
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, io::Read, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Image {
    pub id: String,
    pub label: String,
    pub target: Target,
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Deserialize)]
struct Manifest {
    images: Vec<Image>,
}
pub fn images() -> Vec<Image> {
    serde_json::from_str::<Manifest>(include_str!("../../firmware-tools/images.lock.json"))
        .expect("compiled firmware manifest")
        .images
}
pub fn bundled(id: &str, target: Target) -> Result<Prepared, AppError> {
    let root = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("firmware")))
        .unwrap_or_default();
    load_bundle(root, id, target)
}
fn load_bundle(root: PathBuf, id: &str, target: Target) -> Result<Prepared, AppError> {
    let entry = images()
        .into_iter()
        .find(|i| i.id == id && i.target == target)
        .ok_or_else(|| AppError::new("FIRMWARE_NOT_FOUND", "동봉 펌웨어를 찾을 수 없습니다."))?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let mut file = options.open(root.join(entry.file)).map_err(|_| {
        AppError::new(
            "FIRMWARE_NOT_FOUND",
            "동봉 펌웨어가 없습니다. ZIP 전체를 풀어주세요.",
        )
    })?;
    if file.metadata().map_err(|_| io_error())?.len() != entry.bytes {
        return Err(changed());
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(entry.bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| io_error())?;
    if bytes.len() as u64 != entry.bytes || format!("{:x}", Sha256::digest(&bytes)) != entry.sha256
    {
        return Err(changed());
    }
    image::prepare(target, &bytes)
}
fn changed() -> AppError {
    AppError::new(
        "FIRMWARE_HASH_MISMATCH",
        "동봉 펌웨어의 크기/SHA256이 다릅니다. 기록하지 않았습니다.",
    )
}
fn io_error() -> AppError {
    AppError::new("FIRMWARE_IO_FAILED", "펌웨어 파일을 읽지 못했습니다.")
}
#[derive(Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub target: Target,
    pub name: String,
    pub size: u64,
}
#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct Source {
    pub source_id: String,
    pub target: Target,
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub expires_at: u64,
}
#[derive(Serialize, schemars::JsonSchema)]
pub struct Upload {
    pub upload_id: String,
    pub url: String,
    pub method: String,
    pub content_type: String,
    pub content_length: u64,
    pub expires_at: u64,
    pub single_use: bool,
    pub instructions: String,
}
struct Entry {
    binding: Option<Binding>,
    owner: Principal,
    spec: Spec,
    expires_at: u64,
    image: Option<Prepared>,
}
#[derive(Default)]
pub struct Pool {
    entries: Mutex<HashMap<String, Entry>>,
}
fn valid(spec: &Spec) -> Result<(), AppError> {
    let max = match spec.target {
        Target::Stm32 => 16 * 1024 * 1024,
        Target::Ms2130 => 64 * 1024 * 1024,
    };
    if spec.size == 0
        || spec.size > max
        || spec.name.is_empty()
        || spec.name.len() > 200
        || spec
            .name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "펌웨어 이름/크기를 확인하세요. STM32 최대 16MiB, ROM 최대 64MiB입니다.",
        ));
    }
    Ok(())
}
impl Pool {
    fn reserve(
        &self,
        binding: Option<Binding>,
        owner: Principal,
        spec: Spec,
    ) -> Result<(String, u64), AppError> {
        check_owner(&owner)?;
        valid(&spec)?;
        let mut entries = self.entries.lock();
        let now = now_ms();
        entries.retain(|_, e| {
            e.expires_at > now && !e.owner.cancelled.is_cancelled() && !e.owner.ended.is_cancelled()
        });
        if entries.len() >= 4
            || entries.values().map(|e| e.spec.size).sum::<u64>() + spec.size > 128 * 1024 * 1024
        {
            return Err(AppError::new(
                "FIRMWARE_STAGING_FULL",
                "준비 중인 펌웨어가 많습니다. 만료 후 다시 시도하세요.",
            ));
        }
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce)
            .map_err(|_| AppError::new("RANDOM_FAILED", "업로드 ID를 만들지 못했습니다."))?;
        let id = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let expiry = now + 600_000;
        entries.insert(
            id.clone(),
            Entry {
                binding,
                owner,
                spec,
                expires_at: expiry,
                image: None,
            },
        );
        Ok((id, expiry))
    }
    pub fn upload(
        &self,
        binding: impl Into<Option<Binding>>,
        owner: Principal,
        spec: Spec,
        address: std::net::SocketAddr,
    ) -> Result<Upload, AppError> {
        let binding = binding.into();
        let size = spec.size;
        let (id, expires_at) = self.reserve(binding, owner, spec)?;
        Ok(Upload{url:format!("http://{address}/api/v1/firmware/uploads/{id}"),upload_id:id,method:"POST".into(),content_type:"application/octet-stream".into(),content_length:size,expires_at,single_use:true,instructions:"같은 API 키로 원본 바이트를 POST하세요. 응답 source_id를 firmware_start에 전달해야 기록합니다. MCP에 파일 바이트나 로컬 경로를 넣지 마세요.".into()})
    }
    pub fn stage(
        &self,
        binding: impl Into<Option<Binding>>,
        owner: Principal,
        spec: Spec,
        bytes: &[u8],
    ) -> Result<Source, AppError> {
        let binding = binding.into();
        let (id, _) = self.reserve(binding.clone(), owner.clone(), spec)?;
        self.receive(binding.as_ref(), &owner, &id, bytes)
    }
    pub fn expected<'a>(
        &self,
        binding: impl Into<Option<&'a Binding>>,
        owner: &Principal,
        id: &str,
    ) -> Result<u64, AppError> {
        let binding = binding.into();
        let entries = self.entries.lock();
        let entry = entries.get(id).ok_or_else(missing)?;
        authorize(entry, binding, owner)?;
        if entry.image.is_some() {
            return Err(missing());
        }
        Ok(entry.spec.size)
    }
    pub fn receive<'a>(
        &self,
        binding: impl Into<Option<&'a Binding>>,
        owner: &Principal,
        id: &str,
        bytes: &[u8],
    ) -> Result<Source, AppError> {
        let binding = binding.into();
        let mut entries = self.entries.lock();
        let entry = entries.get(id).ok_or_else(missing)?;
        authorize(entry, binding, owner)?;
        if entry.image.is_some() {
            return Err(missing());
        }
        if entry.spec.size != bytes.len() as u64 {
            entries.remove(id);
            return Err(AppError::new(
                "FIRMWARE_SIZE_MISMATCH",
                "업로드 길이가 준비한 크기와 다릅니다. 기록하지 않았습니다.",
            ));
        }
        let prepared = match image::prepare(entry.spec.target, bytes) {
            Ok(p) => p,
            Err(e) => {
                entries.remove(id);
                return Err(e);
            }
        };
        let entry = entries.get_mut(id).unwrap();
        let source = Source {
            source_id: id.into(),
            target: entry.spec.target,
            name: entry.spec.name.clone(),
            bytes: prepared.bytes.len() as u64,
            sha256: prepared.sha256.clone(),
            expires_at: entry.expires_at,
        };
        entry.image = Some(prepared);
        Ok(source)
    }
    pub fn take<'a>(
        &self,
        binding: impl Into<Option<&'a Binding>>,
        owner: &Principal,
        id: &str,
        target: Target,
    ) -> Result<Prepared, AppError> {
        let binding = binding.into();
        let mut entries = self.entries.lock();
        let entry = entries.get(id).ok_or_else(missing)?;
        authorize(entry, binding, owner)?;
        if entry.spec.target != target || entry.image.is_none() {
            return Err(missing());
        }
        Ok(entries.remove(id).unwrap().image.unwrap())
    }
}
fn missing() -> AppError {
    AppError::new(
        "FIRMWARE_SOURCE_NOT_FOUND",
        "준비한 펌웨어가 없거나 만료/사용됐습니다.",
    )
}
fn authorize(entry: &Entry, binding: Option<&Binding>, owner: &Principal) -> Result<(), AppError> {
    check_owner(owner)?;
    check_owner(&entry.owner)?;
    if entry.owner.key_id != owner.key_id {
        return Err(AppError::new("FORBIDDEN", "다른 API 키의 펌웨어입니다."));
    }
    if entry.binding.as_ref() != binding || entry.expires_at <= now_ms() {
        return Err(missing());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn principal(key: &str) -> Principal {
        Principal {
            key_id: key.into(),
            client_id: "rest".into(),
            cancelled: Default::default(),
            ended: Default::default(),
        }
    }
    #[test]
    fn upload_is_exact_length_single_use_owner_and_epoch_bound() {
        let pool = Pool::default();
        let b = Binding {
            uid: crate::model::Uid::from_bytes(&[1; 12]).unwrap(),
            generation: 7,
        };
        let p = principal("A");
        let spec = Spec {
            target: Target::Ms2130,
            name: "rom.bin".into(),
            size: 8,
        };
        let ticket = pool
            .upload(
                b.clone(),
                p.clone(),
                spec.clone(),
                "127.0.0.1:12345".parse().unwrap(),
            )
            .unwrap();
        assert_eq!(
            pool.expected(&b, &principal("B"), &ticket.upload_id)
                .unwrap_err()
                .code,
            "FORBIDDEN"
        );
        assert!(pool
            .expected(
                &Binding {
                    generation: 8,
                    ..b.clone()
                },
                &p,
                &ticket.upload_id
            )
            .is_err());
        assert!(pool.receive(&b, &p, &ticket.upload_id, &[1; 7]).is_err());
        assert!(pool.expected(&b, &p, &ticket.upload_id).is_err());
        let source = pool.stage(b.clone(), p.clone(), spec, &[1; 8]).unwrap();
        assert!(pool.receive(&b, &p, &source.source_id, &[1; 8]).is_err());
        assert!(pool.take(&b, &p, &source.source_id, Target::Stm32).is_err());
        assert_eq!(
            pool.take(&b, &p, &source.source_id, Target::Ms2130)
                .unwrap()
                .bytes,
            vec![1; 8]
        );
        assert!(pool
            .take(&b, &p, &source.source_id, Target::Ms2130)
            .is_err());
    }
    #[test]
    fn invalid_or_revoked_sources_never_become_a_programmable_image() {
        let pool = Pool::default();
        let b = Binding {
            uid: crate::model::Uid::from_bytes(&[1; 12]).unwrap(),
            generation: 1,
        };
        let p = principal("A");
        let ticket = pool
            .upload(
                b.clone(),
                p.clone(),
                Spec {
                    target: Target::Stm32,
                    name: "bad.elf".into(),
                    size: 8,
                },
                "127.0.0.1:12345".parse().unwrap(),
            )
            .unwrap();
        assert!(pool.receive(&b, &p, &ticket.upload_id, &[0; 8]).is_err());
        assert!(pool.take(&b, &p, &ticket.upload_id, Target::Stm32).is_err());
        let source = pool
            .stage(
                b.clone(),
                p.clone(),
                Spec {
                    target: Target::Ms2130,
                    name: "rom.bin".into(),
                    size: 8,
                },
                &[1; 8],
            )
            .unwrap();
        p.cancelled.cancel();
        assert!(pool
            .take(&b, &principal("A"), &source.source_id, Target::Ms2130)
            .is_err());
    }
}
