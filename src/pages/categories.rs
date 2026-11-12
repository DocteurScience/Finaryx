//! "Catégories et règles" (design/Categories.dc.html), opened from the
//! main menu: categories with their colour, rules, transfers between the
//! user's accounts to confirm, household names found in operations.

use std::rc::Rc;

use adw::prelude::*;
use finaryx_core::categories::{self, CategoryUse, OTHERS_GROUP};
use finaryx_core::dates;
use finaryx_core::models::{builtin, Id};
use finaryx_core::money::Sign;
use finaryx_core::transfers;
use gettextrs::{gettext, ngettext};
use gtk::glib;

use super::{amount, boxed_list, caption, clear, heading, n_ops, section};
use crate::category_dialog;
use crate::charts;
use crate::window::FinaryxWindow;

pub const TAG: &str = "categories";

pub struct CategoriesPage {
    pub page: adw::NavigationPage,
    content: gtk::Box,
    window: glib::WeakRef<FinaryxWindow>,
}

impl std::fmt::Debug for CategoriesPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CategoriesPage").finish_non_exhaustive()
    }
}

/// Subtitle of a category in the list.
fn describe(u: &CategoryUse) -> String {
    let c = &u.category;
    let what = match c.id {
        builtin::OTHERS => gettext("catégorie par défaut, non supprimable"),
        builtin::INCOME => gettext("catégorie spéciale, comptée en revenus"),
        builtin::INTERNAL_TRANSFER => gettext("catégorie spéciale, exclue des statistiques"),
        _ => match c.slot {
            Some(slot) => gettext("couleur de graphique {n}").replace("{n}", &slot.to_string()),
            None => gettext("sans couleur, regroupée dans « Autres » dans les graphiques"),
        },
    };
    format!("{} · {what}", n_ops(u.operations))
}

/// Swatch slot of a category: its colour, the grey for "Autres".
fn swatch_slot(id: Id, slot: Option<u8>) -> Option<u8> {
    slot.or((id == builtin::OTHERS).then_some(OTHERS_GROUP))
}

/// Colour swatch of a category; a dashed outline when it has none.
fn swatch(slot: Option<u8>, size: i32, special: bool) -> gtk::DrawingArea {
    if slot.is_some() {
        return charts::color_swatch(size, move |w| charts::category_color(w, slot));
    }
    let area = gtk::DrawingArea::builder()
        .content_width(size)
        .content_height(size)
        .valign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |area, cr, w, h| {
        let mut c = area.color();
        c.set_alpha(if special { 0.35 } else { 0.6 });
        charts::set_source(cr, &c);
        cr.set_line_width(1.5);
        cr.set_dash(&[2.0, 2.0], 0.0);
        charts::rounded_rect(cr, 1.0, 1.0, w as f64 - 2.0, h as f64 - 2.0, 3.0);
        let _ = cr.stroke();
    });
    area
}

