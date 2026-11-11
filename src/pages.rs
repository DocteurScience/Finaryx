//! Pages of the main view stack and placeholder sub-pages.
//!
//! Step 1 of the work plan only builds the navigation; every page shows
//! an `AdwStatusPage` until its content is implemented.

use adw::prelude::*;
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
    /// Step of the work plan that fills the page in.
    pub planned_step: u32,
}

// Titles are marked with N_() for xgettext and translated when shown.
pub const MAIN_PAGES: [MainPage; 5] = [
    MainPage {
        name: "overview",
        title: N_("Vue d'ensemble"),
        short_title: N_("Aperçu"),
        icon: "finaryx-overview-symbolic",
        has_account_filter: true,
        planned_step: 5,
    },
    MainPage {
        name: "month",
        title: N_("Mois en détail"),
        short_title: N_("Mois"),
        icon: "finaryx-month-symbolic",
        has_account_filter: true,
        planned_step: 5,
    },
    MainPage {
        name: "cumulative",
        title: N_("Cumul"),
        short_title: N_("Cumul"),
        icon: "finaryx-cumulative-symbolic",
        has_account_filter: true,
        planned_step: 5,
    },
    MainPage {
        name: "forecast",
        title: N_("Prévisions"),
        short_title: N_("Prévisions"),
        icon: "finaryx-forecast-symbolic",
        has_account_filter: true,
        planned_step: 7,
    },
    MainPage {
        name: "accounts",
        title: N_("Comptes"),
        short_title: N_("Comptes"),
        icon: "finaryx-accounts-symbolic",
        has_account_filter: false,
        planned_step: 3,
    },
];

/// Sidebar entry that is not a stack page.
pub const IMPORT_TITLE: &str = N_("Importer un CSV");
pub const IMPORT_ICON: &str = "finaryx-import-symbolic";

fn coming_soon(icon: &str, title: &str, step: u32) -> adw::StatusPage {
    adw::StatusPage::builder()
        .icon_name(icon)
        .title(title)
        .description(
            gettext("Cette vue sera construite à l’étape {step} du plan de travail.")
                .replace("{step}", &step.to_string()),
        )
        .vexpand(true)
        .build()
}

/// Builds the content of a main page: the account filter row, then the
/// page body (a placeholder for now). Returns the scrolled page, its body
/// box (whose side margins follow the layout: 24 px wide, 12 px narrow)
/// and its account drop-down. `filter_master` is the drop-down of
/// the first page; the others mirror its selection so that the filter
/// applies to every view.
pub fn build_main_page(
    page: &MainPage,
    accounts_model: &gtk::StringList,
    filter_master: Option<&gtk::DropDown>,
) -> (gtk::Widget, gtk::Box, Option<gtk::DropDown>) {
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(24)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let mut dropdown = None;
    if page.has_account_filter {
        let filter = gtk::DropDown::builder()
            .model(accounts_model)
            .halign(gtk::Align::Start)
            .tooltip_text(gettext("Compte affiché"))
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
    }

    body.append(&coming_soon(
        page.icon,
        &gettext(page.title),
        page.planned_step,
    ));

    let clamp = adw::Clamp::builder()
        .maximum_size(1040)
        .tightening_threshold(1040)
        .child(&body)
        .build();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build();
    (scrolled.upcast(), body, dropdown)
}

/// A page pushed on top of the main view, with its own header bar and
/// back button (Import, Categories and rules, Export…).
pub fn build_sub_page(tag: &str, title: &str, icon: &str, step: u32) -> adw::NavigationPage {
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&coming_soon(icon, title, step)));
    adw::NavigationPage::builder()
        .tag(tag)
        .title(title)
        .child(&toolbar)
        .build()
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
