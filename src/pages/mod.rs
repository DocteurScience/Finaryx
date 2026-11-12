//! Pages of the main view stack and placeholder sub-pages.
//!
//! Step 1 of the work plan only builds the navigation; every page shows
//! an `AdwStatusPage` until its content is implemented.

use adw::prelude::*;
use finaryx_core::locale;
use gettextrs::gettext;

/// Marks a string for extraction without translating it (xgettext keyword `N_`).
#[allow(non_snake_case)]
pub const fn N_(s: &'static str) -> &'static str {
    s
}

/// A page reachable from the sidebar and the bottom view switcher.
#[derive(Debug, Clone, Copy)]
pub struct MainPage {
    /// Stack child name, also the target of `win.show-page`.
    pub name: &'static str,
    /// Sidebar label and header bar title.
    pub title: &'static str,
    /// Label in the narrow bottom view switcher.
    pub short_title: &'static str,
    pub icon: &'static str,
    /// Whether the page shows the account filter (all except Accounts).
    pub has_account_filter: bool,
    /// Maximum width of the content (mockup: 1040 px, 760 px for Comptes).
    pub max_width: i32,
}

// Titles are marked with N_() for xgettext and translated when shown.
pub const MAIN_PAGES: [MainPage; 5] = [
    MainPage {
        name: "overview",
        title: N_("Vue d'ensemble"),
        short_title: N_("Aperçu"),
        icon: "finaryx-overview-symbolic",
        has_account_filter: true,
        max_width: 1040,
    },
    MainPage {
        name: "month",
        title: N_("Mois en détail"),
        short_title: N_("Mois"),
        icon: "finaryx-month-symbolic",
        has_account_filter: true,
        max_width: 1040,
    },
    MainPage {
        name: "cumulative",
        title: N_("Cumul"),
        short_title: N_("Cumul"),
        icon: "finaryx-cumulative-symbolic",
        has_account_filter: true,
        max_width: 1040,
    },
    MainPage {
        name: "forecast",
        title: N_("Prévisions"),
        short_title: N_("Prévisions"),
        icon: "finaryx-forecast-symbolic",
        has_account_filter: true,
        max_width: 1040,
    },
    MainPage {
        name: "accounts",
        title: N_("Comptes"),
        short_title: N_("Comptes"),
        icon: "finaryx-accounts-symbolic",
        has_account_filter: false,
        max_width: 760,
    },
];

/// Sidebar entry that is not a stack page.
pub const IMPORT_TITLE: &str = N_("Importer un CSV");
pub const IMPORT_ICON: &str = "finaryx-import-symbolic";

/// Widgets of a main page that the window updates.
pub struct MainPageWidgets {
    /// Scrolled page added to the view stack.
    pub root: gtk::Widget,
    /// Body box; its side margins follow the layout (24 px, 12 px narrow).
    pub body: gtk::Box,
    pub account_filter: Option<gtk::DropDown>,
    /// "4 comptes · 3 banques", next to the account filter.
    pub summary: Option<gtk::Label>,
}

/// Builds the content of a main page: the account filter row, then the
/// page body: `content`, or a placeholder for views still to build. `filter_master` is the drop-down of
/// the first page; the others mirror its selection so that the filter
/// applies to every view.
pub fn build_main_page(
    page: &MainPage,
    accounts_model: &gtk::StringList,
    filter_master: Option<&gtk::DropDown>,
    content: Option<&gtk::Widget>,
) -> MainPageWidgets {
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let mut dropdown = None;
    let mut summary_label = None;
    if page.has_account_filter {
        let filter = gtk::DropDown::builder()
            .model(accounts_model)
            .halign(gtk::Align::Start)
            .build();
        filter.update_property(&[gtk::accessible::Property::Label(&gettext("Compte affiché"))]);
        if let Some(master) = filter_master {
            master
                .bind_property("selected", &filter, "selected")
                .bidirectional()
                .sync_create()
                .build();
        }
        let summary = gtk::Label::builder()
            .halign(gtk::Align::End)
            .hexpand(true)
            .css_classes(["caption", "dim-label"])
            .build();
        let row = gtk::Box::builder().spacing(12).build();
        row.append(&filter);
        row.append(&summary);
        body.append(&row);
        dropdown = Some(filter);
        summary_label = Some(summary);
    }

    if let Some(content) = content {
        body.append(content);
    }

    let clamp = adw::Clamp::builder()
        .maximum_size(page.max_width)
        .tightening_threshold(page.max_width)
        .child(&body)
        .build();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build();
    MainPageWidgets {
        root: scrolled.upcast(),
        body,
        account_filter: dropdown,
        summary: summary_label,
    }
}

/// Builds a sidebar row: icon and label.
pub fn sidebar_row(name: &str, title: &str, icon: &str) -> gtk::ListBoxRow {
    let content = gtk::Box::builder().spacing(12).build();
    content.append(&gtk::Image::from_icon_name(icon));
    content.append(&gtk::Label::builder().label(title).xalign(0.0).build());
    let row = gtk::ListBoxRow::builder()
        .child(&content)
        .name(name)
        .build();
    row.update_property(&[gtk::accessible::Property::Label(title)]);
    row
}

pub mod account_detail;
pub mod accounts;
pub mod import;

use finaryx_core::money::{self, Currency, Sign};

/// Section title ("Vos comptes"), as `<h2>` in the mockup.
pub fn heading(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["heading"])
        .accessible_role(gtk::AccessibleRole::Heading)
        .build()
}

