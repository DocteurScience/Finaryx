mod account_dialog;
mod application;
mod category_dialog;
mod charts;
mod config;
mod database;
mod pages;
mod preferences;
mod settings;
mod window;

use gettextrs::LocaleCategory;
use gtk::prelude::*;
use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    gettextrs::setlocale(LocaleCategory::LcAll, "");
    gettextrs::bindtextdomain(config::GETTEXT_PACKAGE, config::LOCALEDIR)
        .expect("Unable to bind the text domain");
    gettextrs::bind_textdomain_codeset(config::GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    gettextrs::textdomain(config::GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    gio::resources_register_include!("finaryx.gresource").expect("Failed to register resources");

    glib::set_application_name("Finaryx");
    let app = application::FinaryxApplication::new();
    app.run()
}
