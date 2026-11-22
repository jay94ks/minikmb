use super::*;
use abi::{Disk, Optical};
#[derive(Clone, Copy, Deserialize, Serialize, schemars::JsonSchema, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Path {
    Direct,
    Target,
    Management,
}
impl Path {
    pub fn code(self) -> u8 {
        match self {
            Self::Direct => 0,
            Self::Target => 1,
            Self::Management => 2,
        }
    }
}
#[derive(Clone)]
pub(super) struct DiskClaim {
    pub(super) id: u32,
    pub(super) lease: Option<u32>,
}
#[derive(Clone)]
pub(super) struct Held {
    uid: Uid,
    card_uuid: String,
    wire: Arc<dyn Wire>,
    disk: Option<DiskClaim>,
    optical: Option<u32>,
}
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ResourceInfo {
    pub uid: Uid,
    pub disk_id: Option<u32>,
    pub lease: Option<u32>,
    pub optical_id: Option<u32>,
    pub connection_lost: bool,
}
fn conflict(message: &str) -> AppError {
    AppError::new("MEDIA_CONFLICT", message)
}
async fn raw_info(wire: &Arc<dyn Wire>, op: u16) -> Result<Option<Vec<u8>>, AppError> {
    match wire.cleanup(Cleanup::MediaInfo, op).await {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.status == Some(17) => Ok(None),
        Err(e) => Err(e),
    }
}
async fn clean_job(
    wire: &Arc<dyn Wire>,
    kind: Cleanup,
    op: u16,
    job_kind: u8,
) -> Result<(), AppError> {
    let b = wire.cleanup(kind, op).await?;
    if b.len() != 4 || abi::u32at(&b, 0) == 0 {
        return Err(abi::invalid());
    }
    let id = abi::u32at(&b, 0);
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let b = wire.cleanup(Cleanup::Job(id), 7).await?;
        let j = McuJob::decode(&b, id, job_kind)?;
        if j.state != 0 {
            return if j.state == 1 && j.result == 0 {
                Ok(())
            } else {
                Err(AppError::new(
                    "MEDIA_CLEANUP_FAILED",
                    format!("매체 정리 실패: 상태 {}, 오류 {}", j.state, j.result),
                ))
            };
        }
        if Instant::now() > deadline {
            return Err(AppError::new(
                "MEDIA_CLEANUP_FAILED",
                "매체 정리 완료를 확인하지 못했습니다.",
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
impl Manager {
    pub(super) fn observe_media_card(&self, uid: &Uid, card: &Card) {
        let stale = self
            .state
            .lock()
            .media
            .as_ref()
            .is_some_and(|m| &m.uid == uid && (!card.present || m.card_uuid != card.uuid));
        if stale {
            self.clear_media_part(true, true);
        }
    }
    pub fn resources_info(&self) -> Option<ResourceInfo> {
        self.state.lock().media.as_ref().map(|m| ResourceInfo {
            uid: m.uid.clone(),
            disk_id: m.disk.as_ref().map(|d| d.id),
            lease: m.disk.as_ref().and_then(|d| d.lease),
            optical_id: m.optical,
            connection_lost: !m.wire.live(),
        })
    }
    pub fn media_error(&self) -> Option<AppError> {
        self.state.lock().media_error.clone()
    }
    pub(super) fn clear_media_part(&self, disk: bool, optical: bool) {
        let mut s = self.state.lock();
        let uid = s.media.as_ref().map(|m| m.uid.clone());
        if let Some(m) = &mut s.media {
            if disk {
                m.disk = None;
            }
            if optical {
                m.optical = None;
            }
            if m.disk.is_none() && m.optical.is_none() {
                s.media = None;
            }
        }
        if let Some(snapshot) = uid.and_then(|uid| s.cache.get_mut(&uid)) {
            if disk {
                snapshot.disk = None;
            }
            if optical {
                if let Some(o) = &mut snapshot.optical {
                    o.mounted = false;
                }
            }
            for image in &mut snapshot.images {
                if (disk && image.kind == "disk") || (optical && image.kind == "optical") {
                    image.mounted = false;
                    image.path = 0;
                    image.writable = false;
                }
            }
        }
        s.media_error = None;
    }
    pub(super) async fn release_media(&self, recover: Option<&Uid>) -> Result<(), AppError> {
        self.release_media_parts(true, true, recover).await
    }
    pub(super) async fn release_media_parts(
        &self,
        disk: bool,
        optical: bool,
        recover: Option<&Uid>,
    ) -> Result<(), AppError> {
        let held = self.state.lock().media.clone();
        let Some(held) = held else {
            return Ok(());
        };
        if !held.wire.live() {
            if recover == Some(&held.uid) {
                return Ok(());
            }
            let error = AppError::new(
                "MEDIA_CLEANUP_FAILED",
                format!(
                    "{}의 원래 관리 연결이 종료됐습니다. 이 채널에 다시 연결해 매체를 분리하세요.",
                    held.uid.display()
                ),
            );
            self.state.lock().media_error = Some(error.clone());
            return Err(error);
        }
        let result = async {
            let card = Card::decode(
                &held.wire.cleanup(Cleanup::MediaInfo, 0x308).await?,
                &held.uid,
            )?;
            if !card.present || card.uuid != held.card_uuid {
                // Removal closes the old card's exports; never eject a newly inserted card.
                self.clear_media_part(disk, optical);
                return Ok(());
            }
            if disk {
                if let Some(claim) = held.disk {
                    let info = raw_info(&held.wire, 0x318)
                        .await?
                        .map(|b| Disk::decode(&b))
                        .transpose()?
                        .flatten();
                    if let Some(info) = info {
                        if info.id != claim.id || claim.lease.is_some_and(|l| l != info.lease) {
                            return Err(conflict("원래 디스크 리스와 현재 경로가 다릅니다."));
                        }
                        clean_job(&held.wire, Cleanup::Disk(info.lease), 0x313, 10).await?;
                        if raw_info(&held.wire, 0x318).await?.is_some() {
                            return Err(conflict("디스크 경로가 아직 열려 있습니다."));
                        }
                    }
                    self.clear_media_part(true, false);
                }
            }
            if optical {
                if let Some(id) = held.optical {
                    let info = raw_info(&held.wire, 0x305)
                        .await?
                        .map(|b| Optical::decode(&b))
                        .transpose()?
                        .flatten();
                    if let Some(info) = info.filter(|o| o.mounted) {
                        if info.id != id {
                            return Err(conflict("원래 광학 이미지와 현재 매체가 다릅니다."));
                        }
                        clean_job(&held.wire, Cleanup::Optical(id), 0x307, 4).await?;
                        let after = raw_info(&held.wire, 0x305)
                            .await?
                            .map(|b| Optical::decode(&b))
                            .transpose()?
                            .flatten();
                        if after.is_some_and(|o| o.mounted) {
                            return Err(conflict("광학 매체가 아직 연결되어 있습니다."));
                        }
                    }
                    self.clear_media_part(false, true);
                }
            }
            Ok(())
        }
        .await;
        if let Err(error) = &result {
            self.state.lock().media_error = Some(error.clone());
        }
        result
    }
}
impl Work {
    pub(super) fn claim_media(
        &mut self,
        card_uuid: &str,
        disk: Option<DiskClaim>,
        optical: Option<u32>,
    ) -> Result<(), AppError> {
        self.check()?;
        let mut s = self.manager.state.lock();
        if let Some(m) = &s.media {
            if m.uid != self.job.binding.uid
                || m.wire.session() != self.wire.session()
                || m.card_uuid != card_uuid
            {
                return Err(conflict("이전 연결의 매체를 먼저 분리하세요."));
            }
        }
        let m = s.media.get_or_insert_with(|| Held {
            uid: self.job.binding.uid.clone(),
            card_uuid: card_uuid.into(),
            wire: self.wire.clone(),
            disk: None,
            optical: None,
        });
        if let Some(d) = disk {
            m.disk = Some(d);
        }
        if let Some(o) = optical {
            m.optical = Some(o);
        }
        s.media_error = None;
        Ok(())
    }
    pub(super) async fn media_card(&self, expected: Option<&str>) -> Result<Card, AppError> {
        let card = self.card().await?;
        if !card.present {
            return Err(AppError::new("CARD_MISSING", "SD 카드가 없습니다."));
        }
        if expected.is_some_and(|uuid| uuid != card.uuid) {
            return Err(AppError::new(
                "CARD_CHANGED",
                "선택한 이미지의 SD 카드가 변경됐습니다.",
            ));
        }
        Ok(card)
    }
    pub(super) async fn disk_info(&self) -> Result<Option<Disk>, AppError> {
        self.optional(0x318)
            .await?
            .map(|b| Disk::decode(&b))
            .transpose()
            .map(Option::flatten)
    }
    pub(super) async fn optical_info(&self) -> Result<Option<Optical>, AppError> {
        self.optional(0x305)
            .await?
            .map(|b| Optical::decode(&b))
            .transpose()
            .map(Option::flatten)
    }
    async fn opened(
        &self,
        id: u32,
        direct: bool,
        writable: bool,
        lba: Option<u32>,
    ) -> Result<Disk, AppError> {
        let disk = self
            .disk_info()
            .await?
            .ok_or_else(|| conflict("먼저 선택한 디스크의 경로를 여세요."))?;
        if disk.id != id
            || direct && disk.state != 3
            || writable && !disk.writable
            || lba.is_some_and(|lba| lba >= disk.sectors)
        {
            return Err(conflict(
                "선택 이미지 ID·직접 경로·쓰기 권한·LBA를 확인하세요.",
            ));
        }
        Ok(disk)
    }
    pub(super) async fn media_snapshot(&self) -> Result<Snapshot, AppError> {
        let card = self.card().await?;
        let optical = self.optical_info().await?;
        let disk = self.disk_info().await?;
        let caps = match self.command(3, 0x319, vec![]).await {
            Ok(bytes) => abi::capabilities(&bytes)?,
            Err(e) if e.status == Some(1) => (0, false, false),
            Err(e) => return Err(e),
        };
        self.check()?;
        let mut s = self.manager.state.lock();
        let mut snapshot = s
            .cache
            .get(&self.job.binding.uid)
            .cloned()
            .ok_or_else(abi::invalid)?;
        snapshot.card = Some(card);
        snapshot.optical = optical;
        snapshot.disk = disk;
        snapshot.capabilities = caps.0;
        snapshot.optical_enabled = caps.1;
        snapshot.optical_applied = caps.2;
        snapshot.observed_at = now_ms();
        snapshot.generation = self.job.binding.generation;
        for image in &mut snapshot.images {
            if image.kind == "optical" {
                image.mounted = snapshot
                    .optical
                    .as_ref()
                    .is_some_and(|o| o.id == image.id && o.mounted);
                image.path = u8::from(image.mounted);
                image.writable = false;
            } else {
                let d = snapshot.disk.as_ref().filter(|d| d.id == image.id);
                image.mounted = d.is_some();
                image.path = d
                    .map(|d| match d.state {
                        4 => 1,
                        5 => 2,
                        _ => 0,
                    })
                    .unwrap_or(0);
                image.writable = d.is_some_and(|d| d.writable);
            }
        }
        if let Some(o) = &snapshot.optical {
            if !snapshot
                .images
                .iter()
                .any(|i| i.kind == "optical" && i.id == o.id)
                && snapshot.images.len() < 1024
            {
                snapshot.images.push(Image {
                    id: o.id,
                    kind: "optical".into(),
                    name: o.name.clone(),
                    size: o.size,
                    format: 0,
                    mounted: o.mounted,
                    path: u8::from(o.mounted),
                    writable: false,
                });
            }
        }
        if let Some(d) = &snapshot.disk {
            if !snapshot
                .images
                .iter()
                .any(|i| i.kind == "disk" && i.id == d.id)
                && snapshot.images.len() < 1024
            {
                snapshot.images.push(Image {
                    id: d.id,
                    kind: "disk".into(),
                    name: format!("D{:07X}.IMG", d.id),
                    size: u64::from(d.sectors) * 512,
                    format: d.format,
                    mounted: true,
                    path: match d.state {
                        4 => 1,
                        5 => 2,
                        _ => 0,
                    },
                    writable: d.writable,
                });
            }
        }
        s.cache.insert(snapshot.uid.clone(), snapshot.clone());
        Ok(snapshot)
    }
    async fn optical_set(&mut self, enabled: bool) -> Result<(), AppError> {
        let caps = abi::capabilities(&self.command(3, 0x319, vec![]).await?)?;
        if caps.0 & 1 == 0 {
            return Err(AppError::new(
                "UNSUPPORTED",
                "현재 MCU는 광학 장치 토글을 지원하지 않습니다.",
            ));
        }
        if caps.1 == enabled && caps.2 == enabled {
            return Ok(());
        }
        if self.disk_info().await?.is_some() {
            return Err(conflict(
                "열린 디스크 경로를 먼저 닫은 뒤 광학 장치 구성을 변경하세요.",
            ));
        }
        if !enabled {
            if let Some(image) = self.optical_info().await?.filter(|o| o.mounted) {
                self.mcu_job(0x307, 4, abi::words(&[image.id])).await?;
                self.manager.clear_media_part(false, true);
            }
        }
        self.check()?;
        self.manager
            .backend
            .prepare_target_change(self.job.binding.clone())
            .await?;
        self.check()?;
        self.job.info.lock().side_effects_possible = true;
        let ack = self.command(3, 0x31a, vec![u8::from(enabled)]).await?;
        if !ack.is_empty() {
            return Err(abi::invalid());
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let caps = abi::capabilities(&self.command(3, 0x319, vec![]).await?)?;
            if caps.1 == enabled && caps.2 == enabled {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(AppError::new(
                    "JOB_TIMEOUT",
                    "광학 장치 구성 적용을 확인하지 못했습니다.",
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    pub(super) async fn media_action(&mut self, action: Action) -> Result<Value, AppError> {
        self.phase("매체 경로 처리");
        let mut sector = None;
        match action {
            Action::OpticalSet { enabled } => {
                self.optical_set(enabled).await?;
            }
            Action::OpticalMount {
                id,
                mounted,
                expected_card_uuid,
            } => {
                let card = self.media_card(expected_card_uuid.as_deref()).await?;
                let image = self
                    .optical_info()
                    .await?
                    .ok_or_else(|| conflict("등록된 광학 이미지가 없습니다."))?;
                if image.id != id {
                    return Err(conflict("선택한 광학 이미지가 현재 이미지와 다릅니다."));
                }
                if mounted {
                    if !abi::capabilities(&self.command(3, 0x319, vec![]).await?)?.2 {
                        self.optical_set(true).await?;
                    }
                    self.claim_media(&card.uuid, None, Some(id))?;
                    self.claimed_optical = !image.mounted;
                }
                if image.mounted != mounted {
                    self.mcu_job(
                        if mounted { 0x306 } else { 0x307 },
                        if mounted { 3 } else { 4 },
                        abi::words(&[id]),
                    )
                    .await?;
                }
                let after = self.optical_info().await?.ok_or_else(abi::invalid)?;
                if after.id != id || after.mounted != mounted {
                    return Err(AppError::new(
                        "VERIFY_FAILED",
                        "광학 매체 연결 상태가 요청과 다릅니다.",
                    ));
                }
                if !mounted {
                    self.manager.clear_media_part(false, true);
                }
                self.media_card(Some(&card.uuid)).await?;
            }
            Action::DiskOpen {
                id,
                path,
                writable,
                expected_card_uuid,
            } => {
                let card = self.media_card(expected_card_uuid.as_deref()).await?;
                if self.disk_info().await?.is_some() {
                    return Err(conflict("기존 디스크 경로를 먼저 닫으세요."));
                }
                self.claim_media(&card.uuid, Some(DiskClaim { id, lease: None }), None)?;
                self.claimed_disk = true;
                let mut payload = abi::words(&[id]);
                payload.extend_from_slice(&[u8::from(writable), path.code(), 0, 0]);
                let job = self.mcu_job(0x312, 9, payload).await?;
                let disk = self.opened(id, false, false, None).await?;
                if disk.writable != writable
                    || disk.state != 3 + path.code()
                    || job.object != disk.lease
                {
                    return Err(AppError::new(
                        "VERIFY_FAILED",
                        "디스크 리스·경로·권한이 요청과 다릅니다.",
                    ));
                }
                self.claim_media(
                    &card.uuid,
                    Some(DiskClaim {
                        id,
                        lease: Some(disk.lease),
                    }),
                    None,
                )?;
                self.media_card(Some(&card.uuid)).await?;
            }
            Action::DiskClose { id } => {
                let disk = self.opened(id, false, false, None).await?;
                self.mcu_job(0x313, 10, abi::words(&[disk.lease])).await?;
                match self.command(3, 0x318, vec![]).await {
                    Err(e) if e.status == Some(17) => {}
                    Err(e) => return Err(e),
                    Ok(_) => {
                        return Err(AppError::new(
                            "VERIFY_FAILED",
                            "디스크 경로가 아직 열려 있습니다.",
                        ))
                    }
                }
                self.manager.clear_media_part(true, false);
            }
            Action::DiskSync { id } => {
                let disk = self.opened(id, true, false, None).await?;
                self.mcu_job(0x317, 13, abi::words(&[disk.lease])).await?;
            }
            Action::DiskRead {
                id,
                lba,
                expected_card_uuid,
            } => {
                let card = self.media_card(expected_card_uuid.as_deref()).await?;
                let disk = self.opened(id, true, false, Some(lba)).await?;
                let job = self
                    .mcu_job(0x314, 11, abi::words(&[disk.lease, lba]))
                    .await?;
                let mut bytes = Vec::with_capacity(512);
                for part in 0..2 {
                    let mut p = abi::words(&[disk.lease, job.id]);
                    p.push(part);
                    let b = self.command(3, 0x316, p).await?;
                    if b.len() != 256 {
                        return Err(abi::invalid());
                    }
                    bytes.extend(b);
                }
                if self.card().await?.uuid != card.uuid {
                    return Err(AppError::new(
                        "CARD_CHANGED",
                        "섹터 읽기 중 카드가 변경됐습니다.",
                    ));
                }
                sector = Some(json!({"id":id,"lba":lba,"bytes":bytes}));
            }
            Action::DiskWrite {
                id,
                lba,
                bytes,
                confirmed: _,
                expected_lease,
                expected_card_uuid,
            } => {
                let card = self.media_card(expected_card_uuid.as_deref()).await?;
                let disk = self.opened(id, true, true, Some(lba)).await?;
                if expected_lease.is_some_and(|lease| lease != disk.lease) {
                    return Err(conflict("쓰기 확인 이후 디스크 리스가 변경됐습니다."));
                }
                let mut p = abi::words(&[disk.lease, lba]);
                p.extend_from_slice(&bytes);
                self.mcu_job(0x315, 12, p).await?;
                self.mcu_job(0x317, 13, abi::words(&[disk.lease])).await?;
                if self.card().await?.uuid != card.uuid {
                    return Err(AppError::new(
                        "CARD_CHANGED",
                        "섹터 쓰기 중 카드가 변경됐습니다.",
                    ));
                }
                sector = Some(json!({"id":id,"lba":lba,"bytes_written":512,"synced":true}));
            }
            _ => unreachable!(),
        }
        let snapshot = self.media_snapshot().await?;
        Ok(json!({"snapshot":snapshot,"sector":sector}))
    }
}
