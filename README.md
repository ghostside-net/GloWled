# GloWled

[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
![Platform: Windows](https://img.shields.io/badge/Platform-Windows-blue.svg)

A lightweight Windows Ambilight tool for WLED.

GloWled ist ein extrem ressourcenschonendes Python-Programm, das den Bildschirminhalt in Echtzeit ausliest und ohne spürbare Verzögerung an deine WLED-Geräte überträgt. Der Fokus liegt auf maximaler Performance bei minimalem CPU- und RAM-Verbrauch. Nach dem Start läuft die Anwendung unauffällig im System Tray (Hintergrund), damit deine Hardware voll und ganz für Games oder andere Anwendungen zur Verfügung steht.

---

## 🚀 Features

- **Echtzeit-Synchronisation:** Nahezu latenzfreie Übertragung des Bildschirminhalts an WLED.
- **Multi-Monitor-Support:** Volle Unterstützung für Setups mit mehreren Bildschirmen.
- **System-Tray-Integration:** Läuft unauffällig im Hintergrund und hält den Desktop frei.
- **Ultra-Lightweight:** Effiziente capture-Algorithmen für minimale Systemauslastung.

## ⚠️ Aktuelle Einschränkungen (Limitations)

- **Aktuell nur Windows:** Da das Tool für maximale Performance teils spezifische Windows-APIs nutzt, läuft es derzeit ausschließlich unter Windows. Ein Support für Linux und macOS ist jedoch für die Zukunft angedacht.

---

## 🛠️ Installation & Starten (Development Setup)

Da das Projekt in Python geschrieben ist, kannst du es ganz einfach aus dem Quellcode ausführen.

### Voraussetzungen

Stelle sicher, dass du folgende Tools installiert hast:
- **Python 3.10** (oder neuer)
- Das Python-Paketverwaltungstool `pip`

### Anleitung

1. **Repository klonen:**
   Öffne dein Terminal und führe den Befehl aus dem Abschnitt
   ```bash
   git clone https://github.com/ghostside-net/GloWled
   cd GloWled
   ```
   aus.

3. **Virtuelle Umgebung erstellen (Empfohlen):**
   Erstelle und aktiviere eine virtuelle Umgebung mit den Befehlen
   ```bash
   python -m venv venv
   venv\Scripts\activate
   ```
   aus, um deine globalen Python-Pakete sauber zu halten.

5. **Abhängigkeiten installieren:**
   Führe den Befehl
   ```bash
   pip install -r requirements.txt
   ```
   aus, um alle benötigten Python-Bibliotheken zu installieren.

7. **Anwendung starten:**
   Starte GloWled mit dem Befehl
   ```bash
   python main.py
   ```
   aus.

---

## 🔮 Kommende Features & Roadmap

Das Projekt befindet sich in aktiver Entwicklung. Folgende Verbesserungen und Funktionen sind für zukünftige Updates geplant:

- [ ] **Erweiterte Einstellungen:** Mehr Optionen zur Feineinstellung der Zonen, Helligkeit und Farbanpassung direkt über die UI.
- [ ] **Cross-Platform Support:** Portierung der Capture-Logik auf Linux und eventuell macOS.
- [ ] **Autostart-Option:** Bequemes Aktivieren des Windows-Autostarts über die Benutzeroberfläche.

*Beiträge, Bugfixes und Feature-Requests sind jederzeit willkommen! Erstelle dazu einfach ein Issue oder einen Pull Request.*

---

## 📄 Lizenz

Dieses Projekt ist unter der **GNU General Public License v3.0 (GPL-3.0)** lizenziert. Das bedeutet, dass jeder den Code frei verwenden, modifizieren und weiterverbreiten darf – solange alle abgeleiteten Werke ebenfalls unter derselben Lizenz stehen und der Quellcode offengelegt wird. Details findest du in der [LICENSE](LICENSE)-Datei.
