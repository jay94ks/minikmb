use super::*;
use crate::{
    input::Owner,
    keyboard_capture::{Event as Routed, Policy},
};
use tokio_util::sync::CancellationToken;
fn gate(epoch: u64) -> Gate {
    Gate {
        epoch,
        binding: crate::active::Binding {
            uid: crate::model::Uid::from_bytes(&[1; 12]).unwrap(),
            generation: 1,
        },
        owner: Owner {
            pin: Default::default(),
            id: format!("window-{epoch}"),
            cancelled: CancellationToken::new(),
            command_guard: None,
        },
    }
}
fn k(vk: u32, scan: u32, extended: bool, down: bool) -> Key {
    Key {
        vk,
        scan,
        extended,
        down,
        modifiers: 0,
    }
}
fn routed(p: &mut Policy, key: Key) -> Option<(u16, bool)> {
    match ordinary_route(p, key, true).event {
        Some(Routed::Key { usage, down, .. }) => Some((usage, down)),
        _ => None,
    }
}
#[test]
fn ordinary_physical_keys_preserve_lock_keypad_and_left_right_identity() {
    let mut p = Policy::default();
    p.arm(1);
    for (vk, scan, ex, usage) in [
        (0x14, 0x3a, false, 0x39),
        (0x90, 0x45, false, 0x53),
        (0x31, 0x02, false, 0x1e),
        (0x61, 0x4f, false, 0x59),
        (0x23, 0x4f, false, 0x59),
        (0x23, 0x4f, true, 0x4d),
        (0x0d, 0x1c, false, 0x28),
        (0x0d, 0x1c, true, 0x58),
        (0xa0, 0x2a, false, 0xe1),
        (0xa1, 0x36, false, 0xe5),
        (0xa2, 0x1d, false, 0xe0),
        (0xa3, 0x1d, true, 0xe4),
        (0xa4, 0x38, false, 0xe2),
        (0xa5, 0x38, true, 0xe6),
        (0x7b, 0x58, false, 0x45),
        (0x7c, 0x64, false, 0x68),
    ] {
        assert_eq!(routed(&mut p, k(vk, scan, ex, true)), Some((usage, true)));
        assert_eq!(routed(&mut p, k(vk, scan, ex, true)), None, "repeat");
        assert_eq!(
            routed(&mut p, k(0xe5, scan, ex, false)),
            Some((usage, false)),
            "IME changed VK"
        );
    }
}
#[test]
fn all_v1_dom_keys_preserve_the_special_key_opt_in_boundary() {
    for key in crate::keyboard_capture::parity_tests::keys() {
        let mut p = Policy::default();
        p.arm(14);
        let down = ordinary_route(&mut p, k(key.vk, key.scan, key.extended, true), true);
        if key.special {
            assert!(down.event.is_none(), "{}", key.code);
            assert_eq!(down.release, Some(Routed::Release { epoch: 14 }));
            assert!(!down.swallow); // Windows dispatch is always retained in OFF mode.
        } else {
            assert_eq!(
                down.event,
                Some(Routed::Key {
                    epoch: 14,
                    usage: key.usage,
                    down: true
                }),
                "{}",
                key.code
            );
            assert!(
                ordinary_route(&mut p, k(key.vk, key.scan, key.extended, true), true)
                    .event
                    .is_none()
            );
            assert_eq!(
                ordinary_route(&mut p, k(0xe5, key.scan, key.extended, false), true).event,
                Some(Routed::Key {
                    epoch: 14,
                    usage: key.usage,
                    down: false
                }),
                "{}",
                key.code
            );
        }
    }
}
#[test]
fn host_held_snapshot_does_not_invent_left_aliases_for_right_modifiers() {
    let mut state = [0; 256];
    state[0x11] = 128;
    state[0xa3] = 128;
    state[0x12] = 128;
    state[0xa5] = 128;
    let held = held_keys(&state);
    assert_eq!(held.len(), 2);
    assert!(held
        .iter()
        .all(|k| matches!(k.vk, 0xa3 | 0xa5) && k.extended));
    let mut p = Policy::default();
    p.arm(1);
    for k in held {
        p.host_held(k.vk, k.scan, k.extended);
    }
    assert_eq!(routed(&mut p, k(0xa3, 0x1d, true, false)), None);
    assert_eq!(routed(&mut p, k(0xa5, 0x38, true, false)), None);
    assert_eq!(
        routed(&mut p, k(0xa2, 0x1d, false, true)),
        Some((0xe0, true))
    );
    assert_eq!(
        routed(&mut p, k(0xa4, 0x38, false, true)),
        Some((0xe2, true))
    );
}

#[test]
fn rearming_replaces_stale_host_snapshot_and_preserves_suppressed_up_markers() {
    let mut p = Policy::default();
    p.arm(1);
    p.host_held(0xa3, 0x1d, true);
    assert!(routed(&mut p, k(0x41, 0x1e, false, true)).is_some());
    p.disarm();
    // Right Ctrl was released outside capture; new arm/snapshot clears it.
    p.arm(2);
    assert_eq!(
        routed(&mut p, k(0xa3, 0x1d, true, true)),
        Some((0xe4, true))
    );
    // A suppressed A UP is consumed by the old epoch, never delivered to epoch2.
    assert_eq!(routed(&mut p, k(0x41, 0x1e, false, false)), None);
    assert_eq!(routed(&mut p, k(0x41, 0x1e, false, true)), Some((4, true)));
}

