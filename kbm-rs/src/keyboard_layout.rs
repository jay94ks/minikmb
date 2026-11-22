//! HID usages for the v1 virtual keyboard; modifiers are one-shot selections.
pub struct Key {
    pub label: String,
    pub usage: u16,
    pub width: f32,
}
fn key(label: &str, usage: u16, width: f32) -> Key {
    Key {
        label: label.into(),
        usage,
        width,
    }
}
fn letters(text: &str) -> Vec<Key> {
    text.chars()
        .map(|c| key(&c.to_string(), u16::from(c as u8 - b'A' + 4), 1.0))
        .collect()
}
pub fn rows() -> Vec<Vec<Key>> {
    let mut function = vec![key("Esc", 41, 1.0)];
    for n in 1..=12 {
        function.push(key(&format!("F{n}"), 57 + n, 1.0));
    }
    function.extend([
        key("PrtSc", 70, 1.0),
        key("ScrLk", 71, 1.0),
        key("Pause", 72, 1.0),
    ]);
    let mut number = vec![key("`", 53, 1.0)];
    for n in 1..=9 {
        number.push(key(&n.to_string(), 29 + n, 1.0));
    }
    number.extend([
        key("0", 39, 1.0),
        key("-", 45, 1.0),
        key("=", 46, 1.0),
        key("Backspace", 42, 2.0),
    ]);
    let mut top = vec![key("Tab", 43, 1.5)];
    top.extend(letters("QWERTYUIOP"));
    top.extend([key("[", 47, 1.0), key("]", 48, 1.0), key("\\", 49, 1.5)]);
    let mut middle = vec![key("Caps", 57, 1.8)];
    middle.extend(letters("ASDFGHJKL"));
    middle.extend([key(";", 51, 1.0), key("'", 52, 1.0), key("Enter", 40, 2.2)]);
    let mut bottom = letters("ZXCVBNM");
    bottom.extend([
        key(",", 54, 1.0),
        key(".", 55, 1.0),
        key("/", 56, 1.0),
        key("↑", 82, 1.0),
        key("Del", 76, 1.0),
        key("Ins", 73, 1.0),
    ]);
    let space = vec![
        key("한/영", 0x90, 1.0),
        key("한자", 0x91, 1.0),
        key("Space", 44, 5.0),
        key("Menu", 101, 1.0),
        key("Home", 74, 1.0),
        key("End", 77, 1.0),
        key("PgUp", 75, 1.0),
        key("PgDn", 78, 1.0),
        key("←", 80, 1.0),
        key("↓", 81, 1.0),
        key("→", 79, 1.0),
    ];
    vec![function, number, top, middle, bottom, space]
}
/// Every option offered by Electron v1's custom-chord selector, including F13-F24.
/// These are HID usages; the GUI never translates the labels into typed text.
pub fn chord_keys() -> Vec<Key> {
    let mut keys: Vec<_> = (b'A'..=b'Z')
        .map(|c| key(&(c as char).to_string(), 4 + u16::from(c - b'A'), 1.0))
        .collect();
    for n in 0..10 {
        keys.push(key(&n.to_string(), if n == 0 { 39 } else { 29 + n }, 1.0));
    }
    for n in 1..=24 {
        keys.push(key(
            &format!("F{n}"),
            if n <= 12 { 57 + n } else { 91 + n },
            1.0,
        ));
    }
    for (label, usage) in [
        ("Delete", 76),
        ("Insert", 73),
        ("Home", 74),
        ("End", 77),
        ("PageUp", 75),
        ("PageDown", 78),
        ("Enter", 40),
        ("Tab", 43),
        ("Escape", 41),
        ("Space", 44),
        ("Backspace", 42),
        ("PrintScreen", 70),
        ("Pause", 72),
        ("ContextMenu", 101),
        ("ArrowUp", 82),
        ("ArrowDown", 81),
        ("ArrowLeft", 80),
        ("ArrowRight", 79),
        ("Backquote", 53),
        ("Minus", 45),
        ("Equal", 46),
        ("BracketLeft", 47),
        ("BracketRight", 48),
        ("Backslash", 49),
        ("Semicolon", 51),
        ("Quote", 52),
        ("Comma", 54),
        ("Period", 55),
        ("Slash", 56),
        ("CapsLock", 57),
        ("ScrollLock", 71),
        ("NumLock", 83),
        ("Lang1", 0x90),
        ("Lang2", 0x91),
    ] {
        keys.push(key(label, usage, 1.0));
    }
    keys
}

pub const PRESETS: &[(&str, &[u16])] = &[
    ("Ctrl + Alt + Delete", &[224, 226, 76]),
    ("Windows 키", &[227]),
    ("Win + L", &[227, 15]),
    ("Win + D", &[227, 7]),
    ("Win + E", &[227, 8]),
    ("Win + R", &[227, 21]),
    ("Win + Tab", &[227, 43]),
    ("Alt + Tab", &[226, 43]),
    ("Alt + F4", &[226, 61]),
    ("Ctrl + Shift + Esc", &[224, 225, 41]),
];
#[cfg(test)]
mod tests {
    #[test]
    fn custom_selector_preserves_every_v1_choice_including_extended_function_keys() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../tests/keyboard-chords-v1.json")).unwrap();
        let choices = super::chord_keys();
        let expected = fixture["options"].as_array().unwrap();
        assert_eq!(choices.len(), expected.len());
        for (key, expected) in choices.iter().zip(expected) {
            assert_eq!(key.label, expected["label"].as_str().unwrap());
            assert_eq!(u64::from(key.usage), expected["usage"].as_u64().unwrap());
        }
        assert_eq!(
            choices
                .iter()
                .filter(|k| (104..=115).contains(&k.usage))
                .count(),
            12
        );
    }
    #[test]
    fn rows_preserve_korean_and_navigation_keys_without_a_vendor_fn() {
        let rows = super::rows();
        assert_eq!(rows.len(), 6);
        let keys: Vec<_> = rows.iter().flatten().map(|k| k.usage).collect();
        assert!(keys.contains(&0x90) && keys.contains(&0x91) && keys.contains(&101));
        assert!(rows.iter().flatten().all(|k| k.label != "Fn"));
    }
}
