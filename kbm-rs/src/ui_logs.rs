use eframe::egui::{self, RichText};
use minikmb_native::{
    input_sessions::Principal,
    logging::{Config, Entry, Event, Fields, Level, Status},
    model::Uid,
    service::AppService,
    theme::{self, color},
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
#[derive(Default)]
struct Work {
    busy: bool,
    error: String,
    message: String,
}
#[derive(Default)]
pub struct Panel {
    errors_only: bool,
    active_only: bool,
    filter: String,
    work: Arc<Mutex<Work>>,
    pub preview: bool,
}
fn principal() -> Principal {
    Principal {
        key_id: "ui".into(),
        client_id: "ui:logs".into(),
        cancelled: CancellationToken::new(),
        ended: CancellationToken::new(),
    }
}
fn event_matches(entry: &Entry, filter: &str) -> bool {
    filter.is_empty()
        || format!(
            "{} {}",
            serde_json::to_string(&entry.event).unwrap(),
            entry
                .fields
                .uid
                .as_ref()
                .map(Uid::display)
                .unwrap_or_default()
        )
        .to_ascii_lowercase()
        .contains(&filter.to_ascii_lowercase())
}
impl Panel {
    pub fn new(preview: bool) -> Self {
        Self {
            preview,
            ..Default::default()
        }
    }
    fn submit(
        &mut self,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        name: &'static str,
        args: Value,
    ) {
        if self.preview || self.work.lock().busy {
            return;
        }
        let app = app.clone();
        let work = self.work.clone();
        {
            let mut w = work.lock();
            w.busy = true;
            w.error.clear();
            w.message.clear();
        }
        runtime.spawn(async move {
            let result = app.execute_observability(&principal(), name, args).await;
            let mut w = work.lock();
            w.busy = false;
            match result {
                Ok(_) => {
                    w.message = match name {
                        "logs_open_folder" => "로그 폴더 열기를 요청했습니다.",
                        "logs_flush" => "대기 로그를 저장했습니다.",
                        _ => "로그 설정을 적용하고 대기 기록을 저장했습니다.",
                    }
                    .into()
                }
                Err(e) => w.error = e.message,
            }
        });
    }
    pub fn logs(
        &mut self,
        ui: &mut egui::Ui,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
    ) {
        theme::heading(
            ui,
            "동작 로그",
            "채널 동작과 오류를 기록합니다. 키 입력값·파일 내용·패킷 본문은 기록하지 않습니다.",
        );
        let mut status = app.logs.status();
        if self.preview {
            status = Status {
                enabled: true,
                verbose: false,
                dropped: 0,
                pending: 0,
                write_error: None,
                recent: vec![
                    Entry {
                        id: 1,
                        time_ms: 1730000000000,
                        level: Level::Info,
                        event: Event::DeviceConnected,
                        fields: Fields {
                            uid: Some(Uid::from_bytes(&[0x12; 12]).unwrap()),
                            ..Default::default()
                        },
                    },
                    Entry {
                        id: 2,
                        time_ms: 1730000001000,
                        level: Level::Info,
                        event: Event::Command,
                        fields: Fields {
                            command: Some("wake_configure"),
                            success: Some(true),
                            ..Default::default()
                        },
                    },
                    Entry {
                        id: 3,
                        time_ms: 1730000002000,
                        level: Level::Warn,
                        event: Event::Command,
                        fields: Fields {
                            command: Some("video_open"),
                            code: Some("CAPTURE_NOT_READY"),
                            success: Some(false),
                            ..Default::default()
                        },
                    },
                ],
            };
            ui.label(
                RichText::new("장치 연결 없는 화면 예시입니다.")
                    .small()
                    .color(color::MUTED),
            );
        }
        let busy = self.preview || self.work.lock().busy;
        let mut config = Config {
            enabled: status.enabled,
            verbose: status.verbose,
        };
        ui.horizontal_wrapped(|ui| {
            let changed = ui
                .add_enabled(!busy, egui::Checkbox::new(&mut config.enabled, "파일 기록"))
                .changed()
                | ui.add_enabled(
                    !busy,
                    egui::Checkbox::new(&mut config.verbose, "상세 메타데이터 (진단용)"),
                )
                .changed();
            if changed {
                self.submit(
                    app,
                    runtime,
                    "logs_configure",
                    json!({"enabled":config.enabled,"verbose":config.verbose}),
                );
            }
            if ui
                .add_enabled(!busy, egui::Button::new("대기 기록 저장"))
                .clicked()
            {
                self.submit(app, runtime, "logs_flush", json!({}));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("로그 폴더 열기"))
                .clicked()
            {
                self.submit(app, runtime, "logs_open_folder", json!({}));
            }
            if ui
                .add_enabled(!self.preview, egui::Button::new("최근 로그 복사"))
                .clicked()
            {
                ui.ctx()
                    .copy_text(serde_json::to_string_pretty(&status).unwrap());
            }
        });
        ui.label(RichText::new("기본 ON · JSONL · 파일당 4MiB / 최대 5개 · 최근 300건 · 설정은 이번 실행에 적용됩니다.").small().color(color::MUTED));
        if !self.preview {
            if let Some(directory) = app.logs.directory() {
                ui.label(
                    RichText::new(directory.display().to_string())
                        .small()
                        .color(color::MUTED),
                );
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.errors_only, "경고·오류만");
            ui.checkbox(&mut self.active_only, "활성 채널만");
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .desired_width(260.0)
                    .hint_text("이벤트 또는 UID 검색"),
            );
            ui.label(
                RichText::new(format!(
                    "대기 {}건 · 누락 {}건",
                    status.pending, status.dropped
                ))
                .small()
                .color(color::MUTED),
            );
        });
        if let Some(error) = &status.write_error {
            theme::status(ui, theme::Tone::Warning, &error.message);
        }
        {
            let work = self.work.lock();
            if work.busy {
                ui.spinner();
            }
            if !work.message.is_empty() {
                ui.colored_label(color::ACCENT, &work.message);
            }
            if !work.error.is_empty() {
                theme::status(ui, theme::Tone::Warning, &work.error);
            }
        }
        ui.separator();
        let active = app.active.state().uid;
        let filter = self.filter.to_ascii_lowercase();
        let entries = status
            .recent
            .iter()
            .rev()
            .filter(|entry| {
                (!self.errors_only || matches!(entry.level, Level::Warn | Level::Error))
                    && (!self.active_only || active.is_some() && entry.fields.uid == active)
                    && event_matches(entry, &filter)
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            theme::empty(
                ui,
                "표시할 로그가 없습니다.",
                "필터를 확인하거나 채널 동작 이후 다시 확인하세요.",
            );
        }
        egui::ScrollArea::both()
            .id_salt("operation-logs")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("operation-log-rows")
                    .striped(true)
                    .min_col_width(60.0)
                    .show(ui, |ui| {
                        for title in ["시각 (UTC)", "등급", "동작", "메타데이터"] {
                            ui.strong(title);
                        }
                        ui.end_row();
                        for entry in entries {
                            let t = entry.time_ms;
                            ui.monospace(format!(
                                "{:02}:{:02}:{:02}.{:03}",
                                t / 3600000 % 24,
                                t / 60000 % 60,
                                t / 1000 % 60,
                                t % 1000
                            ))
                            .on_hover_text(format!("Unix milliseconds: {t}"));
                            let color = match entry.level {
                                Level::Warn => color::WARNING,
                                Level::Error => color::ERROR,
                                Level::Debug => color::MUTED,
                                Level::Info => color::TEXT,
                            };
                            ui.colored_label(color, format!("{:?}", entry.level));
                            ui.monospace(
                                serde_json::to_string(&entry.event)
                                    .unwrap()
                                    .trim_matches('"'),
                            );
                            ui.monospace(serde_json::to_string(&entry.fields).unwrap());
                            ui.end_row();
                        }
                    });
            });
    }
    pub fn diagnostics(&mut self, ui: &mut egui::Ui, app: &AppService) {
        theme::heading(
            ui,
            "채널 진단",
            "현재 채널의 연결 상태와 오류·전송 카운터를 확인합니다.",
        );
        let data = if self.preview {
            ui.label(
                RichText::new("장치 연결 없는 화면 예시입니다.")
                    .small()
                    .color(color::MUTED),
            );
            json!({"uid":"0x121212121212121212121212","mcu_fresh":true,"transport":{"session":7,"event_gaps":0,"crc_rejected":0,"mcu":{"uptime_ms":123456,"crc_errors":0,"framing_errors":0,"event_dropped":0,"log_dropped":0}},"input":{"ready":true,"keys_held":0,"buttons_held":0,"release_unconfirmed":false},"video":{"running":true,"frame":{"width":1920,"height":1080,"fps":30}},"tty":null,"network":{"mode":"off"},"storage_job":null})
        } else {
            match app.active.bind().and_then(|b| app.diagnostics(&b)) {
                Ok(data) => data,
                Err(e) => {
                    theme::empty(ui, "진단할 활성 채널이 없습니다.", &e.message);
                    return;
                }
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.monospace(data["uid"].as_str().unwrap_or_default());
            theme::status(
                ui,
                if data["mcu_fresh"] == true {
                    theme::Tone::Success
                } else {
                    theme::Tone::Warning
                },
                if data["mcu_fresh"] == true {
                    "최근 MCU 상태 수신"
                } else {
                    "최근 MCU 상태 미확인"
                },
            );
            if ui
                .add_enabled(!self.preview, egui::Button::new("진단 정보 복사"))
                .clicked()
            {
                ui.ctx()
                    .copy_text(serde_json::to_string_pretty(&data).unwrap());
            }
        });
        ui.label(
            RichText::new("관리 통신과 HID 실행 완료·영상 수신·실제 OS 동작은 별개입니다.")
                .color(color::MUTED),
        );
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("diagnostic-summary")
                .spacing([28.0, 12.0])
                .show(ui, |ui| {
                    for (title, value) in [
                        (
                            "가동 시간 (ms)",
                            data["transport"]["mcu"]["uptime_ms"].clone(),
                        ),
                        (
                            "MCU CRC 오류",
                            data["transport"]["mcu"]["crc_errors"].clone(),
                        ),
                        (
                            "MCU 프레이밍 오류",
                            data["transport"]["mcu"]["framing_errors"].clone(),
                        ),
                        (
                            "MCU 이벤트 누락",
                            data["transport"]["mcu"]["event_dropped"].clone(),
                        ),
                        (
                            "호스트 이벤트 누락",
                            data["transport"]["event_gaps"].clone(),
                        ),
                        ("호스트 CRC 거부", data["transport"]["crc_rejected"].clone()),
                        ("HID 준비", data["input"]["ready"].clone()),
                        (
                            "입력 해제 미확인",
                            data["input"]["release_unconfirmed"].clone(),
                        ),
                        ("영상 캡처 가동", data["video"]["running"].clone()),
                        ("네트워크 모드", data["network"]["mode"].clone()),
                    ] {
                        ui.label(title);
                        ui.monospace(if value.is_null() {
                            "미확인".into()
                        } else if let Some(value) = value.as_bool() {
                            match (title, value) {
                                ("HID 준비", true) => "준비됨",
                                ("HID 준비", false) => "미준비",
                                ("입력 해제 미확인", true) => "확인 필요",
                                ("입력 해제 미확인", false) => "없음",
                                (_, true) => "가동 중",
                                (_, false) => "중지됨",
                            }
                            .into()
                        } else if title == "네트워크 모드" {
                            match value.as_str() {
                                Some("off") => "꺼짐",
                                Some("emulation") => "에뮬레이션",
                                Some("host") => "호스트 공유",
                                _ => "미확인",
                            }
                            .into()
                        } else {
                            value.to_string().trim_matches('"').to_owned()
                        });
                        ui.end_row();
                    }
                });
            ui.add_space(12.0);
            ui.collapsing("세부 진단 정보", |ui| {
                theme::code(ui, &serde_json::to_string_pretty(&data).unwrap())
            });
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_matches_the_displayed_event_name_and_uid_without_case_dependency() {
        let entry = Entry {
            id: 1,
            time_ms: 0,
            level: Level::Info,
            event: Event::DeviceConnected,
            fields: Fields {
                uid: Some(Uid::from_bytes(&[0xab; 12]).unwrap()),
                ..Default::default()
            },
        };
        assert!(event_matches(&entry, "device_connected"));
        assert!(event_matches(&entry, "DEVICE_CONNECTED"));
        assert!(event_matches(&entry, "0xabab"));
        assert!(event_matches(&entry, "0xABAB"));
        assert!(event_matches(&entry, ""));
        assert!(!event_matches(&entry, "device_disconnected"));
    }
}