/// Secondary text (dimmed, small).
pub fn caption(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["caption", "dim-label"])
        .build()
}

pub fn boxed_list() -> gtk::ListBox {
    gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build()
}

/// A section: its title, then `content`, 10 px apart.
pub fn section(title: &gtk::Widget, content: &impl IsA<gtk::Widget>) -> gtk::Box {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .build();
    section.append(title);
    section.append(content);
    section
}

/// `fr-FR` amount in the given currency ("3 995,12 €").
pub fn amount(cents: i64, currency: &str, sign: Sign) -> String {
    let currency = Currency::from_code(currency).unwrap_or(Currency::EUR);
    money::format_amount(cents, &currency, sign)
}

/// Amount rounded to whole units ("14 334 €").
pub fn amount_whole(cents: i64, currency: &str, sign: Sign) -> String {
    let currency = Currency::from_code(currency).unwrap_or(Currency::EUR);
    money::format_amount_whole(cents, &currency, sign)
}

/// A range of whole amounts, the symbol written once ("10 939 – 11 879 €").
pub fn amount_range(low: i64, high: i64, currency: &str) -> String {
    let c = Currency::from_code(currency).unwrap_or(Currency::EUR);
    let low = money::format_amount_whole(low, &c, Sign::Negative);
    let high = money::format_amount_whole(high, &c, Sign::Negative);
    let suffix = format!("{}{}", money::NNBSP, c.symbol);
    format!("{} – {high}", low.strip_suffix(&suffix).unwrap_or(&low))
}

/// "27,9 %" from tenths of a percent, in the active language.
pub fn percent_per_mille(per_mille: u32) -> String {
    locale::current().percent_tenths(per_mille as i64)
}

/// "20,9 %": [`pct1`] with the language's percent sign.
pub fn pct1_label(part: i64, total: i64) -> String {
    locale::current().percent_of(&pct1(part, total))
}

/// "21 %": [`pct0`] with the language's percent sign.
pub fn pct0_label(part: i64, total: i64) -> String {
    locale::current().percent_of(&pct0(part, total))
}

/// "1,2 Mo" / "1.2 MB": a size in the language's units and decimal mark.
pub fn file_size(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        gettext("{n} Ko").replace("{n}", &bytes.div_ceil(1024).max(1).to_string())
    } else {
        let tenths = (bytes * 10).div_ceil(1024 * 1024);
        gettext("{n} Mo").replace(
            "{n}",
            &format!(
                "{}{}{}",
                tenths / 10,
                locale::current().decimal,
                tenths % 10
            ),
        )
    }
}

/// Removes every child of a box.
pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

pub mod categories;
pub mod cumulative;
pub mod export;
pub mod forecast;
pub mod month;
pub mod operations;
pub mod overview;

/// Percentage of `part` in `total` with one decimal, "20,9" (integer
/// arithmetic, half up, decimal mark of the active language).
pub fn pct1(part: i64, total: i64) -> String {
    let decimal = locale::current().decimal;
    if total == 0 {
        return format!("0{decimal}0");
    }
    let tenths = (part as i128 * 1000 + total as i128 / 2) / total as i128;
    let sign = if tenths < 0 { "\u{2212}" } else { "" };
    let t = tenths.abs();
    format!("{sign}{}{decimal}{}", t / 10, t % 10)
}

/// Percentage without decimals, "21".
pub fn pct0(part: i64, total: i64) -> String {
    if total == 0 {
        return "0".into();
    }
    ((part as i128 * 100 + total as i128 / 2) / total as i128).to_string()
}

/// Size of the value of a key figure card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KpiSize {
    /// 32 px (total balance).
    Large,
    /// 26 px.
    Medium,
    /// 20 px, narrow layout (2 cards per line).
    Small,
}

/// A key figure card: caption, value, note.
pub fn kpi_card(title: &str, value: &str, note: &str, size: KpiSize) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .css_classes(["card", "kpi"])
        .build();
    card.append(&caption(title));
    card.append(
        &gtk::Label::builder()
            .label(value)
            .xalign(0.0)
            // Long translations ("2.616 € / Monat") go to the next line
            // between words rather than widening the page.
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::Word)
            .css_classes([match size {
                KpiSize::Large => "amount-large",
                KpiSize::Medium => "amount-medium",
                KpiSize::Small => "amount-small",
            }])
            .build(),
    );
    if !note.is_empty() {
        card.append(&caption(note));
    }
    card
}

