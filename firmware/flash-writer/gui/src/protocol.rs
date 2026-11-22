use crate::model::Device;
use anyhow::{ensure, Context, Result};
use serialport::{ClearBuffer, SerialPortType};
use std::io::{Read, Write};
use std::time::Duration;

pub fn devices() -> Result<Vec<Device>> {
    let mut result = Vec::new();
    for port in serialport::available_ports()? {
        if let SerialPortType::UsbPort(info) = port.port_type {
            if info.vid == 0x2e8a && info.pid == 0x000a {
                if let Some(serial) = info.serial_number.filter(|s| !s.is_empty()) {
                    result.push(Device { port: port.port_name, serial });
                }
            }
        }
    }
    result.sort_by(|a,b| a.port.cmp(&b.port));
    Ok(result)
}

pub fn identity(device: &Device) -> Result<()> {
    ensure!(devices()?.iter().any(|d| d == device), "선택한 Pico의 포트/UID가 바뀌었거나 연결이 끊겼습니다. 다시 연결하세요.");
    Ok(())
}

pub struct Serprog<T: Read + Write>(T);

pub struct ProbeInfo { pub jedec: String, pub actual_hz: u32, pub sfdp: String }

fn sfdp_description(header: &[u8]) -> String {
    if header.len() >= 8 && &header[..4] == b"SFDP" {
        format!("{}.{} (보조 정보)", header[5], header[4])
    } else { "없음 / 미지원 — 칩 DB 사용".into() }
}

impl<T: Read + Write> Serprog<T> {
    fn exact(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut value = vec![0; len];
        self.0.read_exact(&mut value).context("serprog 응답이 짧거나 제한 시간이 지났습니다.")?;
        Ok(value)
    }
    fn command(&mut self, request: &[u8], len: usize) -> Result<Vec<u8>> {
        self.0.write_all(request)?;
        ensure!(self.exact(1)? == [0x06], "serprog 명령 0x{:02X}가 NAK를 반환했습니다.", request[0]);
        self.exact(len)
    }
    fn spi(&mut self, tx: &[u8], len: usize) -> Result<Vec<u8>> {
        ensure!(tx.len() <= 4096 && len <= 4096, "SPI 프레임 한도를 초과했습니다.");
        let mut request = vec![0x13];
        request.extend_from_slice(&(tx.len() as u32).to_le_bytes()[..3]);
        request.extend_from_slice(&(len as u32).to_le_bytes()[..3]);
        request.extend_from_slice(tx);
        self.command(&request, len)
    }
    fn identify(&mut self, hz: u32) -> Result<ProbeInfo> {
        self.0.write_all(&[0x10])?;
        ensure!(self.exact(2)? == [0x15, 0x06], "serprog 동기화 응답이 올바르지 않습니다.");
        ensure!(self.command(&[0x01], 2)? == [1, 0], "serprog v1이 아닙니다.");
        let name = self.command(&[0x03], 16)?;
        ensure!(name.starts_with(b"frwr\0"), "선택한 장치는 frwr 펌웨어가 아닙니다.");
        let mut frequency = vec![0x14];
        frequency.extend_from_slice(&hz.clamp(100000, 12000000).to_le_bytes());
        let actual_hz = u32::from_le_bytes(self.command(&frequency, 4)?.try_into().unwrap());
        self.command(&[0x15, 1], 0)?;
        // Older SPI chips may lack both 9F and SFDP. Neither is a connection gate.
        let jedec = self.spi(&[0x9f], 3).map(|id| id.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" "))
            .unwrap_or_else(|_| "직접 조회 불가 — 칩 DB 탐색 사용".into());
        let sfdp = self.spi(&[0x5a, 0, 0, 0, 0], 8).map(|header| sfdp_description(&header))
            .unwrap_or_else(|_| "응답 없음 — 칩 DB 사용".into());
        let _ = self.command(&[0x15, 0], 0);
        Ok(ProbeInfo { jedec, actual_hz, sfdp })
    }
}

pub fn probe(device: &Device, hz: u32) -> Result<ProbeInfo> {
    identity(device)?;
    let mut port = serialport::new(&device.port, 115200).timeout(Duration::from_secs(4))
        .open().with_context(|| format!("{} 포트를 열 수 없습니다.", device.port))?;
    port.write_data_terminal_ready(true)?;
    port.write_request_to_send(false)?;
    std::thread::sleep(Duration::from_millis(100));
    port.clear(ClearBuffer::All)?;
    let result = Serprog(&mut *port).identify(hz);
    let _ = port.write_data_terminal_ready(false);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    struct Fragmented { bytes: std::collections::VecDeque<u8>, written: Vec<u8> }
    impl Read for Fragmented {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if let Some(b) = self.bytes.pop_front() { buf[0] = b; Ok(1) } else { Ok(0) }
        }
    }
    impl Write for Fragmented {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let n = buf.len().min(2); self.written.extend_from_slice(&buf[..n]); Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    #[test]
    fn partial_serial_io_preserves_spi_wire_frame() {
        let mut protocol = Serprog(Fragmented { bytes: [6, 0xef, 0x40, 0x14].into(), written: vec![] });
        assert_eq!(protocol.spi(&[0x9f], 3).unwrap(), [0xef, 0x40, 0x14]);
        assert_eq!(protocol.0.written, [0x13, 1, 0, 0, 3, 0, 0, 0x9f]);
    }

    #[test]
    fn nak_and_truncated_response_are_errors() {
        for bytes in [vec![0x15], vec![6, 0xef]] {
            let mut protocol = Serprog(Fragmented { bytes: bytes.into(), written: vec![] });
            assert!(protocol.spi(&[0x9f], 3).is_err());
        }
    }

    #[test]
    fn missing_sfdp_and_non_winbond_id_still_connect() {
        let mut bytes = vec![0x15, 6, 6, 1, 0, 6];
        let mut name = [0; 16]; name[..4].copy_from_slice(b"frwr"); bytes.extend(name);
        bytes.push(6); bytes.extend(1_000_000u32.to_le_bytes()); bytes.push(6);
        bytes.extend([6, 0xbf, 0x25, 0x41]); // SST, not W25Q80
        bytes.push(6); bytes.extend([0xff; 8]); bytes.push(6);
        let info = Serprog(Fragmented { bytes: bytes.into(), written: vec![] }).identify(1_000_000).unwrap();
        assert_eq!(info.jedec, "BF 25 41"); assert!(info.sfdp.contains("없음"));
        assert!(sfdp_description(b"SFDP\x00\x01\x00\xff").contains("1.0"));
        assert!(sfdp_description(&[]).contains("없음"));
    }

    #[test]
    fn unsupported_jedec_and_sfdp_queries_do_not_block_programmer_connection() {
        let mut bytes = vec![0x15, 6, 6, 1, 0, 6];
        let mut name = [0; 16]; name[..4].copy_from_slice(b"frwr"); bytes.extend(name);
        bytes.push(6); bytes.extend(1_000_000u32.to_le_bytes()); bytes.push(6);
        bytes.extend([0x15, 0x15, 6]); // optional 9F, optional SFDP, pin cleanup
        let info = Serprog(Fragmented { bytes: bytes.into(), written: vec![] }).identify(1_000_000).unwrap();
        assert!(info.jedec.contains("조회 불가")); assert!(info.sfdp.contains("응답 없음"));
    }
}
