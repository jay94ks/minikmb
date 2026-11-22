use eframe::egui;
use minikmb_native::keyboard_layout::Key;
use std::collections::BTreeSet;

fn combination(usage: u16, keys: &[Key], modifiers: &BTreeSet<u16>) -> Option<(String, Vec<u16>)> {
    let key = keys.iter().find(|k| k.usage == usage)?;
    let mut labels = Vec::new();
    let mut usages = Vec::new();
    for (label, modifier) in [("Ctrl", 224), ("Alt", 226), ("Shift", 225), ("Win", 227)] {
        if modifiers.contains(&modifier) {
            labels.push(label);
            usages.push(modifier);
        }
    }
    labels.push(&key.label);
    usages.push(usage);
    Some((labels.join(" + "), usages))
}

pub(super) fn custom(
    ui: &mut egui::Ui,
    selected: &mut u16,
    keys: &[Key],
    modifiers: &BTreeSet<u16>,
) -> Option<(String, Vec<u16>)> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        ui.label("사용자 지정 키");
        egui::ComboBox::from_id_salt("virtual-custom-key")
            .selected_text(
                keys.iter()
                    .find(|k| k.usage == *selected)
                    .map_or("키 선택", |k| k.label.as_str()),
            )
            .width(140.0)
            .height(220.0)
            .show_ui(ui, |ui| {
                for key in keys {
                    ui.selectable_value(selected, key.usage, &key.label);
                }
            });
        if ui
            .add_enabled(
                keys.iter().any(|k| k.usage == *selected),
                egui::Button::new("조합 전송…"),
            )
            .clicked()
        {
            chosen = combination(*selected, keys, modifiers);
        }
    });
    chosen
}

pub(super) fn row(ui: &mut egui::Ui, keys: &[Key]) -> egui::InnerResponse<Option<u16>> {
    let weight: f32 = keys.iter().map(|key| key.width).sum();
    let unit = ((ui.available_width() - (keys.len() - 1) as f32 * 4.0) / weight).max(22.0);
    let pixels = ui.ctx().pixels_per_point();
    let unit = (unit * pixels).floor() / pixels;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        // The main toolbar's 10pt padding would enlarge Pause/ScrLk beyond their
        // allocated key width in the minimum window. Keep every label and hitbox.
        ui.spacing_mut().button_padding.x = 3.0;
        let mut chosen = None;
        for key in keys {
            if ui
                .add_sized([unit * key.width, 28.0], egui::Button::new(&key.label))
                .clicked()
            {
                chosen = Some(key.usage);
            }
        }
        chosen
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_combinations_match_v1_for_every_key_and_modifier_mask() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/keyboard-chords-v1.json")).unwrap();
        let keys = minikmb_native::keyboard_layout::chord_keys();
        for option in fixture["options"].as_array().unwrap() {
            let usage = option["usage"].as_u64().unwrap() as u16;
            for vector in option["combinations"].as_array().unwrap() {
                let mask = vector["modifiers"].as_u64().unwrap();
                let modifiers = [224, 226, 225, 227]
                    .into_iter()
                    .enumerate()
                    .filter_map(|(bit, usage)| (mask & (1 << bit) != 0).then_some(usage))
                    .collect();
                let (label, result) = combination(usage, &keys, &modifiers).unwrap();
                assert_eq!(
                    serde_json::json!(result),
                    vector["usages"],
                    "{} mask={mask}",
                    option["label"]
                );
                assert!(!label.is_empty());
                for usage in result {
                    assert!(minikmb_native::input::Action::Key { down: true, usage }
                        .packet()
                        .is_ok());
                }
            }
        }
        assert!(combination(0xffff, &keys, &BTreeSet::new()).is_none());
    }
    #[test]
    fn every_virtual_key_row_fits_minimum_content_width_at_each_scale() {
        let ctx = egui::Context::default();
        minikmb_native::theme::apply(&ctx);
        let keys = minikmb_native::keyboard_layout::rows();
        for scale in [1.0, 1.5, 2.0] {
            ctx.set_pixels_per_point(scale);
            for width in [740.0, 1040.0] {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 560.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let right = ui.max_rect().right();
                            for keys in &keys {
                                let shown = row(ui, keys);
                                assert!(shown.inner.is_none());
                                assert!(
                                    shown.response.rect.right() <= right + 0.01,
                                    "row overflows at {scale} and {width}: {:?}",
                                    shown.response.rect
                                );
                            }
                        });
                    },
                );
            }
        }
    }
}
