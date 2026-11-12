//! Stacked columns per month (specification §6): fixed category order
//! from the bottom, a 2 px gap of the card colour between segments,
//! rounded corners on the top segment only, the total above each column,
//! and the share of the category written in each segment at least 16 px
//! high.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use finaryx_core::categories::CHART_GROUPS;

use super::{
    category_color, category_ink, draw_text, redraw_on_theme_change, rounded_rect, set_source,
    theme_color, Align, TextStyle,
};

#[derive(Debug, Clone)]
pub struct Column {
    pub label: String,
    /// Amount per chart group (index 0 = slot 1 … index 12 = "Autres").
    pub values: [i64; CHART_GROUPS],
    /// Total written above the column.
    pub total_label: String,
    /// Share written in each segment ("31 %").
    pub segment_labels: [String; CHART_GROUPS],
    /// Tooltip (markup) of each segment.
    pub tooltips: [String; CHART_GROUPS],
}

#[derive(Debug, Clone)]
pub struct Columns {
    pub columns: Vec<Column>,
    pub ticks: Vec<(i64, String)>,
}

const GUTTER: f64 = 64.0;
const BOTTOM: f64 = 26.0;
const TOP: f64 = 22.0;
const GAP: f64 = 2.0;
const MIN_LABEL_HEIGHT: f64 = 16.0;

struct Geometry {
    w: f64,
    h: f64,
    max: f64,
    n: usize,
}

impl Geometry {
    fn slot_width(&self) -> f64 {
        (self.w - GUTTER) / self.n as f64
    }
    fn column_x(&self, m: usize) -> (f64, f64) {
        let slot = self.slot_width();
        let width = (slot * 0.68).min(64.0);
        (GUTTER + m as f64 * slot + (slot - width) / 2.0, width)
    }
    fn y(&self, v: f64) -> f64 {
        self.h - BOTTOM - v / self.max * (self.h - BOTTOM - TOP)
    }
    /// Segment under the pointer: (month, slot index).
    fn hit(&self, c: &Columns, x: f64, y: f64) -> Option<(usize, usize)> {
        for (m, col) in c.columns.iter().enumerate() {
            let (cx, cw) = self.column_x(m);
            if x < cx || x > cx + cw {
                continue;
            }
            let mut base = 0.0;
            for (k, v) in col.values.iter().enumerate() {
                let v = (*v).max(0) as f64;
                if v > 0.0 && y <= self.y(base) && y >= self.y(base + v) {
                    return Some((m, k));
                }
                base += v;
            }
        }
        None
    }
}

pub fn stacked_columns(chart: Columns, height: i32, accessible_label: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(height)
        .hexpand(true)
        .has_tooltip(true)
        .accessible_role(gtk::AccessibleRole::Img)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    let chart = Rc::new(chart);
    let max = chart.ticks.last().map_or(1, |t| t.0.max(1)) as f64;
    let hover: Rc<Cell<Option<(usize, usize)>>> = Rc::default();

    let (c, hovered) = (chart.clone(), hover.clone());
    area.set_draw_func(move |area, cr, w, h| {
        if c.columns.is_empty() {
            return;
        }
        let g = Geometry {
            w: w as f64,
            h: h as f64,
            max,
            n: c.columns.len(),
        };
        let mut grid = area.color();
        grid.set_alpha(0.08);
        for (value, label) in &c.ticks {
            let yy = g.y(*value as f64).round() + 0.5;
            set_source(cr, &grid);
            cr.set_line_width(1.0);
            cr.move_to(GUTTER, yy);
            cr.line_to(g.w, yy);
            let _ = cr.stroke();
            draw_text(
                area,
                cr,
                label,
                GUTTER - 10.0,
                yy,
                Align::EndMiddle,
                TextStyle::Dim,
            );
        }
        let card = theme_color(area, "card_bg_color");
        for (m, col) in c.columns.iter().enumerate() {
            let (x, cw) = g.column_x(m);
            let top_index = col.values.iter().rposition(|v| *v > 0);
            let mut base = 0.0;
            for (k, v) in col.values.iter().enumerate() {
                let v = (*v).max(0) as f64;
                if v <= 0.0 {
                    continue;
                }
                let (y0, y1) = (g.y(base), g.y(base + v));
                let mut color = category_color(area, Some(k as u8 + 1));
                if hovered.get().is_some_and(|(hm, hk)| (hm, hk) != (m, k)) {
                    color.set_alpha(0.45);
                }
                set_source(cr, &color);
                if Some(k) == top_index {
                    // Rounded top corners only: round the whole rectangle,
                    // then square the bottom.
                    rounded_rect(cr, x, y1, cw, y0 - y1, 3.0);
                    let _ = cr.fill();
                    cr.rectangle(x, y1 + 3.0, cw, (y0 - y1 - 3.0).max(0.0));
                } else {
                    cr.rectangle(x, y1, cw, y0 - y1);
                }
                let _ = cr.fill();
                // 2 px gap of the card colour above each segment.
                if Some(k) != top_index {
                    set_source(cr, &card);
                    cr.rectangle(x, y1 - GAP / 2.0, cw, GAP);
                    let _ = cr.fill();
                }
                if y0 - y1 >= MIN_LABEL_HEIGHT && !col.segment_labels[k].is_empty() {
                    let ink = category_ink(area, Some(k as u8 + 1));
                    draw_text(
                        area,
                        cr,
                        &col.segment_labels[k],
                        x + cw / 2.0,
                        (y0 + y1) / 2.0,
                        Align::Center,
                        TextStyle::OnColor(ink),
                    );
                }
                base += v;
            }
            draw_text(
                area,
                cr,
                &col.total_label,
                x + cw / 2.0,
                g.y(base) - 4.0,
                Align::CenterBottom,
                TextStyle::Bold,
            );
            draw_text(
                area,
                cr,
                &col.label,
                x + cw / 2.0,
                g.h - BOTTOM + 8.0,
                Align::CenterTop,
                TextStyle::Dim,
            );
        }
        let mut axis = area.color();
        axis.set_alpha(0.25);
        set_source(cr, &axis);
        cr.set_line_width(1.0);
        let base_y = g.y(0.0).round() + 0.5;
        cr.move_to(GUTTER, base_y);
        cr.line_to(g.w, base_y);
        let _ = cr.stroke();
    });

    let motion = gtk::EventControllerMotion::new();
    let (c, hovered, weak) = (chart.clone(), hover.clone(), area.downgrade());
    motion.connect_motion(move |_, x, y| {
        let Some(area) = weak.upgrade() else { return };
        let g = Geometry {
            w: area.width() as f64,
            h: area.height() as f64,
            max,
            n: c.columns.len().max(1),
        };
        let hit = g.hit(&c, x, y);
        if hit != hovered.get() {
            hovered.set(hit);
            area.queue_draw();
        }
    });
    let (hovered, weak) = (hover.clone(), area.downgrade());
    motion.connect_leave(move |_| {
        hovered.set(None);
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area.add_controller(motion);

    let c = chart.clone();
    area.connect_query_tooltip(move |area, x, y, _kb, tooltip| {
        let g = Geometry {
            w: area.width() as f64,
            h: area.height() as f64,
            max,
            n: c.columns.len().max(1),
        };
        match g.hit(&c, x as f64, y as f64) {
            Some((m, k)) => {
                tooltip.set_markup(Some(&c.columns[m].tooltips[k]));
                true
            }
            None => false,
        }
    });
    redraw_on_theme_change(&area);
    area
}