/// Section title with a link to the detailed page on the right
/// ("Détail du mois ›").
/// Link of a section title: its label and what it opens.
pub type SectionLink<'a> = Option<(&'a str, Box<dyn Fn()>)>;

pub fn section_header(title: &str, link: SectionLink) -> gtk::Box {
    let header = gtk::Box::builder().spacing(12).build();
    let h = heading(title);
    h.set_hexpand(true);
    header.append(&h);
    if let Some((label, action)) = link {
        let content = gtk::Box::builder().spacing(4).build();
        content.append(&gtk::Label::new(Some(label)));
        content.append(&gtk::Image::from_icon_name("go-next-symbolic"));
        let button = gtk::Button::builder()
            .child(&content)
            .css_classes(["flat", "link-button"])
            .build();
        button.connect_clicked(move |_| action());
        header.append(&button);
    }
    header
}

/// A card with padding, holding `children` vertically.
pub fn chart_card(children: &[&gtk::Widget]) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .css_classes(["card", "padded"])
        .build();
    for c in children {
        card.append(*c);
    }
    card
}

/// FlowBoxes used for layout only: their children are not focusable
/// (they hold no action; the widgets inside keep their own focus).
pub fn unfocusable_children(flow: &gtk::FlowBox) {
    let mut child = flow.first_child();
    while let Some(c) = child {
        c.set_focusable(false);
        child = c.next_sibling();
    }
}

/// A legend: items side by side, wrapping when there is no room.
pub fn legend(items: &[gtk::Box]) -> gtk::FlowBox {
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(6)
        .column_spacing(20)
        .row_spacing(6)
        .build();
    for item in items {
        flow.append(item);
    }
    unfocusable_children(&flow);
    flow
}

/// Legend item: a short line (dashed or not) or a square, and a label.
pub fn legend_item(kind: crate::charts::LegendMark, label: &str) -> gtk::Box {
    let item = gtk::Box::builder().spacing(8).build();
    item.append(&crate::charts::legend_mark(kind));
    item.append(
        &gtk::Label::builder()
            .label(label)
            .css_classes(["caption"])
            .build(),
    );
    item
}

/// "octobre 2026" with a capital: "Octobre 2026".
pub fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// An empty state inside a page.
pub fn empty_state(icon: &str, title: &str, description: &str) -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name(icon)
        .title(title)
        .description(description)
        .css_classes(["compact"])
        .build()
}

/// "17 opérations"
pub fn n_ops(n: i64) -> String {
    gettextrs::ngettext(
        "{n} opération",
        "{n} opérations",
        n.clamp(0, u32::MAX as i64) as u32,
    )
    .replace("{n}", &n.to_string())
}

/// "Détail de « Autres »": the categories grouped in grey in the charts.
pub fn show_others_dialog(
    parent: &impl IsA<gtk::Widget>,
    part: &finaryx_core::stats::ChartPart,
    month: chrono::NaiveDate,
    currency: &str,
) {
    use gettextrs::gettext;
    let list = boxed_list();
    for m in &part.members {
        let name = if m.category_id == finaryx_core::models::builtin::OTHERS {
            gettext("Non classé")
        } else {
            m.name.clone()
        };
        let row = adw::ActionRow::builder()
            .title(name)
            .subtitle(n_ops(m.operations))
            .build();
        row.add_suffix(
            &gtk::Label::builder()
                .label(format!(
                    "{}   {}",
                    amount(m.amount, currency, Sign::Negative),
                    pct1_label(m.amount, part.amount)
                ))
                .css_classes(["numeric"])
                .build(),
        );
        list.append(&row);
    }
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&caption(
        &gettext("{month} · {amount} regroupés dans « Autres » dans les graphiques")
            .replace("{month}", &capitalized(&chrono_month(month)))
            .replace("{amount}", &amount(part.amount, currency, Sign::Negative)),
    ));
    content.append(&list);
    content.append(&caption(&gettext(
        "Les graphiques montrent au plus 12 couleurs, plus « Autres » en gris. Les catégories sans couleur sont comptées ici ; les listes et les tableaux les montrent séparément.",
    )));
    let buttons = gtk::Box::builder()
        .spacing(12)
        .halign(gtk::Align::End)
        .build();
    let manage = gtk::Button::builder()
        .label(gettext("Gérer les couleurs…"))
        .action_name("win.categories")
        .build();
    let close = gtk::Button::builder()
        .label(gettext("Fermer"))
        .css_classes(["suggested-action"])
        .build();
    buttons.append(&manage);
    buttons.append(&close);
    content.append(&buttons);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&content));
    let dialog = adw::Dialog::builder()
        .title(gettext("Détail de « Autres »"))
        .content_width(440)
        .child(&toolbar)
        .build();
    let d = dialog.clone();
    close.connect_clicked(move |_| {
        d.close();
    });
    let d = dialog.clone();
    manage.connect_clicked(move |_| {
        d.close();
    });
    dialog.present(Some(parent));
}

fn chrono_month(m: chrono::NaiveDate) -> String {
    finaryx_core::dates::month_year(m)
}