impl CategoriesPage {
    pub fn new(window: &FinaryxWindow) -> Rc<Self> {
        let header = adw::HeaderBar::new();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(28)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();
        let clamp = adw::Clamp::builder()
            .maximum_size(760)
            .tightening_threshold(760)
            .child(&content)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&clamp)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&scrolled));
        let page = adw::NavigationPage::builder()
            .tag(TAG)
            .title(gettext("Catégories et règles"))
            .child(&toolbar)
            .build();
        let this = Rc::new(CategoriesPage {
            page,
            content,
            window: window.downgrade(),
        });
        this.refresh();
        this
    }

    pub fn refresh(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let Some((uses, rules, proposals, matches, names)) = window.read(|c| {
            Ok((
                categories::list_with_counts(c)?,
                categories::list_rules(c)?,
                transfers::proposals(c)?,
                transfers::household_matches(c)?,
                transfers::household_names(c)?,
            ))
        }) else {
            return;
        };
        clear(&self.content);
        self.content.append(&self.categories_section(&uses));
        self.content.append(&self.rules_section(&rules));
        self.content.append(&self.transfers_section(&proposals));
        self.content
            .append(&self.household_section(&matches, names.is_empty()));
    }

    fn categories_section(self: &Rc<Self>, uses: &[CategoryUse]) -> gtk::Box {
        let header = gtk::Box::builder().spacing(12).build();
        let title = heading(&gettext("Catégories"));
        title.set_hexpand(true);
        header.append(&title);
        let new = gtk::Button::builder()
            .child(
                &adw::ButtonContent::builder()
                    .icon_name("list-add-symbolic")
                    .label(gettext("Nouvelle catégorie"))
                    .build(),
            )
            .css_classes(["suggested-action"])
            .build();
        let w = self.window.clone();
        new.connect_clicked(move |_| {
            if let Some(window) = w.upgrade() {
                category_dialog::present(&window, None, |_| {});
            }
        });
        header.append(&new);

        let list = boxed_list();
        for u in uses {
            let special = matches!(u.category.id, builtin::INCOME | builtin::INTERNAL_TRANSFER);
            let row = adw::ActionRow::builder()
                .title(&u.category.name)
                .subtitle(describe(u))
                .subtitle_lines(2)
                .activatable(true)
                .build();
            row.add_prefix(&swatch(
                swatch_slot(u.category.id, u.category.slot),
                14,
                special,
            ));
            let edit = gtk::Image::from_icon_name("document-edit-symbolic");
            row.add_suffix(&edit);
            row.update_property(&[gtk::accessible::Property::Label(
                &gettext("Modifier la catégorie {name}").replace("{name}", &u.category.name),
            )]);
            let (w, u) = (self.window.clone(), u.clone());
            row.connect_activated(move |_| {
                if let Some(window) = w.upgrade() {
                    category_dialog::present(&window, Some(u.clone()), |_| {});
                }
            });
            list.append(&row);
        }
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        body.append(&list);
        body.append(&caption(&gettext(
            "Les graphiques distinguent au plus 12 couleurs, plus « Autres » en gris. Une catégorie sans couleur est \
             comptée dans « Autres » dans les graphiques, mais reste séparée dans les listes, les filtres et les \
             tableaux. Un clic sur « Autres » dans un graphique en affiche le détail. Pour donner une couleur à une \
             catégorie, libérez-en une en la retirant à une autre catégorie.",
        )));
        section(header.upcast_ref(), &body)
    }

    fn rules_section(self: &Rc<Self>, rules: &[categories::Rule]) -> gtk::Box {
        let list = boxed_list();
        let counts: Vec<usize> = self
            .window
            .upgrade()
            .and_then(|w| {
                w.read(|c| {
                    rules
                        .iter()
                        .map(|r| {
                            Ok(
                                categories::matching_operations(c, &r.pattern, r.is_regex, None)?
                                    .0
                                    .len(),
                            )
                        })
                        .collect()
                })
            })
            .unwrap_or_default();
        for (i, r) in rules.iter().enumerate() {
            let title = if r.is_regex {
                gettext("Le libellé correspond à « {pattern} »")
            } else {
                gettext("Le libellé contient « {pattern} »")
            }
            .replace("{pattern}", &r.pattern);
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&title))
                .subtitle(n_ops(counts.get(i).copied().unwrap_or(0) as i64))
                .build();
            let cat = gtk::Box::builder()
                .spacing(6)
                .valign(gtk::Align::Center)
                .build();
            cat.append(&swatch(
                swatch_slot(r.category_id, r.category_slot),
                10,
                false,
            ));
            cat.append(&gtk::Label::new(Some(&r.category_name)));
            row.add_suffix(&cat);
            let delete = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .tooltip_text(gettext("Supprimer la règle"))
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            let (w, id) = (self.window.clone(), r.id);
            delete.connect_clicked(move |_| {
                if let Some(window) = w.upgrade() {
                    match window.change(|c| categories::delete_rule(c, id)) {
                        Ok(()) => window.toast(&gettext("Règle supprimée")),
                        Err(m) => window.toast(&m),
                    }
                }
            });
            row.add_suffix(&delete);
            list.append(&row);
        }
        if rules.is_empty() {
            list.append(
                &adw::ActionRow::builder()
                    .title(gettext("Aucune règle pour l’instant"))
                    .subtitle(gettext(
                        "Changez la catégorie d’une opération pour en créer une.",
                    ))
                    .build(),
            );
        }
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        body.append(&list);
        body.append(&caption(&gettext(
            "Une correction manuelle faite sur une opération passe avant les règles. Supprimer une règle ne change pas \
             les opérations déjà classées.",
        )));
        section(
            heading(&gettext("Règles de catégorisation")).upcast_ref(),
            &body,
        )
    }

    fn transfers_section(self: &Rc<Self>, proposals: &[transfers::Proposal]) -> gtk::Box {
        let header = gtk::Box::builder().spacing(12).build();
        let titles = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .build();
        titles.append(&heading(&gettext("Virements entre vos comptes")));
        titles.append(&caption(&gettext(
            "Un débit et un crédit du même montant sur deux de vos comptes, à 3 jours d’écart au plus. Confirmés, ils \
             ne comptent ni en dépenses ni en revenus.",
        )));
        header.append(&titles);
        if proposals.len() > 1 {
            let all = gtk::Button::builder()
                .label(gettext("Tout confirmer"))
                .valign(gtk::Align::Center)
                .build();
            let (w, pairs): (_, Vec<(i64, i64)>) = (
                self.window.clone(),
                proposals
                    .iter()
                    .map(|p| (p.debit.id, p.credit.id))
                    .collect(),
            );
            all.connect_clicked(move |_| {
                if let Some(window) = w.upgrade() {
                    let now = crate::window::now_iso();
                    let result = window.change(|c| {
                        for (d, k) in &pairs {
                            transfers::confirm(c, *d, *k, &now)?;
                        }
                        Ok(pairs.len())
                    });
                    match result {
                        Ok(n) => window.toast(
                            &ngettext("{n} virement confirmé", "{n} virements confirmés", n as u32)
                                .replace("{n}", &n.to_string()),
                        ),
                        Err(m) => window.toast(&m),
                    }
                }
            });
            header.append(&all);
        }
        let list = boxed_list();
        for p in proposals {
            let title = format!(
                "{} · {} → {}",
                amount(p.amount_cents, &p.currency, Sign::Negative),
                p.debit.account_name,
                p.credit.account_name
            );
            let subtitle = format!(
                "{} {}\n{} {}",
                dates::day_month_year(p.debit.booked_on),
                p.debit.label,
                dates::day_month_year(p.credit.booked_on),
                p.credit.label
            );
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&title))
                .subtitle(glib::markup_escape_text(&subtitle))
                .subtitle_lines(2)
                .build();
            let ignore = gtk::Button::builder()
                .label(gettext("Ce n’est pas un virement"))
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            let confirm = gtk::Button::builder()
                .label(gettext("Confirmer"))
                .valign(gtk::Align::Center)
                .css_classes(["suggested-action"])
                .build();
            let (w, d, k) = (self.window.clone(), p.debit.id, p.credit.id);
            ignore.connect_clicked(move |_| {
                if let Some(window) = w.upgrade() {
                    if let Err(m) = window.change(|c| transfers::reject(c, d, k)) {
                        window.toast(&m);
                    }
                }
            });
            let w = self.window.clone();
            confirm.connect_clicked(move |_| {
                if let Some(window) = w.upgrade() {
                    let now = crate::window::now_iso();
                    match window.change(|c| transfers::confirm(c, d, k, &now)) {
                        Ok(_) => window.toast(&gettext("Virement confirmé")),
                        Err(m) => window.toast(&m),
                    }
                }
            });
            row.add_suffix(&ignore);
            row.add_suffix(&confirm);
            list.append(&row);
        }
        if proposals.is_empty() {
            list.append(
                &adw::ActionRow::builder()
                    .title(gettext("Aucun virement à confirmer"))
                    .build(),
            );
        }
        section(header.upcast_ref(), &list)
    }

    fn household_section(
        self: &Rc<Self>,
        matches: &[transfers::HouseholdMatch],
        no_names: bool,
    ) -> gtk::Box {
        let header = gtk::Box::builder().spacing(12).build();
        let titles = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .build();
        titles.append(&heading(&gettext("Noms du foyer")));
        titles.append(&caption(&gettext(
            "Les virements venant d’un membre du foyer ne sont pas de vrais revenus. Finaryx propose de les classer \
             en « Virement interne ».",
        )));
        header.append(&titles);
        let edit = gtk::Button::builder()
            .label(gettext("Modifier les noms…"))
            .action_name("app.preferences")
            .valign(gtk::Align::Center)
            .build();
        header.append(&edit);
        let list = boxed_list();
        for m in matches {
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(
                    &ngettext(
                        "{n} opération contient « {name} »",
                        "{n} opérations contiennent « {name} »",
                        m.operations as u32,
                    )
                    .replace("{n}", &m.operations.to_string())
                    .replace("{name}", &m.name),
                ))
                .subtitle(
                    gettext("Crée la règle « le libellé contient {name} → Virement interne »")
                        .replace("{name}", &m.name),
                )
                .build();
            let apply = gtk::Button::builder()
                .label(gettext("Classer en virement interne"))
                .valign(gtk::Align::Center)
                .css_classes(["suggested-action"])
                .build();
            let (w, name) = (self.window.clone(), m.name.clone());
            apply.connect_clicked(move |_| {
                if let Some(window) = w.upgrade() {
                    match window.change(|c| {
                        categories::create_rule(c, &name, builtin::INTERNAL_TRANSFER, true)
                    }) {
                        Ok((_, n)) => window.toast(
                            &ngettext(
                                "Règle créée, {n} opération classée",
                                "Règle créée, {n} opérations classées",
                                n as u32,
                            )
                            .replace("{n}", &n.to_string()),
                        ),
                        Err(m) => window.toast(&m),
                    }
                }
            });
            row.add_suffix(&apply);
            list.append(&row);
        }
        if matches.is_empty() {
            list.append(
                &adw::ActionRow::builder()
                    .title(if no_names {
                        gettext("Aucun nom du foyer pour l’instant")
                    } else {
                        gettext("Aucune opération à classer")
                    })
                    .subtitle(if no_names {
                        gettext("Ajoutez-les dans les préférences, par exemple votre nom et celui de votre conjoint.")
                    } else {
                        gettext("Les opérations de ces noms sont déjà des virements internes.")
                    })
                    .subtitle_lines(2)
                    .build(),
            );
        }
        section(header.upcast_ref(), &list)
    }
}
