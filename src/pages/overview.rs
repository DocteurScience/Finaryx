//! "Vue d'ensemble" (design/Main.dc.html): key figures, month balance,
//! 6-month cumulative, where the money goes, forecast.

use adw::prelude::*;
use chrono::{Datelike, NaiveDate};
use finaryx_core::categories::{CHART_GROUPS, OTHERS_GROUP};
use finaryx_core::db::Result;
use finaryx_core::money::Sign;
use finaryx_core::rusqlite::Connection;
use finaryx_core::stats::{self, ChartPart, Scope};
use finaryx_core::{dates, scale};
use gettextrs::gettext;
use gtk::glib;

use super::{
    amount, amount_whole, capitalized, caption, chart_card, clear, empty_state, kpi_card,
    legend_item, pct0, pct1, section_header, KpiSize,
};
use crate::charts::{self, columns, donut, line, LegendMark};
use crate::window::FinaryxWindow;

/// "d’octobre", "de mai".
pub fn of_month(month: u32) -> String {
    let name = dates::month_name(month);
    if name.starts_with(['a', 'o', 'é', 'e', 'i', 'u']) {
        format!("d’{name}")
    } else {
        format!("de {name}")
    }
}

/// Short and long labels of a month: "oct.", "octobre 2026".
pub fn month_label(m: NaiveDate) -> String {
    dates::month_year(m)
}

pub struct OverviewPage {
    pub root: gtk::Box,
    window: glib::WeakRef<FinaryxWindow>,
}

impl std::fmt::Debug for OverviewPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverviewPage").finish_non_exhaustive()
    }
}

/// Balance curve of a month with the previous month dashed.
pub fn month_line(
    conn: &Connection,
    scope: &Scope,
    month: NaiveDate,
    height: i32,
    x_days: &[u32],
) -> Result<(gtk::DrawingArea, stats::Daily, stats::Daily)> {
    let last = stats::last_operation(conn, scope)?;
    let current = stats::daily(conn, scope, month, last)?;
    let previous = stats::daily(conn, scope, stats::previous_month(month), None)?;
    let all: Vec<i64> = current
        .balances
        .iter()
        .chain(previous.balances.iter())
        .copied()
        .collect();
    let (lo, hi) = (
        all.iter().min().copied().unwrap_or(0),
        all.iter().max().copied().unwrap_or(0),
    );
    let unit = 100;
    let ticks = scale::nice_ticks(lo, hi, 4, unit);
    let cur = &scope.currency;
    let prev_month = stats::previous_month(month);
    let tooltips = (0..stats::days_in_month(month) as usize)
        .map(|i| {
            let day = month.with_day(i as u32 + 1).unwrap();
            let mut lines = vec![format!("<b>{}</b>", dates::day_month_year(day))];
            if let Some(v) = current.balances.get(i) {
                lines.push(format!(
                    "{} : {}",
                    capitalized(dates::month_name(month.month())),
                    amount(*v, cur, Sign::Negative)
                ));
            }
            if let Some(v) = previous.balances.get(i) {
                lines.push(format!(
                    "{} : {}",
                    capitalized(dates::month_name(prev_month.month())),
                    amount(*v, cur, Sign::Negative)
                ));
            }
            if let Some(s) = current.spending.get(i) {
                lines.push(
                    gettext("Dépenses du jour : <b>{amount}</b>")
                        .replace("{amount}", &amount(*s, cur, Sign::Negative)),
                );
            }
            lines.join("\n")
        })
        .collect();
    let x_labels = x_days
        .iter()
        .map(|d| {
            let label = if *d == 1 {
                dates::day_month(month)
            } else {
                d.to_string()
            };
            (*d as usize - 1, label)
        })
        .collect();
    let end = current.balances.last().copied();
    let chart = line::LineChart {
        values: current.balances.clone(),
        ghost: Some(previous.balances.clone()),
        slots: stats::days_in_month(month) as usize,
        tooltips,
        ticks: ticks
            .iter()
            .map(|t| (*t, amount_whole(*t, cur, Sign::Negative)))
            .collect(),
        x_labels,
        end_label: end.map(|e| amount_whole(e, cur, Sign::Negative)),
        gutter: true,
    };
    let description = match (current.balances.first(), end) {
        (Some(first), Some(end)) => {
            gettext("Solde en {month} : de {start} le 1er à {end} le {day}, comparé à {previous}")
                .replace("{month}", dates::month_name(month.month()))
                .replace("{start}", &amount_whole(*first, cur, Sign::Negative))
                .replace("{end}", &amount_whole(end, cur, Sign::Negative))
                .replace("{day}", &current.balances.len().to_string())
                .replace("{previous}", dates::month_name(prev_month.month()))
        }
        _ => gettext("Aucune opération ce mois-ci"),
    };
    Ok((
        line::line_chart(chart, height, &description),
        current,
        previous,
    ))
}

