//! "Où part l'argent" ring (specification §6): parts sorted by amount,
//! from noon clockwise, constant 3 px separators of the card colour, a
//! hole of about 58 % with the total, the percentage written in each part
//! (outside the ring under 5 %), a tooltip per part, and a click on
//! "Autres" for its detail.

use std::cell::Cell;
use std::f64::consts::{FRAC_PI_2, TAU};
use std::rc::Rc;

use adw::prelude::*;

use super::{
    category_color, draw_text, redraw_on_theme_change, set_source, theme_color, Align, TextStyle,
};

#[derive(Debug, Clone)]
pub struct Part {
    /// Colour slot 1..=8.
    pub slot: u8,
    pub amount: i64,
    /// Percentage written in the part ("31 %").
    pub label: String,
    /// Tooltip (Pango markup).
    pub tooltip: String,
}

#[derive(Debug, Clone)]
pub struct Donut {
    /// Largest first.
    pub parts: Vec<Part>,
    /// Centre: total and "dépensés".
    pub center: String,
    pub center_caption: String,
}

const SEPARATOR: f64 = 3.0;
const HOLE: f64 = 0.58;
const SMALL: f64 = 0.05;

/// Angles (start, end) of each part, from noon clockwise.
fn angles(parts: &[Part]) -> Vec<(f64, f64)> {
    let total: i64 = parts.iter().map(|p| p.amount.max(0)).sum();
    let mut start = -FRAC_PI_2;
    parts
        .iter()
        .map(|p| {
            let sweep = if total > 0 {
                p.amount.max(0) as f64 / total as f64 * TAU
            } else {
                0.0
            };
            let a = (start, start + sweep);
            start += sweep;
            a
        })
        .collect()
}

fn part_at(parts: &[Part], w: f64, h: f64, x: f64, y: f64) -> Option<usize> {
    let (cx, cy) = (w / 2.0, h / 2.0);
    let radius = (w.min(h) / 2.0) - 22.0;
    let (dx, dy) = (x - cx, y - cy);
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < radius * HOLE || dist > radius + 4.0 {
        return None;
    }
    let mut a = dy.atan2(dx);
    if a < -FRAC_PI_2 {
        a += TAU;
    }
    angles(parts).iter().position(|(s, e)| a >= *s && a < *e)
}

/// `on_click` receives the slot of the clicked part.
pub fn donut(
    chart: Donut,
    size: i32,
    accessible_label: &str,
    on_click: impl Fn(u8) + 'static,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(size)
        .content_height(size)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .has_tooltip(true)
        .accessible_role(gtk::AccessibleRole::Img)
        .build();
    area.update_property(&[gtk::accessible::Property::Label(accessible_label)]);
    let chart = Rc::new(chart);
    let hover: Rc<Cell<Option<usize>>> = Rc::default();

    let (c, hovered) = (chart.clone(), hover.clone());
    area.set_draw_func(move |area, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        let (cx, cy) = (w / 2.0, h / 2.0);
        // Room around the ring for the labels of small parts.
        let radius = (w.min(h) / 2.0) - 22.0;
        let inner = radius * HOLE;
        let card = theme_color(area, "card_bg_color");
        let spans = angles(&c.parts);

        for (i, (p, (s, e))) in c.parts.iter().zip(&spans).enumerate() {
            let mut color = category_color(area, Some(p.slot));
            if hovered.get().is_some_and(|hi| hi != i) {
                color.set_alpha(0.45);
            }
            set_source(cr, &color);
            cr.new_path();
            cr.arc(cx, cy, radius, *s, *e);
            cr.arc_negative(cx, cy, inner, *e, *s);
            cr.close_path();
            let _ = cr.fill();
        }
        // Separators: radial segments of constant width.
        if c.parts.len() > 1 {
            set_source(cr, &card);
            cr.set_line_width(SEPARATOR);
            for (_, e) in &spans {
                cr.move_to(cx + (inner - 1.0) * e.cos(), cy + (inner - 1.0) * e.sin());
                cr.line_to(cx + (radius + 1.0) * e.cos(), cy + (radius + 1.0) * e.sin());
                let _ = cr.stroke();
            }
        }
        // Percentages: inside, in the text colour of the category; under
        // 5 %, just outside, in secondary text.
        let total: i64 = c.parts.iter().map(|p| p.amount.max(0)).sum();
        for (p, (s, e)) in c.parts.iter().zip(&spans) {
            if total == 0 || p.label.is_empty() {
                continue;
            }
            let mid = (s + e) / 2.0;
            let small = (p.amount as f64 / total as f64) < SMALL;
            let r = if small {
                radius + 13.0
            } else {
                (radius + inner) / 2.0
            };
            let style = if small {
                TextStyle::Dim
            } else {
                TextStyle::OnColor(theme_color(
                    area,
                    &format!("finaryx_i{}", p.slot.clamp(1, 8)),
                ))
            };
            draw_text(
                area,
                cr,
                &p.label,
                cx + r * mid.cos(),
                cy + r * mid.sin(),
                Align::Center,
                style,
            );
        }
        draw_text(
            area,
            cr,
            &c.center,
            cx,
            cy - 2.0,
            Align::CenterBottom,
            TextStyle::Large,
        );
        draw_text(
            area,
            cr,
            &c.center_caption,
            cx,
            cy + 2.0,
            Align::CenterTop,
            TextStyle::Dim,
        );
        let _ = set_source;
    });

    let motion = gtk::EventControllerMotion::new();
    let (c, hovered, weak) = (chart.clone(), hover.clone(), area.downgrade());
    motion.connect_motion(move |_, x, y| {
        let Some(area) = weak.upgrade() else { return };
        let hit = part_at(&c.parts, area.width() as f64, area.height() as f64, x, y);
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

    let click = gtk::GestureClick::new();
    let (c, weak) = (chart.clone(), area.downgrade());
    click.connect_released(move |_, _, x, y| {
        let Some(area) = weak.upgrade() else { return };
        if let Some(i) = part_at(&c.parts, area.width() as f64, area.height() as f64, x, y) {
            on_click(c.parts[i].slot);
        }
    });
    area.add_controller(click);

    let c = chart.clone();
    area.connect_query_tooltip(move |area, x, y, _kb, tooltip| {
        match part_at(
            &c.parts,
            area.width() as f64,
            area.height() as f64,
            x as f64,
            y as f64,
        ) {
            Some(i) => {
                tooltip.set_markup(Some(&c.parts[i].tooltip));
                true
            }
            None => false,
        }
    });
    redraw_on_theme_change(&area);
    area
}
