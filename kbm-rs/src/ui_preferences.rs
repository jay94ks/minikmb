use eframe::egui::{self, RichText};
use minikmb_native::{
    preferences::{
        self,
        legacy::{Plan, Report},
    },
    service::AppService,
    theme::{self, color},
};
use parking_lot::Mutex;
use std::{io::Read, sync::Arc};
#[derive(Default)]
struct Work {
    busy: bool,
    plan: Option<Plan>,
    report: Option<Report>,
    message: String,
    error: String,
}
#[derive(Default)]
pub struct Panel {
    work: Arc<Mutex<Work>>,
    confirmed: bool,
    pub preview: bool,
}
#[cfg(windows)]
fn choose(parent: isize) -> Result<Option<Vec<u8>>, String> {
    use std::{
        ffi::OsString,
        os::windows::{ffi::OsStringExt, fs::OpenOptionsExt},
    };
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::{
            Foundation::HWND,
            System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
            UI::Controls::Dialogs::{
                CommDlgExtendedError, GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST,
                OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
            },
        },
    };
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|_| "파일 선택기를 시작하지 못했습니다.")?;
    }
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _com = Com;
    let mut path = vec![0u16; 32768];
    let filter = "MiniKMB 설정 JSON\0*.json\0\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let title = "Electron MiniKMB 설정 가져오기\0"
        .encode_utf16()
        .collect::<Vec<_>>();
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: HWND(parent as *mut _),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(path.as_mut_ptr()),
        nMaxFile: path.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
        return if unsafe { CommDlgExtendedError() }.0 == 0 {
            Ok(None)
        } else {
            Err("설정 파일을 선택하지 못했습니다.".into())
        };
    }
    let end = path
        .iter()
        .position(|c| *c == 0)
        .ok_or("설정 파일을 선택하지 못했습니다.")?;
    let path = std::path::PathBuf::from(OsString::from_wide(&path[..end]));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .map_err(|_| "설정 파일을 읽지 못했습니다.")?;
    let meta = file
        .metadata()
        .map_err(|_| "설정 파일을 읽지 못했습니다.")?;
    if !meta.is_file() || meta.len() > preferences::MAX_BYTES {
        return Err("설정 JSON은1MiB 이하의 일반 파일이어야 합니다.".into());
    }
    let mut bytes = vec![];
    file.take(preferences::MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "설정 파일을 읽지 못했습니다.")?;
    if bytes.len() as u64 > preferences::MAX_BYTES {
        return Err("설정 파일이 너무 큽니다.".into());
    }
    Ok(Some(bytes))
}
#[derive(Clone, Copy)]
enum Action {
    Pick(isize),
    Apply,
    Reload,
    Recover,
}
impl Panel {
    pub fn new(preview: bool) -> Self {
        Self {
            preview,
            ..Default::default()
        }
    }
    fn submit(&mut self, app: &Arc<AppService>, runtime: &tokio::runtime::Handle, action: Action) {
        if self.preview || self.work.lock().busy {
            return;
        }
        let plan = if matches!(action, Action::Apply) {
            self.work.lock().plan.take()
        } else {
            None
        };
        {
            let mut work = self.work.lock();
            work.busy = true;
            work.error.clear();
            work.message.clear();
            if !matches!(action, Action::Apply) {
                work.plan = None;
                work.report = None;
            }
        }
        let store = app.preferences.clone();
        let work = self.work.clone();
        runtime.spawn_blocking(move|| {
            let result=(||->Result<(),String> {match action {
                Action::Pick(parent)=>{
                    let Some(bytes)=choose(parent)? else{return Ok(());};let plan=store.prepare_import(&bytes).map_err(|error|error.message)?;
                    let mut state=work.lock();state.report=Some(plan.report.clone());state.plan=Some(plan);
                },
                Action::Apply=>{let report=store.apply_import(plan.ok_or("가져오기 미리보기를 다시 만들어 주세요.")?).map_err(|error|error.message)?;let mut state=work.lock();state.report=Some(report);state.message="설정을 가져왔습니다. 실행 중 서비스는 유지하고 다음 시작에 적용합니다.".into();},
                Action::Reload=>{store.reload().map_err(|error|error.message)?;work.lock().message="저장된 설정을 다시 불러왔습니다.".into();},
                Action::Recover=>{store.recover_invalid(true).map_err(|error|error.message)?;work.lock().message="원본을 별도 파일로 보관하고 설정을 초기화했습니다.".into();}
            }Ok(())})();let mut state=work.lock();state.busy=false;if let Err(error)=result {state.error=error;}
        });
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        parent: Option<isize>,
    ) {
        theme::heading(
            ui,
            "설정",
            "영상 모드와 네트워크·포워딩·WOL 설정을 채널 UID별로 보관합니다.",
        );
        minikmb_native::assets::credits(ui);
        if self.preview {
            ui.label(
                RichText::new("장치 연결 없는 화면 예시입니다.")
                    .small()
                    .color(color::MUTED),
            );
        }
        let mut document = app.preferences.snapshot();
        if self.preview {
            document = preferences::Document {
                version: 1,
                revision: 4,
                channel_order: vec![],
                channels: vec![preferences::Row {
                    uid: minikmb_native::model::Uid::from_bytes(&[0x12; 12]).unwrap(),
                    preferences: preferences::Channel {
                        video: Some(minikmb_native::media::CaptureMode {
                            width: 1920,
                            height: 1080,
                            fps: 59.94,
                        }),
                        network: Some(Default::default()),
                        ..Default::default()
                    },
                }],
            };
        }
        let status = app.preferences.status();
        let busy = self.preview || self.work.lock().busy;
        ui.label(format!(
            "저장된 채널 {}개 · 변경 번호 {}",
            document.channels.len(),
            document.revision
        ));
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy && parent.is_some(),
                    egui::Button::new("Electron 설정 JSON 가져오기"),
                )
                .clicked()
            {
                self.submit(app, runtime, Action::Pick(parent.unwrap()));
            }
            if ui
                .add_enabled(!busy, egui::Button::new("다시 불러오기"))
                .clicked()
            {
                self.submit(app, runtime, Action::Reload);
            }
        });
        ui.label(RichText::new("기존 Rust 설정을 우선 보존합니다. 가져온 값으로 네트워크·포워딩·서버를 자동 시작하지 않습니다.").color(color::MUTED));
        if let Some(error) = &status.blocked {
            theme::status(ui, theme::Tone::Warning, &error.message);
            ui.checkbox(&mut self.confirmed, "원본을 보관하고 손상된 설정 초기화");
            if ui
                .add_enabled(
                    !busy && self.confirmed,
                    egui::Button::new("원본 보관 후 초기화"),
                )
                .clicked()
            {
                self.submit(app, runtime, Action::Recover);
                self.confirmed = false;
            }
        } else if let Some(error) = &status.error {
            theme::status(ui, theme::Tone::Warning, &error.message);
        }
        let (report, plan_ready, message, error) = {
            let state = self.work.lock();
            (
                state.report.clone(),
                state.plan.is_some(),
                state.message.clone(),
                state.error.clone(),
            )
        };
        if let Some(report) = report {
            ui.separator();
            theme::group_label(ui, "가져오기 검토");
            ui.label(format!(
                "추가 채널 {} · 새 항목 {} · 기존 값 유지 {} · 제외 키 {}",
                report.added_channels,
                report.imported_fields,
                report.preserved_fields,
                report.ignored_keys
            ));
            egui::ScrollArea::vertical()
                .id_salt("preference-import-issues")
                .max_height(100.0)
                .show(ui, |ui| {
                    for issue in &report.rejected {
                        ui.colored_label(
                            color::WARNING,
                            format!(
                                "{} · {}: {}",
                                issue
                                    .uid
                                    .as_ref()
                                    .map(|uid| uid.display())
                                    .unwrap_or_default(),
                                match issue.field.as_str() {
                                    "video" => "영상",
                                    "network" => "네트워크",
                                    "rules" => "포워딩",
                                    "wol" => "WOL",
                                    _ => "설정",
                                },
                                if issue.code == "LEGACY_ROUTE_UNSUPPORTED" {
                                    "이전 타깃 경로를 자동 변환할 수 없습니다."
                                } else {
                                    "유효한 저장 값이 아닙니다."
                                }
                            ),
                        );
                    }
                });
            if ui
                .add_enabled(
                    !busy && status.blocked.is_none() && plan_ready && report.imported_fields > 0,
                    egui::Button::new("가져오기 적용"),
                )
                .clicked()
            {
                self.submit(app, runtime, Action::Apply);
            }
        }
        if !message.is_empty() {
            ui.colored_label(color::ACCENT, message);
        }
        if !error.is_empty() {
            ui.colored_label(color::ERROR, error);
        }
        if self.work.lock().busy {
            ui.spinner();
        }
        ui.separator();
        theme::group_label(ui, "저장된 채널 설정");
        if document.channels.is_empty() {
            ui.label(RichText::new("저장된 설정이 없습니다.").color(color::MUTED));
        }
        egui::ScrollArea::both()
            .id_salt("saved-preferences")
            .max_height(260.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("saved-preference-grid")
                    .striped(true)
                    .show(ui, |ui| {
                        for (label, width) in [
                            ("채널 UID", 250.0),
                            ("영상 모드", 170.0),
                            ("네트워크", 160.0),
                            ("포워딩", 70.0),
                            ("WOL 설정", 85.0),
                        ] {
                            ui.add_sized(
                                [width, 28.0],
                                egui::Label::new(RichText::new(label).strong())
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        }
                        ui.end_row();
                        for row in document.channels {
                            ui.monospace(row.uid.display());
                            ui.label(
                                row.preferences
                                    .video
                                    .map(|mode| {
                                        if mode.width == 0 {
                                            "자동".into()
                                        } else {
                                            format!(
                                                "{}×{} / {}Hz",
                                                mode.width, mode.height, mode.fps
                                            )
                                        }
                                    })
                                    .unwrap_or_else(|| "기본값".into()),
                            );
                            ui.label(
                                row.preferences
                                    .network
                                    .as_ref()
                                    .map(|config| config.server_ip.as_str())
                                    .unwrap_or("기본값"),
                            );
                            ui.label(row.preferences.rules().len().to_string());
                            ui.label(if row.preferences.wol.is_some() {
                                "있음"
                            } else {
                                "없음"
                            });
                            ui.end_row();
                        }
                    });
            });
    }
}
