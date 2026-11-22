use minikmb_native::model::AppError;
use std::fs::File;
#[cfg(windows)]
pub fn choose(
    parent: isize,
    filter_text: &str,
    title_text: &str,
) -> Result<Option<(File, String, u64)>, AppError> {
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
            .map_err(|_| minikmb_native::storage::import::file_error())?;
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
    let filter: Vec<u16> = filter_text.encode_utf16().collect();
    let title: Vec<u16> = title_text.encode_utf16().collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: HWND(parent as *mut _),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(path.as_mut_ptr()),
        nMaxFile: path.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_EXPLORER,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
        return if unsafe { CommDlgExtendedError() }.0 == 0 {
            Ok(None)
        } else {
            Err(minikmb_native::storage::import::file_error())
        };
    }
    let end = path
        .iter()
        .position(|c| *c == 0)
        .ok_or_else(minikmb_native::storage::import::file_error)?;
    let path = std::path::PathBuf::from(OsString::from_wide(&path[..end]));
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .map_err(|_| minikmb_native::storage::import::file_error())?;
    let meta = file
        .metadata()
        .map_err(|_| minikmb_native::storage::import::file_error())?;
    if !meta.is_file() {
        return Err(minikmb_native::storage::import::file_error());
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(minikmb_native::storage::import::file_error)?
        .to_owned();
    Ok(Some((file, name, meta.len())))
}
