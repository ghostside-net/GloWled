//! Configuration management, JSON persistence, and Windows Registry autostart.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{GlowledError, Result};

/// Supported WLED realtime UDP streaming protocols.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WledProtocol {
    /// WLED DRGB protocol: `[2, timeout_sec, R, G, B, ...]` (default in Python GloWled).
    #[default]
    Drgb,
    /// Distributed Display Protocol (DDP): 10-byte header + raw RGB payload.
    Ddp,
    /// WLED WARLS protocol: `[1, timeout_sec, idx, R, G, B, ...]`.
    Warls,
}

impl WledProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drgb => "DRGB",
            Self::Ddp => "DDP",
            Self::Warls => "WARLS",
        }
    }

    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => Self::Ddp,
            2 => Self::Warls,
            _ => Self::Drgb,
        }
    }

    pub fn to_index(self) -> i32 {
        match self {
            Self::Drgb => 0,
            Self::Ddp => 1,
            Self::Warls => 2,
        }
    }
}

/// Starting corner of the physical LED strip around the monitor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartCorner {
    #[default]
    BottomLeft,
    TopLeft,
    TopRight,
    BottomRight,
}

impl StartCorner {
    pub fn from_index(idx: i32) -> Self {
        match idx {
            1 => Self::TopLeft,
            2 => Self::TopRight,
            3 => Self::BottomRight,
            _ => Self::BottomLeft,
        }
    }

    pub fn to_index(self) -> i32 {
        match self {
            Self::BottomLeft => 0,
            Self::TopLeft => 1,
            Self::TopRight => 2,
            Self::BottomRight => 3,
        }
    }
}

/// Screen edge identifiers.
pub const EDGE_LEFT: &str = "left";
pub const EDGE_TOP: &str = "top";
pub const EDGE_RIGHT: &str = "right";
pub const EDGE_BOTTOM: &str = "bottom";

/// Per-monitor LED geometry and routing configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorConfig {
    /// 1-based display index (matching Python `mss_index`: 1 = primary monitor).
    pub mss_index: usize,
    pub leds_bottom: usize,
    pub leds_left: usize,
    pub leds_top: usize,
    pub leds_right: usize,
    pub start_corner: StartCorner,
    pub clockwise: bool,
    pub edge_order: Vec<String>,
    pub edge_directions: HashMap<String, i8>,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        let mut mon = Self {
            mss_index: 1,
            leds_bottom: 15,
            leds_left: 9,
            leds_top: 15,
            leds_right: 9,
            start_corner: StartCorner::BottomLeft,
            clockwise: true,
            edge_order: Vec::new(),
            edge_directions: HashMap::new(),
        };
        mon.sync_geometry_from_corner_and_direction();
        mon
    }
}

impl MonitorConfig {
    /// Returns the canonical 4-edge order for `(start_corner, clockwise)`.
    pub const fn edge_order_for(start_corner: StartCorner, clockwise: bool) -> [&'static str; 4] {
        match (start_corner, clockwise) {
            (StartCorner::BottomLeft, true) => [EDGE_LEFT, EDGE_TOP, EDGE_RIGHT, EDGE_BOTTOM],
            (StartCorner::BottomLeft, false) => [EDGE_BOTTOM, EDGE_RIGHT, EDGE_TOP, EDGE_LEFT],
            (StartCorner::TopLeft, true) => [EDGE_TOP, EDGE_RIGHT, EDGE_BOTTOM, EDGE_LEFT],
            (StartCorner::TopLeft, false) => [EDGE_LEFT, EDGE_BOTTOM, EDGE_RIGHT, EDGE_TOP],
            (StartCorner::TopRight, true) => [EDGE_RIGHT, EDGE_BOTTOM, EDGE_LEFT, EDGE_TOP],
            (StartCorner::TopRight, false) => [EDGE_TOP, EDGE_LEFT, EDGE_BOTTOM, EDGE_RIGHT],
            (StartCorner::BottomRight, true) => [EDGE_BOTTOM, EDGE_LEFT, EDGE_TOP, EDGE_RIGHT],
            (StartCorner::BottomRight, false) => [EDGE_RIGHT, EDGE_TOP, EDGE_LEFT, EDGE_BOTTOM],
        }
    }

