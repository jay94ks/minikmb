//! Scoped host keyboard routing. Only transient held-key state is retained.
//! The pure policy is tested without installing a Windows hook or injecting keys.
mod status;
#[cfg(windows)]
pub mod windows;
pub use status::{Phase, Status};

#[derive(Clone, Copy, PartialEq)]
enum Held {
    Host,
    Suppressed { usage: u16, epoch: Option<u64> },
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Event {
    Key { epoch: u64, usage: u16, down: bool },
    Release { epoch: u64 },
    HostUp { vk: u32, scan: u32, extended: bool },
}
#[derive(Default)]
pub struct Route {
    pub swallow: bool,
    pub event: Option<Event>,
    pub release: Option<Event>,
    pub host_up: Option<Event>,
}
pub struct Policy {
    held: [Option<Held>; 768],
    epoch: Option<u64>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            held: [None; 768],
            epoch: None,
        }
    }
}
fn token(vk: u32, scan: u32, extended: bool) -> Option<usize> {
    if scan == 0 {
        (vk < 256).then_some(512 + vk as usize)
    } else {
        (scan < 256).then_some(scan as usize + if extended { 256 } else { 0 })
    }
}
impl Policy {
    pub fn arm(&mut self, epoch: u64) -> Option<Event> {
        let release = self.disarm();
        // A fresh OS snapshot follows arm(). Host-owned keys may have been
        // released while the dormant hook was absent; only suppressed UP markers
        // must survive between capture epochs.
        for key in &mut self.held {
            if matches!(key, Some(Held::Host)) {
                *key = None;
            }
        }
        self.epoch = Some(epoch);
        release
    }
    pub fn disarm(&mut self) -> Option<Event> {
        let epoch = self.epoch.take();
        for key in &mut self.held {
            if let Some(Held::Suppressed { epoch, .. }) = key {
                *epoch = None;
            }
        }
        epoch.map(|epoch| Event::Release { epoch })
    }
    pub fn host_held(&mut self, vk: u32, scan: u32, extended: bool) {
        if usage(vk, scan, extended).is_some() {
            if let Some(token) = token(vk, scan, extended) {
                if self.held[token].is_none() {
                    self.held[token] = Some(Held::Host);
                }
            }
        }
    }
    pub fn host_up(&mut self, vk: u32, scan: u32, extended: bool) {
        if let Some(token) = token(vk, scan, extended) {
            if self.held[token] == Some(Held::Host) {
                self.held[token] = None;
            }
        }
    }
    pub fn has_suppressed(&self) -> bool {
        self.held
            .iter()
            .any(|key| matches!(key, Some(Held::Suppressed { .. })))
    }
    pub fn suppressed_ctrl_alt(&self) -> bool {
        let held = |usages: &[u16]| {
            self.held.iter().any(|key|matches!(key,Some(Held::Suppressed{usage,epoch:Some(_)} ) if usages.contains(usage)))
        };
        held(&[0xe0, 0xe4]) && held(&[0xe2, 0xe6])
    }
    pub fn route(
        &mut self,
        vk: u32,
        scan: u32,
        extended: bool,
        down: bool,
        foreground: bool,
        secure_attention: bool,
    ) -> Route {
        let release = if !foreground || secure_attention {
            self.disarm()
        } else {
            None
        };
        let mut route = Route {
            release,
            ..Default::default()
        };
        let Some(token) = token(vk, scan, extended) else {
            return route;
        };
        if let Some(held) = self.held[token] {
            if !down {
                self.held[token] = None;
            }
            match held {
                Held::Host => return route,
                Held::Suppressed { usage, epoch } => {
                    route.swallow = true;
                    if !down && epoch == self.epoch {
                        route.event = epoch.map(|epoch| Event::Key {
                            epoch,
                            usage,
                            down: false,
                        });
                    }
                    if !down && route.event.is_none() {
                        route.host_up = Some(Event::HostUp { vk, scan, extended });
                    }
                    return route;
                }
            }
        }
        // Never capture an unmatched UP or secure-attention key.
        if !down || !foreground || secure_attention {
            return route;
        }
        let (Some(epoch), Some(usage)) = (self.epoch, usage(vk, scan, extended)) else {
            return route;
        };
        self.held[token] = Some(Held::Suppressed {
            usage,
            epoch: Some(epoch),
        });
        route.swallow = true;
        route.event = Some(Event::Key {
            epoch,
            usage,
            down: true,
        });
        route
    }
}
pub fn usage(vk: u32, scan: u32, extended: bool) -> Option<u16> {
    let special = match vk {
        0x15 => Some(0x90),
        0x19 => Some(0x91),
        0x13 => Some(0x48),
        0x2c => Some(0x46),
        0x7c..=0x87 => Some(0x68 + (vk - 0x7c) as u16),
        0xad => Some(0x7f),
        0xaf => Some(0x80),
        0xae => Some(0x81),
        _ => None,
    };
    if special.is_some() {
        return special;
    }
    if scan == 0 {
        return match vk {
            0x41..=0x5a => Some(4 + (vk - 0x41) as u16),
            0xa0 => Some(0xe1),
            0xa1 => Some(0xe5),
            0xa2 | 0x11 => Some(0xe0),
            0xa3 => Some(0xe4),
            0xa4 | 0x12 => Some(0xe2),
            0xa5 => Some(0xe6),
            0x5b => Some(0xe3),
            0x5c => Some(0xe7),
            _ => None,
        };
    }
    if extended {
        return match scan {
            0x1c => Some(0x58),
            0x1d => Some(0xe4),
            0x35 => Some(0x54),
            0x37 => Some(0x46),
            0x38 => Some(0xe6),
            0x47 => Some(0x4a),
            0x48 => Some(0x52),
            0x49 => Some(0x4b),
            0x4b => Some(0x50),
            0x4d => Some(0x4f),
            0x4f => Some(0x4d),
            0x50 => Some(0x51),
            0x51 => Some(0x4e),
            0x52 => Some(0x49),
            0x53 => Some(0x4c),
            0x5b => Some(0xe3),
            0x5c => Some(0xe7),
            0x5d => Some(0x65),
            _ => None,
        };
    }
    // Physical keys present in the v1 DOM map but outside the 89-entry PC table.
    // Microsoft Scan 1 Make table: International1/3 and LANG1/2. VK_HANGUL/
    // VK_HANJA above still cover the Korean 101-key right Alt/Ctrl variants.
    match scan {
        0x73 => return Some(0x87), // IntlRo
        0x7d => return Some(0x89), // IntlYen
        0x72 | 0xf2 => return Some(0x90),
        0x71 | 0xf1 => return Some(0x91),
        _ => {}
    }
    const SET1: [u8; 89] = [
        0, 0x29, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x2d, 0x2e, 0x2a,
        0x2b, 0x14, 0x1a, 0x08, 0x15, 0x17, 0x1c, 0x18, 0x0c, 0x12, 0x13, 0x2f, 0x30, 0x28, 0xe0,
        0x04, 0x16, 0x07, 0x09, 0x0a, 0x0b, 0x0d, 0x0e, 0x0f, 0x33, 0x34, 0x35, 0xe1, 0x31, 0x1d,
        0x1b, 0x06, 0x19, 0x05, 0x11, 0x10, 0x36, 0x37, 0x38, 0xe5, 0x55, 0xe2, 0x2c, 0x39, 0x3a,
        0x3b, 0x3c, 0x3d, 0x3e, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x53, 0x47, 0x5f, 0x60, 0x61, 0x56,
        0x5c, 0x5d, 0x5e, 0x57, 0x59, 0x5a, 0x5b, 0x62, 0x63, 0, 0, 0x64, 0x44, 0x45,
    ];
    SET1.get(scan as usize)
        .copied()
        .filter(|n| *n != 0)
        .map(u16::from)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_legacy_set1_fixture_for_every_scan_code() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/keyboard-capture-v1.json")).unwrap();
        let legacy = fixture["set1"].as_array().unwrap();
        assert_eq!(legacy.len(), 89);
        for (scan, value) in legacy.iter().enumerate() {
            let value = value.as_u64().unwrap() as u16;
            assert_eq!(
                usage(0, scan as u32, false),
                (value != 0).then_some(value),
                "scan {scan:02x}"
            );
        }
        assert!(usage(0, 0xff, false).is_none());
    }
    #[test]
    fn win_ime_keypad_mapping_and_original_up() {
        assert_eq!(usage(0x5b, 0x5b, true), Some(0xe3));
        assert_eq!(usage(0x15, 0x38, true), Some(0x90));
        assert_eq!(usage(0x19, 0x1d, true), Some(0x91));
        assert_eq!(usage(0x60, 0x52, false), Some(0x62));
        assert_eq!(usage(0x2d, 0x52, true), Some(0x49));
        assert_eq!(usage(0x87, 0, false), Some(0x73));
        let mut p = Policy::default();
        p.arm(1);
        assert_eq!(
            p.route(0x15, 0x38, true, true, true, false).event,
            Some(Event::Key {
                epoch: 1,
                usage: 0x90,
                down: true
            })
        );
        let up = p.route(0xa5, 0x38, true, false, true, false);
        assert!(up.swallow);
        assert_eq!(
            up.event,
            Some(Event::Key {
                epoch: 1,
                usage: 0x90,
                down: false
            })
        );
    }
    #[test]
    fn blur_tombstones_repeat_and_new_generation_do_not_retarget() {
        let mut p = Policy::default();
        assert!(!p.route(0x41, 0x1e, false, true, true, false).swallow);
        p.arm(1);
        assert!(p.route(0x5b, 0x5b, true, true, true, false).swallow);
        assert!(p.route(0x5b, 0x5b, true, true, true, false).event.is_none());
        assert_eq!(
            p.route(0x42, 0x30, false, true, false, false).release,
            Some(Event::Release { epoch: 1 })
        );
        p.arm(2);
        let up = p.route(0x5b, 0x5b, true, false, true, false);
        assert!(up.swallow);
        assert!(up.event.is_none());
        assert!(!p.has_suppressed());
        assert!(p
            .route(0x41, 0x1e, false, false, true, false)
            .event
            .is_none());
    }
    #[test]
    fn preheld_host_keys_secure_attention_and_zero_scan_are_distinct() {
        let mut p = Policy::default();
        p.arm(1);
        p.host_held(0xa2, 0x1d, false);
        assert!(!p.route(0xa2, 0x1d, false, true, true, false).swallow);
        assert!(!p.route(0xa2, 0x1d, false, false, true, false).swallow);
        assert!(p.route(0xad, 0, false, true, true, false).swallow);
        assert!(p.route(0xaf, 0, false, true, true, false).swallow);
        assert_eq!(
            p.route(0xad, 0, false, false, true, false).event,
            Some(Event::Key {
                epoch: 1,
                usage: 0x7f,
                down: false
            })
        );
        let sas = p.route(0x2e, 0x53, true, true, true, true);
        assert!(!sas.swallow);
        assert_eq!(sas.release, Some(Event::Release { epoch: 1 }));
    }
    #[test]
    fn suppressed_modifiers_still_exclude_secure_attention() {
        let mut p = Policy::default();
        p.arm(1);
        p.route(0xa2, 0x1d, false, true, true, false);
        p.route(0xa4, 0x38, false, true, true, false);
        assert!(p.suppressed_ctrl_alt());
        let route = p.route(0x2e, 0x53, true, true, true, p.suppressed_ctrl_alt());
        assert!(!route.swallow);
        assert_eq!(route.release, Some(Event::Release { epoch: 1 }));
        assert!(!p.suppressed_ctrl_alt());
    }
}
#[cfg(test)]
pub(crate) mod parity_tests;
