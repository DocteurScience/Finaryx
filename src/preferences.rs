//! Preferences dialog. For now: the style (system, light, dark).

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::settings;

const SCHEMES: [&str; 3] = ["default", "light", "dark"];

/// "Noms du foyer": transfers from these people are proposed as
/// "Virement interne" (page Catégories et règles).
fn household_group(window: &crate::window::FinaryxWindow) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Noms du foyer"))
        .description(gettext(
            "Votre nom, celui de votre conjoint… Un virement dont le libellé contient l’un de ces noms est proposé comme virement interne : ni dépense, ni revenu.",
        ))
        .build();
    fill_household(&group, window);
    group
}

fn fill_household(group: &adw::PreferencesGroup, window: &crate::window::FinaryxWindow) {
    let names = window
        .read(finaryx_core::transfers::household_names)
        .unwrap_or_default();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for (id, name) in names {
        let row = adw::ActionRow::builder()
            .title(glib::markup_escape_text(&name))
            .build();
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text(gettext("Retirer ce nom"))
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let (w, g) = (window.downgrade(), group.downgrade());
        remove.connect_clicked(move |_| {
            if let (Some(window), Some(group)) = (w.upgrade(), g.upgrade()) {
                if window
                    .change(|c| finaryx_core::transfers::remove_household_name(c, id))
                    .is_ok()
                {
                    refill(&group, &window);
                }
            }
        });
        row.add_suffix(&remove);
        list.append(&row);
    }
    let add = adw::EntryRow::builder()
        .title(gettext("Ajouter un nom"))
        .show_apply_button(true)
        .build();
    let (w, g) = (window.downgrade(), group.downgrade());
    add.connect_apply(move |e| {
        let text = e.text().to_string();
        if let (Some(window), Some(group)) = (w.upgrade(), g.upgrade()) {
            match window.change(|c| finaryx_core::transfers::add_household_name(c, &text)) {
                Ok(_) => refill(&group, &window),
                Err(m) => window.toast(&m),
            }
        }
    });
    list.append(&add);
    list.set_widget_name("household-list");
    group.add(&list);
}

fn refill(group: &adw::PreferencesGroup, window: &crate::window::FinaryxWindow) {
    // Remove the previous list, then build it again.
    fn find(w: &gtk::Widget) -> Option<gtk::Widget> {
        if w.widget_name() == "household-list" {
            return Some(w.clone());
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            if let Some(found) = find(&c) {
                return Some(found);
            }
            child = c.next_sibling();
        }
        None
    }
    if let Some(list) = find(group.upcast_ref()) {
        group.remove(&list);
    }
    fill_household(group, window);
}

pub fn preferences_dialog(window: Option<&crate::window::FinaryxWindow>) -> adw::PreferencesDialog {
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
            "Toutes vos données restent sur cet ordinateur. Finaryx n’effectue aucune requête réseau. La base de données n’est pas chiffrée.",
        ))
        .build();

    let page = adw::PreferencesPage::builder()
        .title(gettext("Général"))
        .icon_name("preferences-system-symbolic")
        .build();
    page.add(&appearance);
    if let Some(window) = window {
        page.add(&household_group(window));
    }
    page.add(&privacy);

    let dialog = adw::PreferencesDialog::builder()
        .title(gettext("Préférences"))
        .build();
    dialog.add(&page);
    dialog
}
