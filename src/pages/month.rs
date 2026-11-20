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
use gettextrs::{gettext, ngettext};
use gtk::glib;

use super::overview::{month_donut, no_operations, part_row, section};
use super::{
    amount, amount_whole, boxed_list, capitalized, caption, chart_card, clear, legend_item, n_ops,
    pct1,
};
use crate::charts::{self, line, LegendMark};
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
            &gettext("Mois précédent : {month}").replace("{month}", &dates::month_year(prev)),
        ));
        self.next.set_tooltip_text(Some(
            &gettext("Mois suivant : {month}")
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

        // Spending since the 1st, against the previous month and the
        // average month at the same date.
        let last = stats::last_operation(conn, scope)?;
        let spent = stats::cumulative_spending(conn, scope, month, last)?;
        let daily = stats::daily(conn, scope, month, last)?;
        let days = stats::days_in_month(month) as usize;
        let month_label = capitalized(dates::month_name(month.month()));
        let prev_label = capitalized(dates::month_name(prev.month()));
        let average_label = ngettext(
            "Moyenne sur {n} mois",
            "Moyenne sur {n} mois",
            spent.average_months as u32,
        )
        .replace("{n}", &spent.average_months.to_string());
        let summary = match spent.current.last() {
            Some(total) => {
                let day = spent.current.len();
                let text = gettext("{amount} dépensés au {date}")
                    .replace("{amount}", &amount_whole(*total, cur, Sign::Negative))
                    .replace(
                        "{date}",
                        &dates::day_month_ordinal(month.with_day(day as u32).unwrap()),
                    );
                match spent
                    .previous
                    .get(day.min(spent.previous.len()).wrapping_sub(1))
                {
                    Some(before) => {
                        let gap = total - before;
                        let compared = if gap > 0 {
                            gettext("{amount} de plus que le mois précédent à la même date")
                        } else if gap < 0 {
                            gettext("{amount} de moins que le mois précédent à la même date")
                        } else {
                            gettext("autant que le mois précédent à la même date")
                        }
                        .replace("{amount}", &amount_whole(gap.abs(), cur, Sign::Negative));
                        format!("{text} · {compared}")
                    }
                    None => text,
                }
            }
            None => String::new(),
        };
        let mut items = vec![legend_item(LegendMark::Line, &month_label)];
        if !spent.previous.is_empty() {
            items.push(legend_item(LegendMark::Ghost, &prev_label));
        }
        if !spent.average.is_empty() {
            items.push(legend_item(LegendMark::Area, &average_label));
        }
        let legend = super::legend(&items);

        let all = spent
            .current
            .iter()
            .chain(&spent.previous)
            .chain(&spent.average);
        let (lo, hi) = (
            all.clone().copied().min().unwrap_or(0).min(0),
            all.copied().max().unwrap_or(0).max(100),
        );
        let ticks = scale::nice_ticks(lo, hi, 4, 100);
        let tooltips = (0..days)
            .map(|i| {
                let mut tip =
                    charts::Tip::new(dates::day_month_year(month.with_day(i as u32 + 1).unwrap()));
                for (mark, label, series) in [
                    (LegendMark::Line, &month_label, &spent.current),
                    (LegendMark::Ghost, &prev_label, &spent.previous),
                    (LegendMark::Area, &average_label, &spent.average),
                ] {
                    if let Some(v) = series.get(i) {
                        tip = tip.row(
                            charts::TipMark::Legend(mark),
                            label.clone(),
                            amount(*v, cur, Sign::Negative),
                        );
                    }
                }
                if let Some(s) = daily.spending.get(i) {
                    tip = tip.footer(
                        gettext("Dépenses du jour <b>{amount}</b>")
                            .replace("{amount}", &amount(*s, cur, Sign::Negative)),
                    );
                }
                tip
            })
            .collect();
        let chart = line::LineChart {
            values: spent.current.clone(),
            ghost: (!spent.previous.is_empty()).then(|| spent.previous.clone()),
            reference: (!spent.average.is_empty()).then(|| spent.average.clone()),
            slots: days,
            tooltips,
            ticks: ticks
                .iter()
                .map(|t| (*t, amount_whole(*t, cur, Sign::Negative)))
                .collect(),
            x_labels: super::overview::day_labels(
                month,
                if narrow {
                    &[1, 10, 20, 31]
                } else {
                    &[1, 5, 10, 15, 20, 25, 31]
                },
            ),
            end_label: spent
                .current
                .last()
                .map(|e| amount_whole(*e, cur, Sign::Negative)),
            gutter: true,
            ..Default::default()
        };
        let description = if summary.is_empty() {
            gettext("Aucune opération ce mois-ci")
        } else {
            gettext("Dépenses cumulées depuis le 1er {of_month} : {summary}")
                .replace("{of_month}", &super::overview::of_month(month.month()))
                .replace("{summary}", &summary)
        };
        let chart = line::line_chart(chart, if narrow { 200 } else { 282 }, &description);
        let card = chart_card(&[
            caption(&summary).upcast_ref(),
            legend.upcast_ref(),
            chart.upcast_ref(),
        ]);
        self.root
            .append(&section(&gettext("Rythme des dépenses"), None, &card));

        // Ring and table by category (categories listed separately).
        let categories = stats::spending_by_category(conn, scope, month, stats::next_month(month))?;
        let previous = stats::spending_by_category(conn, scope, prev, month)?;
        let parts = stats::chart_parts(&categories);
        let prev_parts = stats::chart_parts(&previous);
        let weak = self.window.clone();
        let ring = month_donut(
            &parts,
            &prev_parts,
            month,
            cur,
            if narrow { 220 } else { 300 },
            move |slot| {
                if let Some(w) = weak.upgrade() {
                    w.show_chart_group(slot, month, stats::next_month(month));
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
                c.slot,
                &c.name,
                &amount(c.amount, cur, Sign::Negative),
                &pct1(c.amount, total),
                (!narrow).then_some(delta.as_str()),
            );
            let weak = self.window.clone();
            let category = c.category_id;
            table.append(&super::clickable(
                &row,
                &format!(
                    "{} · {}",
                    gettext("Voir les opérations : {name}").replace("{name}", &c.name),
                    n_ops(c.operations)
                ),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.show_category(category, month, stats::next_month(month));
                    }
                },
            ));
        }
        // Ring beside the table when there is room, above it otherwise.
        let body = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .min_children_per_line(1)
            .max_children_per_line(if narrow { 1 } else { 2 })
            .column_spacing(40)
            .row_spacing(16)
            .build();
        body.append(&ring);
        body.append(&table);
        super::unfocusable_children(&body);
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
    if op.is_manual {
        let sub = gtk::Box::builder().spacing(8).build();
        sub.append(&caption(&account));
        sub.append(&super::manual_badge());
        text.append(&sub);
    } else {
        text.append(&caption(&account));
    }
    line.append(&text);
    if op.is_manual {
        line.append(&super::edit_manual_button(op.id));
    }
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
