//! Chart tooltips (reference/tooltips.png): a bold title, rows of a mark,
//! a label and a right-aligned value, then a separator and a footer line.
//! They are shown in the native `GtkTooltip` (`set_custom`).

use std::cell::RefCell;

use adw::prelude::*;

use super::{account_color, category_color, color_swatch, legend_mark, LegendMark};

/// Mark at the start of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TipMark {
    None,
    /// A line or square of a legend.
    Legend(LegendMark),
    /// Square of a category colour (slot, or `None` for "Autres").
    Category(Option<u8>),
    /// Square of an account colour.
    Account(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TipRow {
    pub mark: TipMark,
    pub label: String,
    pub value: String,
}

impl TipRow {
    pub fn new(mark: TipMark, label: impl Into<String>, value: impl Into<String>) -> Self {
        TipRow {
            mark,
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tip {
    /// Plain text, shown in bold.
    pub title: String,
    pub rows: Vec<TipRow>,
    /// Pango markup, under a separator.
    pub footer: Option<String>,
}

impl Tip {
    pub fn new(title: impl Into<String>) -> Self {
        Tip {
            title: title.into(),
            ..Default::default()
        }
    }

    pub fn row(
        mut self,
        mark: TipMark,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.rows.push(TipRow::new(mark, label, value));
        self
    }

    pub fn footer(mut self, markup: impl Into<String>) -> Self {
        self.footer = Some(markup.into());
        self
    }

    pub fn widget(&self) -> gtk::Widget {
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(4)
            .margin_bottom(4)
            .margin_start(4)
            .margin_end(4)
            .build();
        content.append(
            &gtk::Label::builder()
                .label(&self.title)
                .xalign(0.0)
                .css_classes(["heading"])
                .build(),
        );
        if !self.rows.is_empty() {
            let grid = gtk::Grid::builder()
                .column_spacing(24)
                .row_spacing(4)
                .build();
            for (i, row) in self.rows.iter().enumerate() {
                let left = gtk::Box::builder().spacing(8).build();
                match row.mark {
                    TipMark::None => {}
                    TipMark::Legend(kind) => {
                        let m = legend_mark(kind);
                        m.set_content_width(14);
                        left.append(&m);
                    }
                    TipMark::Category(slot) => {
                        left.append(&color_swatch(10, move |w| category_color(w, slot)));
                    }
                    TipMark::Account(slot) => {
                        left.append(&color_swatch(10, move |w| account_color(w, slot)));
                    }
                }
                left.append(&gtk::Label::builder().label(&row.label).xalign(0.0).build());
                grid.attach(&left, 0, i as i32, 1, 1);
                grid.attach(
                    &gtk::Label::builder()
                        .label(&row.value)
                        .xalign(1.0)
                        .hexpand(true)
                        .css_classes(["numeric"])
                        .build(),
                    1,
                    i as i32,
                    1,
                    1,
                );
            }
            content.append(&grid);
        }
        if let Some(footer) = &self.footer {
            content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
            content.append(
                &gtk::Label::builder()
                    .label(footer)
                    .use_markup(true)
                    .xalign(0.0)
                    .build(),
            );
        }
        content.upcast()
    }
}

/// Keeps the widget of the last tooltip shown, so that moving inside the
/// same mark does not rebuild it.
#[derive(Debug, Default)]
pub struct TipCache(RefCell<Option<(usize, gtk::Widget)>>);

impl TipCache {
    /// Shows the tooltip of mark `key`.
    pub fn show(&self, tooltip: &gtk::Tooltip, key: usize, tip: &Tip) -> bool {
        let mut cache = self.0.borrow_mut();
        let widget = match cache.as_ref() {
            Some((k, w)) if *k == key => w.clone(),
            _ => {
                let w = tip.widget();
                *cache = Some((key, w.clone()));
                w
            }
        };
        tooltip.set_custom(Some(&widget));
        true
    }
}
