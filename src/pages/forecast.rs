//! "Prévisions" (design/Previsions.dc.html, EtroitPrevisions.dc.html):
//! learning period and horizon, key figures, end-of-month balance (real,
//! then forecast with its probable range), month by month, and the
//! recurring charges detected.

use std::cell::Cell;

use adw::prelude::*;
use chrono::{Datelike, NaiveDate};
use finaryx_core::dates;
use finaryx_core::db::Result;
use finaryx_core::forecast::{self, Forecast, Outcome, PERIODS};
use finaryx_core::money::Sign;
use finaryx_core::rusqlite::Connection;
use finaryx_core::scale;
use finaryx_core::stats::{self, Scope};
use gettextrs::{gettext, ngettext};
use gtk::glib;

use super::overview::{no_operations, section};
use super::{
    amount, amount_whole, boxed_list, capitalized, caption, chart_card, clear, empty_state,
    kpi_card, legend, legend_item, KpiSize,
};
use crate::charts::{line, LegendMark, Tip, TipMark};
use crate::window::FinaryxWindow;

pub struct ForecastPage {
    pub root: gtk::Box,
    /// Learning period and horizon selectors, placed in the filter row.
    pub controls: gtk::Box,
    window: glib::WeakRef<FinaryxWindow>,
    pub basis: Cell<usize>,
    pub horizon: Cell<usize>,
}

impl std::fmt::Debug for ForecastPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ForecastPage").finish_non_exhaustive()
    }
}

/// "le 1er", "le 5"
pub fn on_day(day: u32) -> String {
    if day == 1 {
        gettext("le 1er")
    } else {
        gettext("le {day}").replace("{day}", &day.to_string())
    }
}

/// "Novembre 2026"
fn month_title(m: NaiveDate) -> String {
    capitalized(&dates::month_year(m))
}

/// A labelled group of linked toggle buttons: 3, 6 or 12 months.
fn period_selector(
    title: &str,
    accessible: &str,
    initial: usize,
    on_change: impl Fn(usize) + Clone + 'static,
) -> gtk::Box {
    let group = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();
    group.append(&caption(title));
    let buttons = gtk::Box::builder()
        .css_classes(["linked"])
        .homogeneous(true)
        .build();
    buttons.update_property(&[gtk::accessible::Property::Label(accessible)]);
    let mut first: Option<gtk::ToggleButton> = None;
    for n in PERIODS {
        let button = gtk::ToggleButton::builder()
            .label(gettext("{n} mois").replace("{n}", &n.to_string()))
            .active(n == initial)
            .build();
        if let Some(f) = &first {
            button.set_group(Some(f));
        } else {
            first = Some(button.clone());
        }
        let on_change = on_change.clone();
        button.connect_toggled(move |b| {
            if b.is_active() {
                on_change(n);
            }
        });
        buttons.append(&button);
    }
    group.append(&buttons);
    group
}

/// Options of the balance chart.
pub struct ChartOptions {
    pub height: i32,
    /// Full chart (Prévisions): a dot on every month, "Aujourd'hui",
    /// annotation of the spending peak.
    pub detailed: bool,
    pub narrow: bool,
    pub today: NaiveDate,
}

