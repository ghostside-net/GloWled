# GloWled

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
![Platform: Windows](https://img.shields.io/badge/Platform-Windows-blue.svg)
![Language: Python](https://img.shields.io/badge/Language-Python-yellow.svg)

A lightweight Windows Ambilight tool for WLED.

GloWled is an ultra-lightweight Windows application that captures your screen content in real-time and transmits it instantly to your WLED devices. Designed with a focus on maximum performance and minimal CPU/RAM footprint, it ensures your system resources remain fully available for gaming or other demanding tasks. 

The application features a simple user interface but is built to stay out of your way, running quietly in the system tray.

---

## 🚀 Features

- **Real-Time Sync:** Virtually latency-free screen capturing and WLED streaming.
- **Multi-Monitor Support:** Ready out of the box for multi-display setups.
- **Ultra-Lightweight:** Highly optimized Python code to prevent background stuttering.
- **System Tray Integration:** Runs quietly in the background without cluttering your taskbar.

## ⚠️ Current Limitations & Future Platform Support

- **Windows Only (For Now):** The tool currently relies on specific Windows APIs for high-performance screen capturing. 
- **Future Roadmap:** Expansion to **Linux** and potentially **macOS** is planned for future releases once cross-platform capturing backends are integrated.

---

## 🛠️ Installation & How to Run

Since GloWled is open-source, you can easily run it from source or build it yourself.

### Prerequisites

Make sure you have the following installed on your system:
- **Python 3.10** (or newer)
- **Pip** (Python package manager)

### Setup Instructions

1. **Clone the Repository:**
   Open your terminal/command prompt and run the command
   ```bash
   git clone https://github.com/ghostside-net/GloWled
   cd GloWled
   ```

3. **Install Dependencies:**
   Navigate into the project directory and install the required Python packages using
   ```bash
   pip install -r requirements.txt
   ```

5. **Run the Application:**
   Start GloWled by executing the main script via
   ```bash
   python main.py
   ```

---

## 🔮 Upcoming Features & Roadmap

GloWled is under active development. The following features and improvements are planned for upcoming releases:

- [ ] **Advanced Configuration UI:** Expanded settings for fine-tuning LED zones, color calibration, and brightness.
- [ ] **Cross-Platform Support:** Bringing the tool to Linux and macOS users.
- [ ] **Performance Tweaks:** Ongoing optimizations to the screen-capture algorithms.

*Contributions, bug reports, and feature requests are highly welcome! Feel free to open an issue or submit a pull request.*

---

## 📄 License

This project is licensed under the **GNU GPLv3 License**. This ensures that the software remains free and open-source. Anyone who modifies or redistributes this code must also make their source code publicly available under the same license. See the [LICENSE](LICENSE) file for details.
