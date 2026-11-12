//! "Importer un CSV" (design/Import.dc.html): file, format, column
//! mapping, bank categories, preview and summary. Nothing is written
//! before "Importer N opérations"; "Annuler" leaves the database as is.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use finaryx_core::accounts::{self as core_accounts, AccountOverview};
use finaryx_core::importers::format::{self, DateFormat, Encoding};
use finaryx_core::importers::mapping::{AmountMode, BankCategoryChoice, Field};
use finaryx_core::importers::plan::{self, Analysis, CommitOptions, Plan, RowState};
use finaryx_core::importers::profiles;
use finaryx_core::models::{Category, Id};
use finaryx_core::money::Sign;
use finaryx_core::repo;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};

use super::{amount, boxed_list, caption, clear, heading, section};
use crate::charts;
use crate::window::FinaryxWindow;

pub const TAG: &str = "import";

/// Applies the selected index of a format row to the mapping.
type MappingSetter = Box<dyn Fn(&mut finaryx_core::importers::mapping::Mapping, u32)>;

/// Operations of the preview.
const PREVIEW_ROWS: usize = 5;
/// Bank categories shown before "Voir les N correspondances".
const CATEGORY_ROWS: usize = 6;

struct State {
    filename: String,
    bytes: Vec<u8>,
    analysis: Analysis,
    /// Bank category keys already known by the recognised profile.
    known_keys: HashSet<String>,
    account_id: Option<Id>,
    plan: Option<Plan>,
    remember: bool,
    format_name: String,
    adjust_opening: bool,
    show_all_categories: bool,
}

pub struct ImportPage {
    pub page: adw::NavigationPage,
    window: glib::WeakRef<FinaryxWindow>,
    title: adw::WindowTitle,
    stack: gtk::Stack,
    /// Sections rebuilt when the table changes.
    content: gtk::Box,
    /// Validation messages and bank categories (rebuilt when the mapping
    /// changes).
    mapping_box: gtk::Box,
    /// Preview, summary and buttons (rebuilt after each change).
    result_box: gtk::Box,
    state: RefCell<Option<State>>,
    /// Set while widgets are updated by the code, to ignore their signals.
    updating: Cell<bool>,
    /// Narrow layout (< 600 sp).
    narrow: Cell<bool>,
}

impl std::fmt::Debug for ImportPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportPage").finish_non_exhaustive()
    }
}

fn n_text(singular: &str, plural: &str, n: usize) -> String {
    ngettext(singular, plural, n.min(u32::MAX as usize) as u32).replace("{n}", &n.to_string())
}

/// "38 Ko", "1,2 Mo"
fn file_size(bytes: usize) -> String {
    if bytes < 1024 * 1024 {
        format!("{} Ko", bytes.div_ceil(1024).max(1))
    } else {
        let tenths = (bytes * 10).div_ceil(1024 * 1024);
        format!("{},{} Mo", tenths / 10, tenths % 10)
    }
}

fn badge(text: &str, late: bool) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .valign(gtk::Align::Center)
        .css_classes(["status-badge"])
        .build();
    if late {
        label.add_css_class("late");
    }
    label
}

fn string_list(items: &[String]) -> gtk::StringList {
    let refs: Vec<&str> = items.iter().map(String::as_str).collect();
    gtk::StringList::new(&refs)
}

impl ImportPage {
    pub fn new(window: &FinaryxWindow) -> Rc<Self> {
        let title = adw::WindowTitle::new(&gettext("Importer un CSV"), "");
        let header = adw::HeaderBar::builder().title_widget(&title).build();

        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(28)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();
        let mapping_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(28)
            .build();
        let result_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(28)
            .build();
        let clamp = adw::Clamp::builder()
            .maximum_size(760)
            .tightening_threshold(760)
            .child(&content)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&clamp)
            .build();

        let choose = gtk::Button::builder()
            .label(gettext("Choisir un fichier…"))
            .halign(gtk::Align::Center)
            .css_classes(["suggested-action", "pill"])
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("finaryx-import-symbolic")
            .title(gettext("Importer un CSV"))
            .description(gettext(
                "Choisissez l’export CSV d’un compte, ou déposez-le ici. Le fichier n’est jamais modifié et rien n’est \
                 enregistré avant votre confirmation.",
            ))
            .child(&choose)
            .vexpand(true)
            .build();

