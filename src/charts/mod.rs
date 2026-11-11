//! Charts drawn with Cairo. Colours always come from the current theme
//! (`data/style.css`, `data/style-dark.css`), never from the code.

pub mod line;
pub mod share_bar;
pub mod sparkline;

use adw::prelude::*;
use gtk::gdk;

/// Looks a named colour up in the theme (`@define-color`), e.g.
/// `"finaryx_a1"`.
///
/// GTK 4.14 has no replacement for `StyleContext::lookup_color` yet, hence
/// the deprecated call, isolated here.
#[allow(deprecated)]
pub fn theme_color(widget: &impl IsA<gtk::Widget>, name: &str) -> gdk::RGBA {
    widget
        .as_ref()
        .style_context()
        .lookup_color(name)
        .unwrap_or_else(|| widget.as_ref().color())
}

/// Colour of an account (`color_slot` 1..=4).
pub fn account_color(widget: &impl IsA<gtk::Widget>, slot: u8) -> gdk::RGBA {
    theme_color(widget, &format!("finaryx_a{}", slot.clamp(1, 4)))
}

/// Colour of a category slot (1..=8; no slot = "Autres", 8).
pub fn category_color(widget: &impl IsA<gtk::Widget>, slot: Option<u8>) -> gdk::RGBA {
    theme_color(
        widget,
        &format!("finaryx_c{}", slot.unwrap_or(8).clamp(1, 8)),
    )
}

pub fn set_source(cr: &gtk::cairo::Context, color: &gdk::RGBA) {
    cr.set_source_rgba(
        color.red() as f64,
        color.green() as f64,
        color.blue() as f64,
        color.alpha() as f64,
    );
}

/// Redraws `area` when the style switches between light and dark.
pub fn redraw_on_theme_change(area: &gtk::DrawingArea) {
    let weak = area.downgrade();
    let handler = adw::StyleManager::default().connect_dark_notify(move |_| {
        if let Some(area) = weak.upgrade() {
            area.queue_draw();
        }
    });
    // Disconnect once the widget is gone.
    let handler = std::cell::RefCell::new(Some(handler));
    area.connect_destroy(move |_| {
        if let Some(h) = handler.borrow_mut().take() {
            adw::StyleManager::default().disconnect(h);
        }
    });
}

/// A small coloured square used in legends and lists (12 px by default).
pub fn color_swatch(
    size: i32,
    color: impl Fn(&gtk::DrawingArea) -> gdk::RGBA + 'static,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(size)
        .content_height(size)
        .valign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |area, cr, w, h| {
        set_source(cr, &color(area));
        rounded_rect(cr, 0.0, 0.0, w as f64, h as f64, 3.0);
        let _ = cr.fill();
    });
    redraw_on_theme_change(&area);
    area
}

pub fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.close_path();
}

