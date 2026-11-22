//! RP2040 frwr writer selection and the capacity gate before any mutation.
//! The public workflow has no chip-model selection.
use super::image::SPI_MAX_BYTES;
use crate::model::AppError;
use std::{ffi::OsString, path::Path};

#[derive(Clone, Debug, serde::Serialize, schemars::JsonSchema)]
pub struct Writer {
    pub id: String,
    pub port: String,
}
pub fn writers() -> Result<Vec<Writer>, AppError> {
    let ports = serialport::available_ports().map_err(|_| {
        AppError::new(
            "WRITER_DISCOVERY_FAILED",
            "RP2040 Writer 목록을 읽지 못했습니다.",
        )
    })?;
    Ok(ports
        .into_iter()
        .filter_map(|port| {
            let serialport::SerialPortType::UsbPort(info) = port.port_type else {
                return None;
            };
            if info.vid != 0x2e8a || info.pid != 0x000a {
                return None;
            }
            let id = info.serial_number?;
            (!id.is_empty() && id.len() <= 64 && id.bytes().all(|c| c.is_ascii_hexdigit()))
                .then_some(Writer {
                    id,
                    port: port.port_name,
                })
        })
        .collect())
}
pub fn selected(current: &[Writer], id: &str) -> Result<Writer, AppError> {
    let found: Vec<_> = current.iter().filter(|w| w.id == id).collect();
    if found.len() != 1 {
        return Err(AppError::new(
            "WRITER_NOT_UNIQUE",
            "선택한 RP2040 Writer를 구분할 수 없습니다.",
        ));
    }
    let writer = found[0];
    if !writer.port.starts_with("COM")
        || writer.port.len() <= 3
        || writer.port.len() > 12
        || !writer.port[3..].bytes().all(|c| c.is_ascii_digit())
    {
        return Err(AppError::new(
            "WRITER_PORT_INVALID",
            "확인된 Windows Writer 포트가 아닙니다.",
        ));
    }
    Ok(writer.clone())
}
pub fn require_capacity(image_bytes: u64, capacity: u64) -> Result<(), AppError> {
    if capacity == 0 || capacity > SPI_MAX_BYTES as u64 || image_bytes == 0 {
        return Err(AppError::new(
            "FLASH_CAPACITY_INVALID",
            "플래시/ROM 크기를 확인할 수 없습니다.",
        ));
    }
    if image_bytes > capacity {
        return Err(AppError::new(
            "FLASH_TOO_SMALL",
            format!("플래시 용량({capacity}바이트)이 ROM({image_bytes}바이트)보다 작습니다."),
        ));
    }
    Ok(())
}
pub fn probe_args(writer: &Writer) -> Vec<OsString> {
    vec![OsString::from("--probe"), OsString::from(&writer.port)]
}
pub fn write_args(writer: &Writer, capacity: u64, source: &Path) -> Vec<OsString> {
    vec![
        OsString::from("--write"),
        OsString::from(&writer.port),
        OsString::from(capacity.to_string()),
        source.as_os_str().to_owned(),
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn firmware_flash_smaller_than_rom_is_rejected_and_equal_size_is_allowed() {
        assert_eq!(
            require_capacity(64468, 32768).unwrap_err().code,
            "FLASH_TOO_SMALL"
        );
        assert!(require_capacity(64468, 1048576).is_ok());
        assert!(require_capacity(1048576, 1048576).is_ok());
        assert!(require_capacity(64468, 0).is_err());
    }
    #[test]
    fn firmware_writer_serial_never_falls_back_to_an_arbitrary_port() {
        let devices = vec![Writer {
            id: "ABC123".into(),
            port: "COM27".into(),
        }];
        assert!(selected(&devices, "OTHER").is_err());
        let chosen = selected(&devices, "ABC123").unwrap();
        assert_eq!(
            write_args(&chosen, 1048576, Path::new("C:/owned/input.rom")).len(),
            4
        );
        assert!(selected(&[devices[0].clone(), devices[0].clone()], "ABC123").is_err());
        assert!(selected(
            &[Writer {
                id: "ABC123".into(),
                port: "COM27:other".into()
            }],
            "ABC123"
        )
        .is_err());
    }
}