        let stack = gtk::Stack::new();
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scrolled, Some("content"));

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&stack));
        let page = adw::NavigationPage::builder()
            .tag(TAG)
            .title(gettext("Importer un CSV"))
            .child(&toolbar)
            .build();

        let this = Rc::new(ImportPage {
            page,
            window: window.downgrade(),
            title,
            stack,
            content,
            mapping_box,
            result_box,
            state: RefCell::new(None),
            updating: Cell::new(false),
            narrow: Cell::new(false),
        });

        let weak = Rc::downgrade(&this);
        choose.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.choose_file();
            }
        });

        // Drag and drop of a file anywhere on the page.
        let drop = gtk::DropTarget::new(gio::File::static_type(), gtk::gdk::DragAction::COPY);
        let weak = Rc::downgrade(&this);
        drop.connect_drop(move |_, value, _, _| {
            let (Some(this), Ok(file)) = (weak.upgrade(), value.get::<gio::File>()) else {
                return false;
            };
            this.load_file(file);
            true
        });
        toolbar.add_controller(drop);
        this
    }

    /// Switches between the wide and narrow layouts.
    pub fn set_narrow(self: &Rc<Self>, narrow: bool) {
        if self.narrow.replace(narrow) != narrow {
            self.rebuild_all_later();
        }
    }

    fn window(&self) -> Option<FinaryxWindow> {
        self.window.upgrade()
    }

    pub fn choose_file(self: &Rc<Self>) {
        let Some(window) = self.window() else { return };
        let csv = gtk::FileFilter::new();
        csv.set_name(Some(&gettext("Fichiers CSV")));
        csv.add_suffix("csv");
        csv.add_mime_type("text/csv");
        let all = gtk::FileFilter::new();
        all.set_name(Some(&gettext("Tous les fichiers")));
        all.add_pattern("*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&csv);
        filters.append(&all);
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Choisir un export CSV"))
            .filters(&filters)
            .default_filter(&csv)
            .modal(true)
            .build();
        let weak = Rc::downgrade(self);
        dialog.open(Some(&window), gio::Cancellable::NONE, move |result| {
            if let (Some(this), Ok(file)) = (weak.upgrade(), result) {
                this.load_file(file);
            }
        });
    }

    pub fn load_file(self: &Rc<Self>, file: gio::File) {
        let weak = Rc::downgrade(self);
        let source = file.clone();
        source.load_contents_async(gio::Cancellable::NONE, move |result| {
            let Some(this) = weak.upgrade() else { return };
            match result {
                Ok((bytes, _)) => {
                    let name = file
                        .basename()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_else(|| gettext("fichier"));
                    this.open_bytes(name, bytes.to_vec());
                }
                Err(e) => {
                    if let Some(w) = this.window() {
                        w.toast(
                            &gettext("Impossible de lire le fichier : {error}")
                                .replace("{error}", e.message()),
                        );
                    }
                }
            }
        });
    }

    /// Analyses a file and shows the whole page.
    pub fn open_bytes(self: &Rc<Self>, filename: String, bytes: Vec<u8>) {
        let Some(window) = self.window() else { return };
        let Some((analysis, accounts)) = window.read(|conn| {
            Ok((
                plan::analyze(conn, &bytes)?,
                core_accounts::overviews(conn, false)?,
            ))
        }) else {
            return;
        };
        let account_id = default_account(&accounts, &analysis, &filename);
        let known_keys = analysis
            .profile
            .as_ref()
            .map(|p| p.mapping.bank_categories.keys().cloned().collect())
            .unwrap_or_default();
        let format_name = match &analysis.profile {
            Some(p) => p.name.clone(),
            None => profiles::name_from_filename(&filename),
        };
        let remember = analysis.profile.as_ref().map_or(true, |p| p.builtin);
        self.title.set_subtitle(&filename);
        self.state.replace(Some(State {
            filename,
            bytes,
            analysis,
            known_keys,
            account_id,
            plan: None,
            remember,
            format_name,
            adjust_opening: false,
            show_all_categories: false,
        }));
        self.stack.set_visible_child_name("content");
        self.rebuild_all();
    }

    // ----- Rebuilding --------------------------------------------------------

    fn rebuild_all(self: &Rc<Self>) {
        clear(&self.content);
        if self.state.borrow().is_none() {
            return;
        }
        self.content.append(&self.file_section());
        self.content.append(&self.format_section());
        self.content.append(&self.columns_section());
        self.content.append(&self.mapping_box);
        self.content.append(&self.result_box);
        self.rebuild_mapping();
    }

    /// After a change of mapping: validation, bank categories, then the
    /// preview.
    fn rebuild_mapping(self: &Rc<Self>) {
        clear(&self.mapping_box);
        let messages = {
            let state = self.state.borrow();
            let Some(s) = state.as_ref() else { return };
            plan::validate(&s.analysis.table, &s.analysis.mapping)
        };
        if !messages.is_empty() {
            self.mapping_box.append(&problems_box(
                &gettext("Le fichier ne se lit pas encore correctement"),
                &messages,
            ));
        }
        if let Some(section) = self.categories_section() {
            self.mapping_box.append(&section);
        }
        self.replan(messages.is_empty());
    }

    /// Computes the plan (if the mapping is valid) and rebuilds the
    /// preview and summary.
    fn replan(self: &Rc<Self>, valid: bool) {
        let Some(window) = self.window() else { return };
        {
            let mut state = self.state.borrow_mut();
            let Some(s) = state.as_mut() else { return };
            s.plan = None;
            if valid {
                if let Some(account) = s.account_id {
                    // Proposals for unknown bank categories become part of
                    // the mapping (and of the saved format).
                    let categories = window.read(repo::list_categories).unwrap_or_default();
                    for u in plan::bank_category_uses(
                        &s.analysis.table,
                        &s.analysis.mapping,
                        &categories,
                    ) {
                        s.analysis
                            .mapping
                            .bank_categories
                            .entry(u.key)
                            .or_insert(u.choice);
                    }
                    s.plan = window.read(|conn| {
                        plan::plan(conn, &s.analysis.table, &s.analysis.mapping, account)
                    });
                }
            }
        }
        clear(&self.result_box);
        self.result_box.append(&self.preview_section());
        self.result_box.append(&self.summary_section());
        self.result_box.append(&self.buttons());
    }

    // ----- Sections ------------------------------------------------------------

    fn file_section(self: &Rc<Self>) -> gtk::Box {
        let state = self.state.borrow();
        let s = state.as_ref().unwrap();
        let list = boxed_list();

        let file_row = adw::ActionRow::builder()
            .title(&s.filename)
            .subtitle(format!(
                "{} · {} · {}",
                file_size(s.bytes.len()),
                n_text("{n} ligne", "{n} lignes", s.analysis.table.rows.len()),
                n_text(
                    "{n} colonne",
                    "{n} colonnes",
                    s.analysis.table.headers.len()
                )
            ))
            .title_lines(2)
            .build();
        let change = gtk::Button::builder()
            .label(gettext("Changer…"))
            .valign(gtk::Align::Center)
            .build();
        let weak = Rc::downgrade(self);
        change.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.choose_file();
            }
        });
        file_row.add_suffix(&change);
        list.append(&file_row);

        // Destination account, or a new one.
        let accounts = self
            .window()
            .and_then(|w| w.read(|conn| core_accounts::overviews(conn, false)))
            .unwrap_or_default();
        if accounts.is_empty() {
            let row = adw::ActionRow::builder()
                .title(gettext("Compte de destination"))
                .subtitle(gettext(
                    "Aucun compte pour l’instant : créez celui qui recevra ces opérations.",
                ))
                .subtitle_lines(2)
                .build();
            let create = gtk::Button::builder()
                .label(gettext("Créer un compte…"))
                .valign(gtk::Align::Center)
                .css_classes(["suggested-action"])
                .build();
            let weak = Rc::downgrade(self);
            create.connect_clicked(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.create_account();
                }
            });
            row.add_suffix(&create);
            list.append(&row);
        } else {
            let mut labels: Vec<String> = accounts
                .iter()
                .map(|o| match &o.bank_name {
                    Some(bank) => format!("{} · {}", o.account.name, bank),
                    None => o.account.name.clone(),
                })
                .collect();
            labels.push(gettext("Nouveau compte…"));
            let account_row = adw::ComboRow::builder()
                .title(gettext("Compte de destination"))
                .subtitle(gettext("Les opérations seront ajoutées à ce compte"))
                .model(&string_list(&labels))
                .build();
            let selected = s
                .account_id
                .and_then(|id| accounts.iter().position(|o| o.account.id == id))
                .unwrap_or(labels.len() - 1);
            account_row.set_selected(selected as u32);
            let ids: Vec<Id> = accounts.iter().map(|o| o.account.id).collect();
            let weak = Rc::downgrade(self);
            account_row.connect_selected_notify(move |row| {
                let Some(this) = weak.upgrade() else { return };
                if this.updating.get() {
                    return;
                }
                match ids.get(row.selected() as usize) {
                    Some(&id) => {
                        this.state.borrow_mut().as_mut().unwrap().account_id = Some(id);
                        this.replan_later();
                    }
                    None => this.create_account(),
                }
            });
            list.append(&account_row);
        }

        // Recognised model or new format.
        let (subtitle, badge_text, late) = match &s.analysis.profile {
            Some(p) => (
                gettext("Modèle reconnu : {name}. Les colonnes sont associées, vérifiez-les si besoin.")
                    .replace("{name}", &p.name),
                gettext("Reconnu"),
                false,
            ),
            None => (
                gettext("Format jamais importé : associez les colonnes ci-dessous. Il sera reconnu la prochaine fois."),
                gettext("Nouveau format"),
                true,
            ),
        };
        let model_row = adw::ActionRow::builder()
            .title(gettext("Modèle de banque"))
            .subtitle(subtitle)
            .subtitle_lines(3)
            .build();
        model_row.add_suffix(&badge(&badge_text, late));
        list.append(&model_row);

        section(heading(&gettext("Fichier")).upcast_ref(), &list)
    }

    fn format_section(self: &Rc<Self>) -> gtk::Box {
        let state = self.state.borrow();
        let s = state.as_ref().unwrap();
        let m = &s.analysis.mapping;
        let list = boxed_list();

        let combo = |title: String, items: Vec<String>, selected: usize| {
            let row = adw::ComboRow::builder()
                .title(title)
                .model(&string_list(&items))
                .build();
            row.set_selected(selected as u32);
            row
        };

        let encoding = combo(
            gettext("Encodage"),
            Encoding::ALL
                .iter()
                .map(|e| e.label().to_string())
                .collect(),
            Encoding::ALL
                .iter()
                .position(|e| *e == m.encoding)
                .unwrap_or(0),
        );
        let separator = combo(
            gettext("Séparateur de colonnes"),
            format::SEPARATORS
                .iter()
                .map(|c| gettext(format::separator_label(*c)))
                .collect(),
            format::SEPARATORS
                .iter()
                .position(|c| *c == m.separator)
                .unwrap_or(0),
        );
        let header = combo(
            gettext("Ligne des en-têtes"),
            (1..=10)
                .map(|n| gettext("Ligne {n}").replace("{n}", &n.to_string()))
                .collect(),
            m.skip_records.min(9),
        );
        let dates = combo(
            gettext("Format des dates"),
            DateFormat::ALL
                .iter()
                .map(|d| d.label().to_string())
                .collect(),
            DateFormat::ALL
                .iter()
                .position(|d| *d == m.date_format)
                .unwrap_or(0),
        );
        if s.analysis.date_ambiguous {
            dates.set_subtitle(&gettext(
                "À vérifier : les jours et les mois peuvent être inversés",
            ));
            dates.add_css_class("warning");
        }
        let decimal = combo(
            gettext("Séparateur décimal"),
            vec![gettext("Virgule ( , )"), gettext("Point ( . )")],
            if m.decimal == '.' { 1 } else { 0 },
        );

        let weak = Rc::downgrade(self);
        let on_change = move |reread: bool, apply: MappingSetter| {
            let weak = weak.clone();
            move |row: &adw::ComboRow| {
                let Some(this) = weak.upgrade() else { return };
                if this.updating.get() {
                    return;
                }
                {
                    let mut state = this.state.borrow_mut();
                    let s = state.as_mut().unwrap();
                    apply(&mut s.analysis.mapping, row.selected());
                    if reread {
                        s.analysis.table = plan::read_table(&s.bytes, &s.analysis.mapping);
                        let (suggested, _) =
                            finaryx_core::importers::mapping::suggest(&s.analysis.table.headers);
                        let columns = &mut s.analysis.mapping.columns;
                        columns.retain(|c, _| suggested.contains_key(c));
                        for (c, f) in suggested {
                            columns.entry(c).or_insert(f);
                        }
                    }
                }
                if reread {
                    this.rebuild_all_later();
                } else {
                    this.rebuild_mapping_later();
                }
            }
        };
        encoding.connect_selected_notify(on_change(
            true,
            Box::new(|m, i| m.encoding = Encoding::ALL[i as usize % Encoding::ALL.len()]),
        ));
        separator.connect_selected_notify(on_change(
            true,
            Box::new(|m, i| {
                m.separator = format::SEPARATORS[i as usize % format::SEPARATORS.len()]
            }),
        ));
        header.connect_selected_notify(on_change(
            true,
            Box::new(|m, i| m.skip_records = i as usize),
        ));
        dates.connect_selected_notify(on_change(
            false,
            Box::new(|m, i| m.date_format = DateFormat::ALL[i as usize % DateFormat::ALL.len()]),
        ));
        decimal.connect_selected_notify(on_change(
            false,
            Box::new(|m, i| m.decimal = if i == 1 { '.' } else { ',' }),
        ));

        for row in [&encoding, &separator, &header, &dates, &decimal] {
            list.append(row);
        }
        let title = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        title.append(&heading(&gettext("Format du fichier")));
        title.append(&caption(&gettext(
            "Détecté automatiquement, à corriger si la lecture est mauvaise",
        )));
        section(title.upcast_ref(), &list)
    }

    fn columns_section(self: &Rc<Self>) -> gtk::Box {
        let state = self.state.borrow();
        let s = state.as_ref().unwrap();
        let m = &s.analysis.mapping;

        let title = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        title.append(&heading(&gettext("Colonnes du fichier")));
        title.append(&caption(&gettext(
            "Dites à quoi sert chaque colonne du fichier. Les colonnes inutiles restent sur « Ignorer ».",
        )));

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .build();

        // How the amount is written: three linked toggle buttons.
        let narrow = self.narrow.get();
        let mode_row = gtk::Box::builder()
            .spacing(if narrow { 8 } else { 16 })
            .orientation(if narrow {
                gtk::Orientation::Vertical
            } else {
                gtk::Orientation::Horizontal
            })
            .build();
        let mode_label = gtk::Label::builder()
            .label(gettext("Comment le montant est écrit"))
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .build();
        mode_row.append(&mode_label);
        let toggles = gtk::Box::builder()
            .css_classes(["linked"])
            .valign(gtk::Align::Center)
            .homogeneous(narrow)
            .build();
        toggles.update_property(&[gtk::accessible::Property::Label(&gettext(
            "Écriture du montant",
        ))]);
        let mut first: Option<gtk::ToggleButton> = None;
        for mode in AmountMode::ALL {
            let button = gtk::ToggleButton::builder()
                .label(gettext(mode.label()))
                .active(m.amount_mode == mode)
                .build();
            if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            }
            if let Some(f) = &first {
                button.set_group(Some(f));
            } else {
                first = Some(button.clone());
            }
            let weak = Rc::downgrade(self);
            button.connect_toggled(move |b| {
                let Some(this) = weak.upgrade() else { return };
                if !b.is_active() || this.updating.get() {
                    return;
                }
                this.state
                    .borrow_mut()
                    .as_mut()
                    .unwrap()
                    .analysis
                    .mapping
                    .amount_mode = mode;
                this.rebuild_all_later();
            });
            toggles.append(&button);
        }
        mode_row.append(&toggles);
        body.append(&mode_row);

        // One row per column.
        let list = boxed_list();
        let fields: Vec<String> = Field::ALL.iter().map(|f| gettext(f.label())).collect();
        let rows: Rc<RefCell<Vec<(String, adw::ComboRow)>>> = Rc::default();
        for header in &s.analysis.table.headers {
            let example = s.analysis.table.example(header).unwrap_or("").to_owned();
            let row = adw::ComboRow::builder()
                .title(header)
                .subtitle(if example.is_empty() {
                    gettext("Exemple : (vide)")
                } else {
                    gettext("Exemple : {value}").replace("{value}", &example)
                })
                .model(&string_list(&fields))
                .build();
            let field = m.columns.get(header).copied().unwrap_or(Field::Ignore);
            row.set_selected(Field::ALL.iter().position(|f| *f == field).unwrap_or(0) as u32);
            row.update_property(&[gtk::accessible::Property::Label(
                &gettext("Champ pour la colonne {column}").replace("{column}", header),
            )]);
            if field == Field::Ignore {
                row.add_css_class("dim-label");
            }
            let weak = Rc::downgrade(self);
            let column = header.clone();
            let all_rows = rows.clone();
            row.connect_selected_notify(move |row| {
                let Some(this) = weak.upgrade() else { return };
                if this.updating.get() {
                    return;
                }
                let field = Field::ALL[row.selected() as usize % Field::ALL.len()];
                if field == Field::Ignore {
                    row.add_css_class("dim-label");
                } else {
                    row.remove_css_class("dim-label");
                }
                {
                    let mut state = this.state.borrow_mut();
                    let columns = &mut state.as_mut().unwrap().analysis.mapping.columns;
                    // One column per field: the previous one is ignored.
                    if field != Field::Ignore {
                        this.updating.set(true);
                        for (other, other_row) in all_rows.borrow().iter() {
                            if *other != column && columns.get(other) == Some(&field) {
                                columns.insert(other.clone(), Field::Ignore);
                                other_row.set_selected(0);
                                other_row.add_css_class("dim-label");
                            }
                        }
                        this.updating.set(false);
                    }
                    columns.insert(column.clone(), field);
                }
                this.rebuild_mapping_later();
            });
            rows.borrow_mut().push((header.clone(), row.clone()));
            list.append(&row);
        }
        body.append(&list);

        // Values of the direction column.
        if m.amount_mode == AmountMode::AmountAndDirection {
            let values = boxed_list();
            for (debit, title, current) in [
                (
                    true,
                    gettext("Valeurs pour un débit"),
                    m.debit_values.join(", "),
                ),
                (
                    false,
                    gettext("Valeurs pour un crédit"),
                    m.credit_values.join(", "),
                ),
            ] {
                let entry = adw::EntryRow::builder()
                    .title(title)
                    .text(current)
                    .show_apply_button(true)
                    .build();
                let weak = Rc::downgrade(self);
                entry.connect_apply(move |e| {
                    let Some(this) = weak.upgrade() else { return };
                    let list: Vec<String> = e
                        .text()
                        .split(',')
                        .map(|v| v.trim().to_owned())
                        .filter(|v| !v.is_empty())
                        .collect();
                    {
                        let mut state = this.state.borrow_mut();
                        let m = &mut state.as_mut().unwrap().analysis.mapping;
                        if debit {
                            m.debit_values = list;
                        } else {
                            m.credit_values = list;
                        }
                    }
                    this.rebuild_mapping_later();
                });
                values.append(&entry);
            }
            body.append(&values);
            body.append(&caption(&gettext(
                "Séparez les valeurs par des virgules ; la casse et les accents ne comptent pas.",
            )));
        }
        section(title.upcast_ref(), &body)
    }

    fn categories_section(self: &Rc<Self>) -> Option<gtk::Box> {
        let window = self.window()?;
        let categories: Vec<Category> = window.read(repo::list_categories)?;
        let state = self.state.borrow();
        let s = state.as_ref()?;
        let uses = plan::bank_category_uses(&s.analysis.table, &s.analysis.mapping, &categories);
        if uses.is_empty() {
            return None;
        }
        let list = boxed_list();
        let names: Vec<String> = categories.iter().map(|c| c.name.clone()).collect();
        let shown = if s.show_all_categories {
            uses.len()
        } else {
            CATEGORY_ROWS.min(uses.len())
        };
        for u in uses.iter().take(shown) {
            let mut subtitle = Vec::new();
            if let Some(sub) = &u.subcategory {
                subtitle.push(sub.clone());
            }
            subtitle.push(n_text("{n} opération", "{n} opérations", u.count));
            let row = adw::ActionRow::builder()
                .title(&u.category)
                .subtitle(subtitle.join(" · "))
                .subtitle_lines(2)
                .build();
            let excluded =
                u.choice.category == finaryx_core::importers::bank_categories::INTERNAL_TRANSFER;
            let (text, late) = if excluded {
                (gettext("Exclue"), false)
            } else if u.choice.needs_review {
                (gettext("À vérifier"), true)
            } else if s.known_keys.contains(&u.key) {
                (gettext("Reconnue"), false)
            } else {
                (gettext("Proposée"), true)
            };
            row.add_suffix(&badge(&text, late));

            let dropdown = gtk::DropDown::builder()
                .model(&string_list(&names))
                .valign(gtk::Align::Center)
                .selected(
                    names
                        .iter()
                        .position(|n| *n == u.choice.category)
                        .unwrap_or(0) as u32,
                )
                .build();
            dropdown.update_property(&[gtk::accessible::Property::Label(
                &gettext("Catégorie de Finaryx pour {bank}").replace("{bank}", &u.key),
            )]);
            let weak = Rc::downgrade(self);
            let key = u.key.clone();
            let names = names.clone();
            dropdown.connect_selected_notify(move |d| {
                let Some(this) = weak.upgrade() else { return };
                if this.updating.get() {
                    return;
                }
                if let Some(name) = names.get(d.selected() as usize) {
                    let mut state = this.state.borrow_mut();
                    state
                        .as_mut()
                        .unwrap()
                        .analysis
                        .mapping
                        .bank_categories
                        .insert(
                            key.clone(),
                            BankCategoryChoice {
                                category: name.clone(),
                                needs_review: false,
                            },
                        );
                }
                this.rebuild_mapping_later();
            });
            let slot = categories
                .iter()
                .find(|c| c.name == u.choice.category)
                .and_then(|c| c.slot);
            let swatch = charts::color_swatch(10, move |w| charts::category_color(w, slot));
            row.add_suffix(&swatch);
            row.add_suffix(&dropdown);
            list.append(&row);
        }
        if uses.len() > CATEGORY_ROWS {
            let label = if s.show_all_categories {
                gettext("Afficher moins")
            } else {
                n_text(
                    "Voir la correspondance",
                    "Voir les {n} correspondances",
                    uses.len(),
                )
            };
            let more = gtk::Button::builder()
                .label(label)
                .css_classes(["flat"])
                .build();
            let weak = Rc::downgrade(self);
            more.connect_clicked(move |_| {
                let Some(this) = weak.upgrade() else { return };
                {
                    let mut state = this.state.borrow_mut();
                    let s = state.as_mut().unwrap();
                    s.show_all_categories = !s.show_all_categories;
                }
                this.rebuild_mapping_later();
            });
            list.append(
                &gtk::ListBoxRow::builder()
                    .child(&more)
                    .activatable(false)
                    .build(),
            );
        }
        let title = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        title.append(&heading(&gettext("Catégories de la banque")));
        title.append(&caption(&gettext(
            "La banque classe déjà ses opérations. Choisissez la catégorie de Finaryx qui correspond à chacune.",
        )));
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        body.append(&list);
        body.append(&caption(&gettext(
            "Cette correspondance est enregistrée avec le format. Une catégorie de la banque qui apparaîtra plus tard \
             vous sera demandée ici.",
        )));
        Some(section(title.upcast_ref(), &body))
    }

    fn preview_section(self: &Rc<Self>) -> gtk::Box {
        let state = self.state.borrow();
        let s = state.as_ref().unwrap();
        let title = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        title.append(&heading(&gettext("Aperçu")));
        title.append(&caption(&gettext(
            "Cinq premières lignes, catégories d’après la correspondance ci-dessus",
        )));
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();

        let Some(plan) = &s.plan else {
            let text = if s.account_id.is_none() {
                gettext("Choisissez ou créez le compte de destination pour voir l’aperçu.")
            } else {
                gettext("L’aperçu apparaîtra quand les colonnes obligatoires seront associées.")
            };
            body.append(&caption(&text));
            return section(title.upcast_ref(), &body);
        };
        let categories = self
            .window()
            .and_then(|w| w.read(repo::list_categories))
            .unwrap_or_default();
        let currency = self.account_currency(s.account_id);

        let list = boxed_list();
        for row in plan.rows.iter().take(PREVIEW_ROWS) {
            let t = &row.txn;
            let line = gtk::Box::builder()
                .spacing(14)
                .margin_top(6)
                .margin_bottom(6)
                .margin_start(16)
                .margin_end(16)
                .build();
            let date = caption(&t.booked_on.format("%d/%m").to_string());
            date.set_width_request(44);
            date.set_wrap(false);
            line.append(&date);
            let text = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .hexpand(true)
                .valign(gtk::Align::Center)
                .build();
            text.append(
                &gtk::Label::builder()
                    .label(&t.label)
                    .xalign(0.0)
                    .wrap(true)
                    .wrap_mode(gtk::pango::WrapMode::WordChar)
                    .build(),
            );
            let category = categories.iter().find(|c| Some(c.id) == t.category_id);
            let cat_line = gtk::Box::builder().spacing(8).build();
            let slot = category.and_then(|c| c.slot);
            cat_line.append(&charts::color_swatch(10, move |w| {
                charts::category_color(w, slot)
            }));
            cat_line.append(&caption(category.map(|c| c.name.as_str()).unwrap_or("—")));
            text.append(&cat_line);
            line.append(&text);
            line.append(
                &gtk::Label::builder()
                    .label(amount(t.amount_cents, &currency, Sign::Negative))
                    .css_classes(["numeric"])
                    .valign(gtk::Align::Center)
                    .build(),
            );
            let (state_text, late) = match row.state {
                RowState::New => (gettext("Nouveau"), false),
                RowState::Duplicate => (gettext("Doublon"), false),
                RowState::Review => (gettext("À vérifier"), true),
            };
            let b = badge(&state_text, late);
            b.set_width_request(88);
            if row.state == RowState::Duplicate {
                line.add_css_class("dim-label");
            }
            line.append(&b);
            list.append(
                &gtk::ListBoxRow::builder()
                    .child(&line)
                    .activatable(false)
                    .build(),
            );
        }
        if plan.rows.is_empty() {
            list.append(
                &adw::ActionRow::builder()
                    .title(gettext("Aucune ligne lisible"))
                    .build(),
            );
        }
        body.append(&list);

        let mut warnings = plan.unreadable_messages();
        if !warnings.is_empty() {
            warnings.push(gettext("Ces lignes ne seront pas importées."));
        }
        if !plan.balance.inconsistent_lines.is_empty() {
            let lines: Vec<String> = plan
                .balance
                .inconsistent_lines
                .iter()
                .take(10)
                .map(|l| l.to_string())
                .collect();
            warnings.push(
                gettext("La colonne de solde ne suit pas les montants : ligne {lines}. L’import reste possible.")
                    .replace("{lines}", &lines.join(", ")),
            );
        }
        if !warnings.is_empty() {
            body.append(&problems_box(
                &gettext("À savoir avant d’importer"),
                &warnings,
            ));
        }
        section(title.upcast_ref(), &body)
    }

    fn summary_section(self: &Rc<Self>) -> gtk::Box {
        let state = self.state.borrow();
        let s = state.as_ref().unwrap();
        let list = boxed_list();
        let count_row = |title: String, n: usize| {
            let row = adw::ActionRow::builder().title(title).build();
            row.add_suffix(
                &gtk::Label::builder()
                    .label(n.to_string())
                    .css_classes(["heading", "numeric"])
                    .build(),
            );
            row
        };
        if let Some(plan) = &s.plan {
            list.append(&count_row(gettext("Lignes lues"), plan.read()));
            list.append(&count_row(
                gettext("Nouvelles opérations"),
                plan.to_import(),
            ));
            list.append(&count_row(
                gettext("Doublons ignorés"),
                plan.count(RowState::Duplicate),
            ));
            list.append(&count_row(
                gettext("À vérifier (catégorie incertaine)"),
                plan.count(RowState::Review),
            ));
            let unreadable: usize = plan.unreadable.values().map(Vec::len).sum();
            if unreadable > 0 {
                list.append(&count_row(
                    gettext("Lignes illisibles, non importées"),
                    unreadable,
                ));
            }
            if let Some(opening) = plan.balance.suggested_opening {
                let currency = self.account_currency(s.account_id);
                let currency = currency.as_str();
                let row = adw::SwitchRow::builder()
                    .title(gettext("Recaler le solde initial"))
                    .subtitle(
                        gettext("D’après la colonne de solde, le solde avant la première opération importée devient {amount}.")
                            .replace("{amount}", &amount(opening, currency, Sign::Negative)),
                    )
                    .active(s.adjust_opening)
                    .build();
                let weak = Rc::downgrade(self);
                row.connect_active_notify(move |r| {
                    if let Some(this) = weak.upgrade() {
                        this.state.borrow_mut().as_mut().unwrap().adjust_opening = r.is_active();
                    }
                });
                list.append(&row);
            }
        }
        let remember = adw::SwitchRow::builder()
            .title(gettext("Retenir ce format"))
            .subtitle(gettext(
                "Le prochain fichier de cette banque sera lu sans rien demander",
            ))
            .active(s.remember)
            .build();
        let name = adw::EntryRow::builder()
            .title(gettext("Nom du format"))
            .text(&s.format_name)
            .sensitive(s.remember)
            .build();
        let weak = Rc::downgrade(self);
        let name_row = name.clone();
        remember.connect_active_notify(move |r| {
            if let Some(this) = weak.upgrade() {
                this.state.borrow_mut().as_mut().unwrap().remember = r.is_active();
                name_row.set_sensitive(r.is_active());
            }
        });
        let weak = Rc::downgrade(self);
        name.connect_changed(move |e| {
            if let Some(this) = weak.upgrade() {
                this.state.borrow_mut().as_mut().unwrap().format_name = e.text().to_string();
            }
        });
        list.append(&remember);
        list.append(&name);

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(10)
            .build();
        body.append(&list);
        body.append(&caption(&gettext(
            "Un doublon a la même date, le même libellé, le même montant et la même référence qu’une opération déjà \
             enregistrée. Vos fichiers ne sont jamais modifiés.",
        )));
        section(heading(&gettext("Résumé")).upcast_ref(), &body)
    }

    fn buttons(self: &Rc<Self>) -> gtk::Box {
        let state = self.state.borrow();
        let s = state.as_ref().unwrap();
        let n = s.plan.as_ref().map_or(0, Plan::to_import);
        let import = gtk::Button::builder()
            .label(if n == 0 {
                gettext("Aucune nouvelle opération")
            } else {
                n_text("Importer {n} opération", "Importer {n} opérations", n)
            })
            .sensitive(n > 0)
            .halign(gtk::Align::Center)
            .width_request(260)
            .css_classes(["suggested-action", "pill"])
            .build();
        let weak = Rc::downgrade(self);
        import.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.commit();
            }
        });
        let cancel = gtk::Button::builder()
            .label(gettext("Annuler"))
            .halign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let weak = Rc::downgrade(self);
        cancel.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.page.activate_action("navigation.pop", None).ok();
            }
        });
        let buttons = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(8)
            .margin_bottom(16)
            .halign(gtk::Align::Center)
            .build();
        buttons.append(&import);
        buttons.append(&cancel);
        buttons
    }

    fn account_currency(&self, account: Option<Id>) -> String {
        let accounts = self
            .window()
            .and_then(|w| w.read(|c| core_accounts::overviews(c, true)));
        account
            .zip(accounts)
            .and_then(|(id, list)| list.into_iter().find(|o| o.account.id == id))
            .map(|o| o.account.currency)
            .unwrap_or_else(|| "EUR".into())
    }

    // ----- Actions -------------------------------------------------------------

    fn create_account(self: &Rc<Self>) {
        let Some(window) = self.window() else { return };
        let bank = self.state.borrow().as_ref().and_then(|s| {
            s.analysis
                .profile
                .as_ref()
                .and_then(|p| p.bank_name.clone())
        });
        let weak = Rc::downgrade(self);
        window.add_account_then(bank, move |id| {
            if let Some(this) = weak.upgrade() {
                if let Some(s) = this.state.borrow_mut().as_mut() {
                    s.account_id = Some(id);
                }
                this.rebuild_all_later();
            }
        });
        // Until an account is created, keep the previous choice.
        self.rebuild_all_later();
    }

    fn commit(self: &Rc<Self>) {
        let Some(window) = self.window() else { return };
        let result = {
            let state = self.state.borrow();
            let Some(s) = state.as_ref() else { return };
            let (Some(account), Some(plan)) = (s.account_id, s.plan.as_ref()) else {
                return;
            };
            let bank = window
                .read(|c| core_accounts::overviews(c, true))
                .and_then(|l| l.into_iter().find(|o| o.account.id == account))
                .and_then(|o| o.bank_name);
            let options = CommitOptions {
                save_profile: s.remember.then(|| (s.format_name.clone(), bank)),
                new_opening: if s.adjust_opening {
                    plan.balance.suggested_opening
                } else {
                    None
                },
            };
            window.change(|conn| {
                plan::commit(
                    conn,
                    account,
                    &s.filename,
                    &s.analysis.table,
                    &s.analysis.mapping,
                    plan,
                    &options,
                    &crate::window::now_iso(),
                )
            })
        };
        match result {
            Ok(summary) => {
                let mut text = n_text(
                    "{n} opération importée",
                    "{n} opérations importées",
                    summary.inserted,
                );
                if summary.duplicates > 0 {
                    text = format!(
                        "{text} · {}",
                        n_text(
                            "{n} doublon ignoré",
                            "{n} doublons ignorés",
                            summary.duplicates
                        )
                    );
                }
                self.state.replace(None);
                window.show_page("accounts");
                window.toast(&text);
                window.signal_transfer_proposals();
            }
            Err(message) => window.toast(&message),
        }
    }

    // Rebuilding from a signal handler of a widget that is about to be
    // destroyed is deferred to the main loop.
    fn rebuild_all_later(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(this) = weak.upgrade() {
                this.rebuild_all();
            }
        });
    }

    fn rebuild_mapping_later(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(this) = weak.upgrade() {
                this.rebuild_mapping();
            }
        });
    }

    fn replan_later(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(this) = weak.upgrade() {
                let valid = this.state.borrow().as_ref().is_some_and(|s| {
                    plan::validate(&s.analysis.table, &s.analysis.mapping).is_empty()
                });
                this.replan(valid);
            }
        });
    }
}

