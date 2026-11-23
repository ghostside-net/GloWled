//! Lightweight, type-safe internationalization (DE/EN) for GloWled.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Supported UI & system-tray languages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    German,
    English,
}

impl Language {
    /// Returns the canonical ISO-639-1 code (`"de"` or `"en"`).
    pub const fn code(self) -> &'static str {
        match self {
            Self::German => "de",
            Self::English => "en",
        }
    }

    /// Maps the language to a UI selector index (`0` = German, `1` = English).
    pub const fn to_index(self) -> i32 {
        match self {
            Self::German => 0,
            Self::English => 1,
        }
    }

    /// Maps a UI selector index back to `Language`, falling back to `German` on invalid indices.
    pub const fn from_index(idx: i32) -> Self {
        match idx {
            1 => Self::English,
            _ => Self::German,
        }
    }

    /// Parses arbitrary strings lossily, defaulting to `Language::German` for unknown values.
    pub fn parse_lossy(raw: &str) -> Self {
        let norm = raw.trim().to_ascii_lowercase();
        if norm == "en"
            || norm == "english"
            || norm == "englisch"
            || norm.starts_with("en-")
            || norm.starts_with("en_")
        {
            Self::English
        } else {
            Self::German
        }
    }

    /// Returns the static translation table for this language.
    pub const fn translations(self) -> &'static Translations {
        match self {
            Self::German => &DE,
            Self::English => &EN,
        }
    }
}

impl Serialize for Language {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Language {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct LossyLanguageVisitor;

        impl<'de> Visitor<'de> for LossyLanguageVisitor {
            type Value = Language;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a language code string such as \"de\" or \"en\"")
            }

            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
                Ok(Language::parse_lossy(v))
            }

            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Self::Value, E> {
                Ok(Language::parse_lossy(&v))
            }

            fn visit_bool<E: de::Error>(self, _: bool) -> std::result::Result<Self::Value, E> {
                Ok(Language::default())
            }

            fn visit_i64<E: de::Error>(self, _: i64) -> std::result::Result<Self::Value, E> {
                Ok(Language::default())
            }

            fn visit_u64<E: de::Error>(self, _: u64) -> std::result::Result<Self::Value, E> {
                Ok(Language::default())
            }

            fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Self::Value, E> {
                Ok(Language::default())
            }

            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(Language::default())
            }

            fn visit_none<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(Language::default())
            }

            fn visit_some<D2: Deserializer<'de>>(
                self,
                deserializer: D2,
            ) -> std::result::Result<Self::Value, D2::Error> {
                Language::deserialize(deserializer)
            }

            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(Language::default())
            }

            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                while map
                    .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                    .is_some()
                {}
                Ok(Language::default())
            }
        }

        deserializer.deserialize_any(LossyLanguageVisitor)
    }
}

/// Complete set of localized UI, tray, and runtime status strings.
#[derive(Clone, Copy, Debug)]
pub struct Translations {
    pub lang: Language,

    // Window titles & common status
    pub window_title: &'static str,
    pub status_active: &'static str,
    pub status_paused: &'static str,
    pub live_ready: &'static str,
    pub language_changed: &'static str,

    // Navigation tabs
    pub tab_connection: &'static str,
    pub tab_monitors: &'static str,
    pub tab_image: &'static str,
    pub tab_system: &'static str,

