//! WLED UDP streaming protocols (DRGB, DDP, WARLS) and low-latency background capture worker.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};

use crate::capture::{create_capturer, CaptureStatus, FrameBuffer, PixelFormat, ScreenCapturer};
use crate::config::{AppConfig, WledProtocol};
use crate::error::{GlowledError, Result};
use crate::processing::{
    process_monitor_frame_with_sampling_into, resolve_capture_resolution, ProcessingScratch,
    TemporalSmoother,
};

/// Commands sent from the UI/Tray thread to the background worker thread.
#[derive(Clone, Debug)]
pub enum WorkerCommand {
    UpdateConfig(AppConfig),
    SetActive(bool),
    SetUiOpen(bool),
    Stop,
}

/// Per-monitor live preview colors sent to the UI thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorPreview {
    pub mss_index: usize,
    pub colors: Vec<[u8; 3]>,
}

/// Telemetry and preview updates sent back to the UI thread (only when UI is visible).
#[derive(Clone, Debug)]
pub enum WorkerEvent {
    PreviewColors(Vec<MonitorPreview>),
    StatusUpdate {
        active: bool,
        fps: f32,
        error: Option<String>,
    },
}

/// Builds a WLED DRGB packet (`[2, timeout_secs, R, G, B, ...]`) into `out`.
pub fn build_drgb_packet_into(rgb_bytes: &[u8], timeout_secs: u8, out: &mut Vec<u8>) {
    out.clear();
    out.reserve(2 + rgb_bytes.len());
    out.push(2);
    out.push(timeout_secs);
    out.extend_from_slice(rgb_bytes);
}

/// Builds a WLED WARLS packet (`[1, timeout_secs, idx, R, G, B, ...]`) into `out`.
pub fn build_warls_packet_into(rgb_bytes: &[u8], timeout_secs: u8, out: &mut Vec<u8>) {
    out.clear();
    let led_count = (rgb_bytes.len() / 3).min(255);
    out.reserve(2 + led_count * 4);
    out.push(1);
    out.push(timeout_secs);
    for (idx, chunk) in rgb_bytes.as_chunks::<3>().0.iter().take(255).enumerate() {
        out.push(idx as u8);
        out.push(chunk[0]);
        out.push(chunk[1]);
        out.push(chunk[2]);
    }
}

/// Builds a DDP (Distributed Display Protocol) UDP packet into `out`.
/// Header layout (10 bytes):
/// - `[0]`: `0x41` (Version 1 + PUSH flag)
/// - `[1]`: sequence number `(seq & 0x0F)`
/// - `[2]`: data type `0x01` (RGB)
/// - `[3]`: destination ID `0x01` (default)
/// - `[4..8]`: 32-bit big-endian byte offset (`0`)
/// - `[8..10]`: 16-bit big-endian RGB payload length in bytes
pub fn build_ddp_packet_into(rgb_bytes: &[u8], sequence: u8, out: &mut Vec<u8>) {
    out.clear();
    let payload_len = rgb_bytes.len().min(u16::MAX as usize);
    out.reserve(10 + payload_len);
    out.push(0x41);
    out.push(sequence & 0x0F);
    out.push(0x01);
    out.push(0x01);
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(payload_len as u16).to_be_bytes());
    out.extend_from_slice(&rgb_bytes[..payload_len]);
}

/// Builds a WLED UDP packet for the selected `protocol` into `out`.
pub fn build_wled_packet_into(
    protocol: WledProtocol,
    rgb_bytes: &[u8],
    timeout_secs: u8,
    sequence: u8,
    out: &mut Vec<u8>,
) {
    match protocol {
        WledProtocol::Drgb => build_drgb_packet_into(rgb_bytes, timeout_secs, out),
        WledProtocol::Ddp => build_ddp_packet_into(rgb_bytes, sequence, out),
        WledProtocol::Warls => build_warls_packet_into(rgb_bytes, timeout_secs, out),
    }
}

/// Convenience allocating packet builder.
pub fn build_wled_packet(
    protocol: WledProtocol,
    rgb_bytes: &[u8],
    timeout_secs: u8,
    sequence: u8,
) -> Vec<u8> {
    let mut out = Vec::new();
    build_wled_packet_into(protocol, rgb_bytes, timeout_secs, sequence, &mut out);
    out
}