/// Ring and its parts for a month; `previous` is used for the tooltips.
pub fn month_donut(
    parts: &[ChartPart],
    previous: &[ChartPart],
    month: NaiveDate,
    currency: &str,
    size: i32,
    on_click: impl Fn(u8) + 'static,
) -> gtk::DrawingArea {
    let total: i64 = parts.iter().map(|p| p.amount).sum();
    let prev_name = dates::month_short(stats::previous_month(month).month());
    let donut_parts = parts
        .iter()
        .map(|p| {
            let before = previous
                .iter()
                .find(|q| q.slot == p.slot)
                .map_or(0, |q| q.amount);
            let tooltip = format!(
                "<b>{}</b>\n{}\n{}\n{}\n{}",
                glib::markup_escape_text(&p.name),
                gettext("Montant : {amount}")
                    .replace("{amount}", &amount(p.amount, currency, Sign::Negative)),
                gettext("Part des dépenses : {pct} %").replace("{pct}", &pct1(p.amount, total)),
                gettext("Écart vs {month} : {amount}")
                    .replace("{month}", prev_name)
                    .replace(
                        "{amount}",
                        &amount(p.amount - before, currency, Sign::Always)
                    ),
                super::n_ops(p.operations),
            );
            donut::Part {
                slot: p.slot,
                amount: p.amount,
                label: format!("{}\u{a0}%", pct0(p.amount, total)),
                tooltip,
            }
        })
        .collect();
    let description =
        gettext("Anneau des dépenses {of_month} par catégorie, le détail est dans la liste")
            .replace("{of_month}", &of_month(month.month()));
    donut::donut(
        donut::Donut {
            parts: donut_parts,
            center: amount(total, currency, Sign::Negative),
            center_caption: gettext("dépensés"),
        },
        size,
        &description,
        on_click,
    )
}

/// Stacked columns of the last months (spending per chart slot).
pub fn months_columns(
    conn: &Connection,
    scope: &Scope,
    months: &[NaiveDate],
    height: i32,
    segment_labels: bool,
) -> Result<(gtk::DrawingArea, Vec<[i64; CHART_GROUPS]>)> {
    let data = stats::monthly_by_slot(conn, scope, months)?;
    let names = slot_names(conn)?;
    let cur = &scope.currency;
    let max = data
        .iter()
        .map(|r| r.iter().sum::<i64>())
        .max()
        .unwrap_or(0);
    let ticks = scale::nice_ticks(0, max, 4, 100);
    let columns = months
        .iter()
        .zip(&data)
        .map(|(m, row)| {
            let total: i64 = row.iter().sum();
            let month_name = dates::month_short(m.month());
            columns::Column {
                label: month_name.to_string(),
                values: *row,
                total_label: amount_whole(total, cur, Sign::Negative),
                segment_labels: std::array::from_fn(|k| {
                    if segment_labels && row[k] > 0 {
                        format!("{}\u{a0}%", pct0(row[k], total))
                    } else {
                        String::new()
                    }
                }),
                tooltips: std::array::from_fn(|k| {
                    format!(
                        "<b>{} · {}</b>\n{}\n{}\n{}",
                        capitalized(month_name),
                        glib::markup_escape_text(&names[k]),
                        gettext("Montant : {amount}")
                            .replace("{amount}", &amount(row[k], cur, Sign::Negative)),
                        gettext("Part du mois : {pct} %").replace("{pct}", &pct1(row[k], total)),
                        gettext("Total du mois : {amount}")
                            .replace("{amount}", &amount(total, cur, Sign::Negative)),
                    )
                }),
            }
        })
        .collect();
    let description = gettext("Dépenses mensuelles empilées par catégorie, de {from} à {to}")
        .replace(
            "{from}",
            dates::month_name(months.first().map_or(1, |m| m.month())),
        )
        .replace(
            "{to}",
            dates::month_name(months.last().map_or(1, |m| m.month())),
        );
    let chart = columns::Columns {
        columns,
        ticks: ticks
            .iter()
            .map(|t| (*t, amount_whole(*t, cur, Sign::Negative)))
            .collect(),
    };
    Ok((columns::stacked_columns(chart, height, &description), data))
}

