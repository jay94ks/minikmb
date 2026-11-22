use super::*;
pub(super) struct Dialog {
    id: Option<u32>,
    format: u8,
    size_mib: u32,
    bytes: u64,
    card_uuid: Option<String>,
}
impl Dialog {
    pub(super) fn new(format: u8, id: Option<u32>, bytes: u64, card_uuid: Option<String>) -> Self {
        Self {
            id,
            format,
            size_mib: 64,
            bytes,
            card_uuid,
        }
    }
    fn action(&self) -> Action {
        if let Some(id) = self.id {
            Action::DiskFormat {
                id,
                format: self.format,
                confirmed: true,
                expected_size: Some(self.bytes),
                expected_card_uuid: self.card_uuid.clone(),
            }
        } else {
            Action::DiskCreate {
                format: self.format,
                size_mib: self.size_mib,
                confirmed: true,
                expected_card_uuid: self.card_uuid.clone(),
            }
        }
    }
    fn request(&self) -> minikmb_native::storage::format::Request {
        minikmb_native::storage::format::Request {
            format: self.format,
            bytes: if self.id.is_some() {
                self.bytes
            } else {
                u64::from(self.size_mib) * 1048576
            },
            existing: self.id.is_some(),
        }
    }
}
impl Storage {
    pub(super) fn format_confirmation(
        &mut self,
        ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        busy: bool,
    ) {
        let Some(dialog) = &mut self.format_dialog else {
            return;
        };
        let mut decision = None;
        egui::Modal::new(egui::Id::new("format-image-confirm")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            theme::heading(
                ui,
                if dialog.id.is_some() {
                    "이미지 포맷 확인"
                } else {
                    "디스크 이미지 만들기"
                },
                "",
            );
            ui.horizontal(|ui| {
                ui.label("파일 시스템");
                egui::ComboBox::from_id_salt("format-filesystem")
                    .selected_text(format_name(dialog.format))
                    .show_ui(ui, |ui| {
                        for code in 1..=3 {
                            ui.selectable_value(&mut dialog.format, code, format_name(code));
                        }
                    });
            });
            if let Some(id) = dialog.id {
                ui.label(format!("선택한 이미지 {id}의 내용이 지워집니다."));
                ui.label(format!(
                    "용량 유지: {} ({}바이트)",
                    size(dialog.bytes),
                    dialog.bytes
                ));
            } else {
                ui.horizontal(|ui| {
                    ui.label("용량 (MiB)");
                    ui.add(egui::DragValue::new(&mut dialog.size_mib).range(64..=32768));
                });
                ui.label("SD 카드에 새 디스크 이미지 파일을 만듭니다.");
            }
            let valid = dialog.request().validate();
            ui.label("SD에 반영할 때 연결된 광학 매체를 분리합니다.");
            if let Err(e) = &valid {
                ui.colored_label(color::WARNING, &e.message);
            }
            ui.horizontal(|ui| {
                if ui.button("취소").clicked() {
                    decision = Some(false);
                }
                if ui
                    .add_enabled(
                        !busy && valid.is_ok(),
                        egui::Button::new(if dialog.id.is_some() {
                            "포맷 실행"
                        } else {
                            "이미지 생성"
                        }),
                    )
                    .clicked()
                {
                    decision = Some(true);
                }
            });
        });
        if let Some(confirmed) = decision {
            let dialog = self.format_dialog.take().unwrap();
            if confirmed {
                self.start(app, runtime, dialog.action());
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_confirmation_pins_id_exact_capacity_and_card() {
        let d = Dialog::new(2, Some(19), 9 * 1048576 + 512, Some("card-a".into()));
        d.request().validate().unwrap();
        let Action::DiskFormat {
            id,
            format,
            confirmed,
            expected_size,
            expected_card_uuid,
        } = d.action()
        else {
            panic!()
        };
        assert_eq!((id, format, confirmed), (19, 2, true));
        assert_eq!(expected_size, Some(9 * 1048576 + 512));
        assert_eq!(expected_card_uuid.as_deref(), Some("card-a"));
        assert!(Dialog::new(1, Some(19), 9 * 1048576 + 512, None)
            .request()
            .validate()
            .is_err());
    }
}