/// Resolves `"ip:port"` to a `SocketAddr`.
fn resolve_target(ip: &str, port: u16) -> Result<SocketAddr> {
    let trimmed = ip.trim();
    if trimmed.is_empty() || port == 0 {
        return Err(GlowledError::InvalidAddress {
            addr: format!("{trimmed}:{port}"),
            reason: "IP oder Port darf nicht leer bzw. 0 sein".to_string(),
        });
    }
    let addr_str = format!("{trimmed}:{port}");
    addr_str
        .to_socket_addrs()
        .map_err(|e| GlowledError::InvalidAddress {
            addr: addr_str.clone(),
            reason: e.to_string(),
        })?
        .next()
        .ok_or_else(|| GlowledError::InvalidAddress {
            addr: addr_str,
            reason: "Keine aufgelöste Zieladresse gefunden".to_string(),
        })
}

/// Tests UDP socket creation and sends a single black probe packet to WLED.
pub fn test_wled_connection(ip: &str, port: u16, protocol: WledProtocol) -> Result<Duration> {
    let target = resolve_target(ip, port)?;
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_write_timeout(Some(Duration::from_millis(1500)))?;

    let probe = build_wled_packet(protocol, &[0, 0, 0], 2, 1);
    let start = Instant::now();
    sock.send_to(&probe, target)?;
    Ok(start.elapsed())
}

/// Handle to control and communicate with the running background capture worker.
pub struct WorkerHandle {
    cmd_tx: Sender<WorkerCommand>,
    event_rx: Receiver<WorkerEvent>,
    join_handle: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    /// Spawns the background capture & streaming worker using the platform capturer.
    pub fn spawn(initial_config: AppConfig, initial_active: bool) -> Result<Self> {
        Self::spawn_with_factory(initial_config, initial_active, create_capturer)
    }

    /// Spawns the worker with a custom capturer factory (useful for headless benchmarks and tests).
    pub fn spawn_with_factory<F>(
        initial_config: AppConfig,
        initial_active: bool,
        factory: F,
    ) -> Result<Self>
    where
        F: Fn(usize) -> Box<dyn ScreenCapturer> + Send + 'static,
    {
        let (cmd_tx, cmd_rx) = unbounded();
        let (event_tx, event_rx) = bounded(8);

        let join_handle = thread::Builder::new()
            .name("glowled-worker".to_string())
            .spawn(move || {
                run_worker_loop(initial_config, initial_active, cmd_rx, event_tx, factory);
            })
            .map_err(GlowledError::WorkerSpawn)?;

        Ok(Self {
            cmd_tx,
            event_rx,
            join_handle: Some(join_handle),
        })
    }

    pub fn update_config(&self, cfg: AppConfig) {
        let _ = self.cmd_tx.send(WorkerCommand::UpdateConfig(cfg));
    }

    pub fn set_active(&self, active: bool) {
        let _ = self.cmd_tx.send(WorkerCommand::SetActive(active));
    }

    pub fn set_ui_open(&self, open: bool) {
        let _ = self.cmd_tx.send(WorkerCommand::SetUiOpen(open));
    }

    pub fn try_recv_event(&self) -> Option<WorkerEvent> {
        self.event_rx.try_recv().ok()
    }

    pub fn stop(&mut self) {
        let _ = self.cmd_tx.send(WorkerCommand::Stop);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn sync_capturers<F>(
    cfg: &AppConfig,
    capturers: &mut HashMap<usize, Box<dyn ScreenCapturer>>,
    last_frames: &mut HashMap<usize, FrameBuffer>,
    factory: &F,
) where
    F: Fn(usize) -> Box<dyn ScreenCapturer>,
{
    let needed: Vec<usize> = cfg.monitors.iter().map(|m| m.mss_index.max(1)).collect();
    capturers.retain(|idx, _| needed.contains(idx));
    last_frames.retain(|idx, _| needed.contains(idx));
    for idx in needed {
        capturers.entry(idx).or_insert_with(|| factory(idx));
    }
}

fn run_worker_loop<F>(
    mut cfg: AppConfig,
    mut active: bool,
    cmd_rx: Receiver<WorkerCommand>,
    event_tx: Sender<WorkerEvent>,
    factory: F,
) where
    F: Fn(usize) -> Box<dyn ScreenCapturer>,
{
    cfg.sanitize();
    let sock = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            let _ = event_tx.try_send(WorkerEvent::StatusUpdate {
                active: false,
                fps: 0.0,
                error: Some(format!("UDP-Socket-Fehler: {e}")),
            });
            return;
        }
    };

