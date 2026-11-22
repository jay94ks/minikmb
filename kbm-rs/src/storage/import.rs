//! Import an app-owned staged file. No public command accepts a host filesystem path.
use super::*;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};
use tokio::io::{AsyncReadExt, BufReader};
#[derive(Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Spec {
    Optical {
        name: String,
        size: u64,
        confirmed: bool,
        expected_card_uuid: Option<String>,
    },
    Disk {
        size: u64,
        format: u8,
        replace_id: Option<u32>,
        confirmed: bool,
        expected_card_uuid: Option<String>,
    },
}
impl Spec {
    pub fn size(&self) -> u64 {
        match self {
            Self::Optical { size, .. } | Self::Disk { size, .. } => *size,
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Self::Optical { .. } => "optical_upload",
            Self::Disk { .. } => "disk_upload",
        }
    }
    pub fn validate(&self) -> Result<(), AppError> {
        let valid = match self {
            Self::Optical {
                name,
                size,
                confirmed,
                ..
            } => {
                *confirmed
                    && !name.is_empty()
                    && name.len() <= 120
                    && !name
                        .chars()
                        .any(|c| c.is_control() || c == '/' || c == '\\')
                    && *size > 18 * 2048
                    && size.is_multiple_of(2048)
                    && *size <= (u64::from(u32::MAX) + 1) * 512
            }
            Self::Disk {
                size,
                format,
                replace_id,
                confirmed,
                ..
            } => {
                *confirmed
                    && *size > 0
                    && size.is_multiple_of(512)
                    && *size / 512 <= u64::from(u32::MAX)
                    && [1, 2, 3].contains(format)
                    && replace_id.is_none_or(|id| (1..=0xfffffff).contains(&id))
            }
        };
        if !valid {
            return Err(AppError::new(
                "INVALID_ARGUMENTS",
                "확인 여부·이미지 크기·이름·형식·교체 ID를 확인하세요.",
            ));
        }
        Ok(())
    }
    fn expected_card(&self) -> Option<&str> {
        match self {
            Self::Optical {
                expected_card_uuid, ..
            }
            | Self::Disk {
                expected_card_uuid, ..
            } => expected_card_uuid.as_deref(),
        }
    }
}
pub fn file_error() -> AppError {
    AppError::new(
        "IMAGE_IO_FAILED",
        "준비한 이미지 파일을 읽거나 기록하지 못했습니다.",
    )
}
impl Work {
    fn progress(&self, done: u64, total: u64, meter: Option<&mut transfer::Meter>) {
        let mut info = self.job.info.lock();
        info.done = done;
        info.total = total;
        info.transfer = meter.map(|m| {
            m.advance(done, Instant::now());
            let progress = m.progress(Instant::now());
            *self.job.meter.lock() = Some(m.clone());
            progress
        });
        if info.transfer.is_none() {
            *self.job.meter.lock() = None;
        }
    }
    pub(super) async fn import_image(&mut self, spec: Spec, file: File) -> Result<Value, AppError> {
        self.import_image_prefix(spec, file, None).await
    }
    // The folder builder alone supplies an allocated prefix. Its new tempfile
    // has a proven zero tail; CRC verification still covers the entire image.
    pub(super) async fn import_image_prefix(
        &mut self,
        spec: Spec,
        mut file: File,
        populated: Option<u32>,
    ) -> Result<Value, AppError> {
        spec.validate()?;
        if populated.is_some_and(|n| u64::from(n) * 512 > spec.size()) {
            return Err(file_error());
        }
        self.check()?;
        self.phase("전송 전 이미지 검사");
        let job = self.job.clone();
        let manager = self.manager.clone();
        let wire = self.wire.clone();
        let check_spec = spec.clone();
        let (file, checksum) = tokio::task::spawn_blocking(move || {
            let check = || -> Result<(), AppError> {
                if job.cancel.is_cancelled() {
                    return Err(AppError::new(
                        "OPERATION_CANCELLED",
                        "이미지 검사를 취소했습니다.",
                    ));
                }
                check_owner(&job.owner)?;
                manager.active.validate(&job.binding)?;
                if !wire.live() {
                    return Err(AppError::new(
                        "STALE_SESSION",
                        "이미지 검사 중 MCU 연결이 종료됐습니다.",
                    ));
                }
                Ok(())
            };
            let size = check_spec.size();
            let metadata = file.metadata().map_err(|_| file_error())?;
            if !metadata.is_file() || metadata.len() != size {
                return Err(file_error());
            }
            if matches!(check_spec, Spec::Optical { .. }) {
                optical::inspect(size, |lba| {
                    check()?;
                    let mut b = [0; 2048];
                    file.seek(SeekFrom::Start(lba * 2048))
                        .map_err(|_| file_error())?;
                    file.read_exact(&mut b).map_err(|_| file_error())?;
                    Ok(b)
                })?;
            }
            file.seek(SeekFrom::Start(0)).map_err(|_| file_error())?;
            let mut checksum = crc32fast::Hasher::new();
            let mut block = vec![0; 1024 * 1024];
            let mut done = 0;
            while done < size {
                check()?;
                let count = usize::try_from((size - done).min(block.len() as u64)).unwrap();
                file.read_exact(&mut block[..count])
                    .map_err(|_| file_error())?;
                checksum.update(&block[..count]);
                done += count as u64;
                let mut info = job.info.lock();
                info.done = done;
                info.total = size;
            }
            file.seek(SeekFrom::Start(0)).map_err(|_| file_error())?;
            check()?;
            Ok::<_, AppError>((file, checksum.finalize()))
        })
        .await
        .map_err(|_| file_error())??;
        self.check()?;
        let card = self.media_card(spec.expected_card()).await?;
        if self.disk_info().await?.is_some() {
            return Err(AppError::new(
                "MEDIA_CONFLICT",
                "열린 디스크 경로를 먼저 닫으세요.",
            ));
        }
        // Validation is complete before ejecting/replacing any existing media.
        if let Some(image) = self.optical_info().await?.filter(|o| o.mounted) {
            self.mcu_job(0x307, 4, abi::words(&[image.id])).await?;
            self.manager.clear_media_part(false, true);
        }
        self.manager
            .backend
            .prepare_target_change(self.job.binding.clone())
            .await?;
        self.check()?;
        let mut reader = BufReader::with_capacity(1024 * 1024, tokio::fs::File::from_std(file));
        let size = spec.size();
        let result = match spec {
            Spec::Optical { name, .. } => {
                self.phase("광학 이미지 준비");
                self.progress(0, size, None);
                let mut begin = vec![0; 44];
                begin[..8].copy_from_slice(&size.to_le_bytes());
                begin[8..12].copy_from_slice(&checksum.to_le_bytes());
                begin[40] = 1;
                begin[42..44].copy_from_slice(&(name.len() as u16).to_le_bytes());
                begin.extend_from_slice(name.as_bytes());
                let job = self.mcu_job(0x301, 1, begin).await?;
                if job.object != job.id || job.object == 0 {
                    return Err(abi::invalid());
                }
                self.upload = Some(job.object);
                self.phase("광학 이미지 전송");
                let mut meter = transfer::Meter::new(size, Instant::now());
                let mut at = 0;
                while at < size {
                    self.check()?;
                    let count = (size - at).min(1008) as usize;
                    let mut p = abi::words(&[job.object]);
                    p.extend_from_slice(&at.to_le_bytes());
                    p.resize(12 + count, 0);
                    reader
                        .read_exact(&mut p[12..])
                        .await
                        .map_err(|_| file_error())?;
                    let ack = self.command(3, 0x302, p).await?;
                    at += count as u64;
                    if ack.len() != 8 || abi::u64at(&ack, 0) != at {
                        return Err(abi::invalid());
                    }
                    self.progress(at, size, Some(&mut meter));
                }
                self.phase("SD 광학 이미지 검증");
                self.progress(0, size, None);
                let committed = self.mcu_job(0x303, 2, abi::words(&[job.object])).await?;
                self.upload = None;
                let image = self.optical_info().await?.ok_or_else(abi::invalid)?;
                if image.id != committed.object
                    || image.size != size
                    || image.crc != checksum
                    || image.name != name
                    || image.mounted
                {
                    return Err(AppError::new(
                        "VERIFY_FAILED",
                        "기록된 광학 이미지 정보가 원본과 다릅니다.",
                    ));
                }
                json!({"id":image.id,"crc32":checksum,"size":size})
            }
            Spec::Disk {
                format, replace_id, ..
            } => {
                let id = if let Some(id) = replace_id {
                    self.phase("선택 디스크 교체");
                    self.mcu_job(0x31d, 16, abi::words(&[id])).await?;
                    id
                } else {
                    let mut b = [0; 4];
                    getrandom::fill(&mut b)
                        .map_err(|_| AppError::new("RANDOM_FAILED", "이미지 ID 생성 실패"))?;
                    u32::from_le_bytes(b) % 0x0fffffff + 1
                };
                self.claim_media(&card.uuid, Some(media::DiskClaim { id, lease: None }), None)?;
                self.claimed_disk = true;
                self.phase("SD 빈 이미지 생성");
                let mut p = abi::words(&[id, (size / 512) as u32]);
                p.extend_from_slice(&[format, 0, 0, 0]);
                let created = self.mcu_job(0x310, 7, p).await?;
                let disk = self.disk_info().await?.ok_or_else(abi::invalid)?;
                if created.object == 0
                    || created.object != disk.lease
                    || disk.id != id
                    || u64::from(disk.sectors) != size / 512
                    || disk.state != 2
                    || !disk.writable
                    || disk.format != format
                {
                    return Err(abi::invalid());
                }
                self.claim_media(
                    &card.uuid,
                    Some(media::DiskClaim {
                        id,
                        lease: Some(disk.lease),
                    }),
                    None,
                )?;
                self.phase("디스크 이미지 기록");
                let mut meter = transfer::Meter::new(size, Instant::now());
                self.progress(0, size, Some(&mut meter));
                let mut block = [0; 512];
                for lba in 0..populated.unwrap_or(disk.sectors) {
                    self.check()?;
                    reader
                        .read_exact(&mut block)
                        .await
                        .map_err(|_| file_error())?;
                    if block.iter().any(|b| *b != 0) {
                        let mut p = abi::words(&[disk.lease, lba]);
                        p.extend_from_slice(&block);
                        self.mcu_job(0x315, 12, p).await?;
                    }
                    self.progress((u64::from(lba) + 1) * 512, size, Some(&mut meter));
                }
                self.phase("SD 디스크 이미지 검증");
                self.progress(0, size, None);
                self.mcu_job(0x311, 8, abi::words(&[disk.lease, checksum]))
                    .await?;
                match self.command(3, 0x318, vec![]).await {
                    Err(e) if e.status == Some(17) => {}
                    Err(e) => return Err(e),
                    Ok(_) => return Err(abi::invalid()),
                };
                self.manager.clear_media_part(true, false);
                self.claimed_disk = false;
                json!({"id":id,"format":format,"size":size,"crc32":checksum})
            }
        };
        self.media_card(Some(&card.uuid)).await?;
        let snapshot = self.refresh().await?;
        Ok(json!({"image":result,"snapshot":snapshot}))
    }
    pub(super) async fn cleanup_upload(&mut self) -> Result<(), AppError> {
        let Some(id) = self.upload else {
            return Ok(());
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut sent = false;
        loop {
            let status = self.wire.cleanup(Cleanup::UploadStatus, 4).await?;
            if status.len() != 64 {
                return Err(abi::invalid());
            }
            let active = abi::u32at(&status, 24);
            if active == 0 && !(2..=4).contains(&status[13]) {
                self.upload = None;
                return Ok(());
            }
            if active != 0 && active != id {
                return Err(AppError::new(
                    "CLEANUP_FAILED",
                    "다른 광학 업로드가 진행 중이어서 정리하지 않았습니다.",
                ));
            }
            if !sent && active == id {
                let ack = self.wire.cleanup(Cleanup::Upload(id), 9).await?;
                if !ack.is_empty() {
                    return Err(abi::invalid());
                }
                sent = true;
            }
            if Instant::now() > deadline {
                return Err(AppError::new(
                    "CLEANUP_FAILED",
                    "광학 업로드 종료를 확인하지 못했습니다.",
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
