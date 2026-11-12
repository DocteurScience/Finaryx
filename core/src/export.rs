//! Data export (specification §4, "Export des données"): full backup with
//! the SQLite backup API, restore of a checked backup, automatic copies
//! after each import, and CSV export of the operations.
//!
//! Nothing is sent anywhere: every function writes to a path chosen by the
//! user or to the application's data folder.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};

use crate::db::{self, Error, Result};
use crate::models::Id;
use crate::money::Currency;

/// Copies kept in the automatic backup folder.
pub const AUTO_BACKUPS_KEPT: usize = 5;
/// Name of the automatic backup folder, inside the data folder.
pub const AUTO_BACKUP_DIR: &str = "sauvegardes";
const AUTO_PREFIX: &str = "finaryx-auto-";
const SAFETY_PREFIX: &str = "finaryx-avant-restauration-";

fn io_error(e: std::io::Error) -> Error {
    Error::Invalid(e.to_string())
}

/// Consistent copy of the open database into `dest`, even while the
/// application writes: SQLite backup API into a temporary file next to
/// `dest`, then a rename (an existing `dest` is replaced only on success).
pub fn backup_to(conn: &Connection, dest: &Path) -> Result<()> {
    let tmp = dest.with_extension("db.partial");
    let _ = fs::remove_file(&tmp);
    let result = (|| -> Result<()> {
        let mut out = Connection::open(&tmp)?;
        {
            let backup = Backup::new(conn, &mut out)?;
            backup.run_to_completion(256, std::time::Duration::from_millis(0), None)?;
        }
        out.close().map_err(|(_, e)| e)?;
        fs::rename(&tmp, dest).map_err(io_error)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// What a backup file holds, once checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    pub schema_version: i32,
    pub accounts: i64,
    pub operations: i64,
}

/// Checks that `path` is a Finaryx database this build can open: SQLite
/// file, Finaryx `application_id`, schema version not newer than ours,
/// integrity check passed.
pub fn check_backup(path: &Path) -> Result<BackupInfo> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let application_id: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
    if application_id != db::APPLICATION_ID {
        return Err(Error::NotFinaryx { application_id });
    }
    db::check_identity(&conn)?;
    let integrity: String = conn.pragma_query_value(None, "integrity_check", |r| r.get(0))?;
    if integrity != "ok" {
        return Err(Error::Invalid(format!("integrity check: {integrity}")));
    }
    let count = |table: &str| -> Result<i64> {
        Ok(conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?)
    };
    Ok(BackupInfo {
        schema_version: db::schema_version(&conn)?,
        accounts: count("account")?,
        operations: count("txn")?,
    })
}

/// Replaces the content of `conn` with the backup at `src`, after checking
/// it. A safety copy of the current data is written first to
/// `safety_copy` (unless `None`, for data that live in memory only). An
/// older backup is brought up to date with the migrations.
pub fn restore(
    conn: &mut Connection,
    src: &Path,
    safety_copy: Option<&Path>,
) -> Result<BackupInfo> {
    let info = check_backup(src)?;
    if let Some(copy) = safety_copy {
        backup_to(conn, copy)?;
    }
    let source = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    {
        let backup = Backup::new(&source, conn)?;
        backup.run_to_completion(256, std::time::Duration::from_millis(0), None)?;
    }
    conn.pragma_update(None, "foreign_keys", true)?;
    db::migrate(conn)?;
    Ok(info)
}

/// "2026-11-02T18:42:00" → "2026-11-02T18-42-00" (valid in file names).
fn stamp(now: &str) -> String {
    now.chars()
        .map(|c| if c == ':' { '-' } else { c })
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == 'T')
        .collect()
}

/// Path of the safety copy made before a restore.
pub fn safety_copy_path(dir: &Path, now: &str) -> PathBuf {
    dir.join(format!("{SAFETY_PREFIX}{}.db", stamp(now)))
}

/// Copy after an import, in `dir`; keeps the [`AUTO_BACKUPS_KEPT`] most
/// recent copies. Returns the path of the new copy.
pub fn auto_backup(conn: &Connection, dir: &Path, now: &str) -> Result<PathBuf> {
    fs::create_dir_all(dir).map_err(io_error)?;
    let path = dir.join(format!("{AUTO_PREFIX}{}.db", stamp(now)));
    backup_to(conn, &path)?;
    let mut copies: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(io_error)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(AUTO_PREFIX) && n.ends_with(".db"))
        })
        .collect();
    // The time stamp sorts like the date.
    copies.sort();
    let excess = copies.len().saturating_sub(AUTO_BACKUPS_KEPT);
    for old in &copies[..excess] {
        let _ = fs::remove_file(old);
    }
    Ok(path)
}