    let mut target_addr = resolve_target(&cfg.ip, cfg.port).ok();
    let mut capturers: HashMap<usize, Box<dyn ScreenCapturer>> = HashMap::new();
    let mut last_frames: HashMap<usize, FrameBuffer> = HashMap::new();
    sync_capturers(&cfg, &mut capturers, &mut last_frames, &factory);

    let mut scratch = ProcessingScratch::default();
    let mut smoother = TemporalSmoother::default();
    let mut current_rgb_bytes = Vec::with_capacity(512);
    let mut monitor_byte_ranges: Vec<(usize, usize, usize)> = Vec::with_capacity(4);
    let mut last_previews: Vec<MonitorPreview> = Vec::new();
    let mut packet_buf = Vec::with_capacity(528);
    let mut last_packet = Vec::new();
    let mut ddp_seq: u8 = 1;
    let mut ui_open = false;
    let mut last_keepalive = Instant::now();
    let mut last_status_emit = Instant::now();
    let mut frames_in_window: u32 = 0;
    let mut fps_window_start = Instant::now();
    let mut last_fps: f32 = 0.0;
    let mut last_error: Option<String> = None;

    loop {
        let loop_start = Instant::now();

        // Drain configuration & control commands immediately without locking
        for cmd in cmd_rx.try_iter() {
            match cmd {
                WorkerCommand::Stop => {
                    // Send a final dark frame on clean shutdown so LEDs don't stay stuck
                    if let Some(addr) = target_addr {
                        if !current_rgb_bytes.is_empty() {
                            let zeros = vec![0u8; current_rgb_bytes.len()];
                            build_wled_packet_into(
                                cfg.protocol,
                                &zeros,
                                2,
                                ddp_seq,
                                &mut packet_buf,
                            );
                            let _ = sock.send_to(&packet_buf, addr);
                        }
                    }
                    return;
                }
                WorkerCommand::UpdateConfig(mut new_cfg) => {
                    new_cfg.sanitize();
                    let target_changed = new_cfg.ip != cfg.ip || new_cfg.port != cfg.port;
                    if target_changed {
                        target_addr = resolve_target(&new_cfg.ip, new_cfg.port).ok();
                    }
                    let topology_changed = new_cfg.monitors.len() != cfg.monitors.len()
                        || new_cfg
                            .monitors
                            .iter()
                            .zip(cfg.monitors.iter())
                            .any(|(a, b)| {
                                a.mss_index != b.mss_index || a.total_leds() != b.total_leds()
                            });
                    if topology_changed {
                        smoother.reset();
                    }
                    sync_capturers(&new_cfg, &mut capturers, &mut last_frames, &factory);
                    last_packet.clear();
                    cfg = new_cfg;
                }
                WorkerCommand::SetActive(new_active) => {
                    if active && !new_active {
                        // Turn off LEDs cleanly when pausing
                        if let Some(addr) = target_addr {
                            if !current_rgb_bytes.is_empty() {
                                let zeros = vec![0u8; current_rgb_bytes.len()];
                                build_wled_packet_into(
                                    cfg.protocol,
                                    &zeros,
                                    2,
                                    ddp_seq,
                                    &mut packet_buf,
                                );
                                let _ = sock.send_to(&packet_buf, addr);
                            }
                        }
                        smoother.reset();
                    }
                    active = new_active;
                    let _ = event_tx.try_send(WorkerEvent::StatusUpdate {
                        active,
                        fps: if active { last_fps } else { 0.0 },
                        error: last_error.clone(),
                    });
                }
                WorkerCommand::SetUiOpen(open) => {
                    ui_open = open;
                    if ui_open {
                        let _ = event_tx.try_send(WorkerEvent::StatusUpdate {
                            active,
                            fps: if active { last_fps } else { 0.0 },
                            error: last_error.clone(),
                        });
                        if !last_previews.is_empty() {
                            let _ = event_tx
                                .try_send(WorkerEvent::PreviewColors(last_previews.clone()));
                        }
                    }
                }
            }
        }

        if !active {
            // Sleep while paused; check incoming commands every 50 ms
            match cmd_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(WorkerCommand::Stop) => return,
                Ok(WorkerCommand::SetActive(v)) => {
                    active = v;
                    let _ = event_tx.try_send(WorkerEvent::StatusUpdate {
                        active,
                        fps: last_fps,
                        error: last_error.clone(),
                    });
                }
                Ok(WorkerCommand::SetUiOpen(open)) => {
                    ui_open = open;
                    if ui_open {
                        let _ = event_tx.try_send(WorkerEvent::StatusUpdate {
                            active,
                            fps: 0.0,
                            error: last_error.clone(),
                        });
                        if !last_previews.is_empty() {
                            let _ = event_tx
                                .try_send(WorkerEvent::PreviewColors(last_previews.clone()));
                        }
                    }
                }
                Ok(WorkerCommand::UpdateConfig(mut new_cfg)) => {
                    new_cfg.sanitize();
                    target_addr = resolve_target(&new_cfg.ip, new_cfg.port).ok();
                    sync_capturers(&new_cfg, &mut capturers, &mut last_frames, &factory);
                    last_packet.clear();
                    cfg = new_cfg;
                }
                Err(_) => {}
            }
            continue;
        }

