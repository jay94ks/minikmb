use eframe::egui::{self, RichText};
use minikmb_native::{
    active::{ActiveState, Binding},
    input_sessions::Principal,
    model::AppError,
    service::AppService,
    storage::{Action, JobInfo, JobState},
    storage_wire::{Card, Snapshot},
    theme::{self, color},
};
use parking_lot::Mutex;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
mod card_format;
mod folder;
mod format;
mod media;
mod upload;
#[derive(Clone, Copy, PartialEq)]
pub enum Page {
    Images,
    Metadata,
}
#[derive(Default)]
struct View {
    job_id: Option<String>,
    pending: bool,
    error: String,
    source: Option<upload::Source>,
    folder: Option<folder::Selection>,
}
pub struct Storage {
    pub open: bool,
    page: Page,
    binding: Option<Binding>,
    owner: Principal,
    view: Arc<Mutex<View>>,
    finished: Option<String>,
    job: Option<JobInfo>,
    card: Option<Card>,
    selected: Option<(String, u32)>,
    name: String,
    user: String,
    hex: bool,
    poll: Instant,
    initial: bool,
    preview: bool,
    preview_snapshot: Option<Snapshot>,
    disk_path: minikmb_native::storage::media::Path,
    disk_writable: bool,
    sector_lba: String,
    sector_hex: String,
    confirm_write: Option<Action>,
    pub native_window: Option<isize>,
    import_format: u8,
    format_dialog: Option<format::Dialog>,
    card_format_dialog: Option<card_format::Dialog>,
}
impl Default for Storage {
    fn default() -> Self {
        Self {
            open: false,
            page: Page::Images,
            binding: None,
            owner: Principal {
                key_id: "ui".into(),
                client_id: "ui:storage".into(),
                cancelled: CancellationToken::new(),
                ended: CancellationToken::new(),
            },
            view: Default::default(),
            finished: None,
            job: None,
            card: None,
            selected: None,
            name: String::new(),
            user: String::new(),
            hex: false,
            poll: Instant::now(),
            initial: false,
            preview: false,
            preview_snapshot: None,
            disk_path: minikmb_native::storage::media::Path::Target,
            disk_writable: true,
            sector_lba: "0".into(),
            sector_hex: String::new(),
            confirm_write: None,
            native_window: None,
            import_format: 1,
            format_dialog: None,
            card_format_dialog: None,
        }
    }
}
impl Storage {
    pub fn preview_card_format() -> Self {
        let mut view = Self::preview(true);
        view.card_format_dialog = Some(card_format::Dialog::new(
            view.card.as_ref().map(|c| c.uuid.clone()),
        ));
        view
    }
    pub fn preview_format(existing: bool) -> Self {
        let mut view = Self::preview_disk(false);
        if existing {
            let item = &mut view.preview_snapshot.as_mut().unwrap().images[1];
            item.size = 9 * 1048576 + 512;
            item.format = 2;
        }
        view.format_dialog = Some(format::Dialog::new(
            if existing { 2 } else { 1 },
            if existing { Some(2) } else { None },
            9 * 1048576 + 512,
            view.card.as_ref().map(|c| c.uuid.clone()),
        ));
        view
    }
    pub fn preview_upload() -> Self {
        let view = Self::preview(false);
        view.view.lock().source = upload::Source::preview();
        view
    }
    pub fn preview_disk(direct: bool) -> Self {
        let mut view = Self::preview(false);
        view.selected = Some(("disk".into(), 2));
        if direct {
            let snapshot = view.preview_snapshot.as_mut().unwrap();
            snapshot.disk = Some(minikmb_native::storage_wire::Disk {
                id: 2,
                sectors: 131072,
                lease: 77,
                format: 1,
                state: 3,
                writable: true,
            });
            snapshot.images[1].mounted = true;
            snapshot.images[1].writable = true;
            view.sector_hex = hex(&[0; 512]);
        }
        view
    }
    pub fn preview(metadata: bool) -> Self {
        let mut view = Self::default();
        let uid = minikmb_native::model::Uid::from_bytes(&[0; 12]).unwrap();
        view.show_page(
            Binding {
                uid: uid.clone(),
                generation: 0,
            },
            if metadata {
                Page::Metadata
            } else {
                Page::Images
            },
            "예시 채널".into(),
        );
        view.preview = true;
        view.initial = false;
        let card = Card {
            present: true,
            initialized: true,
            user_available: true,
            uuid: "00000000000000000000000000000001".into(),
            user_length: 0,
            revision: 1,
            stored_uid: Some(uid.clone()),
            current_uid: uid.clone(),
            profile_status: 0,
            user_status: 0,
        };
        let images = [
            (1, "optical", "boot.iso", 0, 9),
            (2, "disk", "D0000002.IMG", 1, 64),
            (3, "disk", "D0000003.IMG", 2, 8192),
            (4, "disk", "D0000004.IMG", 3, 1024),
        ]
        .into_iter()
        .map(
            |(id, kind, name, format, mib)| minikmb_native::storage_wire::Image {
                id,
                kind: kind.into(),
                name: name.into(),
                size: mib * 1048576,
                format,
                mounted: false,
                path: 0,
                writable: false,
            },
        )
        .collect();
        view.preview_snapshot = Some(Snapshot {
            uid,
            generation: 0,
            observed_at: 0,
            stale: false,
            card: Some(card.clone()),
            optical: None,
            disk: None,
            capabilities: 7,
            optical_enabled: true,
            optical_applied: true,
            images,
        });
        view.card = Some(card);
        view.selected = Some(("optical".into(), 1));
        view.user = "UI 미리보기입니다. 실제 SD 값이 아닙니다.".into();
        view
    }
    pub fn show_page(&mut self, b: Binding, page: Page, name: String) {
        if self.open && self.binding.as_ref() == Some(&b) {
            self.page = page;
            return;
        }
        self.close();
        self.open = true;
        self.page = page;
        self.binding = Some(b);
        self.name = name;
        self.owner.cancelled = CancellationToken::new();
        self.owner.ended = CancellationToken::new();
        self.view = Default::default();
        self.finished = None;
        self.job = None;
        self.card = None;
        self.selected = None;
        self.user.clear();
        self.initial = true;
    }
    fn close(&mut self) {
        self.open = false;
        self.owner.ended.cancel();
        self.binding = None;
        self.confirm_write = None;
        self.format_dialog = None;
        self.card_format_dialog = None;
        self.sector_hex.clear();
        self.view.lock().source = None;
        self.view.lock().folder = None;
    }
    fn start(&mut self, app: &Arc<AppService>, runtime: &tokio::runtime::Handle, action: Action) {
        let Some(b) = self.binding.clone() else {
            return;
        };
        {
            let mut v = self.view.lock();
            if v.pending {
                return;
            }
            v.pending = true;
            v.error.clear();
            v.job_id = None;
        }
        self.job = None;
        self.finished = None;
        let app = app.clone();
        let owner = self.owner.clone();
        let view = self.view.clone();
        runtime.spawn(async move {
            let result = app.execute_storage(
                Some(b),
                &owner,
                "storage_action",
                serde_json::to_value(action).unwrap(),
            );
            let mut v = view.lock();
            v.pending = false;
            match result {
                Ok(value) => v.job_id = value["job_id"].as_str().map(str::to_owned),
                Err(e) => v.error = e.message,
            };
        });
    }
    fn update_job(&mut self, app: &Arc<AppService>) {
        let id = self.view.lock().job_id.clone();
        if let Some(id) = id {
            match app.storage.job(&self.owner, &id) {
                Ok(info) => {
                    if info.finished_at.is_some() && self.finished.as_ref() != Some(&id) {
                        self.finished = Some(id);
                        let mut v = self.view.lock();
                        v.error = info
                            .error
                            .as_ref()
                            .map(|e| e.message.clone())
                            .unwrap_or_default();
                        if let Some(error) = &info.cleanup_error {
                            v.error.push_str(&format!(" 정리: {}", error.message));
                        }
                        drop(v);
                        if info.state == JobState::Completed {
                            if let Some(value) = &info.result {
                                if let Ok(snapshot) =
                                    serde_json::from_value::<Snapshot>(value["snapshot"].clone())
                                {
                                    self.apply_snapshot(snapshot);
                                }
                                if let Ok(bytes) = serde_json::from_value::<Vec<u8>>(
                                    value["sector"]["bytes"].clone(),
                                ) {
                                    self.sector_hex = hex(&bytes);
                                }
                                if info.kind != "refresh" {
                                    if let Ok(card) =
                                        serde_json::from_value::<Card>(value["card"].clone())
                                    {
                                        self.card = Some(card);
                                    }
                                }
                                match info.kind {
                                    "refresh" => {
                                        if let Ok(snapshot) =
                                            serde_json::from_value::<Snapshot>(value.clone())
                                        {
                                            self.apply_snapshot(snapshot);
                                        }
                                    }
                                    "card_info" => {
                                        self.card = serde_json::from_value(value.clone()).ok()
                                    }
                                    "card_format" => {
                                        self.name.clear();
                                        self.user.clear();
                                        self.selected = None;
                                    }
                                    "folder_share" => {
                                        if let Some(id) = value["image"]["id"]
                                            .as_u64()
                                            .and_then(|n| u32::try_from(n).ok())
                                        {
                                            self.selected = Some(("disk".into(), id));
                                        }
                                    }
                                    "card_format_status" => {
                                        if let Some(name) = value["name"].as_str() {
                                            self.name = name.into();
                                        }
                                        if self.card.as_ref().is_some_and(|c| c.user_length == 0) {
                                            self.user.clear();
                                        }
                                        self.selected = None;
                                    }
                                    "name_get" | "name_set" => {
                                        if let Some(name) = value["name"].as_str() {
                                            self.name = name.into();
                                        }
                                    }
                                    "user_read" => {
                                        if let Ok(bytes) = serde_json::from_value::<Vec<u8>>(
                                            value["bytes"].clone(),
                                        ) {
                                            if self.hex {
                                                self.user = hex(&bytes);
                                            } else {
                                                match String::from_utf8(bytes) {
                                                    Ok(s) => self.user = s,
                                                    Err(e) => {
                                                        self.hex = true;
                                                        self.user = hex(e.as_bytes());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    self.job = Some(info);
                }
                Err(e) => self.view.lock().error = e.message,
            }
        }
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        active: &ActiveState,
    ) {
        if !self.open {
            return;
        }
        if !self.preview
            && !self.binding.as_ref().is_some_and(|b| {
                Some(&b.uid) == active.uid.as_ref()
                    && b.generation == active.generation
                    && !active.switching
            })
        {
            self.close();
            return;
        }
        if self.initial {
            self.initial = false;
            self.start(
                app,
                runtime,
                if self.page == Page::Images {
                    Action::Refresh {}
                } else {
                    Action::CardInfo {}
                },
            );
        }
        if !self.preview && self.poll.elapsed() > Duration::from_millis(100) {
            self.poll = Instant::now();
            self.update_job(app);
        }
        let b = self.binding.as_ref().unwrap().clone();
        let snapshot = self
            .preview_snapshot
            .clone()
            .or_else(|| app.storage.cached(&b));
        if self.card.is_none() {
            self.card = snapshot
                .as_ref()
                .filter(|s| !s.stale)
                .and_then(|s| s.card.clone());
        }
        let current = app.storage.current(&b);
        let format_hold = app.storage.card_format_info();
        let operation_busy = self.preview || self.view.lock().pending || current.is_some();
        let busy = operation_busy || format_hold.is_some();
        let mut open = true;
        theme::tool_window(
            ctx,
            if self.preview {
                "SD · 디스크 관리 · UI 미리보기"
            } else {
                "SD · 디스크 관리"
            },
            "storage-dialog",
            [960.0, 570.0],
        )
        .open(&mut open)
        .show(ctx, |ui| {
            if self.preview {
                ui.label(
                    RichText::new("장치 연결 없는 화면 예시입니다.")
                        .small()
                        .color(color::MUTED),
                );
            }
            ui.horizontal(|ui| {
                if theme::tab(ui, self.page == Page::Images, "디스크 이미지").clicked() {
                    self.page = Page::Images;
                }
                if theme::tab(ui, self.page == Page::Metadata, "채널 · 사용자 값").clicked()
                {
                    self.page = Page::Metadata;
                }
            });
            ui.separator();
            if let Some(job) = current.as_ref().or(self.job.as_ref()) {
                if job.finished_at.is_none() {
                    ui.horizontal(|ui| {
                        ui.label(&job.phase);
                        if job.can_cancel
                            && self.view.lock().job_id.as_ref() == Some(&job.job_id)
                            && ui.button("취소").clicked()
                        {
                            if let Err(e) = app.storage.cancel(&self.owner, &job.job_id) {
                                self.view.lock().error = e.message;
                            }
                        }
                    });
                    if job.total > 0 {
                        ui.add(
                            egui::ProgressBar::new((job.done as f64 / job.total as f64) as f32)
                                .text(format!("{} / {}", job.done, job.total)),
                        );
                    }
                    if let Some(transfer) = &job.transfer {
                        ui.label(
                            match (transfer.bytes_per_second, transfer.remaining_seconds) {
                                (Some(rate), Some(seconds)) => format!(
                                    "{} /s · 남은 시간 약 {:.0}초",
                                    size(rate.max(0.0) as u64),
                                    seconds
                                ),
                                (Some(0.0), None) => "전송 응답 대기 중 · 남은 시간 계산 중".into(),
                                _ => "전송 속도·남은 시간 계산 중".into(),
                            },
                        );
                    }
                } else if job.state == JobState::Completed {
                    theme::status(ui, theme::Tone::Success, "작업 완료");
                }
            }
            let error = self.view.lock().error.clone();
            if !error.is_empty() {
                ui.colored_label(color::ERROR, error);
            }
            if let Some(error) = app.storage.media_error() {
                ui.colored_label(color::ERROR, error.message);
            }
            if let Some(hold) = &format_hold {
                theme::status(ui, theme::Tone::Warning, &hold.phase);
                ui.label(format!("원래 채널: {} · 취소 불가", hold.uid.display()));
                if ui
                    .add_enabled(!operation_busy, egui::Button::new("포맷 상태 확인"))
                    .clicked()
                {
                    self.start(app, runtime, Action::CardFormatStatus {});
                }
            }
            egui::ScrollArea::vertical()
                .id_salt("storage-body")
                .max_height(490.0)
                .show(ui, |ui| match self.page {
                    Page::Images => self.images(ui, app, runtime, snapshot.as_ref(), busy),
                    Page::Metadata => self.metadata(ui, app, runtime, busy),
                });
        });
        if !open {
            self.close();
        } else {
            self.write_confirmation(ctx, app, runtime, busy);
            self.upload_confirmation(ctx, app, runtime, busy);
            self.format_confirmation(ctx, app, runtime, busy);
            self.card_format_confirmation(ctx, app, runtime, busy);
            self.folder_confirmation(ctx, app, runtime, busy);
        }
    }
    fn images(
        &mut self,
        ui: &mut egui::Ui,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        snapshot: Option<&Snapshot>,
        busy: bool,
    ) {
        ui.columns(2, |columns| {
            theme::panel().show(&mut columns[0], |ui| {
                theme::heading(ui, "선택한 이미지", "");
                let item = snapshot.and_then(|s| {
                    s.images
                        .iter()
                        .find(|item| self.selected.as_ref() == Some(&(item.kind.clone(), item.id)))
                });
                if let Some(item) = item {
                    ui.strong(&item.name);
                    ui.label(format!(
                        "{} · {}",
                        format_name(item.format),
                        size(item.size)
                    ));
                    ui.label(if item.mounted {
                        if item.writable {
                            "연결됨 · 읽기/쓰기"
                        } else {
                            "연결됨 · 읽기 전용"
                        }
                    } else {
                        "분리됨"
                    });
                    self.media_controls(ui, app, runtime, snapshot.unwrap(), item, busy);
                } else {
                    ui.label(
                        RichText::new("오른쪽 목록에서 이미지를 선택하세요.").color(color::MUTED),
                    );
                }
                self.folder_controls(ui, app, runtime, busy);
            });
            theme::panel().show(&mut columns[1], |ui| {
                ui.horizontal(|ui| {
                    ui.strong("SD 이미지 목록");
                    if ui
                        .add_enabled(!busy, egui::Button::new("목록 갱신"))
                        .clicked()
                    {
                        self.start(app, runtime, Action::Refresh {});
                    }
                    ui.add_enabled_ui(
                        !busy
                            && snapshot.is_some_and(|s| {
                                !s.stale && s.card.as_ref().is_some_and(|c| c.present)
                            }),
                        |ui| {
                            ui.menu_button("+", |ui| {
                                for code in 1..=3 {
                                    if ui
                                        .button(format!("{} 이미지 만들기…", format_name(code)))
                                        .clicked()
                                    {
                                        ui.close();
                                        self.format_dialog = Some(format::Dialog::new(
                                            code,
                                            None,
                                            0,
                                            snapshot
                                                .and_then(|s| s.card.as_ref())
                                                .map(|c| c.uuid.clone()),
                                        ));
                                    }
                                }
                                ui.separator();
                                if ui.button("PC 폴더 공유…").clicked() {
                                    ui.close();
                                    self.pick_folder(app, runtime);
                                }
                                if ui.button("부팅 ISO/UDF 업로드…").clicked() {
                                    ui.close();
                                    self.pick_upload(app, runtime, upload::Kind::Optical);
                                }
                                if ui.button("디스크 이미지 업로드…").clicked() {
                                    ui.close();
                                    self.pick_upload(
                                        app,
                                        runtime,
                                        upload::Kind::Disk { replace_id: None },
                                    );
                                }
                            });
                        },
                    );
                });
                if let Some(snapshot) = snapshot {
                    ui.horizontal(|ui| {
                        ui.label("광학 드라이브");
                        theme::status(
                            ui,
                            if snapshot.optical_applied {
                                theme::Tone::Success
                            } else {
                                theme::Tone::Neutral
                            },
                            if snapshot.optical_applied {
                                "켜짐"
                            } else {
                                "꺼짐"
                            },
                        );
                        if ui
                            .add_enabled(
                                !busy && snapshot.capabilities & 1 != 0,
                                egui::Button::new(if snapshot.optical_enabled {
                                    "장치 끄기"
                                } else {
                                    "장치 켜기"
                                }),
                            )
                            .clicked()
                        {
                            self.start(
                                app,
                                runtime,
                                Action::OpticalSet {
                                    enabled: !snapshot.optical_enabled,
                                },
                            );
                        }
                    });
                }
                if let Some(snapshot) = snapshot {
                    if snapshot.stale {
                        theme::status(ui, theme::Tone::Warning, "목록을 갱신하세요.");
                    }
                    if self.selected.as_ref().is_some_and(|selected| {
                        !snapshot
                            .images
                            .iter()
                            .any(|item| selected == &(item.kind.clone(), item.id))
                    }) {
                        self.selected = None;
                    }
                    if snapshot.images.is_empty() {
                        ui.label("저장된 이미지가 없습니다.");
                    }
                    for item in &snapshot.images {
                        let key = (item.kind.clone(), item.id);
                        if theme::list_row(
                            ui,
                            self.selected.as_ref() == Some(&key),
                            &format!("{}   {}", item.name, size(item.size)),
                        )
                        .clicked()
                        {
                            self.selected = Some(key);
                        }
                    }
                } else {
                    ui.label("SD 이미지 목록을 갱신하세요.");
                }
            });
        });
    }
    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        if self.card.as_ref().map(|c| &c.uuid) != snapshot.card.as_ref().map(|c| &c.uuid) {
            self.selected = None;
        }
        self.card = snapshot.card;
    }
    fn metadata(
        &mut self,
        ui: &mut egui::Ui,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        busy: bool,
    ) {
        if let Some(card) = &self.card {
            theme::status(
                ui,
                if card.present {
                    theme::Tone::Success
                } else {
                    theme::Tone::Neutral
                },
                if card.present {
                    "SD 카드 있음"
                } else {
                    "SD 카드 없음"
                },
            );
            ui.label(
                RichText::new(format!("카드 UUID: {}", card.uuid))
                    .monospace()
                    .small(),
            );
            ui.label(
                RichText::new(format!("MCU UID: {}", card.current_uid.display()))
                    .monospace()
                    .small(),
            );
            ui.label(
                RichText::new(format!(
                    "사용자 값 {} B · revision {}",
                    card.user_length, card.revision
                ))
                .small()
                .color(color::MUTED),
            );
        }
        if ui
            .add_enabled(!busy, egui::Button::new("카드 정보 갱신"))
            .clicked()
        {
            self.start(app, runtime, Action::CardInfo {});
        }
        if ui
            .add_enabled(
                !busy && self.card.as_ref().is_some_and(|c| c.present),
                egui::Button::new("SD 카드 전체 포맷…"),
            )
            .clicked()
        {
            self.card_format_dialog = Some(card_format::Dialog::new(
                self.card.as_ref().map(|c| c.uuid.clone()),
            ));
        }
        ui.separator();
        theme::group_label(ui, "채널 이름");
        ui.horizontal(|ui| {
            ui.add_enabled(
                !busy,
                egui::TextEdit::singleline(&mut self.name)
                    .char_limit(64)
                    .desired_width(320.0),
            );
            if ui
                .add_enabled(!busy, egui::Button::new("이름 읽기"))
                .clicked()
            {
                self.start(app, runtime, Action::NameGet {});
            }
            if ui
                .add_enabled(!busy, egui::Button::new("이름 저장"))
                .clicked()
            {
                self.start(
                    app,
                    runtime,
                    Action::NameSet {
                        name: self.name.clone(),
                    },
                );
            }
        });
        ui.add_space(12.0);
        theme::group_label(ui, "사용자 값 (최대4KiB)");
        let previous = self.hex;
        ui.horizontal(|ui| {
            ui.add_enabled(!busy, egui::Checkbox::new(&mut self.hex, "HEX"));
            if ui.add_enabled(!busy, egui::Button::new("읽기")).clicked() {
                self.start(app, runtime, Action::UserRead {});
            }
            if ui.add_enabled(!busy, egui::Button::new("저장")).clicked() {
                match user_bytes(&self.user, self.hex) {
                    Ok(bytes) => self.start(app, runtime, Action::UserWrite { bytes }),
                    Err(e) => self.view.lock().error = e.message,
                }
            }
            if ui
                .add_enabled(!self.preview, egui::Button::new("복사"))
                .clicked()
            {
                ui.ctx().copy_text(self.user.clone());
            }
        });
        if previous != self.hex {
            match user_bytes(&self.user, previous) {
                Ok(bytes) => {
                    if self.hex {
                        self.user = hex(&bytes);
                    } else {
                        match String::from_utf8(bytes) {
                            Ok(text) => self.user = text,
                            Err(_) => {
                                self.hex = previous;
                                self.view.lock().error =
                                    "UTF-8로 표시할 수 없는 값입니다. HEX로 편집하세요.".into();
                            }
                        }
                    }
                }
                Err(e) => {
                    self.hex = previous;
                    self.view.lock().error = e.message;
                }
            }
        }
        ui.add_enabled(
            !busy,
            egui::TextEdit::multiline(&mut self.user)
                .font(egui::TextStyle::Monospace)
                .char_limit(12288)
                .desired_rows(7)
                .desired_width(f32::INFINITY),
        );
    }
}
impl Drop for Storage {
    fn drop(&mut self) {
        self.close();
    }
}
fn format_name(format: u8) -> &'static str {
    match format {
        0 => "광학 ISO/UDF",
        1 => "FAT32",
        2 => "exFAT",
        3 => "EXT4",
        _ => "알 수 없음",
    }
}
fn size(bytes: u64) -> String {
    if bytes >= 1048576 {
        format!("{:.1} MiB", bytes as f64 / 1048576.0)
    } else {
        format!("{bytes} B")
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes
        .chunks(16)
        .map(|row| {
            row.iter()
                .map(|n| format!("{n:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn user_bytes(text: &str, hex: bool) -> Result<Vec<u8>, AppError> {
    let bytes = if hex {
        let value: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if !value.len().is_multiple_of(2) || !value.is_ascii() {
            return Err(AppError::new(
                "INVALID_ARGUMENTS",
                "올바른 HEX 바이트를 입력하세요.",
            ));
        }
        (0..value.len())
            .step_by(2)
            .map(|at| {
                u8::from_str_radix(&value[at..at + 2], 16).map_err(|_| {
                    AppError::new("INVALID_ARGUMENTS", "올바른 HEX 바이트를 입력하세요.")
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        text.as_bytes().to_vec()
    };
    if bytes.len() > 4096 {
        return Err(AppError::new(
            "INVALID_ARGUMENTS",
            "사용자 값은 UTF-8/바이너리 기준4096바이트까지 저장합니다.",
        ));
    }
    Ok(bytes)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn card_replacement_clears_even_reused_image_ids() {
        let mut view = Storage::preview(false);
        let mut snapshot = view.preview_snapshot.clone().unwrap();
        assert!(view.selected.is_some());
        view.apply_snapshot(snapshot.clone());
        assert!(view.selected.is_some());
        snapshot.card.as_mut().unwrap().uuid = "another-card".into();
        view.apply_snapshot(snapshot);
        assert!(view.selected.is_none());
    }
    #[test]
    fn user_binary_roundtrip_and_actual_utf8_limits() {
        let bytes: Vec<_> = (0..4096).map(|n| (n % 256) as u8).collect();
        assert_eq!(user_bytes(&hex(&bytes), true).unwrap(), bytes);
        assert!(user_bytes(&"가".repeat(1366), false).is_err());
        assert!(user_bytes("x0", true).is_err());
        assert_eq!(user_bytes("", true).unwrap(), Vec::<u8>::new());
    }
}
