//! Spending of each day of the month, under the balance curve, with the
//! amount written above the largest day.

use std::rc::Rc;

use adw::prelude::*;

use super::{draw_text, redraw_on_theme_change, set_source, theme_color, Align, TextStyle};
use super::{tip::TipCache, Tip};

#[derive(Debug, Clone)]
pub struct Bars {
    pub values: Vec<i64>,
    /// Number of positions (days of the month).
    pub slots: usize,
    pub ticks: Vec<(i64, String)>,
    /// Text above the largest bar ("Loyer 690 €").
    pub peak_label: Option<String>,
    /// Tooltip of each bar.
    pub tooltips: Vec<Tip>,
}

const GUTTER: f64 = 64.0;
const END_MARGIN: f64 = 76.0;
const TOP: f64 = 18.0;

fn bar_at(b: &Bars, w: f64, x: f64) -> Option<usize> {
    let slot = (w - GUTTER - END_MARGIN) / b.slots.max(1) as f64;
    let i = ((x - GUTTER) / slot).floor();
    (i >= 0.0 && (i as usize) < b.values.len()).then_some(i as usize)
}

pub fn bars(chart: Bars, height: i32, accessible_label: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(height)
        .hexpand(true)
        .has_tooltip(true)
        .accessible_role(gtk::AccessibleRole::Img)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    let chart = Rc::new(chart);
    let c = chart.clone();
    area.set_draw_func(move |area, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let max = c.ticks.last().map_or(1, |t| t.0.max(1)) as f64;
        let bottom = h - 8.0;
        let y = |v: f64| bottom - v / max * (bottom - TOP);
        let slot = (w - GUTTER - END_MARGIN) / c.slots.max(1) as f64;
        let mut grid = area.color();
        grid.set_alpha(super::grid_alpha());
        for (value, label) in &c.ticks {
            let yy = y(*value as f64).round() + 0.5;
            set_source(cr, &grid);
            cr.set_line_width(1.0);
            cr.move_to(GUTTER, yy);
            cr.line_to(w - END_MARGIN, yy);
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
        set_source(cr, &theme_color(area, "finaryx_bars"));
        let peak = c
            .values
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| **v)
            .map(|(i, _)| i);
        for (i, v) in c.values.iter().enumerate() {
            let v = (*v).max(0) as f64;
            let bw = slot * 0.64;
            let x = GUTTER + i as f64 * slot + (slot - bw) / 2.0;
            cr.rectangle(x, y(v), bw, bottom - y(v));
        }
        let _ = cr.fill();
        if let (Some(i), Some(label)) = (peak, &c.peak_label) {
            let x = GUTTER + (i as f64 + 0.5) * slot;
            let yy = y(c.values[i].max(0) as f64) - 4.0;
            draw_text(
                area,
                cr,
                label,
                x,
                yy.max(12.0),
                Align::CenterBottom,
                TextStyle::Bold,
            );
        }
    });
    let c = chart.clone();
    let cache = TipCache::default();
    area.connect_query_tooltip(move |area, x, _y, _kb, tooltip| {
        match bar_at(&c, area.width() as f64, x as f64).and_then(|i| Some((i, c.tooltips.get(i)?)))
        {
            Some((i, tip)) => cache.show(tooltip, i, tip),
            None => false,
        }
    });
    redraw_on_theme_change(&area);
    area
}