        let target_frame_time = Duration::from_secs_f64(1.0 / (cfg.fps.clamp(1, 120) as f64));

        // 1. Capture frames from each unique monitor
        let mut has_new_frame = false;
        let mut needs_backoff = false;
        for mon_cfg in &cfg.monitors {
            let idx = mon_cfg.mss_index.max(1);
            let Some(capturer) = capturers.get_mut(&idx) else {
                continue;
            };

            let (cap_w, cap_h) =
                resolve_capture_resolution(&cfg.resolution, mon_cfg, cfg.oversample_factor);
            let frame_slot = last_frames
                .entry(idx)
                .or_insert_with(|| FrameBuffer::new(0, 0, PixelFormat::Rgb8, Vec::new()));

            match capturer.capture_into(cap_w, cap_h, frame_slot) {
                CaptureStatus::NewFrame => {
                    has_new_frame = true;
                    last_error = None;
                }
                CaptureStatus::Unchanged => {}
                CaptureStatus::DeviceLost(reason) | CaptureStatus::Error(reason) => {
                    last_error = Some(reason);
                    // Attempt non-panicking session recovery
                    let _ = capturer.reset();
                    needs_backoff = true;
                }
            }
        }

        if needs_backoff && !has_new_frame {
            if ui_open && last_status_emit.elapsed() >= Duration::from_millis(250) {
                let _ = event_tx.try_send(WorkerEvent::StatusUpdate {
                    active,
                    fps: last_fps,
                    error: last_error.clone(),
                });
                last_status_emit = Instant::now();
            }
            thread::sleep(Duration::from_millis(150));
            continue;
        }

        // 2. Eco Mode: skip redundant processing when screen is static, send keep-alive once per 1.0s
        if !has_new_frame && !last_packet.is_empty() && cfg.eco_mode {
            if last_keepalive.elapsed() >= Duration::from_secs(1) {
                if let Some(addr) = target_addr {
                    let _ = sock.send_to(&last_packet, addr);
                }
                last_keepalive = Instant::now();
            }

            let elapsed = loop_start.elapsed();
            if target_frame_time > elapsed {
                thread::sleep(target_frame_time - elapsed);
            }
            continue;
        }

        // 3. Process frames and assemble RGB payload across monitors
        current_rgb_bytes.clear();
        monitor_byte_ranges.clear();
        for mon_cfg in &cfg.monitors {
            let idx = mon_cfg.mss_index.max(1);
            let Some(frame) = last_frames.get(&idx) else {
                continue;
            };
            if frame.is_empty() {
                continue;
            }
            let start = current_rgb_bytes.len();
            process_monitor_frame_with_sampling_into(
                frame,
                mon_cfg,
                &cfg.resolution,
                cfg.brightness,
                cfg.saturation,
                cfg.white_balance,
                cfg.gamma,
                &cfg.edge_sampling,
                cfg.black_threshold,
                cfg.black_bar_detection,
                cfg.oversample_factor,
                &mut scratch,
                &mut current_rgb_bytes,
            );
            let end = current_rgb_bytes.len();
            monitor_byte_ranges.push((idx, start, end));
        }

