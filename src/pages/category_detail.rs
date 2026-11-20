//! Detail of one category over a period, opened from the month and
//! cumulative pages: totals, then every operation, month by month.

use adw::prelude::*;
use chrono::{Datelike, NaiveDate};
use finaryx_core::categories;
use finaryx_core::dates;
use finaryx_core::db::Result;
use finaryx_core::models::Id;
use finaryx_core::money::Sign;
use finaryx_core::rusqlite::Connection;
use finaryx_core::stats::{self, OperationFilter, Scope};
use gettextrs::gettext;

use super::month::operation_row;
use super::{amount, boxed_list, capitalized, caption, heading, kpi_card, n_ops, section, KpiSize};
use crate::charts;
use crate::window::FinaryxWindow;

pub const TAG: &str = "category";

/// "Mai à octobre 2026", "Octobre 2026".
pub fn period_label(from: NaiveDate, last: NaiveDate) -> String {
    if from == last {
        capitalized(&dates::month_year(from))
    } else if from.year() == last.year() {
        gettext("{from} à {to}")
            .replace("{from}", &capitalized(dates::month_name(from.month())))
            .replace("{to}", &dates::month_year(last))
    } else {
        gettext("{from} à {to}")
            .replace("{from}", &capitalized(&dates::month_year(from)))
            .replace("{to}", &dates::month_year(last))
    }
}

/// Builds the page of `category_id` from the month `from` to the month
/// `to` (excluded).
pub fn page(
    window: &FinaryxWindow,
    conn: &Connection,
    scope: &Scope,
    category_id: Id,
    from: NaiveDate,
    to: NaiveDate,
    narrow: bool,
) -> Result<adw::NavigationPage> {
    let category = categories::list_for_display(conn)?
        .into_iter()
        .find(|c| c.id == category_id);
    let name = category
        .as_ref()
        .map_or_else(|| gettext("Sans catégorie"), |c| c.name.clone());
    let slot = category.as_ref().and_then(|c| c.slot);
    let cur = scope.currency.as_str();
    let last = stats::previous_month(to);
    let period = period_label(from, last);

    let months = stats::category_months(conn, scope, category_id, from, to)?;
    let filter = OperationFilter {
        category_id: Some(category_id),
        from: Some(from),
        to: Some(to),
        ..Default::default()
    };
    let operations = stats::operations(conn, scope, &filter, 10_000, 0)?.items;

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(if narrow { 12 } else { 24 })
        .margin_end(if narrow { 12 } else { 24 })
        .build();

    // Name with its colour, then the totals.
    let title = gtk::Box::builder().spacing(10).build();
    title.append(&charts::color_swatch(14, move |w| {
        charts::category_color(w, slot)
    }));
    let title_label = heading(&name);
    title_label.add_css_class("title-3");
    title_label.set_wrap(true);
    title_label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    title.append(&title_label);
    content.append(&title);

    let outflow: i64 = months.iter().map(|m| m.outflow).sum();
    let inflow: i64 = months.iter().map(|m| m.inflow).sum();
    let count: i64 = months.iter().map(|m| m.operations).sum();
    let size = if narrow {
        KpiSize::Small
    } else {
        KpiSize::Medium
    };
    let kpis = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(2)
        .max_children_per_line(3)
        .homogeneous(true)
        .column_spacing(12)
        .row_spacing(12)
        .build();
    kpis.append(&kpi_card(
        &gettext("Sorties"),
        &amount(outflow, cur, Sign::Negative),
        &period,
        size,
    ));
    if inflow > 0 {
        kpis.append(&kpi_card(
            &gettext("Entrées"),
            &amount(inflow, cur, Sign::Negative),
            &gettext("Remboursements et autres crédits"),
            size,
        ));
    }
    kpis.append(&kpi_card(
        &gettext("Opérations"),
        &count.to_string(),
        "",
        size,
    ));
    super::unfocusable_children(&kpis);
    content.append(&kpis);

    if operations.is_empty() {
        content.append(&caption(&gettext(
            "Aucune opération dans cette catégorie sur la période.",
        )));
    }

    // Month by month, when the period has several.
    if months.len() > 1 {
        let list = boxed_list();
        for m in &months {
            let row = adw::ActionRow::builder()
                .title(capitalized(&dates::month_year(m.month)))
                .subtitle(n_ops(m.operations))
                .build();
            row.add_suffix(
                &gtk::Label::builder()
                    .label(amount(m.outflow - m.inflow, cur, Sign::Negative))
                    .css_classes(["numeric"])
                    .build(),
            );
            list.append(&row);
        }
        content.append(&section(heading(&gettext("Par mois")).upcast_ref(), &list));
    }

    // Every operation, under its month.
    let mut start = 0;
    while start < operations.len() {
        let month = stats::first_of_month(operations[start].booked_on);
        let end = operations[start..]
            .iter()
            .position(|o| stats::first_of_month(o.booked_on) != month)
            .map_or(operations.len(), |n| start + n);
        let list = boxed_list();
        for op in &operations[start..end] {
            // The category column would repeat the page's name.
            list.append(&operation_row(op, cur, true));
        }
        content.append(&section(
            heading(&capitalized(&dates::month_year(month))).upcast_ref(),
            &list,
        ));
        start = end;
    }

    // Changing the category of an operation happens in the operations list.
    let open = gtk::Button::builder()
        .label(gettext("Modifier dans la liste des opérations"))
        .halign(gtk::Align::Center)
        .css_classes(["pill"])
        .build();
    let weak = window.downgrade();
    let single_month = (last == from).then_some(from);
    open.connect_clicked(move |_| {
        if let Some(w) = weak.upgrade() {
            w.show_operations(w.selected_account(), single_month, Some(category_id));
        }
    });
    if !operations.is_empty() {
        content.append(&open);
    }

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
    let header = adw::HeaderBar::builder()
        .title_widget(&adw::WindowTitle::new(&name, &period))
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scrolled));
    Ok(adw::NavigationPage::builder()
        .tag(TAG)
        .title(&name)
        .child(&toolbar)
        .build())
}
