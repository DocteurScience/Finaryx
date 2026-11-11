//! Build-time configuration. Meson sets these variables when it calls
//! cargo; plain `cargo run` falls back to development defaults.

pub const APP_ID: &str = match option_env!("FINARYX_APP_ID") {
    Some(id) => id,
    None => "io.github.docteurscience.Finaryx",
};
/// The schema id does not change with the build profile.
pub const SCHEMA_ID: &str = "io.github.docteurscience.Finaryx";
pub const GETTEXT_PACKAGE: &str = "finaryx";
pub const LOCALEDIR: &str = match option_env!("FINARYX_LOCALEDIR") {
    Some(dir) => dir,
    None => "/usr/share/locale",
};
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Base path of the compiled resources; libadwaita loads `style.css`,
/// `style-dark.css`, `gtk/help-overlay.ui` and `icons/` from there.
pub const RESOURCE_BASE: &str = "/io/github/docteurscience/Finaryx";
/// Compiled schema used when the schema is not installed (development).
pub const DEV_SCHEMA_DIR: &str = env!("FINARYX_DEV_SCHEMA_DIR");
