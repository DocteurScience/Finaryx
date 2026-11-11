//! SQLite connection and schema migrations.
//!
//! The database carries a Finaryx-specific `PRAGMA application_id` and its
//! schema version in `PRAGMA user_version`; both are checked when a file is
//! opened (and later when a backup is restored). Opening is isolated here
//! so that an encrypted variant (SQLCipher) can be added without touching
//! the rest of the code.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

/// `PRAGMA application_id` of Finaryx databases: "FNRX" in ASCII.
pub const APPLICATION_ID: i32 = 0x464E_5258;

/// Ordered list of migrations. Migration `i` (0-based) brings the schema
/// from version `i` to `i + 1`. Never edit a migration that has shipped:
/// append a new one.
pub const MIGRATIONS: &[&str] = &[MIGRATION_1];

/// Schema version written by this build of the application.
pub const SCHEMA_VERSION: i32 = MIGRATIONS.len() as i32;

#[derive(Debug)]
pub enum Error {
    Sqlite(rusqlite::Error),
    /// The file is a SQLite database but not a Finaryx one.
    NotFinaryx {
        application_id: i32,
    },
    /// The database was written by a newer Finaryx.
    TooNew {
        version: i32,
        supported: i32,
    },
    /// A consistency check failed (e.g. a demo data invariant).
    Invalid(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Sqlite(e) => write!(f, "SQLite error: {e}"),
            Error::NotFinaryx { application_id } => {
                write!(
                    f,
                    "not a Finaryx database (application_id = {application_id:#x})"
                )
            }
            Error::TooNew { version, supported } => write!(
                f,
                "database schema version {version} is newer than the supported version {supported}"
            ),
            Error::Invalid(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Sqlite(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Opens (creating it if needed) the database at `path` and migrates it.
/// Parent directories must exist.
pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    prepare(conn)
}

/// Opens a fresh in-memory database (tests, demonstration mode).
pub fn open_in_memory() -> Result<Connection> {
    prepare(Connection::open_in_memory()?)
}

fn prepare(mut conn: Connection) -> Result<Connection> {
    conn.pragma_update(None, "foreign_keys", true)?;
    check_identity(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

/// Current schema version of an open database.
pub fn schema_version(conn: &Connection) -> Result<i32> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// Refuses databases that belong to another application or to a newer
/// version of Finaryx. An empty database is accepted (it will be created).
pub fn check_identity(conn: &Connection) -> Result<()> {
    let application_id: i32 = conn.pragma_query_value(None, "application_id", |r| r.get(0))?;
    let version = schema_version(conn)?;
    let has_tables: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table')",
        [],
        |r| r.get(0),
    )?;
    if application_id != APPLICATION_ID && (application_id != 0 || has_tables) {
        return Err(Error::NotFinaryx { application_id });
    }
    if version > SCHEMA_VERSION {
        return Err(Error::TooNew {
            version,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(())
}

/// Applies the missing migrations, each in its own transaction.
pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current = schema_version(conn)?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", index as i32 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// Version 1: schema of the specification (section 2), plus
/// - `account.number`: account number as given by the bank (helps match
///   import files, displayed masked);
/// - `txn.value_on`: value date when the file provides it;
/// - `transfer_pair` / `txn.transfer_pair_id`: the two sides of a transfer
///   between the user's own accounts (proposed, then confirmed);
/// - the built-in categories, in their fixed order.
///
/// Amounts are integers in the minor unit of the account's currency.
const MIGRATION_1: &str = r#"
CREATE TABLE bank (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE COLLATE NOCASE
);

CREATE TABLE account (
  id INTEGER PRIMARY KEY,
  bank_id INTEGER REFERENCES bank(id) ON DELETE RESTRICT,
  name TEXT NOT NULL,
  number TEXT,
  kind TEXT NOT NULL DEFAULT 'checking' CHECK (kind IN ('checking', 'savings')),
  currency TEXT NOT NULL DEFAULT 'EUR' CHECK (length(currency) = 3),
  opening_balance_cents INTEGER NOT NULL DEFAULT 0,
  color_slot INTEGER NOT NULL DEFAULT 1 CHECK (color_slot BETWEEN 1 AND 4),
  archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1))
);

CREATE TABLE category (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (trim(name) <> ''),
  slot INTEGER CHECK (slot BETWEEN 1 AND 8),
  is_income INTEGER NOT NULL DEFAULT 0 CHECK (is_income IN (0, 1)),
  is_builtin INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
  exclude_from_stats INTEGER NOT NULL DEFAULT 0 CHECK (exclude_from_stats IN (0, 1))
);
-- A chart colour belongs to at most one category.
CREATE UNIQUE INDEX category_slot ON category(slot) WHERE slot IS NOT NULL;

CREATE TABLE import_batch (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  filename TEXT,
  imported_at TEXT NOT NULL,
  rows_read INTEGER,
  rows_new INTEGER,
  rows_duplicate INTEGER,
  rows_review INTEGER
);

CREATE TABLE transfer_pair (
  id INTEGER PRIMARY KEY,
  status TEXT NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'confirmed')),
  created_at TEXT NOT NULL
);

CREATE TABLE txn (
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  batch_id INTEGER REFERENCES import_batch(id) ON DELETE CASCADE,
  booked_on TEXT NOT NULL CHECK (booked_on GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]-[0-3][0-9]'),
  label TEXT NOT NULL,
  label_raw TEXT,
  posted_on TEXT,
  value_on TEXT,
  reference TEXT,
  bank_category TEXT,
  label_norm TEXT NOT NULL,
  amount_cents INTEGER NOT NULL,
  category_id INTEGER REFERENCES category(id) ON DELETE SET NULL,
  category_source TEXT NOT NULL DEFAULT 'auto'
    CHECK (category_source IN ('auto', 'rule', 'manual', 'bank')),
  needs_review INTEGER NOT NULL DEFAULT 0 CHECK (needs_review IN (0, 1)),
  transfer_pair_id INTEGER REFERENCES transfer_pair(id) ON DELETE SET NULL,
  dedupe_key TEXT NOT NULL,
  UNIQUE (account_id, dedupe_key)
);
CREATE INDEX txn_date ON txn(booked_on);
CREATE INDEX txn_account_date ON txn(account_id, booked_on);
CREATE INDEX txn_category ON txn(category_id);
CREATE INDEX txn_label_norm ON txn(label_norm);
CREATE INDEX txn_transfer_pair ON txn(transfer_pair_id) WHERE transfer_pair_id IS NOT NULL;

CREATE TABLE import_profile (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  bank_id INTEGER REFERENCES bank(id) ON DELETE SET NULL,
  signature TEXT NOT NULL UNIQUE,
  is_builtin INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
  mapping_json TEXT NOT NULL
);

CREATE TABLE rule (
  id INTEGER PRIMARY KEY,
  pattern TEXT NOT NULL CHECK (pattern <> ''),
  is_regex INTEGER NOT NULL DEFAULT 0 CHECK (is_regex IN (0, 1)),
  category_id INTEGER NOT NULL REFERENCES category(id) ON DELETE CASCADE,
  priority INTEGER NOT NULL DEFAULT 100
);

-- Built-in categories, fixed order and colours (specification §2 and §6).
INSERT INTO category (id, name, slot, is_income, is_builtin, exclude_from_stats) VALUES
  (1,  'Logement',                   1,    0, 1, 0),
  (2,  'Alimentation',               2,    0, 1, 0),
  (3,  'Transports',                 3,    0, 1, 0),
  (4,  'Prêts et crédits',           4,    0, 1, 0),
  (5,  'Shopping et animaux',        5,    0, 1, 0),
  (6,  'Abonnements et télécoms',    6,    0, 1, 0),
  (7,  'Banque et assurances',       7,    0, 1, 0),
  (8,  'Autres',                     8,    0, 1, 0),
  (9,  'Sorties et loisirs',         NULL, 0, 1, 0),
  (10, 'Santé',                      NULL, 0, 1, 0),
  (11, 'Impôts et taxes',            NULL, 0, 1, 0),
  (12, 'Administratif et éducation', NULL, 0, 1, 0),
  (13, 'Revenus',                    NULL, 1, 1, 0),
  (14, 'Virement interne',           NULL, 0, 1, 1);

-- "Autres" is the fallback category and cannot be deleted.
CREATE TRIGGER category_keep_others BEFORE DELETE ON category
WHEN OLD.id = 8
BEGIN
  SELECT RAISE(ABORT, 'the "Autres" category cannot be deleted');
END;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_is_migrated() {
        let conn = open_in_memory().unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        let id: i32 = conn
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .unwrap();
        assert_eq!(id, APPLICATION_ID);
        let fk: bool = conn
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert!(fk);
    }

    #[test]
    fn reopening_keeps_data_and_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("finaryx.db");
        {
            let conn = open(&path).unwrap();
            conn.execute("INSERT INTO bank (name) VALUES ('Banque A')", [])
                .unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(schema_version(&conn).unwrap(), SCHEMA_VERSION);
        let n: i64 = conn
            .query_row("SELECT count(*) FROM bank", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn foreign_database_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("other.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE t (x); PRAGMA application_id = 42;")
                .unwrap();
        }
        assert!(matches!(
            open(&path),
            Err(Error::NotFinaryx { application_id: 42 })
        ));

        // Same without an application id but with tables: not ours either.
        let path = dir.path().join("plain.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE t (x);")
            .unwrap();
        assert!(matches!(
            open(&path),
            Err(Error::NotFinaryx { application_id: 0 })
        ));
    }

    #[test]
    fn newer_schema_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.db");
        {
            let conn = open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        assert!(matches!(open(&path), Err(Error::TooNew { .. })));
    }

    #[test]
    fn builtin_categories_in_fixed_order() {
        let conn = open_in_memory().unwrap();
        let mut stmt = conn
            .prepare("SELECT name, slot FROM category ORDER BY id")
            .unwrap();
        let rows: Vec<(String, Option<i64>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        let colored: Vec<&str> = rows
            .iter()
            .filter(|(_, s)| s.is_some())
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(
            colored,
            [
                "Logement",
                "Alimentation",
                "Transports",
                "Prêts et crédits",
                "Shopping et animaux",
                "Abonnements et télécoms",
                "Banque et assurances",
                "Autres"
            ]
        );
        assert_eq!(rows.len(), 14);
    }

    #[test]
    fn others_cannot_be_deleted_and_slots_are_unique() {
        let conn = open_in_memory().unwrap();
        assert!(conn
            .execute("DELETE FROM category WHERE id = 8", [])
            .is_err());
        assert!(conn
            .execute("UPDATE category SET slot = 1 WHERE id = 9", [])
            .is_err());
        assert!(conn
            .execute("INSERT INTO category (name) VALUES ('logement')", [])
            .is_err());
        assert!(conn
            .execute("INSERT INTO category (name) VALUES ('  ')", [])
            .is_err());
    }
}
