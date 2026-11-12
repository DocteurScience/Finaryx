//! The `AdwApplication` subclass: application-wide actions and windows.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::config;
use crate::database::Source;
use crate::window::FinaryxWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct FinaryxApplication {
        pub settings: std::cell::OnceCell<gio::Settings>,
        /// Set by `--demo`: work on demonstration data in memory.
        pub demo: std::cell::Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FinaryxApplication {
        const NAME: &'static str = "FinaryxApplication";
        type Type = super::FinaryxApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for FinaryxApplication {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().add_main_option(
                "demo",
                glib::Char::from(b'd'),
                glib::OptionFlags::NONE,
                glib::OptionArg::None,
                &gettext("Ouvrir des données de démonstration (rien n’est enregistré)"),
                None,
            );
        }
    }

    impl ApplicationImpl for FinaryxApplication {
        fn handle_local_options(&self, options: &glib::VariantDict) -> glib::ExitCode {
            if options.contains("demo") {
                self.demo.set(true);
                // The demo runs beside a normal instance, never inside it.
                let app = self.obj();
                app.set_flags(app.flags() | gio::ApplicationFlags::NON_UNIQUE);
            }
            self.parent_handle_local_options(options)
        }

        fn open(&self, files: &[gio::File], _hint: &str) {
            self.activate();
            let app = self.obj();
            if let (Some(window), Some(file)) = (app.active_window(), files.first()) {
                if let Ok(window) = window.downcast::<FinaryxWindow>() {
                    window.import_file(file.clone());
                }
            }
        }

        fn activate(&self) {
            let app = self.obj();
            let window = app.active_window().unwrap_or_else(|| {
                let source = if self.demo.get() {
                    Source::Demo
                } else {
                    Source::File(crate::database::default_path())
                };
                FinaryxWindow::new(&*app, &source).upcast()
            });
            window.present();
        }

        fn startup(&self) {
            self.parent_startup();
            let app = self.obj();
            app.setup_actions();

            // Follow the style preference, including changes made elsewhere.
            let settings = crate::settings::settings();
            crate::settings::apply_color_scheme(&settings);
            settings.connect_changed(Some("color-scheme"), |settings, _| {
                crate::settings::apply_color_scheme(settings);
            });
            self.settings.set(settings).unwrap();
        }
    }

    impl GtkApplicationImpl for FinaryxApplication {}
    impl AdwApplicationImpl for FinaryxApplication {}
}

glib::wrapper! {
    pub struct FinaryxApplication(ObjectSubclass<imp::FinaryxApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl FinaryxApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", config::APP_ID)
            .property("resource-base-path", config::RESOURCE_BASE)
            .property("flags", gio::ApplicationFlags::HANDLES_OPEN)
            .build()
    }

    fn setup_actions(&self) {
        let quit = gio::ActionEntry::builder("quit")
            .activate(|app: &Self, _, _| app.quit())
            .build();
        let about = gio::ActionEntry::builder("about")
            .activate(|app: &Self, _, _| app.show_about())
            .build();
        let preferences = gio::ActionEntry::builder("preferences")
            .activate(|app: &Self, _, _| app.show_preferences())
            .build();
        self.add_action_entries([quit, about, preferences]);

        self.set_accels_for_action("app.quit", &["<Control>q"]);
        self.set_accels_for_action("app.preferences", &["<Control>comma"]);
        self.set_accels_for_action("window.close", &["<Control>w"]);
        self.set_accels_for_action("win.show-help-overlay", &["<Control>question"]);
        self.set_accels_for_action("win.import", &["<Control>i"]);
        for (i, page) in crate::pages::MAIN_PAGES.iter().enumerate() {
            let accel = format!("<Alt>{}", i + 1);
            self.set_accels_for_action(&format!("win.show-page::{}", page.name), &[&accel]);
        }
    }

    fn show_about(&self) {
        let dialog = adw::AboutDialog::builder()
            .application_name("Finaryx")
            .application_icon(config::APP_ID)
            .developer_name("DocteurScience")
            .version(config::VERSION)
            .website("https://github.com/DocteurScience/Finaryx")
            .issue_url("https://github.com/DocteurScience/Finaryx/issues")
            .license_type(gtk::License::Agpl30)
            .comments(gettext(
                "Suivi de comptes bancaires. Vos données restent sur cet ordinateur : \
                 aucune requête réseau, aucune télémétrie. La base de données n’est pas chiffrée.",
            ))
            .build();
        dialog.present(self.active_window().as_ref());
    }

    fn show_preferences(&self) {
        let window = self.active_window().and_downcast::<FinaryxWindow>();
        let dialog = crate::preferences::preferences_dialog(window.as_ref());
        dialog.present(self.active_window().as_ref());
    }
}

impl Default for FinaryxApplication {
    fn default() -> Self {
        Self::new()
    }
}
