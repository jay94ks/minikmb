use super::*;
pub(super) struct Dialog {
    format: u8,
    uuid: Option<String>,
    confirmed: bool,
}
impl Dialog {
    pub(super) fn new(uuid: Option<String>) -> Self {
        Self {
            format: 2,
            uuid,
            confirmed: false,
        }
    }
}
impl Storage {
    pub(super) fn card_format_confirmation(
        &mut self,
        ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        busy: bool,
    ) {
        let Some(dialog) = &mut self.card_format_dialog else {
            return;
        };
        let mut decision = None;
        egui::Modal::new(egui::Id::new("whole-card-format-confirm")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            theme::heading(ui, "SD 카드 전체 포맷", "");
            ui.colored_label(
                color::WARNING,
                "이미지 전체·채널 이름·사용자 값이 삭제됩니다.",
            );
            ui.label("연결된 광학 매체를 분리합니다. 열린 디스크 경로는 먼저 닫아야 합니다.");
            ui.horizontal(|ui| {
                ui.label("파일 시스템");
                egui::ComboBox::from_id_salt("whole-card-filesystem")
                    .selected_text(format_name(dialog.format))
                    .show_ui(ui, |ui| {
                        for code in 1..=2 {
                            ui.selectable_value(&mut dialog.format, code, format_name(code));
                        }
                    });
            });
            if let Some(uuid) = &dialog.uuid {
                ui.label(
                    RichText::new(format!("카드 UUID: {uuid}"))
                        .small()
                        .monospace(),
                );
            }
            ui.label("보드 정보 파일은 펌웨어가 자동으로 다시 만듭니다.");
            ui.label("보드가 요청을 수락한 뒤에는 취소할 수 없습니다. 완료까지 전원을 유지하세요.");
            ui.checkbox(&mut dialog.confirmed, "위 SD 카드의 모든 내용 삭제 확인");
            ui.horizontal(|ui| {
                if ui.button("돌아가기").clicked() {
                    decision = Some(false);
                }
                if ui
                    .add_enabled(
                        !busy && dialog.confirmed,
                        egui::Button::new("전체 포맷 실행"),
                    )
                    .clicked()
                {
                    decision = Some(true);
                }
            });
        });
        if let Some(confirmed) = decision {
            let dialog = self.card_format_dialog.take().unwrap();
            if confirmed {
                self.start(
                    app,
                    runtime,
                    Action::CardFormat {
                        format: dialog.format,
                        confirmed: true,
                        expected_card_uuid: dialog.uuid,
                    },
                );
            }
        }
    }
}