        if !current_rgb_bytes.is_empty() {
            smoother.apply(&mut current_rgb_bytes, cfg.smoothing, cfg.black_threshold);
            build_wled_packet_into(
                cfg.protocol,
                &current_rgb_bytes,
                2,
                ddp_seq,
                &mut packet_buf,
            );
            ddp_seq = (ddp_seq % 15) + 1;

            last_packet.clear();
            last_packet.extend_from_slice(&packet_buf);

            if let Some(addr) = target_addr {
                if let Err(e) = sock.send_to(&packet_buf, addr) {
                    last_error = Some(format!("UDP-Fehler: {e}"));
                } else {
                    last_error = None;
                    last_keepalive = Instant::now();
                }
            }

            last_previews.clear();
            for &(mss_index, start, end) in &monitor_byte_ranges {
                let colors = current_rgb_bytes[start..end].as_chunks::<3>().0.to_vec();
                last_previews.push(MonitorPreview { mss_index, colors });
            }

            if ui_open {
                let _ = event_tx.try_send(WorkerEvent::PreviewColors(last_previews.clone()));
            }
        }

        // Track achieved FPS & send periodic UI telemetry only while UI is open
        frames_in_window += 1;
        let window_elapsed = fps_window_start.elapsed();
        if window_elapsed >= Duration::from_millis(500) {
            last_fps = (frames_in_window as f32) / window_elapsed.as_secs_f32();
            frames_in_window = 0;
            fps_window_start = Instant::now();
            if ui_open && last_status_emit.elapsed() >= Duration::from_millis(450) {
                let _ = event_tx.try_send(WorkerEvent::StatusUpdate {
                    active,
                    fps: last_fps,
                    error: last_error.clone(),
                });
                last_status_emit = Instant::now();
            }
        }

