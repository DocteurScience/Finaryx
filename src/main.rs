mod account_dialog;
mod application;
mod category_dialog;
mod charts;
mod config;
mod database;
mod operation_dialog;
mod pages;
mod preferences;
mod settings;
mod window;

use gettextrs::LocaleCategory;
use gtk::prelude::*;
use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    gettextrs::setlocale(LocaleCategory::LcAll, "");
    gettextrs::bindtextdomain(config::GETTEXT_PACKAGE, locale_dir())
        .expect("Unable to bind the text domain");
    gettextrs::bind_textdomain_codeset(config::GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    gettextrs::textdomain(config::GETTEXT_PACKAGE).expect("Unable to switch to the text domain");
    // Formats of dates and amounts follow the language of the messages
    // actually shown: each translation gives its code here
    // (build-aux/translations.py fills it in). TRANSLATORS: do not
    // translate, write your language code (en, de, pt_BR…).
    finaryx_core::locale::set(&gettextrs::gettext("fr"));

    gio::resources_register_include!("finaryx.gresource").expect("Failed to register resources");

    glib::set_application_name("Finaryx");
    let app = application::FinaryxApplication::new();
    app.run()
}

/// Translations: in a snap, the installed files are under `$SNAP`, not
/// at the prefix the build was configured with.
fn locale_dir() -> std::path::PathBuf {
    std::env::var_os("SNAP")
        .map(|root| std::path::Path::new(&root).join(config::LOCALEDIR.trim_start_matches('/')))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| config::LOCALEDIR.into())
}
