//! Convert approved STM32 flash-only ELF32/BIN files to a bounded flat image.
//! Neither addresses nor executable paths come from a remote caller.
use super::Target;
use crate::model::AppError;
use sha2::{Digest, Sha256};

pub const FLASH_BASE: u32 = 0x0800_0000;
pub const FLASH_BYTES: usize = 2 * 1024 * 1024;
pub const SPI_MAX_BYTES: usize = 64 * 1024 * 1024;
pub struct Prepared {
    pub bytes: Vec<u8>,
    pub sha256: String,
    pub base: u32,
}

fn invalid(message: &str) -> AppError {
    AppError::new("INVALID_FIRMWARE", message)
}
fn word(bytes: &[u8], offset: usize) -> Result<u32, AppError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| invalid("펌웨어 헤더가 잘렸습니다."))?;
    Ok(u32::from_le_bytes(value.try_into().unwrap()))
}
fn half(bytes: &[u8], offset: usize) -> Result<u16, AppError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| invalid("펌웨어 헤더가 잘렸습니다."))?;
    Ok(u16::from_le_bytes(value.try_into().unwrap()))
}

pub fn prepare(target: Target, source: &[u8]) -> Result<Prepared, AppError> {
    let (base, bytes) = match target {
        Target::Ms2130 => {
            if source.is_empty() || source.len() > SPI_MAX_BYTES {
                return Err(invalid("MS2130 ROM 크기가 지원 범위를 벗어났습니다."));
            }
            (0, source.to_vec())
        }
        Target::Stm32 => {
            let bytes = if source.starts_with(b"\x7fELF") {
                elf(source)?
            } else {
                if source.len() > FLASH_BYTES {
                    return Err(invalid("STM32 펌웨어가 내부 Flash 크기를 초과합니다."));
                }
                source.to_vec()
            };
            vectors(&bytes)?;
            (FLASH_BASE, bytes)
        }
    };
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    Ok(Prepared {
        base,
        bytes,
        sha256,
    })
}

fn vectors(bytes: &[u8]) -> Result<(), AppError> {
    let stack = word(bytes, 0)?;
    let reset = word(bytes, 4)?;
    let ram = [
        (0x0000_0000, 0x0001_0000),
        (0x2000_0000, 0x2002_0000),
        (0x2400_0000, 0x2408_0000),
        (0x3000_0000, 0x3004_8000),
        (0x3800_0000, 0x3801_0000),
    ];
    if stack % 8 != 0
        || !ram
            .iter()
            .any(|(start, end)| stack > *start && stack <= *end)
        || reset & 1 == 0
        || (reset & !1) < FLASH_BASE
        || u64::from(reset & !1) >= u64::from(FLASH_BASE) + bytes.len() as u64
    {
        return Err(invalid(
            "STM32H743 벡터의 스택/Reset 주소가 올바르지 않습니다.",
        ));
    }
    Ok(())
}

