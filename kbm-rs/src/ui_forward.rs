use eframe::egui::{self, RichText};
use minikmb_native::{
    network::{
        forwarding::{self, Protocol, Rule},
        manager::Status,
    },
    theme::{self, color},
};
use serde_json::{json, Value};
#[derive(Default)]
pub struct Editor {
    pub rules: Vec<Rule>,
}
impl Editor {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        status: &Status,
        busy: bool,
    ) -> Option<(&'static str, Value)> {
        let forward = &status.forwarding;
        let mut action = None;
        if forward.cleanup_required {
            self.rules = forward.rules.clone();
        }
        theme::group_label(ui, "로컬 포트 → 타깃");
        ui.label("수신 주소: 127.0.0.1");
        let editable = !busy && !forward.cleanup_required;
        ui.add_enabled_ui(editable, |ui| {
            let mut remove = None;
            egui::ScrollArea::vertical()
                .id_salt("forward-rules")
                .max_height(230.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    egui::Grid::new("forward-rule-grid")
                        .striped(true)
                        .show(ui, |ui| {
                            for (title, width) in [
                                ("프로토콜", 90.0),
                                ("로컬 포트", 90.0),
                                ("타깃 주소", 210.0),
                                ("타깃 포트", 90.0),
                                ("", 62.0),
                            ] {
                                ui.add_sized(
                                    [width, 28.0],
                                    egui::Label::new(RichText::new(title).strong())
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                );
                            }
                            ui.end_row();
                            for (index, rule) in self.rules.iter_mut().enumerate() {
                                egui::ComboBox::from_id_salt(("forward-protocol", &rule.id))
                                    .selected_text(match rule.protocol {
                                        Protocol::Tcp => "TCP",
                                        Protocol::Udp => "UDP",
                                    })
                                    .width(90.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut rule.protocol,
                                            Protocol::Tcp,
                                            "TCP",
                                        );
                                        ui.selectable_value(
                                            &mut rule.protocol,
                                            Protocol::Udp,
                                            "UDP",
                                        );
                                    });
                                ui.add_sized(
                                    [90.0, 30.0],
                                    egui::DragValue::new(&mut rule.listen_port).range(1..=65535),
                                );
                                ui.add_sized(
                                    [210.0, 30.0],
                                    egui::TextEdit::singleline(&mut rule.target_ip)
                                        .desired_width(210.0),
                                );
                                ui.add_sized(
                                    [90.0, 30.0],
                                    egui::DragValue::new(&mut rule.target_port).range(1..=65535),
                                );
                                if ui
                                    .add_sized([62.0, 30.0], egui::Button::new("삭제"))
                                    .clicked()
                                {
                                    remove = Some(index);
                                }
                                ui.end_row();
                            }
                        });
                });
            if let Some(index) = remove {
                self.rules.remove(index);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.rules.len() < 32, egui::Button::new("+ 규칙 추가"))
                    .clicked()
                {
                    let id = (1..=33)
                        .map(|n| format!("rule-{n}"))
                        .find(|id| !self.rules.iter().any(|rule| &rule.id == id))
                        .unwrap();
                    let mut ip = status.config.validate().unwrap().octets();
                    ip[3] = status.config.pool_start;
                    self.rules.push(Rule {
                        id,
                        protocol: Protocol::Tcp,
                        listen_port: 18022,
                        target_ip: std::net::Ipv4Addr::from(ip).to_string(),
                        target_port: 22,
                    });
                }
                if ui.button("저장된 규칙 불러오기").clicked() {
                    self.rules = forward.rules.clone();
                }
            });
        });
        let valid = if self.rules.is_empty() {
            Ok(())
        } else {
            forwarding::validate(&self.rules, &status.config)
        };
        if let Err(e) = &valid {
            ui.colored_label(color::WARNING, &e.message);
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(editable && valid.is_ok(), egui::Button::new("규칙 적용"))
                .clicked()
            {
                action = Some(("network_forward_configure", json!({"rules":self.rules})));
            }
            if ui
                .add_enabled(
                    editable && valid.is_ok() && !self.rules.is_empty() && status.mcu.link_up,
                    egui::Button::new("포워딩 시작"),
                )
                .clicked()
            {
                action = Some(("network_forward_start", json!({"rules":self.rules})));
            }
            if ui
                .add_enabled(
                    !busy && forward.cleanup_required,
                    egui::Button::new("포워딩 중지"),
                )
                .clicked()
            {
                action = Some(("network_forward_stop", json!({})));
            }
        });
        ui.separator();
        ui.label(if forward.running {
            "포워딩 실행 중"
        } else {
            "포워딩 중지됨"
        });
        ui.label(format!(
            "포워딩 연결 {} · 전체 연결 {} / 32 · 드롭 {}",
            forward.connections, status.flow_usage, forward.dropped
        ));
        if let Some(e) = &forward.error {
            ui.colored_label(color::ERROR, &e.message);
        }
        if !status.mcu.link_up {
            ui.label(RichText::new("타깃 NCM 연결 후 시작할 수 있습니다.").color(color::MUTED));
        }
        action
    }
    pub fn preview() -> Self {
        Self {
            rules: vec![
                Rule {
                    id: "ssh".into(),
                    protocol: Protocol::Tcp,
                    listen_port: 18022,
                    target_ip: "192.168.127.100".into(),
                    target_port: 22,
                },
                Rule {
                    id: "dns".into(),
                    protocol: Protocol::Udp,
                    listen_port: 18053,
                    target_ip: "192.168.127.100".into(),
                    target_port: 53,
                },
            ],
        }
    }
}
