//! Account detail (design/CompteDetail.dc.html), pushed from the accounts
//! page with a back button.

use std::rc::Rc;

use adw::prelude::*;
use chrono::Datelike;
use finaryx_core::accounts::{self, AccountOverview};
use finaryx_core::db::Result;
use finaryx_core::models::{AccountKind, Id};
use finaryx_core::money::Sign;
use finaryx_core::rusqlite::Connection;
use finaryx_core::{dates, scale};
use gettextrs::{gettext, ngettext};

use super::{amount, amount_whole, boxed_list, caption, clear, heading, section};
use crate::account_dialog::Field;
use crate::charts::{self, line};

#[derive(Clone)]
pub struct Callbacks {
    pub edit: Rc<dyn Fn(Id, Field)>,
    pub set_archived: Rc<dyn Fn(Id, bool)>,
    pub delete: Rc<dyn Fn(Id)>,
    pub see_all: Rc<dyn Fn(Id)>,
}

impl std::fmt::Debug for AccountDetailPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountDetailPage").finish_non_exhaustive()
    }
}

pub struct AccountDetailPage {
    pub page: adw::NavigationPage,
    pub account_id: Id,
    title: adw::WindowTitle,
    content: gtk::Box,
    callbacks: Callbacks,
}

pub const TAG: &str = "account";

