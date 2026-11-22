//! Explicit no-device UI measurement. Output/profile directory must be new.
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct Session {
    pub directory: PathBuf,
    started: Instant,
}
impl Session {
    pub fn parse(args: &[String]) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        if !args.iter().any(|a| a == "--ui-benchmark") {
            return Ok(None);
        }
        if args.len() != 3 || args[1] != "--ui-benchmark" {
            return Err(
                "--ui-benchmark requires only a new output directory; probes cannot be combined"
                    .into(),
            );
        }
        let directory = PathBuf::from(&args[2]);
        if !directory.is_absolute() {
            return Err("benchmark output must be absolute".into());
        }
        // Exclusive mkdir: never reuse settings/keys, follow an existing leaf link,
        // overwrite existing results, or remove files supplied by a caller.
        std::fs::create_dir(&directory)?;
        Ok(Some(Self {
            directory,
            started: Instant::now(),
        }))
    }
    pub fn ready(&self, width: usize, height: usize, scale: f32) -> std::io::Result<()> {
        let data = serde_json::json!({"app":"native", "width_px":width,
            "height_px":height,"scale":scale,"content_dip":[1280,820],
            "devices":"disabled", "frame":"frame.png"});
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.directory.join("ready.json"))?;
        serde_json::to_writer(file, &data)?;
        Ok(())
    }
    pub fn should_close(&self) -> bool {
        self.started.elapsed() >= Duration::from_secs(30) || self.directory.join("stop").exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measurement_requires_exclusive_new_directory_and_rejects_probes() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("run");
        let args = vec![
            "app".into(),
            "--ui-benchmark".into(),
            path.to_string_lossy().into(),
        ];
        let session = Session::parse(&args).unwrap().unwrap();
        assert!(!session.should_close());
        session.ready(1600, 1025, 1.25).unwrap();
        assert!(session.ready(1600, 1025, 1.25).is_err());
        assert!(Session::parse(&args).is_err());
        let mut mixed = args.clone();
        mixed.push("--inspect".into());
        assert!(Session::parse(&mixed).is_err());
        std::fs::write(path.join("stop"), "").unwrap();
        assert!(session.should_close());
        assert!(Session::parse(&["app".into()]).unwrap().is_none());
        assert!(
            Session::parse(&["app".into(), "--ui-benchmark".into(), "relative".into()]).is_err()
        );
    }
}
