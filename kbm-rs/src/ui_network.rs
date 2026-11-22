use eframe::egui::{self, RichText};
use minikmb_native::{
    active::Binding,
    input_sessions::Principal,
    network::Config,
    service::AppService,
    theme::{self, color},
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
#[derive(Default)]
struct View {
    busy: bool,
    error: String,
}
pub struct Panel {
    pub open: bool,
    binding: Option<Binding>,
    owner: Principal,
    view: Arc<Mutex<View>>,
    draft: Config,
    preview: bool,
    pane: u8,
    forward_editor: crate::ui_forward::Editor,
    scan_request: minikmb_native::network::scanner::Request,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            open: false,
            binding: None,
            owner: Principal {
                key_id: "ui".into(),
                client_id: "ui:network".into(),
                cancelled: CancellationToken::new(),
                ended: CancellationToken::new(),
            },
            view: Default::default(),
            draft: Default::default(),
            preview: false,
            pane: 0,
            forward_editor: Default::default(),
            scan_request: minikmb_native::network::scanner::Request {
                target: "192.168.127.100".into(),
                start: 1,
                end: 1024,
                timeout_ms: 1000,
            },
        }
    }
}
impl Panel {
    pub fn toggle(&mut self, b: Binding, app: &AppService) {
        if self.open && self.binding.as_ref() == Some(&b) {
            self.close();
            return;
        }
        self.close();
        let status = app.network.status(&b);
        self.draft = status.config;
        self.forward_editor.rules = status.forwarding.rules;
        self.scan_request.target = status
            .leases
            .first()
            .map(|lease| lease.ip.to_string())
            .unwrap_or_else(|| {
                let mut ip = self
                    .draft
                    .validate()
                    .unwrap_or(std::net::Ipv4Addr::new(192, 168, 127, 1))
                    .octets();
                ip[3] = self.draft.pool_start;
                std::net::Ipv4Addr::from(ip).to_string()
            });
        self.binding = Some(b);
        self.open = true;
        self.owner.ended = CancellationToken::new();
        self.view = Default::default();
    }
    fn close(&mut self) {
        self.open = false;
        self.owner.ended.cancel();
        self.binding = None;
    }
    fn submit(
        &mut self,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        name: &'static str,
        args: Value,
    ) {
        if self.preview || self.view.lock().busy {
            return;
        }
        let Some(b) = self.binding.clone() else {
            return;
        };
        let app = app.clone();
        let owner = self.owner.clone();
        let view = self.view.clone();
        {
            let mut v = view.lock();
            v.busy = true;
            v.error.clear();
        }
        runtime.spawn(async move {
            let result = app.execute_network(b, &owner, name, args).await;
            let mut v = view.lock();
            v.busy = false;
            if let Err(e) = result {
                v.error = e.message;
            }
        });
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
    ) {
        if !self.open {
            return;
        }
        let Some(b) = self.binding.clone() else {
            return;
        };
        if !self.preview && app.active.validate(&b).is_err() {
            self.close();
            return;
        }
        let mut status = app.network.status(&b);
        if self.preview && self.pane == 2 {
            status.mcu.enabled = true;
            status.mcu.link_up = true;
            status.forwarding.rules = self.forward_editor.rules.clone();
        }
        if self.preview && self.pane == 1 {
            status.mcu.enabled = true;
            status.mcu.link_up = true;
            status.scan = minikmb_native::network::scanner::Status {
                running: true,
                phase: "probing".into(),
                target: "192.168.127.100".into(),
                start: 1,
                end: 1024,
                timeout_ms: 1000,
                completed: 60,
                total: 1024,
                open: vec![22, 80, 443],
                closed: 40,
                filtered: 17,
                pending: 8,
                error: None,
            };
        }
        let busy = self.preview || self.view.lock().busy;
        let mut open = true;
        theme::tool_window(
            ctx,
            if self.preview {
                "네트워크 · UI 미리보기"
            } else {
                "네트워크 · 포워딩"
            },
            "network-dialog",
            [710.0, 500.0],
        )
        .open(&mut open)
        .show(ctx, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            if self.preview {
                ui.label(
                    RichText::new("장치 연결 없는 화면 예시입니다.")
                        .small()
                        .color(color::MUTED),
                );
            }
            theme::heading(ui, "네트워크", "");
            theme::status(
                ui,
                if status.phase == "failed" && status.cleanup_required {
                    theme::Tone::Warning
                } else if status.mcu.link_up {
                    theme::Tone::Success
                } else {
                    theme::Tone::Neutral
                },
                if status.phase == "failed" && status.cleanup_required {
                    "NCM 상태 확인 필요"
                } else if status.mcu.link_up {
                    "타깃 NCM 연결됨"
                } else if status.mcu.enabled {
                    "타깃 연결 대기"
                } else {
                    "꺼짐"
                },
            );
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("에뮬레이션 모드로 켜기"))
                    .clicked()
                {
                    self.submit(
                        app,
                        runtime,
                        "network_set_mode",
                        json!({"mode":"emulation"}),
                    );
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("호스트 네트워크 공유 켜기"))
                    .clicked()
                {
                    self.submit(app, runtime, "network_set_mode", json!({"mode":"host"}));
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("끄기 / OFF 확인"))
                    .clicked()
                {
                    self.submit(app, runtime, "network_set_mode", json!({"mode":"off"}));
                }
            });
            if busy && !self.preview {
                ui.spinner();
                ui.label("네트워크 모드 변경 중…");
            }
            if let Some(e) = &status.error {
                ui.colored_label(color::ERROR, &e.message);
            }
            if let Some(e) = &status.cleanup_error {
                ui.colored_label(color::ERROR, &e.message);
            }
            let error = self.view.lock().error.clone();
            if !error.is_empty() {
                ui.colored_label(color::ERROR, error);
            }
            if status.cleanup_required && status.phase == "failed" {
                ui.colored_label(color::WARNING, "원래 채널에서 OFF 확인이 필요합니다.");
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.pane, 0, "연결 설정");
                ui.selectable_value(&mut self.pane, 1, "포트 스캔");
                ui.selectable_value(&mut self.pane, 2, "포워딩");
            });
            ui.separator();
            if self.pane == 1 {
                self.show_scan(ui, app, runtime, &status, busy);
                return;
            }
            if self.pane == 2 {
                if let Some((name, args)) = self.forward_editor.show(ui, &status, busy) {
                    self.submit(app, runtime, name, args);
                }
                return;
            }
            theme::group_label(ui, "IPv4 · DHCP");
            ui.add_enabled_ui(!busy && !status.cleanup_required, |ui| {
                ui.horizontal(|ui| {
                    ui.label("서버 / 게이트웨이");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.draft.server_ip).desired_width(190.0),
                    );
                    ui.label("/24");
                });
                ui.horizontal(|ui| {
                    ui.label("임대 범위 끝자리");
                    ui.add(egui::DragValue::new(&mut self.draft.pool_start).range(2..=254));
                    ui.label("~");
                    ui.add(egui::DragValue::new(&mut self.draft.pool_end).range(2..=254));
                    ui.label("임대 시간 (초)");
                    ui.add(egui::DragValue::new(&mut self.draft.lease_seconds).range(60..=86400));
                });
                let valid = self.draft.validate();
                if let Err(e) = &valid {
                    ui.colored_label(color::WARNING, &e.message);
                }
                if ui
                    .add_enabled(valid.is_ok(), egui::Button::new("설정 적용"))
                    .clicked()
                {
                    self.submit(app, runtime, "network_configure", json!(self.draft));
                }
            });
            ui.separator();
            ui.label(format!(
                "수신 {}프레임 · {}B / 송신 {}프레임 · {}B / 대기 {}",
                status.rx_frames,
                status.rx_bytes,
                status.tx_frames,
                status.tx_bytes,
                status.tx_queued
            ));
            ui.label(format!(
                "앱 드롭 {} · MCU RX/TX 드롭 {}/{} · credit 드롭 {}",
                status.dropped,
                status.mcu.rx_dropped,
                status.mcu.tx_dropped,
                status.mcu.credit_dropped
            ));
            if status.mode == minikmb_native::network::Mode::Host {
                ui.label(format!("호스트 연결 {} / 32", status.host_flows));
            }
            theme::group_label(ui, "DHCP 임대");
            if status.leases.is_empty() {
                ui.label(RichText::new("할당된 주소가 없습니다.").color(color::MUTED));
            }
            egui::ScrollArea::vertical()
                .id_salt("network-leases")
                .max_height(180.0)
                .show(ui, |ui| {
                    for lease in &status.leases {
                        ui.label(format!(
                            "{} · {}",
                            lease.ip,
                            lease
                                .mac
                                .iter()
                                .map(|n| format!("{n:02X}"))
                                .collect::<Vec<_>>()
                                .join(":")
                        ));
                    }
                });
        });
        if !open {
            self.close();
        }
    }
    pub fn preview() -> Self {
        Self {
            open: true,
            binding: Some(Binding {
                uid: minikmb_native::model::Uid::from_bytes(&[0; 12]).unwrap(),
                generation: 0,
            }),
            preview: true,
            pane: if std::env::args().any(|arg| arg == "--forward-preview") {
                2
            } else if std::env::args().any(|arg| arg == "--scan-preview") {
                1
            } else {
                0
            },
            forward_editor: if std::env::args().any(|arg| arg == "--forward-preview") {
                crate::ui_forward::Editor::preview()
            } else {
                Default::default()
            },
            ..Default::default()
        }
    }
    fn show_scan(
        &mut self,
        ui: &mut egui::Ui,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        status: &minikmb_native::network::manager::Status,
        busy: bool,
    ) {
        let scan = &status.scan;
        theme::group_label(ui, "타깃 TCP 포트");
        ui.add_enabled_ui(!busy && !scan.running, |ui| {
            ui.horizontal(|ui| {
                ui.label("타깃 IPv4");
                ui.add(
                    egui::TextEdit::singleline(&mut self.scan_request.target).desired_width(190.0),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("포트 범위");
                ui.add(egui::DragValue::new(&mut self.scan_request.start).range(1..=65535));
                ui.label("~");
                ui.add(egui::DragValue::new(&mut self.scan_request.end).range(1..=65535));
                ui.label("대기 시간 (ms)");
                ui.add(egui::DragValue::new(&mut self.scan_request.timeout_ms).range(250..=5000));
            });
        });
        let valid = self.scan_request.validate(&status.config);
        if let Err(e) = &valid {
            ui.colored_label(color::WARNING, &e.message);
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy && !scan.running && status.mcu.link_up && valid.is_ok(),
                    egui::Button::new("스캔 시작"),
                )
                .clicked()
            {
                self.submit(app, runtime, "network_scan_start", json!(self.scan_request));
            }
            if ui
                .add_enabled(!busy && scan.running, egui::Button::new("스캔 중지"))
                .clicked()
            {
                self.submit(app, runtime, "network_scan_stop", json!({}));
            }
        });
        if !status.mcu.link_up {
            ui.label(RichText::new("타깃 NCM 연결 후 시작할 수 있습니다.").color(color::MUTED));
        }
        ui.separator();
        let phase = match scan.phase.as_str() {
            "resolving" => "주소 확인 중",
            "probing" => "스캔 중",
            "completed" => "완료",
            "stopped" => "중지됨",
            "cancelled" => "취소됨",
            "failed" => "실패",
            _ => "시작 대기",
        };
        ui.label(format!("{phase} · {} / {}", scan.completed, scan.total));
        if scan.total > 0 {
            ui.add(
                egui::ProgressBar::new(scan.completed as f32 / scan.total as f32).show_percentage(),
            );
        }
        ui.label(format!(
            "열림 {} · 닫힘 {} · 응답 없음 {}",
            scan.open.len(),
            scan.closed,
            scan.filtered
        ));
        if let Some(e) = &scan.error {
            ui.colored_label(color::ERROR, &e.message);
        }
        theme::group_label(ui, "열린 포트");
        if scan.open.is_empty() {
            ui.label(
                RichText::new(if scan.running {
                    "열린 포트를 찾는 중입니다."
                } else {
                    "열린 포트가 없습니다."
                })
                .color(color::MUTED),
            );
        }
        egui::ScrollArea::vertical()
            .id_salt("scan-open-ports")
            .max_height(200.0)
            .show_rows(ui, 24.0, scan.open.len().div_ceil(8), |ui, range| {
                for row in range {
                    ui.label(
                        scan.open
                            .chunks(8)
                            .nth(row)
                            .unwrap()
                            .iter()
                            .map(u16::to_string)
                            .collect::<Vec<_>>()
                            .join("    "),
                    );
                }
            });
    }
}
