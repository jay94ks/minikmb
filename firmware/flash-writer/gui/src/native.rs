//! In-process, statically linked flashrom. One serialized serprog session owns the library.
use crate::{backend::Event, model::{Definition, Device, Region}};
use anyhow::{ensure, Context, Result};
use std::{ffi::{c_char, c_int, c_void, CStr, CString}, fs::File, io::Write, path::Path,
    ptr, sync::{atomic::{AtomicBool, Ordering}, mpsc::SyncSender, Mutex, MutexGuard}};

static LIBRARY: Mutex<()> = Mutex::new(());

#[repr(C)]
struct Info { vendor: *const c_char, name: *const c_char, capacity: usize, erased_value: u32, capabilities: u32 }
impl Info {
    fn empty() -> Self { Self { vendor: ptr::null(), name: ptr::null(), capacity: 0, erased_value: 255, capabilities: 0 } }
    unsafe fn definition(&self) -> Definition {
        Definition { vendor: string(self.vendor), name: string(self.name), capacity: self.capacity,
            erased_value: self.erased_value as u8, can_read: self.capabilities & 1 != 0,
            can_write: self.capabilities & 2 != 0, can_erase: self.capabilities & 4 != 0 }
    }
}

extern "C" {
    fn flashrom_init(selfcheck: c_int) -> c_int;
    fn flashrom_shutdown() -> c_int;
    fn flashrom_create_context(ctx: *mut *mut c_void) -> c_int;
    fn flashrom_set_log_level(level: c_int);
    fn flashrom_set_log_callback_v2(callback: Option<unsafe extern "C" fn(c_int, *const c_char, *mut c_void)>, data: *mut c_void);
    fn flashrom_set_progress_callback_v2(ctx: *mut c_void, callback: Option<unsafe extern "C" fn(c_int, usize, usize, *mut c_void)>, data: *mut c_void);
    fn flashrom_programmer_init(prog: *mut *mut c_void, name: *const c_char, params: *const c_char) -> c_int;
    fn flashrom_programmer_shutdown(prog: *mut c_void) -> c_int;
    fn flashrom_flash_probe_v2(ctx: *mut c_void, names: *mut *mut *const c_char, prog: *const c_void, name: *const c_char) -> c_int;
    fn flashrom_data_free(data: *mut c_void) -> c_int;
    fn flashrom_flash_release(ctx: *mut c_void);
    fn flashrom_flag_set(ctx: *mut c_void, flag: c_int, value: bool);
    fn flashrom_image_write(ctx: *mut c_void, image: *mut c_void, size: usize, reference: *const c_void) -> c_int;
    fn flashrom_flash_erase(ctx: *mut c_void) -> c_int;
    #[cfg(test)] fn flashrom_supported_programmers() -> *mut *const c_char;
    fn frwr_catalog_get(index: usize, info: *mut Info) -> c_int;
    fn frwr_context_info(ctx: *mut c_void, info: *mut Info) -> c_int;
    fn frwr_read_region(ctx: *mut c_void, data: *mut u8, start: usize, len: usize,
        cancel: Option<unsafe extern "C" fn(*mut c_void) -> c_int>, user: *mut c_void) -> c_int;
}

unsafe fn string(value: *const c_char) -> String {
    if value.is_null() { String::new() } else { CStr::from_ptr(value).to_string_lossy().into_owned() }
}

pub fn catalog() -> Vec<Definition> {
    let mut result = Vec::new();
    let mut info = Info::empty();
    for i in 0..10_000 {
        if unsafe { frwr_catalog_get(i, &mut info) } == 0 { break; }
        let definition = unsafe { info.definition() };
        if definition.capacity > 0 { result.push(definition); }
    }
    result
}

struct Callbacks { tx: SyncSender<Event>, log: Mutex<File> }
unsafe extern "C" fn log_callback(_level: c_int, message: *const c_char, data: *mut c_void) {
    let sink = &*(data as *const Callbacks);
    let message = string(message);
    if let Ok(mut file) = sink.log.lock() { let _ = file.write_all(message.as_bytes()); }
    if !message.trim().is_empty() { let _ = sink.tx.try_send(Event::Log(message.trim_end().to_owned())); }
}
unsafe extern "C" fn progress_callback(stage: c_int, current: usize, total: usize, data: *mut c_void) {
    let sink = &*(data as *const Callbacks);
    let phase = match stage { 0 => "읽기", 1 => "기록", 2 => "지우기", _ => "검증" };
    let _ = sink.tx.try_send(Event::Progress { phase: phase.into(), fraction: (total > 0).then_some(current as f32 / total.max(1) as f32) });
}
unsafe extern "C" fn is_cancelled(data: *mut c_void) -> c_int { (&*(data as *const AtomicBool)).load(Ordering::Relaxed) as c_int }

pub struct Session {
    ctx: *mut c_void,
    prog: *mut c_void,
    programmer_started: bool,
    callbacks: Box<Callbacks>,
    _guard: MutexGuard<'static, ()>,
}

