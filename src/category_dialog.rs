//! "Nouvelle catégorie" and category settings (design/Categories.dc.html):
//! name, chart colour among the 12 shades (a taken one is greyed with the
//! name of its category) or none; merge and delete for an existing one.

use std::rc::Rc;

use adw::prelude::*;
use finaryx_core::categories::{self, CategoryUse, COLOR_SLOTS};
use finaryx_core::models::{builtin, Id};
use gettextrs::{gettext, ngettext};
use gtk::glib;

use crate::charts;
use crate::pages::caption;
use crate::window::FinaryxWindow;

fn protected(id: Id) -> bool {
    matches!(
        id,
        builtin::OTHERS | builtin::INCOME | builtin::INTERNAL_TRANSFER
    )
}

/// Opens the dialog. `existing` is `None` to create a category;
/// `on_done` receives the id of the created or changed category.
pub fn present(
    window: &FinaryxWindow,
    existing: Option<CategoryUse>,
    on_done: impl Fn(Id) + 'static,
) {
    let Some((owners, all)) = window.read(|c| {
        Ok((
            categories::slot_owners(c)?,
            categories::list_with_counts(c)?,
        ))
    }) else {
        return;
    };
    let creating = existing.is_none();
    let current_id = existing.as_ref().map(|u| u.category.id);
    let current_slot = existing.as_ref().and_then(|u| u.category.slot);
    let shown_name = existing.as_ref().map(|u| u.category.name.clone());

    let name = adw::EntryRow::builder()
        .title(gettext("Nom"))
        .text(
            existing
                .as_ref()
                .map(|u| u.category.name.as_str())
                .unwrap_or(""),
        )
        .build();
    let name_group = adw::PreferencesGroup::new();
    name_group.add(&name);

    // Chart colour: 12 shades, then "Aucune couleur".
    let colors = adw::PreferencesGroup::builder()
        .title(gettext("Couleur dans les graphiques"))
        .build();
    let mut checks: Vec<(Option<u8>, gtk::CheckButton)> = Vec::new();
    let mut first: Option<gtk::CheckButton> = None;
    let mut free = 0;
    for slot in 1..=COLOR_SLOTS {
        let owner = owners[slot as usize - 1]
            .as_ref()
            .filter(|(id, _)| Some(*id) != current_id);
        let row = adw::ActionRow::builder()
            .title(gettext("Couleur {n}").replace("{n}", &slot.to_string()))
            .subtitle(match owner {
                Some((_, n)) => gettext("Utilisée par {name}").replace("{name}", n),
                None => gettext("Libre"),
            })
            .sensitive(owner.is_none())
            .build();
        if owner.is_none() {
            free += 1;
        }
        let check = gtk::CheckButton::builder()
            .valign(gtk::Align::Center)
            .active(current_slot == Some(slot))
            .build();
        if let Some(f) = &first {
            check.set_group(Some(f));
        } else {
            first = Some(check.clone());
        }
        let s = Some(slot);
        row.add_prefix(&check);
        row.add_suffix(&charts::color_swatch(16, move |w| {
            charts::category_color(w, s)
        }));
        row.set_activatable_widget(Some(&check));
        colors.add(&row);
        checks.push((Some(slot), check));
    }
    let none_row = adw::ActionRow::builder()
        .title(gettext("Aucune couleur"))
        .subtitle(gettext("Regroupée dans « Autres » dans les graphiques"))
        .build();
    let none_check = gtk::CheckButton::builder()
        .valign(gtk::Align::Center)
        .active(current_slot.is_none())
        .group(first.as_ref().unwrap())
        .build();
    none_row.add_prefix(&none_check);
    none_row.set_activatable_widget(Some(&none_check));
    colors.add(&none_row);
    checks.push((None, none_check));
    if current_id == Some(builtin::OTHERS) {
        colors.set_sensitive(false);
        colors.set_description(Some(&gettext("« Autres » garde toujours le gris.")));
    } else if free == 0 {
        colors.set_description(Some(&gettext(
            "Toutes les couleurs sont prises. Cette catégorie sera comptée dans « Autres » dans les graphiques et séparée partout ailleurs. Libérez une couleur en retirant celle d’une autre catégorie.",
        )));
    }

    let error = gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .visible(false)
        .css_classes(["error"])
        .build();
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&name_group);
    content.append(&colors);
    content.append(&error);

    let cancel = gtk::Button::with_label(&gettext("Annuler"));
    let save = gtk::Button::builder()
        .label(if creating {
            gettext("Créer")
        } else {
            gettext("Enregistrer")
        })
        .css_classes(["suggested-action"])
        .build();
    let header = adw::HeaderBar::builder()
        .show_start_title_buttons(false)
        .show_end_title_buttons(false)
        .build();
    header.pack_start(&cancel);
    header.pack_end(&save);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .child(&content)
        .build();
    toolbar.set_content(Some(&scrolled));
    let dialog = adw::Dialog::builder()
        .title(if creating {
            gettext("Nouvelle catégorie")
        } else {
            gettext("Modifier la catégorie")
        })
        .content_width(460)
        .child(&toolbar)
        .default_widget(&save)
        .build();

    // Merge and delete (existing, non-protected categories).
    if let Some(u) = existing.as_ref().filter(|u| !protected(u.category.id)) {
        let others: Vec<(Id, String)> = all
            .iter()
            .filter(|o| o.category.id != u.category.id)
            .map(|o| (o.category.id, o.category.name.clone()))
            .collect();
        let labels: Vec<&str> = others.iter().map(|(_, n)| n.as_str()).collect();
        let merge_row = adw::ComboRow::builder()
            .title(gettext("Fusionner dans"))
            .subtitle(gettext(
                "Les opérations et les règles passent dans la catégorie choisie",
            ))
            .model(&gtk::StringList::new(&labels))
            .build();
        let merge = gtk::Button::builder()
            .label(gettext("Fusionner…"))
            .valign(gtk::Align::Center)
            .build();
        merge_row.add_suffix(&merge);
        let delete_row = adw::ActionRow::builder()
            .title(gettext("Supprimer la catégorie"))
            .subtitle(
                ngettext(
                    "Son opération passera dans « Autres », à vérifier ; ses règles seront supprimées.",
                    "Ses {n} opérations passeront dans « Autres », à vérifier ; ses règles seront supprimées.",
                    u.operations.clamp(0, u32::MAX as i64) as u32,
                )
                .replace("{n}", &u.operations.to_string()),
            )
            .build();
        let delete = gtk::Button::builder()
            .label(gettext("Supprimer…"))
            .valign(gtk::Align::Center)
            .css_classes(["destructive-action"])
            .build();
        delete_row.add_suffix(&delete);
        let group = adw::PreferencesGroup::new();
        group.add(&merge_row);
        group.add(&delete_row);
        content.append(&group);

        let (id, name_now, count) = (u.category.id, u.category.name.clone(), u.operations);
        let (w, d) = (window.downgrade(), dialog.clone());
        merge.connect_clicked(move |_| {
            let Some((into, into_name)) = others.get(merge_row.selected() as usize).cloned() else { return };
            let Some(window) = w.upgrade() else { return };
            confirm(
                &window,
                &gettext("Fusionner « {from} » dans « {into} » ?").replace("{from}", &name_now).replace("{into}", &into_name),
                &ngettext(
                    "{n} opération et les règles de « {from} » passeront dans « {into} ». Les totaux ne changent pas.",
                    "{n} opérations et les règles de « {from} » passeront dans « {into} ». Les totaux ne changent pas.",
                    count.clamp(0, u32::MAX as i64) as u32,
                )
                .replace("{n}", &count.to_string())
                .replace("{from}", &name_now)
                .replace("{into}", &into_name),
                &gettext("Fusionner"),
                glib::clone!(
                    #[weak]
                    d,
                    move |window: &FinaryxWindow| match window.change(|c| categories::merge_category(c, id, into)) {
                        Ok(_) => {
                            window.toast(&gettext("Catégories fusionnées"));
                            d.close();
                        }
                        Err(m) => window.toast(&m),
                    }
                ),
            );
        });
        let (w, d, name_now) = (window.downgrade(), dialog.clone(), u.category.name.clone());
        delete.connect_clicked(move |_| {
            let Some(window) = w.upgrade() else { return };
            confirm(
                &window,
                &gettext("Supprimer « {name} » ?").replace("{name}", &name_now),
                &ngettext(
                    "Son opération passera dans « Autres », marquée « À vérifier ». Ses règles seront supprimées.",
                    "Ses {n} opérations passeront dans « Autres », marquées « À vérifier ». Ses règles seront supprimées.",
                    count.clamp(0, u32::MAX as i64) as u32,
                )
                .replace("{n}", &count.to_string()),
                &gettext("Supprimer"),
                glib::clone!(
                    #[weak]
                    d,
                    move |window: &FinaryxWindow| match window.change(|c| categories::delete_category(c, id)) {
                        Ok(_) => {
                            window.toast(&gettext("Catégorie supprimée"));
                            d.close();
                        }
                        Err(m) => window.toast(&m),
                    }
                ),
            );
        });
    }

    let d = dialog.clone();
    cancel.connect_clicked(move |_| {
        d.close();
    });
    let on_done = Rc::new(on_done);
    let submit = {
        let (w, d, name, error) = (
            window.downgrade(),
            dialog.clone(),
            name.clone(),
            error.clone(),
        );
        move || {
            let Some(window) = w.upgrade() else { return };
            let slot = checks
                .iter()
                .find(|(_, c)| c.is_active())
                .and_then(|(s, _)| *s);
            let text = name.text().to_string();
            let result = window.change(|c| match current_id {
                None => categories::create_category(c, &text, slot),
                Some(id) => {
                    // The field shows the translated name of a built-in
                    // category: unchanged, it is not written.
                    if Some(text.trim()) != shown_name.as_deref() {
                        categories::rename_category(c, id, &text)?;
                    }
                    if id != builtin::OTHERS {
                        categories::set_slot(c, id, slot)?;
                    }
                    Ok(id)
                }
            });
            match result {
                Ok(id) => {
                    d.close();
                    on_done(id);
                }
                Err(m) => {
                    error.set_label(&m);
                    error.set_visible(true);
                }
            }
        }
    };
    let submit = Rc::new(submit);
    let s = submit.clone();
    save.connect_clicked(move |_| s());
    let s = submit.clone();
    name.connect_entry_activated(move |_| s());
    let save_button = save.clone();
    name.connect_changed(move |e| save_button.set_sensitive(!e.text().trim().is_empty()));
    save.set_sensitive(!name.text().trim().is_empty());

    dialog.present(Some(window));
    name.grab_focus();
    let _ = caption;
}

/// A confirmation with a destructive response.
pub fn confirm(
    window: &FinaryxWindow,
    heading: &str,
    body: &str,
    action: &str,
    on_yes: impl Fn(&FinaryxWindow) + 'static,
) {
    let alert = adw::AlertDialog::new(Some(heading), Some(body));
    alert.add_responses(&[("cancel", &gettext("Annuler")), ("yes", action)]);
    alert.set_response_appearance("yes", adw::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");
    let w = window.downgrade();
    alert.connect_response(Some("yes"), move |_, _| {
        if let Some(window) = w.upgrade() {
            on_yes(&window);
        }
    });
    alert.present(Some(window));
}
