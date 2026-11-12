//! "Opérations" (design/Operations.dc.html), reached from "Tout voir":
//! search, account, period and category filters, total of the filtered
//! operations, category changed in place, "Afficher les N suivantes".

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use chrono::NaiveDate;
use finaryx_core::models::{Category, Id};
use finaryx_core::money::Sign;
use finaryx_core::stats::{self, OperationFilter};
use finaryx_core::{dates, repo};
use gettextrs::{gettext, ngettext};
use gtk::glib;

use super::{amount, boxed_list, capitalized, caption, clear, n_ops, pct1};
use crate::charts;
use crate::window::FinaryxWindow;

pub const TAG: &str = "operations";
const PAGE: usize = 50;

pub struct OperationsPage {
    pub page: adw::NavigationPage,
    window: glib::WeakRef<FinaryxWindow>,
    title: adw::WindowTitle,
    search: gtk::SearchEntry,
    account: gtk::DropDown,
    period: gtk::DropDown,
    chips: gtk::Box,
    filters: gtk::FlowBox,
    summary: gtk::Box,
    /// Narrow layout: one filter per line, compact rows.
    narrow: Cell<bool>,
    list_box: gtk::Box,
    /// Account ids of the account drop-down (index 0: all).
    account_ids: RefCell<Vec<Option<Id>>>,
    /// Months of the period drop-down (index 0: all).
    months: RefCell<Vec<Option<NaiveDate>>>,
    category: Cell<Option<Id>>,
    shown: Cell<usize>,
    updating: Cell<bool>,
}

impl std::fmt::Debug for OperationsPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationsPage").finish_non_exhaustive()
    }
}

