//! Card order is a local preference; dragging never selects or controls a device.
use eframe::egui::{self, CursorIcon, Sense, Stroke};
use minikmb_native::{model::Uid, theme::color};

struct Dragged(Uid);

pub(super) fn handle(ui: &mut egui::Ui, uid: &Uid) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(20.0, 28.0), Sense::drag());
    for x in [-3.0, 3.0] {
        for y in [-5.0, 0.0, 5.0] {
            ui.painter()
                .circle_filled(rect.center() + egui::vec2(x, y), 1.5, color::MUTED);
        }
    }
    response.dnd_set_drag_payload(Dragged(uid.clone()));
    response
        .on_hover_cursor(CursorIcon::Grab)
        .on_hover_text("드래그하여 순서 바꾸기");
}

pub(super) fn target(ui: &egui::Ui, rect: egui::Rect, uid: &Uid) -> Option<(Uid, Uid, bool)> {
    let dragged = egui::DragAndDrop::payload::<Dragged>(ui.ctx())?;
    if &dragged.0 == uid {
        ui.painter().rect_stroke(
            rect,
            4.0,
            Stroke::new(1.5, color::ACCENT),
            egui::StrokeKind::Inside,
        );
        return None;
    }
    if !ui.rect_contains_pointer(rect.expand2(egui::vec2(0.0, 4.0))) {
        return None;
    }
    let pointer = ui.input(|i| i.pointer.interact_pos())?;
    let after = pointer.y >= rect.center().y;
    let y = if after {
        rect.bottom() + 4.0
    } else {
        rect.top() - 4.0
    };
    ui.painter()
        .hline(rect.x_range(), y, Stroke::new(2.0, color::ACCENT));
    if ui.input(|i| i.pointer.any_released()) {
        egui::DragAndDrop::take_payload::<Dragged>(ui.ctx())
            .map(|source| (source.0.clone(), uid.clone(), after))
    } else {
        None
    }
}

pub(super) fn edge_scroll(ui: &egui::Ui) {
    if !egui::DragAndDrop::has_payload_of_type::<Dragged>(ui.ctx()) {
        return;
    }
    let Some(p) = ui.input(|i| i.pointer.interact_pos()) else {
        return;
    };
    let clip = ui.clip_rect();
    if !clip.contains(p) {
        return;
    }
    let delta = if p.y < clip.top() + 24.0 {
        10.0
    } else if p.y > clip.bottom() - 24.0 {
        -10.0
    } else {
        0.0
    };
    if delta != 0.0 {
        ui.scroll_with_delta(egui::vec2(0.0, delta));
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dragging_handle_reorders_but_button_click_does_not_start_drag() {
        let ctx = egui::Context::default();
        let ids = [
            Uid::from_bytes(&[1; 12]).unwrap(),
            Uid::from_bytes(&[2; 12]).unwrap(),
        ];
        let mut handles = Vec::new();
        let mut cards = Vec::new();
        let mut buttons = Vec::new();
        let mut dropped = None;
        let mut clicked = 0;
        let mut pass = |events: Vec<egui::Event>| {
            handles.clear();
            cards.clear();
            buttons.clear();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        for uid in &ids {
                            let card = ui
                                .push_id(uid.raw(), |ui| {
                                    egui::Frame::group(ui.style()).show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            let before = ui.next_widget_position();
                                            handle(ui, uid);
                                            handles.push(egui::Rect::from_min_size(
                                                before,
                                                egui::vec2(20.0, 28.0),
                                            ));
                                            let button = ui.button("control");
                                            buttons.push(button.rect);
                                            if button.clicked() {
                                                clicked += 1;
                                            }
                                        });
                                        ui.label("channel UID");
                                    })
                                })
                                .inner;
                            cards.push(card.response.rect);
                            if let Some(request) = target(ui, card.response.rect, uid) {
                                dropped = Some(request);
                            }
                            ui.add_space(8.0);
                        }
                    });
                },
            );
            (
                handles.clone(),
                cards.clone(),
                buttons.clone(),
                dropped.clone(),
                clicked,
            )
        };
        let (_, _, buttons, _, _) = pass(vec![]);
        let p = buttons[0].center();
        pass(vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        let (h, c, _, drop, count) = pass(vec![egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert_eq!(count, 1);
        assert!(drop.is_none());
        let source = h[0].center();
        let dest = egui::pos2(c[1].center().x, c[1].bottom() - 3.0);
        pass(vec![
            egui::Event::PointerMoved(source),
            egui::Event::PointerButton {
                pos: source,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        pass(vec![egui::Event::PointerMoved(dest)]);
        let (_, _, _, drop, count) = pass(vec![egui::Event::PointerButton {
            pos: dest,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert_eq!(count, 1);
        assert_eq!(drop, Some((ids[0].clone(), ids[1].clone(), true)));
    }
}
