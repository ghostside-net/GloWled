#![windows_subsystem = "windows"]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use glowled::capture::{enumerate_displays, DisplayInfo, FrameBuffer, PixelFormat};
use glowled::config::{
    get_config_file_path, get_current_executable_path, is_autostart_enabled, load_config,
    save_config, set_autostart, AppConfig, StartCorner, WledProtocol,
};
use glowled::processing::{process_monitor_frame_into, ProcessingScratch, TemporalSmoother};
use glowled::tray::{TrayAction, TrayManager};
use glowled::wled::{build_wled_packet_into, test_wled_connection, WorkerEvent, WorkerHandle};
use slint::{Color, ModelRc, SharedString, Timer, TimerMode, VecModel};

slint::include_modules!();

const RESOLUTION_OPTIONS: [&str; 5] = [
    "Auto (Adaptiv)",
    "Performance (64x36)",
    "Balance (128x72)",
    "Quality (256x144)",
    "Ultra (512x288)",
];

fn resolution_to_index(res: &str) -> i32 {
    match res {
        "Performance (64x36)" => 1,
        "Balance (128x72)" | "Balanced (128x72)" => 2,
        "Quality (256x144)" => 3,
        "Ultra (512x288)" => 4,
        _ => 0,
    }
}

fn index_to_resolution(idx: i32) -> &'static str {
    RESOLUTION_OPTIONS
        .get(idx.max(0) as usize)
        .copied()
        .unwrap_or("Auto (Adaptiv)")
}

#[cfg(windows)]
mod single_instance {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows::Win32::System::Threading::CreateMutexW;

    pub struct InstanceGuard(HANDLE);

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub fn acquire() -> Option<InstanceGuard> {
        unsafe {
            let handle = CreateMutexW(None, false, w!("Global\\GloWled_SingleInstance_Mutex_v2"))
                .ok()?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return None;
            }
            Some(InstanceGuard(handle))
        }
    }
}

#[cfg(not(windows))]
mod single_instance {
    pub struct InstanceGuard;
    pub fn acquire() -> Option<InstanceGuard> {
        Some(InstanceGuard)
    }
}

struct CliFlags {
    version: bool,
    headless: bool,
    benchmark: bool,
    autostart: bool,
    minimized: bool,
    force_show: bool,
}

impl CliFlags {
    fn parse() -> Self {
        let mut flags = Self {
            version: false,
            headless: false,
            benchmark: false,
            autostart: false,
            minimized: false,
            force_show: false,
        };
        for arg in std::env::args().skip(1) {
            match arg.as_str() {
                "--version" | "-V" => flags.version = true,
                "--headless" => flags.headless = true,
                "--benchmark" => flags.benchmark = true,
                "--autostart" => flags.autostart = true,
                "--minimized" => flags.minimized = true,
                "--show" => flags.force_show = true,
                _ => {}
            }
        }
        flags
    }
}

fn run_headless_mode(cfg: AppConfig) {
    println!("GloWled v{} — Headless Streaming Service", env!("CARGO_PKG_VERSION"));
    println!(
        "Ziel: {}:{} ({}) @ {} FPS | Eco-Modus: {}",
        cfg.ip,
        cfg.port,
        cfg.protocol.as_str(),
        cfg.fps,
        cfg.eco_mode
    );

    let running = Arc::new(AtomicBool::new(true));
    let mut worker = WorkerHandle::spawn(cfg, true);

    while running.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(250));
    }
    worker.stop();
}

