//! "Comptes" page (design/Comptes.dc.html): total with the share of each
//! account, list of the accounts, import history.

use std::rc::Rc;

use adw::prelude::*;
use chrono::NaiveDate;
use finaryx_core::accounts::{self, AccountOverview, ImportStatus};
use finaryx_core::dates;
use finaryx_core::db::Result;
use finaryx_core::models::{AccountKind, Id};
use finaryx_core::money::Sign;
use finaryx_core::rusqlite::Connection;
use gettextrs::{gettext, ngettext};

use super::{
    amount, amount_whole, boxed_list, caption, clear, heading, percent_per_mille, section,
};
use crate::charts::{self, share_bar, sparkline};

/// What the page asks the window to do.
#[derive(Clone)]
pub struct Callbacks {
    pub open_account: Rc<dyn Fn(Id)>,
    pub undo_import: Rc<dyn Fn(Id)>,
}

impl std::fmt::Debug for AccountsPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountsPage").finish_non_exhaustive()
    }
}

pub struct AccountsPage {
    /// Content of the page (inside the scrolled, clamped page body).
    pub root: gtk::Box,
    callbacks: Callbacks,
    /// Narrow layout (design/EtroitComptes.dc.html): no mini curve, the
    /// import state under the balance, condensed history.
    narrow: std::cell::Cell<bool>,
}

fn n_text(singular: &str, plural: &str, n: i64) -> String {
    ngettext(singular, plural, n.clamp(0, u32::MAX as i64) as u32).replace("{n}", &n.to_string())
}

/// "Banque Populaire · compte courant · ••7305"
fn account_meta(o: &AccountOverview) -> String {
    let mut parts = Vec::new();
    if let Some(bank) = &o.bank_name {
        parts.push(bank.clone());
    }
    parts.push(match o.account.kind {
        AccountKind::Checking => gettext("compte courant"),
        AccountKind::Savings => gettext("épargne"),
    });
    if let Some(number) = &o.account.number {
        parts.push(accounts::masked_number(number));
    }
    if o.account.currency != "EUR" {
        parts.push(o.account.currency.clone());
    }
    parts.join(" · ")
}