/// Destination proposed for a file: the account whose number appears in
/// the file name, else the only account of the recognised bank, else the
/// first account.
fn default_account(
    accounts: &[AccountOverview],
    analysis: &Analysis,
    filename: &str,
) -> Option<Id> {
    let digits: String = filename
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.len() >= 4 {
        if let Some(o) = accounts.iter().find(|o| {
            o.account
                .number
                .as_deref()
                .is_some_and(|n| n.ends_with(&digits) || digits.ends_with(n))
        }) {
            return Some(o.account.id);
        }
    }
    if let Some(bank) = analysis
        .profile
        .as_ref()
        .and_then(|p| p.bank_name.as_deref())
    {
        let same: Vec<&AccountOverview> = accounts
            .iter()
            .filter(|o| o.bank_name.as_deref() == Some(bank))
            .collect();
        if let Some(o) = same.first() {
            return Some(o.account.id);
        }
    }
    accounts.first().map(|o| o.account.id)
}

/// A framed list of messages (validation errors, unreadable lines…).
fn problems_box(title: &str, messages: &[String]) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .css_classes(["card", "padded", "warning-card"])
        .build();
    card.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .wrap(true)
            .css_classes(["heading"])
            .build(),
    );
    for m in messages {
        card.append(
            &gtk::Label::builder()
                .label(format!("• {m}"))
                .xalign(0.0)
                .wrap(true)
                .build(),
        );
    }
    card
}
