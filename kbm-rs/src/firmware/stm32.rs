//! Argument/response contract for the bundled, SHA-pinned probe-rs 0.32.0.
//! No shell, positional probe index, erase-all, unlock or arbitrary address.
use crate::model::{AppError, Uid};
use std::{ffi::OsString, path::Path};

pub const TOOL_SHA256: &str = "11ee6674f3d4584d46aa4e7f3952a5a3f7354091d8438d39d660e8c503fc8262";
pub const CHIP: &str = "STM32H743VI";
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct Probe {
    pub id: String,
    pub label: String,
    pub serial: Option<String>,
    pub selectable: bool,
}
fn invalid(message: &str) -> AppError {
    AppError::new("PROBE_RESPONSE_INVALID", message)
}

pub fn probes(text: &str) -> Result<Vec<Probe>, AppError> {
    if text.len() > 64 * 1024 {
        return Err(invalid("프로브 목록이 너무 큽니다."));
    }
    let mut probes = Vec::new();
    for line in text.lines().map(str::trim) {
        if !line.starts_with('[') {
            continue;
        }
        let Some((label, right)) = line.split_once(" -- ") else {
            return Err(invalid("프로브 목록 형식이 달라졌습니다."));
        };
        if !label.contains("STLink") {
            continue;
        }
        let value = right
            .split_whitespace()
            .next()
            .ok_or_else(|| invalid("프로브 ID가 없습니다."))?;
        let id = value.trim_end_matches(':');
        validate_id(id)?;
        let serial = id.split(':').nth(2).map(str::to_owned);
        probes.push(Probe {
            id: id.into(),
            label: label.split_once(": ").map_or(label, |(_, v)| v).to_owned(),
            serial,
            selectable: true,
        });
        if probes.len() > 32 {
            return Err(invalid("프로브가 너무 많습니다."));
        }
    }
    for i in 0..probes.len() {
        probes[i].selectable = probes.iter().filter(|p| p.id == probes[i].id).count() == 1;
    }
    Ok(probes)
}
fn validate_id(id: &str) -> Result<(), AppError> {
    let parts: Vec<_> = id.split(':').collect();
    if !(2..=3).contains(&parts.len())
        || !parts[0].eq_ignore_ascii_case("0483")
        || parts[1].len() != 4
        || !parts[1].bytes().all(|c| c.is_ascii_hexdigit())
        || parts.get(2).is_some_and(|s| {
            s.is_empty() || s.len() > 128 || !s.bytes().all(|c| c.is_ascii_alphanumeric())
        })
    {
        return Err(invalid("ST-Link의 VID/PID/serial을 확인할 수 없습니다."));
    }
    Ok(())
}
pub fn selected<'a>(current: &'a [Probe], id: &str) -> Result<&'a Probe, AppError> {
    validate_id(id)?;
    let found: Vec<_> = current
        .iter()
        .filter(|p| p.id == id && p.selectable)
        .collect();
    if found.len() != 1 {
        return Err(AppError::new(
            "PROBE_NOT_UNIQUE",
            "선택한 ST-Link를 구분할 수 없습니다. 프로브 목록을 갱신하세요.",
        ));
    }
    Ok(found[0])
}
pub fn uid(text: &str) -> Result<Uid, AppError> {
    let values: Vec<_> = text
        .lines()
        .filter_map(|line| {
            let (address, value) = line.trim().split_once(':')?;
            address.eq_ignore_ascii_case("1ff1e800").then_some(value)
        })
        .collect();
    if values.len() != 1 {
        return Err(invalid("SWD UID 응답을 구분할 수 없습니다."));
    }
    let words: Vec<_> = values[0].split_whitespace().collect();
    if words.len() != 3
        || words
            .iter()
            .any(|s| s.len() != 8 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(invalid("SWD UID 응답 길이가 올바르지 않습니다."));
    }
    let mut bytes = Vec::with_capacity(12);
    for word in words {
        bytes.extend_from_slice(
            &u32::from_str_radix(word, 16)
                .map_err(|_| invalid("SWD UID 형식 오류"))?
                .to_le_bytes(),
        );
    }
    Uid::from_bytes(&bytes).map_err(|_| invalid("SWD UID 형식 오류"))
}
fn connection(id: &str) -> Result<Vec<OsString>, AppError> {
    validate_id(id)?;
    Ok([
        "--chip",
        CHIP,
        "--protocol",
        "swd",
        "--speed",
        "950",
        "--probe",
        id,
        "--non-interactive",
    ]
    .into_iter()
    .map(OsString::from)
    .collect())
}
pub fn read_uid_args(id: &str) -> Result<Vec<OsString>, AppError> {
    let mut args = vec![OsString::from("read")];
    args.extend(connection(id)?);
    args.extend(["b32", "0x1ff1e800", "3"].map(OsString::from));
    Ok(args)
}
pub fn read_capacity_args(id: &str) -> Result<Vec<OsString>, AppError> {
    // STM32H743 CMSIS stm32h743xx.h: FLASHSIZE_BASE, KiB in the low halfword.
    let mut args = vec![OsString::from("read")];
    args.extend(connection(id)?);
    args.extend(["b16", "0x1ff1e880", "1"].map(OsString::from));
    Ok(args)
}
pub fn capacity(text: &str) -> Result<u64, AppError> {
    let mut lines = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("1ff1e880:"));
    let word = lines
        .next()
        .ok_or_else(|| invalid("STM32 플래시 용량 응답이 없습니다."))?
        .trim();
    if lines.next().is_some() || word.len() != 4 || !word.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid("STM32 플래시 용량 응답이 올바르지 않습니다."));
    }
    let bytes =
        u64::from(u16::from_str_radix(word, 16).map_err(|_| invalid("STM32 플래시 용량 오류"))?)
            * 1024;
    if bytes == 0 || bytes > super::image::FLASH_BYTES as u64 {
        return Err(invalid("STM32 플래시 용량이 지원 범위를 벗어났습니다."));
    }
    Ok(bytes)
}
pub fn download_args(id: &str, normalized_binary: &Path) -> Result<Vec<OsString>, AppError> {
    let mut args = vec![OsString::from("download")];
    args.extend(connection(id)?);
    args.extend(
        [
            "--binary-format",
            "bin",
            "--base-address",
            "0x08000000",
            "--verify",
            "--restore-unwritten",
            "--reset",
            "--disable-progressbars",
        ]
        .map(OsString::from),
    );
    args.push(normalized_binary.as_os_str().to_owned());
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_flash_capacity_is_read_instead_of_assuming_the_selected_part_size() {
        assert_eq!(capacity("1ff1e880: 0800\n").unwrap(), 2097152);
        assert_eq!(capacity("1ff1e880: 0400\n").unwrap(), 1048576);
        assert_eq!(
            super::super::ms2130::require_capacity(1500000, capacity("1ff1e880: 0400").unwrap())
                .unwrap_err()
                .code,
            "FLASH_TOO_SMALL"
        );
        for bad in [
            "",
            "1ff1e880: 0000",
            "1ff1e880: ffff",
            "1ff1e880: 0800 1234",
            "1ff1e880: 0800\n1ff1e880: 0800",
        ] {
            assert!(capacity(bad).is_err());
        }
    }
    #[test]
    fn firmware_probes_without_serial_need_a_unique_explicit_match() {
        let one = probes(
            "The following debug probes were found:\n[0]: STLink V2 -- 0483:3748: (ST-LINK)\n",
        )
        .unwrap();
        assert!(one[0].serial.is_none());
        assert!(selected(&one, "0483:3748").is_ok());
        let two = probes(
            "[0]: STLink V2 -- 0483:3748: (ST-LINK)\n[1]: STLink V2 -- 0483:3748: (ST-LINK)\n",
        )
        .unwrap();
        assert!(selected(&two, "0483:3748").is_err());
        for id in [
            "0",
            "0483:3748;rm",
            "0483:3748:--chip-erase",
            "1234:3748",
            "0483",
        ] {
            assert!(read_uid_args(id).is_err());
        }
    }
    #[test]
    fn firmware_swd_uid_matches_management_little_endian_silicon_uid() {
        let actual = uid("WARN speed changed\n1ff1e800: 00400035 35335114 39343632\n").unwrap();
        assert_eq!(actual.display(), "0x350040001451333532363439");
        for value in [
            "1ff1e800: 00400035",
            "1ff1e800: 00400035 35335114 XXXXXXXX",
            "1ff1e800: 1 2 3\n1ff1e800: 1 2 3",
        ] {
            assert!(uid(value).is_err());
        }
    }
    #[test]
    fn firmware_command_plans_never_enable_mass_erase_and_always_verify() {
        let read = read_uid_args("0483:3748:ABC123").unwrap();
        assert!(!read.iter().any(|a| a == "--connect-under-reset"));
        let write = download_args("0483:3748", Path::new("C:/owned staging/firmware.bin")).unwrap();
        // Three-wire SWD/header setups may not carry NRST. Normal attach and a
        // post-verification software reset work; forcing under-reset timed out
        // on the connected board before erase. Never retry this automatically.
        assert!(!write.iter().any(|a| a == "--connect-under-reset"));
        for flag in [
            "--verify",
            "--restore-unwritten",
            "--reset",
            "--non-interactive",
        ] {
            assert!(write.iter().any(|a| a == flag));
        }
        for flag in [
            "--chip-erase",
            "--allow-erase-all",
            "--cycle-power",
            "--dry-run",
        ] {
            assert!(!write.iter().any(|a| a == flag));
        }
        assert_eq!(write.last().unwrap(), "C:/owned staging/firmware.bin");
    }
}