    /// Recomputes `edge_order` and `edge_directions` from `start_corner` and `clockwise`.
    pub fn sync_geometry_from_corner_and_direction(&mut self) {
        let order = Self::edge_order_for(self.start_corner, self.clockwise);
        self.edge_order = order.iter().map(|s| (*s).to_string()).collect();

        let (d_left, d_top, d_right, d_bottom) = if self.clockwise {
            (-1, 1, 1, -1)
        } else {
            (1, -1, -1, 1)
        };
        self.edge_directions.insert(EDGE_LEFT.to_string(), d_left);
        self.edge_directions.insert(EDGE_TOP.to_string(), d_top);
        self.edge_directions.insert(EDGE_RIGHT.to_string(), d_right);
        self.edge_directions
            .insert(EDGE_BOTTOM.to_string(), d_bottom);
    }

    /// Returns total LED count configured for this monitor across active edges in `edge_order`.
    pub fn total_leds(&self) -> usize {
        if self.edge_order.is_empty() {
            return self.leds_left + self.leds_top + self.leds_right + self.leds_bottom;
        }
        self.edge_order
            .iter()
            .map(|edge| self.led_count_for_edge(edge))
            .sum()
    }

    /// Returns LED count for an edge name (`"left"`, `"top"`, `"right"`, `"bottom"`).
    pub fn led_count_for_edge(&self, edge: &str) -> usize {
        match edge {
            EDGE_LEFT => self.leds_left,
            EDGE_TOP => self.leds_top,
            EDGE_RIGHT => self.leds_right,
            EDGE_BOTTOM => self.leds_bottom,
            _ => 0,
        }
    }

    /// Returns whether traversal along `edge` should be reversed (`-1`).
    pub fn is_edge_reversed(&self, edge: &str) -> bool {
        let default_dir = match edge {
            EDGE_LEFT | EDGE_BOTTOM => -1,
            _ => 1,
        };
        self.edge_directions
            .get(edge)
            .copied()
            .unwrap_or(default_dir)
            == -1
    }

    /// Returns effective `edge_order`, falling back to corner/direction when empty.
    pub fn effective_edge_order(&self) -> Vec<&str> {
        if self.edge_order.is_empty() {
            Self::edge_order_for(self.start_corner, self.clockwise).to_vec()
        } else {
            self.edge_order.iter().map(|s| s.as_str()).collect()
        }
    }
}

/// Complete application configuration stored in `%APPDATA%\GloWled\config.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub ip: String,
    pub port: u16,
    pub protocol: WledProtocol,
    pub fps: u32,
    pub brightness: f32,
    pub saturation: f32,
    pub gamma: f32,
    pub white_balance: [f32; 3],
    pub smoothing: f32,
    pub edge_depth: f32,
    pub black_threshold: f32,
    pub black_bar_detection: bool,
    pub oversample_factor: f32,
    pub eco_mode: bool,
    pub resolution: String,
    pub start_minimized: bool,
    pub autostart: bool,
    pub last_known_exe_path: String,
    pub monitors: Vec<MonitorConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".to_string(),
            port: 21324,
            protocol: WledProtocol::Drgb,
            fps: 60,
            brightness: 1.0,
            saturation: 1.0,
            gamma: 1.0,
            white_balance: [1.0, 1.0, 1.0],
            smoothing: 0.1,
            edge_depth: 0.08,
            black_threshold: 15.0,
            black_bar_detection: true,
            oversample_factor: 2.0,
            eco_mode: true,
            resolution: "Auto (Adaptiv)".to_string(),
            start_minimized: true,
            autostart: false,
            last_known_exe_path: String::new(),
            monitors: vec![MonitorConfig::default()],
        }
    }
}

