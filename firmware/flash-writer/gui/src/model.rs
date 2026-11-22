use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::Arc};

pub const DEMO_CAPACITY: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Definition {
    pub vendor: String,
    pub name: String,
    pub capacity: usize,
    pub erased_value: u8,
    pub can_read: bool,
    pub can_write: bool,
    pub can_erase: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub port: String,
    pub serial: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chip {
    pub device: Device,
    pub name: String,
    pub jedec: String,
    pub capacity: usize,
    pub actual_hz: u32,
    pub sfdp: String,
    pub definition: Option<Definition>,
    pub candidates: Vec<String>,
}

impl Chip {
    pub fn ready(&self) -> bool { self.definition.is_some() && self.capacity > 0 }
    pub fn require_definition(&self) -> Result<&Definition> {
        ensure!(self.capacity > 0, "칩 용량을 확정할 수 없습니다. DB에서 정확한 모델을 선택하세요.");
        self.definition.as_ref().context("프로그래머는 연결됐습니다. 칩 모델을 선택하고 다시 식별하세요.")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    pub start: usize,
    pub len: usize,
}

impl Region {
    pub fn new(start: usize, len: usize, capacity: usize) -> Result<Self> {
        ensure!(len > 0, "길이는 0보다 커야 합니다.");
        let end = start.checked_add(len).context("주소 범위가 너무 큽니다.")?;
        ensure!(end <= capacity, "범위가 플래시 용량({capacity} bytes)을 초과합니다.");
        Ok(Self { start, len })
    }

    pub fn end(self) -> usize { self.start + self.len }
    pub fn label(self) -> String { format!("0x{:06X} — 0x{:06X} ({} bytes)", self.start, self.end()-1, self.len) }

}

pub fn number(text: &str) -> Result<usize> {
    let value = text.trim().replace('_', "");
    if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16).context("올바른 16진수 주소가 아닙니다.")
    } else {
        value.parse().context("10진수 또는 0x로 시작하는 16진수를 입력하세요.")
    }
}

pub fn compose_write(original: &[u8], image: &[u8], start: usize) -> Result<Vec<u8>> {
    let range = Region::new(start, image.len(), original.len())?;
    let mut target = original.to_vec();
    target[range.start..range.end()].copy_from_slice(image);
    Ok(target)
}

pub fn compose_erase(original: &[u8], range: Region, erased_value: u8) -> Result<Vec<u8>> {
    Region::new(range.start, range.len, original.len())?;
    let mut target = original.to_vec();
    target[range.start..range.end()].fill(erased_value);
    Ok(target)
}

pub fn sha256(data: &[u8]) -> String { format!("{:x}", Sha256::digest(data)) }

pub fn equal(actual: &[u8], expected: &[u8], offset: usize) -> Result<()> {
    ensure!(actual.len() == expected.len(), "검증 길이가 다릅니다: {} / {}", actual.len(), expected.len());
    if let Some(index) = actual.iter().zip(expected).position(|(a,b)| a != b) {
        bail!("검증 불일치: 0x{:06X}, 실제 {:02X}, 기대 {:02X}", offset + index, actual[index], expected[index]);
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Connect,
    Dump,
    Read { region: Region },
    Write { image: PathBuf, start: usize, expected_sha256: String },
    Erase { region: Region },
    Verify { image: PathBuf, start: usize, expected_sha256: String },
}

impl Action {
    pub fn name(&self) -> &'static str {
        match self { Self::Connect => "연결 / 칩 식별", Self::Dump => "전체 덤프",
            Self::Read {..} => "범위 읽기", Self::Write {..} => "파일 쓰기",
            Self::Erase {..} => "범위 지우기", Self::Verify {..} => "파일 비교 검증" }
    }
    pub fn destructive(&self) -> bool { matches!(self, Self::Write {..} | Self::Erase {..}) }
}

#[derive(Clone)]
pub struct Request {
    pub device: Device,
    pub hz: u32,
    pub model: Option<String>,
    pub expected: Option<Chip>,
    pub backup: bool,
    pub data_dir: PathBuf,
    pub action: Action,
    pub demo: bool,
}

#[derive(Clone, Debug)]
pub struct ReadData {
    pub source: Device,
    pub chip_name: String,
    pub offset: usize,
    pub data: Arc<Vec<u8>>,
    pub sha256: String,
    pub saved: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    const CAPACITY: usize = 1024 * 1024;

    #[test]
    fn checked_ranges_and_hex_input() {
        assert_eq!(number("0x10_000").unwrap(), 65536);
        assert!(number("-1").is_err());
        assert!(Region::new(0, 0, CAPACITY).is_err());
        assert!(Region::new(usize::MAX, 4, CAPACITY).is_err());
        assert!(Region::new(CAPACITY-1, 2, CAPACITY).is_err());
        assert_eq!(Region::new(CAPACITY-1, 1, CAPACITY).unwrap().end(), CAPACITY);
    }

    #[test]
    fn partial_program_preserves_sector_neighbors_and_tail() {
        let original: Vec<_> = (0..CAPACITY).map(|i| (i % 251) as u8).collect();
        let image = vec![0xa5; 64468];
        let start = 4093;
        let target = compose_write(&original, &image, start).unwrap();
        assert_eq!(&target[..start], &original[..start]);
        assert_eq!(&target[start..start+image.len()], &image);
        assert_eq!(&target[start+image.len()..], &original[start+image.len()..]);
        assert!(compose_write(&original, &image, CAPACITY-4).is_err());
        assert!(compose_write(&original, &[], 0).is_err());
    }

    #[test]
    fn erase_changes_only_requested_bytes_without_fixed_sector_assumptions() {
        let original = vec![0x5a; CAPACITY];
        for value in [0, 255] {
            let target = compose_erase(&original, Region { start: 4093, len: 101 }, value).unwrap();
            assert_eq!(&target[..4093], &original[..4093]);
            assert!(target[4093..4194].iter().all(|b| *b == value));
            assert_eq!(&target[4194..], &original[4194..]);
            assert!(equal(&target, &original, 0).unwrap_err().to_string().contains("0x000FFD"));
        }
    }
}