impl AccountsPage {
    pub fn new(callbacks: Callbacks) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(28)
            .build();
        AccountsPage {
            root,
            callbacks,
            narrow: Default::default(),
        }
    }

    /// Switches between the wide and narrow layouts; call `refresh` after.
    pub fn set_narrow(&self, narrow: bool) {
        self.narrow.set(narrow);
        self.root.set_spacing(if narrow { 20 } else { 28 });
    }

    /// Rebuilds the page from the database.
    pub fn refresh(&self, conn: &Connection, today: NaiveDate) -> Result<()> {
        clear(&self.root);
        let all = accounts::overviews(conn, true)?;
        let (active, archived): (Vec<_>, Vec<_>) =
            all.into_iter().partition(|o| !o.account.archived);

        // One total card per currency (never converted).
        let mut currencies: Vec<String> = Vec::new();
        for o in &active {
            if !currencies.contains(&o.account.currency) {
                currencies.push(o.account.currency.clone());
            }
        }
        for currency in &currencies {
            let group: Vec<&AccountOverview> = active
                .iter()
                .filter(|o| &o.account.currency == currency)
                .collect();
            let title = if currencies.len() == 1 {
                gettext("Solde de tous les comptes")
            } else {
                gettext("Solde des comptes en {currency}").replace("{currency}", currency)
            };
            self.root.append(&total_card(&title, currency, &group));
        }

        if !active.is_empty() {
            let list = self.account_list();
            for o in &active {
                list.append(&self.account_row(conn, o, today)?);
            }
            self.root.append(&section(
                heading(&gettext("Vos comptes")).upcast_ref(),
                &list,
            ));
        }
        if !archived.is_empty() {
            let list = self.account_list();
            for o in &archived {
                list.append(&self.account_row(conn, o, today)?);
            }
            let title = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .build();
            title.append(&heading(&gettext("Comptes archivés")));
            title.append(&caption(&gettext("Ils ne comptent pas dans les totaux.")));
            self.root.append(&section(title.upcast_ref(), &list));
        }
        self.root.append(&self.history(conn)?);
        Ok(())
    }

    /// A list of account rows that opens the account on activation.
    fn account_list(&self) -> gtk::ListBox {
        let list = boxed_list();
        let open = self.callbacks.open_account.clone();
        list.connect_row_activated(move |_, row| {
            if let Ok(id) = row.widget_name().parse::<Id>() {
                open(id);
            }
        });
        list
    }

    fn account_row(
        &self,
        conn: &Connection,
        o: &AccountOverview,
        today: NaiveDate,
    ) -> Result<gtk::ListBoxRow> {
        let a = &o.account;
        let narrow = self.narrow.get();
        let row_box = gtk::Box::builder()
            .spacing(if narrow { 12 } else { 14 })
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(if narrow { 14 } else { 16 })
            .margin_end(if narrow { 14 } else { 16 })
            .build();

        let avatar = adw::Avatar::builder()
            .size(40)
            .text(o.bank_name.as_deref().unwrap_or(&a.name))
            .show_initials(true)
            .build();
        row_box.append(&avatar);

        // Name, bank · type · number, import badge.
        let text = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        text.append(
            &gtk::Label::builder()
                .label(&a.name)
                .xalign(0.0)
                .wrap(true)
                .build(),
        );
        let ops = n_text("{n} opération", "{n} opérations", o.operation_count);
        if narrow {
            let bank = o.bank_name.clone().unwrap_or_else(|| account_meta(o));
            text.append(&caption(&format!("{bank} · {ops}")));
        } else {
            text.append(&caption(&account_meta(o)));
        }
        let status = accounts::import_status(
            today,
            o.last_operation,
            o.last_import
                .as_ref()
                .and_then(|b| dates::parse_iso_date(&b.imported_at)),
        );
        let badge = gtk::Label::builder()
            .label(accounts::import_status_label(status))
            .css_classes(["status-badge"])
            .build();
        if !matches!(status, ImportStatus::UpToDate) {
            badge.add_css_class("late");
        }
        let last = match o
            .last_import
            .as_ref()
            .and_then(|b| dates::parse_iso_date(&b.imported_at))
        {
            Some(date) if status == ImportStatus::UpToDate => {
                gettext("Importé le {date}").replace("{date}", &dates::day_month_ordinal(date))
            }
            Some(date) => gettext("Dernier import le {date}")
                .replace("{date}", &dates::day_month_ordinal(date)),
            None => gettext("Ajoutez un export CSV pour voir ses opérations"),
        };
        let status_line = gtk::Box::builder().spacing(8).margin_top(4).build();
        status_line.append(&badge);
        status_line.append(&caption(&last));
        if !narrow {
            text.append(&status_line);
        }
        row_box.append(&text);

        // Mini curve of the last 6 month-end balances.
        let last_day = o.last_operation.unwrap_or(today);
        let series: Vec<i64> = accounts::month_end_balances(conn, a.id, last_day, 6)?
            .into_iter()
            .map(|(_, b)| b)
            .collect();
        if o.operation_count > 0 && !narrow {
            let delta = series.last().unwrap() - series.first().unwrap();
            let trend = if delta >= 0 {
                gettext("Évolution du solde sur 6 mois : en hausse de {amount}")
            } else {
                gettext("Évolution du solde sur 6 mois : en baisse de {amount}")
            }
            .replace(
                "{amount}",
                &amount_whole(delta.abs(), &a.currency, Sign::Negative),
            );
            let spark = sparkline::sparkline(series, &trend);
            spark.set_tooltip_text(Some(&trend));
            row_box.append(&spark);
        }

        // Balance and number of operations.
        let figures = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(if narrow { -1 } else { 112 })
            .valign(gtk::Align::Center)
            .build();
        let balance = amount(o.balance_cents, &a.currency, Sign::Negative);
        figures.append(
            &gtk::Label::builder()
                .label(&balance)
                .xalign(1.0)
                .css_classes(["heading", "numeric"])
                .build(),
        );
        let under_balance = if narrow {
            accounts::import_status_label(status)
        } else {
            ops
        };
        let under = caption(&under_balance);
        under.set_xalign(1.0);
        under.set_wrap(false);
        figures.append(&under);
        row_box.append(&figures);

        row_box.append(
            &gtk::Image::builder()
                .icon_name("go-next-symbolic")
                .css_classes(["dim-label"])
                .build(),
        );

        let row = gtk::ListBoxRow::builder()
            .child(&row_box)
            .activatable(true)
            .build();
        row.update_property(&[gtk::accessible::Property::Label(&format!(
            "{}, {}",
            a.name, balance
        ))]);
        row.set_widget_name(&a.id.to_string());
        Ok(row)
    }

    fn history(&self, conn: &Connection) -> Result<gtk::Box> {
        let entries = accounts::import_history(conn)?;
        let header = gtk::Box::builder().spacing(12).build();
        let titles = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        titles.append(&heading(&gettext("Historique des imports")));
        titles.append(&caption(&gettext(
            "Les doublons ne sont jamais enregistrés deux fois",
        )));
        header.append(&titles);
        let import = gtk::Button::builder()
            .action_name("win.import")
            .valign(gtk::Align::Center)
            .child(
                &adw::ButtonContent::builder()
                    .icon_name("finaryx-import-symbolic")
                    .label(gettext("Importer un CSV…"))
                    .build(),
            )
            .build();
        if self.narrow.get() {
            import.set_child(None::<&gtk::Widget>);
            import.set_icon_name("finaryx-import-symbolic");
            import.set_tooltip_text(Some(&gettext("Importer un CSV")));
            import.add_css_class("flat");
        }
        header.append(&import);

        let list = boxed_list();
        if entries.is_empty() {
            list.append(
                &adw::ActionRow::builder()
                    .title(gettext("Aucun import pour l’instant"))
                    .css_classes(["dim-label"])
                    .build(),
            );
        }
        for e in entries {
            let row = gtk::Box::builder()
                .spacing(14)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(16)
                .margin_end(10)
                .build();
            let date = dates::parse_iso_date(&e.batch.imported_at)
                .map(dates::day_month)
                .unwrap_or_default();
            let narrow = self.narrow.get();
            if !narrow {
                let date_label = caption(&date);
                date_label.set_width_request(52);
                date_label.set_wrap(false);
                row.append(&date_label);
            }

            let names = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .hexpand(true)
                .valign(gtk::Align::Center)
                .build();
            names.append(
                &gtk::Label::builder()
                    .label(e.batch.filename.as_deref().unwrap_or("—"))
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .build(),
            );
            let account = match &e.bank_name {
                Some(bank) => format!("{} · {}", e.account_name, bank),
                None => e.account_name.clone(),
            };
            names.append(&caption(&if narrow {
                format!("{date} · {account}")
            } else {
                account
            }));
            row.append(&names);

            if narrow {
                let new = caption(&n_text("{n} nouvelle", "{n} nouvelles", e.batch.rows_new));
                new.set_wrap(false);
                new.set_valign(gtk::Align::Center);
                row.append(&new);
            }
            let counts = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .valign(gtk::Align::Center)
                .build();
            let added = gtk::Label::builder()
                .label(n_text("{n} ajoutée", "{n} ajoutées", e.batch.rows_new))
                .xalign(1.0)
                .css_classes(["heading", "numeric"])
                .build();
            let dups = caption(&n_text(
                "{n} doublon",
                "{n} doublons",
                e.batch.rows_duplicate,
            ));
            dups.set_xalign(1.0);
            counts.append(&added);
            counts.append(&dups);
            if !narrow {
                row.append(&counts);
            }

            let undo = gtk::Button::builder()
                .icon_name("edit-undo-symbolic")
                .tooltip_text(gettext("Annuler cet import"))
                .valign(gtk::Align::Center)
                .sensitive(e.remaining > 0)
                .css_classes(["flat"])
                .build();
            let callback = self.callbacks.undo_import.clone();
            let batch = e.batch.id;
            undo.connect_clicked(move |_| callback(batch));
            row.append(&undo);
            list.append(
                &gtk::ListBoxRow::builder()
                    .child(&row)
                    .activatable(false)
                    .build(),
            );
        }
        Ok(section(header.upcast_ref(), &list))
    }
}

