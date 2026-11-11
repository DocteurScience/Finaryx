//! Balance curve with horizontal grid lines, labels and a tooltip on
//! hover (account detail; the month view reuses it at step 5).

use adw::prelude::*;
use gtk::pango;

use super::{redraw_on_theme_change, set_source, theme_color};

#[derive(Debug, Clone)]
pub struct LineChart {
    /// Values in minor units, oldest first.
    pub values: Vec<i64>,
    /// Tooltip text for each value ("12 oct. : 4 012,34 €").
    pub tooltips: Vec<String>,
    /// Grid lines: (value, label).
    pub ticks: Vec<(i64, String)>,
    /// Labels under the chart: (index of the value, label).
    pub x_labels: Vec<(usize, String)>,
}

const X_LABELS_HEIGHT: f64 = 22.0;

pub fn line_chart(chart: LineChart, height: i32, accessible_label: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(height)
        .hexpand(true)
        .has_tooltip(true)
        .accessible_role(gtk::AccessibleRole::Img)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    let hover = std::rc::Rc::new(std::cell::Cell::new(None::<usize>));

    let c = chart.clone();
    let hovered = hover.clone();
    area.set_draw_func(move |area, cr, w, h| {
        let (w, h) = (w as f64, h as f64 - X_LABELS_HEIGHT);
        if c.values.len() < 2 || c.ticks.len() < 2 {
            return;
        }
        let lo = c.ticks.first().unwrap().0 as f64;
        let hi = c.ticks.last().unwrap().0 as f64;
        let top = 18.0; // room for the label of the highest grid line
        let y = |v: i64| top + (hi - v as f64) / (hi - lo) * (h - top);
        let x = |i: usize| 6.0 + i as f64 / (c.values.len() - 1) as f64 * (w - 12.0);
        let dim = {
            let mut fg = area.color();
            fg.set_alpha(0.65);
            fg
        };

        // Grid lines (1 px, very light) with their label above, left.
        let mut line_color = area.color();
        line_color.set_alpha(0.08);
        for (value, label) in &c.ticks {
            let yy = y(*value).round() + 0.5;
            set_source(cr, &line_color);
            cr.set_line_width(1.0);
            cr.move_to(0.0, yy);
            cr.line_to(w, yy);
            let _ = cr.stroke();
            let layout = area.create_pango_layout(Some(label));
            layout.set_attributes(Some(&small_attrs()));
            let (_, lh) = layout.pixel_size();
            set_source(cr, &dim);
            cr.move_to(0.0, yy - lh as f64 - 2.0);
            pangocairo::functions::show_layout(cr, &layout);
        }

        // Month labels under the chart.
        for (i, label) in &c.x_labels {
            let layout = area.create_pango_layout(Some(label));
            layout.set_attributes(Some(&small_attrs()));
            let (lw, _) = layout.pixel_size();
            let lx = (x(*i) - lw as f64 / 2.0).clamp(0.0, w - lw as f64);
            set_source(cr, &dim);
            cr.move_to(lx, h + 6.0);
            pangocairo::functions::show_layout(cr, &layout);
        }

        // The curve: 2 px, round caps.
        cr.move_to(x(0), y(c.values[0]));
        for (i, v) in c.values.iter().enumerate().skip(1) {
            cr.line_to(x(i), y(*v));
        }
        set_source(cr, &theme_color(area, "finaryx_s1"));
        cr.set_line_width(2.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        let _ = cr.stroke();

        // Hovered point.
        if let Some(i) = hovered.get() {
            let mut guide = area.color();
            guide.set_alpha(0.25);
            set_source(cr, &guide);
            cr.set_line_width(1.0);
            cr.set_dash(&[3.0, 3.0], 0.0);
            cr.move_to(x(i).round() + 0.5, top);
            cr.line_to(x(i).round() + 0.5, h);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
            set_source(cr, &theme_color(area, "finaryx_s1"));
            cr.arc(x(i), y(c.values[i]), 5.0, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();
        }
    });

    let nearest = move |width: f64, px: f64, n: usize| -> usize {
        let t = ((px - 6.0) / (width - 12.0)).clamp(0.0, 1.0);
        (t * (n - 1) as f64).round() as usize
    };

    let motion = gtk::EventControllerMotion::new();
    let hovered = hover.clone();
    let n = chart.values.len();
    motion.connect_motion(glib_clone_area(&area, move |area, px| {
        if n >= 2 {
            hovered.set(Some(nearest(area.width() as f64, px, n)));
            area.queue_draw();
        }
    }));
    let hovered = hover.clone();
    let weak = area.downgrade();
    motion.connect_leave(move |_| {
        hovered.set(None);
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    area.add_controller(motion);

    area.connect_query_tooltip(move |area, px, _py, _kb, tooltip| {
        if chart.values.len() < 2 {
            return false;
        }
        let i = nearest(area.width() as f64, px as f64, chart.values.len());
        match chart.tooltips.get(i) {
            Some(text) => {
                tooltip.set_text(Some(text));
                true
            }
            None => false,
        }
    });
    redraw_on_theme_change(&area);
    area
}

fn small_attrs() -> pango::AttrList {
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFloat::new_scale(0.85));
    attrs.insert(pango::AttrFontFeatures::new("tnum"));
    attrs
}

/// Wraps a motion handler so that it receives the drawing area.
fn glib_clone_area(
    area: &gtk::DrawingArea,
    f: impl Fn(&gtk::DrawingArea, f64) + 'static,
) -> impl Fn(&gtk::EventControllerMotion, f64, f64) + 'static {
    let weak = area.downgrade();
    move |_, x, _| {
        if let Some(area) = weak.upgrade() {
            f(&area, x);
        }
    }
}
