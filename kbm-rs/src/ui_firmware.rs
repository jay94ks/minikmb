use eframe::egui;
use minikmb_native::{
    active::Binding,
    firmware::{
        backend::{Inspection, Programmer},
        manager::Info,
        sources::{self, Source, Spec},
        Target,
    },
    input_sessions::Principal,
    model::{AppError, Uid},
    service::AppService,
    theme::{self, color},
};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
#[derive(Default)]
struct View {
    pending: bool,
    programmers: Vec<Programmer>,
    inspection: Option<Inspection>,
    source: Option<Source>,
    job_id: Option<String>,
    error: String,
}
pub struct Panel {
    target: Target,
    programmer: String,
    image: String,
    view: Arc<Mutex<View>>,
    owner: Principal,
    preview: bool,
}
impl Panel {
    pub fn new(preview: bool) -> Self {
        Self {
            target: Target::Stm32,
            programmer: String::new(),
            image: "minikmb-fs-hid-r63".into(),
            view: Default::default(),
            owner: Principal {
                key_id: "ui".into(),
                client_id: "ui:firmware".into(),
                cancelled: CancellationToken::new(),
                ended: CancellationToken::new(),
            },
            preview,
        }
    }
    fn command(
        &self,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        binding: Option<Binding>,
        name: &'static str,
        args: Value,
    ) {
        if self.preview {
            return;
        }
        let app = app.clone();
        let owner = self.owner.clone();
        let view = self.view.clone();
        view.lock().pending = true;
        runtime.spawn(async move {
            let result = app.execute_firmware(binding, &owner, name, args).await;
            let mut v = view.lock();
            v.pending = false;
            match result {
                Ok(result) => {
                    v.error.clear();
                    match name {
                        "firmware_programmers" => {
                            v.programmers = serde_json::from_value(result["programmers"].clone())
                                .unwrap_or_default();
                            v.inspection = None;
                        }
                        "firmware_inspect" => v.inspection = serde_json::from_value(result).ok(),
                        "firmware_start" => v.job_id = result["job_id"].as_str().map(str::to_owned),
                        _ => {}
                    }
                }
                Err(e) => v.error = format!("{} · {}", e.code, e.message),
            }
        });
    }
    fn pick(
        &self,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        parent: isize,
        binding: Option<Binding>,
    ) {
        if self.preview {
            return;
        }
        let target = self.target;
        let view = self.view.clone();
        let owner = self.owner.clone();
        let app = app.clone();
        view.lock().pending = true;
        runtime.spawn(async move {
            let picked = tokio::task::spawn_blocking(move || {
                let filter = match target {
                    Target::Stm32 => "STM32 펌웨어\0*.elf;*.bin\0모든 파일\0*.*\0\0",
                    Target::Ms2130 => "MS2130 ROM\0*.bin;*.rom\0모든 파일\0*.*\0\0",
                };
                let Some((mut file, name, size)) =
                    crate::file_picker::choose(parent, filter, "MiniKMB · 펌웨어 선택\0")?
                else {
                    return Ok(None);
                };
                let max = match target {
                    Target::Stm32 => 16 * 1024 * 1024,
                    Target::Ms2130 => 64 * 1024 * 1024,
                };
                if size == 0 || size > max {
                    return Err(AppError::new(
                        "INVALID_ARGUMENTS",
                        "펌웨어 파일 크기가 허용 범위를 벗어났습니다.",
                    ));
                }
                use std::io::Read;
                let mut bytes = Vec::new();
                file.by_ref()
                    .take(max + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| {
                        AppError::new("FIRMWARE_IO_FAILED", "펌웨어를 읽지 못했습니다.")
                    })?;
                Ok::<_, AppError>(Some((Spec { target, name, size }, bytes)))
            })
            .await;
            let result = match picked {
                Ok(Ok(Some((spec, bytes)))) => {
                    let valid = binding.as_ref().map_or(Ok(()), |b| app.active.validate(b));
                    valid
                        .and_then(|_| app.firmware.sources.stage(binding, owner, spec, &bytes))
                        .map(Some)
                }
                Ok(Ok(None)) => Ok(None),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(AppError::new(
                    "FIRMWARE_IO_FAILED",
                    "파일 선택에 실패했습니다.",
                )),
            };
            let mut v = view.lock();
            v.pending = false;
            match result {
                Ok(Some(source)) => {
                    v.source = Some(source);
                    v.error.clear();
                }
                Ok(None) => {}
                Err(e) => v.error = e.message,
            }
        });
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        _ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        parent: Option<isize>,
    ) {
        ui.heading("펌웨어 플래싱");
        ui.label("동봉 도구와 펌웨어를 사용하거나 다른 파일을 선택합니다.");
        ui.add_space(12.0);
        let pending = self.view.lock().pending;
        let current = app.firmware.current();
        let busy = pending || current.is_some();
        ui.add_enabled_ui(!busy, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (target, label) in [
                    (Target::Stm32, "STM32 · ST-Link / SWD"),
                    (Target::Ms2130, "MS2130 · RP2040 Writer"),
                ] {
                    if ui
                        .selectable_value(&mut self.target, target, label)
                        .changed()
                    {
                        self.programmer.clear();
                        self.image = sources::images()
                            .into_iter()
                            .find(|i| i.target == target)
                            .unwrap()
                            .id;
                        *self.view.lock() = View::default();
                    }
                }
            });
        });
        ui.separator();
        if self.target == Target::Stm32 {
            ui.weak("ELF 파일은 디버그 정보를 제외한 Flash 데이터만 기록합니다.");
        }
        let binding = None::<Binding>;
        ui.weak("채널 선택 없이 연결한 프로그래머의 대상에 기록합니다.");
        ui.add_enabled_ui(!busy,|ui|{
            ui.horizontal_wrapped(|ui|{
                if ui.button("프로그래머 검색").clicked(){self.command(app,runtime,None,"firmware_programmers",json!({"target":self.target}));}
                let devices=self.view.lock().programmers.clone();
                egui::ComboBox::from_id_salt("firmware-programmer").selected_text(if self.programmer.is_empty(){"프로그래머 선택"}else{&self.programmer}).width(280.0).show_ui(ui,|ui|{for p in devices{ui.add_enabled_ui(p.selectable,|ui|{if ui.selectable_value(&mut self.programmer,p.id.clone(),p.label).changed(){self.view.lock().inspection=None;}});}});
                if ui.add_enabled(!self.programmer.is_empty(),egui::Button::new("대상 확인")).clicked(){self.command(app,runtime,binding.clone(),"firmware_inspect",json!({"target":self.target,"programmer_id":self.programmer}));}
            });
            if let Some(i)=self.view.lock().inspection.clone(){ui.label(format!("플래시 용량: {}바이트{}",i.capacity,i.uid.map(|u|format!(" · MCU {}",u.display())).unwrap_or_default()));}
            ui.add_space(10.0);
            for image in sources::images().into_iter().filter(|i|i.target==self.target){ui.radio_value(&mut self.image,image.id.clone(),format!("동봉: {}",image.label));if self.image==image.id{ui.small(format!("원본 {}바이트 · SHA256 {}",image.bytes,image.sha256));}}
            ui.horizontal_wrapped(|ui|{
                ui.radio_value(&mut self.image,String::new(),"다른 파일");
                if ui.add_enabled(parent.is_some(),egui::Button::new("파일 선택…")).clicked(){self.image.clear();self.pick(app,runtime,parent.unwrap(),binding.clone());}
            });
            let source=self.view.lock().source.clone();
            if self.image.is_empty(){if let Some(s)=&source{ui.label(format!("{} · 기록 {}바이트",s.name,s.bytes));ui.small(format!("SHA256 {}",s.sha256));}else{ui.weak("기록할 파일을 선택하세요.");}}
            ui.add_space(10.0);
            if self.target==Target::Ms2130{ui.label("Writer를 기록할 MS2130 플래시에 연결하세요. 채널·칩 모델 선택은 필요 없습니다.");}
            let ready=!self.programmer.is_empty()&&(!self.image.is_empty()||source.is_some());
            if ui.add_enabled(ready,egui::Button::new("기록 시작")).clicked(){
                let mut args=json!({"target":self.target,"programmer_id":self.programmer,"confirmed":true});
                if self.image.is_empty(){args["source_id"]=json!(source.unwrap().source_id);}else{args["image_id"]=json!(self.image);}
                self.command(app,runtime,None,"firmware_start",args);
            }
        });
        let job_id = self.view.lock().job_id.clone();
        let job = job_id
            .as_deref()
            .and_then(|id| app.firmware.job(&self.owner, id).ok())
            .or(current);
        if let Some(job) = job {
            self.job(ui, app, runtime, &job);
        }
        if pending {
            ui.spinner();
            ui.label("확인 중…");
        }
        let error = self.view.lock().error.clone();
        if !error.is_empty() {
            ui.colored_label(color::ERROR, error);
        }
    }

    fn job(
        &self,
        ui: &mut egui::Ui,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        job: &Info,
    ) {
        ui.separator();
        ui.label(format!(
            "{} · {}",
            job.uid
                .as_ref()
                .map(Uid::display)
                .unwrap_or_else(|| job.programmer_id.clone()),
            job.phase
        ));
        if job.total > 0 {
            ui.add(
                egui::ProgressBar::new((job.done as f32 / job.total as f32).clamp(0.0, 1.0))
                    .show_percentage(),
            );
        }
        if !job.terminal() {
            ui.spinner();
            if job.can_cancel && ui.button("기록 전 취소").clicked() {
                self.command(
                    app,
                    runtime,
                    None,
                    "firmware_job_cancel",
                    json!({"job_id":job.job_id}),
                );
            }
        }
        if let Some(result) = &job.result {
            theme::status(
                ui,
                theme::Tone::Success,
                &format!(
                    "기록 {}바이트 · 비교 검증 {}바이트 완료",
                    result.bytes, result.verified_bytes
                ),
            );
            if result.target == Target::Ms2130 {
                ui.label("MS2130 ROM은 다음 보드 재기동부터 적용됩니다.");
            }
        }
        if let Some(e) = &job.error {
            ui.colored_label(color::ERROR, format!("{} · {}", e.code, e.message));
        }
    }
}