/// Card with the total balance and the share of each account.
fn total_card(title: &str, currency: &str, group: &[&AccountOverview]) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .css_classes(["card", "padded"])
        .build();
    let total: i64 = group.iter().map(|o| o.balance_cents).sum();
    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    head.append(&caption(title));
    head.append(
        &gtk::Label::builder()
            .label(amount(total, currency, Sign::Negative))
            .xalign(0.0)
            .css_classes(["amount-large"])
            .build(),
    );
    card.append(&head);

    let balances: Vec<i64> = group.iter().map(|o| o.balance_cents).collect();
    let shares = accounts::shares_per_mille(&balances);
    let segments: Vec<share_bar::Segment> = group
        .iter()
        .zip(&shares)
        .map(|(o, s)| share_bar::Segment {
            per_mille: *s,
            color_slot: o.account.color_slot,
            tooltip: format!(
                "{} : {} ({})",
                o.account.name,
                amount(o.balance_cents, currency, Sign::Negative),
                percent_per_mille(*s)
            ),
        })
        .collect();
    let description = group
        .iter()
        .zip(&shares)
        .map(|(o, s)| format!("{} {}", o.account.name, percent_per_mille(*s)))
        .collect::<Vec<_>>()
        .join(", ");
    card.append(&share_bar::share_bar(
        segments,
        &gettext("Part de chaque compte dans le solde total : {list}")
            .replace("{list}", &description),
    ));

    // Legend: colour, name and share are always written.
    let legend = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(24)
        .row_spacing(6)
        .max_children_per_line(8)
        .build();
    for (o, s) in group.iter().zip(&shares) {
        let item = gtk::Box::builder().spacing(8).build();
        let slot = o.account.color_slot;
        item.append(&charts::color_swatch(12, move |w| {
            charts::account_color(w, slot)
        }));
        item.append(&gtk::Label::new(Some(&o.account.name)));
        let pct = gtk::Label::builder()
            .label(percent_per_mille(*s))
            .css_classes(["dim-label", "numeric"])
            .build();
        item.append(&pct);
        legend.append(&item);
    }
    card.append(&legend);
    card
}