impl OperationsPage {
    pub fn new(window: &FinaryxWindow) -> Rc<Self> {
        let title = adw::WindowTitle::new(&gettext("Opérations"), "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        let search = gtk::SearchEntry::builder()
            .placeholder_text(gettext("Rechercher un libellé"))
            .hexpand(true)
            .build();
        search.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Rechercher une opération",
        ))]);
        let account = gtk::DropDown::builder().build();
        account.update_property(&[gtk::accessible::Property::Label(&gettext("Compte"))]);
        let period = gtk::DropDown::builder().build();
        period.update_property(&[gtk::accessible::Property::Label(&gettext("Période"))]);
        let filters = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .min_children_per_line(1)
            .max_children_per_line(3)
            .column_spacing(10)
            .row_spacing(10)
            .build();
        filters.append(&search);
        filters.append(&account);
        filters.append(&period);
        let chips = gtk::Box::builder().spacing(6).margin_bottom(6).build();
        let chips_scroller = gtk::ScrolledWindow::builder()
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .child(&chips)
            .build();
        chips.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Filtrer par catégorie",
        ))]);
        let summary = gtk::Box::builder().spacing(12).build();
        let list_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(16)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();
        content.append(&filters);
        content.append(&chips_scroller);
        content.append(&summary);
        content.append(&list_box);
        content.append(&caption(&gettext(
            "Cliquer sur une catégorie change la catégorie de l’opération et propose d’en faire une règle pour les \
             libellés identiques.",
        )));
        let clamp = adw::Clamp::builder()
            .maximum_size(1040)
            .tightening_threshold(1040)
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
            .title(gettext("Opérations"))
            .child(&toolbar)
            .build();
        // Ctrl+F, or typing anywhere in the page, goes to the search.
        search.set_key_capture_widget(Some(&page));
        let shortcuts = gtk::ShortcutController::new();
        shortcuts.set_scope(gtk::ShortcutScope::Managed);
        let entry = search.clone();
        shortcuts.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string("<Control>f"),
            Some(gtk::CallbackAction::new(move |_, _| {
                entry.grab_focus();
                glib::Propagation::Stop
            })),
        ));
        page.add_controller(shortcuts);

        let this = Rc::new(OperationsPage {
            page,
            window: window.downgrade(),
            title,
            search,
            account,
            period,
            chips,
            filters,
            summary,
            narrow: Cell::new(false),
            list_box,
            account_ids: RefCell::default(),
            months: RefCell::default(),
            category: Cell::new(None),
            shown: Cell::new(PAGE),
            updating: Cell::new(false),
        });
        let weak = Rc::downgrade(&this);
        this.search.connect_search_changed(move |_| {
            if let Some(this) = weak.upgrade() {
                this.shown.set(PAGE);
                this.refresh_list();
            }
        });
        for dropdown in [&this.account, &this.period] {
            let weak = Rc::downgrade(&this);
            dropdown.connect_selected_notify(move |_| {
                if let Some(this) = weak.upgrade() {
                    if !this.updating.get() {
                        this.shown.set(PAGE);
                        this.refresh_list();
                    }
                }
            });
        }
        this
    }

    /// Fills the filters (account, month, category) and shows the list.
    pub fn setup(
        self: &Rc<Self>,
        account: Option<Id>,
        month: Option<NaiveDate>,
        category: Option<Id>,
    ) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        self.updating.set(true);
        let accounts = window
            .read(|c| repo::list_accounts(c, false))
            .unwrap_or_default();
        let mut names = vec![gettext("Tous les comptes")];
        let mut ids = vec![None];
        for a in &accounts {
            names.push(a.name.clone());
            ids.push(Some(a.id));
        }
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        self.account.set_model(Some(&gtk::StringList::new(&refs)));
        self.account
            .set_selected(ids.iter().position(|i| *i == account).unwrap_or(0) as u32);
        self.account_ids.replace(ids);

        // Months with operations, most recent first.
        let scope = window.read(|c| stats::scope(c, None)).flatten();
        let mut months: Vec<Option<NaiveDate>> = vec![None];
        if let Some(scope) = &scope {
            let range = window.read(|c| {
                Ok((
                    stats::earliest_month(c, scope)?,
                    stats::latest_month(c, scope)?,
                ))
            });
            if let Some((Some(first), Some(last))) = range {
                let mut m = last;
                while m >= first {
                    months.push(Some(m));
                    m = stats::previous_month(m);
                }
            }
        }
        let labels: Vec<String> = months
            .iter()
            .map(|m| match m {
                Some(m) => capitalized(&dates::month_year(*m)),
                None => gettext("Toute la période"),
            })
            .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.period.set_model(Some(&gtk::StringList::new(&refs)));
        self.period
            .set_selected(months.iter().position(|m| *m == month).unwrap_or(0) as u32);
        self.months.replace(months);
        self.category.set(category);
        self.updating.set(false);
        self.build_chips();
        self.refresh_list();
    }

    fn categories(&self) -> Vec<Category> {
        self.window
            .upgrade()
            .and_then(|w| w.read(repo::list_categories))
            .unwrap_or_default()
    }

    fn build_chips(self: &Rc<Self>) {
        clear(&self.chips);
        let mut first: Option<gtk::ToggleButton> = None;
        let mut entries: Vec<(Option<Id>, String, Option<u8>)> =
            vec![(None, gettext("Toutes"), None)];
        entries.extend(
            self.categories()
                .into_iter()
                .map(|c| (Some(c.id), c.name, c.slot)),
        );
        for (id, name, slot) in entries {
            let content = gtk::Box::builder().spacing(6).build();
            if id.is_some() {
                let s = slot;
                content.append(&charts::color_swatch(10, move |w| {
                    charts::category_color(w, s)
                }));
            }
            content.append(&gtk::Label::new(Some(&name)));
            let chip = gtk::ToggleButton::builder()
                .child(&content)
                .active(id == self.category.get())
                .css_classes(["pill", "chip"])
                .build();
            if let Some(f) = &first {
                chip.set_group(Some(f));
            } else {
                first = Some(chip.clone());
            }
            let weak = Rc::downgrade(self);
            chip.connect_toggled(move |b| {
                if let Some(this) = weak.upgrade() {
                    if b.is_active() && !this.updating.get() {
                        this.category.set(id);
                        this.shown.set(PAGE);
                        this.refresh_list();
                    }
                }
            });
            self.chips.append(&chip);
        }
    }

    fn filter(&self) -> (Option<Id>, OperationFilter, Option<NaiveDate>) {
        let account = self
            .account_ids
            .borrow()
            .get(self.account.selected() as usize)
            .copied()
            .flatten();
        let month = self
            .months
            .borrow()
            .get(self.period.selected() as usize)
            .copied()
            .flatten();
        let filter = OperationFilter {
            search: self.search.text().to_string(),
            category_id: self.category.get(),
            from: month,
            to: month.map(stats::next_month),
        };
        (account, filter, month)
    }

    /// Re-reads the operations with the current filters.
    pub fn set_narrow(self: &Rc<Self>, narrow: bool) {
        self.narrow.set(narrow);
        self.filters
            .set_max_children_per_line(if narrow { 1 } else { 3 });
        self.summary.set_orientation(if narrow {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
        self.summary.set_spacing(if narrow { 4 } else { 12 });
        self.refresh_list();
    }

    pub fn refresh_list(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let (account, filter, month) = self.filter();
        let shown = self.shown.get();
        let result = window.read(|c| {
            let Some(scope) = stats::scope(c, account)? else {
                return Ok(None);
            };
            let page = stats::operations(c, &scope, &filter, shown, 0)?;
            // Share of the spending, when a spending category is selected.
            let spending = match (filter.from, filter.to) {
                (Some(from), Some(to)) => stats::totals(c, &scope, from, to)?.spending,
                _ => stats::totals(c, &scope, NaiveDate::MIN, NaiveDate::MAX)?.spending,
            };
            Ok(Some((scope, page, spending)))
        });
        let Some(Some((scope, page, spending))) = result else {
            return;
        };
        let categories = self.categories();
        let cur = scope.currency.clone();

        // Summary line.
        clear(&self.summary);
        let period = match month {
            Some(m) => dates::month_year(m),
            None => gettext("toute la période"),
        };
        let mut parts = vec![n_ops(page.count)];
        if let Some(c) = categories
            .iter()
            .find(|c| Some(c.id) == self.category.get())
        {
            parts.push(c.name.clone());
        }
        parts.push(period.clone());
        let left = caption(&parts.join(" · "));
        left.set_hexpand(true);
        self.summary.append(&left);
        let narrow = self.narrow.get();
        let mut total_text = amount(page.total, &cur, Sign::Negative);
        let is_spending = categories
            .iter()
            .find(|c| Some(c.id) == self.category.get())
            .is_some_and(|c| !c.is_income && !c.exclude_from_stats);
        if is_spending && spending > 0 {
            total_text = format!(
                "{total_text} · {}",
                gettext("{pct} % des dépenses").replace("{pct}", &pct1(-page.total, spending))
            );
        }
        self.summary.append(
            &gtk::Label::builder()
                .label(total_text)
                .xalign(if narrow { 0.0 } else { 1.0 })
                .wrap(true)
                .css_classes(["heading", "numeric"])
                .build(),
        );
        self.title
            .set_subtitle(&format!("{} · {}", capitalized(&period), n_ops(page.count)));

        // The list.
        clear(&self.list_box);
        let list = boxed_list();
        for op in &page.items {
            list.append(&self.row(op, &categories, &cur));
        }
        if page.items.is_empty() {
            list.append(
                &adw::ActionRow::builder()
                    .title(gettext("Aucune opération ne correspond à ces filtres"))
                    .build(),
            );
        }
        self.list_box.append(&list);
        let remaining = (page.count as usize).saturating_sub(page.items.len());
        if remaining > 0 {
            let next = remaining.min(PAGE);
            let more = gtk::Button::builder()
                .label(
                    ngettext(
                        "Afficher la suivante",
                        "Afficher les {n} suivantes",
                        next as u32,
                    )
                    .replace("{n}", &next.to_string()),
                )
                .halign(gtk::Align::Center)
                .css_classes(["pill"])
                .build();
            let weak = Rc::downgrade(self);
            more.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.shown.set(this.shown.get() + PAGE);
                    this.refresh_list();
                }
            });
            self.list_box.append(&more);
        }
    }

    fn row(
        self: &Rc<Self>,
        op: &stats::OperationItem,
        categories: &[Category],
        currency: &str,
    ) -> gtk::ListBoxRow {
        let narrow = self.narrow.get();
        let line = gtk::Box::builder()
            .spacing(12)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(if narrow { 12 } else { 16 })
            .margin_end(12)
            .build();
        if !narrow {
            let date = caption(&dates::day_month(op.booked_on));
            date.set_width_request(52);
            date.set_wrap(false);
            line.append(&date);
        }
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        text.append(
            &gtk::Label::builder()
                .label(&op.label)
                .xalign(0.0)
                .wrap(true)
                .css_classes(["heading"])
                .build(),
        );
        let mut account = match &op.bank_name {
            Some(b) => format!("{} · {}", op.account_name, b),
            None => op.account_name.clone(),
        };
        if narrow {
            account = format!("{} · {}", dates::day_month(op.booked_on), op.account_name);
        }
        let sub = gtk::Box::builder().spacing(8).build();
        sub.append(&caption(&account));
        if op.needs_review {
            sub.append(
                &gtk::Label::builder()
                    .label(gettext("À vérifier"))
                    .css_classes(["status-badge", "late"])
                    .build(),
            );
        }
        text.append(&sub);
        line.append(&text);

        // Category button: a menu of all categories.
        let content = gtk::Box::builder().spacing(6).build();
        let slot = op.category_slot;
        content.append(&charts::color_swatch(10, move |w| {
            charts::category_color(w, slot)
        }));
        content.append(
            &gtk::Label::builder()
                .label(op.category_name.as_deref().unwrap_or("—"))
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .max_width_chars(18)
                .build(),
        );
        let popover_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["navigation-sidebar"])
            .build();
        for c in categories {
            let row_box = gtk::Box::builder().spacing(8).build();
            let s = c.slot;
            row_box.append(&charts::color_swatch(10, move |w| {
                charts::category_color(w, s)
            }));
            row_box.append(
                &gtk::Label::builder()
                    .label(&c.name)
                    .xalign(0.0)
                    .hexpand(true)
                    .build(),
            );
            let check = gtk::Image::from_icon_name("object-select-symbolic");
            check.set_opacity(if Some(c.id) == op.category_id {
                1.0
            } else {
                0.0
            });
            row_box.append(&check);
            let row = gtk::ListBoxRow::builder().child(&row_box).build();
            row.set_widget_name(&c.id.to_string());
            popover_list.append(&row);
        }
        let new_row = gtk::ListBoxRow::builder()
            .child(
                &gtk::Label::builder()
                    .label(gettext("Nouvelle catégorie…"))
                    .xalign(0.0)
                    .build(),
            )
            .build();
        new_row.set_widget_name("new");
        popover_list.append(&new_row);
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(360)
            .child(&popover_list)
            .build();
        let popover = gtk::Popover::builder().child(&scroller).build();
        let button = gtk::MenuButton::builder()
            .child(&content)
            .popover(&popover)
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(
            &gettext("Changer la catégorie de {label} (actuellement {category})")
                .replace("{label}", &op.label)
                .replace("{category}", op.category_name.as_deref().unwrap_or("—")),
        )]);
        let weak = Rc::downgrade(self);
        let txn = op.id;
        let pop = popover.clone();
        let label = op.label.clone();
        popover_list.connect_row_activated(move |_, row| {
            pop.popdown();
            let Some(this) = weak.upgrade() else { return };
            let Some(window) = this.window.upgrade() else {
                return;
            };
            if row.widget_name() == "new" {
                // Create a category, then give it to the operation.
                let weak = Rc::downgrade(&this);
                let label = label.clone();
                crate::category_dialog::present(&window, None, move |category| {
                    if let Some(this) = weak.upgrade() {
                        this.apply_category(txn, &label, category);
                    }
                });
                return;
            }
            if let Ok(category) = row.widget_name().parse::<Id>() {
                this.apply_category(txn, &label, category);
            }
        });
        if narrow {
            // Under the label, aligned with its text.
            button.set_halign(gtk::Align::Start);
            text.append(&button);
        } else {
            line.append(&button);
        }
        line.append(
            &gtk::Label::builder()
                .label(amount(op.amount_cents, currency, Sign::Negative))
                .width_chars(if narrow { 0 } else { 11 })
                .xalign(1.0)
                .css_classes(["heading", "numeric"])
                .valign(if narrow {
                    gtk::Align::Start
                } else {
                    gtk::Align::Center
                })
                .build(),
        );
        gtk::ListBoxRow::builder()
            .child(&line)
            .activatable(false)
            .build()
    }
}

