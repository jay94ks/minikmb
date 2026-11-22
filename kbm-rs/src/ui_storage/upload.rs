use super::*;
use minikmb_native::storage::import::Spec;
use std::fs::File;
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Optical,
    Disk { replace_id: Option<u32> },
}
pub(super) struct Source {
    file: File,
    name: String,
    size: u64,
    session: u64,
    card_uuid: Option<String>,
    kind: Kind,
}
impl Source {
    pub(super) fn preview() -> Option<Self> {
        Some(Self {
            file: tempfile::tempfile().ok()?,
            name: "boot.iso".into(),
            size: 81920,
            session: 0,
            card_uuid: None,
            kind: Kind::Optical,
        })
    }
}

impl Storage {
    pub(super) fn pick_upload(
        &mut self,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        kind: Kind,
    ) {
        if self.preview {
            return;
        }
        let (Some(binding), Some(parent)) = (self.binding.clone(), self.native_window) else {
            return;
        };
        let session = match app.storage.import_session(&binding) {
            Ok(s) => s,
            Err(e) => {
                self.view.lock().error = e.message;
                return;
            }
        };
        let card_uuid = self.card.as_ref().map(|c| c.uuid.clone());
        let view = self.view.clone();
        let owner = self.owner.clone();
        let app = app.clone();
        view.lock().pending = true;
        runtime.spawn(async move{
            let picked=tokio::task::spawn_blocking(move||crate::file_picker::choose(parent,"이미지 파일\0*.iso;*.udf;*.img;*.bin\0모든 파일\0*.*\0\0","MiniKMB · 업로드할 이미지 선택\0")).await;
            let mut v=view.lock();v.pending=false;
            if owner.ended.is_cancelled() || owner.cancelled.is_cancelled() || app.active.validate(&binding).is_err(){return;}
            match picked{Ok(Ok(Some((file,name,size))))=>v.source=Some(Source{file,name,size,session,card_uuid,kind}),Ok(Ok(None))=>{},_=>v.error="이미지 파일을 열지 못했습니다. 다른 프로그램의 쓰기 작업이 끝났는지 확인하세요.".into()}
        });
    }
    pub(super) fn upload_confirmation(
        &mut self,
        ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        busy: bool,
    ) {
        let summary = self
            .view
            .lock()
            .source
            .as_ref()
            .map(|s| (s.name.clone(), s.size, s.kind));
        let Some((name, bytes, kind)) = summary else {
            return;
        };
        let mut decision = None;
        egui::Modal::new(egui::Id::new("image-upload-confirm")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            theme::heading(ui, "이미지 업로드 확인", "");
            ui.label(format!("{} · {}", name, size(bytes)));
            match kind {
                Kind::Optical => {
                    ui.label("현재 광학 매체를 분리하고 SD의 광학 이미지를 교체합니다.");
                }
                Kind::Disk { replace_id } => {
                    ui.horizontal(|ui| {
                        ui.label("이미지 형식");
                        egui::ComboBox::from_id_salt("import-format")
                            .selected_text(format_name(self.import_format))
                            .show_ui(ui, |ui| {
                                for code in 1..=3 {
                                    ui.selectable_value(
                                        &mut self.import_format,
                                        code,
                                        format_name(code),
                                    );
                                }
                            });
                    });
                    ui.label(if let Some(id) = replace_id {
                        format!("선택한 디스크 {id}를 교체합니다.")
                    } else {
                        "SD에 새 디스크 이미지를 만듭니다.".into()
                    });
                }
            }
            ui.horizontal(|ui| {
                if ui.button("취소").clicked() {
                    decision = Some(false);
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("검사 후 업로드"))
                    .clicked()
                {
                    decision = Some(true);
                }
            });
        });
        if let Some(confirmed) = decision {
            let source = self.view.lock().source.take();
            if !confirmed {
                return;
            }
            let Some(source) = source else {
                return;
            };
            let Some(binding) = self.binding.clone() else {
                return;
            };
            let spec = match source.kind {
                Kind::Optical => Spec::Optical {
                    name: source.name,
                    size: source.size,
                    confirmed: true,
                    expected_card_uuid: source.card_uuid,
                },
                Kind::Disk { replace_id } => Spec::Disk {
                    size: source.size,
                    format: self.import_format,
                    replace_id,
                    confirmed: true,
                    expected_card_uuid: source.card_uuid,
                },
            };
            self.job = None;
            self.finished = None;
            let owner = self.owner.clone();
            let app = app.clone();
            let view = self.view.clone();
            {
                let mut v = view.lock();
                v.pending = true;
                v.error.clear();
                v.job_id = None;
            }
            runtime.spawn(async move {
                let result =
                    app.storage
                        .start_import(binding, owner, spec, source.file, source.session);
                let mut v = view.lock();
                v.pending = false;
                match result {
                    Ok(info) => v.job_id = Some(info.job_id),
                    Err(e) => v.error = e.message,
                }
            });
        }
    }
}