fn elf(source: &[u8]) -> Result<Vec<u8>, AppError> {
    if source.len() < 52
        || source.len() > 16 * 1024 * 1024
        || source[4..7] != [1, 1, 1]
        || half(source, 16)? != 2
        || half(source, 18)? != 40
        || word(source, 20)? != 1
        || half(source, 42)? != 32
    {
        return Err(invalid("ARM little-endian ELF32 실행 파일이 필요합니다."));
    }
    let offset = word(source, 28)? as usize;
    let count = usize::from(half(source, 44)?);
    if !(1..=64).contains(&count)
        || offset
            .checked_add(count * 32)
            .is_none_or(|end| end > source.len())
    {
        return Err(invalid("ELF 프로그램 헤더 범위가 올바르지 않습니다."));
    }
    let mut segments = Vec::new();
    for n in 0..count {
        let at = offset + n * 32;
        if word(source, at)? != 1 {
            continue;
        }
        let size = word(source, at + 16)? as usize;
        if size == 0 {
            continue;
        }
        let start = word(source, at + 12)?;
        let input = word(source, at + 4)? as usize;
        if size > word(source, at + 20)? as usize
            || start < FLASH_BASE
            || u64::from(start) + size as u64 > u64::from(FLASH_BASE) + FLASH_BYTES as u64
            || input.checked_add(size).is_none_or(|end| end > source.len())
        {
            return Err(invalid(
                "ELF에 내부 Flash 밖의 기록 구간 또는 잘못된 데이터가 있습니다.",
            ));
        }
        segments.push((start, input, size));
    }
    segments.sort_by_key(|s| s.0);
    if segments.first().is_none_or(|s| s.0 != FLASH_BASE) {
        return Err(invalid("Flash 시작 주소의 벡터 테이블이 없습니다."));
    }
    let mut end = FLASH_BASE as u64;
    for (start, _, size) in &segments {
        if u64::from(*start) < end {
            return Err(invalid("ELF 기록 구간이 중복됩니다."));
        }
        end = u64::from(*start) + *size as u64;
    }
    let entry = word(source, 24)? & !1;
    if entry < FLASH_BASE || u64::from(entry) >= end {
        return Err(invalid("ELF 진입 주소가 펌웨어 밖에 있습니다."));
    }
    let mut bytes = vec![0xff; (end - u64::from(FLASH_BASE)) as usize];
    for (start, input, size) in segments {
        let out = (start - FLASH_BASE) as usize;
        bytes[out..out + size].copy_from_slice(&source[input..input + size]);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binary() -> Vec<u8> {
        let mut v = vec![0x56; 512];
        v[..4].copy_from_slice(&0x2002_0000u32.to_le_bytes());
        v[4..8].copy_from_slice(&(FLASH_BASE + 0x101).to_le_bytes());
        v
    }
    fn executable() -> Vec<u8> {
        let mut e = vec![0; 128 + 512 + 32];
        e[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        for (at, value) in [(16, 2u16), (18, 40), (42, 32), (44, 2)] {
            e[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (at, value) in [(20, 1u32), (24, FLASH_BASE + 0x101), (28, 52)] {
            e[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (header, start, input, size) in [
            (52, FLASH_BASE, 128u32, 512u32),
            (84, FLASH_BASE + 1024, 640, 32),
        ] {
            for (delta, value) in [
                (0, 1u32),
                (4, input),
                (8, start),
                (12, start),
                (16, size),
                (20, size),
            ] {
                e[header + delta..header + delta + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        e[128..640].copy_from_slice(&binary());
        e[640..].fill(0x9a);
        e
    }
    #[test]
    fn firmware_vectors_and_elf_segments_are_bounded_to_internal_flash() {
        let prepared = prepare(Target::Stm32, &executable()).unwrap();
        assert_eq!(prepared.base, FLASH_BASE);
        assert_eq!(prepared.bytes.len(), 1056);
        assert_eq!(&prepared.bytes[..512], &binary());
        assert!(prepared.bytes[512..1024].iter().all(|b| *b == 0xff));
        for (at, value) in [
            (64, 0x2400_0000),
            (96, FLASH_BASE + 16),
            (28, u32::MAX),
            (24, 0x2000_0000),
        ] {
            let mut invalid = executable();
            invalid[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(prepare(Target::Stm32, &invalid).is_err(), "{at}");
        }
    }
    #[test]
    fn corrupt_vectors_and_empty_or_oversize_images_are_rejected() {
        assert!(prepare(Target::Stm32, &binary()).is_ok());
        for (at, value) in [
            (0, 0u32),
            (0, 0x2000_0001),
            (4, FLASH_BASE + 256),
            (4, FLASH_BASE + 1025),
        ] {
            let mut invalid = binary();
            invalid[at..at + 4].copy_from_slice(&value.to_le_bytes());
            assert!(prepare(Target::Stm32, &invalid).is_err());
        }
        assert!(prepare(Target::Stm32, &[]).is_err());
        assert!(prepare(Target::Stm32, &vec![0; FLASH_BYTES + 1]).is_err());
        assert!(prepare(Target::Ms2130, &[]).is_err());
        assert_eq!(
            prepare(Target::Ms2130, &[0xaa; 31]).unwrap().bytes,
            vec![0xaa; 31]
        );
    }
}
