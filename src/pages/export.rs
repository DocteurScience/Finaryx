//! "Exporter les données" (design/Export.dc.html): full backup and
//! restore, CSV export of the operations, automatic backup after each
//! import. Nothing is sent online.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use chrono::NaiveDate;
use finaryx_core::export::{self, CsvFilter, CsvFormat};
use finaryx_core::models::Id;
use finaryx_core::{dates, db, repo, stats};
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};

use super::{capitalized, caption, clear};
use crate::window::FinaryxWindow;

pub const TAG: &str = "export";

/// Periods of the CSV export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Period {
    All,
    LastMonths(u32),
    Custom,
}

const PERIODS: [Period; 6] = [
    Period::All,
    Period::LastMonths(1),
    Period::LastMonths(3),
    Period::LastMonths(6),
    Period::LastMonths(12),
    Period::Custom,
];

pub struct ExportPage {
    pub page: adw::NavigationPage,
    window: glib::WeakRef<FinaryxWindow>,
    content: gtk::Box,
    account_ids: RefCell<Vec<Option<Id>>>,
    accounts: adw::ComboRow,
    period: adw::ComboRow,
    from: adw::EntryRow,
    to: adw::EntryRow,
    french: gtk::ToggleButton,
    csv_row: adw::ActionRow,
    db_row: adw::ActionRow,
    /// Month of the most recent operation.
    last_month: Cell<Option<NaiveDate>>,
}

impl std::fmt::Debug for ExportPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportPage").finish_non_exhaustive()
    }
}

/// "JJ/MM/AAAA" or "AAAA-MM-JJ".
fn parse_date(text: &str) -> Option<NaiveDate> {
    let t = text.trim();
    NaiveDate::parse_from_str(t, "%d/%m/%Y")
        .or_else(|_| NaiveDate::parse_from_str(t, "%Y-%m-%d"))
        .ok()
}

/// Clear message for a failed export or restore.
fn error_message(e: &db::Error) -> String {
    match e {
        db::Error::NotFinaryx { .. } => gettext("Ce fichier n’est pas une sauvegarde Finaryx."),
        db::Error::TooNew { .. } => gettext(
            "Cette sauvegarde vient d’une version plus récente de Finaryx : mettez l’application à jour pour l’ouvrir.",
        ),
        db::Error::Sqlite(_) => gettext("Ce fichier n’est pas une base de données lisible."),
        db::Error::Invalid(m) => {
            gettext("Le fichier n’a pas pu être écrit ou lu : {error}").replace("{error}", m)
        }
    }
}

fn row_button(label: &str) -> gtk::Button {
    gtk::Button::builder()
        .label(label)
        .valign(gtk::Align::Center)
        .build()
}

