//! Preferences dialog. For now: the style (system, light, dark).

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::settings;

const SCHEMES: [&str; 3] = ["default", "light", "dark"];

pub fn preferences_dialog() -> adw::PreferencesDialog {
    let settings = settings::settings();

    let style_row = adw::ComboRow::builder()
        .title(gettext("Style"))
        .model(&gtk::StringList::new(&[
            &gettext("Suivre le système"),
            &gettext("Clair"),
            &gettext("Sombre"),
        ]))
        .build();
    let current = settings.string("color-scheme");
    let index = SCHEMES
        .iter()
        .position(|s| *s == current.as_str())
        .unwrap_or(0);
    style_row.set_selected(index as u32);
    style_row.connect_selected_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            let scheme = SCHEMES
                .get(row.selected() as usize)
                .copied()
                .unwrap_or("default");
            // The application applies the change (see `startup`).
            let _ = settings.set_string("color-scheme", scheme);
        }
    ));

    let appearance = adw::PreferencesGroup::builder()
        .title(gettext("Apparence"))
        .build();
    appearance.add(&style_row);

    let privacy = adw::PreferencesGroup::builder()
        .title(gettext("Confidentialité"))
        .description(gettext(
            "Toutes vos données restent sur cet ordinateur. Finaryx n’effectue aucune \
             requête réseau. La base de données n’est pas chiffrée.",
        ))
        .build();

    let page = adw::PreferencesPage::builder()
        .title(gettext("Général"))
        .icon_name("preferences-system-symbolic")
        .build();
    page.add(&appearance);
    page.add(&privacy);

    let dialog = adw::PreferencesDialog::builder()
        .title(gettext("Préférences"))
        .build();
    dialog.add(&page);
    dialog
}
