//! "Mois en détail" (design/Mois.dc.html): balance day by day with the
//! previous month, daily spending, ring and list by category with the
//! difference from the previous month, latest operations.

use std::cell::Cell;

use adw::prelude::*;
use chrono::{Datelike, NaiveDate};
use finaryx_core::db::Result;
use finaryx_core::money::Sign;
use finaryx_core::rusqlite::Connection;
use finaryx_core::stats::{self, OperationFilter, Scope};
use finaryx_core::{dates, scale};
use gettextrs::gettext;
use gtk::glib;

use super::overview::{month_donut, month_line, no_operations, part_row, section};
use super::{
    amount, amount_whole, boxed_list, capitalized, caption, chart_card, clear, legend_item, n_ops,
    pct1,
};
use crate::charts::{self, bars, LegendMark};
use crate::window::FinaryxWindow;

pub struct MonthPage {
    pub root: gtk::Box,
    /// Month navigation, placed in the filter row by the window.
    pub navigation: gtk::Box,
    previous: gtk::Button,
    next: gtk::Button,
    label: gtk::Label,
    window: glib::WeakRef<FinaryxWindow>,
    /// Month shown (`None`: the latest one with operations).
    pub month: Cell<Option<NaiveDate>>,
}

impl std::fmt::Debug for MonthPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MonthPage").finish_non_exhaustive()
    }
}

