use eframe::egui::{self, RichText};
use minikmb_native::{
    api_docs as docs,
    service::{AppService, COMMANDS},
    theme::{self, color},
};
#[derive(Default, PartialEq)]
enum Tab {
    #[default]
    Request,
    Response,
    Errors,
    Workflow,
}
#[derive(Default)]
pub struct Panel {
    query: String,
    group: Option<&'static str>,
    selected: usize,
    tab: Tab,
    cached: Option<usize>,
    input: String,
    output: String,
    error: String,
    message: String,
}
impl Panel {
    pub fn preview() -> Self {
        Self {
            selected: COMMANDS
                .iter()
                .position(|c| c.name == "video_snapshot")
                .unwrap(),
            tab: Tab::Response,
            ..Default::default()
        }
    }
    pub fn rest(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        app: &AppService,
        parent: Option<isize>,
    ) {
        theme::heading(
            ui,
            "REST API 문서",
            "요청·응답·오류와 사용 흐름을 확인합니다.",
        );
        let address = app.remote.read().rest;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("명령·경로·설명 검색")
                    .desired_width(280.0),
            );
            egui::ComboBox::from_id_salt("reference-group")
                .selected_text(self.group.unwrap_or("전체 기능"))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.group, None, "전체 기능");
                    for &group in docs::GROUPS {
                        ui.selectable_value(&mut self.group, Some(group), group);
                    }
                });
            if ui.button("OpenAPI 복사").clicked() {
                ctx.copy_text(
                    serde_json::to_string_pretty(&minikmb_native::remote::openapi_for(address))
                        .unwrap(),
                );
                self.message = "OpenAPI JSON을 복사했습니다.".into();
            }
            if ui
                .add_enabled(parent.is_some(), egui::Button::new("JSON 내보내기"))
                .clicked()
            {
                match save(
                    parent.unwrap(),
                    &minikmb_native::remote::openapi_for(address),
                ) {
                    Ok(true) => self.message = "OpenAPI JSON을 저장했습니다.".into(),
                    Ok(false) => {}
                    Err(e) => self.message = e,
                }
            }
        });
        ui.label(
            RichText::new(address.map_or("REST 서버 OFF · 오프라인 문서".into(), |a| {
                format!("http://{a} · Bearer 인증")
            }))
            .small()
            .color(color::MUTED),
        );
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        ui.separator();
        let filtered: Vec<_> = COMMANDS
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                docs::matches(c, &self.query) && self.group.is_none_or(|g| g == docs::group(c.name))
            })
            .map(|(i, _)| i)
            .collect();
        if filtered.is_empty() {
            ui.label("검색 결과가 없습니다.");
            return;
        }
        if !filtered.contains(&self.selected) {
            self.selected = filtered[0];
        }
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(258.0,height),egui::Layout::top_down(egui::Align::Min),|ui| {
                egui::ScrollArea::vertical().id_salt("reference-command-list").max_height(height).show(ui,|ui| {
                    for &group in docs::GROUPS {
                        let rows:Vec<_>=filtered.iter().copied().filter(|&i|docs::group(COMMANDS[i].name)==group).collect();
                        if rows.is_empty() {continue;}
                        ui.label(RichText::new(group).small().color(color::ACCENT));
                        for i in rows {
                            let c=&COMMANDS[i];
                            if theme::list_row(ui,self.selected==i,&format!("{}  {}",c.method,c.title)).clicked(){self.selected=i;}
                        }
                        ui.add_space(8.0);
                    }
                });
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Wrap);
                if self.cached!=Some(self.selected) {
                    self.cached=Some(self.selected);
                    let c=&COMMANDS[self.selected];
                    self.input=serde_json::to_string_pretty(&minikmb_native::remote::input_schema(c.name)).unwrap();
                    self.output=serde_json::to_string_pretty(&docs::response_schema(c.name).unwrap()).unwrap();
                    self.error=serde_json::to_string_pretty(&docs::error_schema()).unwrap();
                }
                let c=&COMMANDS[self.selected];
                theme::heading(ui,c.title,c.description);
                ui.label(RichText::new(format!("{} {}",c.method,c.path)).monospace().color(color::ACCENT));
                ui.label(RichText::new(format!("MCP: {}  ·  {}",c.name,if c.active_required {"현재 활성 채널"}else{"앱 전역/소유 작업"})).small().color(color::MUTED));
                ui.horizontal(|ui| {
                    for (tab,label) in [(Tab::Request,"요청"),(Tab::Response,"응답"),(Tab::Errors,"오류"),(Tab::Workflow,"사용 흐름")] {if theme::tab(ui,self.tab==tab,label).clicked(){self.tab=tab;}}
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt("reference-detail").auto_shrink([false,false]).max_height(ui.available_height().max(1.0)).show(ui,|ui|match self.tab {
                    Tab::Request=>{
                        ui.label("PowerShell 7 예제 · 복사는 요청을 실행하지 않습니다.");
                        let example=minikmb_native::remote::request_example(c,address);
                        if ui.button("요청 예제 복사").clicked(){ctx.copy_text(example.clone());}
                        theme::code(ui,&example);
                        ui.add_space(12.0);
                        if c.name=="storage_upload_prepare" {ui.label("반환된 url에 같은 키·정확한 Content-Length로 파일 원문을 POST합니다. 파일 전체를 MCP 인자에 넣지 마세요.");}
                        if c.name=="storage_action" {ui.label("action별 본문은 아래 anyOf 분기와 $defs에 있습니다. 예제는 읽기 전용 목록 갱신입니다.");}
                        egui::CollapsingHeader::new("파라미터·본문 JSON Schema").default_open(true).show(ui,|ui|theme::code(ui,&self.input));
                    }
                    Tab::Response=>{
                        if c.name=="video_snapshot" {
                            ui.label("REST: image/jpeg 또는 image/png 바이너리. Cache-Control: no-store.");
                            ui.label("헤더: x-minikmb-uid / generation / frame / stream / width / height / received-at (각 x-minikmb- 접두사).");
                            ui.label("MCP: image content와 아래 FrameInfo structuredContent. 최근1초 이내 프레임만 반환합니다.");
                        } else {ui.label("REST: application/json · MCP: 같은 structuredContent");}
                        if c.name.starts_with("storage_"){ui.label("job_id는 수락된 작업입니다. state·error·cleanup_error를 조회해 완료를 확인하세요. result는 action/phase에 따라 달라집니다. can_cancel=false인 전체 카드 포맷은 취소할 수 없습니다.");}
                        if c.name.starts_with("hid_"){ui.label("usb_completed는 USB 보고서 완료이며 타깃 OS/앱의 동작 확인은 아닙니다. no_op와 ticket을 함께 확인하세요.");}
                        if ui.button("응답 스키마 복사").clicked(){ctx.copy_text(self.output.clone());}
                        theme::code(ui,&self.output);
                    }
                    Tab::Errors=>{
                        ui.label("request_id·code·message와 원래 channel_uid/generation을 확인하세요. 필요한 경우 job_id가 포함됩니다. 채널 문맥이 없거나 인증 실패면 null입니다. MCU status/uncertain도 확인하고 uncertain DOWN/쓰기는 무조건 재시도하지 마세요. REST 인증·파싱 오류도 같은 JSON이며, MCP 프로토콜 오류는 SDK 형식을 유지합니다.");
                        egui::Grid::new("reference-errors").striped(true).num_columns(2).show(ui,|ui|{for (status,description) in docs::ERROR_STATUSES {ui.monospace(*status);ui.label(*description);ui.end_row();}});
                        egui::CollapsingHeader::new("업무 오류 JSON Schema").show(ui,|ui|theme::code(ui,&self.error));
                    }
                    Tab::Workflow=>{
                        for topic in docs::GUIDE_TOPICS.iter().filter(|t|t.commands.contains(&c.name)) {
                            ui.label(RichText::new(topic.title).color(color::ACCENT));ui.label(topic.body);ui.add_space(12.0);
                        }
                        ui.label(docs::OVERVIEW);
                    }
                });
            });
        });
    }
    pub fn guide(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, app: &AppService) {
        theme::heading(
            ui,
            "MCP 연동 안내",
            "Streamable HTTP · 같은 활성 채널과 공통 명령 서비스",
        );
        let servers = app.remote.read().clone();
        ui.horizontal_wrapped(|ui| {
            if ui.button("연결 설정 복사").clicked() {
                ctx.copy_text(
                    serde_json::to_string_pretty(&docs::mcp_config(servers.mcp)).unwrap(),
                );
            }
            if ui.button("전체 안내 복사").clicked() {
                ctx.copy_text(docs::mcp_guide(servers.rest, servers.mcp));
            }
            ui.label(
                RichText::new(if servers.mcp.is_some() {
                    "MCP 실행 중"
                } else {
                    "MCP 서버 OFF · 아래 주소는 예시"
                })
                .small()
                .color(color::MUTED),
            );
        });
        egui::ScrollArea::vertical().id_salt("mcp-reference-guide").show(ui,|ui| {
            theme::code(ui,&serde_json::to_string_pretty(&docs::mcp_config(servers.mcp)).unwrap());
            ui.label("이 설정은 연결 정보 예시입니다. 클라이언트마다 설정 파일 형식이 다릅니다.");
            for (i,topic) in docs::GUIDE_TOPICS.iter().enumerate() {
                egui::CollapsingHeader::new(topic.title).id_salt(("mcp-topic",i)).default_open(i<2).show(ui,|ui| {
                    ui.label(topic.body);
                    ui.label(RichText::new(topic.commands.join(" · ")).monospace().small().color(color::MUTED));
                });
            }
            egui::CollapsingHeader::new("읽기 전용 리소스").default_open(true).show(ui,|ui|{
                for r in minikmb_native::remote_resources::RESOURCES {ui.horizontal_wrapped(|ui|{ui.monospace(r.uri);if ui.small_button("복사").clicked(){ctx.copy_text(r.uri.into());}ui.label(r.name);});ui.label(RichText::new(r.description).small().color(color::MUTED));}
                ui.label("상태 URI는 변경 알림 뒤 다시 읽습니다. URI에 UID나 호스트 경로를 붙이지 마세요.");
            });
            egui::CollapsingHeader::new("전체 도구 목록").show(ui,|ui|{for c in COMMANDS {ui.horizontal_wrapped(|ui|{ui.monospace(c.name);ui.label(c.title);});}});
        });
    }
}