/// Separator and decimal mark of the CSV export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsvFormat {
    /// `;`, decimal comma, UTF-8 with a BOM (opens in a French spreadsheet).
    French,
    /// `,`, decimal point, UTF-8.
    Standard,
}

/// Operations to export: one account or all; a period (both ends
/// included).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CsvFilter {
    pub account: Option<Id>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

/// Columns of the export. `devise` and `reference` come after the six of
/// the specification: each account has its own currency, and the bank
/// reference is part of the duplicate key, so that re-importing the file
/// adds nothing.
pub const CSV_COLUMNS: [&str; 8] = [
    "date",
    "libelle",
    "montant",
    "categorie",
    "compte",
    "banque",
    "devise",
    "reference",
];

/// Amount in minor units, written without going through a float:
/// "-1234,56" (French) or "-1234.56" (standard); no thousands separator.
pub fn csv_amount(minor: i64, exponent: u32, format: CsvFormat) -> String {
    let sign = if minor < 0 { "-" } else { "" };
    let abs = minor.unsigned_abs();
    if exponent == 0 {
        return format!("{sign}{abs}");
    }
    let unit = 10u64.pow(exponent);
    let mark = match format {
        CsvFormat::French => ',',
        CsvFormat::Standard => '.',
    };
    format!(
        "{sign}{}{mark}{:0width$}",
        abs / unit,
        abs % unit,
        width = exponent as usize
    )
}

fn filter_sql(filter: &CsvFilter) -> (String, Vec<String>) {
    let mut clauses = vec!["1".to_owned()];
    let mut params = Vec::new();
    if let Some(a) = filter.account {
        clauses.push("t.account_id = ?".into());
        params.push(a.to_string());
    }
    if let Some(d) = filter.from {
        clauses.push("t.booked_on >= ?".into());
        params.push(d.to_string());
    }
    if let Some(d) = filter.to {
        clauses.push("t.booked_on <= ?".into());
        params.push(d.to_string());
    }
    (clauses.join(" AND "), params)
}

/// Number of operations matched by `filter`.
pub fn csv_count(conn: &Connection, filter: &CsvFilter) -> Result<i64> {
    let (clause, params) = filter_sql(filter);
    Ok(conn.query_row(
        &format!("SELECT count(*) FROM txn t WHERE {clause}"),
        rusqlite::params_from_iter(params),
        |r| r.get(0),
    )?)
}

/// The CSV file (bytes) and its number of operations, oldest first.
pub fn operations_csv(
    conn: &Connection,
    filter: &CsvFilter,
    format: CsvFormat,
) -> Result<(Vec<u8>, usize)> {
    let (clause, params) = filter_sql(filter);
    let mut stmt = conn.prepare(&format!(
        "SELECT t.booked_on, t.label, t.amount_cents, coalesce(c.name, ''), a.name,
                coalesce(b.name, ''), a.currency, coalesce(t.reference, '')
         FROM txn t
         JOIN account a ON a.id = t.account_id
         LEFT JOIN bank b ON b.id = a.bank_id
         LEFT JOIN category c ON c.id = t.category_id
         WHERE {clause}
         ORDER BY t.account_id, t.booked_on, t.id"
    ))?;
    let mut out: Vec<u8> = Vec::new();
    if format == CsvFormat::French {
        out.extend_from_slice("\u{feff}".as_bytes());
    }
    let mut writer = csv::WriterBuilder::new()
        .delimiter(match format {
            CsvFormat::French => b';',
            CsvFormat::Standard => b',',
        })
        .from_writer(Vec::new());
    let csv_error = |e: csv::Error| Error::Invalid(e.to_string());
    writer.write_record(CSV_COLUMNS).map_err(csv_error)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
    let mut n = 0;
    while let Some(r) = rows.next()? {
        let currency: String = r.get(6)?;
        let exponent = Currency::from_code(&currency).map_or(2, |c| c.exponent);
        let date: String = r.get(0)?;
        let label: String = r.get(1)?;
        let amount = csv_amount(r.get(2)?, exponent, format);
        let category: String = r.get(3)?;
        let account: String = r.get(4)?;
        let bank: String = r.get(5)?;
        let reference: String = r.get(7)?;
        writer
            .write_record([
                &date, &label, &amount, &category, &account, &bank, &currency, &reference,
            ])
            .map_err(csv_error)?;
        n += 1;
    }
    out.extend(
        writer
            .into_inner()
            .map_err(|e| Error::Invalid(e.to_string()))?,
    );
    Ok((out, n))
}