#[test]
fn switching_from_special_to_ordinary_does_not_lose_next_press_after_swallowed_up() {
    let mut special = Policy::default();
    special.arm(1);
    special.route(0x41, 0x1e, false, true, true, false);
    special.disarm();
    let mut ordinary = Policy::default();
    ordinary.arm(2);
    ordinary.host_held(0x41, 0x1e, false);
    let release = special.route(0x41, 0x1e, false, false, false, false);
    assert!(release.swallow);
    assert!(release.event.is_none());
    let Some(Routed::HostUp { vk, scan, extended }) = release.host_up else {
        panic!("missing physical release");
    };
    ordinary.host_up(vk, scan, extended);
    assert_eq!(
        routed(&mut ordinary, k(0x41, 0x1e, false, true)),
        Some((4, true))
    );
    // A delayed host-release notification cannot erase a current target DOWN.
    ordinary.host_up(vk, scan, extended);
    assert_eq!(
        routed(&mut ordinary, k(0x41, 0x1e, false, false)),
        Some((4, false))
    );
}
#[test]
fn host_shortcuts_special_keys_and_focus_loss_release_the_original_epoch() {
    for mut key in [
        k(0x5b, 0x5b, true, true),
        k(0x15, 0x38, true, true),
        k(0x09, 0x0f, false, true),
        k(0x73, 0x3e, false, true),
        k(0x2e, 0x53, true, true),
    ] {
        if key.vk == 0x09 || key.vk == 0x73 {
            key.modifiers = 2;
        }
        if key.vk == 0x2e {
            key.modifiers = 3;
        }
        let mut p = Policy::default();
        p.arm(22);
        assert!(routed(&mut p, k(0x41, 0x1e, false, true)).is_some());
        let r = ordinary_route(&mut p, key, true);
        assert!(r.event.is_none());
        assert_eq!(r.release, Some(Routed::Release { epoch: 22 }));
        assert_eq!(
            ordinary_route(&mut p, k(0x41, 0x1e, false, false), false).event,
            None
        );
    }
}
#[test]
fn native_fifo_coalesces_only_adjacent_moves_and_keeps_key_button_order() {
    let input = WindowInput::default();
    input.publish(1, gate(1), 1.0);
    let mut s = input.0.lock();
    for e in [
        Event::Move { x: 1, y: 1 },
        Event::Move { x: 2, y: 2 },
        Event::Button {
            x: 2,
            y: 2,
            button: 1,
            down: true,
            held: vec![],
        },
        Event::Key(k(0x41, 0x1e, false, true)),
        Event::Move { x: 3, y: 3 },
        Event::Move { x: 4, y: 4 },
        Event::Key(k(0x41, 0x1e, false, false)),
        Event::Button {
            x: 4,
            y: 4,
            button: 1,
            down: false,
            held: vec![],
        },
    ] {
        WindowInput::push(&mut s, e);
    }
    drop(s);
    let messages = input.drain().unwrap();
    assert_eq!(messages.len(), 6);
    assert!(matches!(messages[0].event, Event::Move { x: 2, .. }));
    assert!(matches!(
        messages[1].event,
        Event::Button { down: true, .. }
    ));
    assert!(matches!(
        messages[2].event,
        Event::Key(Key { down: true, .. })
    ));
    assert!(matches!(messages[3].event, Event::Move { x: 4, .. }));
    assert!(matches!(
        messages[4].event,
        Event::Key(Key { down: false, .. })
    ));
    assert!(matches!(
        messages[5].event,
        Event::Button { down: false, .. }
    ));
    assert!(messages.iter().all(|m| m.gate.owner.id == "window-1"));
}
#[test]
fn overflow_cancels_original_source_and_replacement_cannot_replay_old_keys() {
    let input = WindowInput::default();
    let a = gate(1);
    input.publish(1, a.clone(), 1.0);
    {
        let mut s = input.0.lock();
        for _ in 0..513 {
            WindowInput::push(&mut s, Event::Key(k(0x41, 0x1e, false, true)));
        }
    }
    assert!(a.owner.cancelled.is_cancelled());
    assert_eq!(input.drain().err().unwrap().code, "INPUT_QUEUE_FULL");
    assert!(input.drain().unwrap().is_empty());
    input.publish(1, gate(2), 1.0);
    WindowInput::push(&mut input.0.lock(), Event::Key(k(0x41, 0x1e, false, true)));
    input.publish(1, gate(3), 1.0);
    assert!(input.drain().unwrap().is_empty());
}
#[test]
fn win32_fields_distinguish_extended_keys_repeat_and_double_click() {
    let k = key(WM_SYSKEYDOWN, 0x11, (0x1d << 16) | (1 << 24) | (1 << 30));
    assert!(k.down && k.extended);
    assert_eq!(k.scan, 0x1d);
    assert_eq!(
        crate::keyboard_capture::usage(k.vk, k.scan, k.extended),
        Some(0xe4)
    );
    assert_eq!(button(WM_LBUTTONDBLCLK, 0), (1, true));
    assert_eq!(button(WM_XBUTTONDOWN, 2 << 16), (5, true));
    assert_eq!(button(WM_XBUTTONUP, 1 << 16), (4, false));
}