fn read_process_rss_kb() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb_str = rest.split_whitespace().next()?;
                return kb_str.parse::<u64>().ok();
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn run_benchmark_mode() {
    let mut cfg = AppConfig::default();
    cfg.fps = 60;
    cfg.black_bar_detection = true;
    let mon = &cfg.monitors[0];

    // Simulate 1920x1080 BGRA desktop capture frame downsampled into processing thumbnail
    let src_w = 1920u32;
    let src_h = 1080u32;
    let mut bgra = vec![0u8; (src_w * src_h * 4) as usize];
    for y in 0..src_h {
        for x in 0..src_w {
            let idx = ((y * src_w + x) * 4) as usize;
            bgra[idx] = (x % 255) as u8;
            bgra[idx + 1] = (y % 255) as u8;
            bgra[idx + 2] = 180;
            bgra[idx + 3] = 255;
        }
    }

    let mut thumb = FrameBuffer::new(0, 0, PixelFormat::Rgb8, Vec::new());
    let mut scratch = ProcessingScratch::default();
    let mut smoother = TemporalSmoother::default();
    let mut rgb_out = Vec::with_capacity(mon.total_leds() * 3);
    let mut pkt = Vec::with_capacity(mon.total_leds() * 3 + 16);

    let iterations = 600u32; // 10 seconds worth of 60 FPS frames
    let start = Instant::now();
    for i in 0..iterations {
        glowled::capture::downsample_raw_into_rgb8(
            &bgra,
            (src_w as usize) * 4,
            src_w,
            src_h,
            PixelFormat::Bgra8,
            128,
            72,
            &mut thumb,
        );
        rgb_out.clear();
        process_monitor_frame_into(
            &thumb,
            mon,
            &cfg.resolution,
            cfg.brightness,
            cfg.saturation,
            cfg.white_balance,
            cfg.gamma,
            cfg.edge_depth,
            cfg.black_threshold,
            cfg.black_bar_detection,
            cfg.oversample_factor,
            &mut scratch,
            &mut rgb_out,
        );
        smoother.apply(&mut rgb_out, cfg.smoothing, cfg.black_threshold);
        build_wled_packet_into(cfg.protocol, &rgb_out, 2, (i % 15) as u8 + 1, &mut pkt);
    }
    let total = start.elapsed();
    let per_frame_us = total.as_secs_f64() * 1_000_000.0 / (iterations as f64);
    let budget_60fps_us = 1_000_000.0 / 60.0;
    let single_core_pct = (per_frame_us / budget_60fps_us) * 100.0;

    println!("GloWled Benchmark ({iterations} Frames @ 1920x1080 BGRA -> 48 LEDs):");
    println!("  Gesamtzeit:         {:.2} ms", total.as_secs_f64() * 1000.0);
    println!("  Zeit pro Frame:     {:.2} µs ({:.3} ms)", per_frame_us, per_frame_us / 1000.0);
    println!(
        "  60 FPS Single-Core: {:.2}% (4-Core Äquivalent: {:.2}%)",
        single_core_pct,
        single_core_pct / 4.0
    );
    if let Some(rss_kb) = read_process_rss_kb() {
        println!("  Prozess RSS:        {:.2} MB ({} KB)", rss_kb as f64 / 1024.0, rss_kb);
    }
}

fn build_monitor_models(displays: &[DisplayInfo]) -> ModelRc<SharedString> {
    let items: Vec<SharedString> = displays
        .iter()
        .map(|d| SharedString::from(format!("{} ({}x{})", d.name, d.width, d.height)))
        .collect();
    ModelRc::new(VecModel::from(items))
}

fn populate_ui_from_config(
    ui: &SettingsWindow,
    cfg: &AppConfig,
    displays: &[DisplayInfo],
    active: bool,
) {
    ui.set_wled_ip(SharedString::from(cfg.ip.as_str()));
    ui.set_wled_port(SharedString::from(cfg.port.to_string()));
    ui.set_protocol_index(cfg.protocol.to_index());

    let mon = cfg.monitors.first().cloned().unwrap_or_default();
    ui.set_leds_top(mon.leds_top as i32);
    ui.set_leds_bottom(mon.leds_bottom as i32);
    ui.set_leds_left(mon.leds_left as i32);
    ui.set_leds_right(mon.leds_right as i32);
    ui.set_start_corner_index(mon.start_corner.to_index());
    ui.set_clockwise(mon.clockwise);

    ui.set_fps_limit(cfg.fps as i32);
    ui.set_gamma(cfg.gamma);
    ui.set_brightness(cfg.brightness);
    ui.set_saturation(cfg.saturation);
    ui.set_smoothing_pct((cfg.smoothing * 100.0).round() as i32);
    ui.set_black_threshold(cfg.black_threshold.round() as i32);
    ui.set_edge_depth_pct((cfg.edge_depth * 100.0).round().clamp(2.0, 50.0) as i32);
    ui.set_black_bar_detection(cfg.black_bar_detection);
    ui.set_resolution_index(resolution_to_index(&cfg.resolution));

    ui.set_monitor_models(build_monitor_models(displays));
    let sel_idx = displays
        .iter()
        .position(|d| d.index == mon.mss_index)
        .unwrap_or(0) as i32;
    ui.set_selected_monitor_index(sel_idx);

    ui.set_ambilight_active(active);
    ui.set_eco_mode(cfg.eco_mode);
    ui.set_autostart(cfg.autostart || is_autostart_enabled());
    ui.set_start_minimized(cfg.start_minimized);
    ui.set_status_badge_text(SharedString::from(if active { "AKTIV" } else { "PAUSIERT" }));
}

