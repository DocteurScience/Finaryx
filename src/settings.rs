//! Access to the application's GSettings.

use gtk::gio;
use gtk::prelude::*;

use crate::config;

/// Returns the settings, from the installed schema if there is one,
/// otherwise from the schema compiled next to the binary at build time.
pub fn settings() -> gio::Settings {
    let source = gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup(config::SCHEMA_ID, true))
        .or_else(|| {
            gio::SettingsSchemaSource::from_directory(
                config::DEV_SCHEMA_DIR,
                gio::SettingsSchemaSource::default().as_ref(),
                false,
            )
            .ok()
            .and_then(|s| s.lookup(config::SCHEMA_ID, false))
        })
        .expect("GSettings schema not found");
    gio::Settings::new_full(&source, None::<&gio::SettingsBackend>, None)
}

/// Applies the `color-scheme` key to libadwaita's style manager.
pub fn apply_color_scheme(settings: &gio::Settings) {
    let scheme = match settings.string("color-scheme").as_str() {
        "light" => adw::ColorScheme::ForceLight,
        "dark" => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    };
    adw::StyleManager::default().set_color_scheme(scheme);
}