/// Writes `bytes` to `dest` through a temporary file and a rename.
pub fn write_file(dest: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = dest.with_extension("partial");
    fs::write(&tmp, bytes).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        io_error(e)
    })?;
    fs::rename(&tmp, dest).map_err(io_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::importers::plan;
    use crate::stats;

    fn demo() -> Connection {
        let mut conn = open_in_memory().unwrap();
        crate::demo::load(&mut conn).unwrap();
        conn
    }

    /// Aggregates compared before and after a round trip.
    fn fingerprint(conn: &Connection) -> (i64, i64, Vec<i64>, i64) {
        let scope = stats::scope(conn, None).unwrap().unwrap();
        let last = stats::last_operation(conn, &scope).unwrap();
        let months = stats::months_ending(last.unwrap(), 6);
        let monthly: Vec<i64> = months
            .iter()
            .map(|m| stats::month_totals(conn, &scope, *m).unwrap().spending)
            .collect();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM txn", [], |r| r.get(0))
            .unwrap();
        let categories: i64 = conn
            .query_row("SELECT count(*) FROM category", [], |r| r.get(0))
            .unwrap();
        (
            stats::balance_at(conn, &scope, last).unwrap(),
            count,
            monthly,
            categories,
        )
    }

    #[test]
    fn backup_then_restore_gives_the_same_data() {
        let dir = tempfile::tempdir().unwrap();
        let conn = demo();
        let before = fingerprint(&conn);
        let file = dir.path().join("finaryx-2026-11-02.db");
        backup_to(&conn, &file).unwrap();
        let info = check_backup(&file).unwrap();
        assert_eq!(info.operations, before.1);
        assert_eq!(info.accounts, 4);
        assert_eq!(info.schema_version, db::SCHEMA_VERSION);

        // Restore into an empty database, with a safety copy of it.
        let mut other = open_in_memory().unwrap();
        let safety = safety_copy_path(dir.path(), "2026-11-03T10:00:00");
        restore(&mut other, &file, Some(&safety)).unwrap();
        assert_eq!(fingerprint(&other), before);
        assert_eq!(check_backup(&safety).unwrap().operations, 0);
        assert!(safety.ends_with("finaryx-avant-restauration-2026-11-03T10-00-00.db"));
    }

    #[test]
    fn invalid_restores_are_refused_without_touching_the_data() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = demo();
        let before = fingerprint(&conn);

        // Not a database.
        let text = dir.path().join("notes.db");
        fs::write(&text, "ceci n'est pas une base").unwrap();
        // Another application's SQLite database.
        let foreign = dir.path().join("autre.db");
        Connection::open(&foreign)
            .unwrap()
            .execute_batch("CREATE TABLE t (x); PRAGMA application_id = 42;")
            .unwrap();
        // A Finaryx database from a newer version.
        let newer = dir.path().join("futur.db");
        backup_to(&conn, &newer).unwrap();
        Connection::open(&newer)
            .unwrap()
            .pragma_update(None, "user_version", db::SCHEMA_VERSION + 1)
            .unwrap();

        for bad in [&text, &foreign, &newer] {
            let safety = dir.path().join("securite.db");
            assert!(restore(&mut conn, bad, Some(&safety)).is_err(), "{bad:?}");
            assert!(!safety.exists());
            assert_eq!(fingerprint(&conn), before);
        }
        assert!(matches!(check_backup(&newer), Err(Error::TooNew { .. })));
    }

    #[test]
    fn automatic_backups_keep_the_last_five() {
        let dir = tempfile::tempdir().unwrap();
        let conn = demo();
        let folder = dir.path().join(AUTO_BACKUP_DIR);
        for minute in 0..7 {
            auto_backup(&conn, &folder, &format!("2026-11-02T18:{minute:02}:00")).unwrap();
        }
        fs::write(folder.join("autre-fichier.txt"), "garde-moi").unwrap();
        let mut names: Vec<String> = fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "autre-fichier.txt",
                "finaryx-auto-2026-11-02T18-02-00.db",
                "finaryx-auto-2026-11-02T18-03-00.db",
                "finaryx-auto-2026-11-02T18-04-00.db",
                "finaryx-auto-2026-11-02T18-05-00.db",
                "finaryx-auto-2026-11-02T18-06-00.db",
            ]
        );
    }

    #[test]
    fn amounts_without_floats() {
        assert_eq!(csv_amount(-123_456, 2, CsvFormat::French), "-1234,56");
        assert_eq!(csv_amount(5, 2, CsvFormat::Standard), "0.05");
        assert_eq!(csv_amount(-7, 2, CsvFormat::French), "-0,07");
        assert_eq!(csv_amount(1_500, 0, CsvFormat::French), "1500");
        assert_eq!(
            csv_amount(i64::MIN, 2, CsvFormat::Standard),
            "-92233720368547758.08"
        );
    }

    #[test]
    fn french_csv_has_bom_semicolons_and_decimal_commas() {
        let conn = demo();
        let courant: Id = 1;
        let filter = CsvFilter {
            account: Some(courant),
            from: "2026-10-01".parse().ok(),
            to: "2026-10-31".parse().ok(),
        };
        let (bytes, n) = operations_csv(&conn, &filter, CsvFormat::French).unwrap();
        assert_eq!(n as i64, csv_count(&conn, &filter).unwrap());
        let text = String::from_utf8(bytes).unwrap();
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            "\u{feff}date;libelle;montant;categorie;compte;banque;devise;reference"
        );
        assert!(
            text.contains("2026-10-05;LOYER;-690,00;Logement;Compte courant;Banque Populaire;EUR;"),
            "{text}"
        );
        assert!(text.contains("2026-10-01;VIREMENT SALAIRE;2415,00;Revenus;"));
        // The sum of the file is the change of the account over the month.
        let sum: i64 = text
            .lines()
            .skip(1)
            .map(|l| {
                let amount = l.split(';').nth(2).unwrap().replace(',', "");
                amount.parse::<i64>().unwrap()
            })
            .sum();
        assert_eq!(
            sum,
            crate::accounts::month_change(&conn, courant, "2026-10-31".parse().unwrap()).unwrap()
        );

        let (standard, _) = operations_csv(&conn, &filter, CsvFormat::Standard).unwrap();
        let standard = String::from_utf8(standard).unwrap();
        assert!(standard.starts_with("date,libelle,montant"));
        assert!(standard.contains("2026-10-05,LOYER,-690.00,Logement,"));
    }

    #[test]
    fn export_then_reimport_adds_nothing() {
        for format in [CsvFormat::French, CsvFormat::Standard] {
            let conn = demo();
            for account in 1..=4 {
                let filter = CsvFilter {
                    account: Some(account),
                    ..Default::default()
                };
                let (bytes, n) = operations_csv(&conn, &filter, format).unwrap();
                let a = plan::analyze(&conn, &bytes).unwrap();
                let p = plan::plan(&conn, &a.table, &a.mapping, account).unwrap();
                assert!(p.unreadable.is_empty(), "{:?}", p.unreadable_messages());
                assert_eq!(p.rows.len(), n);
                assert_eq!(p.to_import(), 0, "{format:?}, account {account}");
            }
        }
    }

    #[test]
    fn reference_round_trips() {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO bank (id, name) VALUES (1, 'Banque A');
             INSERT INTO account (id, bank_id, name) VALUES (1, 1, 'Courant');
             INSERT INTO txn (account_id, booked_on, label, label_norm, amount_cents, reference, dedupe_key)
               VALUES (1, '2026-10-02', 'CARTE 1', 'CARTE', -1000, 'REF-1', 'x');",
        )
        .unwrap();
        // The stored key is the importer's.
        let key = crate::normalize::dedupe_key("2026-10-02", -1000, "CARTE", Some("REF-1"), 0);
        conn.execute("UPDATE txn SET dedupe_key = ?1", [key])
            .unwrap();
        let (bytes, _) = operations_csv(&conn, &CsvFilter::default(), CsvFormat::French).unwrap();
        let a = plan::analyze(&conn, &bytes).unwrap();
        let p = plan::plan(&conn, &a.table, &a.mapping, 1).unwrap();
        assert_eq!(p.to_import(), 0);
    }
}
