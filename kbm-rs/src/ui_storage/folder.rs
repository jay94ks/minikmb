use super::*;
use minikmb_native::storage::{folder::SourceInfo, folder_image::Root};
#[derive(Clone)]
pub(super) struct Selection {
    source: SourceInfo,
    card_uuid: Option<String>,
}
fn error() -> AppError {
    AppError::new(
        "FOLDER_SELECTION_FAILED",
        "폴더를 선택하거나 읽기 전용으로 열지 못했습니다.",
    )
}
#[cfg(windows)]
fn choose(parent: isize) -> Result<Option<Root>, AppError> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};
    use windows::{
        core::{w, HRESULT},
        Win32::{
            Foundation::HWND,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{
                FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR,
                FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
            },
        },
    };
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
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
    let dialog: IFileOpenDialog =
        unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| error())?;
    unsafe {
        dialog
            .SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR)
    }
    .map_err(|_| error())?;
    unsafe { dialog.SetTitle(w!("MiniKMB · 읽기 전용으로 공유할 폴더 선택")) }
        .map_err(|_| error())?;
    if let Err(e) = unsafe { dialog.Show(Some(HWND(parent as *mut _))) } {
        if e.code() == HRESULT(0x800704C7u32 as i32) {
            return Ok(None);
        }
        return Err(error());
    }
    let item = unsafe { dialog.GetResult() }.map_err(|_| error())?;
    let name = unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }.map_err(|_| error())?;
    let path = PathBuf::from(OsString::from_wide(unsafe { name.as_wide() }));
    unsafe {
        CoTaskMemFree(Some(name.0 as *const _));
    }
    Ok(Some(Root::approve(path)?))
}
impl Storage {
    pub(super) fn pick_folder(&mut self, app: &Arc<AppService>, runtime: &tokio::runtime::Handle) {
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
        let view = self.view.clone();
        let card_uuid = self.card.as_ref().map(|c| c.uuid.clone());
        let owner = self.owner.clone();
        let app = app.clone();
        view.lock().pending = true;
        runtime.spawn(async move {
            let picked = tokio::task::spawn_blocking(move || choose(parent)).await;
            let mut v = view.lock();
            v.pending = false;
            if owner.ended.is_cancelled()
                || owner.cancelled.is_cancelled()
                || app.active.validate(&binding).is_err()
            {
                return;
            }
            match picked {
                Ok(Ok(Some(root))) => match app.storage.approve_folder(binding, root, session) {
                    Ok(info) => {
                        v.folder = Some(Selection {
                            source: info,
                            card_uuid,
                        })
                    }
                    Err(e) => v.error = e.message,
                },
                Ok(Ok(None)) => {}
                _ => v.error = error().message,
            }
        });
    }
    pub(super) fn folder_controls(
        &mut self,
        ui: &mut egui::Ui,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        busy: bool,
    ) {
        let Some(b) = &self.binding else {
            return;
        };
        let sources = app.storage.folder_sources(b);
        if sources.is_empty() {
            return;
        }
        ui.separator();
        theme::group_label(ui, "선택한 PC 폴더");
        for source in sources {
            ui.horizontal(|ui| {
                ui.label(&source.name);
                if ui
                    .add_enabled(!busy, egui::Button::new("새 스냅샷…"))
                    .clicked()
                {
                    self.view.lock().folder = Some(Selection {
                        source: source.clone(),
                        card_uuid: self.card.as_ref().map(|c| c.uuid.clone()),
                    });
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("목록에서 해제"))
                    .clicked()
                {
                    self.start(
                        app,
                        runtime,
                        Action::FolderForget {
                            source_id: source.source_id.clone(),
                        },
                    );
                }
            });
        }
    }
    pub(super) fn folder_confirmation(
        &mut self,
        ctx: &egui::Context,
        app: &Arc<AppService>,
        runtime: &tokio::runtime::Handle,
        busy: bool,
    ) {
        let Some(source) = self.view.lock().folder.clone() else {
            return;
        };
        let mut decision = None;
        egui::Modal::new(egui::Id::new("folder-share-confirm")).show(ctx,|ui|{
            ui.set_width(460.0);ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Wrap);
            theme::heading(ui,"폴더 공유 확인","");ui.strong(&source.source.name);
            ui.label("원본 폴더를 변경하지 않고 FAT32 스냅샷을 SD 카드에 만듭니다. 완료 후 타깃에 읽기 전용으로 연결합니다.");
            ui.label("이후 원본 편집은 자동 반영되지 않습니다. 열린 디스크 경로를 닫은 뒤 새 스냅샷을 만드세요. 연결된 광학 매체는 분리합니다.");
            ui.label("최대10,000개 항목·깊이32·이미지32GiB, 개별 파일4GiB 미만입니다. 링크/정션은 포함하지 않습니다.");
            ui.horizontal(|ui| {
                if ui.button("돌아가기").clicked(){decision=Some(false);}
                if ui.add_enabled(!busy,egui::Button::new("스냅샷 만들고 공유")).clicked(){decision=Some(true);}
            });
        });
        if let Some(yes) = decision {
            self.view.lock().folder = None;
            if yes {
                self.start(
                    app,
                    runtime,
                    Action::FolderShare {
                        source_id: source.source.source_id,
                        confirmed: true,
                        expected_card_uuid: source.card_uuid,
                    },
                );
            }
        }
    }
    pub fn preview_folder() -> Self {
        let view = Self::preview(false);
        view.view.lock().folder = Some(Selection {
            source: SourceInfo {
                source_id: "0".repeat(64),
                name: "전달할 패키지".into(),
                expires_at: 0,
            },
            card_uuid: None,
        });
        view
    }
}