/// End-of-month balance: real months, then the forecast with its range.
pub fn balance_chart(f: &Forecast, currency: &str, o: &ChartOptions) -> gtk::DrawingArea {
    let real: Vec<i64> = f.history.iter().map(|(_, b)| *b).collect();
    let fore: Vec<i64> = f.months.iter().map(|m| m.balance).collect();
    let band: Vec<(i64, i64)> = f
        .months
        .iter()
        .map(|m| (m.balance - m.half_width, m.balance + m.half_width))
        .collect();
    let lo = real
        .iter()
        .copied()
        .chain(band.iter().map(|b| b.0))
        .min()
        .unwrap_or(0);
    let hi = real
        .iter()
        .copied()
        .chain(band.iter().map(|b| b.1))
        .max()
        .unwrap_or(0);
    let ticks = scale::nice_ticks(lo, hi, 4, 100);

    let months: Vec<NaiveDate> = f
        .history
        .iter()
        .map(|(m, _)| *m)
        .chain(f.months.iter().map(|m| m.month))
        .collect();
    let max_labels = if o.narrow {
        4
    } else if o.detailed {
        12
    } else {
        6
    };
    let step = months.len().div_ceil(max_labels).max(1);
    // Labels counted from the last point so that it always has one.
    let last = months.len() - 1;
    let x_labels = months
        .iter()
        .enumerate()
        .filter(|(i, _)| (last - i) % step == 0)
        .map(|(i, m)| (i, dates::month_short(m.month()).to_string()))
        .collect();

    let mut tooltips: Vec<Tip> = f
        .history
        .iter()
        .map(|(m, b)| {
            Tip::new(month_title(*m)).row(
                TipMark::Legend(LegendMark::Line),
                gettext("Solde en fin de mois"),
                amount(*b, currency, Sign::Negative),
            )
        })
        .collect();
    tooltips.extend(f.months.iter().map(|m| {
        Tip::new(gettext("{month} · prévu").replace("{month}", &month_title(m.month)))
            .row(
                TipMark::Legend(LegendMark::Dashed),
                gettext("Solde en fin de mois"),
                amount(m.balance, currency, Sign::Negative),
            )
            .row(
                TipMark::Legend(LegendMark::Band),
                gettext("Fourchette"),
                super::amount_range(m.balance - m.half_width, m.balance + m.half_width, currency),
            )
            .footer(
                gettext("Dépenses attendues <b>{spending}</b> · revenus <b>{income}</b>")
                    .replace(
                        "{spending}",
                        &amount_whole(m.spending, currency, Sign::Negative),
                    )
                    .replace(
                        "{income}",
                        &amount_whole(m.income, currency, Sign::Negative),
                    ),
            )
    }));

    let n_real = real.len();
    let marker = o.detailed.then(|| {
        let after = f.history.last().map(|(m, _)| stats::next_month(*m));
        // The month under way: between its start and its end.
        let pos = match after {
            Some(m) if f.as_of >= m => {
                (n_real - 1) as f64 + f.as_of.day() as f64 / stats::days_in_month(f.as_of) as f64
            }
            _ => (n_real - 1) as f64,
        };
        let label = if (o.today - f.as_of).num_days() <= 14 {
            gettext("Aujourd’hui")
        } else {
            gettext("Dernière opération")
        };
        (pos, label)
    });
    let annotation = if o.detailed && !o.narrow {
        f.peak().and_then(|p| {
            let i = f.months.iter().position(|m| m.month == p.month)?;
            Some((
                n_real + i,
                gettext("{month} : pic de dépenses\nattendu ({amount})")
                    .replace("{month}", &capitalized(dates::month_name(p.month.month())))
                    .replace(
                        "{amount}",
                        &amount_whole(p.spending, currency, Sign::Negative),
                    ),
            ))
        })
    } else {
        None
    };

    let end = f.months.last();
    let chart = line::LineChart {
        values: real.clone(),
        tooltips,
        ticks: ticks
            .iter()
            .map(|t| (*t, amount_whole(*t, currency, Sign::Negative)))
            .collect(),
        x_labels,
        end_label: end.map(|m| amount_whole(m.balance, currency, Sign::Negative)),
        gutter: true,
        forecast: fore,
        band,
        dots: o.detailed,
        marker,
        annotation,
        ..Default::default()
    };
    let description = match (f.history.last(), end) {
        (Some((m0, b0)), Some(e)) => gettext(
            "Solde en fin de mois : {real} en {month}, prévu à {end} en {last}, avec une fourchette de {low} à {high}",
        )
        .replace("{real}", &amount_whole(*b0, currency, Sign::Negative))
        .replace("{month}", dates::month_name(m0.month()))
        .replace("{end}", &amount_whole(e.balance, currency, Sign::Negative))
        .replace("{last}", dates::month_name(e.month.month()))
        .replace(
            "{low}",
            &amount_whole(e.balance - e.half_width, currency, Sign::Negative),
        )
        .replace(
            "{high}",
            &amount_whole(e.balance + e.half_width, currency, Sign::Negative),
        ),
        _ => String::new(),
    };
    line::line_chart(chart, o.height, &description)
}