/// Names of the 13 chart groups: the category holding each colour
/// (empty when the colour is free), then "Autres".
pub fn slot_names(conn: &Connection) -> Result<[String; CHART_GROUPS]> {
    let categories = finaryx_core::repo::list_categories(conn)?;
    Ok(std::array::from_fn(|k| {
        let group = k as u8 + 1;
        if group == OTHERS_GROUP {
            return gettext("Autres");
        }
        categories
            .iter()
            .find(|c| c.slot == Some(group))
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }))
}

/// A row of the "where the money goes" list: colour, name, amount, share
/// (and an optional difference with the previous month).
pub fn part_row(
    slot: Option<u8>,
    name: &str,
    amount_text: &str,
    pct: &str,
    delta: Option<&str>,
) -> gtk::Box {
    let row = gtk::Box::builder()
        .spacing(10)
        .css_classes(["list-line"])
        .build();
    let s = slot;
    row.append(&charts::color_swatch(12, move |w| {
        charts::category_color(w, s)
    }));
    row.append(
        &gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .build(),
    );
    row.append(
        &gtk::Label::builder()
            .label(amount_text)
            .css_classes(["numeric"])
            .build(),
    );
    row.append(
        &gtk::Label::builder()
            .label(format!("{pct}\u{a0}%"))
            .width_chars(7)
            .xalign(1.0)
            .css_classes(["dim-label", "numeric"])
            .build(),
    );
    if let Some(d) = delta {
        row.append(
            &gtk::Label::builder()
                .label(d)
                .width_chars(10)
                .xalign(1.0)
                .css_classes(["dim-label", "numeric"])
                .build(),
        );
    }
    row
}

