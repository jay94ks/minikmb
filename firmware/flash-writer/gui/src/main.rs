#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;

use anyhow::{Context, Result};
use frwr_ui::{backend, model::*, protocol};
use std::path::PathBuf;

fn argument(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|s| s == flag).and_then(|i| args.get(i+1)).cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|s| s == "--help") {
        println!("frwr-ui [--demo]\nRead-only CLI: --probe | --dump | --read START LENGTH\n  [--save FILE] --port COM27 --serial UID [--chip MODEL] [--data-dir DIR] [--hz 1000000]\nReads go to memory; --save explicitly exports the captured bytes.\nEmbedded serprog SPI engine. Writing/erasing and optional backup are configured in the GUI.");
        return Ok(());
    }
    let smoke = argument(&args, "--smoke-ui").map(PathBuf::from);
    if let Some(path) = &smoke {
        anyhow::ensure!(!path.exists(), "Use a new screenshot path");
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
    }
    let demo = args.iter().any(|s| s == "--demo") || smoke.is_some();
    if args.iter().any(|s| ["--probe", "--dump", "--read"].contains(&s.as_str())) {
        let device = if demo { Device { port: "DEMO".into(), serial: "SIMULATED".into() } }
            else {
                let port = argument(&args, "--port").context("--port is required")?;
                let serial = argument(&args, "--serial").context("--serial is required")?;
                let device = Device { port, serial }; protocol::identity(&device)?; device
            };
        let action = if let Some(index) = args.iter().position(|s| s == "--dump") {
                anyhow::ensure!(args.get(index+1).is_none_or(|s| s.starts_with("--")), "Use --dump --save FILE to explicitly save a dump");
                Action::Dump
            }
            else if let Some(index) = args.iter().position(|s| s == "--read") {
                let start = number(args.get(index+1).context("read START missing")?)?;
                let len = number(args.get(index+2).context("read LENGTH missing")?)?;
                anyhow::ensure!(args.get(index+3).is_none_or(|s| s.starts_with("--")), "Use --read START LENGTH --save FILE to explicitly save a range");
                Action::Read { region: Region::new(start, len, u32::MAX as usize)? }
            } else { Action::Connect };
        let save = argument(&args, "--save").map(PathBuf::from);
        anyhow::ensure!(!args.iter().any(|s| s == "--save") || save.as_ref().is_some_and(|p| !p.as_os_str().is_empty() && !p.to_string_lossy().starts_with("--")), "--save FILE is required");
        let repeats: usize = argument(&args, "--repeat").unwrap_or("1".into()).parse()?;
        anyhow::ensure!((1..=20).contains(&repeats) && (repeats == 1 || matches!(action, Action::Connect)), "--repeat 1..20 is for --probe only");
        let request = Request { device, action, demo,
            hz: argument(&args, "--hz").unwrap_or("1000000".into()).parse()?,
            model: argument(&args, "--chip"), expected: None, backup: false,
            data_dir: argument(&args, "--data-dir").map(PathBuf::from).unwrap_or_else(app::default_data_dir),
        };
        let flash = backend::demo_flash();
        for _ in 0..repeats { backend::execute_cli_with_save(request.clone(), flash.clone(), save.as_deref())?; }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1180.0, 850.0]).with_min_inner_size([980.0, 700.0]),
        ..Default::default()
    };
    let app_smoke = smoke.clone();
    let smoke_view = argument(&args, "--smoke-view").unwrap_or("read".into());
    eframe::run_native("frwr Flash Utility", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, demo, app_smoke, smoke_view)))))
        .map_err(|e| anyhow::anyhow!("GUI start failed: {e}"))?;
    if let Some(path) = smoke { anyhow::ensure!(path.is_file(), "GUI smoke screenshot was not produced"); }
    Ok(())
}
