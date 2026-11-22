use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
struct Source {
    path: String,
    sha256: String,
}
#[derive(Deserialize)]
pub(crate) struct Key {
    pub code: String,
    pub vk: u32,
    pub scan: u32,
    pub extended: bool,
    pub usage: u16,
    pub special: bool,
}
#[derive(Deserialize)]
struct Fixture {
    sources: Vec<Source>,
    keys: Vec<Key>,
}
pub(crate) fn keys() -> Vec<Key> {
    let f: Fixture =
        serde_json::from_str(include_str!("../../tests/keyboard-dom-v1.json")).unwrap();
    let sources = [
        include_str!("../../../kbm/src/services/hid-keyboard.ts"),
        include_str!("../../../kbm/src/services/mini-keyboard.ts"),
    ];
    assert_eq!(f.sources.len(), sources.len());
    for (expected, source) in f.sources.iter().zip(sources) {
        assert_eq!(
            format!("{:x}", Sha256::digest(source.as_bytes())),
            expected.sha256,
            "{} changed: rerun scripts/export-keyboard-fixture.cjs and review parity",
            expected.path
        );
    }
    assert!(f.keys.len() > 100);
    f.keys
}
#[test]
fn every_v1_dom_key_maps_to_the_same_usage_and_valid_wire_packet() {
    for key in keys() {
        assert_eq!(
            usage(key.vk, key.scan, key.extended),
            Some(key.usage),
            "{}",
            key.code
        );
        for down in [true, false] {
            let (op, body) = crate::input::Action::Key {
                usage: key.usage,
                down,
            }
            .packet()
            .unwrap();
            assert_eq!(op, if down { 0x101 } else { 0x102 });
            assert_eq!(body, key.usage.to_le_bytes(), "{}", key.code);
        }
    }
}
#[cfg(windows)]
#[test]
fn native_mapping_matches_all_v1_keys_without_installing_hook() {
    use std::{
        io::Write,
        os::windows::process::CommandExt,
        process::{Command, Stdio},
    };
    let exe = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("tools/minikmb-keyboard.exe");
    let mut child = Command::new(exe)
        .arg("--map-test")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .creation_flags(0x08000000)
        .spawn()
        .unwrap();
    let keys = keys();
    let mut stdin = child.stdin.take().unwrap();
    for k in &keys {
        writeln!(stdin, "{} {} {}", k.vk, k.scan, u8::from(k.extended)).unwrap();
    }
    drop(stdin);
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    let output = String::from_utf8(result.stdout).unwrap();
    let values: Vec<u16> = output.lines().map(|s| s.parse().unwrap()).collect();
    assert_eq!(values.len(), keys.len());
    for (actual, key) in values.into_iter().zip(keys) {
        assert_eq!(actual, key.usage, "{}", key.code);
    }
}
#[test]
fn every_v1_dom_key_uses_original_usage_for_up_and_never_repeats_down() {
    for k in keys() {
        let mut p = Policy::default();
        p.arm(18);
        let down = p.route(k.vk, k.scan, k.extended, true, true, false);
        assert_eq!(
            down.event,
            Some(Event::Key {
                epoch: 18,
                usage: k.usage,
                down: true
            }),
            "{}",
            k.code
        );
        assert!(down.swallow);
        assert!(p
            .route(k.vk, k.scan, k.extended, true, true, false)
            .event
            .is_none());
        // Even if IME turns the logical key into VK_PROCESSKEY, the same
        // physical UP must release the usage selected by the original DOWN.
        let up = p.route(0xe5, k.scan, k.extended, false, true, false);
        assert_eq!(
            up.event,
            Some(Event::Key {
                epoch: 18,
                usage: k.usage,
                down: false
            }),
            "{}",
            k.code
        );
        assert!(up.swallow);
        assert!(!p.has_suppressed());
    }
}
#[test]
fn international_physical_variants_do_not_alias_extended_navigation_or_new_epochs() {
    for (scan, u) in [
        (0x73, 0x87),
        (0x7d, 0x89),
        (0x71, 0x91),
        (0xf1, 0x91),
        (0x72, 0x90),
        (0xf2, 0x90),
    ] {
        assert_eq!(usage(0, scan, false), Some(u));
        assert_eq!(usage(0xe5, scan, false), Some(u));
        assert!(usage(0, scan, true).is_none());
        let mut p = Policy::default();
        p.arm(1);
        p.host_held(0, scan, false);
        assert!(!p.route(0, scan, false, true, true, false).swallow);
        assert!(!p.route(0, scan, false, false, true, false).swallow);
        assert!(p.route(0, scan, false, true, true, false).swallow);
        assert_eq!(p.disarm(), Some(Event::Release { epoch: 1 }));
        p.arm(2);
        assert!(p.route(0, scan, false, true, true, false).event.is_none());
        let old_up = p.route(0xe5, scan, false, false, true, false);
        assert!(old_up.swallow);
        assert!(old_up.event.is_none());
        assert_eq!(
            p.route(0, scan, false, true, true, false).event,
            Some(Event::Key {
                epoch: 2,
                usage: u,
                down: true
            })
        );
    }
    // v1's function-key VK precedence remains, even when a scan is ambiguous.
    assert_eq!(usage(0x87, 0x76, false), Some(0x73));
}
