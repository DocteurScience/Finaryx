//! Balance curves: grid lines with labels, optional previous period in
//! dashed grey (5/4), a 12 px dot on the last point with its value, and a
//! tooltip on hover.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

use super::{draw_text, redraw_on_theme_change, set_source, theme_color, Align, TextStyle};

#[derive(Debug, Clone, Default)]
pub struct LineChart {
    /// Values in minor units, oldest first.
    pub values: Vec<i64>,
    /// Previous period, drawn dashed in `finaryx_ghost`.
    pub ghost: Option<Vec<i64>>,
    /// Number of positions on the x axis (0: the longest series).
    pub slots: usize,
    /// Tooltip (Pango markup) for each position.
    pub tooltips: Vec<String>,
    /// Grid lines: (value, label), lowest first.
    pub ticks: Vec<(i64, String)>,
    /// Labels under the chart: (position, label).
    pub x_labels: Vec<(usize, String)>,
    /// Value written next to the dot of the last point.
    pub end_label: Option<String>,
    /// Labels of the grid lines in a left gutter (overview, month) rather
    /// than above each line (account detail).
    pub gutter: bool,
}

const X_LABELS_HEIGHT: f64 = 22.0;
const GUTTER: f64 = 64.0;
const END_MARGIN: f64 = 76.0;

struct Geometry {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    lo: f64,
    hi: f64,
    slots: usize,
}

impl Geometry {
    fn new(c: &LineChart, w: f64, h: f64) -> Option<Geometry> {
        let slots = if c.slots > 0 {
            c.slots
        } else {
            c.values.len().max(c.ghost.as_ref().map_or(0, Vec::len))
        };
        if slots < 2 || c.ticks.len() < 2 {
            return None;
        }
        let left = if c.gutter { GUTTER } else { 6.0 };
        let right = w - if c.end_label.is_some() {
            END_MARGIN
        } else {
            6.0
        };
        let top = if c.gutter { 10.0 } else { 18.0 };
        Some(Geometry {
            left,
            right,
            top,
            bottom: h - X_LABELS_HEIGHT,
            lo: c.ticks.first().unwrap().0 as f64,
            hi: c.ticks.last().unwrap().0 as f64,
            slots,
        })
    }

    fn x(&self, i: usize) -> f64 {
        self.left + i as f64 / (self.slots - 1) as f64 * (self.right - self.left)
    }

    fn y(&self, v: i64) -> f64 {
        self.top + (self.hi - v as f64) / (self.hi - self.lo) * (self.bottom - self.top)
    }

    fn nearest(&self, px: f64) -> usize {
        let t = ((px - self.left) / (self.right - self.left)).clamp(0.0, 1.0);
        (t * (self.slots - 1) as f64).round() as usize
    }
}

fn stroke_series(cr: &gtk::cairo::Context, g: &Geometry, values: &[i64]) {
    if values.is_empty() {
        return;
    }
    cr.move_to(g.x(0), g.y(values[0]));
    for (i, v) in values.iter().enumerate().skip(1) {
        cr.line_to(g.x(i), g.y(*v));
    }
    cr.set_line_width(2.0);
    cr.set_line_cap(gtk::cairo::LineCap::Round);
    cr.set_line_join(gtk::cairo::LineJoin::Round);
    let _ = cr.stroke();
}

pub fn line_chart(chart: LineChart, height: i32, accessible_label: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(height)
        .hexpand(true)
        .has_tooltip(true)
        .accessible_role(gtk::AccessibleRole::Img)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    let chart = Rc::new(chart);
    let hover: Rc<Cell<Option<usize>>> = Rc::default();

    let (c, hovered) = (chart.clone(), hover.clone());
    area.set_draw_func(move |area, cr, w, h| {
        let Some(g) = Geometry::new(&c, w as f64, h as f64) else {
            return;
        };
        let mut line_color = area.color();
        line_color.set_alpha(0.08);

        for (value, label) in &c.ticks {
            let yy = g.y(*value).round() + 0.5;
            set_source(cr, &line_color);
            cr.set_line_width(1.0);
            cr.move_to(g.left, yy);
            cr.line_to(if c.gutter { g.right } else { w as f64 }, yy);
            let _ = cr.stroke();
            if c.gutter {
                draw_text(
                    area,
                    cr,
                    label,
                    GUTTER - 10.0,
                    yy,
                    Align::EndMiddle,
                    TextStyle::Dim,
                );
            } else {
                draw_text(
                    area,
                    cr,
                    label,
                    0.0,
                    yy - 2.0,
                    Align::StartBottom,
                    TextStyle::Dim,
                );
            }
        }
        for (i, label) in &c.x_labels {
            draw_text(
                area,
                cr,
                label,
                g.x(*i),
                g.bottom + 6.0,
                Align::CenterTop,
                TextStyle::Dim,
            );
        }

        if let Some(ghost) = &c.ghost {
            set_source(cr, &theme_color(area, "finaryx_ghost"));
            cr.set_dash(&[5.0, 4.0], 0.0);
            stroke_series(cr, &g, ghost);
            cr.set_dash(&[], 0.0);
        }
        let s1 = theme_color(area, "finaryx_s1");
        set_source(cr, &s1);
        stroke_series(cr, &g, &c.values);

        // 12 px dot on the last point, with its value.
        if let (Some(label), Some(last)) = (&c.end_label, c.values.last()) {
            let (x, y) = (g.x(c.values.len() - 1), g.y(*last));
            set_source(cr, &theme_color(area, "card_bg_color"));
            cr.arc(x, y, 6.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            set_source(cr, &s1);
            cr.arc(x, y, 4.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
            draw_text(
                area,
                cr,
                label,
                x + 12.0,
                y,
                Align::StartMiddle,
                TextStyle::Bold,
            );
        }

        if let Some(i) = hovered.get() {
            let mut guide = area.color();
            guide.set_alpha(0.3);
            set_source(cr, &guide);
            cr.set_line_width(1.0);
            cr.set_dash(&[3.0, 3.0], 0.0);
            cr.move_to(g.x(i).round() + 0.5, g.top);
            cr.line_to(g.x(i).round() + 0.5, g.bottom);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
            if let Some(v) = c.ghost.as_ref().and_then(|gh| gh.get(i)) {
                set_source(cr, &theme_color(area, "finaryx_ghost"));
                cr.arc(g.x(i), g.y(*v), 4.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            }
            if let Some(v) = c.values.get(i) {
                set_source(cr, &s1);
                cr.arc(g.x(i), g.y(*v), 5.0, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            }
        }
    });

    let motion = gtk::EventControllerMotion::new();
    let (c, hovered, weak) = (chart.clone(), hover.clone(), area.downgrade());
    motion.connect_motion(move |_, x, _| {
        let Some(area) = weak.upgrade() else { return };
        if let Some(g) = Geometry::new(&c, area.width() as f64, area.height() as f64) {
            hovered.set(Some(g.nearest(x)));
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
    area.connect_query_tooltip(move |area, px, _py, _kb, tooltip| {
        let Some(g) = Geometry::new(&c, area.width() as f64, area.height() as f64) else {
            return false;
        };
        match c.tooltips.get(g.nearest(px as f64)) {
            Some(text) => {
                tooltip.set_markup(Some(text));
                true
            }
            None => false,
        }
    });
    redraw_on_theme_change(&area);
    area
}
