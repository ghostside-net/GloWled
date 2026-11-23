//! GloWled — High-performance Ambilight controller library.

pub mod capture;
pub mod config;
pub mod error;
pub mod i18n;
pub mod processing;
pub mod tray;
pub mod updater;
pub mod wled;

pub use error::{GlowledError, Result};
