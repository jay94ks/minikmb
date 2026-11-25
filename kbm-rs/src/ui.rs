use eframe::egui::{self, RichText};
mod channel_order;
mod notice;
mod virtual_keys;
mod window;
use minikmb_native::keyboard_capture::windows as keyboard;
use minikmb_native::{
    auth::SecretToken,
    model::Uid,
    service::AppService,
    theme::{self, color, size},
};
use std::{collections::BTreeSet, sync::Arc};

#[derive(PartialEq)]
enum Page {
    Firmware,
    Channels,
    Control,
    ApiDocs,
    McpGuide,
    Keys,
    StylePreview,
    Preferences,
    Logs,
    Diagnostics,
}
pub struct Desktop {
    app: Arc<AppService>,
    runtime: tokio::runtime::Runtime,
    page: Page,
    filter: String,
    key_name: String,
    new_key: Option<SecretToken>,
    message: String,
    notice_timer: notice::Timer,
    reference_ui: crate::ui_reference::Panel,
    servers: Arc<minikmb_native::remote::RemoteServers>,
    ipc: Option<minikmb_native::ipc::Server>,
    screenshot: Option<std::path::PathBuf>,
    benchmark: Option<crate::ui_benchmark::Session>,
    layout_preview: Option<crate::ui_preview::Layout>,
    paints: usize,
    screenshot_requested: bool,
    preview_started: std::time::Instant,
    action_result: Arc<parking_lot::Mutex<Option<String>>>,
    video_mode: minikmb_native::media::CaptureMode,
    manual_video: bool,
    texture: Option<egui::TextureHandle>,
    displayed_frame: String,
    input: minikmb_native::ui_input::UiInput,
    video_focused: bool,
    video_rect: Option<egui::Rect>,
    video_info: Option<minikmb_native::video::FrameInfo>,
    video_guard: Option<tokio_util::sync::CancellationToken>,
    input_owner: Option<minikmb_native::input::Owner>,
    input_epoch: u64,
    window_input: Arc<minikmb_native::window_input::WindowInput>,
    normal_policy: minikmb_native::keyboard_capture::Policy,
    normal_wheel: [i32; 2],
    buttons: BTreeSet<u8>,
    pointer: Option<egui::Pos2>,
    virtual_open: bool,
    virtual_mods: BTreeSet<u16>,
    virtual_rows: Vec<Vec<minikmb_native::keyboard_layout::Key>>,
    virtual_choices: Vec<minikmb_native::keyboard_layout::Key>,
    virtual_choice: u16,
    virtual_rect: Option<egui::Rect>,
    pending_preset: Option<(String, Vec<u16>, minikmb_native::active::Binding)>,
    rendered_active: minikmb_native::active::ActiveState,
    terminal: crate::ui_tty::Terminal,
    storage_ui: crate::ui_storage::Storage,
    network_ui: crate::ui_network::Panel,
    wake_ui: crate::ui_wake::Panel,
    preferences_ui: crate::ui_preferences::Panel,
    logs_ui: crate::ui_logs::Panel,
    firmware_ui: crate::ui_firmware::Panel,
    rename_ui: Option<crate::ui_rename::Dialog>,
    native_window: Option<isize>,
    capture: Option<keyboard::Capture>,
    capture_epoch: Option<u64>,
    capture_gate: Option<keyboard::Gate>,
    special_keys: bool,
    capture_reported_counts: (u64, u64, u64),
    capture_reported_at: std::time::Instant,
    capture_reported: Option<(
        minikmb_native::keyboard_capture::Phase,
        u64,
        bool,
        Option<u32>,
    )>,
}
impl Desktop {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        app: Arc<AppService>,
        runtime: tokio::runtime::Runtime,
        screenshot: Option<std::path::PathBuf>,
        window_input: Arc<minikmb_native::window_input::WindowInput>,
        benchmark: Option<crate::ui_benchmark::Session>,
        layout_preview: Option<crate::ui_preview::Layout>,
    ) -> Self {
        theme::apply(&cc.egui_ctx);
        if let Some(layout) = layout_preview {
            layout.begin(&cc.egui_ctx);
        }
        if let (Some(benchmark), Some(gpu)) = (&benchmark, &cc.wgpu_render_state) {
            let info = gpu.adapter.get_info();
            let metadata = serde_json::json!({"adapter":info.name,"vendor":info.vendor,
                "device":info.device,"backend":format!("{:?}",info.backend),
                "device_type":format!("{:?}",info.device_type),"requested_memory_hint":"MemoryUsage"});
            if let Err(error) = std::fs::write(
                benchmark.directory.join("renderer.json"),
                metadata.to_string(),
            ) {
                eprintln!("Benchmark renderer metadata failed: {error}");
            }
        }
        let preview = screenshot.is_some() && std::env::args().any(|a| a == "--style-preview");
        let servers = minikmb_native::remote::RemoteServers::new(app.clone());
        let (ipc, ipc_error) = if screenshot.is_none() {
            match runtime.block_on(minikmb_native::ipc::Server::start(
                app.clone(),
                &app.profile_dir_for_ipc(),
            )) {
                Ok(server) => (Some(server), None),
                Err(error) => (
                    None,
                    Some(format!("CLI IPC를 시작하지 못했습니다: {error}")),
                ),
            }
        } else {
            (None, None)
        };
        let action_result = Arc::new(parking_lot::Mutex::new(ipc_error));
        let input =
            minikmb_native::ui_input::UiInput::new(&app, runtime.handle(), action_result.clone());
        let native_window = if screenshot.is_none() {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            cc.window_handle().ok().and_then(|w| match w.as_raw() {
                RawWindowHandle::Win32(w) => Some(w.hwnd.get()),
                _ => None,
            })
        } else {
            None
        };
        Self {
            app,
            runtime,
            page: if screenshot.is_some()
                && std::env::args().any(|a| a == "--video-ui-probe" || a == "--control-preview")
            {
                Page::Control
            } else if preview && std::env::args().any(|arg| arg == "--firmware-preview") {
                Page::Firmware
            } else if preview && std::env::args().any(|arg| arg == "--logs-preview") {
                Page::Logs
            } else if preview && std::env::args().any(|arg| arg == "--diagnostics-preview") {
                Page::Diagnostics
            } else if preview && std::env::args().any(|arg| arg == "--preferences-preview") {
                Page::Preferences
            } else if preview && std::env::args().any(|arg| arg == "--keys-preview") {
                Page::Keys
            } else if preview && std::env::args().any(|a| a == "--api-preview") {
                Page::ApiDocs
            } else if preview && std::env::args().any(|a| a == "--mcp-preview") {
                Page::McpGuide
            } else if preview {
                Page::StylePreview
            } else {
                Page::Channels
            },
            filter: String::new(),
            key_name: String::new(),
            new_key: None,
            message: String::new(),
            notice_timer: Default::default(),
            reference_ui: if preview {
                crate::ui_reference::Panel::preview()
            } else {
                Default::default()
            },
            servers,
            ipc,
            screenshot: screenshot.clone(),
            benchmark,
            layout_preview,
            paints: 0,
            screenshot_requested: false,
            preview_started: std::time::Instant::now(),
            action_result,
            video_mode: Default::default(),
            manual_video: false,
            texture: None,
            displayed_frame: String::new(),
            input,
            video_focused: false,
            video_rect: None,
            video_info: None,
            video_guard: None,
            input_owner: None,
            input_epoch: 0,
            window_input,
            normal_policy: Default::default(),
            normal_wheel: [0; 2],
            buttons: Default::default(),
            pointer: None,
            virtual_open: true,
            virtual_mods: Default::default(),
            virtual_rows: minikmb_native::keyboard_layout::rows(),
            virtual_choices: minikmb_native::keyboard_layout::chord_keys(),
            virtual_choice: 76,
            virtual_rect: None,
            pending_preset: None,
            rendered_active: Default::default(),
            terminal: if preview && std::env::args().any(|a| a == "--tty-preview") {
                crate::ui_tty::Terminal::preview()
            } else {
                Default::default()
            },
            native_window,
            preferences_ui: crate::ui_preferences::Panel::new(preview),
            logs_ui: crate::ui_logs::Panel::new(preview),
            firmware_ui: crate::ui_firmware::Panel::new(screenshot.is_some()),
            rename_ui: None,
            wake_ui: if preview && std::env::args().any(|arg| arg == "--wake-preview") {
                crate::ui_wake::Panel::preview()
            } else {
                Default::default()
            },
            network_ui: if preview
                && std::env::args().any(|a| {
                    a == "--network-preview" || a == "--scan-preview" || a == "--forward-preview"
                }) {
                crate::ui_network::Panel::preview()
            } else {
                Default::default()
            },
            storage_ui: if preview && std::env::args().any(|a| a == "--folder-preview") {
                crate::ui_storage::Storage::preview_folder()
            } else if preview && std::env::args().any(|a| a == "--card-format-preview") {
                crate::ui_storage::Storage::preview_card_format()
            } else if preview
                && std::env::args().any(|a| a == "--format-preview" || a == "--create-preview")
            {
                crate::ui_storage::Storage::preview_format(
                    std::env::args().any(|a| a == "--format-preview"),
                )
            } else if preview && std::env::args().any(|a| a == "--upload-preview") {
                crate::ui_storage::Storage::preview_upload()
            } else if preview
                && std::env::args().any(|a| a == "--disk-preview" || a == "--sector-preview")
            {
                crate::ui_storage::Storage::preview_disk(
                    std::env::args().any(|a| a == "--sector-preview"),
                )
            } else if preview
                && std::env::args().any(|a| a == "--storage-preview" || a == "--metadata-preview")
            {
                crate::ui_storage::Storage::preview(
                    std::env::args().any(|a| a == "--metadata-preview"),
                )
            } else {
                Default::default()
            },
            capture: None,
            capture_epoch: None,
            capture_gate: None,
            special_keys: false,
            capture_reported_counts: (0, 0, 0),
            capture_reported_at: std::time::Instant::now(),
            capture_reported: None,
        }
    }
    fn navigate(&mut self, page: Page) {
        if self.page == page {
            return;
        }
        self.clear_input();
        self.input.stop();
        // A completed request from the view we left must not resurrect its
        // dismissed notification in the new view. Its cleanup still runs.
        self.message.clear();
        self.notice_timer.reset();
        self.action_result = Arc::new(parking_lot::Mutex::new(None));
        self.input = minikmb_native::ui_input::UiInput::new(
            &self.app,
            self.runtime.handle(),
            self.action_result.clone(),
        );
        self.page = page;
    }
    fn select(&mut self, uid: Option<Uid>) {
        let app = self.app.clone();
        let message = self.action_result.clone();
        let open = uid.is_some();
        self.runtime.spawn(async move {
            let result = async {
                let mode = uid
                    .as_ref()
                    .and_then(|uid| app.preferences.channel(uid).video)
                    .unwrap_or_default();
                let selected = app.select(uid).await?;
                if open {
                    let binding = minikmb_native::active::Binding {
                        uid: selected.uid.ok_or_else(|| {
                            minikmb_native::model::AppError::new(
                                "NO_ACTIVE_CHANNEL",
                                "활성 채널이 없습니다.",
                            )
                        })?,
                        generation: selected.generation,
                    };
                    app.open_pinned_video(binding, mode).await?;
                }
                Ok::<_, minikmb_native::model::AppError>(())
            }
            .await;
            if let Err(e) = result {
                *message.lock() = Some(e.message);
            }
        });
    }
    fn video_action(&self, open: bool) {
        let app = self.app.clone();
        let message = self.action_result.clone();
        let mode = self.video_mode;
        let binding = self.rendered_binding();
        self.runtime.spawn(async move {
            let result = async {
                let binding = binding?;
                if open {
                    app.open_pinned_video(binding, mode).await.map(|_| ())
                } else {
                    app.close_pinned_video(binding).await
                }
            }
            .await;
            if let Err(e) = result {
                *message.lock() = Some(e.message);
            }
        });
    }
    fn video_toolbar(&mut self, ui: &mut egui::Ui, video: &minikmb_native::video::VideoStatus) {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("video-mode")
                .selected_text(if self.video_mode.width == 0 {
                    "자동 모드"
                } else {
                    "수동 모드"
                })
                .width(100.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.video_mode.width == 0, "자동 모드")
                        .clicked()
                    {
                        self.video_mode = Default::default();
                    }
                    if ui
                        .selectable_label(self.video_mode.width > 0, "수동 설정…")
                        .clicked()
                    {
                        if self.video_mode.width == 0 {
                            self.video_mode = minikmb_native::media::CaptureMode {
                                width: 1920,
                                height: 1080,
                                fps: 30.0,
                            };
                        }
                        self.manual_video = true;
                    }
                });
            if ui.button("영상 열기").clicked() {
                self.video_action(true);
            }
            if ui.button("가상 키보드").clicked() {
                self.virtual_open = !self.virtual_open;
                self.virtual_mods.clear();
            }
            if ui.add_enabled(self.native_window.is_some() && self.input_binding().is_some(),egui::Checkbox::new(&mut self.special_keys,"특수 키 전송")).on_hover_text("켜면 영상 입력으로 돌아가 Windows·한영 키 등을 호스트에서 차단하고 타깃으로 전달합니다.").changed(){
                self.clear_input();
                if self.special_keys {
                    if self.capture.as_ref().is_some_and(|c|c.finished()){self.capture=None;}
                    if self.capture.is_none(){
                        match keyboard::Capture::new(self.native_window.unwrap(),self.app.active.clone()) {Ok(c)=>self.capture=Some(c),Err(e)=>{self.special_keys=false;self.message=e.message;}}
                    }
                    if let Some(c)=&self.capture{c.retry();}
                }
                // Like v1's stage.focus(): clicking the toolbar must not leave
                // the mode checked while capture waits for another video click.
                if self.special_keys {
                    self.video_focused = true;
                    self.sync_capture();
                }
            }
            if self.special_keys {
                if let Some(c)=&self.capture {let state=c.status();
                    let tint=if state.active(){color::SUCCESS}else{color::WARNING};
                    ui.label(RichText::new(state.phase.label()).small().color(tint)).on_hover_text(format!("앱 내부 스레드: {:?}\n설치: {} · 활성 세대: {} / 요청 {}\nLL 콜백: {} · 차단: {} · GUI 콜백: {} · 차단: {} · 앱 포커스: {}\n수신: {} · 입력 큐: {} · 폐기: {}\nWin32 오류: {:?}",state.native_thread_id,state.installed,state.armed_epoch,state.requested_epoch,state.callbacks,state.swallowed,state.window_callbacks,state.window_swallowed,state.foreground,state.received_events,state.queued_events,state.rejected_events,state.win32_error));
                }
            }
            if ui.button("시리얼 중계").clicked() && self.screenshot.is_none() {
                if let Ok(b) = self.rendered_binding() {
                    self.clear_input();
                    self.terminal.toggle(b);
                }
            }
            for(label,page) in [("디스크 공유",crate::ui_storage::Page::Images),("채널 정보",crate::ui_storage::Page::Metadata)]{
                if ui.button(label).clicked()&&self.screenshot.is_none(){if let Ok(b)=self.rendered_binding(){self.clear_input();let name=self.app.inventory.read().channels.iter().find(|c|c.uid==b.uid).map(|c|c.name.clone()).unwrap_or_default();self.storage_ui.show_page(b,page,name);}}
            }
            if ui.button("네트워크 · 포워딩").clicked()&&self.screenshot.is_none(){if let Ok(b)=self.rendered_binding(){self.clear_input();self.network_ui.toggle(b,&self.app);}}
            if ui.button("PC 깨우기").clicked()&&self.screenshot.is_none(){if let Ok(b)=self.rendered_binding(){self.clear_input();self.wake_ui.toggle(b,&self.app,self.runtime.handle());}}
            if ui.button("펌웨어").clicked(){self.clear_input();self.navigate(Page::Firmware);}
            if ui.button("진단").clicked(){self.clear_input();self.navigate(Page::Diagnostics);}
            if ui.button("동작 로그").clicked(){self.clear_input();self.navigate(Page::Logs);}
            if ui
                .add_enabled(video.running, egui::Button::new("중지"))
                .clicked()
            {
                self.video_action(false);
            }
            if self.video_mode.width > 0 && ui.button("W / H / Hz").clicked() {
                self.manual_video = true;
            }
            if let Some(f) = video.frame.as_ref().filter(|_| video.running) {
                theme::inline_metric(ui, format!("{}×{} · {:.2}Hz", f.width, f.height, f.fps));
            }
            if !video.notice.is_empty() {
                ui.colored_label(color::WARNING, &video.notice);
            }
        });
    }
    fn video_view(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snapshot: &minikmb_native::service::Snapshot,
    ) {
        if snapshot.active.uid.is_none() {
            self.texture = None;
            self.displayed_frame.clear();
            theme::empty(
                ui,
                "활성 채널이 없습니다.",
                "채널 목록에서 제어할 채널을 선택하세요.",
            );
            return;
        }
        let frame = self.app.current_frame().ok().filter(|f| {
            Some(&f.info.uid) == snapshot.active.uid.as_ref()
                && f.info.generation == snapshot.active.generation
        });
        if let Some(frame) = frame {
            self.video_info = Some(frame.info.clone());
            self.video_guard = Some(frame.life.clone());
            if self.displayed_frame != frame.info.frame_id {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [frame.info.width as usize, frame.info.height as usize],
                    &frame.data.rgba,
                );
                if let Some(texture) = self.texture.as_mut() {
                    texture.set(image, egui::TextureOptions::LINEAR);
                } else {
                    self.texture =
                        Some(ctx.load_texture("active-hdmi", image, egui::TextureOptions::LINEAR));
                }
                self.displayed_frame = frame.info.frame_id;
            }
        } else {
            self.texture = None;
            self.displayed_frame.clear();
            self.video_info = None;
            self.video_guard = None;
            self.video_rect = None;
            self.clear_input();
        }
        let space = ui.available_size().max(egui::vec2(1.0, 1.0));
        let (rect, _) = ui.allocate_exact_size(space, egui::Sense::hover());
        ui.painter().rect_filled(rect, 0, color::VIDEO);
        if let Some(texture) = &self.texture {
            let source = texture.size_vec2();
            let fit = source * (space.x / source.x).min(space.y / source.y);
            let video = egui::Rect::from_center_size(rect.center(), fit);
            self.video_rect = Some(video);
            ui.painter().image(
                texture.id(),
                video,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            let text = if !snapshot.video.error.is_empty() {
                snapshot.video.error.as_str()
            } else if snapshot.video.running {
                "첫 영상 프레임을 기다리고 있습니다."
            } else {
                "상단의 영상 열기를 눌러 영상을 확인하세요."
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(14.0),
                color::MUTED,
            );
        }
    }
    fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
        theme::heading(ui, title, subtitle);
    }
    fn clear_input(&mut self) {
        self.window_input.clear();
        self.normal_policy = Default::default();
        self.normal_wheel = [0; 2];
        if let Some(c) = &self.capture {
            c.disarm();
        }
        self.capture_epoch = None;
        self.capture_gate = None;
        if let Some(owner) = self.input_owner.take() {
            self.input.cancel(&owner);
        }
        self.video_focused = false;
        self.buttons.clear();
    }
    fn clear_input_reason(&mut self, code: &'static str) {
        if self.capture_epoch.is_some() {
            self.report_capture();
            self.app.logs.emit(
                minikmb_native::logging::Level::Info,
                minikmb_native::logging::Event::KeyboardCaptureChanged,
                minikmb_native::logging::Fields {
                    uid: self.capture_gate.as_ref().map(|g| g.binding.uid.clone()),
                    code: Some(code),
                    ..Default::default()
                },
            );
        }
        self.clear_input();
    }
    fn rendered_binding(
        &self,
    ) -> Result<minikmb_native::active::Binding, minikmb_native::model::AppError> {
        if self.rendered_active.switching {
            return Err(minikmb_native::model::AppError::new(
                "CHANNEL_SWITCHING",
                "채널을 전환하고 있습니다.",
            ));
        }
        let uid = self.rendered_active.uid.clone().ok_or_else(|| {
            minikmb_native::model::AppError::new("NO_ACTIVE_CHANNEL", "활성 채널이 없습니다.")
        })?;
        Ok(minikmb_native::active::Binding {
            uid,
            generation: self.rendered_active.generation,
        })
    }
    fn input_binding(&self) -> Option<minikmb_native::active::Binding> {
        let info = self.video_info.as_ref()?;
        let b = minikmb_native::active::Binding {
            uid: info.uid.clone(),
            generation: info.generation,
        };
        self.app.active.validate(&b).ok()?;
        self.app.validate_frame_reference(&b, &info.frame_id).ok()?;
        Some(b)
    }
    fn owner(
        &mut self,
    ) -> Option<(
        minikmb_native::input::Owner,
        minikmb_native::active::Binding,
    )> {
        if !self.video_focused {
            return None;
        }
        self.source_owner()
    }
    fn input_notice(&mut self, error: minikmb_native::model::AppError) {
        if minikmb_native::ui_input::show_failure(&error) {
            self.message = error.message;
        }
    }
    fn source_owner(
        &mut self,
    ) -> Option<(
        minikmb_native::input::Owner,
        minikmb_native::active::Binding,
    )> {
        if self.screenshot.is_some() {
            return None;
        }
        let b = self.input_binding()?;
        if self.input_owner.as_ref().is_some_and(|o| {
            o.cancelled.is_cancelled()
                || o.command_guard.as_ref().is_some_and(|g| g.is_cancelled())
                || (o.command_guard.is_none() && self.video_guard.is_some())
        }) {
            self.clear_input();
            return None;
        }
        if self.input_owner.is_none() {
            self.input_epoch += 1;
            self.input_owner = Some(minikmb_native::input::Owner {
                pin: Default::default(),
                id: format!("ui:{}", self.input_epoch),
                cancelled: tokio_util::sync::CancellationToken::new(),
                command_guard: self.video_guard.clone(),
            });
        }
        let owner = self.input_owner.as_ref()?.clone();
        // Pin before handing the owner to the native keyboard event queue.
        if let Err(e) = self.app.input.prepare_owner(&owner, &b) {
            self.input_notice(e);
            self.clear_input();
            self.special_keys = false;
            return None;
        }
        Some((owner, b))
    }
    fn send_input(&mut self, action: minikmb_native::input::Action) {
        if let Some((owner, b)) = self.owner() {
            if let Err(e) = self.input.action(owner, b, action) {
                self.input_notice(e);
                self.clear_input();
                self.special_keys = false;
            }
        }
    }
    fn captured_keys(&mut self) {
        self.report_capture();
        if self.capture.as_ref().is_some_and(|c| c.failed()) {
            self.capture.take();
            self.clear_input();
            self.special_keys = false;
            let status = self.app.keyboard_capture.read().clone();
            self.message = status
                .map(|s| {
                    format!(
                        "{} · Win32 오류 {:?}. 눌린 입력을 해제했습니다.",
                        s.phase.label(),
                        s.win32_error
                    )
                })
                .unwrap_or_else(|| "특수 키 전달이 중단되어 눌린 입력을 해제했습니다.".into());
        }
        let events = self
            .capture
            .as_ref()
            .map(|c| c.events())
            .unwrap_or_default();
        for event in events {
            if let minikmb_native::keyboard_capture::Event::HostUp { vk, scan, extended } = event {
                self.normal_policy.host_up(vk, scan, extended);
                continue;
            }
            if let Some(gate) = self
                .capture_gate
                .clone()
                .filter(|_| self.special_keys && self.video_focused)
            {
                match gate.dispatch(&self.app.active, &self.input, event) {
                    Ok(true) => self.clear_input(),
                    Ok(false) => {}
                    Err(e) => {
                        self.clear_input();
                        self.special_keys = false;
                        self.input_notice(e);
                    }
                }
            }
        }
    }
    fn sync_capture(&mut self) {
        if !self.special_keys || !self.video_focused {
            return;
        }
        let Some((owner, binding)) = self.owner() else {
            self.clear_input();
            return;
        };
        if self.capture_epoch == Some(self.input_epoch) {
            return;
        }
        if let Some(c) = &self.capture {
            let gate = keyboard::Gate {
                epoch: self.input_epoch,
                binding,
                owner,
            };
            match c.arm(gate.clone()) {
                Ok(()) => {
                    self.capture_epoch = Some(self.input_epoch);
                    self.capture_gate = Some(gate);
                }
                Err(e) => {
                    self.clear_input();
                    self.special_keys = false;
                    self.input_notice(e);
                }
            }
        }
    }
    fn report_capture(&mut self) {
        let Some(c) = &self.capture else {
            return;
        };
        let status = c.status();
        let marker = (
            status.phase,
            status.requested_epoch,
            status.installed,
            status.win32_error,
        );
        let counts = (status.callbacks, status.swallowed, status.queued_events);
        if self.capture_reported != Some(marker)
            || (counts != self.capture_reported_counts
                && self.capture_reported_at.elapsed() >= std::time::Duration::from_secs(1))
        {
            self.capture_reported = Some(marker);
            self.capture_reported_counts = counts;
            self.capture_reported_at = std::time::Instant::now();
            self.app.logs.emit(
                minikmb_native::logging::Level::Info,
                minikmb_native::logging::Event::KeyboardCaptureChanged,
                minikmb_native::logging::Fields {
                    uid: self.capture_gate.as_ref().map(|g| g.binding.uid.clone()),
                    generation: self.capture_gate.as_ref().map(|g| g.binding.generation),
                    code: Some(status.phase.code()),
                    success: Some(status.active()),
                    enabled: Some(status.installed),
                    count: Some(status.callbacks),
                    swallowed: Some(status.swallowed),
                    window_callbacks: Some(status.window_callbacks),
                    window_swallowed: Some(status.window_swallowed),
                    reentrant_callbacks: Some(status.reentrant_callbacks),
                    foreground: Some(status.foreground),
                    os_error: status.win32_error,
                    helper_pid: status.helper_pid,
                    native_thread_id: status.native_thread_id,
                    received_events: Some(status.received_events),
                    queued_events: Some(status.queued_events),
                    rejected_events: Some(status.rejected_events),
                    ..Default::default()
                },
            );
        }
        *self.app.keyboard_capture.write() = Some(status);
    }
    fn move_input(&mut self, pos: egui::Pos2, clamp: bool) {
        let (Some(rect), Some(info)) = (self.video_rect, self.video_info.clone()) else {
            return;
        };
        if !clamp && !rect.contains(pos) {
            return;
        }
        let Some((mut owner, b)) = self.owner() else {
            return;
        };
        let x = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0)
            * info.width.saturating_sub(1) as f32;
        let y = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0)
            * info.height.saturating_sub(1) as f32;
        match self.app.pixel_action(
            &b,
            minikmb_native::active::PixelPoint {
                x: f64::from(x),
                y: f64::from(y),
            },
            Some(&info.frame_id),
        ) {
            Ok((action, guard)) => {
                owner.command_guard = Some(guard);
                if let Err(e) = self.input.action(owner, b, action) {
                    self.input_notice(e);
                    self.clear_input();
                }
            }
            Err(e) => {
                self.input_notice(e);
                self.clear_input();
            }
        }
    }
    fn copy_secret(&mut self) {
        if let Some(token) = &self.new_key {
            match arboard::Clipboard::new().and_then(|mut c| c.set_text(token.expose())) {
                Ok(()) => self.message = "키를 클립보드에 복사했습니다.".into(),
                Err(_) => self.message = "클립보드 복사에 실패했습니다. 다시 복사하세요.".into(),
            }
        }
    }
    fn send_virtual(&mut self, binding: minikmb_native::active::Binding, keys: Vec<u16>) {
        if self.screenshot.is_some() {
            return;
        }
        self.clear_input();
        self.input_epoch += 1;
        let owner = minikmb_native::input::Owner {
            pin: Default::default(),
            id: format!("ui:{}", self.input_epoch),
            cancelled: tokio_util::sync::CancellationToken::new(),
            command_guard: self.video_guard.clone(),
        };
        self.input_owner = Some(owner.clone());
        if let Err(e) = self.input.chord(owner, binding, keys) {
            self.input_notice(e);
        }
    }
    fn virtual_keyboard(
        &mut self,
        ctx: &egui::Context,
        snapshot: &minikmb_native::service::Snapshot,
    ) {
        self.virtual_rect = None;
        if self.page != Page::Control || !self.virtual_open {
            return;
        }
        let binding = snapshot
            .active
            .uid
            .as_ref()
            .map(|uid| minikmb_native::active::Binding {
                uid: uid.clone(),
                generation: snapshot.active.generation,
            });
        let mut open = self.virtual_open;
        let mut chosen = None;
        let mut preset = None;
        let shown=theme::tool_window(ctx,"가상 키보드","virtual-keyboard",[1040.0,380.0]).hscroll(false).open(&mut open).resizable(true).default_pos(egui::pos2(110.0,440.0)).show(ctx,|ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui,|ui| {
                ui.add_enabled_ui(binding.is_some()&&!snapshot.active.switching,|ui| {
                    ui.horizontal_wrapped(|ui|{for (label,keys) in minikmb_native::keyboard_layout::PRESETS {if ui.small_button(*label).clicked(){preset=Some((label.to_string(),keys.to_vec()));}}});ui.separator();
                    ui.horizontal_wrapped(|ui|{for (label,usage) in [("Ctrl",224),("Alt",226),("Shift",225),("Win",227)] {if ui.selectable_label(self.virtual_mods.contains(&usage),label).clicked() && !self.virtual_mods.remove(&usage){self.virtual_mods.insert(usage);}}if ui.small_button("보조키 해제").clicked(){self.virtual_mods.clear();}ui.label(RichText::new("보조키는 다음 키에 한 번 적용").small().color(color::MUTED));});
                    if let Some(custom)=virtual_keys::custom(ui,&mut self.virtual_choice,&self.virtual_choices,&self.virtual_mods) { preset=Some(custom); }
                    for row in &self.virtual_rows {
                        if let Some(usage)=virtual_keys::row(ui,row).inner { chosen=Some(usage); }
                    }
                });
                ui.label(RichText::new("현재 STM32 프로토콜은 System Control 전원 보고서를 지원하지 않습니다.").small().color(color::MUTED));
            });
        });
        self.virtual_open = open;
        if let Some(shown) = shown {
            self.virtual_rect = Some(shown.response.rect);
        }
        if let (Some((label, keys)), Some(b)) = (preset, binding.clone()) {
            self.clear_input();
            self.pending_preset = Some((label, keys, b));
        }
        if let (Some(usage), Some(b)) = (chosen, binding) {
            let mut keys: Vec<_> = [224, 226, 225, 227]
                .into_iter()
                .filter(|u| self.virtual_mods.contains(u))
                .collect();
            keys.push(usage);
            self.virtual_mods.clear();
            self.send_virtual(b, keys);
        }
    }
}
impl eframe::App for Desktop {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if self.screenshot.is_some() {
            raw.events
                .retain(|e| matches!(e, egui::Event::Screenshot { .. }));
            return;
        }
        if self.rename_ui.is_some()
            || self.pending_preset.is_some()
            || self.manual_video
            || self.terminal.open
            || self.storage_ui.open
            || self.network_ui.open
            || self.wake_ui.open
        {
            self.clear_input_reason("HOOK_UI_PANEL");
            return;
        }
        if !raw.focused {
            self.clear_input_reason("HOOK_UI_BLUR");
            return;
        }
        if self.page != Page::Control || self.input_binding().is_none() {
            self.clear_input_reason("HOOK_UI_SOURCE_UNAVAILABLE");
            return;
        }
        self.normal_events(_ctx, raw);
        self.captured_keys();
        if self.video_focused && self.special_keys {
            self.sync_capture();
        }
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let action_message = self.action_result.lock().take();
        if let Some(message) = action_message {
            self.message = message;
        }
        self.paints += 1;
        if self.paints == 2 {
            if let Some(layout) = self.layout_preview {
                layout.resize(ctx);
            }
        }
        let preview_geometry_ready = self
            .layout_preview
            .is_none_or(|layout| self.paints >= 6 && layout.matches(ctx));
        if self.layout_preview.is_some()
            && self.preview_started.elapsed() > std::time::Duration::from_secs(8)
            && !self.screenshot_requested
        {
            eprintln!("Layout preview geometry did not settle");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.benchmark.as_ref().is_some_and(|b| b.should_close()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let live_preview =
            self.screenshot.is_some() && std::env::args().any(|a| a == "--video-ui-probe");
        if self.screenshot.is_some()
            && self.paints >= 4
            && !self.screenshot_requested
            && preview_geometry_ready
            && (!live_preview || self.app.current_frame().is_ok())
        {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        if live_preview
            && !self.screenshot_requested
            && self.preview_started.elapsed() > std::time::Duration::from_secs(12)
        {
            eprintln!("Video UI probe timed out before first frame");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(path) = &self.screenshot {
            let captured = ctx.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(picture) = captured {
                let bytes: Vec<u8> = picture.pixels.iter().flat_map(|p| p.to_array()).collect();
                let saved = image::save_buffer(
                    path,
                    &bytes,
                    picture.width() as u32,
                    picture.height() as u32,
                    image::ColorType::Rgba8,
                );
                if let Err(e) = &saved {
                    eprintln!("Screenshot failed: {e}");
                }
                if let Some(layout) = self.layout_preview {
                    let metadata = serde_json::json!({"scope":"controlled egui layout scale, not OS DPI transition",
                        "logical_size":layout.size,"requested_scale":layout.scale,"actual_scale":ctx.pixels_per_point(),
                        "native_scale":ctx.native_pixels_per_point(),"viewport_size":ctx.input(|i| { let size=i.viewport_rect().size(); [size.x,size.y] }),
                        "pixels":[picture.width(),picture.height()],"hardware_disabled":true});
                    if let Err(error) =
                        std::fs::write(path.with_extension("json"), metadata.to_string())
                    {
                        eprintln!("Layout preview metadata failed: {error}");
                    }
                }
                if let Some(b) = &self.benchmark {
                    if saved.is_err()
                        || b.ready(picture.width(), picture.height(), ctx.pixels_per_point())
                            .is_err()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if ctx.input(|i| i.viewport().close_requested())
            && self.app.firmware.current().is_some_and(|j| !j.can_cancel)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.message = "펌웨어 기록·검증 완료 후 닫아주세요.".into();
        }
        let snapshot = self.app.snapshot();
        self.storage_ui.native_window = self.native_window;
        if self.rendered_active != snapshot.active {
            self.clear_input();
            self.special_keys = false;
            self.video_mode = snapshot
                .active
                .uid
                .as_ref()
                .and_then(|uid| self.app.preferences.channel(uid).video)
                .unwrap_or_default();
            self.manual_video = false;
        }
        self.rendered_active = snapshot.active.clone();
        ctx.request_repaint_after(std::time::Duration::from_millis(
            if self.page == Page::Control && snapshot.video.running {
                16
            } else {
                250
            },
        ));
        let toolbar = egui::TopBottomPanel::top("top")
            .min_height(size::HEADER)
            .frame(
                egui::Frame::new()
                    .fill(color::TOOLBAR)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .stroke(egui::Stroke::new(1.0, color::BORDER)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    let active =
                        snapshot.active.uid.as_ref().and_then(|uid| {
                            snapshot.inventory.channels.iter().find(|c| &c.uid == uid)
                        });
                    if let Some(c) = active {
                        let title = if c.name.is_empty() {
                            format!(
                                "채널 {}",
                                c.slot.map(|n| n.to_string()).unwrap_or("—".into())
                            )
                        } else {
                            c.name.clone()
                        };
                        crate::ui_rename::title_label(ui, &title, 310.0)
                            .on_hover_text(format!("{title}\n{}", c.uid.display()));
                        if self.page == Page::Control && crate::ui_rename::edit_button(ui).clicked()
                        {
                            self.clear_input();
                            self.rename_ui =
                                Some(crate::ui_rename::Dialog::new(c.uid.clone(), c.name.clone()));
                        }
                        if ui.available_width() > 900.0 {
                            ui.label(RichText::new(c.uid.display()).small().color(color::MUTED));
                        }
                    } else {
                        ui.label(RichText::new("MiniKMB").size(17.0).strong());
                        ui.separator();
                        ui.label(RichText::new("채널 관리").color(color::MUTED));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("외부 제어").clicked() {
                            self.navigate(Page::Keys);
                        }
                        if ui.button("장치 검색").clicked() {
                            self.app.request_rescan();
                        }
                        if ui.button("채널 목록").clicked() {
                            self.navigate(Page::Channels);
                        }
                    });
                });
                if self.page == Page::Control {
                    self.video_toolbar(ui, &snapshot.video);
                }
            });
        theme::toolbar_bottom(ctx, toolbar.response.rect.bottom());
        if self.page != Page::Control {
            egui::SidePanel::left("navigation")
                .exact_width(size::SIDEBAR)
                .resizable(false)
                .frame(egui::Frame::new().fill(color::NAVIGATION).inner_margin(8))
                .show(ctx, |ui| {
                    theme::group_label(ui, "작업 공간");
                    use minikmb_native::assets::Icon;
                    for (page, icon, label) in [
                        (Page::Channels, Icon::Channels, "채널"),
                        (Page::Control, Icon::Control, "원격 제어"),
                        (Page::Keys, Icon::Keys, "서버 · API 키"),
                        (Page::ApiDocs, Icon::ApiDocs, "REST API 문서"),
                        (Page::McpGuide, Icon::McpGuide, "MCP 연동 안내"),
                        (Page::Preferences, Icon::Preferences, "설정"),
                        (Page::Diagnostics, Icon::Diagnostics, "진단"),
                        (Page::Firmware, Icon::Preferences, "펌웨어"),
                        (Page::Logs, Icon::Logs, "동작 로그"),
                    ] {
                        if theme::navigation_icon(ui, self.page == page, icon, label).clicked() {
                            self.navigate(page);
                        }
                    }
                    ui.add_space(16.0);
                    ui.separator();
                    ui.label(format!(
                        "연결된 채널 {}개",
                        snapshot.inventory.channels.len()
                    ));
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(color::WORKSPACE)
                    .inner_margin(size::PANEL_PADDING),
            )
            .show(ctx, |ui| {
                if self.page != Page::Preferences {
                    if let Some(error) = snapshot
                        .preferences
                        .blocked
                        .as_ref()
                        .or(snapshot.preferences.error.as_ref())
                    {
                        ui.horizontal_wrapped(|ui| {
                            theme::status(ui, theme::Tone::Warning, &error.message);
                            if ui.button("설정 확인").clicked() {
                                self.clear_input();
                                self.navigate(Page::Preferences);
                            }
                        });
                        ui.separator();
                    }
                }
                if let Some(hold) = &snapshot.storage_card_format {
                    theme::status(
                        ui,
                        theme::Tone::Warning,
                        &format!("{} · {}", hold.phase, hold.uid.display()),
                    );
                    ui.label(
                        RichText::new("원래 채널의 SD 정보에서 포맷 상태를 확인할 수 있습니다.")
                            .small()
                            .color(color::MUTED),
                    );
                    ui.separator();
                }
                match self.page {
                    Page::Channels => {
                        Self::heading(ui, "채널", "연결된 보드를 찾고 제어할 채널을 선택하세요.");
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.filter)
                                    .hint_text("이름 또는 UID 검색")
                                    .desired_width(300.0),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{}개 채널",
                                    snapshot.inventory.channels.len()
                                ))
                                .small()
                                .color(color::MUTED),
                            );
                        });
                        ui.add_space(12.0);
                        if !snapshot.inventory.error.is_empty() {
                            theme::status(ui, theme::Tone::Warning, &snapshot.inventory.error);
                        }
                        if snapshot.inventory.channels.is_empty() {
                            theme::empty(
                                ui,
                                "연결된 채널이 없습니다.",
                                "보드의 관리 USB를 연결하고 장치를 검색하세요.",
                            );
                        }
                        let observed: Vec<_> = snapshot
                            .inventory
                            .channels
                            .iter()
                            .map(|c| c.uid.clone())
                            .collect();
                        let mut moved = None;
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for c in &snapshot.inventory.channels {
                                let filter = self.filter.to_lowercase();
                                if !c.name.to_lowercase().contains(&filter)
                                    && !c.uid.raw().to_lowercase().contains(&filter)
                                {
                                    continue;
                                }
                                let card = ui
                                    .push_id(("channel-card", c.uid.raw()), |ui| {
                                        theme::panel().show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                channel_order::handle(ui, &c.uid);
                                                let title = if c.name.is_empty() {
                                                    format!(
                                                        "채널 {}",
                                                        c.slot
                                                            .map(|n| n.to_string())
                                                            .unwrap_or("—".into())
                                                    )
                                                } else {
                                                    c.name.clone()
                                                };
                                                crate::ui_rename::title_label(ui, &title, 220.0)
                                                    .on_hover_text(&title);
                                                if ui
                                                    .add_enabled_ui(
                                                        c.connected,
                                                        crate::ui_rename::edit_button,
                                                    )
                                                    .inner
                                                    .clicked()
                                                {
                                                    self.clear_input();
                                                    self.rename_ui =
                                                        Some(crate::ui_rename::Dialog::new(
                                                            c.uid.clone(),
                                                            c.name.clone(),
                                                        ));
                                                }
                                                theme::status(
                                                    ui,
                                                    if c.connected {
                                                        theme::Tone::Success
                                                    } else {
                                                        theme::Tone::Neutral
                                                    },
                                                    if c.connected {
                                                        "연결됨"
                                                    } else {
                                                        "연결 끊김"
                                                    },
                                                );
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        if ui.button("제어하기").clicked() {
                                                            self.navigate(Page::Control);
                                                            self.select(Some(c.uid.clone()));
                                                        }
                                                    },
                                                );
                                            });
                                            ui.label(
                                                RichText::new(c.uid.display())
                                                    .monospace()
                                                    .small()
                                                    .color(color::MUTED),
                                            );
                                            ui.label(match c.target_state {
                                                1 => "타깃 Full Speed",
                                                2 => "타깃 High Speed",
                                                3 => "타깃 절전",
                                                _ => "타깃 미연결",
                                            });
                                            if !c.pairing_error.is_empty() {
                                                theme::status(
                                                    ui,
                                                    theme::Tone::Warning,
                                                    &c.pairing_error,
                                                );
                                            }
                                        })
                                    })
                                    .inner;
                                if let Some(request) =
                                    channel_order::target(ui, card.response.rect, &c.uid)
                                {
                                    moved = Some(request);
                                }
                                ui.add_space(8.0);
                            }
                            channel_order::edge_scroll(ui);
                        });
                        if let Some((source, target, after)) = moved {
                            if let Err(error) = self
                                .app
                                .preferences
                                .move_channel(&source, &target, after, &observed)
                            {
                                self.message = error.message;
                            }
                        }
                    }
                    Page::Preferences => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            self.preferences_ui.show(
                                ui,
                                &self.app,
                                self.runtime.handle(),
                                self.native_window,
                            )
                        });
                    }
                    Page::Logs => {
                        self.logs_ui.logs(ui, &self.app, self.runtime.handle());
                    }
                    Page::Firmware => {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            self.firmware_ui.show(
                                ui,
                                ctx,
                                &self.app,
                                self.runtime.handle(),
                                self.native_window,
                            )
                        });
                    }
                    Page::Diagnostics => {
                        self.logs_ui.diagnostics(ui, &self.app);
                    }
                    Page::Control => {
                        self.video_view(ui, ctx, &snapshot);
                    }
                    Page::Keys => {
                        Self::heading(
                            ui,
                            "서버 · API 키",
                            "외부 제어에 사용할 인증 키를 관리합니다.",
                        );
                        let remote = self.app.remote.read().clone();
                        ui.horizontal(|ui| {
                            if ui
                                .button(if remote.rest.is_some() {
                                    "REST 끄기"
                                } else {
                                    "REST 켜기"
                                })
                                .clicked()
                            {
                                let servers = self.servers.clone();
                                let on = remote.rest.is_none();
                                self.runtime.spawn(async move {
                                    let _ = servers.rest(on, 18180).await;
                                });
                            }
                            if ui
                                .button(if remote.mcp.is_some() {
                                    "MCP 끄기"
                                } else {
                                    "MCP 켜기"
                                })
                                .clicked()
                            {
                                let servers = self.servers.clone();
                                let on = remote.mcp.is_none();
                                self.runtime.spawn(async move {
                                    let _ = servers.mcp(on, 18180, 18181).await;
                                });
                            }
                        });
                        if let Some(addr) = remote.rest {
                            ui.code(format!("REST  http://{addr}/api/v1"));
                        }
                        if let Some(addr) = remote.mcp {
                            ui.code(format!("MCP   http://{addr}/mcp"));
                        }
                        let streams = self.app.events.status();
                        ui.label(format!(
                            "상태 구독 {} / {}",
                            streams.subscriptions, streams.limit
                        ));
                        if !remote.error.is_empty() {
                            theme::status(ui, theme::Tone::Error, &remote.error);
                        }
                        ui.add_space(16.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.key_name).hint_text("키 이름"),
                            );
                            if ui.button("키 만들기").clicked() {
                                match self.app.keys.create(&self.key_name) {
                                    Ok(k) => {
                                        self.new_key = Some(k.token);
                                        self.key_name.clear();
                                        self.copy_secret();
                                    }
                                    Err(e) => self.message = e.message,
                                }
                            }
                        });
                        if self.new_key.is_some() {
                            ui.horizontal(|ui| {
                                if ui.button("다시 복사").clicked() {
                                    self.copy_secret();
                                }
                                if ui.button("생성 화면 닫기").clicked() {
                                    self.new_key = None;
                                }
                            });
                        }
                        ui.add_space(16.0);
                        for key in self.app.keys.list() {
                            ui.horizontal(|ui| {
                                ui.label(&key.name);
                                ui.label(RichText::new(&key.fingerprint).monospace().small());
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("삭제").clicked() {
                                            match self.app.keys.revoke(&key.id) {
                                                Ok(_) => self.message = "키를 삭제했습니다.".into(),
                                                Err(e) => self.message = e.message,
                                            }
                                        }
                                    },
                                );
                            });
                            ui.separator();
                        }
                    }
                    Page::ApiDocs => {
                        self.reference_ui
                            .rest(ui, ctx, &self.app, self.native_window);
                    }
                    Page::McpGuide => {
                        self.reference_ui.guide(ui, ctx, &self.app);
                    }
                    Page::StylePreview => {
                        egui::ScrollArea::vertical().show(ui, theme::preview);
                    }
                }
            });
        if self.page != Page::Keys {
            self.new_key = None;
        }
        self.virtual_keyboard(ctx, &snapshot);
        self.terminal
            .show(ctx, &self.app, self.runtime.handle(), &snapshot.active);
        self.storage_ui
            .show(ctx, &self.app, self.runtime.handle(), &snapshot.active);
        self.network_ui.show(ctx, &self.app, self.runtime.handle());
        self.wake_ui.show(ctx, &self.app, self.runtime.handle());
        if let Some((label, keys, binding)) = self.pending_preset.clone() {
            egui::Modal::new(egui::Id::new("confirm-key-preset")).show(ctx, |ui| {
                ui.heading("키 조합 전송");
                ui.label(format!("{}에 {label}을 전송할까요?", binding.uid.display()));
                ui.horizontal(|ui| {
                    if ui.button("전송").clicked() {
                        self.pending_preset = None;
                        self.virtual_mods.clear();
                        self.send_virtual(binding, keys);
                    }
                    if ui.button("취소").clicked() {
                        self.pending_preset = None;
                    }
                });
            });
        }
        let mut manual = self.manual_video;
        theme::tool_window(ctx, "수동 영상 모드", "manual-video", [460.0, 230.0])
            .open(&mut manual)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("캡처 장치가 실제 지원하는 조합으로 새로 엽니다.");
                egui::Grid::new("manual-video-fields").show(ui, |ui| {
                    ui.label("너비");
                    ui.add(egui::DragValue::new(&mut self.video_mode.width).range(1..=8192));
                    ui.end_row();
                    ui.label("높이");
                    ui.add(egui::DragValue::new(&mut self.video_mode.height).range(1..=8192));
                    ui.end_row();
                    ui.label("주사율 (0 = 기본)");
                    ui.add(
                        egui::DragValue::new(&mut self.video_mode.fps)
                            .range(0.0..=240.0)
                            .speed(0.01),
                    );
                    ui.end_row();
                });
                if ui.button("이 모드로 열기").clicked() {
                    self.video_action(true);
                    self.manual_video = false;
                }
            });
        self.manual_video &= manual;
        if let Some(rename) = &mut self.rename_ui {
            if rename.show(ctx, &self.app, self.runtime.handle()) {
                self.rename_ui = None;
            }
        }
        self.publish_normal(ctx);
        self.report_capture();
        self.notice_timer
            .tick(&mut self.message, std::time::Instant::now());
        notice::show(ctx, &self.message);
    }
}
impl Drop for Desktop {
    fn drop(&mut self) {
        self.clear_input();
        self.capture.take();
        self.input.stop();
        if let Some(ipc) = self.ipc.take() {
            self.runtime.block_on(ipc.shutdown());
        }
        self.app.stop();
        self.runtime.block_on(self.servers.shutdown());
        self.runtime.block_on(self.app.shutdown());
    }
}
