use crate::model::{now_ms, AppError};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Write, path::PathBuf};
use subtle::ConstantTimeEq;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyInfo {
    pub id: String,
    pub name: String,
    pub fingerprint: String,
    pub created_at: u64,
    pub last_used: Option<u64>,
}
#[derive(Serialize, Deserialize)]
struct Record {
    info: KeyInfo,
    digest: [u8; 32],
    #[serde(skip)]
    revoked: tokio_util::sync::CancellationToken,
}
#[derive(Clone)]
pub struct AuthLease {
    pub info: KeyInfo,
    pub revoked: tokio_util::sync::CancellationToken,
}
pub struct SecretToken(String);
impl SecretToken {
    pub fn expose(&self) -> &str {
        &self.0
    }
}
pub struct CreatedKey {
    pub info: KeyInfo,
    pub token: SecretToken,
}
pub struct KeyStore {
    file: PathBuf,
    records: Mutex<Vec<Record>>,
}
impl KeyStore {
    pub fn load(file: PathBuf) -> Result<Self, AppError> {
        let records = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice::<Vec<Record>>(&bytes).map_err(|_| {
                AppError::new("KEY_STORE_INVALID", "인증 키 파일을 읽을 수 없습니다.")
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => {
                return Err(AppError::new(
                    "KEY_STORE_IO",
                    "인증 키 파일에 접근할 수 없습니다.",
                ))
            }
        };
        Ok(Self {
            file,
            records: Mutex::new(records),
        })
    }
    fn save(&self, records: &[Record]) -> Result<(), AppError> {
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            let parent = self.file.parent().ok_or("invalid key path")?;
            std::fs::create_dir_all(parent)?;
            let mut temp = tempfile::NamedTempFile::new_in(parent)?;
            temp.write_all(&serde_json::to_vec_pretty(records)?)?;
            temp.as_file().sync_all()?;
            temp.persist(&self.file).map_err(|e| e.error)?;
            Ok(())
        })();
        result.map_err(|_| AppError::new("KEY_STORE_IO", "인증 키를 저장할 수 없습니다."))
    }
    pub fn list(&self) -> Vec<KeyInfo> {
        self.records.lock().iter().map(|r| r.info.clone()).collect()
    }
    pub fn create(&self, name: &str) -> Result<CreatedKey, AppError> {
        let mut random = [0u8; 32];
        getrandom::fill(&mut random)
            .map_err(|_| AppError::new("RANDOM_FAILED", "난수 생성에 실패했습니다."))?;
        let token = format!("mkb_{}", URL_SAFE_NO_PAD.encode(random));
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let mut id = [0u8; 16];
        getrandom::fill(&mut id)
            .map_err(|_| AppError::new("RANDOM_FAILED", "난수 생성에 실패했습니다."))?;
        let info = KeyInfo {
            id: URL_SAFE_NO_PAD.encode(id),
            name: name.trim().chars().take(64).collect(),
            fingerprint: URL_SAFE_NO_PAD.encode(&digest[..6]),
            created_at: now_ms(),
            last_used: None,
        };
        let mut records = self.records.lock();
        if records.len() >= 128 {
            return Err(AppError::new(
                "KEY_LIMIT",
                "사용하지 않는 키를 삭제한 뒤 다시 시도하세요.",
            ));
        }
        records.push(Record {
            info: info.clone(),
            digest,
            revoked: tokio_util::sync::CancellationToken::new(),
        });
        if let Err(e) = self.save(&records) {
            records.pop();
            return Err(e);
        }
        Ok(CreatedKey {
            info,
            token: SecretToken(token),
        })
    }
    pub fn authenticate(&self, token: &str) -> Option<KeyInfo> {
        self.authorize(token).map(|a| a.info)
    }
    pub fn authorize(&self, token: &str) -> Option<AuthLease> {
        if !token.starts_with("mkb_") || token.len() > 128 {
            return None;
        }
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let mut records = self.records.lock();
        let record = records
            .iter_mut()
            .find(|r| bool::from(r.digest.ct_eq(&digest)))?;
        record.info.last_used = Some(now_ms());
        Some(AuthLease {
            info: record.info.clone(),
            revoked: record.revoked.clone(),
        })
    }
    pub fn revoke(&self, id: &str) -> Result<bool, AppError> {
        let mut records = self.records.lock();
        let Some(at) = records.iter().position(|r| r.info.id == id) else {
            return Ok(false);
        };
        let record = records.remove(at);
        if let Err(e) = self.save(&records) {
            records.insert(at, record);
            return Err(e);
        }
        record.revoked.cancel();
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiple_keys_persist_without_secrets_and_revoke_independently() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("keys.json");
        let store = KeyStore::load(file.clone()).unwrap();
        let a = store.create("A").unwrap();
        let b = store.create("B").unwrap();
        assert_ne!(a.token.expose(), b.token.expose());
        let raw = std::fs::read_to_string(&file).unwrap();
        assert!(!raw.contains(a.token.expose()));
        assert!(!raw.contains(b.token.expose()));
        let store = KeyStore::load(file).unwrap();
        assert!(store.authenticate(a.token.expose()).is_some());
        let lease_a = store.authorize(a.token.expose()).unwrap();
        let lease_b = store.authorize(b.token.expose()).unwrap();
        assert!(store.revoke(&a.info.id).unwrap());
        assert!(lease_a.revoked.is_cancelled());
        assert!(!lease_b.revoked.is_cancelled());
        assert!(store.authenticate(a.token.expose()).is_none());
        assert!(store.authenticate(b.token.expose()).is_some());
        assert!(store.authenticate("invalid").is_none());
    }
}