impl Session {
    pub fn open(device: &Device, hz: u32, folder: &Path, tx: &SyncSender<Event>) -> Result<Self> {
        let guard = LIBRARY.lock().map_err(|_| anyhow::anyhow!("내장 엔진 잠금 오류"))?;
        let callbacks = Box::new(Callbacks { tx: tx.clone(), log: Mutex::new(File::create(folder.join("embedded-engine.log"))?) });
        let mut session = Self { ctx: ptr::null_mut(), prog: ptr::null_mut(), programmer_started: false, callbacks, _guard: guard };
        let user = (&mut *session.callbacks as *mut Callbacks).cast();
        unsafe {
            flashrom_set_log_level(3);
            flashrom_set_log_callback_v2(Some(log_callback), user);
            ensure!(flashrom_init(1) == 0, "내장 flashrom 자체 검사 실패");
            ensure!(flashrom_create_context(&mut session.ctx) == 0, "칩 컨텍스트 생성 실패");
            let params = CString::new(format!("dev={}:115200,spispeed={hz}", device.port))?;
            // Upstream v1.8 does not populate the public programmer pointer.
            // Its global programmer must still be shut down even when that pointer is NULL.
            session.programmer_started = true;
            ensure!(flashrom_programmer_init(&mut session.prog, c"serprog".as_ptr(), params.as_ptr()) == 0, "serprog 연결 실패: 엔진 로그를 확인하세요.");
            flashrom_set_progress_callback_v2(session.ctx, Some(progress_callback), user);
        }
        Ok(session)
    }

    pub fn detect(&mut self, model: Option<&str>) -> Result<(Vec<String>, Option<Definition>)> {
        let model = model.map(CString::new).transpose()?;
        let mut names = ptr::null_mut();
        let count = unsafe { flashrom_flash_probe_v2(self.ctx, &mut names, self.prog, model.as_ref().map_or(ptr::null(), |s| s.as_ptr())) };
        let mut found = Vec::new();
        if !names.is_null() {
            for i in 0..count.max(0) as usize { found.push(unsafe { string(*names.add(i)) }); }
            unsafe { flashrom_data_free(names.cast()); }
        }
        ensure!(count >= 0, "내장 칩 탐색 실패: 엔진 로그를 확인하세요.");
        let definition = if count == 1 {
            let mut info = Info::empty();
            ensure!(unsafe { frwr_context_info(self.ctx, &mut info) } != 0, "탐색한 칩 정보가 없습니다.");
            let definition = unsafe { info.definition() };
            (definition.capacity > 0).then_some(definition)
        } else { None };
        Ok((found, definition))
    }

    pub fn read(&mut self, region: Region, cancel: Option<&AtomicBool>) -> Result<Vec<u8>> {
        let mut data = Vec::new();
        data.try_reserve_exact(region.len).context("읽기 버퍼를 위한 메모리가 부족합니다.")?;
        data.resize(region.len, 0);
        let (callback, user) = match cancel {
            Some(flag) => (Some(is_cancelled as unsafe extern "C" fn(*mut c_void) -> c_int), (flag as *const AtomicBool).cast_mut().cast()),
            None => (None, ptr::null_mut()),
        };
        let status = unsafe { frwr_read_region(self.ctx, data.as_mut_ptr(), region.start, region.len, callback, user) };
        ensure!(status != -2, "읽기를 취소했습니다. 플래시는 변경하지 않았습니다.");
        ensure!(status == 0, "내장 읽기 실패({status}): 엔진 로그를 확인하세요.");
        Ok(data)
    }

    pub fn write(&mut self, target: &mut [u8]) -> Result<()> {
        unsafe {
            flashrom_flag_set(self.ctx, 2, true); // VERIFY_AFTER_WRITE
            flashrom_flag_set(self.ctx, 3, true); // VERIFY_WHOLE_CHIP
            let status = flashrom_image_write(self.ctx, target.as_mut_ptr().cast(), target.len(), ptr::null());
            ensure!(status == 0, "내장 기록/검증 실패({status}). 로그와 선택한 백업 정책을 확인하세요.");
        }
        Ok(())
    }

    pub fn erase_all(&mut self) -> Result<()> {
        let status = unsafe { flashrom_flash_erase(self.ctx) };
        ensure!(status == 0, "내장 전체 지우기 실패({status}). 로그를 확인하세요.");
        Ok(())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() { flashrom_flash_release(self.ctx); }
            if self.programmer_started { flashrom_programmer_shutdown(self.prog); }
            flashrom_shutdown();
            flashrom_set_log_callback_v2(None, ptr::null_mut());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_catalog_is_spi_and_multi_vendor_with_dynamic_sizes() {
        let list = catalog();
        assert!(list.len() > 100);
        assert!(list.iter().any(|d| d.name == "W25Q80BV/W25Q80DV" && d.capacity == 1_048_576));
        assert!(list.iter().any(|d| d.vendor == "SST"));
        assert!(list.iter().any(|d| d.capacity > 16*1024*1024));
        let programmers = unsafe { flashrom_supported_programmers() };
        assert!(!programmers.is_null());
        unsafe {
            assert_eq!(string(*programmers), "serprog");
            assert!((*programmers.add(1)).is_null());
            flashrom_data_free(programmers.cast());
        }
    }
}
