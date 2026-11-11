//! Main window: sidebar navigation, view stack, sub-pages and the narrow
//! (< 600 sp) layout with a bottom view switcher.

use adw::prelude::*;
use adw::subclass::prelude::*;
use std::rc::Rc;

use chrono::NaiveDate;
use finaryx_core::accounts::{self, AccountChanges};
use finaryx_core::rusqlite::Connection;
use finaryx_core::{dates, repo};
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};

use crate::account_dialog::{self, Field};
use crate::database::{self, Source};
use crate::pages::account_detail::{self, AccountDetailPage};
use crate::pages::accounts::{self as accounts_page, AccountsPage};
use crate::pages::{self, MAIN_PAGES};

/// Sidebar index of the "Importer un CSV" row (after the main pages).
const IMPORT_ROW: i32 = MAIN_PAGES.len() as i32;
/// Index of the first row of the second sidebar group (after a separator).
const ACCOUNTS_ROW: i32 = 4;

mod imp {
    use super::*;
    use std::cell::{Cell, OnceCell, RefCell};

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/docteurscience/Finaryx/ui/window.ui")]
    pub struct FinaryxWindow {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub narrow_breakpoint: TemplateChild<adw::Breakpoint>,
        #[template_child]
        pub sidebar_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub content_nav: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub main_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub window_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub import_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub import_icon_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub add_account_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub add_account_icon_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub content_menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub stack: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub root_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub demo_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub error_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub last_import_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub last_import_label: TemplateChild<gtk::Label>,

        pub settings: OnceCell<gio::Settings>,
        pub accounts_model: OnceCell<gtk::StringList>,
        pub narrow: Cell<bool>,
        pub page_bodies: RefCell<Vec<gtk::Box>>,
        pub summary_labels: RefCell<Vec<gtk::Label>>,
        /// Open database; `None` until `open_database()` succeeds.
        pub db: RefCell<Option<Connection>>,
        pub accounts_page: OnceCell<AccountsPage>,
        /// Account detail currently pushed, if any.
        pub detail: RefCell<Option<Rc<AccountDetailPage>>>,
        /// "4 comptes · 3 banques"
        pub summary: RefCell<String>,
        /// Guards against feedback loops while syncing the sidebar selection.
        pub syncing: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FinaryxWindow {
        const NAME: &'static str = "FinaryxWindow";
        type Type = super::FinaryxWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for FinaryxWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.setup_settings();
            obj.setup_pages();
            obj.setup_sidebar();
            obj.setup_actions();
            obj.setup_signals();
            obj.sync_with_stack();
        }
    }

    impl WidgetImpl for FinaryxWindow {}