fn read_config_from_ui(
    ui: &SettingsWindow,
    current_cfg: &AppConfig,
    displays: &[DisplayInfo],
) -> AppConfig {
    let mut cfg = current_cfg.clone();

    let ip = ui.get_wled_ip().trim().to_string();
    if !ip.is_empty() {
        cfg.ip = ip;
    }
    if let Ok(port) = ui.get_wled_port().trim().parse::<u16>() {
        if port > 0 {
            cfg.port = port;
        }
    }
    cfg.protocol = WledProtocol::from_index(ui.get_protocol_index());

    cfg.fps = ui.get_fps_limit().clamp(15, 120) as u32;
    cfg.gamma = ui.get_gamma().clamp(0.5, 4.0);
    cfg.brightness = ui.get_brightness().clamp(0.05, 3.0);
    cfg.saturation = ui.get_saturation().clamp(0.0, 3.0);
    cfg.smoothing = (ui.get_smoothing_pct() as f32 / 100.0).clamp(0.0, 0.95);
    cfg.black_threshold = (ui.get_black_threshold() as f32).clamp(0.0, 255.0);
    cfg.edge_depth = (ui.get_edge_depth_pct() as f32 / 100.0).clamp(0.01, 0.50);
    cfg.black_bar_detection = ui.get_black_bar_detection();
    cfg.resolution = index_to_resolution(ui.get_resolution_index()).to_string();
    cfg.eco_mode = ui.get_eco_mode();
    cfg.autostart = ui.get_autostart();
    cfg.start_minimized = ui.get_start_minimized();
    cfg.last_known_exe_path = get_current_executable_path();

    let sel_mon = ui.get_selected_monitor_index().max(0) as usize;
    let mss_index = displays.get(sel_mon).map(|d| d.index).unwrap_or(1);

    let mut mon = cfg.monitors.first().cloned().unwrap_or_default();
    mon.mss_index = mss_index;
    mon.leds_top = ui.get_leds_top().max(0) as usize;
    mon.leds_bottom = ui.get_leds_bottom().max(0) as usize;
    mon.leds_left = ui.get_leds_left().max(0) as usize;
    mon.leds_right = ui.get_leds_right().max(0) as usize;
    mon.start_corner = StartCorner::from_index(ui.get_start_corner_index());
    mon.clockwise = ui.get_clockwise();
    mon.sync_geometry_from_corner_and_direction();

    if cfg.monitors.is_empty() {
        cfg.monitors.push(mon);
    } else {
        cfg.monitors[0] = mon;
    }

    cfg.sanitize();
    cfg
}

