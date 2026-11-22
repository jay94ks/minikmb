use eframe::egui::{self, Color32, RichText};
use frwr_ui::{backend::{self, Event, Worker}, model::*, native, protocol};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, fs, path::{Path, PathBuf}, sync::atomic::Ordering, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

const ACCENT: Color32 = Color32::from_rgb(63, 199, 181);
const MUTED: Color32 = Color32::from_rgb(154, 168, 186);
const PANEL: Color32 = Color32::from_rgb(24, 31, 42);

#[derive(Clone, Copy, PartialEq)]
enum Tab { Dump, Read, Write, Erase, Verify }
impl Tab {
    fn title(self) -> &'static str { match self { Self::Dump => "전체 덤프", Self::Read => "범위 읽기", Self::Write => "파일 쓰기", Self::Erase => "지우기", Self::Verify => "비교 검증" } }
}

#[derive(Serialize, Deserialize)]
struct Settings { data_dir: PathBuf, hz: u32, #[serde(default)] backup: bool, #[serde(default)] model: String }

pub fn default_data_dir() -> PathBuf {
    std::env::current_exe().unwrap_or_default().parent().unwrap_or(Path::new(".")).join("frwr-data")
}

fn settings_file() -> PathBuf {
    std::env::current_exe().unwrap_or_default().parent().unwrap_or(Path::new(".")).join("frwr-settings.json")
}

struct InputFile { path: PathBuf, len: usize, hash: String }
struct Confirmation { request: Request, summary: String, word: &'static str, typed: String }

pub struct App {
    demo: bool,
    demo_flash: backend::DemoFlash,
    ports: Vec<Device>,
    selected: usize,
    chip: Option<Chip>,
    catalog: Vec<Definition>,
    chip_filter: String,
    settings: Settings,
    tab: Tab,
    start: String,
    len: String,
    input_path: String,
    input: Option<InputFile>,
    worker: Option<Worker>,
    phase: String,
    fraction: Option<f32>,
    started: Option<Instant>,
    committed: bool,
    close_after_work: bool,
    logs: VecDeque<String>,
    message: String,
    error: bool,
    last_folder: Option<PathBuf>,
    data: Option<ReadData>,
    confirmation: Option<Confirmation>,
    smoke: Option<PathBuf>,
    smoke_phase: u8,
    smoke_view: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, demo: bool, smoke: Option<PathBuf>, smoke_view: String) -> Self {
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals.panel_fill = Color32::from_rgb(17, 23, 32);
        style.visuals.window_fill = PANEL;
        style.visuals.selection.bg_fill = Color32::from_rgb(29, 99, 94);
        style.spacing.item_spacing = egui::vec2(10.0, 9.0);
        style.spacing.button_padding = egui::vec2(13.0, 8.0);
        style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        cc.egui_ctx.set_style(style);
        if let Ok(bytes) = fs::read("C:/Windows/Fonts/malgun.ttf") {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("korean".into(), egui::FontData::from_owned(bytes).into());
            fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "korean".into());
            fonts.families.entry(egui::FontFamily::Monospace).or_default().push("korean".into());
            cc.egui_ctx.set_fonts(fonts);
        }
        let mut settings = fs::read(settings_file()).ok().and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Settings { data_dir: default_data_dir(), hz: 1000000, backup: false, model: String::new() });
        if let Some(path) = &smoke {
            settings.data_dir = path.parent().unwrap_or(Path::new(".")).join(format!("smoke-data-{}", stamp()));
            settings.model.clear();
        }
        let mut app = Self { demo, demo_flash: backend::demo_flash(), ports: vec![], selected: 0, chip: None,
            catalog: native::catalog(), chip_filter: String::new(),
            settings, tab: Tab::Dump, start: "0x000000".into(), len: "0x1000".into(), input_path: String::new(),
            input: None, worker: None, phase: String::new(), fraction: None, started: None,
            committed: false, close_after_work: false, logs: VecDeque::new(),
            message: "Pico를 선택하고 연결하면 외부 플래시를 식별합니다.".into(), error: false,
            last_folder: None, data: None, confirmation: None, smoke, smoke_phase: 0, smoke_view };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        self.chip = None;
        if self.demo {
            self.ports = vec![Device { port: "DEMO".into(), serial: "SIMULATED".into() }];
        } else {
            match protocol::devices() { Ok(ports) => self.ports = ports, Err(e) => self.fail(format!("포트 조회 실패: {e:#}")) }
        }
        self.selected = self.selected.min(self.ports.len().saturating_sub(1));
    }

    fn fail(&mut self, message: String) { self.message = message; self.error = true; }
    fn push_log(&mut self, text: String) {
        if self.logs.len() >= 600 { self.logs.pop_front(); }
        self.logs.push_back(text);
    }
    fn busy(&self) -> bool { self.worker.is_some() }
    fn ready(&self) -> bool { self.chip.as_ref().is_some_and(Chip::ready) }
    fn capacity(&self) -> usize { self.chip.as_ref().map_or(0, |c| c.capacity) }
    fn request(&self, action: Action) -> anyhow::Result<Request> {
        let device = if matches!(action, Action::Connect) { self.ports.get(self.selected).cloned() }
            else { self.chip.as_ref().map(|c| c.device.clone()) }.ok_or_else(|| anyhow::anyhow!("먼저 Pico를 연결하세요."))?;
        let connecting = matches!(action, Action::Connect);
        let model = if connecting { (!self.settings.model.is_empty()).then(|| self.settings.model.clone()) }
            else { self.chip.as_ref().map(|c| c.name.clone()) };
        Ok(Request { device, hz: self.settings.hz, model, expected: if connecting {None} else {self.chip.clone()},
            backup: self.settings.backup && action.destructive(), data_dir: self.settings.data_dir.clone(), action, demo: self.demo })
    }

    fn start_request(&mut self, request: Request) {
        self.push_log(format!("--- {} · {} · {} ---", request.action.name(), request.device.port, request.device.serial));
        self.phase = request.action.name().into(); self.fraction = None;
        self.started = Some(Instant::now()); self.committed = false; self.error = false;
        self.message = "작업 중입니다.".into();
        self.worker = Some(backend::start(request, self.demo_flash.clone()));
    }

    fn launch(&mut self, action: Action) {
        let request = match self.request(action.clone()) { Ok(r) => r, Err(e) => { self.fail(e.to_string()); return; } };
        if action.destructive() {
            let details = match &action {
                Action::Write { image, start, expected_sha256 } => {
                    let len = self.input.as_ref().map_or(0, |i| i.len);
                    format!("{}\n\n파일: {}\n파일 크기: {} bytes\n범위: {}\nSHA-256: {}\n\n범위 밖 데이터를 보존하고 기록 후 전체 칩을 검증합니다.",
                        request.device.serial, image.display(), len,
                        Region::new(*start, len, self.capacity()).map_or_else(|e| e.to_string(), Region::label), expected_sha256)
                }
                Action::Erase { region } => format!("{}\n\n지우기 범위: {}\n\n선택 범위를 칩의 지워진 값으로 바꾸고 전체 칩을 검증합니다. 범위 밖 데이터는 보존합니다.", request.device.serial, region.label()),
                _ => unreachable!(),
            };
            let word = if matches!(action, Action::Write {..}) { "WRITE" } else { "ERASE" };
            let summary = format!("칩: {} · {} bytes\n백업 파일 저장: {}\n\n{}", request.model.as_deref().unwrap_or("자동"),
                self.capacity(), if request.backup {"사용"} else {"안 함"}, details);
            self.confirmation = Some(Confirmation { request, summary, word, typed: String::new() });
        } else { self.start_request(request); }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let events: Vec<_> = self.worker.as_ref().map(|w| w.rx.try_iter().collect()).unwrap_or_default();
        for event in events {
            match event {
                Event::Log(text) => self.push_log(text),
                Event::Progress { phase, fraction } => { self.phase = phase; self.fraction = fraction; }
                Event::CommitStarted => self.committed = true,
                Event::Done { chip, read, message, folder } => {
                    self.chip = Some(chip); if read.is_some() { self.data = read; }
                    self.message = message.clone(); self.push_log(message); self.error = false;
                    self.last_folder = Some(folder); self.worker = None; self.fraction = Some(1.0);
                    if self.close_after_work { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                }
                Event::Saved { path } => {
                    if let Some(data) = &mut self.data { data.saved = Some(path.clone()); }
                    self.message = format!("메모리 버퍼 저장 완료: {}", path.display());
                    self.push_log(self.message.clone()); self.error = false;
                    self.worker = None; self.fraction = Some(1.0);
                    if self.close_after_work { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                }
                Event::SaveFailed { message } => {
                    self.fail(message.clone()); self.push_log(message);
                    self.worker = None; self.close_after_work = false;
                }
                Event::Failed { message, folder } => {
                    self.fail(message.clone()); self.push_log(message); self.last_folder = Some(folder);
                    self.worker = None;
                    if self.close_after_work && !self.committed { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                    else { self.close_after_work = false; }
                }
            }
        }
        if self.busy() { ctx.request_repaint_after(Duration::from_millis(40)); }
    }

    fn connection(&mut self, ui: &mut egui::Ui) {
        ui.heading("장치");
        ui.label(RichText::new("frwr Pico · USB CDC").small().color(MUTED));
        ui.add_space(6.0);
        ui.add_enabled_ui(!self.busy() && self.confirmation.is_none(), |ui| {
            ui.horizontal(|ui| {
                ui.label("COM 포트");
                if ui.small_button("새로 고침").clicked() { self.refresh(); }
            });
            let selected = self.ports.get(self.selected).map_or("Pico 없음".into(), |p| format!("{} · {}", p.port, p.serial));
            let previous_port = self.selected;
            egui::ComboBox::from_id_salt("port").width(225.0).selected_text(selected).show_ui(ui, |ui| {
                for (i, port) in self.ports.iter().enumerate() {
                    ui.selectable_value(&mut self.selected, i, format!("{} · {}", port.port, port.serial));
                }
            });
            if self.selected != previous_port { self.chip = None; }
            ui.label("SPI 요청 속도");
            let previous_hz = self.settings.hz;
            egui::ComboBox::from_id_salt("hz").width(225.0).selected_text(format_hz(self.settings.hz)).show_ui(ui, |ui| {
                for hz in [100000, 250000, 500000, 1000000, 2000000, 4000000, 8000000, 12000000] {
                    ui.selectable_value(&mut self.settings.hz, hz, format_hz(hz));
                }
            });
            if self.settings.hz != previous_hz { self.chip = None; }
            ui.add_space(5.0);
            ui.label(format!("내장 SPI 칩 DB · {}개 정의", self.catalog.len()));
            ui.add(egui::TextEdit::singleline(&mut self.chip_filter).desired_width(225.0).hint_text("제조사 / 모델 검색"));
            let previous_model = self.settings.model.clone();
            let candidates = self.chip.as_ref().map(|c| c.candidates.clone()).unwrap_or_default();
            egui::ComboBox::from_id_salt("chip-model").width(225.0)
                .selected_text(if self.settings.model.is_empty() {"자동 탐색"} else {&self.settings.model})
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.settings.model, String::new(), "자동 탐색");
                    if candidates.len() > 1 {
                        ui.label("탐색된 후보");
                        for name in candidates { ui.selectable_value(&mut self.settings.model, name.clone(), name); }
                        ui.separator();
                    }
                    let filter = self.chip_filter.to_lowercase();
                    for definition in self.catalog.iter().filter(|d| format!("{} {}", d.vendor, d.name).to_lowercase().contains(&filter)) {
                        ui.selectable_value(&mut self.settings.model, definition.name.clone(),
                            format!("{} · {} ({} B)", definition.vendor, definition.name, definition.capacity));
                    }
                });
            if self.settings.model != previous_model { self.chip = None; }
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.ports.is_empty(), egui::Button::new("연결 / 식별").fill(Color32::from_rgb(22, 101, 94))).clicked() {
                    self.launch(Action::Connect);
                }
                if ui.add_enabled(self.chip.is_some(), egui::Button::new("연결 해제")).clicked() { self.chip = None; }
            });
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            if let Some(chip) = &self.chip {
                ui.label(RichText::new(if chip.ready() {"칩 확인됨"} else {"프로그래머 연결됨"}).color(ACCENT).strong());
                ui.label(&chip.name);
                ui.separator();
                info(ui, "JEDEC", &chip.jedec);
                info(ui, "용량", &if chip.capacity > 0 {format!("{} bytes · {:.2} MiB", chip.capacity, chip.capacity as f64/1048576.0)} else {"모델 선택 후 확인".into()});
                info(ui, "SPI 설정", &format_hz(chip.actual_hz));
                info(ui, "SFDP", &chip.sfdp);
                ui.label(RichText::new(&chip.device.serial).monospace().small());
            } else {
                ui.label(RichText::new("연결 전").color(MUTED));
                ui.label("frwr 응답과 외부 SPI 플래시를 확인합니다. SFDP는 선택 정보입니다.");
            }
        });
        ui.add_space(12.0);
        ui.collapsing("도구 / 저장 위치", |ui| {
            ui.add_enabled_ui(!self.busy() && self.confirmation.is_none(), |ui| {
                ui.label("내장 libflashrom 1.8.0 · serprog SPI");
                ui.separator();
                ui.label("백업 · 로그 · 보고서");
                ui.label(RichText::new(self.settings.data_dir.display().to_string()).small());
                if ui.button("작업 폴더 선택").clicked() {
                    if let Some(path) = rfd::FileDialog::new().pick_folder() { self.settings.data_dir = path; }
                }
            });
        });
        if self.demo { ui.add_space(15.0); ui.label(RichText::new("데모 모드\n실제 USB·플래시에 접근하지 않습니다.").color(Color32::YELLOW)); }
    }

    fn load_input(&mut self, path: PathBuf) {
        let result = (|| -> anyhow::Result<InputFile> {
            if self.ready() { anyhow::ensure!(fs::metadata(&path)?.len() <= self.capacity() as u64, "입력 파일이 현재 칩 용량보다 큽니다."); }
            let bytes = fs::read(&path)?;
            anyhow::ensure!(!bytes.is_empty(), "빈 파일은 사용할 수 없습니다.");
            Ok(InputFile { path, len: bytes.len(), hash: sha256(&bytes) })
        })();
        match result {
            Ok(input) => { self.input_path = input.path.display().to_string(); self.input = Some(input); self.error = false; }
            Err(e) => { self.input = None; self.fail(format!("파일 열기 실패: {e:#}")); }
        }
    }

    fn file_input(&mut self, ui: &mut egui::Ui) {
        ui.label("입력 ROM / 바이너리");
        ui.horizontal(|ui| {
            if ui.add(egui::TextEdit::singleline(&mut self.input_path).desired_width((ui.available_width()-160.0).max(200.0))).changed() { self.input = None; }
            if ui.button("찾아보기").clicked() {
                if let Some(path) = rfd::FileDialog::new().add_filter("바이너리 이미지", &["bin", "rom", "img"]).pick_file() { self.load_input(path); }
            }
            if ui.button("열기").clicked() { self.load_input(PathBuf::from(&self.input_path)); }
        });
        if let Some(input) = &self.input {
            ui.label(RichText::new(format!("{} bytes · SHA-256 {}", input.len, input.hash)).monospace().small().color(MUTED));
        }
    }

    fn start_buffer_save(&mut self, output: PathBuf) {
        let Some(data) = self.data.clone() else { return; };
        self.phase = "메모리 버퍼 저장".into(); self.fraction = None;
        self.started = Some(Instant::now()); self.committed = false; self.error = false;
        self.message = "읽어둔 메모리 버퍼를 저장하고 있습니다.".into();
        self.worker = Some(backend::start_save(data, output));
    }

    fn choose_buffer_save(&mut self) {
        let Some(data) = &self.data else { return; };
        let name = format!("flash-{:06X}-{:06X}.bin", data.offset, data.offset+data.data.len()-1);
        if let Some(path) = rfd::FileDialog::new().set_file_name(name).add_filter("바이너리", &["bin"]).save_file() {
            self.start_buffer_save(path);
        }
    }

    fn range(&mut self, ui: &mut egui::Ui, length: bool) -> anyhow::Result<Region> {
        ui.horizontal(|ui| {
            ui.label("시작 주소"); ui.add(egui::TextEdit::singleline(&mut self.start).desired_width(135.0).font(egui::TextStyle::Monospace));
            if length {
                ui.label("길이 (bytes)"); ui.add(egui::TextEdit::singleline(&mut self.len).desired_width(135.0).font(egui::TextStyle::Monospace));
                if ui.small_button("전체 영역").clicked() { self.start = "0x000000".into(); self.len = format!("0x{:X}", self.capacity()); }
            }
        });
        let start = number(&self.start)?;
        let len = if length { number(&self.len)? } else { self.input.as_ref().map_or(0, |i| i.len) };
        Region::new(start, len, self.capacity())
    }

    fn operations(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for tab in [Tab::Dump, Tab::Read, Tab::Write, Tab::Erase, Tab::Verify] {
                ui.selectable_value(&mut self.tab, tab, tab.title());
            }
        });
        ui.add_space(9.0);
        let enabled = !self.busy() && self.confirmation.is_none();
        let readable = self.ready() && self.chip.as_ref().and_then(|c| c.definition.as_ref()).is_some_and(|d| d.can_read);
        let writable = self.ready() && self.chip.as_ref().and_then(|c| c.definition.as_ref()).is_some_and(|d| d.can_read && d.can_write && d.can_erase);
        let erasable = self.ready() && self.chip.as_ref().and_then(|c| c.definition.as_ref()).is_some_and(|d| d.can_read && d.can_erase);
        card(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.heading(self.tab.title());
                ui.add_space(5.0);
                if matches!(self.tab, Tab::Write | Tab::Erase) {
                    ui.checkbox(&mut self.settings.backup, "변경 전 전체 백업 파일 저장 (선택)");
                    ui.label(RichText::new(if self.settings.backup {"원본 전체를 작업 폴더에 저장합니다."}
                        else {"백업 파일을 만들지 않습니다. 범위 밖 보존에 필요한 읽기는 메모리에서만 처리합니다."}).small().color(MUTED));
                }
                match self.tab {
                    Tab::Dump => {
                        ui.label(format!("선택한 플래시의 전체 {} bytes를 메모리에 읽습니다.", self.capacity()));
                        ui.label(RichText::new("읽은 뒤 아래 저장 버튼으로 파일에 저장할 수 있습니다.").small().color(MUTED));
                        if ui.add_enabled(readable, primary("전체 덤프 읽기")).clicked() {
                            self.launch(Action::Dump);
                        }
                    }
                    Tab::Read => {
                        let range = self.range(ui, true);
                        range_hint(ui, &range);
                        ui.label(RichText::new("메모리에 읽어둔 뒤 저장 버튼으로 파일에 저장합니다.").small().color(MUTED));
                        if ui.add_enabled(readable && range.is_ok(), primary("선택 범위 읽기")).clicked() {
                            self.launch(Action::Read { region: range.unwrap() });
                        }
                    }
                    Tab::Write | Tab::Verify => {
                        self.file_input(ui);
                        let range = self.range(ui, false);
                        range_hint(ui, &range);
                        let write = self.tab == Tab::Write;
                        ui.label(RichText::new(if write { "범위 밖 보존 · 선택 백업 · 변경 후 전체 검증" }
                            else { "플래시를 변경하지 않고 선택한 파일과 비교합니다." }).small().color(MUTED));
                        if ui.add_enabled((if write {writable} else {readable}) && range.is_ok(), primary(if write { "쓰기 검토…" } else { "파일과 비교 검증" })).clicked() {
                            let input = self.input.as_ref().unwrap();
                            let action = if write { Action::Write { image: input.path.clone(), start: range.unwrap().start, expected_sha256: input.hash.clone() } }
                                else { Action::Verify { image: input.path.clone(), start: range.unwrap().start, expected_sha256: input.hash.clone() } };
                            self.launch(action);
                        }
                    }
                    Tab::Erase => {
                        ui.label("선택 범위를 칩의 지워진 값으로 바꿉니다. 전체 영역 선택 시 칩 전체를 지웁니다.");
                        let range = self.range(ui, true);
                        range_hint(ui, &range);
                        ui.label(RichText::new("부분 영역은 내장 드라이버가 블록 지우기·주변 데이터 복원 후 전체 검증합니다.").small().color(MUTED));
                        let full = range.as_ref().is_ok_and(|r| r.start == 0 && r.len == self.capacity());
                        if ui.add_enabled(erasable && (full || writable) && range.is_ok(), egui::Button::new("지우기 검토…").fill(Color32::from_rgb(120, 49, 49))).clicked() {
                            self.launch(Action::Erase { region: range.unwrap() });
                        }
                    }
                }
            });
        });
    }

    fn confirmation(&mut self, ctx: &egui::Context) {
        if let Some(mut confirm) = self.confirmation.take() {
            let mut execute = false; let mut dismiss = false;
            egui::Window::new(if confirm.request.demo { "데모 변경 확인" } else { "플래시 변경 확인" })
                .collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0,0.0]).default_width(610.0)
                .show(ctx, |ui| {
                    ui.heading(format!("{} · {}", confirm.request.action.name(), confirm.request.device.port));
                    ui.label(&confirm.summary);
                    ui.separator();
                    ui.label(format!("계속하려면 {}를 입력하세요.", confirm.word));
                    ui.text_edit_singleline(&mut confirm.typed);
                    ui.horizontal(|ui| {
                        if ui.button("돌아가기").clicked() { dismiss = true; }
                        if ui.add_enabled(confirm.typed.trim() == confirm.word, egui::Button::new("확인한 작업 실행").fill(Color32::from_rgb(130, 54, 48))).clicked() { execute = true; }
                    });
                });
            if execute { self.start_request(confirm.request); }
            else if !dismiss { self.confirmation = Some(confirm); }
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        let mut save = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("메모리 버퍼").strong());
            if let Some(data) = &self.data { ui.label(RichText::new(format!("{} bytes · 시작 0x{:06X}", data.data.len(), data.offset)).small().color(MUTED)); }
            if ui.add_enabled(self.data.is_some() && !self.busy() && self.confirmation.is_none(), primary("저장…")).clicked() { save = true; }
        });
        if save { self.choose_buffer_save(); }
        if let Some(data) = &self.data {
            ui.label(RichText::new(format!("원본: {} · {} · {}", data.source.port, data.source.serial, data.chip_name)).small().color(MUTED));
            ui.label(RichText::new(format!("SHA-256 {}", data.sha256)).monospace().small().color(MUTED));
            if let Some(saved) = &data.saved { ui.label(RichText::new(format!("저장한 파일: {}", saved.display())).small().color(MUTED)); }
            else { ui.label(RichText::new("미저장 · 메모리에만 보관 중 (앱 종료 시 사라짐)").small().color(ACCENT)); }
            let rows = data.data.len().div_ceil(16);
            let height = ui.available_height().max(100.0);
            let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
            egui::ScrollArea::both().id_salt("hex-preview").max_height(height).show_rows(ui, row_height, rows, |ui, range| {
                for row in range {
                    let start = row*16;
                    let bytes = &data.data[start..(start+16).min(data.data.len())];
                    let hex = bytes.iter().map(|b| format!("{b:02X} ")).collect::<String>();
                    let ascii: String = bytes.iter().map(|b| if (32..127).contains(b) { *b as char } else { '.' }).collect();
                    ui.label(RichText::new(format!("{:06X}   {:48}  {}", data.offset+start, hex, ascii)).text_style(egui::TextStyle::Monospace).color(Color32::from_rgb(193, 211, 226)));
                }
            });
        } else {
            card(ui, |ui| { ui.add_space(16.0); ui.label(RichText::new("전체 덤프 또는 범위 읽기 결과가 여기에 표시됩니다.").color(MUTED)); ui.add_space(16.0); });
        }
    }

    fn smoke_test(&mut self, ctx: &egui::Context) {
        let Some(path) = self.smoke.clone() else { return; };
        if self.error {
            let _ = fs::write(path.with_extension("error.txt"), &self.message);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        match self.smoke_phase {
            0 => { self.launch(Action::Connect); self.smoke_phase = 1; }
            1 if self.ready() && !self.busy() => {
                if self.smoke_view == "dump" { self.tab = Tab::Dump; self.launch(Action::Dump); }
                else { self.tab = Tab::Read; self.launch(Action::Read { region: Region { start: 0, len: 4096 } }); }
                self.smoke_phase = 2;
            }
            2 if self.data.is_some() && !self.busy() => {
                if self.smoke_view == "saved" {
                    self.start_buffer_save(self.settings.data_dir.join("saved-buffer.bin"));
                    self.smoke_phase = 6;
                    return;
                }
                if self.smoke_view == "write" {
                    self.tab = Tab::Write;
                    let input = self.settings.data_dir.join("demo-input.bin");
                    match fs::write(&input, [0xa5; 1024]) {
                        Ok(()) => self.load_input(input), Err(e) => self.fail(e.to_string()),
                    }
                } else if self.smoke_view == "erase" { self.tab = Tab::Erase; }
                self.smoke_phase = 3;
            }
            6 if !self.busy() && self.data.as_ref().is_some_and(|d| d.saved.is_some()) => { self.smoke_phase = 3; }
            3 => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.smoke_phase = 4;
            }
            4 => {
                let image = ctx.input(|i| i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }
                }));
                if let Some(image) = image {
                    let result = (|| -> anyhow::Result<()> {
                        let file = fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
                        let mut encoder = png::Encoder::new(file, image.size[0] as u32, image.size[1] as u32);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                        encoder.write_header()?.write_image_data(&bytes)?;
                        Ok(())
                    })();
                    if let Err(e) = result { let _ = fs::write(path.with_extension("error.txt"), format!("{e:#}")); }
                    self.smoke_phase = 5;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            _ => (),
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.busy() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_after_work = true;
            self.worker.as_ref().unwrap().cancel.store(true, Ordering::Relaxed);
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(7.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("frwr").size(25.0).strong().color(ACCENT));
                ui.label(RichText::new("FLASH UTILITY").size(16.0).strong());
                ui.label(RichText::new("Pico SPI NOR").small().color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(if self.demo { "DEMO · 모의 플래시" } else if self.chip.is_some() { "장치 확인됨" } else { "연결 대기" })
                        .color(if self.demo { Color32::YELLOW } else { ACCENT }));
                });
            });
            ui.add_space(7.0);
        });
        egui::TopBottomPanel::bottom("logs").resizable(true).default_height(165.0).min_height(95.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.strong("작업 로그");
                if ui.small_button("복사").clicked() { ctx.copy_text(self.logs.iter().cloned().collect::<Vec<_>>().join("\n")); }
                if ui.small_button("비우기").clicked() { self.logs.clear(); }
                if let Some(folder) = &self.last_folder {
                    if ui.small_button("작업 폴더 열기").clicked() { open_folder(folder); }
                    ui.label(RichText::new(folder.display().to_string()).small().color(MUTED));
                }
            });
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                for line in &self.logs { ui.label(RichText::new(line).monospace().size(12.0).color(MUTED)); }
            });
        });
        egui::SidePanel::left("connection").exact_width(265.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.connection(ui));
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            self.operations(ui);
            ui.add_space(8.0);
            if self.busy() {
                ui.horizontal(|ui| {
                    ui.spinner(); ui.label(&self.phase);
                    if let Some(started) = self.started { ui.label(format!("{:.0}초", started.elapsed().as_secs_f32())); }
                    if ui.button(if self.committed { "완료 후 중단" } else { "취소" }).clicked() {
                        self.worker.as_ref().unwrap().cancel.store(true, Ordering::Relaxed);
                    }
                });
                let bar = egui::ProgressBar::new(self.fraction.unwrap_or(0.0)).animate(self.fraction.is_none());
                ui.add(if self.fraction.is_some() { bar.show_percentage() } else { bar.text("진행 중") });
                if self.committed { ui.label(RichText::new("변경 시작 후에는 현재 작업과 전체 검증을 완료합니다.").small().color(MUTED)); }
                if self.close_after_work { ui.label("작업이 안전하게 끝나면 창을 닫습니다."); }
            } else {
                ui.label(RichText::new(&self.message).color(if self.error { Color32::from_rgb(247, 131, 124) } else { ACCENT }));
            }
            self.preview(ui);
        });
        self.confirmation(ctx);
        self.smoke_test(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.smoke.is_none() {
            if let Ok(bytes) = serde_json::to_vec_pretty(&self.settings) { let _ = fs::write(settings_file(), bytes); }
        }
    }
}

fn stamp() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() }
fn format_hz(hz: u32) -> String { if hz >= 1000000 { format!("{:.3} MHz", hz as f64/1000000.0) } else { format!("{:.1} kHz", hz as f64/1000.0) } }
fn primary(label: &str) -> egui::Button<'_> { egui::Button::new(label).fill(Color32::from_rgb(22, 101, 94)) }
fn card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(PANEL).inner_margin(13).corner_radius(8).stroke(egui::Stroke::new(1.0, Color32::from_rgb(43, 54, 68))).show(ui, content);
}
fn info(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(RichText::new(label).small().color(MUTED)); ui.label(value);
}
fn range_hint(ui: &mut egui::Ui, range: &anyhow::Result<Region>) {
    match range {
        Ok(region) => { ui.label(RichText::new(region.label()).monospace().small().color(ACCENT)); }
        Err(e) => { ui.label(RichText::new(e.to_string()).small().color(MUTED)); }
    }
}
fn open_folder(path: &Path) {
    #[cfg(windows)] { let _ = std::process::Command::new("explorer.exe").arg(path).spawn(); }
    #[cfg(not(windows))] { let _ = std::process::Command::new("xdg-open").arg(path).spawn(); }
}