#[cfg(windows)]
fn save(parent: isize, document: &serde_json::Value) -> Result<bool, String> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
    use windows::{
        core::{w, HRESULT},
        Win32::{
            Foundation::HWND,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
            },
            UI::Shell::{
                Common::COMDLG_FILTERSPEC, FileSaveDialog, IFileSaveDialog, FOS_FORCEFILESYSTEM,
                FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, SIGDN_FILESYSPATH,
            },
        },
    };
    let error = || "OpenAPI JSON을 선택한 파일에 저장하지 못했습니다.".to_owned();
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }
        .ok()
        .map_err(|_| error())?;
    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    let _com = Com;
    let dialog: IFileSaveDialog =
        unsafe { CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| error())?;
    unsafe {
        dialog
            .SetOptions(
                FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR | FOS_PATHMUSTEXIST | FOS_OVERWRITEPROMPT,
            )
            .map_err(|_| error())?;
        dialog
            .SetTitle(w!("MiniKMB · OpenAPI JSON 내보내기"))
            .map_err(|_| error())?;
        dialog
            .SetFileName(w!("minikmb-openapi.json"))
            .map_err(|_| error())?;
        dialog
            .SetDefaultExtension(w!("json"))
            .map_err(|_| error())?;
        dialog
            .SetFileTypes(&[COMDLG_FILTERSPEC {
                pszName: w!("JSON"),
                pszSpec: w!("*.json"),
            }])
            .map_err(|_| error())?;
        if let Err(e) = dialog.Show(Some(HWND(parent as *mut _))) {
            if e.code() == HRESULT(0x800704c7u32 as i32) {
                return Ok(false);
            }
            return Err(error());
        }
        let item = dialog.GetResult().map_err(|_| error())?;
        let name = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|_| error())?;
        let path = PathBuf::from(OsString::from_wide(name.as_wide()));
        CoTaskMemFree(Some(name.0 as *const _));
        std::fs::write(
            path,
            serde_json::to_vec_pretty(document).map_err(|_| error())?,
        )
        .map_err(|_| error())?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_reference_and_guide_render_without_starting_any_service() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().to_owned()).unwrap();
        let ctx = egui::Context::default();
        let mut panel = Panel::preview();
        for guide in [false, true] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 820.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if guide {
                            panel.guide(ui, ctx, &app);
                        } else {
                            panel.rest(ui, ctx, &app, None);
                        }
                    });
                },
            );
            assert!(!output.shapes.is_empty());
        }
        assert!(app.active.state().uid.is_none());
        assert!(app.remote.read().rest.is_none() && app.remote.read().mcp.is_none());
        assert!(app.storage.resources_info().is_none());
        assert!(app.network.resource().is_none());
        app.logs.shutdown();
    }
    #[test]
    fn filtering_selects_the_visible_document_and_handles_an_empty_result() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppService::new(dir.path().to_owned()).unwrap();
        let ctx = egui::Context::default();
        let mut panel = Panel {
            query: "storage_upload_prepare".into(),
            ..Default::default()
        };
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| panel.rest(ui, ctx, &app, None));
        });
        assert_eq!(COMMANDS[panel.selected].name, "storage_upload_prepare");
        panel.query = "not-a-command".into();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| panel.rest(ui, ctx, &app, None));
        });
        assert!(app.active.state().uid.is_none());
        app.logs.shutdown();
    }
}
