use super::*;
use minikmb_native::{
    input::Action,
    keyboard_capture::Event as KeyEvent,
    window_input::{ordinary_route, Event},
};

impl Desktop {
    fn normal_available(&self) -> bool {
        self.screenshot.is_none()
            && self.native_window.is_some()
            && self.rename_ui.is_none()
            && self.page == Page::Control
            && !self.manual_video
            && !self.terminal.open
            && !self.storage_ui.open
            && !self.network_ui.open
            && !self.wake_ui.open
            && self.pending_preset.is_none()
            && self.video_rect.is_some()
            && self.input_binding().is_some()
    }
    pub(super) fn publish_normal(&mut self, ctx: &egui::Context) {
        if !self.normal_available() || !ctx.input(|i| i.focused) {
            self.window_input.clear();
            return;
        }
        if let Some((owner, binding)) = self.source_owner() {
            self.window_input.publish(
                self.native_window.unwrap(),
                keyboard::Gate {
                    epoch: self.input_epoch,
                    owner,
                    binding,
                },
                ctx.pixels_per_point(),
            );
            // The GUI-thread WH_KEYBOARD hook owns special-mode consumption
            // before GetMessage returns. Never consume host-owned UPs later.
        }
    }
    fn over_video(&self, pos: egui::Pos2) -> bool {
        self.video_rect.is_some_and(|r| r.contains(pos))
            && !self.virtual_rect.is_some_and(|r| r.contains(pos))
    }
    /// Process native keys and pointers together, preserving message ordering even
    /// when egui omits lock/modifier/IME key events or coalesces pointer movement.
    pub(super) fn normal_events(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let resized = raw
            .screen_rect
            .is_some_and(|r| r != ctx.input(|i| i.viewport_rect()));
        let dpi_changed = raw
            .viewports
            .get(&egui::ViewportId::ROOT)
            .and_then(|v| v.native_pixels_per_point)
            .zip(ctx.native_pixels_per_point())
            .is_some_and(|(a, b)| a != b);
        if resized || dpi_changed {
            self.clear_input();
            return;
        }
        let events = match self.window_input.drain() {
            Ok(events) => events,
            Err(e) => {
                self.message = e.message;
                self.clear_input();
                return;
            }
        };
        let mut consumed_keyboard = self.video_focused;
        for message in events {
            let gate = message.gate;
            if self
                .input_owner
                .as_ref()
                .is_none_or(|o| o.id != gate.owner.id)
            {
                continue;
            }
            if !gate.valid(&self.app.active) {
                self.clear_input();
                continue;
            }
            let pos = |x: i32, y: i32| {
                egui::pos2(
                    x as f32 / message.pixels_per_point,
                    y as f32 / message.pixels_per_point,
                )
            };
            match message.event {
                Event::Blur => self.clear_input(),
                Event::Key(k) => {
                    if self.special_keys && self.video_focused {
                        self.sync_capture();
                        if k.vk == 0x2e && k.modifiers & 3 == 3 {
                            self.clear_input();
                            continue;
                        }
                        // Mapped keys have already been routed/consumed by the
                        // native GUI hook. Passed-through host keys must not be
                        // sent a second time from this later observer.
                        consumed_keyboard = true;
                        continue;
                    }
                    let route = ordinary_route(&mut self.normal_policy, k, self.video_focused);
                    if route.release.is_some() {
                        self.clear_input();
                    }
                    if let Some(KeyEvent::Key { usage, down, .. }) = route.event {
                        consumed_keyboard = true;
                        if let Err(e) =
                            self.input
                                .action(gate.owner, gate.binding, Action::Key { usage, down })
                        {
                            self.message = e.message;
                            self.clear_input();
                        }
                    }
                }
                Event::Move { x, y } => {
                    let p = pos(x, y);
                    self.pointer = Some(p);
                    if self.video_focused {
                        self.move_input(p, !self.buttons.is_empty());
                    }
                }
                Event::Button {
                    x,
                    y,
                    button,
                    down,
                    held,
                } => {
                    let p = pos(x, y);
                    self.pointer = Some(p);
                    if down {
                        if !self.over_video(p) {
                            self.clear_input();
                            continue;
                        }
                        if !self.video_focused {
                            self.video_focused = true;
                            self.normal_policy.arm(gate.epoch);
                            for k in held {
                                self.normal_policy.host_held(k.vk, k.scan, k.extended);
                            }
                        }
                        consumed_keyboard = true;
                        self.move_input(p, false);
                        if self.buttons.insert(button) {
                            self.send_input(Action::Button { down: true, button });
                        }
                    } else if self.buttons.remove(&button) {
                        self.move_input(p, true);
                        self.send_input(Action::Button {
                            down: false,
                            button,
                        });
                    }
                }
                Event::Wheel {
                    x,
                    y,
                    vertical,
                    horizontal,
                } => {
                    let p = pos(x, y);
                    self.pointer = Some(p);
                    if self.video_focused && self.over_video(p) {
                        self.normal_wheel[0] += i32::from(vertical);
                        self.normal_wheel[1] -= i32::from(horizontal);
                        let v = (self.normal_wheel[0] / 120).clamp(-127, 127) as i16;
                        let h = (self.normal_wheel[1] / 120).clamp(-127, 127) as i16;
                        self.normal_wheel[0] -= i32::from(v) * 120;
                        self.normal_wheel[1] -= i32::from(h) * 120;
                        if v != 0 || h != 0 {
                            self.send_input(Action::Wheel {
                                vertical: v,
                                horizontal: h,
                            });
                        }
                    }
                }
            }
        }
        // Let normal UI controls receive their native winit events. Do not route
        // translated egui key/pointer events to HID a second time.
        raw.events.retain(|e| match e {
            egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Paste(_)
            | egui::Event::Ime(_) => !consumed_keyboard,
            egui::Event::PointerButton { pos, .. } => !self.over_video(*pos),
            egui::Event::MouseWheel { .. } => {
                !(self.video_focused && self.pointer.is_some_and(|p| self.over_video(p)))
            }
            _ => true,
        });
    }
}