impl ExportPage {
    pub fn new(window: &FinaryxWindow) -> Rc<Self> {
        let header = adw::HeaderBar::new();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(24)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
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
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&scrolled));
        let page = adw::NavigationPage::builder()
            .tag(TAG)
            .title(gettext("Exporter les données"))
            .child(&toolbar)
            .build();

        let period_labels: Vec<String> = PERIODS
            .iter()
            .map(|p| match p {
                Period::All => gettext("Toutes les opérations"),
                Period::LastMonths(1) => gettext("Dernier mois"),
                Period::LastMonths(n) => {
                    gettext("{n} derniers mois").replace("{n}", &n.to_string())
                }
                Period::Custom => gettext("Période choisie"),
            })
            .collect();
        let period_labels: Vec<&str> = period_labels.iter().map(String::as_str).collect();
        let french = gtk::ToggleButton::builder()
            .label(gettext("Français"))
            .active(true)
            .build();
        let this = Rc::new(ExportPage {
            page,
            window: window.downgrade(),
            content,
            account_ids: RefCell::default(),
            accounts: adw::ComboRow::builder().title(gettext("Comptes")).build(),
            period: adw::ComboRow::builder()
                .title(gettext("Période"))
                .model(&gtk::StringList::new(&period_labels))
                .build(),
            from: adw::EntryRow::builder()
                .title(gettext("Du (JJ/MM/AAAA)"))
                .visible(false)
                .build(),
            to: adw::EntryRow::builder()
                .title(gettext("Au (JJ/MM/AAAA)"))
                .visible(false)
                .build(),
            french,
            csv_row: adw::ActionRow::builder()
                .title(gettext("Opérations"))
                .build(),
            db_row: adw::ActionRow::builder()
                .title(gettext("Exporter la base de données"))
                .build(),
            last_month: Cell::new(None),
        });
        this.build();
        this.refresh();
        this
    }

    fn build(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        clear(&self.content);
        self.content.append(&caption(&gettext(
            "Les données restent sur votre ordinateur. Un export crée une copie dans le dossier de votre choix ; rien n’est envoyé en ligne.",
        )));

        // Full backup.
        let backup = adw::PreferencesGroup::builder()
            .title(gettext("Sauvegarde complète"))
            .build();
        self.db_row
            .add_prefix(&gtk::Image::from_icon_name("drive-harddisk-symbolic"));
        let export_db = row_button(&gettext("Exporter…"));
        self.db_row.add_suffix(&export_db);
        backup.add(&self.db_row);
        let restore_row = adw::ActionRow::builder()
            .title(gettext("Restaurer une sauvegarde"))
            .subtitle(gettext(
                "Remplace les données actuelles. Une copie de sécurité est faite avant.",
            ))
            .build();
        let restore = row_button(&gettext("Restaurer…"));
        restore_row.add_suffix(&restore);
        backup.add(&restore_row);
        self.content.append(&backup);
        let weak = Rc::downgrade(self);
        export_db.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.export_database();
            }
        });
        let weak = Rc::downgrade(self);
        restore.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.choose_backup();
            }
        });

        // CSV.
        let csv = adw::PreferencesGroup::builder()
            .title(gettext("Exporter en CSV"))
            .build();
        csv.add(&self.accounts);
        csv.add(&self.period);
        csv.add(&self.from);
        csv.add(&self.to);
        let format_row = adw::ActionRow::builder()
            .title(gettext("Séparateur et décimales"))
            .build();
        let standard = gtk::ToggleButton::builder()
            .label(gettext("Standard"))
            .group(&self.french)
            .build();
        let toggles = gtk::Box::builder()
            .css_classes(["linked"])
            .valign(gtk::Align::Center)
            .build();
        toggles.update_property(&[gtk::accessible::Property::Label(&gettext("Format du CSV"))]);
        toggles.append(&self.french);
        toggles.append(&standard);
        format_row.add_suffix(&toggles);
        csv.add(&format_row);
        let export_csv = row_button(&gettext("Exporter en CSV…"));
        export_csv.add_css_class("suggested-action");
        self.csv_row.add_suffix(&export_csv);
        csv.add(&self.csv_row);
        self.content.append(&csv);

        let update_format = {
            let (format_row, french) = (format_row.clone(), self.french.clone());
            move || {
                format_row.set_subtitle(&if french.is_active() {
                    gettext("Point-virgule et virgule décimale : s’ouvre directement dans un tableur en français.")
                } else {
                    gettext("Virgule et point décimal : le format le plus courant hors de France.")
                });
            }
        };
        update_format();
        self.french.connect_toggled(move |_| update_format());
        for combo in [&self.accounts, &self.period] {
            let weak = Rc::downgrade(self);
            combo.connect_selected_notify(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_csv_count();
                }
            });
        }
        for entry in [&self.from, &self.to] {
            let weak = Rc::downgrade(self);
            entry.connect_changed(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_csv_count();
                }
            });
        }
        let weak = Rc::downgrade(self);
        export_csv.connect_clicked(move |_| {
            if let Some(this) = weak.upgrade() {
                this.export_csv();
            }
        });

        // Automatic backup.
        let auto = adw::PreferencesGroup::builder()
            .title(gettext("Sauvegarde automatique"))
            .build();
        let switch = adw::SwitchRow::builder()
            .title(gettext("Copier la base après chaque import"))
            .build();
        crate::settings::settings()
            .bind("auto-backup", &switch, "active")
            .flags(gio::SettingsBindFlags::DEFAULT | gio::SettingsBindFlags::NO_SENSITIVITY)
            .build();
        match window.backup_dir() {
            Some(dir) => switch.set_subtitle(
                &gettext("Garde les {n} dernières copies dans {dir}.")
                    .replace("{n}", &export::AUTO_BACKUPS_KEPT.to_string())
                    .replace("{dir}", &display_path(&dir)),
            ),
            None => {
                switch.set_subtitle(&gettext(
                    "Désactivée en démonstration : rien n’est enregistré.",
                ));
                switch.set_sensitive(false);
            }
        }
        auto.add(&switch);
        self.content.append(&auto);

        self.content.append(&caption(&gettext(
            "La base n’est pas chiffrée : gardez les exports dans un dossier privé.",
        )));
    }

    /// Re-reads the accounts, the number of operations and the size.
    pub fn refresh(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let Some((accounts, count, last)) = window.read(|c| {
            let last: Option<NaiveDate> =
                c.query_row("SELECT max(booked_on) FROM txn", [], |r| r.get(0))?;
            Ok((
                repo::list_accounts(c, true)?,
                export::csv_count(c, &CsvFilter::default())?,
                last,
            ))
        }) else {
            return;
        };
        self.last_month.set(last.map(stats::first_of_month));
        let size = window
            .database_path()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| super::file_size(m.len()));
        let mut subtitle = gettext(
            "Copie fidèle du fichier finaryx.db (SQLite) : comptes, opérations, catégories et règles.",
        );
        subtitle.push(' ');
        subtitle.push_str(&super::n_ops(count));
        if let Some(size) = size {
            subtitle.push_str(&format!(" · {size}"));
        }
        subtitle.push('.');
        self.db_row.set_subtitle(&subtitle);

        let selected = self.accounts.selected() as usize;
        let previous = self.account_ids.borrow().get(selected).copied().flatten();
        let mut ids = vec![None];
        let mut names = vec![gettext("Tous les comptes")];
        for a in &accounts {
            ids.push(Some(a.id));
            names.push(a.name.clone());
        }
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        self.accounts.set_model(Some(&gtk::StringList::new(&names)));
        let index = ids.iter().position(|i| *i == previous).unwrap_or(0);
        self.account_ids.replace(ids);
        self.accounts.set_selected(index as u32);
        self.update_csv_count();
    }

    fn filter(&self) -> Option<CsvFilter> {
        let account = self
            .account_ids
            .borrow()
            .get(self.accounts.selected() as usize)
            .copied()
            .flatten();
        let period = PERIODS[(self.period.selected() as usize).min(PERIODS.len() - 1)];
        let custom = period == Period::Custom;
        self.from.set_visible(custom);
        self.to.set_visible(custom);
        let (from, to) = match period {
            Period::All => (None, None),
            Period::LastMonths(n) => match self.last_month.get() {
                Some(last) => {
                    let months = stats::months_ending(last, n as usize);
                    (Some(months[0]), None)
                }
                None => (None, None),
            },
            Period::Custom => {
                let from = parse_date(&self.from.text());
                let to = parse_date(&self.to.text());
                for (row, ok) in [(&self.from, from.is_some()), (&self.to, to.is_some())] {
                    if ok || row.text().is_empty() {
                        row.remove_css_class("error");
                    } else {
                        row.add_css_class("error");
                    }
                }
                if from.is_none() || to.is_none() || from > to {
                    return None;
                }
                (from, to)
            }
        };
        Some(CsvFilter { account, from, to })
    }

    fn update_csv_count(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let columns = gettext("Date, libellé, montant, catégorie, compte, banque.");
        let Some(filter) = self.filter() else {
            self.csv_row.set_subtitle(&format!(
                "{columns} {}",
                gettext("Choisissez les deux dates.")
            ));
            return;
        };
        let since = filter
            .from
            .map(|d| gettext("depuis {month}").replace("{month}", &dates::month_year(d)));
        let n = window.read(|c| export::csv_count(c, &filter)).unwrap_or(0);
        let mut text = ngettext(
            "{n} ligne avec ces réglages",
            "{n} lignes avec ces réglages",
            n.clamp(0, u32::MAX as i64) as u32,
        )
        .replace("{n}", &n.to_string());
        if let (Some(since), false) = (since, self.period.selected() as usize == PERIODS.len() - 1)
        {
            text = format!("{text}, {since}");
        }
        self.csv_row
            .set_subtitle(&format!("{columns} {}.", capitalized(&text)));
    }

    fn export_database(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let today = window.today();
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Exporter la base de données"))
            .initial_name(format!("finaryx-{}.db", today.format("%Y-%m-%d")))
            .modal(true)
            .build();
        let weak = window.downgrade();
        dialog.save(Some(&window), gio::Cancellable::NONE, move |result| {
            let (Some(window), Ok(file)) = (weak.upgrade(), result) else {
                return;
            };
            let Some(path) = file.path() else { return };
            match window.read_result(|c| export::backup_to(c, &path)) {
                Ok(()) => show_exported(&window, &file, &gettext("Base exportée")),
                Err(e) => window.toast(&error_message(&e)),
            }
        });
    }

    fn export_csv(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let Some(filter) = self.filter() else {
            window.toast(&gettext("Choisissez les deux dates de la période."));
            return;
        };
        let format = if self.french.is_active() {
            CsvFormat::French
        } else {
            CsvFormat::Standard
        };
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Exporter en CSV"))
            .initial_name(format!(
                "finaryx-operations-{}.csv",
                window.today().format("%Y-%m-%d")
            ))
            .modal(true)
            .build();
        let weak = window.downgrade();
        dialog.save(Some(&window), gio::Cancellable::NONE, move |result| {
            let (Some(window), Ok(file)) = (weak.upgrade(), result) else {
                return;
            };
            let Some(path) = file.path() else { return };
            let written = window.read_result(|c| {
                let (bytes, n) = export::operations_csv(c, &filter, format)?;
                export::write_file(&path, &bytes)?;
                Ok(n)
            });
            match written {
                Ok(n) => show_exported(
                    &window,
                    &file,
                    &ngettext(
                        "{n} opération exportée",
                        "{n} opérations exportées",
                        n as u32,
                    )
                    .replace("{n}", &n.to_string()),
                ),
                Err(e) => window.toast(&error_message(&e)),
            }
        });
    }

    fn choose_backup(self: &Rc<Self>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Sauvegardes Finaryx")));
        filter.add_suffix("db");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Restaurer une sauvegarde"))
            .filters(&filters)
            .modal(true)
            .build();
        let weak = window.downgrade();
        dialog.open(Some(&window), gio::Cancellable::NONE, move |result| {
            let (Some(window), Ok(file)) = (weak.upgrade(), result) else {
                return;
            };
            let Some(path) = file.path() else { return };
            match export::check_backup(&path) {
                Ok(info) => confirm_restore(&window, path, info),
                Err(e) => window.toast(&error_message(&e)),
            }
        });
    }
}

