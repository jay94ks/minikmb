//! MiniKMB visual language. Values are inspired by reference screenshots, not
//! copied product chrome. Keep reusable widgets and semantic tokens together.
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, TextStyle};

pub mod color {
    use super::Color32;
    pub const WORKSPACE: Color32 = Color32::from_rgb(46, 50, 54);
    pub const NAVIGATION: Color32 = Color32::from_rgb(39, 43, 47);
    pub const TOOLBAR: Color32 = Color32::from_rgb(57, 63, 68);
    pub const SURFACE: Color32 = Color32::from_rgb(60, 66, 71);
    pub const RAISED: Color32 = Color32::from_rgb(68, 75, 81);
    pub const INPUT: Color32 = Color32::from_rgb(43, 48, 52);
    pub const VIDEO: Color32 = Color32::from_rgb(18, 21, 24);
    pub const BORDER: Color32 = Color32::from_rgb(83, 92, 100);
    pub const TEXT: Color32 = Color32::from_rgb(239, 242, 245);
    pub const MUTED: Color32 = Color32::from_rgb(183, 193, 202);
    pub const ACCENT: Color32 = Color32::from_rgb(167, 210, 249);
    pub const SELECTED: Color32 = Color32::from_rgb(31, 79, 116);
    pub const SELECTED_ROW: Color32 = Color32::from_rgb(79, 88, 98);
    pub const FOCUS: Color32 = Color32::from_rgb(147, 201, 250);
    pub const SUCCESS: Color32 = Color32::from_rgb(143, 220, 179);
    pub const WARNING: Color32 = Color32::from_rgb(244, 202, 139);
    pub const ERROR: Color32 = Color32::from_rgb(255, 168, 164);
}
pub mod size {
    pub const UNIT: f32 = 4.0;
    pub const GAP: f32 = 8.0;
    pub const PANEL_PADDING: i8 = 16;
    pub const HEADER: f32 = 48.0;
    pub const SIDEBAR: f32 = 196.0;
    pub const CONTROL: f32 = 30.0;
    pub const ROW: f32 = 32.0;
    pub const RADIUS: u8 = 4;
}
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut s = egui::Style {
        text_styles: [
            (
                TextStyle::Heading,
                FontId::new(20.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
        ]
        .into(),
        ..Default::default()
    };
    s.spacing.item_spacing = egui::vec2(size::GAP, size::GAP);
    s.spacing.button_padding = egui::vec2(10.0, 5.0);
    s.spacing.interact_size = egui::vec2(size::CONTROL, size::CONTROL);
    s.spacing.indent = 16.0;
    s.spacing.window_margin = egui::Margin::same(size::PANEL_PADDING);
    s.spacing.menu_margin = egui::Margin::same(6);
    // Keep long lists discoverable without overlaying their last column.
    s.spacing.scroll = egui::style::ScrollStyle {
        bar_width: 8.0,
        handle_min_length: 24.0,
        foreground_color: true,
        ..egui::style::ScrollStyle::solid()
    };
    let v = &mut s.visuals;
    *v = egui::Visuals::dark();
    v.override_text_color = Some(color::TEXT);
    v.panel_fill = color::NAVIGATION;
    v.window_fill = color::SURFACE;
    v.window_stroke = Stroke::new(1.0, color::BORDER);
    v.window_corner_radius = egui::CornerRadius::same(6);
    v.menu_corner_radius = egui::CornerRadius::same(size::RADIUS);
    v.faint_bg_color = color::SURFACE;
    v.extreme_bg_color = color::INPUT;
    v.text_edit_bg_color = Some(color::INPUT);
    v.code_bg_color = color::INPUT;
    v.hyperlink_color = color::ACCENT;
    v.selection.bg_fill = color::SELECTED;
    v.selection.stroke = Stroke::new(1.0, color::TEXT);
    v.warn_fg_color = color::WARNING;
    v.error_fg_color = color::ERROR;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = egui::CornerRadius::same(size::RADIUS);
        w.bg_fill = color::SURFACE;
        w.weak_bg_fill = color::SURFACE;
        w.bg_stroke = Stroke::new(1.0, color::BORDER);
        w.fg_stroke = Stroke::new(1.0, color::TEXT);
        w.expansion = 0.0;
    }
    v.widgets.hovered.bg_fill = color::RAISED;
    v.widgets.hovered.weak_bg_fill = color::RAISED;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, color::FOCUS);
    v.widgets.active.bg_fill = color::SELECTED;
    v.widgets.active.weak_bg_fill = color::SELECTED;
    v.widgets.active.bg_stroke = Stroke::new(1.5, color::FOCUS);
    v.widgets.open.bg_fill = color::RAISED;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, color::BORDER);
    ctx.set_style(s);
    install_font(ctx);
}
fn install_font(ctx: &egui::Context) {
    ctx.set_fonts(crate::assets::font_definitions());
}
pub fn panel() -> egui::Frame {
    egui::Frame::new()
        .fill(color::SURFACE)
        .stroke(Stroke::new(1.0, color::BORDER))
        .corner_radius(size::RADIUS)
        .inner_margin(size::PANEL_PADDING)
}
pub fn heading(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.heading(RichText::new(title).color(color::ACCENT));
    if !description.is_empty() {
        ui.label(RichText::new(description).small().color(color::MUTED));
    }
    ui.add_space(size::GAP);
}
pub fn group_label(ui: &mut egui::Ui, title: &str) {
    ui.add_space(size::GAP);
    ui.label(RichText::new(title).small().color(color::MUTED));
}
/// Keep a short value and its unit together when a toolbar wraps to another row.
pub fn inline_metric(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.add(
        egui::Label::new(RichText::new(text.into()).small().color(color::MUTED))
            .wrap_mode(egui::TextWrapMode::Extend),
    )
}
pub fn toolbar_bottom(ctx: &egui::Context, bottom: f32) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("minikmb-toolbar-bottom"), bottom));
}
pub fn dialog_bounds(ctx: &egui::Context) -> egui::Rect {
    let mut bounds = ctx.content_rect().shrink(size::GAP);
    let toolbar = ctx.data(|data| data.get_temp::<f32>(egui::Id::new("minikmb-toolbar-bottom")));
    if let Some(bottom) = toolbar {
        bounds.min.y = bounds.min.y.max(bottom + size::GAP);
    }
    bounds
}
/// Keep title/close/resize controls inside the client area and the toolbar visible.
pub fn tool_window<'open>(
    ctx: &egui::Context,
    title: impl Into<egui::WidgetText>,
    id: &str,
    preferred: [f32; 2],
) -> egui::Window<'open> {
    let bounds = dialog_bounds(ctx);
    let style = ctx.style();
    let margins = style.spacing.window_margin.sum();
    // egui adds the window margin both around content and inside its title bar.
    let stroke = style.visuals.window_stroke.width * 2.0;
    let title_height = style
        .spacing
        .interact_size
        .y
        .max(style.text_styles[&TextStyle::Heading].size);
    let maximum = egui::vec2(
        (bounds.width() - margins.x - stroke - 4.0).max(160.0),
        (bounds.height() - margins.y * 2.0 - title_height - stroke - 4.0).max(100.0),
    );
    egui::Window::new(title)
        .id(egui::Id::new(id))
        .default_size(egui::vec2(preferred[0], preferred[1]).min(maximum))
        .max_size(maximum)
        .constrain_to(bounds)
        .scroll([true, true])
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn toolbar_metric_wraps_as_one_value_without_leaving_its_unit_behind() {
        let ctx = egui::Context::default();
        apply(&ctx);
        for scale in [1.0, 1.5, 2.0] {
            ctx.set_pixels_per_point(scale);
            let mut rectangles = None;
            for _ in 0..2 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(320.0, 120.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let right = ui.max_rect().right();
                            ui.horizontal_wrapped(|ui| {
                                let prefix = ui
                                    .allocate_exact_size(
                                        egui::vec2(240.0, 24.0),
                                        egui::Sense::hover(),
                                    )
                                    .0;
                                let value = inline_metric(ui, "1920×1080 · 30.00Hz").rect;
                                rectangles = Some((prefix, value, right));
                            });
                        });
                    },
                );
            }
            let (prefix, value, right) = rectangles.unwrap();
            assert!(
                value.top() >= prefix.bottom(),
                "metric must move to a new toolbar row: {value:?}"
            );
            assert!(
                value.height() <= prefix.height(),
                "metric must remain one line: {value:?}"
            );
            assert!(
                value.right() <= right + 0.01,
                "metric cannot overflow the toolbar"
            );
        }
    }
    #[test]
    fn tools_stay_below_toolbar_and_inside_a_resized_viewport_with_large_content() {
        let ctx = egui::Context::default();
        apply(&ctx);
        for scale in [1.0, 1.5, 2.0] {
            ctx.set_pixels_per_point(scale);
            // Reuse egui window memory while shrinking the available client area.
            for size in [egui::vec2(1280.0, 820.0), egui::vec2(840.0, 560.0)] {
                let mut result = None;
                for _ in 0..4 {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            ..Default::default()
                        },
                        |ctx| {
                            let top = egui::TopBottomPanel::top("test-tools-header")
                                .min_height(80.0)
                                .show(ctx, |ui| {
                                    ui.label("채널 · 고정 상단바");
                                });
                            toolbar_bottom(ctx, top.response.rect.bottom());
                            let bounds = dialog_bounds(ctx);
                            let mut open = true;
                            let shown = tool_window(
                                ctx,
                                "긴 내용의 도구 창",
                                "test-tool-window",
                                [960.0, 570.0],
                            )
                            .open(&mut open)
                            .show(ctx, |ui| {
                                ui.add(
                                    egui::Label::new("W".repeat(200))
                                        .wrap_mode(egui::TextWrapMode::Extend),
                                );
                                for row in 0..100 {
                                    ui.label(format!("항목 {row}"));
                                }
                            })
                            .unwrap();
                            result = Some((bounds, shown.response.rect));
                        },
                    );
                }
                let (bounds, window) = result.unwrap();
                assert!(
                    bounds.expand(1.0).contains_rect(window),
                    "bounds {bounds:?}, tool {window:?}"
                );
            }
        }
    }
}
pub fn navigation(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    let button = egui::Button::new(RichText::new(label).color(color::TEXT))
        .fill(if selected {
            color::SELECTED
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::NONE)
        .corner_radius(2)
        .min_size(egui::vec2(ui.available_width(), size::ROW));
    ui.add(button)
}
pub fn navigation_icon(
    ui: &mut egui::Ui,
    selected: bool,
    icon: crate::assets::Icon,
    label: &str,
) -> egui::Response {
    let mut text = egui::text::LayoutJob::default();
    text.append(
        &icon.glyph().to_string(),
        0.0,
        egui::TextFormat {
            font_id: crate::assets::Icon::font(16.0),
            color: color::TEXT,
            ..Default::default()
        },
    );
    text.append(
        label,
        8.0,
        egui::TextFormat {
            font_id: FontId::proportional(14.0),
            color: color::TEXT,
            ..Default::default()
        },
    );
    let response = ui.add(
        egui::Button::new(text)
            .fill(if selected {
                color::SELECTED
            } else {
                Color32::TRANSPARENT
            })
            .stroke(Stroke::NONE)
            .corner_radius(2)
            .min_size(egui::vec2(ui.available_width(), size::ROW)),
    );
    // Screen readers receive the human label rather than a private-use glyph.
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    response
}
/// A local selection is quieter than the application's primary navigation.
pub fn list_row(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    let response = ui.add(
        egui::Button::new(RichText::new(label).color(color::TEXT))
            .fill(if selected {
                color::SELECTED_ROW
            } else {
                Color32::TRANSPARENT
            })
            .stroke(Stroke::NONE)
            .corner_radius(2)
            .min_size(egui::vec2(ui.available_width(), size::ROW)),
    );
    if selected {
        let r = response.rect;
        ui.painter().line_segment(
            [r.left_top(), r.left_bottom()],
            Stroke::new(2.0, color::ACCENT),
        );
    }
    response
}
pub fn tab(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    let response = ui.add(
        egui::Button::new(RichText::new(label).color(if selected {
            color::ACCENT
        } else {
            color::TEXT
        }))
        .frame(false)
        .min_size(egui::vec2(64.0, size::ROW)),
    );
    if selected {
        let r = response.rect;
        ui.painter().line_segment(
            [r.left_bottom(), r.right_bottom()],
            Stroke::new(2.0, color::ACCENT),
        );
    }
    response
}
#[derive(Clone, Copy)]
pub enum Tone {
    Neutral,
    Success,
    Warning,
    Error,
}
pub fn status(ui: &mut egui::Ui, tone: Tone, label: &str) {
    let color = match tone {
        Tone::Neutral => color::MUTED,
        Tone::Success => color::SUCCESS,
        Tone::Warning => color::WARNING,
        Tone::Error => color::ERROR,
    };
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(r.center(), 3.0, color);
        ui.label(RichText::new(label).small().color(color));
    });
}
pub fn code(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(color::INPUT)
        .stroke(Stroke::new(1.0, color::BORDER))
        .corner_radius(size::RADIUS)
        .inner_margin(12)
        .show(ui, |ui| {
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(text).monospace().color(color::TEXT))
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            });
        });
}
pub fn empty(ui: &mut egui::Ui, title: &str, description: &str) {
    panel().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.add_space(size::GAP);
        ui.strong(title);
        ui.label(RichText::new(description).color(color::MUTED));
        ui.add_space(size::GAP);
    });
}
/// QA-only component sheet, never claims device state or performs I/O.
pub fn preview(ui: &mut egui::Ui) {
    heading(
        ui,
        "MiniKMB 공통 UI",
        "회색 작업면 · 명확한 계층 · 절제된 파란 강조 · 조밀한 조작부",
    );
    ui.columns(2, |cols| {
        panel().show(&mut cols[0], |ui| {
            heading(ui, "버튼과 입력", "기본 조작 높이 30pt / 간격 8pt");
            ui.horizontal(|ui| {
                ui.button("목록 갱신").on_hover_text("기본 버튼");
                ui.add(egui::Button::new("연결").fill(color::SELECTED));
                ui.add_enabled(false, egui::Button::new("비활성"));
            });
            let mut value = String::new();
            ui.add(egui::TextEdit::singleline(&mut value).hint_text("이름 또는 UID 검색"));
            let mut checked = true;
            ui.checkbox(&mut checked, "특수 키 전송");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                tab(ui, true, "개요");
                tab(ui, false, "설정");
                tab(ui, false, "기록");
            });
            ui.separator();
            group_label(ui, "상태는 색상과 텍스트를 함께 사용");
            status(ui, Tone::Success, "연결됨");
            status(ui, Tone::Neutral, "연결 대기");
            status(ui, Tone::Warning, "확인이 필요합니다");
            status(ui, Tone::Error, "연결에 실패했습니다");
        });
        panel().show(&mut cols[1], |ui| {
            heading(
                ui,
                "목록과 선택",
                "전체 행 선택 / 얇은 구분선 / 작은 모서리",
            );
            navigation(ui, false, "채널 목록");
            navigation(ui, true, "선택한 채널");
            navigation(ui, false, "서버 · API 키");
            group_label(ui, "패널 내부 목록 선택");
            list_row(ui, false, "채널 상태 조회");
            list_row(ui, true, "현재 장면 캡처");
            ui.add_space(4.0);
            group_label(ui, "본문 · 보조 정보 · 식별자");
            ui.label("타깃 PC를 제어할 채널을 선택하세요.");
            ui.label(
                RichText::new("보조 설명은 한 단계 낮은 대비로 표시합니다.").color(color::MUTED),
            );
            ui.monospace("0x000102030405060708090A0B");
            ui.add_space(12.0);
            code(ui, "GET /api/v1/active\nAuthorization: Bearer <API_KEY>");
        });
    });
    ui.add_space(16.0);
    empty(
        ui,
        "영상이 열려 있지 않습니다.",
        "상단의 영상 열기를 눌러 현재 채널의 영상을 확인하세요.",
    );
    ui.add_space(16.0);
    ui.horizontal_wrapped(|ui| {
        for (name, c) in [
            ("작업면", color::WORKSPACE),
            ("패널", color::SURFACE),
            ("입력", color::INPUT),
            ("선택", color::SELECTED),
            ("목록 선택", color::SELECTED_ROW),
            ("강조", color::ACCENT),
        ] {
            let (r, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 3, c);
            ui.label(name);
            ui.add_space(12.0);
        }
    });
}