impl AccountDetailPage {
    pub fn new(account_id: Id, callbacks: Callbacks) -> Self {
        let title = adw::WindowTitle::new("", "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
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
            .child(&clamp)
            .vexpand(true)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&scrolled));
        let page = adw::NavigationPage::builder()
            .tag(TAG)
            .title(gettext("Compte"))
            .child(&toolbar)
            .build();
        AccountDetailPage {
            page,
            account_id,
            title,
            content,
            callbacks,
        }
    }

    /// Rebuilds the page. Returns `false` if the account no longer exists.
    pub fn refresh(&self, conn: &Connection) -> Result<bool> {
        let Some(o) = accounts::overviews(conn, true)?
            .into_iter()
            .find(|o| o.account.id == self.account_id)
        else {
            return Ok(false);
        };
        clear(&self.content);
        let a = &o.account;
        self.page.set_title(&a.name);
        self.title.set_title(&a.name);
        self.title
            .set_subtitle(o.bank_name.as_deref().unwrap_or(""));

        self.content.append(&self.balance_card(conn, &o)?);
        self.content.append(&self.settings(&o));
        self.content.append(&self.recent(conn, &o)?);
        self.content.append(&self.danger_zone(&o));
        Ok(true)
    }

    fn balance_card(&self, conn: &Connection, o: &AccountOverview) -> Result<gtk::Box> {
        let a = &o.account;
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(14)
            .css_classes(["card", "padded"])
            .build();
        let top = gtk::Box::builder().spacing(14).build();
        top.append(
            &adw::Avatar::builder()
                .size(48)
                .text(o.bank_name.as_deref().unwrap_or(&a.name))
                .show_initials(true)
                .build(),
        );
        let main = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        let as_of = match o.last_operation {
            Some(d) => gettext("Solde au {date}").replace("{date}", &dates::day_month_year_long(d)),
            None => gettext("Solde"),
        };
        main.append(&caption(&as_of));
        main.append(
            &gtk::Label::builder()
                .label(amount(o.balance_cents, &a.currency, Sign::Negative))
                .xalign(0.0)
                .css_classes(["amount-large"])
                .build(),
        );
        top.append(&main);

        let side = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .build();
        if let Some(last) = o.last_operation {
            let change = accounts::month_change(conn, a.id, last)?;
            let text = gettext("{amount} en {month}")
                .replace("{amount}", &amount(change, &a.currency, Sign::Always))
                .replace("{month}", dates::month_name(last.month()));
            let l = caption(&text);
            l.set_xalign(1.0);
            side.append(&l);
        }
        let count = ngettext(
            "{n} opération enregistrée",
            "{n} opérations enregistrées",
            o.operation_count.clamp(0, u32::MAX as i64) as u32,
        )
        .replace("{n}", &o.operation_count.to_string());
        let l = caption(&count);
        l.set_xalign(1.0);
        side.append(&l);
        top.append(&side);
        card.append(&top);

        // Weekly balance over the last 6 months.
        if let (Some(first), Some(last)) = (o.first_operation, o.last_operation) {
            let from = accounts::months_back(last, 6).max(first);
            let points = accounts::weekly_balances(conn, a.id, from, last)?;
            if points.len() >= 2 {
                let values: Vec<i64> = points.iter().map(|p| p.1).collect();
                let unit = 10i64.pow(
                    finaryx_core::money::Currency::from_code(&a.currency).map_or(2, |c| c.exponent),
                );
                let ticks = scale::nice_ticks(
                    *values.iter().min().unwrap(),
                    *values.iter().max().unwrap(),
                    3,
                    unit,
                );
                let mut x_labels = Vec::new();
                let mut last_month = 0;
                for (i, (d, _)) in points.iter().enumerate() {
                    if d.month() != last_month {
                        last_month = d.month();
                        x_labels.push((i, dates::month_short(d.month()).to_string()));
                    }
                }
                // Every other month, plus the last one (as in the mockup).
                let n = x_labels.len();
                let x_labels: Vec<_> = x_labels
                    .into_iter()
                    .enumerate()
                    .filter(|(k, _)| k % 2 == 0 || *k == n - 1)
                    .map(|(_, l)| l)
                    .collect();
                let chart = line::LineChart {
                    tooltips: points
                        .iter()
                        .map(|(d, v)| {
                            format!(
                                "{} : {}",
                                dates::day_month_year(*d),
                                amount(*v, &a.currency, Sign::Negative)
                            )
                        })
                        .collect(),
                    ticks: ticks
                        .iter()
                        .map(|t| (*t, amount_whole(*t, &a.currency, Sign::Negative)))
                        .collect(),
                    x_labels,
                    values: values.clone(),
                    ..Default::default()
                };
                let description =
                    gettext("Solde du compte sur 6 mois : de {start} en {from} à {end} en {to}")
                        .replace(
                            "{start}",
                            &amount_whole(values[0], &a.currency, Sign::Negative),
                        )
                        .replace("{from}", dates::month_name(points[0].0.month()))
                        .replace(
                            "{end}",
                            &amount_whole(*values.last().unwrap(), &a.currency, Sign::Negative),
                        )
                        .replace("{to}", dates::month_name(last.month()));
                card.append(&line::line_chart(chart, 150 + 22, &description));
            }
        }
        Ok(card)
    }

    fn settings(&self, o: &AccountOverview) -> gtk::Box {
        let a = &o.account;
        let list = boxed_list();
        let kind = match a.kind {
            AccountKind::Checking => gettext("Compte courant"),
            AccountKind::Savings => gettext("Épargne"),
        };
        let rows = [
            (gettext("Nom"), a.name.clone(), Field::Name),
            (
                gettext("Banque"),
                o.bank_name.clone().unwrap_or_else(|| gettext("Aucune")),
                Field::Bank,
            ),
            (gettext("Type"), kind, Field::Kind),
            (gettext("Devise"), a.currency.clone(), Field::Currency),
            (
                gettext("Solde avant la première opération importée"),
                amount(a.opening_balance_cents, &a.currency, Sign::Negative),
                Field::OpeningBalance,
            ),
        ];
        for (title, value, field) in rows {
            let row = adw::ActionRow::builder()
                .title(&title)
                .subtitle(&value)
                .build();
            let button = gtk::Button::builder()
                .label(gettext("Modifier"))
                .valign(gtk::Align::Center)
                .build();
            button.update_property(&[gtk::accessible::Property::Label(
                &gettext("Modifier : {field}").replace("{field}", &title),
            )]);
            let edit = self.callbacks.edit.clone();
            let id = a.id;
            button.connect_clicked(move |_| edit(id, field));
            row.add_suffix(&button);
            list.append(&row);
        }
        section(heading(&gettext("Réglages du compte")).upcast_ref(), &list)
    }

    fn recent(&self, conn: &Connection, o: &AccountOverview) -> Result<gtk::Box> {
        let a = &o.account;
        let header = gtk::Box::builder().spacing(12).build();
        let title = heading(&gettext("Dernières opérations"));
        title.set_hexpand(true);
        header.append(&title);
        // Label then chevron, as a link to the full list.
        let see_all_content = gtk::Box::builder().spacing(4).build();
        see_all_content.append(&gtk::Label::new(Some(&gettext("Tout voir"))));
        see_all_content.append(&gtk::Image::from_icon_name("go-next-symbolic"));
        let see_all = gtk::Button::builder()
            .child(&see_all_content)
            .css_classes(["flat"])
            .build();
        let callback = self.callbacks.see_all.clone();
        let id = a.id;
        see_all.connect_clicked(move |_| callback(id));
        header.append(&see_all);

        let list = boxed_list();
        let ops = accounts::recent_operations(conn, a.id, 5)?;
        if ops.is_empty() {
            list.append(
                &adw::ActionRow::builder()
                    .title(gettext("Aucune opération"))
                    .build(),
            );
        }
        for op in ops {
            let row = gtk::Box::builder()
                .spacing(12)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(16)
                .margin_end(16)
                .build();
            let date = caption(&dates::day_month(op.booked_on));
            date.set_width_request(56);
            date.set_wrap(false);
            row.append(&date);

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
            let cat = gtk::Box::builder().spacing(6).build();
            let slot = op.category_slot;
            cat.append(&charts::color_swatch(10, move |w| {
                charts::category_color(w, slot)
            }));
            cat.append(&caption(
                op.category_name
                    .as_deref()
                    .unwrap_or(&gettext("Sans catégorie")),
            ));
            if op.needs_review {
                cat.append(
                    &gtk::Label::builder()
                        .label(gettext("À vérifier"))
                        .css_classes(["status-badge", "late"])
                        .build(),
                );
            }
            text.append(&cat);
            row.append(&text);
            row.append(
                &gtk::Label::builder()
                    .label(amount(op.amount_cents, &a.currency, Sign::Negative))
                    .css_classes(["heading", "numeric"])
                    .valign(gtk::Align::Center)
                    .build(),
            );
            list.append(
                &gtk::ListBoxRow::builder()
                    .child(&row)
                    .activatable(false)
                    .build(),
            );
        }
        Ok(section(header.upcast_ref(), &list))
    }

    fn danger_zone(&self, o: &AccountOverview) -> gtk::Box {
        let a = &o.account;
        let list = boxed_list();
        let (title, subtitle, label) = if a.archived {
            (
                gettext("Désarchiver le compte"),
                gettext("Il revient dans les totaux."),
                gettext("Désarchiver"),
            )
        } else {
            (
                gettext("Archiver le compte"),
                gettext("Il disparaît des totaux, ses opérations restent consultables."),
                gettext("Archiver"),
            )
        };
        let archive_row = adw::ActionRow::builder()
            .title(&title)
            .subtitle(&subtitle)
            .build();
        let archive = gtk::Button::builder()
            .label(&label)
            .valign(gtk::Align::Center)
            .build();
        let callback = self.callbacks.set_archived.clone();
        let (id, archived) = (a.id, a.archived);
        archive.connect_clicked(move |_| callback(id, !archived));
        archive_row.add_suffix(&archive);
        list.append(&archive_row);

        let count = o.operation_count.clamp(0, u32::MAX as i64) as u32;
        let delete_row = adw::ActionRow::builder()
            .title(gettext("Supprimer le compte et ses opérations"))
            .subtitle(
                ngettext(
                    "Action définitive, {n} opération serait effacée.",
                    "Action définitive, {n} opérations seraient effacées.",
                    count,
                )
                .replace("{n}", &count.to_string()),
            )
            .build();
        let delete = gtk::Button::builder()
            .label(gettext("Supprimer…"))
            .valign(gtk::Align::Center)
            .css_classes(["destructive-action"])
            .build();
        let callback = self.callbacks.delete.clone();
        delete.connect_clicked(move |_| callback(id));
        delete_row.add_suffix(&delete);
        list.append(&delete_row);
        section(heading(&gettext("Zone sensible")).upcast_ref(), &list)
    }
}