    // Tab 0: Connection
    pub section_wled: &'static str,
    pub label_wled_ip: &'static str,
    pub placeholder_wled_ip: &'static str,
    pub label_udp_port: &'static str,
    pub label_protocol: &'static str,
    pub protocol_options: [&'static str; 3],
    pub btn_test_connection: &'static str,

    // Tab 1: Monitors & LEDs
    pub section_detected_monitors: &'static str,
    pub btn_detect: &'static str,
    pub badge_added: &'static str,
    pub btn_add: &'static str,
    pub btn_remove: &'static str,
    pub label_top: &'static str,
    pub label_bottom: &'static str,
    pub label_left: &'static str,
    pub label_right: &'static str,
    pub label_start_corner: &'static str,
    pub corner_options: [&'static str; 4],
    pub label_direction: &'static str,
    pub dir_cw: &'static str,
    pub dir_ccw: &'static str,
    pub summary_monitors_prefix: &'static str,
    pub summary_total_leds_mid: &'static str,
    pub leds_unit: &'static str,

    // Tab 2: Image & Color
    pub section_sampling_fps: &'static str,
    pub label_resolution: &'static str,
    pub resolution_options: [&'static str; 5],
    pub label_fps_limit: &'static str,
    pub section_color_gamma: &'static str,
    pub label_brightness: &'static str,
    pub label_saturation: &'static str,
    pub label_gamma: &'static str,
    pub label_smoothing: &'static str,
    pub label_black_threshold: &'static str,
    pub check_black_bar: &'static str,
    pub section_edge_strips: &'static str,
    pub edge_strips_hint: &'static str,
    pub edge_depth_label: &'static str,
    pub edge_offset_label: &'static str,
    pub side_top: &'static str,
    pub side_bottom: &'static str,
    pub side_left: &'static str,
    pub side_right: &'static str,

    // Tab 3: System
    pub section_operation: &'static str,
    pub label_language: &'static str,
    pub check_ambilight_active: &'static str,
    pub check_eco_mode: &'static str,
    pub check_autostart: &'static str,
    pub check_start_minimized: &'static str,
    pub check_updates_startup: &'static str,
    pub section_updates: &'static str,
    pub label_installed_version: &'static str,
    pub btn_check_updates: &'static str,
    pub btn_checking: &'static str,
    pub update_not_checked: &'static str,

    // Bottom bar
    pub btn_hide_to_tray: &'static str,
    pub btn_apply_save: &'static str,

    // Update dialog
    pub update_dialog_title: &'static str,
    pub update_heading: &'static str,
    pub update_section_version: &'static str,
    pub update_current_prefix: &'static str,
    pub update_new_prefix: &'static str,
    pub update_package_prefix: &'static str,
    pub update_section_notes: &'static str,
    pub update_confirm_prompt: &'static str,
    pub btn_later: &'static str,
    pub btn_update_now: &'static str,
    pub btn_installing: &'static str,

    // System tray
    pub tray_open_settings: &'static str,
    pub tray_check_updates: &'static str,
    pub tray_quit: &'static str,
    pub tray_status_active: &'static str,
    pub tray_status_paused: &'static str,
    pub tray_tooltip_active: &'static str,
    pub tray_tooltip_paused: &'static str,

    // Runtime messages
    pub autostart_enabled: &'static str,
    pub autostart_disabled: &'static str,
    pub update_preparing: &'static str,
    pub update_already_running: &'static str,
    pub update_checking_github: &'static str,
    pub update_check_already_running: &'static str,
    pub update_already_latest: &'static str,
    pub worker_streaming_paused: &'static str,
}

impl Translations {
    pub fn monitor_not_detected(&self, mss_index: usize) -> String {
        match self.lang {
            Language::German => format!("Monitor {mss_index} (nicht erkannt)"),
            Language::English => format!("Monitor {mss_index} (not detected)"),
        }
    }

    pub fn wled_probe_ok(&self, proto: &str, ip: &str, port: u16, ms: f64) -> String {
        match self.lang {
            Language::German => format!("✓ {proto} Probe an {ip}:{port} gesendet ({ms:.2} ms)"),
            Language::English => format!("✓ {proto} probe sent to {ip}:{port} ({ms:.2} ms)"),
        }
    }

    pub fn monitors_detected(&self, count: usize) -> String {
        match self.lang {
            Language::German => format!("{count} Monitor(e) erkannt"),
            Language::English => format!("{count} monitor(s) detected"),
        }
    }

    pub fn monitor_added(&self, idx: usize) -> String {
        match self.lang {
            Language::German => format!("Monitor {idx} eingebunden (ungespeichert)"),
            Language::English => format!("Monitor {idx} added (unsaved)"),
        }
    }

    pub fn monitor_removed(&self, idx: usize) -> String {
        match self.lang {
            Language::German => format!("Monitor {idx} entfernt (ungespeichert)"),
            Language::English => format!("Monitor {idx} removed (unsaved)"),
        }
    }

    pub fn autostart_error(&self, err: &dyn fmt::Display) -> String {
        match self.lang {
            Language::German => format!("Autostart-Fehler: {err}"),
            Language::English => format!("Autostart error: {err}"),
        }
    }

    pub fn settings_saved(&self, monitors: usize, ip: &str, port: u16, proto: &str) -> String {
        match self.lang {
            Language::German => {
                format!("Gespeichert • {monitors} Monitor(e) • {ip}:{port} ({proto})")
            }
            Language::English => format!("Saved • {monitors} monitor(s) • {ip}:{port} ({proto})"),
        }
    }

    pub fn save_error(&self, err: &dyn fmt::Display) -> String {
        match self.lang {
            Language::German => format!("Speicherfehler: {err}"),
            Language::English => format!("Save error: {err}"),
        }
    }

    pub fn update_up_to_date(&self, current: &str, latest: &str) -> String {
        match self.lang {
            Language::German => format!("Aktuell: v{current} (neuestes Release: v{latest})"),
            Language::English => format!("Up to date: v{current} (latest release: v{latest})"),
        }
    }

    pub fn update_available(&self, new_ver: &str) -> String {
        match self.lang {
            Language::German => format!("Update verfügbar: v{new_ver}"),
            Language::English => format!("Update available: v{new_ver}"),
        }
    }

    pub fn update_check_failed(&self, err: &str) -> String {
        match self.lang {
            Language::German => format!("Update-Prüfung fehlgeschlagen: {err}"),
            Language::English => format!("Update check failed: {err}"),
        }
    }

    pub fn update_install_failed_dialog(&self, err: &str) -> String {
        match self.lang {
            Language::German => {
                format!("Update fehlgeschlagen (vorherige Version bleibt aktiv): {err}")
            }
            Language::English => format!("Update failed (previous version kept active): {err}"),
        }
    }

    pub fn update_install_failed_short(&self, err: &str) -> String {
        match self.lang {
            Language::German => format!("Update-Fehler: {err}"),
            Language::English => format!("Update error: {err}"),
        }
    }

    pub fn update_restart_failed(&self, new_ver: &str, err: &dyn fmt::Display) -> String {
        match self.lang {
            Language::German => {
                format!("Update auf v{new_ver} installiert, Neustart fehlgeschlagen: {err}")
            }
            Language::English => format!("Updated to v{new_ver}, restart failed: {err}"),
        }
    }

    pub fn worker_notice(&self, err: &str) -> String {
        match self.lang {
            Language::German => format!("Hinweis: {err}"),
            Language::English => format!("Notice: {err}"),
        }
    }

    pub fn worker_streaming_fps(&self, fps: f32) -> String {
        match self.lang {
            Language::German => format!("Streaming aktiv • {fps:.1} FPS"),
            Language::English => format!("Streaming active • {fps:.1} FPS"),
        }
    }

    pub fn localize_update_stage(&self, stage: &str) -> String {
        match self.lang {
            Language::German => stage.to_string(),
            Language::English => {
                if let Some(rest) = stage
                    .strip_prefix("Lade ")
                    .and_then(|s| s.strip_suffix(" herunter..."))
                {
                    format!("Downloading {rest}...")
                } else if stage == "Prüfe SHA-256-Prüfsumme..." {
                    "Verifying SHA-256 checksum...".to_string()
                } else if stage == "Entpacke Update-Archiv..." {
                    "Extracting update archive...".to_string()
                } else if stage == "Installiere neue Version..." {
                    "Installing new version...".to_string()
                } else {
                    stage.to_string()
                }
            }
        }
    }
}

/// German translation table (default).
pub const DE: Translations = Translations {
    lang: Language::German,

    window_title: "GloWled — Ambilight Controller",
    status_active: "AKTIV",
    status_paused: "PAUSIERT",
    live_ready: "Bereit",
    language_changed: "Sprache: Deutsch",

    tab_connection: "1. Verbindung",
    tab_monitors: "2. Monitore & LEDs",
    tab_image: "3. Bild & Farbe",
    tab_system: "4. System",

    section_wled: "WLED-Controller",
    label_wled_ip: "WLED IP-Adresse:",
    placeholder_wled_ip: "z. B. 192.168.1.50",
    label_udp_port: "UDP Port:",
    label_protocol: "UDP-Protokoll:",
    protocol_options: ["DRGB (Standard)", "DDP", "WARLS"],
    btn_test_connection: "Verbindung testen",

    section_detected_monitors: "Erkannte Monitore",
    btn_detect: "Erkennen",
    badge_added: "✓ Eingebunden",
    btn_add: "+ Einbinden",
    btn_remove: "Entfernen",
    label_top: "Oben:",
    label_bottom: "Unten:",
    label_left: "Links:",
    label_right: "Rechts:",
    label_start_corner: "Start-Ecke:",
    corner_options: ["Unten Links", "Oben Links", "Oben Rechts", "Unten Rechts"],
    label_direction: "Ausrichtung:",
    dir_cw: "Uhrzeigersinn (CW)",
    dir_ccw: "Gegenuhrzeigersinn (CCW)",
    summary_monitors_prefix: "Monitore: ",
    summary_total_leds_mid: " • Gesamt: ",
    leds_unit: "LEDs",

    section_sampling_fps: "Abtastung & Bildrate",
    label_resolution: "Auflösung:",
    resolution_options: [
        "Auto (Adaptiv)",
        "Performance (64x36)",
        "Balance (128x72)",
        "Quality (256x144)",
        "Ultra (512x288)",
    ],
    label_fps_limit: "FPS-Limit: ",
    section_color_gamma: "Farbe, Gamma & Kinobalken",
    label_brightness: "Helligkeit: ",
    label_saturation: "Sättigung: ",
    label_gamma: "Gamma: ",
    label_smoothing: "Glättung: ",
    label_black_threshold: "Schwarz-Schwelle: ",
    check_black_bar: "Kinobalken automatisch erkennen",
    section_edge_strips: "Abtast-Streifen",
    edge_strips_hint: "Breite & Randabstand pro Seite",
    edge_depth_label: "Breite: ",
    edge_offset_label: "Abstand: ",
    side_top: "Oben",
    side_bottom: "Unten",
    side_left: "Links",
    side_right: "Rechts",

    section_operation: "Betrieb & System-Tray",
    label_language: "Sprache:",
    check_ambilight_active: "Ambilight aktiv",
    check_eco_mode: "Eco-Modus (Standbild-Pause)",
    check_autostart: "Mit Windows starten",
    check_start_minimized: "Minimiert im Tray starten",
    check_updates_startup: "Beim Start nach Updates suchen",
    section_updates: "Updates",
    label_installed_version: "Installierte Version: v",
    btn_check_updates: "Nach Updates suchen",
    btn_checking: "Prüfe...",
    update_not_checked: "Noch nicht geprüft",

    btn_hide_to_tray: "In Tray ausblenden",
    btn_apply_save: "Übernehmen & Speichern",

    update_dialog_title: "GloWled — Update verfügbar",
    update_heading: "Neue Version verfügbar",
    update_section_version: "Version",
    update_current_prefix: "Aktuell: v",
    update_new_prefix: "   →   Neu: v",
    update_package_prefix: "Paket: ",
    update_section_notes: "Release-Notes",
    update_confirm_prompt: "Update jetzt herunterladen und installieren?",
    btn_later: "Später",
    btn_update_now: "Jetzt aktualisieren",
    btn_installing: "Installiere...",

    tray_open_settings: "Einstellungen öffnen",
    tray_check_updates: "Nach Updates suchen",
    tray_quit: "Beenden",
    tray_status_active: "Status: Aktiv (Klicken zum Pausieren)",
    tray_status_paused: "Status: Pausiert (Klicken zum Aktivieren)",
    tray_tooltip_active: "GloWled — Aktiv",
    tray_tooltip_paused: "GloWled — Pausiert",

    autostart_enabled: "Autostart aktiviert",
    autostart_disabled: "Autostart deaktiviert",
    update_preparing: "Update wird vorbereitet...",
    update_already_running: "Ein Update-Vorgang läuft bereits.",
    update_checking_github: "Suche nach Updates auf GitHub...",
    update_check_already_running: "Update-Prüfung läuft bereits.",
    update_already_latest: "GloWled ist bereits auf dem neuesten Stand.",
    worker_streaming_paused: "Streaming pausiert",
};

/// English translation table.
pub const EN: Translations = Translations {
    lang: Language::English,

    window_title: "GloWled — Ambilight Controller",
    status_active: "ACTIVE",
    status_paused: "PAUSED",
    live_ready: "Ready",
    language_changed: "Language: English",

    tab_connection: "1. Connection",
    tab_monitors: "2. Monitors & LEDs",
    tab_image: "3. Image & Color",
    tab_system: "4. System",

    section_wled: "WLED Controller",
    label_wled_ip: "WLED IP Address:",
    placeholder_wled_ip: "e.g. 192.168.1.50",
    label_udp_port: "UDP Port:",
    label_protocol: "UDP Protocol:",
    protocol_options: ["DRGB (Default)", "DDP", "WARLS"],
    btn_test_connection: "Test Connection",

    section_detected_monitors: "Detected Monitors",
    btn_detect: "Detect",
    badge_added: "✓ Added",
    btn_add: "+ Add",
    btn_remove: "Remove",
    label_top: "Top:",
    label_bottom: "Bottom:",
    label_left: "Left:",
    label_right: "Right:",
    label_start_corner: "Start Corner:",
    corner_options: ["Bottom Left", "Top Left", "Top Right", "Bottom Right"],
    label_direction: "Direction:",
    dir_cw: "Clockwise (CW)",
    dir_ccw: "Counter-clockwise (CCW)",
    summary_monitors_prefix: "Monitors: ",
    summary_total_leds_mid: " • Total: ",
    leds_unit: "LEDs",

    section_sampling_fps: "Sampling & Frame Rate",
    label_resolution: "Resolution:",
    resolution_options: [
        "Auto (Adaptive)",
        "Performance (64x36)",
        "Balanced (128x72)",
        "Quality (256x144)",
        "Ultra (512x288)",
    ],
    label_fps_limit: "FPS Limit: ",
    section_color_gamma: "Color, Gamma & Black Bars",
    label_brightness: "Brightness: ",
    label_saturation: "Saturation: ",
    label_gamma: "Gamma: ",
    label_smoothing: "Smoothing: ",
    label_black_threshold: "Black Threshold: ",
    check_black_bar: "Auto-detect black bars",
    section_edge_strips: "Sampling Strips",
    edge_strips_hint: "Strip width & border offset per side",
    edge_depth_label: "Width: ",
    edge_offset_label: "Offset: ",
    side_top: "Top",
    side_bottom: "Bottom",
    side_left: "Left",
    side_right: "Right",

    section_operation: "Operation & System Tray",
    label_language: "Language:",
    check_ambilight_active: "Ambilight active",
    check_eco_mode: "Eco Mode (pause on static image)",
    check_autostart: "Start with Windows",
    check_start_minimized: "Start minimized to tray",
    check_updates_startup: "Check for updates on startup",
    section_updates: "Updates",
    label_installed_version: "Installed Version: v",
    btn_check_updates: "Check for Updates",
    btn_checking: "Checking...",
    update_not_checked: "Not checked yet",

    btn_hide_to_tray: "Hide to Tray",
    btn_apply_save: "Apply & Save",

    update_dialog_title: "GloWled — Update Available",
    update_heading: "New Version Available",
    update_section_version: "Version",
    update_current_prefix: "Current: v",
    update_new_prefix: "   →   New: v",
    update_package_prefix: "Package: ",
    update_section_notes: "Release Notes",
    update_confirm_prompt: "Download and install this update now?",
    btn_later: "Later",
    btn_update_now: "Update Now",
    btn_installing: "Installing...",

    tray_open_settings: "Open Settings",
    tray_check_updates: "Check for Updates",
    tray_quit: "Quit",
    tray_status_active: "Status: Active (Click to Pause)",
    tray_status_paused: "Status: Paused (Click to Resume)",
    tray_tooltip_active: "GloWled — Active",
    tray_tooltip_paused: "GloWled — Paused",

    autostart_enabled: "Autostart enabled",
    autostart_disabled: "Autostart disabled",
    update_preparing: "Preparing update...",
    update_already_running: "An update is already in progress.",
    update_checking_github: "Checking GitHub for updates...",
    update_check_already_running: "Update check already in progress.",
    update_already_latest: "GloWled is already up to date.",
    worker_streaming_paused: "Streaming paused",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_codes_and_indices_roundtrip() {
        assert_eq!(Language::German.code(), "de");
        assert_eq!(Language::English.code(), "en");
        assert_eq!(Language::German.to_index(), 0);
        assert_eq!(Language::English.to_index(), 1);
        assert_eq!(Language::from_index(0), Language::German);
        assert_eq!(Language::from_index(1), Language::English);
        assert_eq!(Language::from_index(-1), Language::German);
        assert_eq!(Language::from_index(99), Language::German);
    }

    #[test]
    fn lossy_parser_handles_variants_and_falls_back_to_german() {
        assert_eq!(Language::parse_lossy("en"), Language::English);
        assert_eq!(Language::parse_lossy("EN-US"), Language::English);
        assert_eq!(Language::parse_lossy("en_GB"), Language::English);
        assert_eq!(Language::parse_lossy(" English "), Language::English);
        assert_eq!(Language::parse_lossy("englisch"), Language::English);

        assert_eq!(Language::parse_lossy("de"), Language::German);
        assert_eq!(Language::parse_lossy("de-DE"), Language::German);
        assert_eq!(Language::parse_lossy("fr"), Language::German);
        assert_eq!(Language::parse_lossy(""), Language::German);
    }

    #[test]
    fn serde_roundtrip_and_invalid_values_fallback_cleanly() {
        assert_eq!(serde_json::to_string(&Language::German).unwrap(), "\"de\"");
        assert_eq!(serde_json::to_string(&Language::English).unwrap(), "\"en\"");

        assert_eq!(
            serde_json::from_str::<Language>("\"en\"").unwrap(),
            Language::English
        );
        assert_eq!(
            serde_json::from_str::<Language>("\"de\"").unwrap(),
            Language::German
        );

        for raw in ["\"fr\"", "\"\"", "123", "true", "null", "[]", "{}"] {
            let parsed: Language = serde_json::from_str(raw).unwrap();
            assert_eq!(parsed, Language::German, "failed for {raw}");
        }
    }

    #[test]
    fn translations_have_non_empty_strings_and_distinct_formatting() {
        for t in [&DE, &EN] {
            assert!(!t.tab_connection.is_empty());
            assert!(!t.btn_apply_save.is_empty());
            assert!(!t.tray_open_settings.is_empty());
            assert!(!t.update_dialog_title.is_empty());
            assert!(!t.monitor_not_detected(2).is_empty());
            assert!(!t.wled_probe_ok("DRGB", "127.0.0.1", 21324, 0.42).is_empty());
            assert!(!t.settings_saved(1, "127.0.0.1", 21324, "DRGB").is_empty());
            assert!(!t.worker_streaming_fps(59.9).is_empty());
        }

        assert_ne!(DE.tab_connection, EN.tab_connection);
        assert_ne!(DE.btn_apply_save, EN.btn_apply_save);
        assert_ne!(DE.tray_quit, EN.tray_quit);
        assert_eq!(
            EN.localize_update_stage("Prüfe SHA-256-Prüfsumme..."),
            "Verifying SHA-256 checksum..."
        );
        assert_eq!(
            EN.localize_update_stage("Lade glowled.exe herunter..."),
            "Downloading glowled.exe..."
        );
    }
}
