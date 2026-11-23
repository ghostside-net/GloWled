//! System tray integration using `tray-icon` and `muda` native menus.

use std::cell::Cell;

use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::error::{GlowledError, Result};
use crate::i18n::Language;

const EMBEDDED_ICO: &[u8] = include_bytes!("../icon.ico");

/// Actions emitted by the system tray icon or context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    ToggleActive,
    OpenSettings,
    CheckUpdates,
    Quit,
}

/// Manages the OS system tray icon and its context menu.
pub struct TrayManager {
    _tray_icon: TrayIcon,
    status_item: MenuItem,
    open_item: MenuItem,
    update_item: MenuItem,
    quit_item: MenuItem,
    language: Cell<Language>,
}

impl TrayManager {
    /// Creates the system tray icon and localized context menu.
    pub fn new(initial_active: bool, language: Language) -> Result<Self> {
        let icon = load_tray_icon()?;
        let t = language.translations();

        let status_item = MenuItem::new(status_label(initial_active, language), true, None);
        let open_item = MenuItem::new(t.tray_open_settings, true, None);
        let update_item = MenuItem::new(t.tray_check_updates, true, None);
        let quit_item = MenuItem::new(t.tray_quit, true, None);

        let menu = Menu::new();
        menu.append(&status_item)
            .map_err(|e| GlowledError::Tray(format!("Menu append failed: {e}")))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| GlowledError::Tray(format!("Menu separator failed: {e}")))?;
        menu.append(&open_item)
            .map_err(|e| GlowledError::Tray(format!("Menu append failed: {e}")))?;
        menu.append(&update_item)
            .map_err(|e| GlowledError::Tray(format!("Menu append failed: {e}")))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| GlowledError::Tray(format!("Menu separator failed: {e}")))?;
        menu.append(&quit_item)
            .map_err(|e| GlowledError::Tray(format!("Menu append failed: {e}")))?;

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip_label(initial_active, language))
            .with_icon(icon)
            .build()
            .map_err(|e| GlowledError::Tray(format!("Failed to build TrayIcon: {e}")))?;

        Ok(Self {
            _tray_icon: tray_icon,
            status_item,
            open_item,
            update_item,
            quit_item,
            language: Cell::new(language),
        })
    }

    /// Updates the menu item text and tooltip when Ambilight active/paused state changes.
    pub fn set_active(&self, active: bool) {
        let lang = self.language.get();
        self.status_item.set_text(status_label(active, lang));
        let _ = self
            ._tray_icon
            .set_tooltip(Some(tooltip_label(active, lang)));
    }

    /// Updates all tray menu labels and tooltip immediately when the language changes.
    pub fn set_language(&self, language: Language, active: bool) {
        self.language.set(language);
        let t = language.translations();
        self.status_item.set_text(status_label(active, language));
        self.open_item.set_text(t.tray_open_settings);
        self.update_item.set_text(t.tray_check_updates);
        self.quit_item.set_text(t.tray_quit);
        let _ = self
            ._tray_icon
            .set_tooltip(Some(tooltip_label(active, language)));
    }

    /// Non-blocking poll for tray clicks and menu item selections (intended for a 50 ms Slint timer).
    pub fn poll_actions(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
                | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => {
                    actions.push(TrayAction::OpenSettings);
                }
                _ => {}
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == *self.status_item.id() {
                actions.push(TrayAction::ToggleActive);
            } else if event.id == *self.open_item.id() {
                actions.push(TrayAction::OpenSettings);
            } else if event.id == *self.update_item.id() {
                actions.push(TrayAction::CheckUpdates);
            } else if event.id == *self.quit_item.id() {
                actions.push(TrayAction::Quit);
            }
        }

        actions
    }
}

fn status_label(active: bool, language: Language) -> &'static str {
    let t = language.translations();
    if active {
        t.tray_status_active
    } else {
        t.tray_status_paused
    }
}

fn tooltip_label(active: bool, language: Language) -> &'static str {
    let t = language.translations();
    if active {
        t.tray_tooltip_active
    } else {
        t.tray_tooltip_paused
    }
}

/// Decodes embedded `icon.ico` into an RGBA tray icon, falling back to a generated icon if needed.
pub fn load_tray_icon() -> Result<Icon> {
    if let Ok(img) = image::load_from_memory_with_format(EMBEDDED_ICO, image::ImageFormat::Ico) {
        let rgba = img
            .resize_exact(32, 32, image::imageops::FilterType::Triangle)
            .to_rgba8();
        let (w, h) = rgba.dimensions();
        if let Ok(icon) = Icon::from_rgba(rgba.into_raw(), w, h) {
            return Ok(icon);
        }
    }
    build_fallback_icon()
}

fn build_fallback_icon() -> Result<Icon> {
    let size = 32u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let center = (size as f32) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) - center;
            let dy = (y as f32 + 0.5) - center;
            let dist = (dx * dx + dy * dy).sqrt();
            if (8.0..=14.0).contains(&dist) {
                rgba.extend_from_slice(&[56, 189, 248, 255]); // Sky blue ring
            } else if dist < 8.0 {
                rgba.extend_from_slice(&[16, 185, 129, 255]); // Emerald core
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Icon::from_rgba(rgba, size, size)
        .map_err(|e| GlowledError::Tray(format!("Fallback icon creation failed: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_icon_decodes_cleanly() {
        assert!(load_tray_icon().is_ok());
    }

    #[test]
    fn tray_labels_localize_cleanly() {
        assert_eq!(
            status_label(true, Language::German),
            "Status: Aktiv (Klicken zum Pausieren)"
        );
        assert_eq!(
            status_label(false, Language::German),
            "Status: Pausiert (Klicken zum Aktivieren)"
        );
        assert_eq!(
            status_label(true, Language::English),
            "Status: Active (Click to Pause)"
        );
        assert_eq!(
            status_label(false, Language::English),
            "Status: Paused (Click to Resume)"
        );
        assert_ne!(
            tooltip_label(true, Language::German),
            tooltip_label(true, Language::English)
        );
    }
}
