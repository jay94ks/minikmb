use super::*;
use minikmb_native::{storage::media::Path, storage_wire::Image};

impl Storage {
    pub(super) fn media_controls(
        &mut self,
        ui: &mut egui::Ui,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        snapshot: &Snapshot,
        item: &Image,
        busy: bool,
    ) {
        let expected_card_uuid = snapshot.card.as_ref().map(|c| c.uuid.clone());
        let ready = !busy && !snapshot.stale && snapshot.card.as_ref().is_some_and(|c| c.present);
        ui.add_space(8.0);
        if item.kind == "optical" {
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new(if item.mounted {
                        "광학 매체 분리"
                    } else {
                        "타깃에 연결"
                    }),
                )
                .clicked()
            {
                self.start(
                    app,
                    runtime,
                    Action::OpticalMount {
                        id: item.id,
                        mounted: !item.mounted,
                        expected_card_uuid,
                    },
                );
            }
            return;
        }
        let disk = snapshot.disk.as_ref().filter(|d| d.id == item.id);
        ui.add_enabled_ui(ready, |ui| {
            if let Some(disk) = disk {
                ui.label(match disk.state {
                    4 => "타깃 PC에 연결됨",
                    5 => "관리 PC에 연결됨",
                    _ => "관리 명령으로 직접 접근 중",
                });
                ui.horizontal(|ui| {
                    if ui.button("경로 닫기").clicked() {
                        self.start(app, runtime, Action::DiskClose { id: item.id });
                    }
                    if disk.state == 3 && ui.button("동기화").clicked() {
                        self.start(app, runtime, Action::DiskSync { id: item.id });
                    }
                });
                if disk.state == 3 {
                    ui.separator();
                    theme::group_label(ui, "섹터 직접 접근");
                    ui.horizontal(|ui| {
                        ui.label("LBA");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.sector_lba)
                                .desired_width(110.0)
                                .char_limit(10),
                        );
                        if ui.button("읽기").clicked() {
                            match sector_lba(&self.sector_lba, disk.sectors) {
                                Ok(lba) => self.start(
                                    app,
                                    runtime,
                                    Action::DiskRead {
                                        id: item.id,
                                        lba,
                                        expected_card_uuid: expected_card_uuid.clone(),
                                    },
                                ),
                                Err(e) => self.view.lock().error = e.message,
                            }
                        }
                    });
                    ui.label(RichText::new("512바이트 HEX").small().color(color::MUTED));
                    egui::ScrollArea::vertical()
                        .id_salt("sector-hex")
                        .max_height(110.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.sector_hex)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(5)
                                    .char_limit(2048),
                            );
                        });
                    if ui
                        .add_enabled(disk.writable, egui::Button::new("섹터 쓰기…"))
                        .clicked()
                    {
                        match sector_write(
                            item.id,
                            &self.sector_lba,
                            &self.sector_hex,
                            disk.sectors,
                            disk.lease,
                            expected_card_uuid.clone(),
                        ) {
                            Ok(action) => self.confirm_write = Some(action),
                            Err(e) => self.view.lock().error = e.message,
                        }
                    }
                }
            } else {
                if ui
                    .add_enabled(snapshot.disk.is_none(), egui::Button::new("이미지 포맷…"))
                    .clicked()
                {
                    self.format_dialog = Some(super::format::Dialog::new(
                        item.format,
                        Some(item.id),
                        item.size,
                        expected_card_uuid.clone(),
                    ));
                }
                if ui.button("이미지 파일로 교체…").clicked() {
                    self.import_format = item.format;
                    self.pick_upload(
                        app,
                        runtime,
                        super::upload::Kind::Disk {
                            replace_id: Some(item.id),
                        },
                    );
                }
                ui.horizontal(|ui| {
                    ui.label("연결 경로");
                    egui::ComboBox::from_id_salt("disk-path")
                        .selected_text(match self.disk_path {
                            Path::Target => "타깃 PC",
                            Path::Management => "관리 PC",
                            Path::Direct => "관리 명령 (마운트 없음)",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.disk_path, Path::Target, "타깃 PC");
                            ui.selectable_value(&mut self.disk_path, Path::Management, "관리 PC");
                            ui.selectable_value(
                                &mut self.disk_path,
                                Path::Direct,
                                "관리 명령 (마운트 없음)",
                            );
                        });
                });
                ui.checkbox(&mut self.disk_writable, "읽기/쓰기 허용");
                if ui
                    .add_enabled(snapshot.disk.is_none(), egui::Button::new("디스크 연결"))
                    .clicked()
                {
                    self.start(
                        app,
                        runtime,
                        Action::DiskOpen {
                            id: item.id,
                            path: self.disk_path,
                            writable: self.disk_writable,
                            expected_card_uuid,
                        },
                    );
                }
                if snapshot.disk.is_some() {
                    ui.label("현재 열린 디스크의 경로를 먼저 닫으세요.");
                }
            }
        });
    }
    pub(super) fn write_confirmation(
        &mut self,
        ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        busy: bool,
    ) {
        let Some(action) = self.confirm_write.clone() else {
            return;
        };
        let Action::DiskWrite { id, lba, .. } = action else {
            return;
        };
        let mut decision = None;
        egui::Modal::new(egui::Id::new("sector-write-confirm")).show(ctx, |ui| {
            theme::heading(ui, "섹터 쓰기 확인", "");
            ui.label(format!(
                "이미지 {id}의 LBA {lba}에 512바이트를 쓰고 동기화합니다."
            ));
            ui.label("파일 시스템과 기존 파일이 변경될 수 있습니다.");
            ui.horizontal(|ui| {
                if ui.button("취소").clicked() {
                    decision = Some(false);
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("쓰기 실행"))
                    .clicked()
                {
                    decision = Some(true);
                }
            });
        });
        if let Some(confirmed) = decision {
            self.confirm_write = None;
            if confirmed {
                self.start(app, runtime, action);
            }
        }
    }
}
fn sector_lba(text: &str, sectors: u32) -> Result<u32, AppError> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|lba| *lba < sectors)
        .ok_or_else(|| {
            AppError::new(
                "INVALID_ARGUMENTS",
                "LBA는 디스크 섹터 범위의 정수여야 합니다.",
            )
        })
}
fn sector_write(
    id: u32,
    lba: &str,
    text: &str,
    sectors: u32,
    lease: u32,
    uuid: Option<String>,
) -> Result<Action, AppError> {
    let lba = sector_lba(lba, sectors)?;
    let bytes = user_bytes(text, true)?;
    if bytes.len() != 512 {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "섹터 쓰기에는 정확히512바이트의 HEX가 필요합니다.",
        ));
    }
    Ok(Action::DiskWrite {
        id,
        lba,
        bytes,
        confirmed: true,
        expected_lease: Some(lease),
        expected_card_uuid: uuid,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sector_confirmation_freezes_original_bytes_id_lba_and_card() {
        let action =
            sector_write(7, "127", &"ef ".repeat(512), 128, 77, Some("card-A".into())).unwrap();
        let Action::DiskWrite {
            id,
            lba,
            bytes,
            confirmed,
            expected_lease,
            expected_card_uuid,
        } = action
        else {
            panic!()
        };
        assert_eq!((id, lba, bytes.len(), confirmed), (7, 127, 512, true));
        assert!(bytes.iter().all(|b| *b == 0xef));
        assert_eq!(expected_card_uuid.as_deref(), Some("card-A"));
        assert_eq!(expected_lease, Some(77));
        assert!(sector_write(7, "128", &"00".repeat(512), 128, 77, None).is_err());
        assert!(sector_write(7, "-1", &"00".repeat(512), 128, 77, None).is_err());
        assert!(sector_write(7, "0", &"00".repeat(511), 128, 77, None).is_err());
        assert!(sector_write(7, "0", &"zz".repeat(512), 128, 77, None).is_err());
    }
}
