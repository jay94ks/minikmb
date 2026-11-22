//! The deliberately limited ISO/UDF + El Torito profile used by Electron v1.0.
use crate::model::AppError;
const BLOCK: u64 = 2048;
fn bad(message: &str) -> AppError {
    AppError::new("INVALID_OPTICAL_IMAGE", message)
}
pub fn inspect(
    size: u64,
    mut sector: impl FnMut(u64) -> Result<[u8; 2048], AppError>,
) -> Result<(), AppError> {
    if size <= 18 * BLOCK || !size.is_multiple_of(BLOCK) || size > (u64::from(u32::MAX) + 1) * 512 {
        return Err(bad(
            "2048바이트 단위의 지원 범위 내 부팅 ISO/UDF 이미지가 필요합니다.",
        ));
    }
    let blocks = size / BLOCK;
    let (mut iso, mut end, mut vrs, mut catalog) = (false, false, 0, None);
    for lba in 16..blocks.min(64) {
        let b = sector(lba)?;
        let id = &b[1..6];
        if id == b"CD001" && b[6] == 1 {
            if b[0] == 1 && !end {
                let n = u64::from(u32::from_le_bytes(b[80..84].try_into().unwrap()));
                iso = n > 0
                    && n <= blocks
                    && n == u64::from(u32::from_be_bytes(b[84..88].try_into().unwrap()))
                    && b[128..130] == 2048u16.to_le_bytes()
                    && b[130..132] == 2048u16.to_be_bytes();
            }
            if b[0] == 255 {
                end = true;
            }
            if b[0] == 0
                && &b[7..30] == b"EL TORITO SPECIFICATION"
                && b[30..39].iter().all(|b| *b == 0 || *b == b' ')
            {
                let candidate = u64::from(u32::from_le_bytes(b[71..75].try_into().unwrap()));
                if catalog.is_some_and(|old| old != candidate) {
                    return Err(bad("충돌하는 부팅 카탈로그입니다."));
                }
                catalog = Some(candidate);
            }
        } else if b[0] == 0 && b[6] == 1 {
            match id {
                b"BEA01" => vrs = 1,
                b"NSR02" | b"NSR03" if vrs == 1 => vrs = 2,
                b"TEA01" if vrs == 2 => vrs = 3,
                _ => {}
            }
        }
    }
    let mut udf = false;
    if vrs == 3 {
        for lba in [Some(256), blocks.checked_sub(257), blocks.checked_sub(1)]
            .into_iter()
            .flatten()
            .filter(|l| *l >= 256 && *l < blocks)
        {
            let b = sector(lba)?;
            let sum = b[..16]
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != 4)
                .fold(0u8, |s, (_, v)| s.wrapping_add(*v));
            if b[..2] == 2u16.to_le_bytes()
                && u64::from(u32::from_le_bytes(b[12..16].try_into().unwrap())) == lba
                && sum == b[4]
            {
                udf = true;
            }
        }
    }
    if !(iso && end || udf) {
        return Err(bad("지원하는 ISO 9660/UDF 볼륨 구조가 없습니다."));
    }
    let catalog = catalog
        .filter(|l| *l > 16 && *l < blocks)
        .ok_or_else(|| bad("El Torito 부팅 카탈로그가 없습니다."))?;
    let b = sector(catalog)?;
    let sum = b[..32].chunks_exact(2).fold(0u16, |s, v| {
        s.wrapping_add(u16::from_le_bytes([v[0], v[1]]))
    });
    if b[0] != 1 || b[30..32] != [0x55, 0xaa] || sum != 0 {
        return Err(bad("부팅 카탈로그 검증에 실패했습니다."));
    }
    let check = |at: usize, platform: u8| -> Result<bool, AppError> {
        if b[at] != 0x88 || ![0, 0xef].contains(&platform) || b[at + 1] != 0 {
            return Ok(false);
        }
        let lba = u64::from(u32::from_le_bytes(b[at + 8..at + 12].try_into().unwrap()));
        let count = u64::from(u16::from_le_bytes(b[at + 6..at + 8].try_into().unwrap()));
        if lba == 0
            || lba >= blocks
            || lba * BLOCK + count * 512 > size
            || count == 0 && platform == 0
        {
            return Err(bad("부팅 이미지가 범위를 벗어났습니다."));
        }
        Ok(true)
    };
    let mut bootable = check(32, b[1])?;
    let mut at = 64;
    while at + 32 <= b.len() {
        if b[at..at + 32].iter().all(|n| *n == 0) {
            break;
        }
        let kind = b[at];
        let platform = b[at + 1];
        let count = usize::from(u16::from_le_bytes([b[at + 2], b[at + 3]]));
        if ![0x90, 0x91].contains(&kind) || count == 0 || at + (count + 1) * 32 > b.len() {
            return Err(bad("지원 범위 밖의 부팅 카탈로그입니다."));
        }
        at += 32;
        let limit = at + count * 32;
        while at < limit {
            bootable |= check(at, platform)?;
            at += 32;
        }
        if kind == 0x91 {
            break;
        }
    }
    if !bootable {
        return Err(bad("BIOS/UEFI no-emulation 부팅 항목이 없습니다."));
    }
    Ok(())
}
