//! "Cumul" (design/Cumul.dc.html): 3, 6 or 12 months; key figures,
//! stacked columns with the share written in each segment, and the table
//! of the share of each category.

use std::cell::Cell;

use adw::prelude::*;
use chrono::Datelike;
use finaryx_core::dates;
use finaryx_core::db::Result;
use finaryx_core::money::Sign;
use finaryx_core::rusqlite::Connection;
use finaryx_core::stats::{self, Scope};
use gettextrs::gettext;
use gtk::glib;

use super::overview::{months_columns, no_operations, section, slot_names};
use super::pct1_label;
use super::{amount, amount_whole, capitalized, caption, chart_card, clear, kpi_card, KpiSize};
use crate::charts;
use crate::window::FinaryxWindow;
use finaryx_core::locale;

pub const PERIODS: [usize; 3] = [3, 6, 12];

pub struct CumulativePage {
    pub root: gtk::Box,
    /// Period selector, placed in the filter row by the window.
    pub periods: gtk::Box,
    window: glib::WeakRef<FinaryxWindow>,
    pub months: Cell<usize>,
}

impl std::fmt::Debug for CumulativePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CumulativePage").finish_non_exhaustive()
    }
}

impl CumulativePage {
    pub fn new(window: &FinaryxWindow) -> Self {
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .build();
        let periods = gtk::Box::builder()
            .css_classes(["linked"])
            .halign(gtk::Align::End)
            .build();
        periods.update_property(&[gtk::accessible::Property::Label(&gettext("Période"))]);
        let mut first: Option<gtk::ToggleButton> = None;
        for n in PERIODS {
            let button = gtk::ToggleButton::builder()
                .label(gettext("{n} mois").replace("{n}", &n.to_string()))
                .active(n == 6)
                .build();
            if let Some(f) = &first {
                button.set_group(Some(f));
            } else {
                first = Some(button.clone());
            }
            let weak = window.downgrade();
            button.connect_toggled(move |b| {
                if b.is_active() {
                    if let Some(w) = weak.upgrade() {
                        w.set_cumulative_months(n);
                    }
                }
            });
            periods.append(&button);
        }
        CumulativePage {
            root,
            periods,
            window: window.downgrade(),
            months: Cell::new(6),
        }
    }

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
        let Some(last) = stats::latest_month(conn, scope)? else {
            self.root.append(&no_operations(&self.window));
            return Ok(String::new());
        };
        let cur = scope.currency.as_str();
        let n = self.months.get();
        let months = stats::months_ending(last, n);
        let (chart, data) =
            months_columns(conn, scope, &months, if narrow { 300 } else { 420 }, true)?;
        let totals: Vec<i64> = data.iter().map(|r| r.iter().sum()).collect();
        let grand: i64 = totals.iter().sum();
        let average = grand / n as i64;
        let (max_i, min_i) = (
            (0..n).max_by_key(|&i| (totals[i], i)).unwrap_or(0),
            (0..n)
                .min_by_key(|&i| (totals[i], usize::MAX - i))
                .unwrap_or(0),
        );

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
            &gettext("Total sur {n} mois").replace("{n}", &n.to_string()),
            &amount(grand, cur, Sign::Negative),
            "",
            if narrow {
                KpiSize::Small
            } else {
                KpiSize::Medium
            },
        ));
        kpis.append(&kpi_card(
            &gettext("Moyenne par mois"),
            &amount(average, cur, Sign::Negative),
            "",
            if narrow {
                KpiSize::Small
            } else {
                KpiSize::Medium
            },
        ));
        kpis.append(&kpi_card(
            &gettext("Mois le plus chargé"),
            &amount_whole(totals[max_i], cur, Sign::Negative),
            &dates::month_year(months[max_i]),
            if narrow {
                KpiSize::Small
            } else {
                KpiSize::Medium
            },
        ));
        kpis.append(&kpi_card(
            &gettext("Mois le plus léger"),
            &amount_whole(totals[min_i], cur, Sign::Negative),
            &dates::month_year(months[min_i]),
            if narrow {
                KpiSize::Small
            } else {
                KpiSize::Medium
            },
        ));
        super::unfocusable_children(&kpis);
        self.root.append(&kpis);

        // Columns with their legend.
        let names = slot_names(conn)?;
        let legend = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .max_children_per_line(8)
            .column_spacing(16)
            .row_spacing(6)
            .build();
        for (k, name) in names.iter().enumerate() {
            if data.iter().all(|r| r[k] == 0) {
                continue;
            }
            let item = gtk::Box::builder().spacing(8).build();
            let slot = Some(k as u8 + 1);
            item.append(&charts::color_swatch(12, move |w| {
                charts::category_color(w, slot)
            }));
            item.append(
                &gtk::Label::builder()
                    .label(name)
                    .css_classes(["caption"])
                    .build(),
            );
            legend.append(&item);
        }
        super::unfocusable_children(&legend);
        let card = chart_card(&[
            caption(&gettext(
                "Le pourcentage est la part de la catégorie dans les dépenses du mois. Les parts trop fines pour être écrites sont dans le tableau en dessous.",
            ))
            .upcast_ref(),
            legend.upcast_ref(),
            chart.upcast_ref(),
        ]);
        self.root.append(&section(
            &gettext("Mois par mois, par catégorie"),
            None,
            &card,
        ));

        // Share of each category (categories without a colour listed
        // separately).
        let rows = stats::monthly_by_category(conn, scope, &months)?;
        let grid = gtk::Grid::builder()
            .column_spacing(16)
            .row_spacing(0)
            .css_classes(["data-table"])
            .build();
        let mut col = 0;
        let cell =
            |grid: &gtk::Grid, text: &str, col: i32, row: i32, xalign: f32, classes: &[&str]| {
                let label = gtk::Label::builder()
                    .label(text)
                    .xalign(xalign)
                    .margin_top(9)
                    .margin_bottom(9)
                    .css_classes(classes.iter().map(|s| s.to_string()).collect::<Vec<_>>())
                    .build();
                grid.attach(&label, col, row, 1, 1);
                label
            };
        cell(
            &grid,
            &gettext("Catégorie"),
            col,
            0,
            0.0,
            &["caption", "dim-label", "header"],
        )
        .set_hexpand(true);
        col += 1;
        if !narrow {
            for m in &months {
                cell(
                    &grid,
                    dates::month_short(m.month()),
                    col,
                    0,
                    1.0,
                    &["caption", "dim-label", "header"],
                );
                col += 1;
            }
        }
        let share_col = col;
        cell(
            &grid,
            &gettext("{n} mois").replace("{n}", &n.to_string()),
            share_col,
            0,
            1.0,
            &["caption", "dim-label", "header"],
        );
        cell(
            &grid,
            &gettext("Cumul"),
            share_col + 1,
            0,
            1.0,
            &["caption", "dim-label", "header"],
        );
        if !narrow {
            cell(
                &grid,
                &gettext("Par mois"),
                share_col + 2,
                0,
                1.0,
                &["caption", "dim-label", "header"],
            );
        }
        for (r, row) in rows.iter().enumerate() {
            let y = r as i32 + 1;
            let name = gtk::Box::builder().spacing(8).build();
            let slot = row.slot;
            name.append(&charts::color_swatch(12, move |w| {
                charts::category_color(w, slot)
            }));
            name.append(&gtk::Label::builder().label(&row.name).xalign(0.0).build());
            grid.attach(&name, 0, y, 1, 1);
            if !narrow {
                for (i, v) in row.months.iter().enumerate() {
                    cell(
                        &grid,
                        &pct1_label(*v, totals[i]),
                        i as i32 + 1,
                        y,
                        1.0,
                        &["dim-label"],
                    );
                }
            }
            cell(&grid, &pct1_label(row.total, grand), share_col, y, 1.0, &[]);
            cell(
                &grid,
                &amount(row.total, cur, Sign::Negative),
                share_col + 1,
                y,
                1.0,
                &[],
            );
            if !narrow {
                cell(
                    &grid,
                    &amount(row.total / n as i64, cur, Sign::Negative),
                    share_col + 2,
                    y,
                    1.0,
                    &["dim-label"],
                );
            }
        }
        let y = rows.len() as i32 + 1;
        cell(&grid, &gettext("Total"), 0, y, 0.0, &["heading"]);
        if !narrow {
            for (i, t) in totals.iter().enumerate() {
                cell(
                    &grid,
                    &amount_whole(*t, cur, Sign::Negative),
                    i as i32 + 1,
                    y,
                    1.0,
                    &["heading"],
                );
            }
        }
        cell(
            &grid,
            &locale::current().percent_whole(100),
            share_col,
            y,
            1.0,
            &["heading"],
        );
        cell(
            &grid,
            &amount(grand, cur, Sign::Negative),
            share_col + 1,
            y,
            1.0,
            &["heading"],
        );
        if !narrow {
            cell(
                &grid,
                &amount(average, cur, Sign::Negative),
                share_col + 2,
                y,
                1.0,
                &["heading"],
            );
        }
        let scroller = gtk::ScrolledWindow::builder()
            .vscrollbar_policy(gtk::PolicyType::Never)
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .propagate_natural_height(true)
            .child(&grid)
            .build();
        let card = chart_card(&[scroller.upcast_ref()]);
        let note = caption(&if narrow {
            gettext("Part de chaque catégorie sur la période et montant cumulé.")
        } else {
            gettext("Les colonnes de mois donnent la part de la catégorie ; la ligne Total donne les dépenses du mois.")
        });
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        body.append(&card);
        body.append(&note);
        self.root
            .append(&section(&gettext("Part de chaque catégorie"), None, &body));

        let from = months[0];
        Ok(if from.year() == last.year() {
            gettext("{from} à {to}")
                .replace("{from}", &capitalized(dates::month_name(from.month())))
                .replace("{to}", &dates::month_year(last))
        } else {
            gettext("{from} à {to}")
                .replace("{from}", &capitalized(&dates::month_year(from)))
                .replace("{to}", &dates::month_year(last))
        })
    }
}
