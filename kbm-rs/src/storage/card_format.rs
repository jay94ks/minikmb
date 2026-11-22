//! Irreversible whole-card format. The acceptance packet is sent once; follow-up
//! reads are pinned to its original management session and can never cancel it.
use super::*;

#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Info {
    pub uid: Uid,
    pub job_id: Option<u32>,
    pub phase: String,
    pub can_cancel: bool,
    pub mcu_job: Option<McuJob>,
}
pub(super) struct Hold {
    pub info: Info,
    session: u64,
}
pub(super) fn pending() -> AppError {
    AppError::new(
        "CARD_FORMAT_PENDING",
        "전체 SD 카드 포맷의 종료를 먼저 확인하세요. 수락된 포맷은 취소할 수 없습니다.",
    )
}
fn uncertain() -> AppError {
    AppError::uncertain("CARD_FORMAT_UNCONFIRMED", "전체 카드 포맷의 결과를 확인하지 못했습니다. 원래 채널에서 포맷 상태 확인을 실행하세요. 포맷 명령을 다시 보내지 마세요.")
}
impl Manager {
    pub fn card_format_info(&self) -> Option<Info> {
        self.state
            .lock()
            .card_format
            .as_ref()
            .map(|h| h.info.clone())
    }
}
impl Work {
    fn clear_format(&self) {
        let mut s = self.manager.state.lock();
        s.card_format = None;
        s.cache.remove(&self.job.binding.uid);
    }
    fn hold_format(&self, id: Option<u32>, phase: &str) {
        let mut s = self.manager.state.lock();
        let session = s
            .card_format
            .as_ref()
            .filter(|h| h.info.uid == self.job.binding.uid)
            .map(|h| h.session)
            .unwrap_or_else(|| self.wire.session());
        s.card_format = Some(Hold {
            session,
            info: Info {
                uid: self.job.binding.uid.clone(),
                job_id: id,
                phase: phase.into(),
                can_cancel: false,
                mcu_job: None,
            },
        });
        s.cache.remove(&self.job.binding.uid);
    }
    fn format_progress(&self, status: &McuJob) {
        let mut s = self.manager.state.lock();
        if let Some(h) = &mut s.card_format {
            h.info.job_id = Some(status.id);
            h.info.mcu_job = Some(status.clone());
            h.info.phase = "전체 카드 포맷 진행 중".into();
        }
        let mut info = self.job.info.lock();
        info.mcu_job = Some(status.clone());
        info.done = status.done;
        info.total = status.total;
    }
    // Used at app restart/refresh before starting a catalog MCU job. Never
    // interpret metadata BAD_STATE or a cached empty catalog as format completion.
    pub(super) async fn observe_card_format(&self) -> Result<(), AppError> {
        let status = self.command(0, 4, vec![]).await?;
        if status.len() != 64 {
            return Err(abi::invalid());
        }
        if status[13] == 10 {
            let id = abi::u32at(&status, 16);
            self.hold_format((id != 0).then_some(id), "진행 중인 포맷 발견");
            if id == 0 {
                self.hold_format(None, "포맷 작업 ID 확인 필요");
                return Err(uncertain());
            }
            let reply = self.command(0, 7, abi::words(&[id])).await?;
            let job = McuJob::decode(&reply, id, 14)?;
            self.hold_format(Some(id), "진행 중인 포맷 발견");
            self.format_progress(&job);
            return Err(pending());
        }
        Ok(())
    }
    pub(super) async fn format_card(
        &mut self,
        format: u8,
        expected: Option<String>,
    ) -> Result<Value, AppError> {
        self.observe_card_format().await?;
        let card = self.media_card(expected.as_deref()).await?;
        // Only NOT_FOUND proves a closed raw path. BAD_STATE is not sufficient.
        match self.command(3, 0x318, vec![]).await {
            Err(e) if e.status == Some(17) => {}
            Err(e) => return Err(e),
            Ok(_) => {
                return Err(AppError::new(
                    "MEDIA_CONFLICT",
                    "전체 카드 포맷 전에 모든 디스크 경로를 닫으세요.",
                ))
            }
        }
        if let Some(optical) = self.optical_info().await?.filter(|o| o.mounted) {
            self.mcu_job(0x307, 4, abi::words(&[optical.id])).await?;
            self.manager.clear_media_part(false, true);
        }
        self.manager
            .backend
            .prepare_target_change(self.job.binding.clone())
            .await?;
        self.media_card(Some(&card.uuid)).await?;
        self.check()?;
        let prior_effects = self.job.info.lock().side_effects_possible;
        {
            // Same mutex as reserve_switch/cancel/stop: no switch can invalidate
            // generation between this seal and accepting a noncancellable job.
            let mut s = self.manager.state.lock();
            if s.suspended || s.stopped || self.job.cancel.is_cancelled() {
                return Err(AppError::new(
                    "OPERATION_CANCELLED",
                    "포맷 요청 전에 취소됐습니다.",
                ));
            }
            check_owner(&self.job.owner)?;
            let mut info = self.job.info.lock();
            info.can_cancel = false;
            info.side_effects_possible = true;
            info.phase = "전체 카드 포맷 수락 확인 중".into();
            s.card_format = Some(Hold {
                session: self.wire.session(),
                info: Info {
                    uid: self.job.binding.uid.clone(),
                    job_id: None,
                    phase: info.phase.clone(),
                    can_cancel: false,
                    mcu_job: None,
                },
            });
            s.cache.remove(&self.job.binding.uid);
        }
        // Never assign kind14 to Work.mcu: generic cleanup sends SYS_CANCEL.
        let ack = match self.command(3, 0x30f, vec![format, 0, 0, 0]).await {
            Ok(b) => b,
            Err(e) => {
                if !e.uncertain {
                    self.clear_format();
                    self.job.info.lock().side_effects_possible = prior_effects;
                    return Err(e);
                }
                return Err(uncertain());
            }
        };
        if ack.len() != 4 || abi::u32at(&ack, 0) == 0 {
            return Err(uncertain());
        }
        let id = abi::u32at(&ack, 0);
        self.hold_format(Some(id), "전체 카드 포맷 진행 중");
        self.phase("전체 카드 포맷 진행 중 · 취소 불가");
        let deadline = Instant::now() + Duration::from_secs(3600);
        loop {
            if !self.wire.live() || self.manager.state.lock().stopped || Instant::now() >= deadline
            {
                return Err(uncertain());
            }
            // Revoking a key/client after acceptance ends its control authority,
            // not the MCU's format. This narrowly allowed scope is read-only.
            let b = tokio::time::timeout(
                Duration::from_secs(3),
                self.wire.cleanup(Cleanup::CardFormat(id), 7),
            )
            .await
            .map_err(|_| uncertain())?
            .map_err(|_| uncertain())?;
            let status = McuJob::decode(&b, id, 14).map_err(|_| uncertain())?;
            self.format_progress(&status);
            if status.state != 0 {
                self.clear_format();
                if status.state != 1 || status.result != 0 {
                    let mut error = AppError::new(
                        "MCU_JOB_FAILED",
                        "MCU가 전체 카드 포맷 실패를 보고했습니다.",
                    );
                    error.status = Some(status.result);
                    return Err(error);
                }
                let card = Card::decode(
                    &self.wire.cleanup(Cleanup::CardFormat(id), 0x308).await?,
                    &self.job.binding.uid,
                )?;
                let name =
                    abi::name_result(&self.wire.cleanup(Cleanup::CardFormat(id), 0x31e).await?)?;
                if !card.present
                    || !card.initialized
                    || card.profile_status != 0
                    || card.stored_uid.as_ref() != Some(&card.current_uid)
                    || card.user_length != 0
                    || !name.is_empty()
                {
                    return Err(AppError::new(
                        "VERIFY_FAILED",
                        "포맷 후 내부 보드 정보·채널 이름·사용자 값 초기화를 확인하지 못했습니다.",
                    ));
                }
                self.wire.verified_name("")?;
                self.cache_card(&card);
                return Ok(json!({"formatted":true,"format":format,"verified":true,"card":card}));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    pub(super) async fn card_format_status(&self) -> Result<Value, AppError> {
        self.phase("전체 카드 포맷 상태 확인");
        let status = self.command(0, 4, vec![]).await?;
        if status.len() != 64 {
            return Err(abi::invalid());
        }
        let formatting = status[13] == 10;
        let (old_id, old_session) = self
            .manager
            .state
            .lock()
            .card_format
            .as_ref()
            .map(|h| (h.info.job_id, h.session))
            .unwrap_or((None, self.wire.session()));
        let id = if formatting {
            Some(abi::u32at(&status, 16)).filter(|n| *n != 0)
        } else {
            old_id
        };
        let Some(id) = id else {
            if formatting {
                self.hold_format(None, "포맷 작업 ID 확인 필요");
                return Err(uncertain());
            }
            let had_hold = self.manager.card_format_info().is_some();
            self.clear_format();
            return Ok(json!({"formatting":false,"outcome":if had_hold {"unknown"} else {"idle"}}));
        };
        if formatting && self.manager.card_format_info().is_none() {
            self.hold_format(Some(id), "진행 중인 포맷 발견");
        }
        let response = self.command(0, 7, abi::words(&[id])).await;
        let status = match response {
            Ok(b) => McuJob::decode(&b, id, 14),
            Err(e) if e.status == Some(17) && !formatting => {
                self.clear_format();
                return Ok(json!({"formatting":false,"outcome":"unknown"}));
            }
            Err(e) => return Err(e),
        }?;
        if status.state == 0 {
            self.hold_format(Some(id), "전체 카드 포맷 진행 중");
            self.format_progress(&status);
            return Ok(json!({"formatting":true,"job":status,"can_cancel":false}));
        }
        self.clear_format();
        // This is an observed terminal state. Do not upgrade the original failed
        // app job to success, especially after session change/reboot/job-ID reuse.
        let card = self.card().await?;
        let name = abi::name_result(&self.command(3, 0x31e, vec![]).await?)?;
        self.wire.verified_name(&name)?;
        Ok(
            json!({"formatting":false,"outcome":"observed_terminal","same_session":old_session == self.wire.session(),"job":status,"card":card,"name":name}),
        )
    }
}