/// Legend of the balance chart: one row, or wrapping in the narrow
/// layout.
pub fn chart_legend(narrow: bool, short: bool, real: &str) -> gtk::Widget {
    let items = [
        legend_item(LegendMark::Line, real),
        legend_item(LegendMark::Dashed, &gettext("Prévu")),
        legend_item(
            LegendMark::Band,
            &if narrow || short {
                gettext("Fourchette")
            } else {
                gettext("Fourchette probable")
            },
        ),
    ];
    if narrow {
        return legend(&items).upcast();
    }
    let row = gtk::Box::builder().spacing(20).build();
    for item in &items {
        row.append(item);
    }
    row.upcast()
}

/// "Pas encore assez de données pour une prévision".
pub fn not_enough_data(months: usize) -> adw::StatusPage {
    let missing = forecast::MIN_MONTHS.saturating_sub(months);
    let page = empty_state(
        "finaryx-forecast-symbolic",
        &gettext("Pas encore assez de données pour une prévision"),
        &ngettext(
            "Finaryx a besoin d’au moins 3 mois d’opérations pour repérer vos habitudes. Il vous en manque {n} : importez un export plus ancien pour débloquer cette vue.",
            "Finaryx a besoin d’au moins 3 mois d’opérations pour repérer vos habitudes. Il vous en manque {n} : importez un export plus ancien pour débloquer cette vue.",
            missing as u32,
        )
        .replace("{n}", &missing.to_string()),
    );
    let button = gtk::Button::builder()
        .label(gettext("Importer un export plus ancien"))
        .halign(gtk::Align::Center)
        .action_name("win.import")
        .css_classes(["pill"])
        .build();
    page.set_child(Some(&button));
    page
}

/// A row with a title, a subtitle and two right-aligned lines.
fn month_row(title: &str, subtitle: &str, value: &str, note: &str) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .activatable(false)
        .build();
    let right = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .valign(gtk::Align::Center)
        .build();
    right.append(
        &gtk::Label::builder()
            .label(value)
            .xalign(1.0)
            .css_classes(["heading", "numeric"])
            .build(),
    );
    right.append(
        &gtk::Label::builder()
            .label(note)
            .xalign(1.0)
            .css_classes(["caption", "dim-label", "numeric"])
            .build(),
    );
    row.add_suffix(&right);
    row
}

