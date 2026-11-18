//! Typed error definitions for GloWled using `thiserror`.

use std::path::PathBuf;

use thiserror::Error;

/// Unified result alias for GloWled operations.
pub type Result<T> = std::result::Result<T, GlowledError>;

/// Domain-specific error variants replacing legacy Python exceptions.
#[derive(Debug, Error)]
pub enum GlowledError {
    #[error("Konfigurationsverzeichnis {path} konnte nicht erstellt werden: {source}")]
    ConfigDirCreate {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Konfigurationsdatei {path} konnte nicht geschrieben werden: {source}")]
    ConfigWrite {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Konfiguration konnte nicht serialisiert werden: {0}")]
    ConfigSerialize(#[from] serde_json::Error),

    #[error("Autostart-Konfiguration fehlgeschlagen: {0}")]
    Autostart(String),

    #[error("Screen-Capture-Fehler ({backend}): {message}")]
    Capture {
        backend: &'static str,
        message: String,
    },

    #[error("Ungültige WLED-Zieladresse '{addr}': {reason}")]
    InvalidAddress { addr: String, reason: String },

    #[error("Netzwerk-/UDP-Fehler: {0}")]
    Network(#[from] std::io::Error),

    #[error("Worker-Thread konnte nicht gestartet werden: {0}")]
    WorkerSpawn(std::io::Error),

    #[error("System-Tray-Fehler: {0}")]
    Tray(String),

    #[error("UI-Fehler: {0}")]
    Ui(String),
}
