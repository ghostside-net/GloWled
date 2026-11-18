# GloWled 🌈✨

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)](https://www.rust-lang.org/)
[![UI: Slint](https://img.shields.io/badge/UI-Slint-blue.svg)](https://slint.dev/)
[![Platform: Windows Desktop](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux-blue.svg)](https://github.com/ghostside-net/GloWled)

> **High-Performance Ambilight Controller in Rust & Slint**  
> GloWled captures screen edges in real time via Windows DXGI Desktop Duplication (with automatic GDI fallback) and streams edge colors with minimal latency to any **WLED** controller over raw UDP.

---

## ⚡ Key Features & Architecture (v2.0.0)

- **Native Single-Binary (`#![windows_subsystem = "windows"]`):** Starts silently minimized in the Windows System Tray without console windows.
- **Minimal Resource Footprint:**
  - **Headless / Background RSS:** ~5.8 MB (`< 20 MB` target)
  - **60 FPS Processing:** ~0.03 ms per frame (1920x1080 BGRA → 48 LEDs), ~0.18% single-core (~0.04% 4-core equivalent)
  - **Zero Idle UI Overhead:** Uses Slint software rendering and `HideWindow` on close (`[X]`); worker stops emitting UI preview updates while hidden.
- **Resilient Capture Pipeline (`src/capture.rs`):**
  - Windows **DXGI Desktop Duplication API** (`ID3D11Device`, `IDXGIOutputDuplication`) with direct downsampling from mapped staging textures.
  - Automatic recovery without `panic!` on `DXGI_ERROR_ACCESS_LOST`, resolution changes, UAC prompts, monitor standby, and fullscreen games (including automatic GDI `BitBlt` fallback).
- **Edge Processing & Color Pipeline (`src/processing.rs`):**
  - Nyquist-Shannon adaptive capture resolution (`oversample_factor`) and preset modes.
  - Configurable edge depth (percentage or pixel distance) with overlap clamping.
  - **Symmetric Black-Bar Detection:** Automatically skips top/bottom letterbox and left/right pillarbox cinema bars.
  - **Color Corrections:** BGRA/RGBA alpha sanitization, HSV saturation, brightness, RGB white balance, precomputed Gamma LUT, and Hermite smoothstep black-level cutoff (`apply_black_level_threshold`).
  - **Temporal EMA Smoothing:** Exponential moving average filter with black-threshold snapping.
- **WLED Realtime Protocols (`src/wled.rs`):**
  - Supports **DRGB** (`[2, timeout, R, G, B, ...]`, default), **DDP** (10-byte Distributed Display Protocol header + RGB), and **WARLS** (`[1, timeout, idx, R, G, B, ...]`).
  - Eco Mode skips processing on static frames and sends 1 Hz UDP keep-alive packets.
- **System Tray & Windows Integration (`src/tray.rs`, `src/config.rs`):**
  - Native tray menu (`tray-icon` + `muda`): *Status (Aktiv/Pausiert)*, *Einstellungen öffnen*, *Beenden*.
  - Configuration persisted at `%APPDATA%\GloWled\config.json` (backward-compatible with Python v1.x configs; `~/.glowled/config.json` on Unix).
  - Windows Autostart via Registry `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
- **Slint Dark-Mode Settings UI (`ui/settings.slint`):**
  - Four structured tabs: **1. Verbindung**, **2. LED-Geometrie**, **3. Bild & Farbe**, **4. System**, plus live edge preview bar while visible.

---

## 🛠️ Building & Testing

### Prerequisites
- Rust toolchain (`rustc` / `cargo`, edition 2021)
- For Windows builds: target `x86_64-pc-windows-msvc` (with MSVC Build Tools / Windows SDK)

### Build & Run Commands

```bash
# Run all Rust unit and integration tests
cargo test

# Build size- & speed-optimized release binary (LTO, opt-level="z", strip=true)
cargo build --release

# Windows target build
cargo build --release --target x86_64-pc-windows-msvc
```

### CLI Options

```bash
glowled                 # Standard launch (minimized to System Tray by default)
glowled --show          # Launch with settings window open
glowled --minimized     # Force start minimized to System Tray
glowled --autostart     # Silent background autostart launch
glowled --headless      # Run capture/streaming worker in headless CLI mode
glowled --benchmark     # Run 600-frame 1080p BGRA -> LED benchmark and report RSS/CPU
glowled --version       # Print version and exit
```

---

## ⚙️ Configuration Reference (`config.json`)

| Parameter | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `ip` | `string` | `"127.0.0.1"` | Target WLED IP address |
| `port` | `u16` | `21324` | Target UDP port |
| `protocol` | `string` | `"drgb"` | UDP protocol (`"drgb"`, `"ddp"`, `"warls"`) |
| `fps` | `u32` | `60` | Frame pacing rate (1–120 FPS) |
| `brightness` | `f32` | `1.0` | Brightness multiplier (0.05–3.0) |
| `saturation` | `f32` | `1.0` | HSV saturation multiplier (0.0–3.0) |
| `gamma` | `f32` | `1.0` | Gamma exponent (0.5–4.0) |
| `white_balance` | `[f32; 3]` | `[1.0, 1.0, 1.0]` | Per-channel RGB gains |
| `smoothing` | `f32` | `0.1` | Temporal EMA factor (0.0–0.95) |
| `edge_depth` | `f32` | `0.08` | Border sampling depth ratio (`0.02`–`0.50`) or pixels (`> 1.0`) |
| `black_threshold` | `f32` | `15.0` | Black cutoff threshold (0–255) |
| `black_bar_detection` | `bool` | `true` | Skip symmetric letterbox/pillarbox black bars |
| `oversample_factor` | `f32` | `2.0` | Nyquist-Shannon adaptive oversampling factor |
| `eco_mode` | `bool` | `true` | Idle on unchanged frames with 1 Hz keep-alive |
| `resolution` | `string` | `"Auto (Adaptiv)"` | Adaptive or fixed capture downscale preset |
| `start_minimized` | `bool` | `true` | Start hidden in System Tray |
| `autostart` | `bool` | `false` | Register `HKCU\...\Run\GloWled` entry |
| `monitors` | `array` | `[...]` | Per-monitor index, per-edge LEDs, corner, direction, order |

---

## 📄 License

This project is licensed under the **GNU General Public License v3 (GPLv3)**. See [LICENSE](LICENSE) for details.