impl MonthPage {
    pub fn new(window: &FinaryxWindow) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .build();
        let previous = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .css_classes(["flat"])
            .build();
        let next = gtk::Button::builder()
            .icon_name("go-next-symbolic")
            .css_classes(["flat"])
            .build();
        let label = gtk::Label::builder()
            .css_classes(["heading"])
            .width_chars(14)
            .build();
        let navigation = gtk::Box::builder()
            .spacing(4)
            .halign(gtk::Align::End)
            .build();
        navigation.append(&previous);
        navigation.append(&label);
        navigation.append(&next);
        for (button, delta) in [(&previous, -1), (&next, 1)] {
            let weak = window.downgrade();
            button.connect_clicked(move |_| {
                if let Some(w) = weak.upgrade() {
                    w.shift_month(delta);
                }
            });
        }
        MonthPage {
            root,
            navigation,
            previous,
            next,
            label,
            window: window.downgrade(),
            month: Cell::new(None),
        }
    }

    /// Rebuilds the page; returns the subtitle.
    pub fn refresh(
        &self,
        conn: &Connection,
        scope: Option<&Scope>,
        narrow: bool,
    ) -> Result<String> {
        clear(&self.root);
        let Some(scope) = scope else {
            return Ok(String::new());
        };
        let (Some(latest), Some(earliest)) = (
            stats::latest_month(conn, scope)?,
            stats::earliest_month(conn, scope)?,
        ) else {
            self.navigation.set_visible(false);
            self.root.append(&no_operations(&self.window));
            return Ok(String::new());
        };
        let month = self.month.get().unwrap_or(latest).clamp(earliest, latest);
        self.month.set(Some(month));
        let prev = stats::previous_month(month);
        let cur = scope.currency.as_str();

        // Navigation between months.
        self.navigation.set_visible(true);
        self.label
            .set_label(&capitalized(&dates::month_year(month)));
        self.previous.set_sensitive(month > earliest);
        self.next.set_sensitive(month < latest);
        self.previous.set_tooltip_text(Some(
            &gettext("Mois précédent : {month}").replace("{month}", &dates::month_year(prev)),
        ));
        self.next.set_tooltip_text(Some(
            &gettext("Mois suivant : {month}")
                .replace("{month}", &dates::month_year(stats::next_month(month))),
        ));
        self.previous
            .update_property(&[gtk::accessible::Property::Label(
                &self.previous.tooltip_text().unwrap_or_default(),
            )]);
        self.next
            .update_property(&[gtk::accessible::Property::Label(
                &self.next.tooltip_text().unwrap_or_default(),
            )]);

        let totals = stats::month_totals(conn, scope, month)?;

        // Balance day by day, then spending of each day.
        let (chart, current, _) = month_line(
            conn,
            scope,
            month,
            if narrow { 200 } else { 282 },
            &[1, 5, 10, 15, 20, 25, 31],
        )?;
        let summary = match (current.balances.first(), current.balances.last()) {
            (Some(first), Some(last)) => gettext("{first} le 1er, {last} au {day} {month}")
                .replace("{first}", &amount_whole(*first, cur, Sign::Negative))
                .replace("{last}", &amount_whole(*last, cur, Sign::Negative))
                .replace("{day}", &current.balances.len().to_string())
                .replace("{month}", dates::month_name(month.month())),
            _ => String::new(),
        };
        let legend = super::legend(&[
            legend_item(
                LegendMark::Line,
                &gettext("Solde {of_month}")
                    .replace("{of_month}", &super::overview::of_month(month.month())),
            ),
            legend_item(
                LegendMark::Ghost,
                &capitalized(dates::month_name(prev.month())),
            ),
            legend_item(LegendMark::Bars, &gettext("Dépenses du jour")),
        ]);

        let max_day = current.spending.iter().copied().max().unwrap_or(0);
        let ticks = scale::nice_ticks(0, max_day.max(100), 3, 100);
        let peak_day = current
            .spending
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| **v)
            .map(|(i, _)| i);
        let peak_label = match peak_day {
            Some(i) if max_day > 0 => {
                let day = month.with_day(i as u32 + 1).unwrap();
                let filter = OperationFilter {
                    from: Some(day),
                    to: day.succ_opt(),
                    ..Default::default()
                };
                let biggest = stats::operations(conn, scope, &filter, 200, 0)?
                    .items
                    .into_iter()
                    .min_by_key(|o| o.amount_cents);
                biggest.map(|o| {
                    format!(
                        "{} {}",
                        capitalized(&o.label.to_lowercase()),
                        amount_whole(-o.amount_cents, cur, Sign::Negative)
                    )
                })
            }
            _ => None,
        };
        let tooltips = current
            .spending
            .iter()
            .enumerate()
            .map(|(i, v)| {
                format!(
                    "<b>{}</b>\n{}",
                    dates::day_month(month.with_day(i as u32 + 1).unwrap()),
                    gettext("{amount} dépensés")
                        .replace("{amount}", &amount(*v, cur, Sign::Negative))
                )
            })
            .collect();
        let description = match (&peak_label, peak_day) {
            (Some(p), Some(i)) => {
                gettext("Dépenses de chaque jour {of_month} ; le {day}, {peak} domine")
                    .replace("{of_month}", &super::overview::of_month(month.month()))
                    .replace("{day}", &(i + 1).to_string())
                    .replace("{peak}", p)
            }
            _ => gettext("Dépenses de chaque jour du mois"),
        };
        let daily_bars = bars::bars(
            bars::Bars {
                values: current.spending.clone(),
                slots: stats::days_in_month(month) as usize,
                ticks: ticks
                    .iter()
                    .map(|t| (*t, amount_whole(*t, cur, Sign::Negative)))
                    .collect(),
                peak_label,
                tooltips,
            },
            if narrow { 90 } else { 120 },
            &description,
        );
        let card = chart_card(&[
            caption(&summary).upcast_ref(),
            legend.upcast_ref(),
            chart.upcast_ref(),
            daily_bars.upcast_ref(),
        ]);
        self.root
            .append(&section(&gettext("Évolution du solde"), None, &card));

        // Ring and table by category (categories listed separately).
        let categories = stats::spending_by_category(conn, scope, month, stats::next_month(month))?;
        let previous = stats::spending_by_category(conn, scope, prev, month)?;
        let parts = stats::chart_parts(&categories);
        let prev_parts = stats::chart_parts(&previous);
        let weak = self.window.clone();
        let parts_for_click = parts.clone();
        let currency = scope.currency.clone();
        let ring = month_donut(
            &parts,
            &prev_parts,
            month,
            cur,
            if narrow { 220 } else { 300 },
            move |slot| {
                if slot == 8 {
                    if let (Some(w), Some(p)) =
                        (weak.upgrade(), parts_for_click.iter().find(|p| p.slot == 8))
                    {
                        super::show_others_dialog(&w, p, month, &currency);
                    }
                }
            },
        );
        let table = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .build();
        let header = gtk::Box::builder()
            .spacing(10)
            .css_classes(["caption", "dim-label"])
            .build();
        header.append(
            &gtk::Label::builder()
                .label(gettext("Catégorie"))
                .xalign(0.0)
                .hexpand(true)
                .margin_start(22)
                .build(),
        );
        header.append(&gtk::Label::builder().label(gettext("Montant")).build());
        header.append(
            &gtk::Label::builder()
                .label(gettext("Part"))
                .width_chars(7)
                .xalign(1.0)
                .build(),
        );
        if !narrow {
            header.append(
                &gtk::Label::builder()
                    .label(
                        gettext("vs {month}").replace("{month}", dates::month_short(prev.month())),
                    )
                    .width_chars(10)
                    .xalign(1.0)
                    .build(),
            );
        }
        table.append(&header);
        let total: i64 = categories.iter().map(|c| c.amount).sum();
        for c in &categories {
            let before = previous
                .iter()
                .find(|p| p.category_id == c.category_id)
                .map_or(0, |p| p.amount);
            let delta = amount(c.amount - before, cur, Sign::Always);
            let row = part_row(
                Some(c.slot.unwrap_or(8)),
                &c.name,
                &amount(c.amount, cur, Sign::Negative),
                &pct1(c.amount, total),
                (!narrow).then_some(delta.as_str()),
            );
            row.set_tooltip_text(Some(&n_ops(c.operations)));
            table.append(&row);
        }
        let body = gtk::Box::builder()
            .orientation(if narrow {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            })
            .spacing(if narrow { 16 } else { 40 })
            .build();
        body.append(&ring);
        body.append(&table);
        let card = chart_card(&[body.upcast_ref()]);
        self.root
            .append(&section(&gettext("Où part l’argent"), None, &card));

        // Latest operations.
        let filter = OperationFilter {
            from: Some(month),
            to: Some(stats::next_month(month)),
            ..Default::default()
        };
        let page = stats::operations(conn, scope, &filter, 6, 0)?;
        let list = boxed_list();
        for op in &page.items {
            list.append(&operation_row(op, cur, narrow));
        }
        if page.items.is_empty() {
            list.append(
                &adw::ActionRow::builder()
                    .title(gettext("Aucune opération ce mois-ci"))
                    .build(),
            );
        }
        let weak = self.window.clone();
        let see_all: Box<dyn Fn()> = Box::new(move || {
            if let Some(w) = weak.upgrade() {
                w.show_operations(w.selected_account(), Some(month), None);
            }
        });
        self.root.append(&section(
            &gettext("Dernières opérations"),
            Some((&gettext("Tout voir"), see_all)),
            &list,
        ));

        Ok(format!(
            "{} · {}",
            capitalized(&dates::month_year(month)),
            n_ops(totals.operations)
        ))
    }
}

