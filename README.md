# GloWled

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
![Platform: Windows](https://img.shields.io/badge/Platform-Windows-blue.svg)

A lightweight Windows Ambilight tool for WLED.

GloWled ist ein extrem ressourcenschonendes Windows-Programm, das den Bildschirminhalt in Echtzeit ausliest und ohne spürbare Verzögerung an deine WLED-Geräte überträgt. Der Fokus liegt auf maximaler Performance bei minimalem CPU- und RAM-Verbrauch, damit deine Hardware voll und ganz für Games oder andere Anwendungen zur Verfügung steht.

---

## 🚀 Features

- **Echtzeit-Synchronisation:** Nahezu latenzfreie Übertragung des Bildschirminhalts an WLED.
- **Ultra-Lightweight:** Optimierter Code für minimale Systemauslastung im Hintergrund.
- **Simpel & Fokussiert:** Keine überladene UI, sondern pure Funktionalität.

## ⚠️ Aktuelle Einschränkungen (Limitations)

- **Nur Windows:** Das Tool nutzt spezifische Windows-APIs (z.B. zur Bildschirmaufnahme), weshalb es derzeit **ausschließlich unter Windows** läuft.
- **Single-Monitor:** Aktuell wird primär der Hauptbildschirm unterstützt.

---

## 🛠️ Installation & Build-Prozess

Da das Projekt Open-Source ist, kannst du dir die Anwendung ganz einfach selbst aus dem Quellcode bauen.

### Voraussetzungen

Stelle sicher, dass du folgende Tools installiert hast:
- Einen C++ Compiler (z.B. MSVC über Visual Studio Build Tools oder GCC/MinGW)
- **CMake** (Version 3.20 oder neuer)
- *Optional:* Eine passende Entwicklungsumgebung wie die IDEs von JetBrains (CLion) oder Visual Studio.

### Build-Anleitung

1. **Repository klonen:**
   Öffne dein Terminal und führe den Befehl
   ```bash
   git clone https://github.com/ghostside-net/GloWled
   cd GloWled
   ```
   aus.

3. **Build-Verzeichnis erstellen:**
   Wechsle in den Projektordner und erstelle ein neues Build-Verzeichnis mit den Befehlen
   ```bash
   mkdir build
   cd build
   ```

4. **CMake konfigurieren und Projekt bauen:**
   Führe abschließend die CMake-Befehle
   ```bash
   cmake ..
   cmake --build . --config Release
   ```
   aus, um die Anwendung zu kompilieren.

Nach erfolgreichem Build findest du die ausführbare Datei (`GloWled.exe`) im Ordner `build/Release/`.

---

## 🔮 Kommende Features & Roadmap

Das Projekt befindet sich in aktiver Entwicklung. Folgende Verbesserungen und Funktionen sind für zukünftige Updates geplant:

- [ ] **Multi-Monitor-Support:** Auswahlmöglichkeit, welcher Bildschirm ausgelesen werden soll.
- [ ] **Erweiterte Zone-Configurations:** Feingranularere Aufteilung der LEDs für eine noch präzisere Farbdarstellung.
- [ ] **Autostart & System-Tray:** Option, das Tool minimiert im Windows-System-Tray zu starten.
- [ ] **Performance-Tweaks:** Kontinuierliche Optimierung der Capture-Algorithmen.

*Beiträge, Bugfixes und Feature-Requests sind jederzeit willkommen! Erstelle dazu einfach ein Issue oder einen Pull Request.*

---

## 📄 Lizenz

Dieses Projekt ist unter der **MIT-Lizenz** lizenziert. Das bedeutet, du kannst den Code frei verwenden, modifizieren und teilen, solange die ursprüngliche Urheberrechtserklärung erhalten bleibt. Details findest du in der [LICENSE](LICENSE)-Datei.