impl OverviewPage {
    pub fn new(window: &FinaryxWindow) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .build();
        OverviewPage {
            root,
            window: window.downgrade(),
        }
    }

    fn go(&self, page: &'static str) -> Box<dyn Fn()> {
        let weak = self.window.clone();
        Box::new(move || {
            if let Some(w) = weak.upgrade() {
                w.show_page(page);
            }
        })
    }

    /// Rebuilds the page; returns the subtitle ("Octobre 2026").
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
        let Some(month) = stats::latest_month(conn, scope)? else {
            self.root.append(&no_operations(&self.window));
            return Ok(String::new());
        };
        let cur = scope.currency.as_str();
        let prev = stats::previous_month(month);
        let totals = stats::month_totals(conn, scope, month)?;
        let prev_totals = stats::month_totals(conn, scope, prev)?;
        let last = stats::last_operation(conn, scope)?;
        let balance = stats::balance_at(conn, scope, last)?;
        let start = stats::balance_at(conn, scope, month.pred_opt())?;

        // Key figures.
        let kpis = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(if narrow { 2 } else { 1 })
            .max_children_per_line(4)
            .column_spacing(12)
            .row_spacing(12)
            .build();
        kpis.append(&kpi_card(
            &gettext("Solde total"),
            &amount(balance, cur, Sign::Negative),
            &gettext("{amount} depuis le 1er {month}")
                .replace("{amount}", &amount(balance - start, cur, Sign::Always))
                .replace("{month}", dates::month_name(month.month())),
            if narrow {
                KpiSize::Small
            } else {
                KpiSize::Large
            },
        ));
        let spending_note = if prev_totals.spending > 0 {
            gettext("{pct} % par rapport à {month}")
                .replace(
                    "{pct}",
                    &format!(
                        "{}{}",
                        if totals.spending >= prev_totals.spending {
                            "+"
                        } else {
                            ""
                        },
                        pct1(totals.spending - prev_totals.spending, prev_totals.spending)
                    ),
                )
                .replace("{month}", dates::month_name(prev.month()))
        } else {
            String::new()
        };
        kpis.append(&kpi_card(
            &gettext("Dépenses {of_month}").replace("{of_month}", &of_month(month.month())),
            &amount(totals.spending, cur, Sign::Negative),
            &spending_note,
            if narrow {
                KpiSize::Small
            } else {
                KpiSize::Medium
            },
        ));
        let income_note = match &totals.largest_income {
            Some((label, value)) => {
                let norm = finaryx_core::normalize::label_norm(label);
                if norm.contains("SALAIRE") || norm.contains("SALARY") {
                    gettext("dont salaire {amount}")
                        .replace("{amount}", &amount(*value, cur, Sign::Negative))
                } else {
                    gettext("dont {label} : {amount}")
                        .replace("{label}", label)
                        .replace("{amount}", &amount(*value, cur, Sign::Negative))
                }
            }
            None => String::new(),
        };
        kpis.append(&kpi_card(
            &gettext("Revenus {of_month}").replace("{of_month}", &of_month(month.month())),
            &amount(totals.income, cur, Sign::Negative),
            &income_note,
            if narrow {
                KpiSize::Small
            } else {
                KpiSize::Medium
            },
        ));
        let next = stats::next_month(month);
        kpis.append(&kpi_card(
            &gettext("Fin {month} (prévu)").replace("{month}", dates::month_name(next.month())),
            "—",
            &gettext("Prévision bientôt disponible"),
            if narrow {
                KpiSize::Small
            } else {
                KpiSize::Medium
            },
        ));
        super::unfocusable_children(&kpis);
        self.root.append(&kpis);

        // Two columns (one in the narrow layout).
        let columns = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(1)
            .max_children_per_line(if narrow { 1 } else { 2 })
            .column_spacing(24)
            .row_spacing(24)
            .build();
        let left = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .valign(gtk::Align::Start)
            .build();
        let right = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .valign(gtk::Align::Start)
            .build();

        // Month balance.
        let (chart, _, _) = month_line(
            conn,
            scope,
            month,
            if narrow { 180 } else { 242 },
            &[1, 8, 15, 22, 29],
        )?;
        let legend = super::legend(&[
            legend_item(
                LegendMark::Line,
                &capitalized(dates::month_name(month.month())),
            ),
            legend_item(
                LegendMark::Ghost,
                &capitalized(dates::month_name(prev.month())),
            ),
        ]);
        let what = if scope.accounts.len() > 1 {
            gettext("Solde de tous les comptes, jour après jour")
        } else {
            gettext("Solde du compte, jour après jour")
        };
        let card = chart_card(&[
            caption(&what).upcast_ref(),
            legend.upcast_ref(),
            chart.upcast_ref(),
        ]);
        left.append(&section(
            &gettext("Évolution du mois"),
            Some((&gettext("Détail du mois"), self.go("month"))),
            &card,
        ));

        // 6-month cumulative.
        let months = stats::months_ending(month, 6);
        let (chart, data) = months_columns(conn, scope, &months, 222, false)?;
        let total: i64 = data.iter().map(|r| r.iter().sum::<i64>()).sum();
        let text = gettext("{amount} dépensés de {from} à {to}")
            .replace("{amount}", &amount(total, cur, Sign::Negative))
            .replace("{from}", dates::month_name(months[0].month()))
            .replace("{to}", dates::month_name(month.month()));
        let card = chart_card(&[caption(&text).upcast_ref(), chart.upcast_ref()]);
        left.append(&section(
            &gettext("Cumul sur 6 mois"),
            Some((&gettext("Voir le cumul"), self.go("cumulative"))),
            &card,
        ));

        // Where the money goes.
        let parts = stats::chart_parts(&stats::spending_by_category(
            conn,
            scope,
            month,
            stats::next_month(month),
        )?);
        let prev_parts =
            stats::chart_parts(&stats::spending_by_category(conn, scope, prev, month)?);
        let weak = self.window.clone();
        let parts_for_click = parts.clone();
        let month_for_click = month;
        let currency = scope.currency.clone();
        let ring = month_donut(
            &parts,
            &prev_parts,
            month,
            cur,
            if narrow { 220 } else { 284 },
            move |slot| {
                if slot == OTHERS_GROUP {
                    if let (Some(w), Some(p)) = (
                        weak.upgrade(),
                        parts_for_click.iter().find(|p| p.slot == OTHERS_GROUP),
                    ) {
                        super::show_others_dialog(&w, p, month_for_click, &currency);
                    }
                }
            },
        );
        let list = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .build();
        let total: i64 = parts.iter().map(|p| p.amount).sum();
        for p in &parts {
            list.append(&part_row(
                Some(p.slot),
                &p.name,
                &amount(p.amount, cur, Sign::Negative),
                &pct1(p.amount, total),
                None,
            ));
        }
        let body = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .min_children_per_line(1)
            .max_children_per_line(2)
            .column_spacing(40)
            .row_spacing(16)
            .build();
        body.append(&ring);
        body.append(&list);
        super::unfocusable_children(&body);
        let text = gettext("{month} · {amount} dépensés")
            .replace("{month}", &capitalized(&month_label(month)))
            .replace("{amount}", &amount(totals.spending, cur, Sign::Negative));
        let card = chart_card(&[caption(&text).upcast_ref(), body.upcast_ref()]);
        right.append(&section(
            &gettext("Où part l’argent"),
            Some((&gettext("Par catégorie"), self.go("month"))),
            &card,
        ));

        // Forecast (step 7).
        let forecast = caption(&gettext(
            "La prévision d’après vos habitudes arrive avec la page Prévisions (étape 7 du plan de travail).",
        ));
        let card = chart_card(&[forecast.upcast_ref()]);
        right.append(&section(
            &gettext("Prévision à 6 mois"),
            Some((&gettext("Prévisions"), self.go("forecast"))),
            &card,
        ));

        columns.append(&left);
        columns.append(&right);
        super::unfocusable_children(&columns);
        self.root.append(&columns);
        let _ = glib::markup_escape_text;
        Ok(capitalized(&month_label(month)))
    }
}

/// A section: header with a link, then content.
pub fn section(title: &str, link: super::SectionLink, content: &impl IsA<gtk::Widget>) -> gtk::Box {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();
    section.append(&section_header(title, link));
    section.append(content);
    section
}

/// Empty state when the accounts have no operation yet.
pub fn no_operations(window: &glib::WeakRef<FinaryxWindow>) -> adw::StatusPage {
    let button = gtk::Button::builder()
        .label(gettext("Importer un CSV"))
        .halign(gtk::Align::Center)
        .action_name("win.import")
        .css_classes(["suggested-action", "pill"])
        .build();
    let page = empty_state(
        "finaryx-import-symbolic",
        &gettext("Pas encore d’opérations"),
        &gettext("Importez l’export CSV d’un compte pour voir cette vue."),
    );
    page.set_child(Some(&button));
    let _ = window;
    page
}
