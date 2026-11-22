use eframe::egui::{self, RichText};
use minikmb_native::{
    input_sessions::Principal,
    model::{AppError, Uid},
    service::AppService,
    storage::{Action, JobState},
    theme::{self, color},
};
use parking_lot::Mutex;
use std::sync::Arc;
#[derive(Default)]
struct ResultView {
    busy: bool,
    finished: bool,
    error: String,
}
pub struct Dialog {
    uid: Uid,
    name: String,
    owner: Principal,
    result: Arc<Mutex<ResultView>>,
    focus: bool,
}
pub fn edit_button(ui: &mut egui::Ui) -> egui::Response {
    use minikmb_native::assets::Icon;
    let response = ui
        .add(
            egui::Button::new(
                RichText::new(Icon::Pencil.glyph().to_string()).font(Icon::font(16.0)),
            )
            .frame(false)
            .min_size(egui::vec2(26.0, 26.0)),
        )
        .on_hover_text("채널 이름 변경");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "채널 이름 변경")
    });
    response
}
pub fn title_label(ui: &mut egui::Ui, title: &str, reserved: f32) -> egui::Response {
    let width = ui
        .painter()
        .layout_no_wrap(title.into(), egui::FontId::proportional(16.0), color::TEXT)
        .size()
        .x
        .min((ui.available_width() - reserved).clamp(60.0, 340.0));
    ui.add_sized(
        [width, 26.0],
        egui::Label::new(RichText::new(title).size(16.0).strong()).truncate(),
    )
}
impl Dialog {
    pub fn new(uid: Uid, name: String) -> Self {
        Self {
            uid,
            name,
            owner: Principal {
                key_id: "ui".into(),
                client_id: "ui:rename".into(),
                cancelled: Default::default(),
                ended: Default::default(),
            },
            result: Default::default(),
            focus: true,
        }
    }
    fn save(&self, app: &Arc<AppService>, runtime: &tokio::runtime::Handle) {
        let name = match minikmb_native::storage_wire::channel_name(&self.name) {
            Ok(s) => s,
            Err(e) => {
                self.result.lock().error = e.message;
                return;
            }
        };
        let app = app.clone();
        let target = self.uid.clone();
        let owner = self.owner.clone();
        let result = self.result.clone();
        result.lock().busy = true;
        runtime.spawn(async move {
            let done = async {
                // Explicit card selection stays on this page; no video opens.
                let binding = app.bind_for_rename(target).await?;
                app.active.validate(&binding)?;
                let job = app.execute_storage(
                    Some(binding),
                    &owner,
                    "storage_action",
                    serde_json::json!(Action::NameSet { name }),
                )?;
                let id = job["job_id"].as_str().ok_or_else(|| {
                    AppError::new("INVALID_RESPONSE", "이름 저장 작업을 확인하지 못했습니다.")
                })?;
                loop {
                    let info = app.storage.job(&owner, id)?;
                    if info.finished_at.is_some() {
                        return if info.state == JobState::Completed {
                            Ok(())
                        } else {
                            Err(info.error.unwrap_or_else(|| {
                                AppError::new(
                                    "NAME_SAVE_FAILED",
                                    "이름 저장을 확인하지 못했습니다.",
                                )
                            }))
                        };
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
            .await;
            let mut v = result.lock();
            v.busy = false;
            match done {
                Ok(()) => {
                    v.finished = true;
                    app.request_rescan();
                }
                Err(e) => v.error = e.message,
            }
        });
    }
    fn select(&self, app: &Arc<AppService>, runtime: &tokio::runtime::Handle) {
        let app = app.clone();
        let uid = self.uid.clone();
        let result = self.result.clone();
        result.lock().busy = true;
        runtime.spawn(async move {
            let response = app.select(Some(uid)).await;
            let mut v = result.lock();
            v.busy = false;
            v.error = response.err().map(|e| e.message).unwrap_or_default();
        });
    }
    /// Returns true when the dialog can close. An accepted name job finishes its
    /// verification even if the initiating page changes.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
    ) -> bool {
        let busy = self.result.lock().busy;
        let mut close = false;
        let mut save = false;
        let other_active = app
            .active
            .state()
            .uid
            .as_ref()
            .is_some_and(|uid| uid != &self.uid);
        let mut select = false;
        theme::tool_window(ctx, "채널 이름 변경", "rename-channel", [420.0, 210.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(self.uid.display())
                        .small()
                        .color(color::MUTED),
                );
                let field = ui.add_enabled(
                    !busy,
                    egui::TextEdit::singleline(&mut self.name)
                        .desired_width(f32::INFINITY)
                        .char_limit(64)
                        .hint_text("채널 이름"),
                );
                if self.focus {
                    field.request_focus();
                    self.focus = false;
                }
                ui.small(
                    "SD 카드에 저장합니다. 최대 UTF-8 64바이트, 빈 이름은 기본 표시로 되돌립니다.",
                );
                if app.active.state().uid.as_ref() != Some(&self.uid) {
                    ui.small("저장할 때 이 채널을 선택합니다. 제어 화면은 열지 않습니다.");
                }
            if other_active {
                ui.label("다른 채널이 사용 중입니다. 전환하면 기존 채널의 영상·디스크·네트워크 연결을 정리합니다.");
                if ui.add_enabled(!busy,egui::Button::new("이 채널 선택")).clicked(){select=true;}
            }
                let v = self.result.lock();
                if !v.error.is_empty() {
                    ui.colored_label(color::ERROR, &v.error);
                }
                if v.finished {
                    theme::status(ui, theme::Tone::Success, "이름 저장·재조회 확인 완료");
                }
                drop(v);
                if busy {
                    ui.spinner();
                    ui.label("SD 저장·확인 중…");
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new("닫기")).clicked() {
                        close = true;
                    }
                if ui.add_enabled(!busy && !other_active, egui::Button::new("저장")).clicked() {
                        save = true;
                    }
                });
            save |= !busy && !other_active && field.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            });
        if save {
            self.result.lock().finished = false;
            self.result.lock().error.clear();
            self.save(app, runtime);
        }
        if select {
            self.select(app, runtime);
        }
        close
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn name_and_pencil_stay_adjacent_without_a_wide_empty_title_cell() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        for scale in [1.0, 1.5, 2.0] {
            ctx.set_pixels_per_point(scale);
            for width in [600.0, 1000.0] {
                for name in ["채널 0".to_owned(), "개발 서버".to_owned(), "W".repeat(64)] {
                    let mut rects = None;
                    for _ in 0..3 {
                        let _ = ctx.run(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 140.0),
                                )),
                                ..Default::default()
                            },
                            |ctx| {
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    ui.horizontal(|ui| {
                                        let name = title_label(ui, &name, 220.0);
                                        let edit = edit_button(ui);
                                        rects = Some((name.rect, edit.rect));
                                    });
                                });
                            },
                        );
                    }
                    let (name, edit) = rects.unwrap();
                    assert!(edit.left() >= name.right());
                    assert!(edit.left() - name.right() < 20.0);
                    assert!(edit.right() <= width);
                    assert!(name.left() < 20.0);
                    if name.width() < 200.0 {
                        assert!(edit.right() < 260.0, "short name/pencil floated away");
                    }
                }
            }
        }
    }
}