fn clamp_finite(val: f32, min: f32, max: f32, default: f32) -> f32 {
    if val.is_finite() {
        val.clamp(min, max)
    } else {
        default
    }
}

impl AppConfig {
    /// Clamps numeric fields to safe operating ranges.
    pub fn sanitize(&mut self) {
        if self.ip.trim().is_empty() {
            self.ip = "127.0.0.1".to_string();
        } else {
            self.ip = self.ip.trim().to_string();
        }
        if self.port == 0 {
            self.port = 21324;
        }
        self.fps = self.fps.clamp(1, 120);
        self.brightness = clamp_finite(self.brightness, 0.05, 3.0, 1.0);
        self.saturation = clamp_finite(self.saturation, 0.0, 3.0, 1.0);
        self.gamma = clamp_finite(self.gamma, 0.5, 4.0, 1.0);
        for c in &mut self.white_balance {
            *c = clamp_finite(*c, 0.1, 2.0, 1.0);
        }
        self.smoothing = clamp_finite(self.smoothing, 0.0, 0.95, 0.1);
        if !self.edge_depth.is_finite() || self.edge_depth < 0.0 {
            self.edge_depth = 0.08;
        }
        self.black_threshold = clamp_finite(self.black_threshold, 0.0, 255.0, 15.0);
        self.oversample_factor = clamp_finite(self.oversample_factor, 1.0, 8.0, 2.0);
        if self.monitors.is_empty() {
            self.monitors.push(MonitorConfig::default());
        }
    }
}

/// Returns the GloWled application data directory (`%APPDATA%\GloWled` on Windows,
/// migrating legacy `AmbilightController` if present, or `~/.glowled` on Unix).
pub fn get_app_data_dir() -> PathBuf {
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_empty() {
            let glowled_dir = PathBuf::from(&appdata).join("GloWled");
            let legacy_dir = PathBuf::from(&appdata).join("AmbilightController");
            if !glowled_dir.join("config.json").exists() && legacy_dir.join("config.json").exists()
            {
                return legacy_dir;
            }
            return glowled_dir;
        }
    }

    if let Some(base_dirs) = directories::BaseDirs::new() {
        #[cfg(windows)]
        {
            let glowled_dir = base_dirs.config_dir().join("GloWled");
            return glowled_dir;
        }
        #[cfg(not(windows))]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                if !appdata.is_empty() {
                    return PathBuf::from(appdata).join("GloWled");
                }
            }
            return base_dirs.home_dir().join(".glowled");
        }
    }

    PathBuf::from(".glowled")
}

/// Returns the full path to `config.json`.
pub fn get_config_file_path() -> PathBuf {
    get_app_data_dir().join("config.json")
}

/// Loads configuration from `path`, merging missing keys with defaults for backward compatibility.
pub fn load_config_from_path(path: &Path) -> AppConfig {
    let mut cfg = AppConfig::default();
    if path.exists() {
        match fs::read_to_string(path) {
            Ok(raw) => match serde_json::from_str::<AppConfig>(&raw) {
                Ok(mut loaded) => {
                    loaded.sanitize();
                    return loaded;
                }
                Err(err) => {
                    eprintln!(
                        "[Config] Failed to parse {}: {err}. Using defaults.",
                        path.display()
                    );
                }
            },
            Err(err) => {
                eprintln!(
                    "[Config] Failed to read {}: {err}. Using defaults.",
                    path.display()
                );
            }
        }
    }
    cfg.sanitize();
    cfg
}

/// Loads configuration from the active `config.json` path.
pub fn load_config() -> AppConfig {
    load_config_from_path(&get_config_file_path())
}

