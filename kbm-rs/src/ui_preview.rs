//! Controlled egui layout scale; never changes Windows DPI or opens devices.
#[derive(Clone, Copy)]
pub struct Layout {
    pub scale: f32,
    pub size: [f32; 2],
}
impl Layout {
    pub fn parse(args: &[String]) -> Result<Option<Self>, &'static str> {
        if !args
            .iter()
            .any(|s| s == "--preview-scale" || s == "--preview-size")
        {
            return Ok(None);
        }
        if [
            "--inspect",
            "--media-list",
            "--video-probe",
            "--video-ui-probe",
            "--ui-benchmark",
        ]
        .iter()
        .any(|flag| args.iter().any(|s| s == flag))
        {
            return Err("layout preview cannot be combined with hardware or performance probes");
        }
        let value = |flag: &str| -> Result<&str, &'static str> {
            let matches: Vec<_> = args
                .iter()
                .enumerate()
                .filter(|(_, s)| *s == flag)
                .collect();
            if matches.len() != 1 {
                return Err("each preview option must occur exactly once");
            }
            args.get(matches[0].0 + 1)
                .map(String::as_str)
                .ok_or("preview option value missing")
        };
        if !std::path::Path::new(value("--screenshot")?).is_absolute() {
            return Err("layout preview requires an absolute screenshot output");
        }
        let scale = match value("--preview-scale")? {
            "100" => 1.0,
            "150" => 1.5,
            "200" => 2.0,
            _ => return Err("preview scale must be 100, 150 or 200"),
        };
        let size = match value("--preview-size")? {
            "840x560" => [840.0, 560.0],
            "1280x820" => [1280.0, 820.0],
            _ => return Err("preview size must be 840x560 or 1280x820 logical points"),
        };
        Ok(Some(Self { scale, size }))
    }
    pub fn begin(&self, ctx: &eframe::egui::Context) {
        ctx.set_zoom_factor(self.scale / ctx.native_pixels_per_point().unwrap_or(1.0));
    }
    pub fn resize(&self, ctx: &eframe::egui::Context) {
        use eframe::egui::{vec2, ViewportCommand};
        // Use a later frame, after egui activates its pending zoom factor.
        ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(vec2(840.0, 560.0)));
        ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.size.into()));
    }
    pub fn matches(&self, ctx: &eframe::egui::Context) -> bool {
        let actual = ctx.input(|i| i.viewport_rect().size());
        (ctx.pixels_per_point() - self.scale).abs() < 0.001
            && (actual.x - self.size[0]).abs() < 1.0
            && (actual.y - self.size[1]).abs() < 1.0
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_requires_bounded_geometry_explicit_output_and_no_hardware_probe() {
        let output = tempfile::tempdir().unwrap().path().join("preview.png");
        let args = vec![
            "app".into(),
            "--screenshot".into(),
            output.to_string_lossy().into(),
            "--preview-scale".into(),
            "150".into(),
            "--preview-size".into(),
            "840x560".into(),
        ];
        let layout = Layout::parse(&args).unwrap().unwrap();
        assert_eq!(layout.scale, 1.5);
        assert_eq!(layout.size, [840.0, 560.0]);
        for extra in [
            "--inspect",
            "--media-list",
            "--video-probe",
            "--video-ui-probe",
            "--ui-benchmark",
            "--preview-scale",
        ] {
            let mut mixed = args.clone();
            mixed.push(extra.into());
            assert!(Layout::parse(&mixed).is_err());
        }
        for value in ["0", "nan", "infinity", "1000"] {
            let mut invalid = args.clone();
            invalid[4] = value.into();
            assert!(Layout::parse(&invalid).is_err());
        }
        let mut relative = args.clone();
        relative[2] = "relative.png".into();
        assert!(Layout::parse(&relative).is_err());
        assert!(Layout::parse(&["app".into()]).unwrap().is_none());
    }
}
