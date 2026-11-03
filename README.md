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
- 🍃 **Intelligent Eco Mode:** Dynamically detects static screens (reading, paused media, desktop idling) and throttles redundant processing, dropping CPU load to nearly **0.1%**!
- 🎨 **Cinematic Color & Smoothing:**
  - **Color Smoothing:** Adjustable exponential transition filter (0–85%) prevents jarring flicker.
  - **Saturation & Brightness Multipliers:** Give your room that extra vibrant punch or dim down for nighttime sessions.
  - **Edge Sampling Depth:** Customize margin crop (4–25%) to ignore letterbox black bars in movies.
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
- **🎨 Bild & Effekte:** Live brightness, saturation boost, color smoothing, and border depth calibration.
- **🖥️ Monitore:** Card-based multi-display management, per-side LED counters, total LED tally, and edge ordering.
- **ℹ️ Info & Hilfe:** Built-in connection guide and shortcuts.

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

## 🧪 Testing

GloWled comes with a complete automated test suite covering configuration persistence, backward compatibility, edge cropping, SIMD color processing, and thread safety:

```bash
pytest -v
```

---

## 🔮 Roadmap

- [x] **Advanced Configuration UI:** Fine-tuning LED counts, interactive edge ordering, and color calibration.
- [x] **Performance Optimization:** Zero-copy SIMD processing, intelligent static-screen detection, and sub-1% CPU usage.
- [x] **Cross-Platform Engine:** Graceful fallback capture backend (MSS) alongside DirectX (DXCam).
- [ ] **Audio Reactive Sync:** Optional sound-reactive ambient glow modes.
- [ ] **Zone Segment Mapping:** Advanced matrix and multiple strip segment assignment.

*Feedback, bug reports, and pull requests are warmly welcomed! Feel free to open an issue or contribute.*

---

## 📄 License

This project is licensed under the **GNU General Public License v3 (GPLv3)**.  
You are free to use, modify, and redistribute this software. See the [LICENSE](LICENSE) file for the full license text.
