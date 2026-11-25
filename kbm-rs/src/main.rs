#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod file_picker;
mod ui;
mod ui_benchmark;
mod ui_firmware;
mod ui_forward;
mod ui_logs;
mod ui_network;
mod ui_preferences;
mod ui_preview;
mod ui_reference;
mod ui_rename;
mod ui_storage;
mod ui_tty;
mod ui_wake;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let profile = args
        .iter()
        .position(|a| a == "--profile")
        .map(|at| {
            args.get(at + 1)
                .filter(|s| !s.starts_with("--"))
                .map(std::path::PathBuf::from)
                .ok_or("--profile requires a directory")
        })
        .transpose()?;
    // Resolve exclusive diagnostic mode before any hardware probe branch.
    let benchmark = ui_benchmark::Session::parse(&args)?;
    let layout_preview = ui_preview::Layout::parse(&args)?;
    #[cfg(windows)]
    if args.iter().any(|a| a == "--media-list") {
        println!(
            "{}",
            serde_json::to_string_pretty(&minikmb_native::media::devices()?)?
        );
        return Ok(());
    }
    #[cfg(windows)]
    if let Some(at) = args.iter().position(|a| a == "--video-probe") {
        let root = args.get(at + 1).ok_or("video root required")?;
        let output = args.get(at + 2).ok_or("output PNG required")?;
        let mut capture = minikmb_native::media::Capture::start(
            root.clone(),
            minikmb_native::media::CaptureMode {
                width: 1920,
                height: 1080,
                fps: 30.0,
            },
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(7);
        loop {
            let (frame, error) = {
                let s = capture.state.lock();
                (s.frame.clone(), s.error.clone())
            };
            if let Some(frame) = frame {
                image::load_from_memory_with_format(&frame.jpeg, image::ImageFormat::Jpeg)?
                    .save(output)?;
                println!(
                    "{}",
                    serde_json::json!({"width":frame.width,"height":frame.height,"fps":frame.fps,"frame_id":frame.id,"jpeg_bytes":frame.jpeg.len()})
                );
                capture.stop();
                return Ok(());
            }
            if !error.is_empty() {
                return Err(error.into());
            }
            if std::time::Instant::now() > deadline {
                return Err("video probe timeout".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    if std::env::args().any(|a| a == "--inspect") {
        let mut devices = minikmb_native::device::DeviceManager::default();
        println!("{}", serde_json::to_string_pretty(&devices.refresh(true))?);
        return Ok(());
    }
    let video_probe = args.iter().position(|a| a == "--video-ui-probe");
    let temporary = (video_probe.is_some() || args.iter().any(|s| s == "--screenshot"))
        .then(tempfile::tempdir)
        .transpose()?;
    let data = benchmark
        .as_ref()
        .map(|b| b.directory.join("profile"))
        .or_else(|| temporary.as_ref().map(|d| d.path().to_owned()))
        .or(profile)
        .unwrap_or(
            directories::ProjectDirs::from("local", "MiniKMB", "MiniKMB Native")
                .ok_or("configuration directory missing")?
                .data_local_dir()
                .to_owned(),
        );
    let app = minikmb_native::service::AppService::new(data)?;
    let screenshot = benchmark
        .as_ref()
        .map(|b| b.directory.join("frame.png"))
        .or_else(|| {
            video_probe
                .and_then(|at| args.get(at + 2))
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    args.iter()
                        .position(|a| a == "--screenshot")
                        .and_then(|at| args.get(at + 1))
                        .map(std::path::PathBuf::from)
                })
        });
    if video_probe.is_some() && screenshot.is_none() {
        return Err("--video-ui-probe requires exact UID and output path".into());
    }
    if screenshot.is_some() {
        app.logs.quiet();
    } else {
        app.logs.emit(
            minikmb_native::logging::Level::Info,
            minikmb_native::logging::Event::AppStarted,
            Default::default(),
        );
    }
    let worker = if screenshot.is_none() {
        Some(app.start_discovery())
    } else {
        None
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut video_devices = None;
    let input_monitor = if screenshot.is_none() {
        Some(app.start_input_monitor(runtime.handle()))
    } else {
        None
    };
    let preference_monitor = if screenshot.is_none() {
        Some(app.start_preference_monitor())
    } else {
        None
    };
    if let Some(at) = video_probe {
        let uid = minikmb_native::model::Uid::try_from(
            args.get(at + 1).ok_or("exact UID required")?.clone(),
        )?;
        let mut devices = minikmb_native::device::DeviceManager::default();
        *app.inventory.write() = devices.refresh(true);
        runtime.block_on(async {
            app.select(Some(uid)).await?;
            app.open_video(Default::default()).await?;
            Ok::<_, minikmb_native::model::AppError>(())
        })?;
        video_devices = Some(devices);
    }
    let window_input = std::sync::Arc::new(minikmb_native::window_input::WindowInput::default());
    let observed = window_input.clone();
    let observe_window = screenshot.is_none();
    let preview_window = screenshot.is_some();
    // Keep egui's feature/limit selection; only request smaller allocation pools.
    // Large long-lived GPU heaps are unnecessary for a latest-frame desktop UI.
    let mut gpu_setup = eframe::egui_wgpu::WgpuSetupCreateNew::default();
    let default_descriptor = gpu_setup.device_descriptor.clone();
    gpu_setup.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut descriptor = default_descriptor(adapter);
        descriptor.memory_hints = wgpu::MemoryHints::MemoryUsage;
        descriptor
    });
    let options = eframe::NativeOptions {
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: gpu_setup.into(),
            ..Default::default()
        },
        event_loop_builder: Some(Box::new(move |builder| {
            if observe_window {
                use winit::platform::windows::EventLoopBuilderExtWindows;
                builder.with_msg_hook(move |message| unsafe { observed.observe(message) });
            }
        })),
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([840.0, 560.0])
            .with_active(!preview_window),
        ..Default::default()
    };
    let service = app.clone();
    let result = eframe::run_native(
        "MiniKMB",
        options,
        Box::new(move |cc| {
            Ok(Box::new(ui::Desktop::new(
                cc,
                service,
                runtime,
                screenshot,
                window_input,
                benchmark,
                layout_preview,
            )))
        }),
    );
    app.stop();
    if let Some(monitor) = input_monitor {
        monitor.abort();
    }
    drop(video_devices);
    if let Some(worker) = worker {
        let _ = worker.join();
    }
    if let Some(worker) = preference_monitor {
        let _ = worker.join();
    }
    result.map_err(|e| e.to_string().into())
}
