mod app;
mod config;
mod gltile;
mod platform;
mod player;
mod tunnel;

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::Duration;

use config::{Config, StreamKind};
use tunnel::TunnelManager;

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_sig: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

pub fn shutdown_requested() -> bool {
    SHUTDOWN.load(Ordering::SeqCst)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "CamView\n\n  camview                    open the viewer\n  camview --import-m3u         import a legacy M3U playlist (read-only)\n  camview --selftest           test tunnels, decoding and recording\n"
        );
        return;
    }

    platform::log_line("CamView starting");

    if args.iter().any(|a| a == "--import-m3u") {
        match config::import_from_m3u() {
            Some((cfg, note)) => match cfg.save() {
                Ok(()) => println!("{note}\nConfig: {}", platform::config_path().display()),
                Err(e) => eprintln!("Could not save: {e}"),
            },
            None => eprintln!("No legacy M3U playlist found in the home directory"),
        }
        return;
    }

    if args.iter().any(|a| a == "--selftest") {
        if let Err(e) = selftest() {
            eprintln!("SELFTEST FALLÓ: {e}");
            std::process::exit(1);
        }
        return;
    }

    let _lock = match platform::acquire_single_instance() {
        Some(lock) => lock,
        None => {
            eprintln!("CamView is already running.");
            // Try to bring the existing window to the front; otherwise notify.
            let _ = std::process::Command::new("xdotool")
                .args(["search", "--name", "^CamView$", "windowactivate"])
                .status();
            let _ = std::process::Command::new("notify-send")
                .args(["CamView", "CamView is already running"])
                .status();
            return;
        }
    };

    unsafe {
        libc::signal(libc::SIGTERM, on_signal as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as libc::sighandler_t);
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            .with_title("CamView"),
        renderer: eframe::Renderer::Glow,
        vsync: true,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        centered: true,
        ..Default::default()
    };

    let result = eframe::run_native(
        "CamView",
        options,
        Box::new(|cc| Ok(Box::new(app::CamViewApp::new(cc)))),
    );
    if let Err(e) = result {
        platform::log_line(&format!("UI error: {e}"));
        eprintln!("Could not open the UI: {e}");
        std::process::exit(1);
    }
}

fn selftest() -> Result<(), String> {
    let (cfg, note) = Config::load_or_import();
    if let Some(note) = note {
        println!("{note}");
    }
    if cfg.cameras.is_empty() {
        return Err("No cameras configured".into());
    }

    let mut tunnels = TunnelManager::new();
    for cam in cfg.cameras.iter().filter(|c| c.enabled) {
        let port = tunnels.ensure(cam)?;
        println!(
            "{}: tunnel {}",
            cam.name,
            port.map(|p| format!("127.0.0.1:{p}"))
                .unwrap_or_else(|| format!("directo {}:{}", cam.host, cam.rtsp_port))
        );
    }

    let mut first = true;
    for cam in cfg.cameras.iter().filter(|c| c.enabled) {
        let port = tunnels.port_for(&cam.id);
        let url = cam.url_for(port, StreamKind::Sub);
        let mpv = player::create_headless()?;
        mpv.command("loadfile", &[&url, "replace"])
            .map_err(|e| format!("{}: {e}", cam.name))?;
        sleep(Duration::from_secs(7));
        let w: i64 = mpv.get_property("video-params/w").unwrap_or(0);
        let h: i64 = mpv.get_property("video-params/h").unwrap_or(0);
        let hw: String = mpv.get_property("hwdec-current").unwrap_or_default();
        let idle: bool = mpv.get_property("core-idle").unwrap_or(true);
        println!("{}: {}x{} hwdec={} idle={}", cam.name, w, h, hw, idle);
        if w == 0 || h == 0 {
            return Err(format!("{} did not provide video", cam.name));
        }

        if first {
            first = false;
            let rec = Path::new("/tmp/camview-selftest.mkv");
            let _ = std::fs::remove_file(rec);
            mpv.set_property("stream-record", rec.to_string_lossy().to_string())
                .map_err(|e| format!("stream-record: {e}"))?;
            sleep(Duration::from_secs(6));
            mpv.set_property("stream-record", String::new())
                .map_err(|e| format!("detener stream-record: {e}"))?;
            sleep(Duration::from_millis(800));
            let meta = std::fs::metadata(rec).map_err(|e| format!("recording: {e}"))?;
            println!("test recording: {} bytes", meta.len());
            if meta.len() < 10_000 {
                return Err("the recorded file is too small".into());
            }
            let out = Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_entries",
                    "stream=codec_name,width,height",
                    "-of",
                    "default=noprint_wrappers=1",
                ])
                .arg(rec)
                .output()
                .map_err(|e| format!("ffprobe: {e}"))?;
            if !out.status.success() {
                return Err("ffprobe could not open the recording".into());
            }
            println!("{}", String::from_utf8_lossy(&out.stdout));
        }
    }

    println!("SELFTEST OK");
    Ok(())
}