/// Saves configuration to `path`, creating parent directories as needed.
pub fn save_config_to_path(cfg: &AppConfig, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| GlowledError::ConfigDirCreate {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    let mut sanitized = cfg.clone();
    sanitized.sanitize();
    let json = serde_json::to_string_pretty(&sanitized)?;
    fs::write(path, json).map_err(|source| GlowledError::ConfigWrite {
        path: path.to_path_buf(),
        source,
    })
}

/// Saves configuration to the active `config.json` path.
pub fn save_config(cfg: &AppConfig) -> Result<()> {
    save_config_to_path(cfg, &get_config_file_path())
}

/// Returns the current executable path as a string.
pub fn get_current_executable_path() -> String {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Formats the Windows Run registry command string for a given executable path.
pub fn format_autostart_command(exe_path: &str) -> String {
    format!("\"{exe_path}\" --autostart")
}

/// Windows Registry key path for user autostart.
pub const WIN_RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Windows Registry value name for GloWled autostart.
pub const WIN_RUN_VALUE_NAME: &str = "GloWled";

/// Removes legacy Python v1.x VBScript startup entries if present on Windows.
#[cfg(windows)]
fn cleanup_legacy_startup_vbs() {
    if let Ok(appdata) = std::env::var("APPDATA") {
        if appdata.is_empty() {
            return;
        }
        let startup_dir = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Startup");
        for legacy_name in ["GloWled.vbs", "AmbilightController.vbs"] {
            let candidate = startup_dir.join(legacy_name);
            if candidate.exists() {
                let _ = fs::remove_file(candidate);
            }
        }
    }
}

/// Enables or disables autostart via Windows Registry (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`)
/// on Windows, or a marker file in the config directory on non-Windows targets.
pub fn set_autostart(enable: bool, exe_path: Option<&str>) -> Result<()> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (run_key, _) = hkcu
            .create_subkey_with_flags(WIN_RUN_KEY_PATH, KEY_SET_VALUE)
            .map_err(|e| {
                GlowledError::Autostart(format!(
                    "HKCU\\{WIN_RUN_KEY_PATH} öffnen fehlgeschlagen: {e}"
                ))
            })?;

        if enable {
            let owned;
            let resolved = match exe_path {
                Some(p) if !p.is_empty() => p,
                _ => {
                    owned = get_current_executable_path();
                    &owned
                }
            };
            let cmd = format_autostart_command(resolved);
            run_key.set_value(WIN_RUN_VALUE_NAME, &cmd).map_err(|e| {
                GlowledError::Autostart(format!("Registry-Wert schreiben fehlgeschlagen: {e}"))
            })?;
        } else {
            match run_key.delete_value(WIN_RUN_VALUE_NAME) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(GlowledError::Autostart(format!(
                        "Registry-Wert löschen fehlgeschlagen: {e}"
                    )))
                }
            }
        }
        cleanup_legacy_startup_vbs();
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let marker = get_app_data_dir().join(".autostart_enabled");
        if enable {
            let owned;
            let resolved = match exe_path {
                Some(p) if !p.is_empty() => p,
                _ => {
                    owned = get_current_executable_path();
                    &owned
                }
            };
            if let Some(parent) = marker.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    GlowledError::Autostart(format!("Autostart-Verzeichnis fehlgeschlagen: {e}"))
                })?;
            }
            fs::write(&marker, format_autostart_command(resolved)).map_err(|e| {
                GlowledError::Autostart(format!("Autostart-Marker schreiben fehlgeschlagen: {e}"))
            })?;
        } else if marker.exists() {
            fs::remove_file(&marker).map_err(|e| {
                GlowledError::Autostart(format!("Autostart-Marker entfernen fehlgeschlagen: {e}"))
            })?;
        }
        Ok(())
    }
}

