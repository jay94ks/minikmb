//! Versioned, licensed UI assets embedded in the executable. No runtime fetch.
use eframe::egui::{self, FontFamily, FontId};
pub const NOTO: &[u8] = include_bytes!("../assets/fonts/NotoSansKR-Regular.otf");
pub const LUCIDE: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");
pub const NOTO_NOTICE: &str = include_str!("../assets/fonts/Noto-NOTICE.txt");
pub const NOTO_LICENSE: &str = include_str!("../assets/fonts/Noto-OFL.txt");
pub const LUCIDE_LICENSE: &str = include_str!("../assets/fonts/Lucide-LICENSE.txt");
pub const MANIFEST: &str = include_str!("../assets/manifest.json");
const ICON_FAMILY: &str = "MiniKMB Lucide";

pub fn font_definitions() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Noto Sans KR".into(),
        egui::FontData::from_static(NOTO).into(),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Noto Sans KR".into());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push("Noto Sans KR".into());
    fonts.font_data.insert(
        ICON_FAMILY.into(),
        egui::FontData::from_static(LUCIDE).into(),
    );
    // Isolate private-use icon codepoints from user text and monospace output.
    fonts.families.insert(
        FontFamily::Name(ICON_FAMILY.into()),
        vec![ICON_FAMILY.into()],
    );
    fonts
}
#[derive(Clone, Copy, Debug)]
pub enum Icon {
    Pencil,
    Channels,
    Control,
    Keys,
    ApiDocs,
    McpGuide,
    Preferences,
    Diagnostics,
    Logs,
}
impl Icon {
    pub const ALL: [Self; 9] = [
        Self::Pencil,
        Self::Channels,
        Self::Control,
        Self::Keys,
        Self::ApiDocs,
        Self::McpGuide,
        Self::Preferences,
        Self::Diagnostics,
        Self::Logs,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Pencil => "pencil",
            Self::Channels => "panels-top-left",
            Self::Control => "monitor",
            Self::Keys => "key-round",
            Self::ApiDocs => "book-open",
            Self::McpGuide => "plug",
            Self::Preferences => "settings",
            Self::Diagnostics => "activity",
            Self::Logs => "logs",
        }
    }
    pub fn glyph(self) -> char {
        match self {
            Self::Pencil => '\u{e1f9}',
            Self::Channels => '\u{e12c}',
            Self::Control => '\u{e11d}',
            Self::Keys => '\u{e4a3}',
            Self::ApiDocs => '\u{e05f}',
            Self::McpGuide => '\u{e37f}',
            Self::Preferences => '\u{e154}',
            Self::Diagnostics => '\u{e038}',
            Self::Logs => '\u{e5f4}',
        }
    }
    pub fn font(size: f32) -> FontId {
        FontId::new(size, FontFamily::Name(ICON_FAMILY.into()))
    }
}
pub fn credits(ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("글꼴·아이콘 라이선스").show(ui, |ui| {
        ui.label("Noto Sans KR Regular 2.004 · SIL Open Font License 1.1");
        ui.hyperlink_to(
            "Noto 원본",
            "https://github.com/notofonts/noto-cjk/tree/Sans2.004",
        );
        ui.label(NOTO_NOTICE);
        ui.label("Lucide 0.577.0 · ISC / Feather 유래 부분 MIT");
        ui.hyperlink_to(
            "Lucide 원본",
            "https://github.com/lucide-icons/lucide/tree/0.577.0",
        );
        for (name, text) in [
            ("Noto 라이선스 전문", NOTO_LICENSE),
            ("Lucide 라이선스 전문", LUCIDE_LICENSE),
        ] {
            egui::CollapsingHeader::new(name).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(name)
                    .max_height(240.0)
                    .show(ui, |ui| {
                        ui.label(text);
                    });
            });
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn embedded_assets_and_licenses_match_the_pinned_distribution() {
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        for (name, font, license) in [
            ("noto", NOTO, NOTO_LICENSE),
            ("lucide", LUCIDE, LUCIDE_LICENSE),
        ] {
            let a = &manifest[name];
            assert_eq!(a["bytes"], font.len());
            assert_eq!(a["sha256"], format!("{:x}", Sha256::digest(font)));
            assert_eq!(
                a["license_sha256"],
                format!("{:x}", Sha256::digest(license.as_bytes()))
            );
        }
        for icon in Icon::ALL {
            assert_eq!(
                manifest["lucide"]["glyphs"][icon.name()],
                u32::from(icon.glyph())
            );
        }
        assert!(NOTO_NOTICE.contains("Adobe"));
        assert_eq!(
            manifest["noto"]["notice_sha256"],
            format!("{:x}", Sha256::digest(NOTO_NOTICE.as_bytes()))
        );
        assert!(NOTO_LICENSE.contains("SIL OPEN FONT LICENSE"));
        assert!(LUCIDE_LICENSE.contains("ISC License") && LUCIDE_LICENSE.contains("Feather"));
    }
    #[test]
    fn bundled_fonts_cover_hangul_and_icons_at_each_requested_dpi() {
        let ctx = egui::Context::default();
        ctx.set_fonts(font_definitions());
        for dpi in [1.0, 1.5, 2.0] {
            ctx.set_pixels_per_point(dpi);
            let _ = ctx.run(Default::default(), |ctx| {
                ctx.fonts_mut(|f| {
                    let body = FontId::proportional(14.0);
                    // Every modern precomposed Hangul syllable, not just labels.
                    for scalar in 0xac00..=0xd7a3 {
                        assert!(
                            f.has_glyph(&body, char::from_u32(scalar).unwrap()),
                            "Hangul {scalar:x}"
                        );
                    }
                    assert!(f.has_glyphs(&FontId::monospace(12.0), "채널 UID 0x0123456789ABCDEF"));
                    for icon in Icon::ALL {
                        assert!(f.has_glyph(&Icon::font(16.0), icon.glyph()), "{:?}", icon);
                        assert!(f.glyph_width(&Icon::font(16.0), icon.glyph()) > 0.0);
                    }
                });
                egui::CentralPanel::default().show(ctx, credits);
            });
        }
    }
}