    impl WindowImpl for FinaryxWindow {
        fn close_request(&self) -> glib::Propagation {
            self.obj().save_window_size();
            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for FinaryxWindow {}
    impl AdwApplicationWindowImpl for FinaryxWindow {}
}

glib::wrapper! {
    pub struct FinaryxWindow(ObjectSubclass<imp::FinaryxWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl FinaryxWindow {
    pub fn new<P: IsA<gtk::Application>>(app: &P, source: &Source) -> Self {
        let window: Self = glib::Object::builder().property("application", app).build();
        window.open_database(source);
        window
    }

    /// Opens the database (or the demonstration data) and shows its
    /// content, or an error page if it cannot be opened.
    fn open_database(&self, source: &Source) {
        let imp = self.imp();
        imp.demo_banner.set_revealed(source.is_demo());
        match database::open(source) {
            Ok(conn) => {
                imp.db.replace(Some(conn));
                self.reload();
            }
            Err(message) => {
                glib::g_warning!("finaryx", "Cannot open the database: {message}");
                imp.error_page.set_description(Some(&message));
                imp.root_stack.set_visible_child_name("error");
            }
        }
    }

    /// Re-reads what the window shows from the database.
    pub fn reload(&self) {
        if let Err(e) = self.try_reload() {
            glib::g_warning!("finaryx", "Cannot read the database: {e}");
            self.toast(&gettext("Impossible de lire la base de données."));
        }
    }

    fn try_reload(&self) -> finaryx_core::db::Result<()> {
        let imp = self.imp();
        let db = imp.db.borrow();
        let Some(conn) = db.as_ref() else {
            return Ok(());
        };

        let accounts = repo::list_accounts(conn, false)?;
        imp.root_stack
            .set_visible_child_name(if accounts.is_empty() {
                "welcome"
            } else {
                "main"
            });

        // Account filter: "Tous les comptes", then each account.
        let model = imp.accounts_model.get().unwrap();
        let names: Vec<String> = accounts.iter().map(|a| a.name.clone()).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        model.splice(1, model.n_items() - 1, &names);

        // "4 comptes · 3 banques"
        let (n_accounts, n_banks) = repo::account_and_bank_counts(conn)?;
        let summary = format!(
            "{} · {}",
            ngettext("{n} compte", "{n} comptes", n_accounts as u32)
                .replace("{n}", &n_accounts.to_string()),
            ngettext("{n} banque", "{n} banques", n_banks as u32)
                .replace("{n}", &n_banks.to_string()),
        );
        for label in imp.summary_labels.borrow().iter() {
            label.set_label(&summary);
        }
        imp.summary.replace(summary);
        self.sync_with_stack();

        imp.accounts_page.get().unwrap().refresh(conn, today())?;
        let detail = imp.detail.borrow().clone();
        if let Some(detail) = detail {
            if !detail.refresh(conn)? {
                imp.detail.replace(None);
                imp.content_nav.pop_to_tag("main");
            }
        }

        // Sidebar footer: "2 nov. 2026 · Compte courant"
        match repo::last_batch(conn)? {
            Some(batch) => {
                let date = dates::parse_iso_date(&batch.imported_at)
                    .map(dates::day_month_year)
                    .unwrap_or(batch.imported_at.clone());
                let account = repo::account_name(conn, batch.account_id)?;
                imp.last_import_label
                    .set_label(&format!("{date} · {account}"));
                imp.last_import_box.set_visible(true);
            }
            None => imp.last_import_box.set_visible(false),
        }
        Ok(())
    }

    fn settings(&self) -> &gio::Settings {
        self.imp()
            .settings
            .get()
            .expect("settings are set in constructed()")
    }

    fn setup_settings(&self) {
        let settings = crate::settings::settings();
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
        if settings.boolean("window-maximized") {
            self.maximize();
        }
        self.imp().settings.set(settings).unwrap();
    }

    fn save_window_size(&self) {
        let settings = self.settings();
        let (width, height) = self.default_size();
        // Errors only mean the key is not writable (e.g. locked down): ignore.
        let _ = settings.set_int("window-width", width);
        let _ = settings.set_int("window-height", height);
        let _ = settings.set_boolean("window-maximized", self.is_maximized());
    }

    /// Fills the view stack with the main pages.
    fn setup_pages(&self) {
        let imp = self.imp();
        let accounts_model = gtk::StringList::new(&[&gettext("Tous les comptes")]);
        let mut master: Option<gtk::DropDown> = None;

        let weak = self.downgrade();
        let open_account = move |id| {
            if let Some(w) = weak.upgrade() {
                w.open_account(id);
            }
        };
        let weak = self.downgrade();
        let undo_import = move |id| {
            if let Some(w) = weak.upgrade() {
                w.confirm_undo_import(id);
            }
        };
        let accounts = AccountsPage::new(accounts_page::Callbacks {
            open_account: Rc::new(open_account),
            undo_import: Rc::new(undo_import),
        });

        for page in MAIN_PAGES.iter() {
            let content: Option<gtk::Widget> =
                (page.name == "accounts").then(|| accounts.root.clone().upcast());
            let widgets =
                pages::build_main_page(page, &accounts_model, master.as_ref(), content.as_ref());
            imp.page_bodies.borrow_mut().push(widgets.body);
            imp.summary_labels.borrow_mut().extend(widgets.summary);
            if master.is_none() {
                master = widgets.account_filter;
            }
            let stack_page =
                imp.stack
                    .add_titled(&widgets.root, Some(page.name), &gettext(page.short_title));
            stack_page.set_icon_name(Some(page.icon));
        }
        imp.accounts_model.set(accounts_model).unwrap();
        let _ = imp.accounts_page.set(accounts);
    }

    fn setup_sidebar(&self) {
        let list = &self.imp().sidebar_list;
        for page in MAIN_PAGES.iter() {
            list.append(&pages::sidebar_row(
                page.name,
                &gettext(page.title),
                page.icon,
            ));
        }
        list.append(&pages::sidebar_row(
            "import",
            &gettext(pages::IMPORT_TITLE),
            pages::IMPORT_ICON,
        ));

        // Separator between the views and the account-related entries.
        list.set_header_func(|row, _| {
            if row.index() == ACCOUNTS_ROW {
                let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
                separator.set_margin_top(6);
                separator.set_margin_bottom(6);
                separator.set_margin_start(6);
                separator.set_margin_end(6);
                row.set_header(Some(&separator));
            } else {
                row.set_header(None::<&gtk::Widget>);
            }
        });

        list.connect_row_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, row| {
                if window.imp().syncing.get() {
                    return;
                }
                let index = row.index();
                if index == IMPORT_ROW {
                    window.show_import();
                } else if let Some(page) = MAIN_PAGES.get(index as usize) {
                    window.show_page(page.name);
                }
            }
        ));
    }

    fn setup_actions(&self) {
        let show_page = gio::ActionEntry::builder("show-page")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|window: &Self, _, param| {
                if let Some(name) = param.and_then(|p| p.get::<String>()) {
                    window.show_page(&name);
                }
            })
            .build();
        let import = gio::ActionEntry::builder("import")
            .activate(|window: &Self, _, _| window.show_import())
            .build();
        let add_account = gio::ActionEntry::builder("add-account")
            .activate(|window: &Self, _, _| window.show_add_account())
            .build();
        let categories = gio::ActionEntry::builder("categories")
            .activate(|window: &Self, _, _| {
                window.push_sub_page(
                    "categories",
                    &gettext("Catégories et règles"),
                    "view-list-bullet-symbolic",
                    6,
                );
            })
            .build();
        let export = gio::ActionEntry::builder("export")
            .activate(|window: &Self, _, _| {
                window.push_sub_page(
                    "export",
                    &gettext("Exporter les données"),
                    "document-send-symbolic",
                    4,
                );
            })
            .build();
        self.add_action_entries([show_page, import, add_account, categories, export]);
    }

