//! Horizontal bar split by account (share of the total balance).

use adw::prelude::*;

use super::{account_color, rounded_rect, set_source, theme_color};

/// One segment: share in tenths of a percent, account colour slot, and
/// the tooltip text.
#[derive(Debug, Clone)]
pub struct Segment {
    pub per_mille: u32,
    pub color_slot: u8,
    pub tooltip: String,
}

const HEIGHT: i32 = 16;
const GAP: f64 = 2.0;
const RADIUS: f64 = 5.0;

fn layout(segments: &[Segment], width: f64) -> Vec<(f64, f64)> {
    let visible = segments.iter().filter(|s| s.per_mille > 0).count();
    let usable = (width - GAP * visible.saturating_sub(1) as f64).max(0.0);
    let mut x = 0.0;
    segments
        .iter()
        .map(|s| {
            if s.per_mille == 0 {
                return (x, 0.0);
            }
            let w = usable * s.per_mille as f64 / 1000.0;
            let start = x;
            x += w + GAP;
            (start, w)
        })
        .collect()
}

pub fn share_bar(segments: Vec<Segment>, accessible_label: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(HEIGHT)
        .hexpand(true)
        .has_tooltip(true)
        .accessible_role(gtk::AccessibleRole::Img)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(accessible_label)]);

    let segs = segments.clone();
    area.set_draw_func(move |area, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        // Clip to the rounded bar, then draw the segments.
        rounded_rect(cr, 0.0, 0.0, w, h, RADIUS);
        cr.clip();
        if segs.iter().all(|s| s.per_mille == 0) {
            set_source(cr, &theme_color(area, "finaryx_c8"));
            let _ = cr.paint();
            return;
        }
        for (s, (x, sw)) in segs.iter().zip(layout(&segs, w)) {
            if sw <= 0.0 {
                continue;
            }
            set_source(cr, &account_color(area, s.color_slot));
            cr.rectangle(x, 0.0, sw, h);
            let _ = cr.fill();
        }
    });

    area.connect_query_tooltip(move |area, x, _y, _kb, tooltip| {
        let width = area.width() as f64;
        for (s, (sx, sw)) in segments.iter().zip(layout(&segments, width)) {
            if sw > 0.0 && (x as f64) >= sx && (x as f64) <= sx + sw {
                tooltip.set_text(Some(&s.tooltip));
                return true;
            }
        }
        false
    });
    super::redraw_on_theme_change(&area);
    area
}