/// Checks whether autostart is currently enabled.
pub fn is_autostart_enabled() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run_key) = hkcu.open_subkey_with_flags(WIN_RUN_KEY_PATH, KEY_READ) {
            let val: std::io::Result<String> = run_key.get_value(WIN_RUN_VALUE_NAME);
            return val.map(|s| !s.trim().is_empty()).unwrap_or(false);
        }
        false
    }

    #[cfg(not(windows))]
    {
        get_app_data_dir().join(".autostart_enabled").exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values_and_geometry() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.ip, "127.0.0.1");
        assert_eq!(cfg.port, 21324);
        assert_eq!(cfg.protocol, WledProtocol::Drgb);
        assert_eq!(cfg.fps, 60);
        assert!(cfg.start_minimized);
        assert!(cfg.black_bar_detection);
        assert_eq!(cfg.monitors.len(), 1);
        assert_eq!(cfg.monitors[0].total_leds(), 48);
        assert_eq!(
            cfg.monitors[0].edge_order,
            vec!["left", "top", "right", "bottom"]
        );
        assert!(cfg.monitors[0].is_edge_reversed("left"));
        assert!(!cfg.monitors[0].is_edge_reversed("top"));
        assert!(!cfg.monitors[0].is_edge_reversed("right"));
        assert!(cfg.monitors[0].is_edge_reversed("bottom"));
    }

    #[test]
    fn save_and_load_roundtrip_and_legacy_python_json() {
        let dir = std::env::temp_dir().join(format!(
            "glowled_cfg_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let cfg_path = dir.join("config.json");

        // Legacy Python config without gamma, white_balance, protocol, black_bar_detection
        let legacy_json = r#"{
            "ip": "192.168.1.55",
            "port": 21324,
            "fps": 45,
            "brightness": 1.3,
            "saturation": 1.1,
            "resolution": "Quality (256x144)",
            "monitors": [
                {
                    "mss_index": 1,
                    "leds_left": 10,
                    "leds_top": 20,
                    "leds_right": 10,
                    "leds_bottom": 20,
                    "edge_order": ["top", "right", "bottom", "left"],
                    "edge_directions": {"left": 1, "top": 1, "right": 1, "bottom": 1}
                }
            ]
        }"#;
        fs::create_dir_all(&dir).unwrap();
        fs::write(&cfg_path, legacy_json).unwrap();

        let loaded = load_config_from_path(&cfg_path);
        assert_eq!(loaded.ip, "192.168.1.55");
        assert_eq!(loaded.fps, 45);
        assert_eq!(loaded.protocol, WledProtocol::Drgb);
        assert_eq!(loaded.gamma, 1.0);
        assert_eq!(loaded.white_balance, [1.0, 1.0, 1.0]);
        assert!(loaded.black_bar_detection);
        assert_eq!(loaded.monitors[0].total_leds(), 60);
        assert!(!loaded.monitors[0].is_edge_reversed("left"));

        // Round-trip save
        let mut modified = loaded.clone();
        modified.protocol = WledProtocol::Ddp;
        modified.gamma = 2.2;
        modified.start_minimized = false;
        save_config_to_path(&modified, &cfg_path).unwrap();

        let reloaded = load_config_from_path(&cfg_path);
        assert_eq!(reloaded.protocol, WledProtocol::Ddp);
        assert!((reloaded.gamma - 2.2).abs() < 1e-5);
        assert!(!reloaded.start_minimized);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn start_corner_and_clockwise_sync() {
        let mut mon = MonitorConfig {
            start_corner: StartCorner::TopLeft,
            clockwise: false,
            ..Default::default()
        };
        mon.sync_geometry_from_corner_and_direction();
        assert_eq!(mon.edge_order, vec!["left", "bottom", "right", "top"]);
        assert!(!mon.is_edge_reversed("left"));
        assert!(!mon.is_edge_reversed("bottom"));
        assert!(mon.is_edge_reversed("right"));
        assert!(mon.is_edge_reversed("top"));
    }

    #[test]
    fn format_autostart_command_quotes_path() {
        assert_eq!(
            format_autostart_command(r"C:\Program Files\GloWled\glowled.exe"),
            r#""C:\Program Files\GloWled\glowled.exe" --autostart"#
        );
    }
}
