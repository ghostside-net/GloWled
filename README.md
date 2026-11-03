# GloWled 🌈✨

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Platform: Windows & Cross-Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux-blue.svg)](https://github.com/ghostside-net/GloWled)
[![Language: Python 3.10+](https://img.shields.io/badge/Python-3.10%2B-yellow.svg)](https://www.python.org/)
[![Performance: Zero-Copy SIMD](https://img.shields.io/badge/Performance-Zero--Copy%20SIMD-success.svg)](https://github.com/ghostside-net/GloWled)
[![CPU Usage: < 1%](https://img.shields.io/badge/CPU%20Impact-%3C%201%25-brightgreen.svg)](https://github.com/ghostside-net/GloWled)

> **Bring your battlestation to life!** 🌟  
> GloWled is an ultra-lightweight, high-performance ambient lighting controller that mirrors your screen colors live to any **WLED**-powered LED setup with virtually zero latency and minimal CPU footprint.

Whether you are deep in an intense gaming session, enjoying a cinematic movie night, or chilling to music, GloWled seamlessly extends your visual field onto your walls without stealing valuable frames from your games.

---

## ⚡ Highlights & Key Features

- 🏎️ **Blazing Real-Time Performance:** GPU/DirectX Desktop Duplication (`DXCam`) captures screens with sub-millisecond overhead.
- 📐 **Adaptive Sampling Resolution (Nyquist-Shannon):** Automatically computes optimal downscaling resolutions based on per-edge LED counts with configurable oversampling (2.0x–4.0x), eliminating aliasing while avoiding redundant processing.
- 🌑 **True Black Level & Dark Cutoff:** Eliminates unwanted white standby glow on dark or black scenes. Configurable `black_threshold` completely shuts LEDs off (`(0, 0, 0)`) below the threshold, with a smooth Hermite smoothstep fade-out to prevent flicker.
- 📏 **Configurable Edge Depth & ROIs:** Calibrate sampling margin depth via relative percentages (0–50%) or absolute pixel distances, automatically handling letterbox black bars and asymmetric aspect ratios without edge overlap.
- 🍃 **Intelligent Eco Mode:** Dynamically detects static screens (reading, paused media, desktop idling) and throttles redundant processing, dropping CPU load to nearly **0.1%**!
- 🎨 **Cinematic Color & Smoothing:**
  - **Color Smoothing:** Adjustable exponential transition filter (0–85%) prevents jarring flicker.
  - **Saturation & Brightness Multipliers:** Give your room that extra vibrant punch or dim down for nighttime sessions.
- 🖥️ **Multi-Monitor & Smart Edge Routing:**
  - Multi-display setup out-of-the-box with automatic display detection.
  - 1-click presets: *Clockwise*, *Counter-Clockwise*, and custom interactive edge sequencing.
  - Individual edge direction inversion (flip left, top, right, or bottom LED strips independently).
- 🪟 **Modern, Intuitive GUI:** Sleek dark-mode interface powered by CustomTkinter, featuring real-time connection diagnostics (WLED UDP ping test).
- 🛡️ **Unobtrusive System Tray Integration:** Closes to the system tray, with quick controls to start, pause, or configure ambient lighting.
- 🚀 **Silent Background Autostart:** Can launch silently minimized to the tray when Windows boots.
- 🐧 **Cross-Platform Compatibility:** Automatic fallback to MSS capture engine for Linux and environments without DXGI.

---

## 📸 Interface Preview

The application offers an intuitive tabbed interface designed for quick tuning:
- **⚡ Dashboard:** Live status badge, active target device, real-time FPS counter, and 1-click UDP ping check.
- **⚙️ WLED & Netzwerk:** Target IP & Port, FPS sliders with quick presets (Eco 30 FPS, Smooth 45 FPS, Ultra 60 FPS), and Eco Mode toggles.
- **🎨 Bild & Effekte:** Live brightness, saturation boost, color smoothing, border sampling depth, black cutoff threshold, and adaptive resolution presets.
- **🖥️ Monitore:** Card-based multi-display management, per-side LED counters, total LED tally, and edge ordering.
- **ℹ️ Info & Hilfe:** Built-in connection guide and shortcuts.

---

## ⚙️ Configuration Reference

Configuration is persisted in JSON format (`config.json`) in `%APPDATA%\GloWled` (Windows) or `~/.glowled` (Linux):

| Parameter | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `ip` | `str` | `"127.0.0.1"` | Target IP address of the WLED device. |
| `port` | `int` | `21324` | WLED realtime UDP (DRGB) listener port. |
| `fps` | `int` | `30` | Streaming frame rate (1–120 FPS). |
| `brightness` | `float` | `1.0` | Global brightness multiplier (0.1x–3.0x). |
| `saturation` | `float` | `1.0` | Color saturation multiplier (0.0x–3.0x). |
| `smoothing` | `float` | `0.1` | Exponential moving average filter factor (0.0–0.85). |
| `black_threshold` | `int` | `15` | Pixel intensity cutoff below which LEDs turn completely off (`(0, 0, 0)`). |
| `edge_depth` | `float \| int` | `0.08` | Border sampling depth as relative ratio (`0.02`–`0.50`) or pixel distance. |
| `resolution` | `str` | `"Auto (Adaptiv)"` | Downscaling mode: `"Auto (Adaptiv)"` or static presets. |
| `oversample_factor` | `float` | `2.0` | Nyquist-Shannon oversampling factor for adaptive resolution (1.0x–8.0x). |
| `eco_mode` | `bool` | `true` | Drops CPU usage on static screens by throttling redundant frames. |
| `start_minimized` | `bool` | `false` | Start silently in system tray without opening window. |
| `monitors` | `list` | *See below* | Per-monitor LED counts, edge order, and direction mappings. |

---

## 🛠️ Quick Start & Installation

### Prerequisites

- **Python 3.10** or newer
- **Pip** package manager
- Any device running **WLED** (v0.12+) connected to your local network

### Setup Instructions

1. **Clone the repository:**
   ```bash
   git clone https://github.com/ghostside-net/GloWled.git
   cd GloWled
   ```

2. **Install dependencies:**
   ```bash
   pip install -r requirements.txt
   ```

3. **Launch GloWled:**
   ```bash
   python main.py
   ```
   *Tip:* You can also run GloWled in background headless streaming mode via:
   ```bash
   python main.py --headless
   ```

---

## 💡 2-Minute WLED Configuration

GloWled communicates using WLED's ultra-low-latency realtime **UDP (DRGB)** protocol:

1. Open your WLED web interface (e.g. `http://4.3.2.1` or your WLED IP) in any browser.
2. Navigate to **Config** ➔ **Sync Interfaces**.
3. Under **Realtime**, make sure:
   - **Receive:** `Live data` is checked.
   - **UDP Port:** `21324` (default).
4. Enter your WLED IP into GloWled, click **Ping testen** to verify communication, and hit **STARTEN & ÜBERNEHMEN**! 🎉

---

## 📦 Building a Standalone Windows Executable

If you want a portable single `.exe` file that runs without a Python installation:

```bash
pip install pyinstaller
pyinstaller --noconsole --onefile --icon=icon.ico --add-data "icon.ico;." --name=GloWled main.py
```

The compiled binary will be located in the `dist/` directory (`dist/GloWled.exe`).

---

## 🧪 Testing & Code Quality

GloWled comes with an automated test suite covering configuration persistence, backward compatibility, edge cropping, SIMD color processing, adaptive resolution, black level handling, and thread safety:

```bash
# Run pytest test suite
pytest -v

# Run linting and code formatting validation
ruff check .

# Run static type checking
mypy .
```

---

## 📝 Changelog

### v1.2.0
- **Adaptive Sampling Resolution:** Dynamic resolution scaling based on Nyquist-Shannon sampling theorem (`oversample_factor`), with automatic fallbacks for 0 or outlier LED counts.
- **Configurable Edge Depth & ROIs:** Support for relative percentages (0.0 to 0.5) and absolute pixel distances with overlap prevention and minimum 1-pixel clamping.
- **True Black Level Handling:** Fixed black screen standby glow; introduced `black_threshold` with Hermite smoothstep fade-out transfer curve and sanitized 4-channel BGRA/RGBA alpha handling.
- **Modern Packaging & Tooling:** Added `pyproject.toml` with strict Ruff and Mypy integration.
- **Expanded Test Suite:** Added 13 new unit and integration tests covering resolution adaptation, ROI calculation, black level cutoff, and alpha channel sanitization.

### v1.1.0
- Modern CustomTkinter UI with dark mode theme.
- Exponential color smoothing transition filter.
- Intelligent Eco Mode for sub-1% CPU usage during static screen states.
- System tray background minimization and silent autostart.

---

## 📄 License

This project is licensed under the **GNU General Public License v3 (GPLv3)**.  
You are free to use, modify, and redistribute this software. See the [LICENSE](LICENSE) file for the full license text.
