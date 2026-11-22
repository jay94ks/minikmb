//! Transient UI feedback; acknowledgement never requires a button.
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Timer {
    shown: String,
    since: Option<Instant>,
}
impl Timer {
    pub(super) fn reset(&mut self) {
        self.shown.clear();
        self.since = None;
    }
    pub(super) fn tick(&mut self, message: &mut String, now: Instant) {
        if message.is_empty() {
            self.reset();
        } else if *message != self.shown {
            self.shown.clone_from(message);
            self.since = Some(now);
        } else if self
            .since
            .is_some_and(|at| now.duration_since(at) >= Duration::from_secs(4))
        {
            message.clear();
            self.reset();
        }
    }
}

pub(super) fn show(ctx: &eframe::egui::Context, message: &str) {
    use eframe::egui::{self, Align2, RichText, Stroke};
    use minikmb_native::theme::{color, size};
    if message.is_empty() {
        return;
    }
    let width = (ctx.input(|i| i.viewport_rect().width()) - 48.0).clamp(120.0, 380.0);
    egui::Area::new(egui::Id::new("global-toast"))
        .order(egui::Order::Tooltip)
        .anchor(Align2::RIGHT_BOTTOM, [-16.0, -16.0])
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(color::RAISED)
                .stroke(Stroke::new(1.0, color::BORDER))
                .corner_radius(size::RADIUS)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_max_width(width);
                    ui.add(egui::Label::new(RichText::new(message).color(color::TEXT)).wrap());
                });
        });
    // Keep expiry reliable while idle, without continuous rendering.
    ctx.request_repaint_after(Duration::from_millis(100));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notice_expires_without_navigation_or_confirmation_and_new_error_gets_its_own_time() {
        let now = Instant::now();
        let mut timer = Timer::default();
        let mut message = "release unconfirmed".to_owned();
        timer.tick(&mut message, now);
        timer.tick(&mut message, now + Duration::from_secs(3));
        assert!(!message.is_empty());
        timer.tick(&mut message, now + Duration::from_secs(4));
        assert!(message.is_empty());
        message = "another error".into();
        timer.tick(&mut message, now + Duration::from_secs(5));
        timer.tick(&mut message, now + Duration::from_secs(8));
        assert!(!message.is_empty());
        timer.tick(&mut message, now + Duration::from_secs(9));
        assert!(message.is_empty());
    }
}
