//! Mini balance curve of an account (no axis), 84 × 32 px.

use adw::prelude::*;

use super::{redraw_on_theme_change, set_source};

/// Builds a sparkline of `values` (oldest first). `accessible_label`
/// describes the trend in words, since the curve alone carries it.
pub fn sparkline(values: Vec<i64>, accessible_label: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(84)
        .content_height(32)
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Img)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    area.set_draw_func(move |area, cr, w, h| {
        if values.len() < 2 {
            return;
        }
        let min = *values.iter().min().unwrap() as f64;
        let max = *values.iter().max().unwrap() as f64;
        let span = if max > min { max - min } else { 1.0 };
        // Same margins as the mockup: 8 % top and bottom.
        let (w, h) = (w as f64, h as f64);
        let x = |i: usize| 1.0 + i as f64 / (values.len() - 1) as f64 * (w - 2.0);
        let y = |v: i64| h * 0.92 - (v as f64 - min) / span * h * 0.84;
        cr.move_to(x(0), y(values[0]));
        for (i, v) in values.iter().enumerate().skip(1) {
            cr.line_to(x(i), y(*v));
        }
        set_source(cr, &area.color());
        cr.set_line_width(2.0);
        cr.set_line_cap(gtk::cairo::LineCap::Round);
        cr.set_line_join(gtk::cairo::LineJoin::Round);
        let _ = cr.stroke();
    });
    redraw_on_theme_change(&area);
    area
}