    fn setup_signals(&self) {
        let imp = self.imp();
        imp.stack.connect_visible_child_name_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.sync_with_stack()
        ));
        imp.content_nav.connect_visible_page_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.sync_with_stack()
        ));
        imp.narrow_breakpoint.connect_apply(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.imp().narrow.set(true);
                window.imp().accounts_page.get().unwrap().set_narrow(true);
                window.reload();
            }
        ));
        imp.narrow_breakpoint.connect_unapply(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.imp().narrow.set(false);
                window.imp().accounts_page.get().unwrap().set_narrow(false);
                window.reload();
            }
        ));
    }

    /// Shows a main page: back to the root of the content, page selected.
    pub fn show_page(&self, name: &str) {
        let imp = self.imp();
        imp.content_nav.pop_to_tag("main");
        imp.stack.set_visible_child_name(name);
        imp.split_view.set_show_content(true);
    }

    pub fn show_import(&self) {
        self.imp().root_stack.set_visible_child_name("main");
        self.push_sub_page(
            "import",
            &gettext(pages::IMPORT_TITLE),
            pages::IMPORT_ICON,
            4,
        );
    }

    /// Pushes a sub-page with a back button, replacing any other sub-page.
    fn push_sub_page(&self, tag: &str, title: &str, icon: &str, step: u32) {
        let imp = self.imp();
        imp.content_nav.pop_to_tag("main");
        imp.content_nav
            .push(&pages::build_sub_page(tag, title, icon, step));
        imp.split_view.set_show_content(true);
    }

    pub fn toast(&self, title: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(title));
    }

    /// Updates the header bar and the sidebar selection after a change of
    /// page, sub-page or width.
    fn sync_with_stack(&self) {
        let imp = self.imp();
        let name = imp.stack.visible_child_name().unwrap_or_default();
        let index = MAIN_PAGES
            .iter()
            .position(|p| p.name == name.as_str())
            .unwrap_or(0);
        let page = &MAIN_PAGES[index];

        let title = gettext(page.title);
        imp.window_title.set_title(&title);
        imp.main_page.set_title(&title);

        // Start of the header bar: "Ajouter" on Accounts, "Importer"
        // elsewhere; icon only in the narrow layout (mockup Etroit*).
        let narrow = imp.narrow.get();
        let on_accounts = page.name == "accounts";
        // Subtitle: "4 comptes · 3 banques" on the accounts page (step 5
        // adds the month of the other views).
        let subtitle = if on_accounts {
            imp.summary.borrow().clone()
        } else {
            String::new()
        };
        imp.window_title.set_subtitle(&subtitle);
        imp.import_button.set_visible(!on_accounts && !narrow);
        imp.import_icon_button.set_visible(!on_accounts && narrow);
        imp.add_account_button.set_visible(on_accounts && !narrow);
        imp.add_account_icon_button
            .set_visible(on_accounts && narrow);

        // Page padding: 24 px, 12 px on the sides in the narrow layout.
        let side = if narrow { 12 } else { 24 };
        for body in imp.page_bodies.borrow().iter() {
            body.set_margin_start(side);
            body.set_margin_end(side);
        }

        // Sidebar: the current main page, the import row, or nothing when
        // another sub-page (categories, export…) is shown.
        let row_index = match imp.content_nav.visible_page().and_then(|p| p.tag()) {
            Some(tag) if tag == "main" => Some(index as i32),
            Some(tag) if tag == "import" => Some(IMPORT_ROW),
            Some(tag) if tag == account_detail::TAG => Some(ACCOUNTS_ROW),
            _ => None,
        };
        imp.syncing.set(true);
        match row_index.and_then(|i| imp.sidebar_list.row_at_index(i)) {
            Some(row) => imp.sidebar_list.select_row(Some(&row)),
            None => imp.sidebar_list.unselect_all(),
        }
        imp.syncing.set(false);
    }

    // ----- Database changes ------------------------------------------------

    /// Runs a change on the database, then reloads the views. Errors are
    /// logged and returned as a message for the user.
    fn change<R>(
        &self,
        f: impl FnOnce(&mut Connection) -> finaryx_core::db::Result<R>,
    ) -> Result<R, String> {
        let result = {
            let mut db = self.imp().db.borrow_mut();
            let Some(conn) = db.as_mut() else {
                return Err(gettext("La base de données n’est pas ouverte."));
            };
            f(conn)
        };
        match result {
            Ok(value) => {
                self.reload();
                Ok(value)
            }
            Err(e) => {
                glib::g_warning!("finaryx", "Database change failed: {e}");
                Err(
                    gettext("La modification n’a pas pu être enregistrée : {error}")
                        .replace("{error}", &e.to_string()),
                )
            }
        }
    }

    fn read<R>(&self, f: impl FnOnce(&Connection) -> finaryx_core::db::Result<R>) -> Option<R> {
        let db = self.imp().db.borrow();
        match f(db.as_ref()?) {
            Ok(value) => Some(value),
            Err(e) => {
                glib::g_warning!("finaryx", "Cannot read the database: {e}");
                self.toast(&gettext("Impossible de lire la base de données."));
                None
            }
        }
    }

    // ----- Accounts ----------------------------------------------------------

    pub fn show_add_account(&self) {
        let weak = self.downgrade();
        account_dialog::present(self, None, Field::Name, move |changes| {
            let window = weak.upgrade()?;
            match window.change(|conn| accounts::create_account(conn, &changes)) {
                Ok(_) => {
                    window.show_page("accounts");
                    window.toast(&gettext("Compte ajouté"));
                    None
                }
                Err(message) => Some(message),
            }
        });
    }

    fn account_changes(&self, id: i64) -> Option<AccountChanges> {
        let overview = self
            .read(|conn| accounts::overviews(conn, true))?
            .into_iter()
            .find(|o| o.account.id == id)?;
        let a = overview.account;
        Some(AccountChanges {
            name: a.name,
            bank_name: overview.bank_name.unwrap_or_default(),
            number: a.number,
            kind: a.kind,
            currency: a.currency,
            opening_balance_cents: a.opening_balance_cents,
        })
    }

    fn edit_account(&self, id: i64, field: Field) {
        let Some(initial) = self.account_changes(id) else {
            return;
        };
        let weak = self.downgrade();
        account_dialog::present(self, Some(&initial), field, move |changes| {
            let window = weak.upgrade()?;
            window
                .change(|conn| accounts::update_account(conn, id, &changes))
                .err()
        });
    }

    fn set_account_archived(&self, id: i64, archived: bool) {
        if let Err(message) = self.change(|conn| accounts::set_archived(conn, id, archived)) {
            self.toast(&message);
            return;
        }
        let toast = adw::Toast::builder()
            .title(if archived {
                gettext("Compte archivé")
            } else {
                gettext("Compte désarchivé")
            })
            .button_label(gettext("Annuler"))
            .build();
        let weak = self.downgrade();
        toast.connect_button_clicked(move |_| {
            if let Some(window) = weak.upgrade() {
                let _ = window.change(|conn| accounts::set_archived(conn, id, !archived));
            }
        });
        self.imp().toast_overlay.add_toast(toast);
    }

    fn confirm_delete_account(&self, id: i64) {
        let Some(overview) = self
            .read(|conn| accounts::overviews(conn, true))
            .and_then(|list| list.into_iter().find(|o| o.account.id == id))
        else {
            return;
        };
        let n = overview.operation_count.clamp(0, u32::MAX as i64) as u32;
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Supprimer « {name} » ?").replace("{name}", &overview.account.name)),
            Some(
                &ngettext(
                    "Le compte, son historique d’import et {n} opération seront effacés définitivement.",
                    "Le compte, son historique d’import et ses {n} opérations seront effacés définitivement.",
                    n,
                )
                .replace("{n}", &n.to_string()),
            ),
        );
        dialog.add_responses(&[
            ("cancel", &gettext("Annuler")),
            ("delete", &gettext("Supprimer")),
        ]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let weak = self.downgrade();
        dialog.connect_response(Some("delete"), move |_, _| {
            let Some(window) = weak.upgrade() else { return };
            match window.change(|conn| accounts::delete_account(conn, id)) {
                Ok(count) => {
                    window.imp().content_nav.pop_to_tag("main");
                    let count = count.clamp(0, u32::MAX as i64) as u32;
                    window.toast(
                        &ngettext(
                            "Compte supprimé, {n} opération effacée",
                            "Compte supprimé, {n} opérations effacées",
                            count,
                        )
                        .replace("{n}", &count.to_string()),
                    );
                }
                Err(message) => window.toast(&message),
            }
        });
        dialog.present(Some(self));
    }

    fn confirm_undo_import(&self, batch_id: i64) {
        let Some(entry) = self
            .read(accounts::import_history)
            .and_then(|list| list.into_iter().find(|e| e.batch.id == batch_id))
        else {
            return;
        };
        let n = entry.remaining.clamp(0, u32::MAX as i64) as u32;
        let file = entry
            .batch
            .filename
            .clone()
            .unwrap_or_else(|| gettext("sans nom"));
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Annuler cet import ?")),
            Some(
                &ngettext(
                    "{n} opération ajoutée par « {file} » sur {account} sera supprimée. Les autres imports ne sont pas modifiés.",
                    "Les {n} opérations ajoutées par « {file} » sur {account} seront supprimées. Les autres imports ne sont pas modifiés.",
                    n,
                )
                .replace("{n}", &n.to_string())
                .replace("{file}", &file)
                .replace("{account}", &entry.account_name),
            ),
        );
        dialog.add_responses(&[
            ("cancel", &gettext("Garder")),
            ("undo", &gettext("Supprimer les opérations")),
        ]);
        dialog.set_response_appearance("undo", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let weak = self.downgrade();
        dialog.connect_response(Some("undo"), move |_, _| {
            let Some(window) = weak.upgrade() else { return };
            match window.change(|conn| accounts::undo_import(conn, batch_id)) {
                Ok(_) => window.toast(&gettext("Import annulé")),
                Err(message) => window.toast(&message),
            }
        });
        dialog.present(Some(self));
    }

    pub fn open_account(&self, id: i64) {
        let weak = self.downgrade();
        let w = move || weak.upgrade();
        let (w1, w2, w3, w4) = (w.clone(), w.clone(), w.clone(), w);
        let callbacks = account_detail::Callbacks {
            edit: Rc::new(move |id, field| {
                if let Some(window) = w1() {
                    window.edit_account(id, field);
                }
            }),
            set_archived: Rc::new(move |id, archived| {
                if let Some(window) = w2() {
                    window.set_account_archived(id, archived);
                }
            }),
            delete: Rc::new(move |id| {
                if let Some(window) = w3() {
                    window.confirm_delete_account(id);
                }
            }),
            see_all: Rc::new(move |_| {
                if let Some(window) = w4() {
                    window.push_sub_page(
                        "operations",
                        &gettext("Opérations"),
                        "view-list-bullet-symbolic",
                        5,
                    );
                }
            }),
        };
        let detail = Rc::new(AccountDetailPage::new(id, callbacks));
        if self.read(|conn| detail.refresh(conn)) != Some(true) {
            return;
        }
        let imp = self.imp();
        imp.content_nav.pop_to_tag("main");
        imp.content_nav.push(&detail.page);
        imp.detail.replace(Some(detail));
        imp.split_view.set_show_content(true);
    }
}

/// Today's date in the local time zone.
fn today() -> NaiveDate {
    glib::DateTime::now_local()
        .ok()
        .and_then(|d| NaiveDate::from_ymd_opt(d.year(), d.month() as u32, d.day_of_month() as u32))
        .unwrap_or_default()
}