/// "~/.local/share/finaryx/sauvegardes"
fn display_path(path: &std::path::Path) -> String {
    let home = glib::home_dir();
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Toast with an "Ouvrir le dossier" button.
fn show_exported(window: &FinaryxWindow, file: &gio::File, title: &str) {
    let toast = adw::Toast::builder()
        .title(title)
        .button_label(gettext("Ouvrir le dossier"))
        .timeout(6)
        .build();
    let (file, weak) = (file.clone(), window.downgrade());
    toast.connect_button_clicked(move |_| {
        let launcher = gtk::FileLauncher::new(Some(&file));
        let parent = weak.upgrade();
        launcher.open_containing_folder(parent.as_ref(), gio::Cancellable::NONE, |_| {});
    });
    window.add_toast(toast);
}

fn confirm_restore(window: &FinaryxWindow, path: PathBuf, info: export::BackupInfo) {
    let mut body = ngettext(
        "Cette sauvegarde contient {accounts} compte et {operations}.",
        "Cette sauvegarde contient {accounts} comptes et {operations}.",
        info.accounts.clamp(0, u32::MAX as i64) as u32,
    )
    .replace("{accounts}", &info.accounts.to_string())
    .replace("{operations}", &super::n_ops(info.operations));
    body.push(' ');
    body.push_str(&match window.backup_dir() {
        Some(dir) => gettext(
            "Les données actuelles seront remplacées. Une copie de sécurité est d’abord enregistrée dans {dir}.",
        )
        .replace("{dir}", &display_path(&dir)),
        None => gettext("Les données de démonstration seront remplacées, en mémoire seulement."),
    });
    let alert = adw::AlertDialog::new(Some(&gettext("Restaurer cette sauvegarde ?")), Some(&body));
    alert.add_responses(&[
        ("cancel", &gettext("Annuler")),
        ("restore", &gettext("Restaurer")),
    ]);
    alert.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
    alert.set_default_response(Some("cancel"));
    alert.set_close_response("cancel");
    let weak = window.downgrade();
    alert.connect_response(Some("restore"), move |_, _| {
        let Some(window) = weak.upgrade() else { return };
        let safety = window.backup_dir().map(|dir| {
            let _ = std::fs::create_dir_all(&dir);
            export::safety_copy_path(&dir, &crate::window::now_iso())
        });
        match window.change_result(|c| export::restore(c, &path, safety.as_deref())) {
            Ok(_) => window.toast(&gettext("Sauvegarde restaurée")),
            Err(e) => window.toast(&error_message(&e)),
        }
    });
    alert.present(Some(window));
}
