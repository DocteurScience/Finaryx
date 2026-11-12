//! Charts drawn with Cairo. Colours always come from the current theme
//! (`data/style.css`, `data/style-dark.css`), never from the code.

pub mod bars;
pub mod columns;
pub mod donut;
pub mod line;
pub mod share_bar;
pub mod sparkline;
pub mod tip;

pub use tip::{Tip, TipMark};

use adw::prelude::*;
use finaryx_core::stats::chart_group;
use gtk::{gdk, glib};

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

/// Colour of a category slot (1..=12) or chart group (13 or no slot =
/// "Autres", in grey).
pub fn category_color(widget: &impl IsA<gtk::Widget>, slot: Option<u8>) -> gdk::RGBA {
    theme_color(widget, &format!("finaryx_c{}", chart_group(slot)))
}

/// Colour of text drawn on a category colour.
pub fn category_ink(widget: &impl IsA<gtk::Widget>, slot: Option<u8>) -> gdk::RGBA {
    theme_color(widget, &format!("finaryx_i{}", chart_group(slot)))
}

pub fn set_source(cr: &gtk::cairo::Context, color: &gdk::RGBA) {
    cr.set_source_rgba(
        color.red() as f64,
        color.green() as f64,
        color.blue() as f64,
        color.alpha() as f64,
    );
}

/// Redraws `area` when the style switches between light and dark, or
/// in and out of high contrast.
pub fn redraw_on_theme_change(area: &gtk::DrawingArea) {
    let manager = adw::StyleManager::default();
    let handlers: Vec<glib::SignalHandlerId> = ["notify::dark", "notify::high-contrast"]
        .iter()
        .map(|signal| {
            let weak = area.downgrade();
            manager.connect_local(signal, false, move |_| {
                if let Some(area) = weak.upgrade() {
                    area.queue_draw();
                }
                None
            })
        })
        .collect();
    // Disconnect once the widget is gone.
    let handlers = std::cell::RefCell::new(handlers);
    area.connect_destroy(move |_| {
        for h in handlers.borrow_mut().drain(..) {
            adw::StyleManager::default().disconnect(h);
        }
    });
}

/// Opacity of the grid lines: discreet, stronger in high contrast.
pub fn grid_alpha() -> f32 {
    if adw::StyleManager::default().is_high_contrast() {
        0.35
    } else {
        0.08
    }
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

/// Anchor of a text drawn with [`draw_text`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    StartTop,
    StartMiddle,
    StartBottom,
    EndMiddle,
    CenterTop,
    CenterBottom,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextStyle {
    /// Secondary text, 12 px (axis labels).
    Dim,
    /// Bold, 13 px (values).
    Bold,
    /// Primary text, 12 px (notes on a chart).
    Plain,
    /// Bold, 12 px, in the given colour (labels inside parts).
    OnColor(gdk::RGBA),
    /// Large bold (centre of the ring).
    Large,
}

/// Draws a one-line text at (x, y) with the given anchor.
pub fn draw_text(
    widget: &impl IsA<gtk::Widget>,
    cr: &gtk::cairo::Context,
    text: &str,
    x: f64,
    y: f64,
    align: Align,
    style: TextStyle,
) {
    let (layout, color) = styled_layout(widget, text, style);
    let (w, h) = layout.pixel_size();
    let (w, h) = (w as f64, h as f64);
    let (dx, dy) = match align {
        Align::StartTop => (0.0, 0.0),
        Align::StartMiddle => (0.0, -h / 2.0),
        Align::StartBottom => (0.0, -h),
        Align::EndMiddle => (-w, -h / 2.0),
        Align::CenterTop => (-w / 2.0, 0.0),
        Align::CenterBottom => (-w / 2.0, -h),
        Align::Center => (-w / 2.0, -h / 2.0),
    };
    set_source(cr, &color);
    cr.move_to(x + dx, y + dy);
    pangocairo::functions::show_layout(cr, &layout);
}

/// Size in pixels of a text as [`draw_text`] would draw it.
pub fn text_size(widget: &impl IsA<gtk::Widget>, text: &str, style: TextStyle) -> (f64, f64) {
    let (w, h) = styled_layout(widget, text, style).0.pixel_size();
    (w as f64, h as f64)
}

fn styled_layout(
    widget: &impl IsA<gtk::Widget>,
    text: &str,
    style: TextStyle,
) -> (gtk::pango::Layout, gdk::RGBA) {
    use gtk::pango;
    let layout = widget.as_ref().create_pango_layout(Some(text));
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFontFeatures::new("tnum"));
    let color = match style {
        TextStyle::Dim => {
            attrs.insert(pango::AttrFloat::new_scale(0.8));
            let mut c = widget.as_ref().color();
            c.set_alpha(0.7);
            c
        }
        TextStyle::Plain => {
            attrs.insert(pango::AttrFloat::new_scale(0.8));
            widget.as_ref().color()
        }
        TextStyle::Bold => {
            attrs.insert(pango::AttrFloat::new_scale(0.87));
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
            widget.as_ref().color()
        }
        TextStyle::OnColor(c) => {
            attrs.insert(pango::AttrFloat::new_scale(0.8));
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
            c
        }
        TextStyle::Large => {
            attrs.insert(pango::AttrFloat::new_scale(1.33));
            attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
            widget.as_ref().color()
        }
    };
    layout.set_attributes(Some(&attrs));
    (layout, color)
}

/// Mark of a legend item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendMark {
    /// Solid line of the main series.
    Line,
    /// Dashed grey line of the previous period.
    Ghost,
    /// Dashed line of the main series (forecast).
    Dashed,
    /// Square of the daily bars.
    Bars,
    /// Square of the forecast range.
    Band,
}

pub fn legend_mark(kind: LegendMark) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(20)
        .content_height(10)
        .valign(gtk::Align::Center)
        .build();
    area.set_draw_func(move |area, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        match kind {
            LegendMark::Line | LegendMark::Ghost | LegendMark::Dashed => {
                let color = if kind == LegendMark::Ghost {
                    "finaryx_ghost"
                } else {
                    "finaryx_s1"
                };
                set_source(cr, &theme_color(area, color));
                if kind != LegendMark::Line {
                    cr.set_dash(&[5.0, 4.0], 0.0);
                }
                cr.set_line_width(2.0);
                cr.move_to(0.0, h / 2.0);
                cr.line_to(w, h / 2.0);
                let _ = cr.stroke();
            }
            LegendMark::Bars | LegendMark::Band => {
                let color = if kind == LegendMark::Bars {
                    "finaryx_bars"
                } else {
                    "finaryx_band"
                };
                set_source(cr, &theme_color(area, color));
                cr.rectangle(0.0, 0.0, w, h);
                let _ = cr.fill();
                if kind == LegendMark::Band {
                    // Outline: the range stays visible on any background.
                    let mut edge = theme_color(area, "finaryx_s1");
                    edge.set_alpha(0.6);
                    set_source(cr, &edge);
                    cr.set_line_width(1.0);
                    cr.rectangle(0.5, 0.5, w - 1.0, h - 1.0);
                    let _ = cr.stroke();
                }
            }
        }
    });
    redraw_on_theme_change(&area);
    area
}