impl OperationsPage {
    /// Sets the category of an operation by hand, then proposes a rule for
    /// the same labels ("Créer une règle ?").
    fn apply_category(self: &Rc<Self>, txn: Id, label: &str, category: Id) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        if let Err(message) = window.change(|c| stats::set_category(c, txn, category)) {
            window.toast(&message);
            return;
        }
        let categories = self.categories();
        let name = categories
            .iter()
            .find(|c| c.id == category)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let pattern = finaryx_core::categories::pattern_from_label(label);
        let changeable = window
            .read(|c| {
                Ok(finaryx_core::categories::matching_operations(
                    c,
                    &pattern,
                    false,
                    Some(category),
                )?
                .1
                .len())
            })
            .unwrap_or(0);
        let alert = adw::AlertDialog::builder()
            .heading(gettext("Créer une règle ?"))
            .body(
                gettext(
                    "Les libellés qui contiennent <b>{pattern}</b> seront classés en <b>{category}</b> lors des prochains imports.",
                )
                .replace("{pattern}", &glib::markup_escape_text(&pattern))
                .replace("{category}", &glib::markup_escape_text(&name)),
            )
            .body_use_markup(true)
            .build();
        let apply = adw::SwitchRow::builder()
            .title(gettext("Appliquer aux opérations existantes"))
            .subtitle(
                ngettext(
                    "{n} opération sera modifiée, sauf celles corrigées à la main.",
                    "{n} opérations seront modifiées, sauf celles corrigées à la main.",
                    changeable as u32,
                )
                .replace("{n}", &changeable.to_string()),
            )
            .active(true)
            .build();
        if changeable > 0 {
            let list = boxed_list();
            list.append(&apply);
            alert.set_extra_child(Some(&list));
        }
        alert.add_responses(&[
            ("only", &gettext("Seulement cette opération")),
            ("rule", &gettext("Créer la règle")),
        ]);
        alert.set_response_appearance("rule", adw::ResponseAppearance::Suggested);
        alert.set_default_response(Some("rule"));
        alert.set_close_response("only");
        let w = self.window.clone();
        alert.connect_response(None, move |_, response| {
            let Some(window) = w.upgrade() else { return };
            if response == "rule" {
                let all = changeable > 0 && apply.is_active();
                match window
                    .change(|c| finaryx_core::categories::create_rule(c, &pattern, category, all))
                {
                    Ok((_, n)) if n > 0 => window.toast(
                        &ngettext(
                            "Règle créée, {n} autre opération classée",
                            "Règle créée, {n} autres opérations classées",
                            n as u32,
                        )
                        .replace("{n}", &n.to_string()),
                    ),
                    Ok(_) => window.toast(&gettext("Règle créée")),
                    Err(m) => window.toast(&m),
                }
            } else {
                window.toast(&gettext("Catégorie modifiée"));
            }
        });
        alert.present(Some(&window));
    }
}