impl ForecastPage {
    pub fn new(window: &FinaryxWindow) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .build();
        let controls = gtk::Box::builder()
            .spacing(20)
            .halign(gtk::Align::End)
            .build();
        let weak = window.downgrade();
        controls.append(&period_selector(
            &gettext("Habitudes calculées sur"),
            &gettext("Période de référence"),
            6,
            move |n| {
                if let Some(w) = weak.upgrade() {
                    w.set_forecast_periods(Some(n), None);
                }
            },
        ));
        let weak = window.downgrade();
        controls.append(&period_selector(
            &gettext("Horizon"),
            &gettext("Horizon de prévision"),
            6,
            move |n| {
                if let Some(w) = weak.upgrade() {
                    w.set_forecast_periods(None, Some(n));
                }
            },
        ));
        ForecastPage {
            root,
            controls,
            window: window.downgrade(),
            basis: Cell::new(6),
            horizon: Cell::new(6),
        }
    }

    /// Rebuilds the page; returns the subtitle ("Novembre 2026 à avril 2027").
    pub fn refresh(
        &self,
        conn: &Connection,
        scope: Option<&Scope>,
        narrow: bool,
        today: NaiveDate,
    ) -> Result<String> {
        clear(&self.root);
        // Narrow layout: the selectors take the whole width.
        self.controls.set_orientation(if narrow {
            gtk::Orientation::Vertical
        } else {
            gtk::Orientation::Horizontal
        });
        self.controls.set_spacing(if narrow { 10 } else { 20 });
        self.controls.set_hexpand(true);
        self.controls.set_halign(if narrow {
            gtk::Align::Fill
        } else {
            gtk::Align::End
        });
        self.controls.set_visible(false);
        let Some(scope) = scope else {
            return Ok(String::new());
        };
        if stats::last_operation(conn, scope)?.is_none() {
            self.root.append(&no_operations(&self.window));
            return Ok(String::new());
        }
        let cur = scope.currency.as_str();
        let (basis, horizon) = (self.basis.get(), self.horizon.get());
        let f = match forecast::forecast(conn, scope, today, basis, horizon)? {
            Outcome::NotEnoughData { months } => {
                self.root.append(&not_enough_data(months));
                return Ok(String::new());
            }
            Outcome::Ready(f) => f,
        };
        self.controls.set_visible(true);
        let (Some(first), Some(last)) = (f.months.first(), f.months.last()) else {
            return Ok(String::new());
        };
        let size = if narrow {
            KpiSize::Small
        } else {
            KpiSize::Medium
        };

        // Key figures.
        let kpis = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(if narrow { 2 } else { 1 })
            .max_children_per_line(3)
            .column_spacing(12)
            .row_spacing(12)
            .build();
        let range = gettext("{low} à {high}")
            .replace(
                "{low}",
                &amount_whole(last.balance - last.half_width, cur, Sign::Negative),
            )
            .replace(
                "{high}",
                &amount_whole(last.balance + last.half_width, cur, Sign::Negative),
            );
        kpis.append(&kpi_card(
            &if narrow {
                gettext("Fin {month} (prévu)")
            } else {
                gettext("Solde fin {month} (prévu)")
            }
            .replace("{month}", dates::month_name(last.month.month())),
            &amount(last.balance, cur, Sign::Negative),
            &if narrow {
                range
            } else {
                gettext("Fourchette : {range}").replace("{range}", &range)
            },
            size,
        ));
        kpis.append(&kpi_card(
            &gettext("Dépenses prévues"),
            &gettext("{amount} / mois").replace(
                "{amount}",
                &amount_whole(f.mean_spending(), cur, Sign::Negative),
            ),
            &if narrow {
                String::new()
            } else {
                ngettext(
                    "Moyenne sur le mois à venir",
                    "Moyenne sur les {n} mois à venir",
                    f.months.len() as u32,
                )
                .replace("{n}", &f.months.len().to_string())
            },
            size,
        ));
        if !narrow {
            let regular = f.regular_income();
            let total: i64 = regular.iter().map(|r| r.amount).sum();
            let (value, note) = match regular.first() {
                Some(r) => {
                    let norm = finaryx_core::normalize::label_norm(&r.label);
                    let what = if norm.contains("SALAIRE") || norm.contains("SALARY") {
                        gettext("Salaire détecté {day} du mois")
                    } else {
                        gettext("{label} détecté {day} du mois").replace("{label}", &r.label)
                    };
                    (
                        gettext("{amount} / mois")
                            .replace("{amount}", &amount_whole(total, cur, Sign::Negative)),
                        what.replace("{day}", &on_day(r.day)),
                    )
                }
                None => ("—".to_owned(), gettext("Aucun revenu régulier détecté")),
            };
            kpis.append(&kpi_card(
                &gettext("Revenus réguliers"),
                &value,
                &note,
                size,
            ));
        }
        super::unfocusable_children(&kpis);
        self.root.append(&kpis);

        // End-of-month balance.
        let span = match (f.history.first(), f.history.last()) {
            (Some((a, _)), Some((b, _))) => {
                gettext("Réel de {from} à {to}, prévu de {first} à {last}")
                    .replace("{from}", dates::month_name(a.month()))
                    .replace("{to}", dates::month_name(b.month()))
                    .replace("{first}", dates::month_name(first.month.month()))
                    .replace("{last}", dates::month_name(last.month.month()))
            }
            _ => String::new(),
        };
        let header = gtk::Box::builder().spacing(20).build();
        let span_label = caption(&span);
        span_label.set_hexpand(true);
        let chart_legend = chart_legend(narrow, false, &gettext("Réel"));
        let chart = balance_chart(
            &f,
            cur,
            &ChartOptions {
                height: if narrow { 220 } else { 320 },
                detailed: true,
                narrow,
                today,
            },
        );
        let mut notes = vec![if narrow {
            gettext("Estimation d’après vos charges récurrentes et vos dépenses moyennes. La fourchette s’élargit avec l’horizon.")
        } else {
            gettext(
                "Estimation calculée à partir de vos charges récurrentes et de la moyenne de vos dépenses variables. La fourchette s’élargit avec l’horizon : plus on regarde loin, moins la prévision est sûre.",
            )
        }];
        if f.basis < basis {
            notes.push(
                ngettext(
                    "Habitudes calculées sur {n} mois : l’historique ne remonte pas plus loin.",
                    "Habitudes calculées sur {n} mois : l’historique ne remonte pas plus loin.",
                    f.basis as u32,
                )
                .replace("{n}", &f.basis.to_string()),
            );
        }
        if !f.seasonal {
            notes.push(gettext(
                "Saisonnalité non prise en compte : moins de 12 mois d’historique.",
            ));
        }
        let card = if narrow {
            chart_card(&[
                chart_legend.upcast_ref(),
                chart.upcast_ref(),
                caption(&notes.join(" ")).upcast_ref(),
            ])
        } else {
            header.append(&span_label);
            chart_legend.set_halign(gtk::Align::End);
            header.append(&chart_legend);
            chart_card(&[
                header.upcast_ref(),
                chart.upcast_ref(),
                caption(&notes.join(" ")).upcast_ref(),
            ])
        };
        self.root
            .append(&section(&gettext("Solde en fin de mois"), None, &card));

        // Month by month, and the recurring charges.
        let months = boxed_list();
        let mut previous = f.history.last().map_or(f.start_balance, |(_, b)| *b);
        for m in &f.months {
            let title = if m.partial {
                gettext("{month} (en cours)").replace("{month}", &month_title(m.month))
            } else {
                month_title(m.month)
            };
            let spending = gettext("{amount} de dépenses")
                .replace("{amount}", &amount_whole(m.spending, cur, Sign::Negative));
            let subtitle = if narrow {
                spending
            } else {
                gettext("{income} de revenus · {spending}")
                    .replace("{income}", &amount_whole(m.income, cur, Sign::Negative))
                    .replace("{spending}", &spending)
            };
            let net = gettext("{amount} sur le mois").replace(
                "{amount}",
                &amount_whole(m.balance - previous, cur, Sign::Always),
            );
            months.append(&month_row(
                &title,
                &subtitle,
                &amount(m.balance, cur, Sign::Negative),
                &net,
            ));
            previous = m.balance;
        }

        let charges = boxed_list();
        let mut any = false;
        for r in f.charges() {
            any = true;
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&r.label))
                .activatable(false)
                .build();
            row.add_suffix(
                &gtk::Label::builder()
                    .label(on_day(r.day))
                    .css_classes(["dim-label"])
                    .build(),
            );
            row.add_suffix(
                &gtk::Label::builder()
                    .label(amount(-r.amount, cur, Sign::Negative))
                    .width_chars(10)
                    .xalign(1.0)
                    .css_classes(["numeric"])
                    .build(),
            );
            charges.append(&row);
        }
        if any {
            let total = adw::ActionRow::builder()
                .title(format!("<b>{}</b>", gettext("Charges fixes")))
                .activatable(false)
                .build();
            total.add_suffix(
                &gtk::Label::builder()
                    .label(
                        gettext("{amount} / mois")
                            .replace("{amount}", &amount(f.charges_total(), cur, Sign::Negative)),
                    )
                    .css_classes(["heading", "numeric"])
                    .build(),
            );
            charges.append(&total);
        } else {
            charges.append(
                &adw::ActionRow::builder()
                    .title(gettext("Aucune charge récurrente détectée"))
                    .subtitle(gettext(
                        "Une charge est récurrente quand le même libellé revient chaque mois, pour un montant proche.",
                    ))
                    .activatable(false)
                    .build(),
            );
        }

        let columns = gtk::Box::builder()
            .spacing(24)
            .orientation(if narrow {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            })
            .build();
        let left = section(&gettext("Mois par mois"), None, &months);
        let right = section(&gettext("Charges récurrentes détectées"), None, &charges);
        left.set_hexpand(true);
        right.set_hexpand(true);
        left.set_valign(gtk::Align::Start);
        right.set_valign(gtk::Align::Start);
        columns.append(&left);
        columns.append(&right);
        self.root.append(&columns);

        Ok(gettext("{from} à {to}")
            .replace("{from}", &month_title(first.month))
            .replace("{to}", &dates::month_year(last.month)))
    }
}
