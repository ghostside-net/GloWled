//! GloWled — High-performance Ambilight controller library.

pub mod capture;
pub mod config;
pub mod error;
pub mod processing;
pub mod tray;
pub mod wled;

pub use error::{GlowledError, Result};