/// A read-only operation row: date, label, account, category, amount.
pub fn operation_row(op: &stats::OperationItem, currency: &str, narrow: bool) -> gtk::ListBoxRow {
    let line = gtk::Box::builder()
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(16)
        .margin_end(16)
        .build();
    let date = caption(&dates::day_month(op.booked_on));
    date.set_width_request(52);
    date.set_wrap(false);
    line.append(&date);
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
    let account = match &op.bank_name {
        Some(bank) => format!("{} · {}", op.account_name, bank),
        None => op.account_name.clone(),
    };
    text.append(&caption(&account));
    line.append(&text);
    if !narrow {
        let cat = gtk::Box::builder()
            .spacing(6)
            .valign(gtk::Align::Center)
            .build();
        let slot = op.category_slot;
        cat.append(&charts::color_swatch(10, move |w| {
            charts::category_color(w, slot)
        }));
        cat.append(&caption(op.category_name.as_deref().unwrap_or("—")));
        line.append(&cat);
    }
    line.append(
        &gtk::Label::builder()
            .label(amount(op.amount_cents, currency, Sign::Negative))
            .css_classes(["heading", "numeric"])
            .valign(gtk::Align::Center)
            .build(),
    );
    gtk::ListBoxRow::builder()
        .child(&line)
        .activatable(false)
        .build()
}