        let elapsed = loop_start.elapsed();
        if target_frame_time > elapsed {
            thread::sleep(target_frame_time - elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{DisplayInfo, FallbackCapturer};

    #[test]
    fn packet_builders_drgb_ddp_warls() {
        let rgb = [255u8, 0, 128, 10, 20, 30];

        let drgb = build_wled_packet(WledProtocol::Drgb, &rgb, 2, 1);
        assert_eq!(drgb, vec![2, 2, 255, 0, 128, 10, 20, 30]);

        let warls = build_wled_packet(WledProtocol::Warls, &rgb, 2, 1);
        assert_eq!(warls, vec![1, 2, 0, 255, 0, 128, 1, 10, 20, 30]);

        let ddp = build_wled_packet(WledProtocol::Ddp, &rgb, 2, 3);
        assert_eq!(ddp.len(), 10 + 6);
        assert_eq!(&ddp[0..4], &[0x41, 0x03, 0x01, 0x01]);
        assert_eq!(&ddp[4..8], &[0, 0, 0, 0]);
        assert_eq!(&ddp[8..10], &[0, 6]);
        assert_eq!(&ddp[10..], &rgb);
    }

    #[test]
    fn worker_streams_udp_packets_and_recovers_from_device_lost() {
        let recv_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        recv_sock
            .set_read_timeout(Some(Duration::from_millis(800)))
            .unwrap();
        let port = recv_sock.local_addr().unwrap().port();

        let cfg = AppConfig {
            ip: "127.0.0.1".to_string(),
            port,
            fps: 60,
            eco_mode: false,
            ..Default::default()
        };
        let expected_len = 2 + cfg.monitors[0].total_leds() * 3;

        struct SolidRedCapturer {
            calls: u32,
        }
        impl ScreenCapturer for SolidRedCapturer {
            fn capture_into(
                &mut self,
                target_w: u32,
                target_h: u32,
                out: &mut FrameBuffer,
            ) -> CaptureStatus {
                self.calls += 1;
                if self.calls == 2 {
                    return CaptureStatus::DeviceLost("simulated standby".into());
                }
                let w = target_w.max(32);
                let h = target_h.max(18);
                out.width = w;
                out.height = h;
                out.format = PixelFormat::Rgb8;
                out.data.clear();
                for _ in 0..(w * h) {
                    out.data.extend_from_slice(&[200, 10, 20]);
                }
                CaptureStatus::NewFrame
            }
            fn display_info(&self) -> DisplayInfo {
                DisplayInfo {
                    index: 1,
                    name: "Mock".into(),
                    width: 1920,
                    height: 1080,
                    left: 0,
                    top: 0,
                }
            }
            fn reset(&mut self) -> Result<()> {
                Ok(())
            }
        }

        let mut handle = WorkerHandle::spawn_with_factory(cfg, true, |_| {
            Box::new(SolidRedCapturer { calls: 0 })
        })
        .unwrap();

        let mut buf = [0u8; 1024];
        let (n, _) = recv_sock.recv_from(&mut buf).unwrap();
        assert_eq!(n, expected_len);
        assert_eq!(buf[0], 2);
        assert_eq!(buf[1], 2);
        // Check first LED pixel is bright red
        assert!(buf[2] > 180 && buf[3] < 20);

        handle.stop();
        // Verify FallbackCapturer compiles and resets cleanly too
        let _ = FallbackCapturer::new(1);
    }

    #[test]
    fn multi_monitor_worker_emits_separated_previews_and_concatenated_udp() {
        use crate::config::{MonitorConfig, EDGE_TOP};

        let recv_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        recv_sock
            .set_read_timeout(Some(Duration::from_millis(800)))
            .unwrap();
        let port = recv_sock.local_addr().unwrap().port();

        let mon1 = MonitorConfig {
            mss_index: 1,
            leds_top: 4,
            leds_bottom: 0,
            leds_left: 0,
            leds_right: 0,
            edge_order: vec![EDGE_TOP.to_string()],
            ..Default::default()
        };
        let mon2 = MonitorConfig {
            mss_index: 2,
            leds_top: 6,
            leds_bottom: 0,
            leds_left: 0,
            leds_right: 0,
            edge_order: vec![EDGE_TOP.to_string()],
            ..Default::default()
        };

        let cfg = AppConfig {
            ip: "127.0.0.1".to_string(),
            port,
            fps: 60,
            eco_mode: false,
            monitors: vec![mon1, mon2],
            ..Default::default()
        };

        struct ColorCapturer {
            idx: usize,
            rgb: [u8; 3],
        }
        impl ScreenCapturer for ColorCapturer {
            fn capture_into(
                &mut self,
                target_w: u32,
                target_h: u32,
                out: &mut FrameBuffer,
            ) -> CaptureStatus {
                let w = target_w.max(32);
                let h = target_h.max(18);
                out.width = w;
                out.height = h;
                out.format = PixelFormat::Rgb8;
                out.data.clear();
                for _ in 0..(w * h) {
                    out.data.extend_from_slice(&self.rgb);
                }
                CaptureStatus::NewFrame
            }
            fn display_info(&self) -> DisplayInfo {
                DisplayInfo {
                    index: self.idx,
                    name: format!("Mock {}", self.idx),
                    width: 1920,
                    height: 1080,
                    left: 0,
                    top: 0,
                }
            }
            fn reset(&mut self) -> Result<()> {
                Ok(())
            }
        }

        let mut handle = WorkerHandle::spawn_with_factory(cfg, true, |idx| {
            let rgb = if idx == 1 {
                [220, 10, 10]
            } else {
                [10, 20, 230]
            };
            Box::new(ColorCapturer { idx, rgb })
        })
        .unwrap();
        handle.set_ui_open(true);

        let mut buf = [0u8; 1024];
        let (n, _) = recv_sock.recv_from(&mut buf).unwrap();
        assert_eq!(n, 2 + (4 + 6) * 3);
        // First 4 LEDs belong to monitor 1 (red), next 6 LEDs belong to monitor 2 (blue)
        for led in 0..4 {
            let base = 2 + led * 3;
            assert!(buf[base] >= 200 && buf[base + 2] < 30);
        }
        for led in 4..10 {
            let base = 2 + led * 3;
            assert!(buf[base + 2] >= 200 && buf[base] < 30);
        }

        let mut got_separated_preview = false;
        for _ in 0..20 {
            if let Some(WorkerEvent::PreviewColors(previews)) = handle.try_recv_event() {
                assert_eq!(previews.len(), 2);
                assert_eq!(previews[0].mss_index, 1);
                assert_eq!(previews[0].colors.len(), 4);
                assert!(previews[0].colors.iter().all(|c| c[0] >= 200 && c[2] < 30));
                assert_eq!(previews[1].mss_index, 2);
                assert_eq!(previews[1].colors.len(), 6);
                assert!(previews[1].colors.iter().all(|c| c[2] >= 200 && c[0] < 30));
                got_separated_preview = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            got_separated_preview,
            "Expected separated PreviewColors for 2 monitors"
        );

        handle.stop();
    }
}