fn main() {
    let flags = CliFlags::parse();
    if flags.version {
        println!("GloWled v{}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if flags.benchmark {
        run_benchmark_mode();
        return;
    }

    let _instance_guard = match single_instance::acquire() {
        Some(guard) => guard,
        None => return,
    };

    let mut cfg = load_config();
    cfg.sanitize();
    if cfg.autostart {
        let exe = get_current_executable_path();
        if !exe.is_empty() {
            let _ = set_autostart(true, Some(&exe));
        }
    }

    if flags.headless {
        run_headless_mode(cfg);
        return;
    }

    let start_minimized = if flags.force_show {
        false
    } else {
        flags.autostart || flags.minimized || cfg.start_minimized
    };

    let ui = SettingsWindow::new().expect("Failed to create Slint SettingsWindow");
    let displays = Rc::new(RefCell::new(enumerate_displays()));
    let cfg_state = Rc::new(RefCell::new(cfg.clone()));
    let active_state = Rc::new(Cell::new(true));
    let ui_visible = Rc::new(Cell::new(!start_minimized));

    let worker = Rc::new(RefCell::new(WorkerHandle::spawn(cfg.clone(), true)));
    worker.borrow().set_ui_open(!start_minimized);

    let tray = match TrayManager::new(true) {
        Ok(t) => Some(Rc::new(t)),
        Err(e) => {
            eprintln!("[Tray] System-Tray nicht verfügbar: {e}");
            None
        }
    };

    populate_ui_from_config(&ui, &cfg, &displays.borrow(), true);
    if !get_config_file_path().exists() {
        let _ = save_config(&cfg);
    }

    // Close button [X] hides window to tray instead of exiting the process
    {
        let worker_ref = worker.clone();
        let vis_ref = ui_visible.clone();
        ui.window().on_close_requested(move || {
            vis_ref.set(false);
            worker_ref.borrow().set_ui_open(false);
            slint::CloseRequestResponse::HideWindow
        });
    }

    // Hide-to-tray button callback
    {
        let ui_weak = ui.as_weak();
        let worker_ref = worker.clone();
        let vis_ref = ui_visible.clone();
        ui.on_hide_to_tray(move || {
            vis_ref.set(false);
            worker_ref.borrow().set_ui_open(false);
            if let Some(win) = ui_weak.upgrade() {
                let _ = win.hide();
            }
        });
    }

    // Test WLED UDP connection callback
    {
        let ui_weak = ui.as_weak();
        ui.on_test_connection(move || {
            let Some(win) = ui_weak.upgrade() else { return };
            let ip = win.get_wled_ip().trim().to_string();
            let port = win.get_wled_port().trim().parse::<u16>().unwrap_or(21324);
            let proto = WledProtocol::from_index(win.get_protocol_index());

            match test_wled_connection(&ip, port, proto) {
                Ok(dur) => {
                    win.set_connection_ok(true);
                    win.set_connection_status(SharedString::from(format!(
                        "✓ {} Probe an {}:{} gesendet ({:.2} ms)",
                        proto.as_str(),
                        ip,
                        port,
                        dur.as_secs_f64() * 1000.0
                    )));
                }
                Err(err) => {
                    win.set_connection_ok(false);
                    win.set_connection_status(SharedString::from(format!("⚠ {err}")));
                }
            }
        });
    }

    // Detect monitors callback
    {
        let ui_weak = ui.as_weak();
        let displays_ref = displays.clone();
        ui.on_detect_monitors(move || {
            let new_displays = enumerate_displays();
            *displays_ref.borrow_mut() = new_displays.clone();
            if let Some(win) = ui_weak.upgrade() {
                win.set_monitor_models(build_monitor_models(&new_displays));
                win.set_live_status_text(SharedString::from(format!(
                    "{} Monitor(e) erkannt",
                    new_displays.len()
                )));
            }
        });
    }

    // Autostart toggle callback
    {
        let ui_weak = ui.as_weak();
        let cfg_ref = cfg_state.clone();
        ui.on_toggle_autostart(move |enabled| {
            let exe = get_current_executable_path();
            let res = set_autostart(enabled, (!exe.is_empty()).then_some(exe.as_str()));
            let mut cfg = cfg_ref.borrow_mut();
            cfg.autostart = enabled;
            let _ = save_config(&cfg);
            if let Some(win) = ui_weak.upgrade() {
                match res {
                    Ok(()) => win.set_live_status_text(SharedString::from(if enabled {
                        "Autostart in Windows-Registry aktiviert"
                    } else {
                        "Autostart aus Windows-Registry entfernt"
                    })),
                    Err(e) => win.set_live_status_text(SharedString::from(format!(
                        "Autostart-Fehler: {e}"
                    ))),
                }
            }
        });
    }

    // Toggle active streaming callback
    {
        let ui_weak = ui.as_weak();
        let worker_ref = worker.clone();
        let active_ref = active_state.clone();
        let tray_ref = tray.clone();
        ui.on_toggle_active(move |active| {
            active_ref.set(active);
            worker_ref.borrow().set_active(active);
            if let Some(t) = &tray_ref {
                t.set_active(active);
            }
            if let Some(win) = ui_weak.upgrade() {
                win.set_status_badge_text(SharedString::from(if active {
                    "AKTIV"
                } else {
                    "PAUSIERT"
                }));
            }
        });
    }

    // Apply & save settings callback
    {
        let ui_weak = ui.as_weak();
        let cfg_ref = cfg_state.clone();
        let displays_ref = displays.clone();
        let worker_ref = worker.clone();
        ui.on_apply_settings(move || {
            let Some(win) = ui_weak.upgrade() else { return };
            let new_cfg = read_config_from_ui(&win, &cfg_ref.borrow(), &displays_ref.borrow());
            *cfg_ref.borrow_mut() = new_cfg.clone();
            worker_ref.borrow().update_config(new_cfg.clone());
            match save_config(&new_cfg) {
                Ok(()) => win.set_live_status_text(SharedString::from(format!(
                    "Gespeichert • Ziel {}:{} ({})",
                    new_cfg.ip,
                    new_cfg.port,
                    new_cfg.protocol.as_str()
                ))),
                Err(e) => win.set_live_status_text(SharedString::from(format!(
                    "Speicherfehler: {e}"
                ))),
            }
        });
    }

    // 50 ms Slint Timer for Tray events & live UI preview (no-op on UI when hidden!)
    let poll_timer = Timer::default();
    {
        let ui_weak = ui.as_weak();
        let tray_ref = tray.clone();
        let worker_ref = worker.clone();
        let active_ref = active_state.clone();
        let vis_ref = ui_visible.clone();

        poll_timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
            if let Some(t) = &tray_ref {
                for action in t.poll_actions() {
                    match action {
                        TrayAction::ToggleActive => {
                            let next = !active_ref.get();
                            active_ref.set(next);
                            worker_ref.borrow().set_active(next);
                            t.set_active(next);
                            if let Some(win) = ui_weak.upgrade() {
                                win.set_ambilight_active(next);
                                win.set_status_badge_text(SharedString::from(if next {
                                    "AKTIV"
                                } else {
                                    "PAUSIERT"
                                }));
                            }
                        }
                        TrayAction::OpenSettings => {
                            vis_ref.set(true);
                            worker_ref.borrow().set_ui_open(true);
                            if let Some(win) = ui_weak.upgrade() {
                                let _ = win.show();
                            }
                        }
                        TrayAction::Quit => {
                            worker_ref.borrow_mut().stop();
                            let _ = slint::quit_event_loop();
                            return;
                        }
                    }
                }
            }

            // Drain worker telemetry; only mutate Slint models while window is visible
            while let Some(ev) = worker_ref.borrow().try_recv_event() {
                if !vis_ref.get() {
                    continue;
                }
                let Some(win) = ui_weak.upgrade() else {
                    continue;
                };
                match ev {
                    WorkerEvent::PreviewColors(colors) => {
                        let slint_colors: Vec<Color> = colors
                            .into_iter()
                            .map(|[r, g, b]| Color::from_rgb_u8(r, g, b))
                            .collect();
                        win.set_preview_colors(ModelRc::new(VecModel::from(slint_colors)));
                    }
                    WorkerEvent::StatusUpdate { active, fps, error } => {
                        win.set_ambilight_active(active);
                        win.set_status_badge_text(SharedString::from(if active {
                            "AKTIV"
                        } else {
                            "PAUSIERT"
                        }));
                        let msg = match error {
                            Some(err) => format!("Hinweis: {err}"),
                            None if active => format!("Streaming aktiv • {fps:.1} FPS"),
                            None => "Streaming pausiert".to_string(),
                        };
                        win.set_live_status_text(SharedString::from(msg));
                    }
                }
            }
        });
    }

    if !start_minimized {
        ui.show().expect("Failed to show GloWled settings window");
    }

    slint::run_event_loop_until_quit().expect("Slint event loop failed");
    worker.borrow_mut().stop();
}
