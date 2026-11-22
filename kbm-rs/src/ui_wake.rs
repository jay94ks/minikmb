use eframe::egui::{self, RichText};
use minikmb_native::{
    active::Binding,
    input_sessions::Principal,
    preferences::Wol,
    service::AppService,
    theme::{self, color},
    wake::Adapter,
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
#[derive(Default)]
struct Work {
    busy: bool,
    adapters: Vec<Adapter>,
    config: Option<Wol>,
    message: String,
    error: String,
}
pub struct Panel {
    pub open: bool,
    binding: Option<Binding>,
    owner: Principal,
    draft: Wol,
    work: Arc<Mutex<Work>>,
    preview: bool,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            open: false,
            binding: None,
            owner: Principal {
                key_id: "ui".into(),
                client_id: "ui:wake".into(),
                cancelled: CancellationToken::new(),
                ended: CancellationToken::new(),
            },
            draft: Default::default(),
            work: Default::default(),
            preview: false,
        }
    }
}
impl Panel {
    pub fn preview() -> Self {
        Self {
            open: true,
            preview: true,
            draft: Wol {
                enabled: true,
                mac: "02:aa:bb:cc:dd:ee".into(),
                local_address: "192.168.1.4".into(),
            },
            work: Arc::new(Mutex::new(Work {
                adapters: vec![Adapter {
                    name: "Ethernet (예시)".into(),
                    address: "192.168.1.4".into(),
                    broadcast: "192.168.1.255".into(),
                }],
                ..Default::default()
            })),
            ..Default::default()
        }
    }
    fn close(&mut self) {
        self.open = false;
        self.owner.ended.cancel();
        self.binding = None;
    }
    pub fn toggle(&mut self, b: Binding, app: &Arc<AppService>, runtime: &tokio::runtime::Handle) {
        if self.open && self.binding.as_ref() == Some(&b) {
            self.close();
            return;
        }
        self.close();
        self.draft = app.preferences.channel(&b.uid).wol.unwrap_or_default();
        self.binding = Some(b);
        self.open = true;
        self.owner.ended = CancellationToken::new();
        self.work = Default::default();
        self.submit(app, runtime, "wake_list_adapters", json!({}));
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
        let Some(b) = self.binding.clone() else {
            return;
        };
        let app = app.clone();
        let owner = self.owner.clone();
        let work = self.work.clone();
        {
            let mut w = work.lock();
            w.busy = true;
            w.error.clear();
            w.message.clear();
        }
        runtime.spawn(async move {
            let result = app.execute_wake(b, &owner, name, args).await;
            let mut w = work.lock();
            w.busy = false;
            match result {
                Err(e) => w.error = e.message,
                Ok(value) => match name {
                    "wake_list_adapters" => {
                        // This UI consumes the service's own typed adapters, not remote input.
                        w.adapters = value["adapters"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|v| {
                                Some(Adapter {
                                    name: v["name"].as_str()?.into(),
                                    address: v["address"].as_str()?.into(),
                                    broadcast: v["broadcast"].as_str()?.into(),
                                })
                            })
                            .collect();
                    }
                    "wake_configure" => {
                        w.config = serde_json::from_value(value["config"].clone()).ok();
                        w.message = "WOL 설정을 저장했습니다.".into();
                    }
                    "wake_send" => {
                        w.message = value["message"]
                            .as_str()
                            .unwrap_or("발신 결과를 확인하지 못했습니다.")
                            .into()
                    }
                    _ => {}
                },
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
        if !self.preview
            && self
                .binding
                .as_ref()
                .is_none_or(|b| app.active.validate(b).is_err())
        {
            self.close();
            return;
        }
        if let Some(config) = self.work.lock().config.take() {
            self.draft = config;
        }
        let mut open = self.open;
        theme::tool_window(ctx,"PC 깨우기","pc-wake",[600.0,420.0]).open(&mut open).resizable(true).show(ctx,|ui|{
            theme::heading(ui,"Wake on LAN","선택한 관리 PC의 LAN 연결로 타깃 PC에 Magic Packet을 보냅니다.");
            if self.preview {ui.label(RichText::new("장치 연결 없는 화면 예시입니다.").color(color::MUTED));}
            let busy=self.preview||self.work.lock().busy;
            ui.add_enabled(!busy,egui::Checkbox::new(&mut self.draft.enabled,"WOL 사용"));
            egui::Grid::new("wol-fields").num_columns(2).spacing([16.0,12.0]).show(ui,|ui|{
                ui.label("타깃 유선 LAN MAC");ui.add_enabled(!busy,egui::TextEdit::singleline(&mut self.draft.mac).char_limit(32).desired_width(300.0).hint_text("02:aa:bb:cc:dd:ee"));ui.end_row();
                ui.label("관리 PC LAN IPv4");
                ui.add_enabled_ui(!busy,|ui|{
                    let adapters=self.work.lock().adapters.clone();
                    egui::ComboBox::from_id_salt("wol-adapter").width(300.0).selected_text(if self.draft.local_address.is_empty(){"LAN 연결 선택"}else{&self.draft.local_address}).show_ui(ui,|ui|{
                        for adapter in &adapters {ui.selectable_value(&mut self.draft.local_address,adapter.address.clone(),format!("{} · {}",adapter.name,adapter.address));}
                    });
                });ui.end_row();
            });
            if let Some(adapter)=self.work.lock().adapters.iter().find(|a|a.address==self.draft.local_address){ui.label(RichText::new(format!("{} · {} → {}:9",adapter.name,adapter.address,adapter.broadcast)).small().color(color::MUTED));}
            ui.add_space(8.0);
            let saved=self.binding.as_ref().map(|b|app.preferences.channel(&b.uid).wol.unwrap_or_default());
            let dirty=saved.as_ref().is_some_and(|s|s!=&self.draft);
            ui.horizontal(|ui|{
                if ui.add_enabled(!busy,egui::Button::new("LAN 목록 갱신")).clicked(){self.submit(app,runtime,"wake_list_adapters",json!({}));}
                if ui.add_enabled(!busy&&dirty,egui::Button::new("설정 저장")).clicked(){self.submit(app,runtime,"wake_configure",json!(self.draft));}
                if ui.add_enabled(!busy&&!dirty&&self.draft.enabled,egui::Button::new("WOL 보내기")).clicked(){self.submit(app,runtime,"wake_send",json!({}));}
            });
            if dirty {ui.label(RichText::new("변경한 설정을 저장한 뒤 보내세요.").small().color(color::MUTED));}
            ui.label(RichText::new("패킷 발신은 기상 성공을 뜻하지 않습니다. 타깃 PC의 LAN·전원 설정이 필요합니다.").color(color::MUTED));
            ui.separator();
            ui.label(RichText::new("USB 깨우기 · System Control: 현재 펌웨어에서 지원하지 않습니다.").small().color(color::MUTED));
            let work=self.work.lock();
            if work.busy {ui.spinner();}
            if !work.message.is_empty(){theme::status(ui,theme::Tone::Neutral,&work.message);}
            if !work.error.is_empty(){theme::status(ui,theme::Tone::Warning,&work.error);}
        });
        if !open {
            self.close();
        }
    }
}
