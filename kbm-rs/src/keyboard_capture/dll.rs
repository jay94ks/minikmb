//! Same-process C++ hook thread. FFI only polls memory; no IPC or Rust callback.
use super::Gate;
use crate::{
    active::ActiveCoordinator,
    keyboard_capture::{Event, Phase, Status},
    model::AppError,
};
use std::{
    cell::RefCell, collections::VecDeque, ffi::OsStr, os::windows::ffi::OsStrExt, sync::Arc,
};
use windows::{
    core::{PCSTR, PCWSTR},
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{GetProcAddress, LoadLibraryW},
    },
};

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativeEvent {
    kind: u8,
    pad: [u8; 3],
    epoch: u32,
    usage: u32,
    down: u32,
}
#[repr(C)]
#[derive(Default)]
struct NativeStats {
    size: u32,
    flags: u32,
    error: u32,
    thread_id: u32,
    epoch: u32,
    desired: u32,
    callbacks: u64,
    swallowed: u64,
    window_callbacks: u64,
    window_swallowed: u64,
}
type Start = unsafe extern "C" fn(usize, u32, u32) -> u32;
type Enable = unsafe extern "C" fn(u32) -> u32;
type Refresh = unsafe extern "C" fn(u32);
type Poll = unsafe extern "C" fn(*mut NativeEvent, u32) -> u32;
type GetStatus = unsafe extern "C" fn(*mut NativeStats);
type Stop = unsafe extern "C" fn() -> u32;
type WindowKey = unsafe extern "C" fn(u32, u32, u32, u32, u32) -> u32;
#[cfg(test)]
type TestRoute = unsafe extern "C" fn(u32, u32, u32, u32, u32) -> u32;
struct Native {
    window_key: WindowKey,
    module: HMODULE,
    enable: Enable,
    refresh: Refresh,
    poll: Poll,
    status: GetStatus,
    stop: Stop,
    started: bool,
    #[cfg(test)]
    test_route: TestRoute,
}
impl Native {
    fn load(window: isize, testing: bool) -> Result<Self, AppError> {
        let path = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("tools/minikmb-keyboard.dll")))
            .ok_or_else(|| error("네이티브 키보드 DLL 경로를 확인하지 못했습니다."))?;
        let wide: Vec<u16> = OsStr::new(&path).encode_wide().chain(Some(0)).collect();
        let module = unsafe { LoadLibraryW(PCWSTR(wide.as_ptr())) }
            .map_err(|_| error("동봉된 tools/minikmb-keyboard.dll을 불러오지 못했습니다."))?;
        unsafe fn get<T: Copy>(module: HMODULE, name: &'static [u8]) -> Result<T, AppError> {
            let address = unsafe { GetProcAddress(module, PCSTR(name.as_ptr())) }
                .ok_or_else(|| error("네이티브 키보드 DLL 버전이 맞지 않습니다."))?;
            assert_eq!(std::mem::size_of::<T>(), std::mem::size_of_val(&address));
            Ok(unsafe { std::mem::transmute_copy(&address) })
        }
        let loaded: Result<(Start, Self), AppError> = (|| unsafe {
            let start: Start = get(module, b"kbm_start\0")?;
            let native = Self {
                window_key: get(module, b"kbm_window_key\0")?,
                module,
                enable: get(module, b"kbm_enable\0")?,
                refresh: get(module, b"kbm_refresh\0")?,
                poll: get(module, b"kbm_poll\0")?,
                status: get(module, b"kbm_status_v2\0")?,
                stop: get(module, b"kbm_stop\0")?,
                started: false,
                #[cfg(test)]
                test_route: get(module, b"kbm_test_route\0")?,
            };
            Ok((start, native))
        })();
        let (start, mut native) = match loaded {
            Ok(value) => value,
            Err(e) => {
                unsafe {
                    let _ = FreeLibrary(module);
                }
                return Err(e);
            }
        };
        let result = unsafe { start(window as usize, std::process::id(), u32::from(testing)) };
        native.started = result == 1 || result == 3;
        if result != 1 {
            return Err(error("네이티브 키보드 스레드를 시작하지 못했습니다."));
        }
        Ok(native)
    }
    fn stats(&self) -> NativeStats {
        let mut s = NativeStats::default();
        unsafe {
            (self.status)(&mut s);
        }
        s
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        if self.started && unsafe { (self.stop)() } == 0 {
            return;
        }
        unsafe {
            let _ = FreeLibrary(self.module);
        }
    }
}
fn error(message: &str) -> AppError {
    AppError::new("KEYBOARD_CAPTURE_FAILED", message)
}
#[derive(Default)]
struct State {
    gate: Option<Gate>,
    desired: u64,
    phase: Phase,
    failed: bool,
    events: VecDeque<Event>,
    received: u64,
    queued: u64,
    rejected: u64,
}
pub struct Capture {
    native: Native,
    active: Arc<ActiveCoordinator>,
    state: RefCell<State>,
}
impl Capture {
    pub fn new(window: isize, active: Arc<ActiveCoordinator>) -> Result<Self, AppError> {
        Self::start(window, active, false)
    }
    fn start(
        window: isize,
        active: Arc<ActiveCoordinator>,
        testing: bool,
    ) -> Result<Self, AppError> {
        Ok(Self {
            native: Native::load(window, testing)?,
            active,
            state: RefCell::new(State::default()),
        })
    }
    fn collect(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(g) = state.gate.as_ref() {
            if g.valid(&self.active) && state.desired == g.epoch {
                unsafe {
                    (self.native.refresh)(g.epoch as u32);
                }
            } else {
                unsafe {
                    (self.native.enable)(0);
                }
                state.desired = 0;
                state.phase = Phase::Disabled;
                if let Some(g) = state.gate.take() {
                    g.owner.cancelled.cancel();
                    state.events.push_back(Event::Release { epoch: g.epoch });
                }
            }
        }
        let mut batch = [NativeEvent::default(); 128];
        for _ in 0..4 {
            let count =
                unsafe { (self.native.poll)(batch.as_mut_ptr(), batch.len() as u32) } as usize;
            if count > batch.len() {
                state.failed = true;
                break;
            }
            for event in &batch[..count] {
                let epoch = u64::from(event.epoch);
                match event.kind {
                    b'S' if state.desired == epoch && epoch != 0 => {
                        state.phase = if event.usage == 1 {
                            Phase::Armed
                        } else {
                            Phase::WaitingForeground
                        };
                    }
                    b'R' if state.gate.as_ref().is_some_and(|g| g.epoch == epoch) => {
                        if let Some(g) = state.gate.take() {
                            g.owner.cancelled.cancel();
                        }
                        state.desired = 0;
                        state.phase = Phase::Disabled;
                        state.events.push_back(Event::Release { epoch });
                    }
                    b'K' => {
                        state.received += 1;
                        if state.desired == epoch
                            && state
                                .gate
                                .as_ref()
                                .is_some_and(|g| g.epoch == epoch && g.valid(&self.active))
                        {
                            let usage = event.usage as u16;
                            if ((4..=0xa4).contains(&usage) || (0xe0..=0xe7).contains(&usage))
                                && event.down <= 1
                            {
                                state.queued += 1;
                                state.events.push_back(Event::Key {
                                    epoch,
                                    usage,
                                    down: event.down != 0,
                                });
                            } else {
                                state.failed = true;
                            }
                        } else {
                            state.rejected += 1;
                        }
                    }
                    b'U' if event.epoch < 256 && event.usage < 256 && event.down <= 1 => {
                        state.events.push_back(Event::HostUp {
                            vk: event.epoch,
                            scan: event.usage,
                            extended: event.down != 0,
                        })
                    }
                    _ => {}
                }
                if state.events.len() > 512 {
                    state.failed = true;
                    break;
                }
            }
            if count < batch.len() || state.failed {
                break;
            }
        }
        let stats = self.native.stats();
        if stats.flags & 8 != 0 {
            state.failed = true;
        }
        if state.failed {
            unsafe {
                (self.native.enable)(0);
            }
            state.desired = 0;
            state.phase = Phase::ThreadFailed;
        }
    }
    pub fn arm(&self, gate: Gate) -> Result<(), AppError> {
        if gate.epoch == 0
            || gate.epoch > u32::MAX as u64
            || !gate.valid(&self.active)
            || self.failed()
        {
            return Err(error("특수 키 입력 소유권을 확인하지 못했습니다."));
        }
        let mut s = self.state.borrow_mut();
        if s.gate
            .as_ref()
            .is_some_and(|g| g.epoch == gate.epoch && g.owner.id == gate.owner.id)
        {
            return Ok(());
        }
        if let Some(g) = s.gate.take() {
            g.owner.cancelled.cancel();
            s.events.push_back(Event::Release { epoch: g.epoch });
        }
        s.desired = gate.epoch;
        s.phase = Phase::Requested;
        let epoch = gate.epoch as u32;
        s.gate = Some(gate);
        if unsafe { (self.native.enable)(epoch) } == 0 {
            s.failed = true;
            return Err(error("네이티브 키보드 활성화에 실패했습니다."));
        }
        Ok(())
    }
    pub fn disarm(&self) {
        unsafe {
            (self.native.enable)(0);
        }
        let mut s = self.state.borrow_mut();
        s.desired = 0;
        s.phase = Phase::Disabled;
        s.events.retain(|e| matches!(e, Event::HostUp { .. }));
        if let Some(g) = s.gate.take() {
            g.owner.cancelled.cancel();
            s.events.push_back(Event::Release { epoch: g.epoch });
        }
    }
    pub fn window_key(&self, epoch: u64, key: crate::window_input::Key) -> Result<(), AppError> {
        let s = self.state.borrow();
        if s.desired != epoch
            || !s
                .gate
                .as_ref()
                .is_some_and(|g| g.epoch == epoch && g.valid(&self.active))
        {
            return Err(error("창 키 입력의 소유권이 종료됐습니다."));
        }
        if unsafe {
            (self.native.window_key)(
                epoch as u32,
                key.vk,
                key.scan,
                u32::from(key.extended),
                u32::from(key.down),
            )
        } == 0
        {
            return Err(error("창 키 입력을 네이티브 큐에 전달하지 못했습니다."));
        }
        Ok(())
    }
    pub fn status(&self) -> Status {
        self.collect();
        let n = self.native.stats();
        let s = self.state.borrow();
        Status {
            helper_pid: None,
            native_thread_id: Some(n.thread_id),
            phase: s.phase,
            requested_epoch: s.desired,
            armed_epoch: u64::from(n.epoch),
            installed: n.flags & 1 != 0,
            foreground: n.flags & 2 != 0,
            install_count: 1,
            callbacks: n.callbacks,
            reentrant_callbacks: 0,
            swallowed: n.swallowed,
            window_callbacks: n.window_callbacks,
            window_swallowed: n.window_swallowed,
            win32_error: (n.error != 0).then_some(n.error),
            received_events: s.received,
            queued_events: s.queued,
            rejected_events: s.rejected,
        }
    }
    pub fn events(&self) -> Vec<Event> {
        self.collect();
        self.state.borrow_mut().events.drain(..).collect()
    }
    pub fn armed(&self, epoch: u64) -> bool {
        let s = self.status();
        s.active() && s.armed_epoch == epoch
    }
    pub fn failed(&self) -> bool {
        self.collect();
        self.state.borrow().failed
    }
    pub fn finished(&self) -> bool {
        self.native.stats().flags & 8 != 0
    }
    pub fn retry(&self) {}
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.disarm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    fn window() -> windows::Win32::Foundation::HWND {
        use windows::Win32::UI::WindowsAndMessaging::*;
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::w!("STATIC"),
                windows::core::w!("Native DLL hidden test"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap()
    }
    #[test]
    fn actual_dll_hook_is_in_this_process_and_hidden_window_never_arms() {
        let _lock = LOCK.blocking_lock();
        let w = window();
        let c = Capture::new(w.0 as isize, Arc::new(ActiveCoordinator::default())).unwrap();
        let s = c.status();
        assert!(s.installed);
        assert!(s.helper_pid.is_none());
        assert_eq!(
            s.native_thread_id,
            Some(unsafe { windows::Win32::System::Threading::GetCurrentThreadId() })
        );
        assert_eq!(s.armed_epoch, 0);
        assert_eq!(s.swallowed, 0);
        // Only a hidden test HWND receives synthetic queue messages. Neither
        // real OS input nor MCU HID is generated, and capture remains unarmed.
        unsafe {
            use windows::Win32::{
                Foundation::{LPARAM, WPARAM},
                UI::WindowsAndMessaging::*,
            };
            PostMessageW(Some(w), WM_KEYDOWN, WPARAM(0x87), LPARAM(0x76 << 16)).unwrap();
            PostMessageW(
                Some(w),
                WM_KEYUP,
                WPARAM(0x87),
                LPARAM((0x76 << 16) | (1isize << 31)),
            )
            .unwrap();
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(w), WM_KEYDOWN, WM_KEYUP, PM_REMOVE).as_bool() {}
        }
        assert!(c.status().window_callbacks >= 2);
        assert_eq!(c.status().window_swallowed, 0);
        assert!(Capture::new(w.0 as isize, Arc::new(ActiveCoordinator::default())).is_err());
        assert!(c.status().installed); // A rejected second owner must not stop this session.
        drop(c);
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::DestroyWindow(w).unwrap();
        }
    }
    #[tokio::test]
    async fn dll_queue_win_down_up_reaches_original_mock_mcu_and_blur_releases() {
        let _lock = LOCK.lock().await;
        let (_dir, app, wire) = crate::input_api::tests::setup();
        let w = window();
        let c = Capture::start(w.0 as isize, app.active.clone(), true).unwrap();
        let gate = Gate {
            epoch: 19,
            binding: app.active.bind().unwrap(),
            owner: crate::input::Owner {
                pin: Default::default(),
                id: "dll-win-test".into(),
                cancelled: Default::default(),
                command_guard: None,
            },
        };
        app.input.prepare_owner(&gate.owner, &gate.binding).unwrap();
        c.arm(gate.clone()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while c.status().phase != Phase::Armed {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert!(!c.status().installed); // Pure native event test mode installs no OS hook.
        unsafe {
            assert_eq!((c.native.test_route)(0x5b, 0x5b, 1, 1, 1), 1);
        }
        let key = crate::window_input::Key {
            vk: 0x5b,
            scan: 0x5b,
            extended: true,
            down: true,
            modifiers: 0,
        };
        c.window_key(19, key).unwrap();
        c.window_key(19, crate::window_input::Key { down: false, ..key })
            .unwrap();
        let input = crate::ui_input::UiInput::new(
            &app,
            &tokio::runtime::Handle::current(),
            Default::default(),
        );
        while wire.calls.lock().len() < 2 {
            for event in c.events() {
                gate.dispatch(&app.active, &input, event).unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        {
            let calls = wire.calls.lock();
            assert_eq!(calls[0].1, 0x101);
            assert_eq!(calls[1].1, 0x102);
            assert_eq!(calls[0].2, vec![0xe3, 0]);
            assert_eq!(calls[1].2, vec![0xe3, 0]);
        }
        let ordinary = crate::window_input::Key {
            vk: 0x41,
            scan: 0x1e,
            extended: false,
            down: true,
            modifiers: 0,
        };
        c.window_key(19, ordinary).unwrap();
        c.window_key(
            19,
            crate::window_input::Key {
                down: false,
                ..ordinary
            },
        )
        .unwrap();
        while wire.calls.lock().len() < 4 {
            for event in c.events() {
                gate.dispatch(&app.active, &input, event).unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        {
            let calls = wire.calls.lock();
            assert_eq!(calls[2].2, vec![4, 0]);
            assert_eq!(calls[3].2, vec![4, 0]);
        }
        // A language toggle releases the target immediately, even when the
        // platform never supplies UP. Repeated DOWN does not toggle again.
        let language = crate::window_input::Key {
            vk: 0x15,
            scan: 0x38,
            extended: true,
            down: true,
            modifiers: 0,
        };
        c.window_key(19, language).unwrap();
        c.window_key(19, language).unwrap();
        c.window_key(
            19,
            crate::window_input::Key {
                down: false,
                ..language
            },
        )
        .unwrap();
        while wire.calls.lock().len() < 6 {
            for event in c.events() {
                gate.dispatch(&app.active, &input, event).unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        {
            let calls = wire.calls.lock();
            assert_eq!((calls[4].1, &calls[4].2), (0x101, &vec![0x90, 0]));
            assert_eq!((calls[5].1, &calls[5].2), (0x102, &vec![0x90, 0]));
        }
        // Dedicated LANG scan codes are standalone strokes. Consecutive
        // presses with no complementary edge must each reach the MCU once.
        for key in [
            crate::window_input::Key {
                vk: 0x15,
                scan: 0xf2,
                extended: true,
                down: true,
                modifiers: 0,
            },
            crate::window_input::Key {
                vk: 0x19,
                scan: 0x71,
                extended: false,
                down: false,
                modifiers: 0,
            },
        ] {
            for _ in 0..3 {
                c.window_key(19, key).unwrap();
            }
        }
        while wire.calls.lock().len() < 18 {
            for event in c.events() {
                gate.dispatch(&app.active, &input, event).unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        {
            let calls = wire.calls.lock();
            for (i, usage) in [0x90, 0x90, 0x90, 0x91, 0x91, 0x91].into_iter().enumerate() {
                assert_eq!(
                    (calls[6 + 2 * i].1, &calls[6 + 2 * i].2),
                    (0x101, &vec![usage, 0])
                );
                assert_eq!(
                    (calls[7 + 2 * i].1, &calls[7 + 2 * i].2),
                    (0x102, &vec![usage, 0])
                );
            }
        }
        unsafe {
            (c.native.test_route)(0x41, 0x1e, 0, 1, 0);
        }
        loop {
            let events = c.events();
            if events.contains(&Event::Release { epoch: 19 }) {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        assert!(gate.owner.cancelled.is_cancelled());
        assert_eq!(wire.calls.lock().len(), 18);
        input.stop();
        drop(c);
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::DestroyWindow(w).unwrap();
        }
    }
    #[tokio::test]
    async fn native_lease_expires_without_gui_polling_and_releases_owner_without_hid() {
        let _lock = LOCK.lock().await;
        let (_dir, app, wire) = crate::input_api::tests::setup();
        let w = window();
        let c = Capture::start(w.0 as isize, app.active.clone(), true).unwrap();
        let owner = crate::input::Owner {
            pin: Default::default(),
            id: "dll-lease".into(),
            cancelled: Default::default(),
            command_guard: None,
        };
        c.arm(Gate {
            epoch: 29,
            binding: app.active.bind().unwrap(),
            owner: owner.clone(),
        })
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while c.status().phase != Phase::Armed {
            assert!(std::time::Instant::now() < deadline);
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(850)).await;
        assert_eq!(c.native.stats().epoch, 0); // Native timer stopped capture while GUI did not poll.
        assert!(c.events().contains(&Event::Release { epoch: 29 }));
        assert!(owner.cancelled.is_cancelled());
        assert!(wire.calls.lock().is_empty());
        drop(c);
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::DestroyWindow(w).unwrap();
        }
    }
}
