//! Root-window input observer. No global hook, injection, or USB I/O here.
//! Native key and pointer messages share one bounded FIFO before egui translation.
use crate::{keyboard_capture::windows::Gate, model::AppError};
use parking_lot::Mutex;
use std::collections::VecDeque;
use windows::Win32::{
    Foundation::POINT,
    Graphics::Gdi::ScreenToClient,
    UI::{
        Input::KeyboardAndMouse::{GetKeyboardState, MapVirtualKeyW, MAPVK_VK_TO_VSC_EX},
        WindowsAndMessaging::*,
    },
};

#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub vk: u32,
    pub scan: u32,
    pub extended: bool,
    pub down: bool,
    pub modifiers: u8,
}
#[derive(Clone, Debug)]
pub enum Event {
    Key(Key),
    Move {
        x: i32,
        y: i32,
    },
    Button {
        x: i32,
        y: i32,
        button: u8,
        down: bool,
        held: Vec<Key>,
    },
    Wheel {
        x: i32,
        y: i32,
        vertical: i16,
        horizontal: i16,
    },
    Blur,
}
pub struct Message {
    pub gate: Gate,
    pub pixels_per_point: f32,
    pub event: Event,
}
#[derive(Default)]
struct State {
    window: isize,
    pixels_per_point: f32,
    gate: Option<Gate>,
    queue: VecDeque<Message>,
    overflow: bool,
}
#[derive(Default)]
pub struct WindowInput(Mutex<State>);
impl WindowInput {
    pub fn publish(&self, window: isize, gate: Gate, pixels_per_point: f32) {
        let mut s = self.0.lock();
        if s.window != window || s.gate.as_ref().is_none_or(|g| g.epoch != gate.epoch) {
            s.queue.clear();
        }
        s.window = window;
        s.pixels_per_point = pixels_per_point;
        s.gate = Some(gate);
    }
    pub fn clear(&self) {
        let mut s = self.0.lock();
        s.gate = None;
        s.queue.clear();
    }
    pub fn drain(&self) -> Result<Vec<Message>, AppError> {
        let mut s = self.0.lock();
        if std::mem::take(&mut s.overflow) {
            return Err(AppError::new(
                "INPUT_QUEUE_FULL",
                "창 입력 큐가 가득 차 이전 입력을 취소했습니다.",
            ));
        }
        Ok(s.queue.drain(..).collect())
    }
    fn push(s: &mut State, event: Event) {
        let Some(gate) = s.gate.clone().filter(|g| !g.owner.cancelled.is_cancelled()) else {
            return;
        };
        if let (
            Some(Message {
                event: Event::Move { x, y },
                ..
            }),
            Event::Move { x: nx, y: ny },
        ) = (s.queue.back_mut(), &event)
        {
            *x = *nx;
            *y = *ny;
            return;
        }
        if s.queue.len() == 512 {
            gate.owner.cancelled.cancel();
            s.queue.clear();
            s.gate = None;
            s.overflow = true;
            return;
        }
        s.queue.push_back(Message {
            gate,
            pixels_per_point: s.pixels_per_point,
            event,
        });
    }
    /// Called only by winit's message callback while its MSG pointer is valid.
    /// Leaves Windows dispatch unchanged. Special-key consumption belongs to
    /// the native GUI-thread hook before GetMessage returns, not this observer.
    ///
    /// # Safety
    /// `raw` must point to the live Win32 MSG supplied by winit.
    pub unsafe fn observe(&self, raw: *const std::ffi::c_void) -> bool {
        if raw.is_null() {
            return false;
        }
        let msg = unsafe { &*raw.cast::<MSG>() };
        let mut s = self.0.lock();
        if s.gate.is_none() || s.window != msg.hwnd.0 as isize {
            return false;
        }
        if matches!(msg.message, WM_KILLFOCUS | WM_CANCELMODE) {
            Self::push(&mut s, Event::Blur);
            return false;
        }
        if unsafe { GetForegroundWindow() } != msg.hwnd {
            return false;
        }
        let xy = || {
            (
                (msg.lParam.0 as u16 as i16) as i32,
                ((msg.lParam.0 >> 16) as u16 as i16) as i32,
            )
        };
        let event = match msg.message {
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
                let mut k = key(msg.message, msg.wParam.0, msg.lParam.0);
                let mut keys = [0u8; 256];
                if unsafe { GetKeyboardState(&mut keys) }.is_err() {
                    Self::push(&mut s, Event::Blur);
                    return false;
                }
                k.modifiers = modifiers(&keys);
                Event::Key(k)
            }
            WM_MOUSEMOVE => {
                let (x, y) = xy();
                Event::Move { x, y }
            }
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_RBUTTONDOWN
            | WM_RBUTTONDBLCLK | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONDBLCLK
            | WM_MBUTTONUP | WM_XBUTTONDOWN | WM_XBUTTONDBLCLK | WM_XBUTTONUP => {
                let (button, down) = button(msg.message, msg.wParam.0);
                let held = if down {
                    let mut keys = [0u8; 256];
                    if unsafe { GetKeyboardState(&mut keys) }.is_err() {
                        Self::push(&mut s, Event::Blur);
                        return false;
                    }
                    held_keys(&keys)
                } else {
                    Vec::new()
                };
                let (x, y) = xy();
                Event::Button {
                    x,
                    y,
                    button,
                    down,
                    held,
                }
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let (x, y) = xy();
                let mut point = POINT { x, y };
                if !unsafe { ScreenToClient(msg.hwnd, &mut point) }.as_bool() {
                    return false;
                }
                let delta = (msg.wParam.0 >> 16) as u16 as i16;
                Event::Wheel {
                    x: point.x,
                    y: point.y,
                    vertical: if msg.message == WM_MOUSEWHEEL {
                        delta
                    } else {
                        0
                    },
                    horizontal: if msg.message == WM_MOUSEHWHEEL {
                        delta
                    } else {
                        0
                    },
                }
            }
            _ => return false,
        };
        Self::push(&mut s, event);
        false
    }
}
fn key(message: u32, wp: usize, lp: isize) -> Key {
    Key {
        vk: wp as u32,
        scan: ((lp >> 16) & 255) as u32,
        extended: lp & (1 << 24) != 0,
        down: matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN),
        modifiers: 0,
    }
}
fn button(message: u32, wp: usize) -> (u8, bool) {
    (
        match message {
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP => 1,
            WM_RBUTTONDOWN | WM_RBUTTONDBLCLK | WM_RBUTTONUP => 2,
            WM_MBUTTONDOWN | WM_MBUTTONDBLCLK | WM_MBUTTONUP => 3,
            _ => {
                if wp >> 16 == 1 {
                    4
                } else {
                    5
                }
            }
        },
        matches!(
            message,
            WM_LBUTTONDOWN
                | WM_LBUTTONDBLCLK
                | WM_RBUTTONDOWN
                | WM_RBUTTONDBLCLK
                | WM_MBUTTONDOWN
                | WM_MBUTTONDBLCLK
                | WM_XBUTTONDOWN
                | WM_XBUTTONDBLCLK
        ),
    )
}
pub(crate) fn individual_key(vk: u32) -> bool {
    // Generic aliases would invent a second (left) modifier for a held right key.
    !matches!(vk, 0x10..=0x12)
}
fn held_keys(state: &[u8; 256]) -> Vec<Key> {
    state
        .iter()
        .enumerate()
        .filter_map(|(vk, value)| {
            let vk = vk as u32;
            if value & 128 == 0 || !individual_key(vk) {
                return None;
            }
            Some(snapshot_key(vk))
        })
        .collect()
}
pub(crate) fn snapshot_key(vk: u32) -> Key {
    // MapVirtualKey is layout-dependent (notably Korean right Alt/Ctrl).
    // Side-specific modifier VKs have fixed physical Set1 identities.
    let scan = match vk {
        0xa0 => 0x2a,
        0xa1 => 0x36,
        0xa2 => 0x1d,
        0xa3 => 0xe01d,
        0xa4 => 0x38,
        0xa5 => 0xe038,
        _ => unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX) },
    };
    Key {
        vk,
        scan: scan & 255,
        extended: scan & 0xff00 != 0,
        down: true,
        modifiers: 0,
    }
}
fn modifiers(state: &[u8; 256]) -> u8 {
    u8::from(state[0x11] & 128 != 0)
        | (u8::from(state[0x12] & 128 != 0) << 1)
        | (u8::from((state[0x5b] | state[0x5c]) & 128 != 0) << 2)
}
pub fn ordinary_route(
    policy: &mut crate::keyboard_capture::Policy,
    k: Key,
    focused: bool,
) -> crate::keyboard_capture::Route {
    let usage = crate::keyboard_capture::usage(k.vk, k.scan, k.extended);
    let special = usage.is_some_and(|u| matches!(u,0xe3|0xe7|0x65|0x46..=0x48|0x87..=0x98));
    let host_shortcut = k.down
        && (special
            || k.modifiers & 4 != 0
            || (k.modifiers & 2 != 0 && matches!(usage, Some(0x2b | 0x3d)))
            || (k.modifiers & 3 == 3 && usage == Some(0x4c)));
    policy.route(
        k.vk,
        k.scan,
        k.extended,
        k.down,
        focused,
        !focused || host_shortcut,
    )
}

#[cfg(test)]
mod tests;
