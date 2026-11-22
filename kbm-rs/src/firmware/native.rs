use super::{
    backend::{Backend, Control, Inspection, Notify, Programmed, Programmer},
    image::Prepared,
    ms2130,
    process::{Mode, Tools},
    stm32, Target,
};
use crate::model::AppError;
use futures_util::future::BoxFuture;
use std::{ffi::OsString, sync::Arc};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Native {
    pub tools: Tools,
}
impl Default for Native {
    fn default() -> Self {
        Self {
            tools: Tools::bundled(),
        }
    }
}
fn readonly(cancel: CancellationToken) -> Control {
    Control {
        cancel,
        commit: Arc::new(|| Err(AppError::new("WRITE_NOT_ALLOWED", "읽기 전용 작업입니다."))),
    }
}
fn quiet() -> Notify {
    Arc::new(|_| {})
}
fn io_error() -> AppError {
    AppError::new(
        "FIRMWARE_IO_FAILED",
        "펌웨어 작업 파일을 준비하지 못했습니다.",
    )
}

impl Native {
    fn inspect_sync(
        &self,
        target: Target,
        id: &str,
        cancel: CancellationToken,
    ) -> Result<Inspection, AppError> {
        let control = readonly(cancel);
        match target {
            Target::Stm32 => {
                let list = self.tools.run(
                    "swd",
                    &[OsString::from("list")],
                    Mode::Read,
                    &control,
                    &quiet(),
                )?;
                stm32::selected(&stm32::probes(&list.text)?, id)?;
                let flash = self.tools.run(
                    "swd",
                    &stm32::read_capacity_args(id)?,
                    Mode::Read,
                    &control,
                    &quiet(),
                )?;
                Ok(Inspection {
                    target,
                    programmer_id: id.into(),
                    uid: None,
                    capacity: stm32::capacity(&flash.text)?,
                })
            }
            Target::Ms2130 => {
                let selected = ms2130::selected(&ms2130::writers()?, id)?;
                let data = self.tools.run(
                    "spi",
                    &ms2130::probe_args(&selected),
                    Mode::Read,
                    &control,
                    &quiet(),
                )?;
                let last = data.last.ok_or_else(|| {
                    AppError::new(
                        "FLASH_CAPACITY_INVALID",
                        "플래시 용량을 확인하지 못했습니다.",
                    )
                })?;
                let capacity = last["chip"]["capacity"].as_u64().unwrap_or(0);
                ms2130::require_capacity(1, capacity)?;
                Ok(Inspection {
                    target,
                    programmer_id: id.into(),
                    uid: None,
                    capacity,
                })
            }
        }
    }
    fn program_sync(
        &self,
        expected: Inspection,
        image: Prepared,
        control: Control,
        notify: Notify,
    ) -> Result<Programmed, AppError> {
        control.check()?;
        let validated = super::image::prepare(expected.target, &image.bytes)?;
        if validated.sha256 != image.sha256 || validated.base != image.base {
            return Err(AppError::new(
                "FIRMWARE_IMAGE_CHANGED",
                "준비한 펌웨어 내용이 변경됐습니다.",
            ));
        }
        let actual = self.inspect_sync(
            expected.target,
            &expected.programmer_id,
            control.cancel.clone(),
        )?;
        if actual.uid != expected.uid || actual.capacity != expected.capacity {
            return Err(AppError::new(
                "PROGRAMMER_TARGET_CHANGED",
                "플래싱 대상/용량이 바뀌었습니다. 기록하지 않았습니다.",
            ));
        }
        ms2130::require_capacity(image.bytes.len() as u64, actual.capacity)?;
        let work = tempfile::tempdir().map_err(|_| io_error())?;
        let file = super::process::staged_binary(work.path(), &image.bytes)?;
        let bytes = image.bytes.len() as u64;
        match actual.target {
            Target::Stm32 => {
                self.tools.run(
                    "swd",
                    &stm32::download_args(&actual.programmer_id, &file)?,
                    Mode::Stm32Write,
                    &control,
                    &notify,
                )?;
                Ok(Programmed {
                    target: actual.target,
                    programmer_id: actual.programmer_id,
                    bytes,
                    verified_bytes: bytes,
                    verified: true,
                    reset: true,
                    tail_preserved: true,
                })
            }
            Target::Ms2130 => {
                let selected = ms2130::selected(&ms2130::writers()?, &actual.programmer_id)?;
                let result = self.tools.run(
                    "spi",
                    &ms2130::write_args(&selected, actual.capacity, &file),
                    Mode::SpiWrite,
                    &control,
                    &notify,
                )?;
                let final_result = result.last.ok_or_else(|| {
                    AppError::uncertain(
                        "FLASH_VERIFY_FAILED",
                        "ROM 기록/검증 완료 응답이 없습니다.",
                    )
                })?;
                if final_result["event"] != "completed"
                    || final_result["verified"] != true
                    || final_result["tail_preserved"] != true
                    || final_result["image_bytes"] != bytes
                    || final_result["verified_bytes"] != actual.capacity
                {
                    return Err(AppError::uncertain(
                        "FLASH_VERIFY_FAILED",
                        "ROM 전체 비교 검증을 확인하지 못했습니다.",
                    ));
                }
                Ok(Programmed {
                    target: actual.target,
                    programmer_id: actual.programmer_id,
                    bytes,
                    verified_bytes: actual.capacity,
                    verified: true,
                    reset: false,
                    tail_preserved: true,
                })
            }
        }
    }
}
impl Backend for Native {
    fn list(
        &self,
        target: Target,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<Vec<Programmer>, AppError>> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let control = readonly(cancel);
                control.check()?;
                match target {
                    Target::Stm32 => {
                        let list = this.tools.run(
                            "swd",
                            &[OsString::from("list")],
                            Mode::Read,
                            &control,
                            &quiet(),
                        )?;
                        Ok(stm32::probes(&list.text)?
                            .into_iter()
                            .map(|p| Programmer {
                                id: p.id,
                                label: p.label,
                                target,
                                selectable: p.selectable,
                            })
                            .collect())
                    }
                    Target::Ms2130 => Ok(ms2130::writers()?
                        .into_iter()
                        .map(|w| Programmer {
                            id: w.id.clone(),
                            label: format!("RP2040 · {} · {}", w.port, w.id),
                            target,
                            selectable: true,
                        })
                        .collect()),
                }
            })
            .await
            .map_err(|_| {
                AppError::new("PROGRAMMER_FAILED", "프로그래머 검색 작업에 실패했습니다.")
            })?
        })
    }
    fn inspect(
        &self,
        target: Target,
        programmer: String,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<Inspection, AppError>> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.inspect_sync(target, &programmer, cancel))
                .await
                .map_err(|_| {
                    AppError::new("PROGRAMMER_FAILED", "프로그래머 확인 작업에 실패했습니다.")
                })?
        })
    }
    fn program(
        &self,
        expected: Inspection,
        image: Prepared,
        control: Control,
        notify: Notify,
    ) -> BoxFuture<'static, Result<Programmed, AppError>> {
        let this = self.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || this.program_sync(expected, image, control, notify))
                .await
                .map_err(|_| {
                    AppError::uncertain(
                        "PROGRAMMER_FAILED",
                        "플래시 작업 완료를 확인하지 못했습니다.",
                    )
                })?
        })
    }
}
