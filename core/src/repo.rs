//! Reading and writing the model. Aggregates are computed in SQL.

use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::db::{Error, Result};
use crate::models::*;
use crate::normalize::{dedupe_key, label_norm, OccurrenceCounter};

// ----- Banks -------------------------------------------------------------

/// Returns the id of the bank with this name, creating it if needed.
pub fn bank_id_or_insert(conn: &Connection, name: &str) -> Result<Id> {
    if let Some(id) = conn
        .query_row("SELECT id FROM bank WHERE name = ?1", [name], |r| r.get(0))
        .optional()?
    {
        return Ok(id);
    }
    conn.execute("INSERT INTO bank (name) VALUES (?1)", [name])?;
    Ok(conn.last_insert_rowid())
}

pub fn list_banks(conn: &Connection) -> Result<Vec<Bank>> {
    let mut stmt = conn.prepare("SELECT id, name FROM bank ORDER BY name")?;
    let rows = stmt.query_map([], |r| {
        Ok(Bank {
            id: r.get(0)?,
            name: r.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// ----- Accounts ----------------------------------------------------------

pub fn insert_account(conn: &Connection, account: &NewAccount) -> Result<Id> {
    conn.execute(
        "INSERT INTO account (bank_id, name, number, kind, currency, opening_balance_cents, color_slot)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            account.bank_id,
            account.name,
            account.number,
            account.kind.as_str(),
            account.currency,
            account.opening_balance_cents,
            account.color_slot,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

fn account_from_row(r: &Row) -> rusqlite::Result<Account> {
    let kind: String = r.get(4)?;
    Ok(Account {
        id: r.get(0)?,
        bank_id: r.get(1)?,
        name: r.get(2)?,
        number: r.get(3)?,
        kind: AccountKind::parse(&kind).unwrap_or(AccountKind::Checking),
        currency: r.get(5)?,
        opening_balance_cents: r.get(6)?,
        color_slot: r.get(7)?,
        archived: r.get(8)?,
    })
}

const ACCOUNT_COLUMNS: &str =
    "id, bank_id, name, number, kind, currency, opening_balance_cents, color_slot, archived";

/// Accounts in creation order.
pub fn list_accounts(conn: &Connection, include_archived: bool) -> Result<Vec<Account>> {
    let sql = format!(
        "SELECT {ACCOUNT_COLUMNS} FROM account {} ORDER BY id",
        if include_archived {
            ""
        } else {
            "WHERE archived = 0"
        }
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], account_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn account_name(conn: &Connection, account_id: Id) -> Result<String> {
    Ok(conn.query_row(
        "SELECT name FROM account WHERE id = ?1",
        [account_id],
        |r| r.get(0),
    )?)
}

/// Number of non-archived accounts and of distinct banks they belong to.
pub fn account_and_bank_counts(conn: &Connection) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        "SELECT count(*), count(DISTINCT bank_id) FROM account WHERE archived = 0",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

pub fn set_opening_balance(conn: &Connection, account_id: Id, cents: i64) -> Result<()> {
    conn.execute(
        "UPDATE account SET opening_balance_cents = ?2 WHERE id = ?1",
        params![account_id, cents],
    )?;
    Ok(())
}

// ----- Categories --------------------------------------------------------

/// Categories in their fixed order: built-in ones, then the user's.
pub fn list_categories(conn: &Connection) -> Result<Vec<Category>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, slot, is_income, is_builtin, exclude_from_stats
         FROM category ORDER BY is_builtin DESC, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Category {
            id: r.get(0)?,
            name: r.get(1)?,
            slot: r.get(2)?,
            is_income: r.get(3)?,
            is_builtin: r.get(4)?,
            exclude_from_stats: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

// ----- Import batches ----------------------------------------------------

pub fn insert_batch(
    conn: &Connection,
    account_id: Id,
    filename: Option<&str>,
    imported_at: &str,
) -> Result<Id> {
    conn.execute(
        "INSERT INTO import_batch (account_id, filename, imported_at, rows_read, rows_new, rows_duplicate, rows_review)
         VALUES (?1, ?2, ?3, 0, 0, 0, 0)",
        params![account_id, filename, imported_at],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_batch_counts(conn: &Connection, batch_id: Id, outcome: &InsertOutcome) -> Result<()> {
    conn.execute(
        "UPDATE import_batch SET rows_read = ?2, rows_new = ?3, rows_duplicate = ?4, rows_review = ?5
         WHERE id = ?1",
        params![
            batch_id,
            outcome.read(),
            outcome.inserted.len() as i64,
            outcome.duplicates as i64,
            outcome.review as i64
        ],
    )?;
    Ok(())
}

/// Most recent import, all accounts together.
pub fn last_batch(conn: &Connection) -> Result<Option<ImportBatch>> {
    Ok(conn
        .query_row(
            "SELECT id, account_id, filename, imported_at, rows_read, rows_new, rows_duplicate, rows_review
             FROM import_batch ORDER BY imported_at DESC, id DESC LIMIT 1",
            [],
            |r| {
                Ok(ImportBatch {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    filename: r.get(2)?,
                    imported_at: r.get(3)?,
                    rows_read: r.get(4)?,
                    rows_new: r.get(5)?,
                    rows_duplicate: r.get(6)?,
                    rows_review: r.get(7)?,
                })
            },
        )
        .optional()?)
}

// ----- Transactions ------------------------------------------------------

/// Result of [`insert_transactions`].
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InsertOutcome {
    /// Ids of the new operations, in input order (duplicates skipped).
    pub inserted: Vec<Id>,
    pub duplicates: usize,
    /// New operations flagged "À vérifier".
    pub review: usize,
}

impl InsertOutcome {
    pub fn read(&self) -> i64 {
        (self.inserted.len() + self.duplicates) as i64
    }
}

/// Inserts operations of one account, skipping those already present
/// (same duplicate key). Run it inside a transaction to make an import
/// atomic.
pub fn insert_transactions(
    conn: &Connection,
    account_id: Id,
    batch_id: Option<Id>,
    txns: &[NewTransaction],
) -> Result<InsertOutcome> {
    let mut stmt = conn.prepare_cached(
        "INSERT INTO txn (account_id, batch_id, booked_on, label, label_raw, posted_on, value_on,
                          reference, bank_category, label_norm, amount_cents, category_id,
                          category_source, needs_review, dedupe_key)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT (account_id, dedupe_key) DO NOTHING",
    )?;
    let mut counter = OccurrenceCounter::default();
    let mut outcome = InsertOutcome::default();
    for t in txns {
        let date = t.booked_on.to_string();
        let norm = label_norm(&t.label);
        let reference = t.reference.as_deref().filter(|r| !r.is_empty());
        let n = counter.next(&date, t.amount_cents, &norm, reference);
        let key = dedupe_key(&date, t.amount_cents, &norm, reference, n);
        let changed = stmt.execute(params![
            account_id,
            batch_id,
            t.booked_on,
            t.label,
            t.label_raw,
            t.posted_on,
            t.value_on,
            t.reference,
            t.bank_category,
            norm,
            t.amount_cents,
            t.category_id,
            t.category_source.as_str(),
            t.needs_review,
            key,
        ])?;
        if changed == 1 {
            outcome.inserted.push(conn.last_insert_rowid());
            if t.needs_review {
                outcome.review += 1;
            }
        } else {
            outcome.duplicates += 1;
        }
    }
    Ok(outcome)
}

/// Links two operations of different accounts as one transfer between the
/// user's own accounts. Their amounts must be opposite.
pub fn pair_transfer(
    conn: &Connection,
    debit_id: Id,
    credit_id: Id,
    status: TransferStatus,
    created_at: &str,
) -> Result<Id> {
    let (a_account, a_amount): (Id, i64) = conn.query_row(
        "SELECT account_id, amount_cents FROM txn WHERE id = ?1",
        [debit_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (b_account, b_amount): (Id, i64) = conn.query_row(
        "SELECT account_id, amount_cents FROM txn WHERE id = ?1",
        [credit_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if a_account == b_account || a_amount != -b_amount {
        return Err(Error::Invalid(format!(
            "operations {debit_id} and {credit_id} are not the two sides of a transfer"
        )));
    }
    conn.execute(
        "INSERT INTO transfer_pair (status, created_at) VALUES (?1, ?2)",
        params![status.as_str(), created_at],
    )?;
    let pair = conn.last_insert_rowid();
    conn.execute(
        "UPDATE txn SET transfer_pair_id = ?1 WHERE id IN (?2, ?3)",
        params![pair, debit_id, credit_id],
    )?;
    Ok(pair)
}

pub fn transaction_count(conn: &Connection, account_id: Option<Id>) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT count(*) FROM txn WHERE ?1 IS NULL OR account_id = ?1",
        [account_id],
        |r| r.get(0),
    )?)
}

// ----- Balances ----------------------------------------------------------

/// Balance of an account at the end of `on` (or after all its operations):
/// opening balance + operations up to that date.
pub fn account_balance(conn: &Connection, account_id: Id, on: Option<NaiveDate>) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT a.opening_balance_cents + coalesce(
                  (SELECT sum(amount_cents) FROM txn
                   WHERE account_id = a.id AND (?2 IS NULL OR booked_on <= ?2)), 0)
         FROM account a WHERE a.id = ?1",
        params![account_id, on],
        |r| r.get(0),
    )?)
}

/// Total balance of the non-archived accounts, one entry per currency
/// (never converted), sorted by currency code.
pub fn total_balance_by_currency(
    conn: &Connection,
    on: Option<NaiveDate>,
) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT a.currency,
                sum(a.opening_balance_cents) + coalesce(sum(t.total), 0)
         FROM account a
         LEFT JOIN (SELECT account_id, sum(amount_cents) AS total FROM txn
                    WHERE ?1 IS NULL OR booked_on <= ?1 GROUP BY account_id) t
                ON t.account_id = a.id
         WHERE a.archived = 0
         GROUP BY a.currency ORDER BY a.currency",
    )?;
    let rows = stmt.query_map([on], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn setup() -> (Connection, Id, Id) {
        let conn = open_in_memory().unwrap();
        let bank = bank_id_or_insert(&conn, "Banque A").unwrap();
        assert_eq!(bank_id_or_insert(&conn, "Banque A").unwrap(), bank);
        let new = |name: &str, currency: &str, opening: i64| NewAccount {
            bank_id: Some(bank),
            name: name.into(),
            number: None,
            kind: AccountKind::Checking,
            currency: currency.into(),
            opening_balance_cents: opening,
            color_slot: 1,
        };
        let a = insert_account(&conn, &new("Courant", "EUR", 100_000)).unwrap();
        let b = insert_account(&conn, &new("Épargne", "EUR", 50_000)).unwrap();
        (conn, a, b)
    }

    #[test]
    fn reimport_creates_nothing_and_identical_rows_are_kept() {
        let (conn, a, _) = setup();
        let rows = vec![
            NewTransaction::simple(d("2026-10-01"), "CAFE", -250, builtin::FOOD),
            NewTransaction::simple(d("2026-10-01"), "CAFE", -250, builtin::FOOD),
            NewTransaction::simple(d("2026-10-02"), "LOYER", -69_000, builtin::HOUSING),
        ];
        let first = insert_transactions(&conn, a, None, &rows).unwrap();
        assert_eq!(first.inserted.len(), 3);
        let again = insert_transactions(&conn, a, None, &rows).unwrap();
        assert_eq!(again.inserted.len(), 0);
        assert_eq!(again.duplicates, 3);
        assert_eq!(transaction_count(&conn, Some(a)).unwrap(), 3);
    }

    #[test]
    fn balances_per_account_date_and_currency() {
        let (conn, a, b) = setup();
        insert_transactions(
            &conn,
            a,
            None,
            &[
                NewTransaction::simple(d("2026-10-01"), "SALAIRE", 200_000, builtin::INCOME),
                NewTransaction::simple(d("2026-10-05"), "LOYER", -69_000, builtin::HOUSING),
            ],
        )
        .unwrap();
        assert_eq!(
            account_balance(&conn, a, Some(d("2026-09-30"))).unwrap(),
            100_000
        );
        assert_eq!(
            account_balance(&conn, a, Some(d("2026-10-01"))).unwrap(),
            300_000
        );
        assert_eq!(account_balance(&conn, a, None).unwrap(), 231_000);
        assert_eq!(account_balance(&conn, b, None).unwrap(), 50_000);
        assert_eq!(account_and_bank_counts(&conn).unwrap(), (2, 1));
        assert_eq!(account_name(&conn, b).unwrap(), "Épargne");

        let bank = bank_id_or_insert(&conn, "Banque B").unwrap();
        insert_account(
            &conn,
            &NewAccount {
                bank_id: Some(bank),
                name: "Dollars".into(),
                number: None,
                kind: AccountKind::Checking,
                currency: "USD".into(),
                opening_balance_cents: 1_000,
                color_slot: 3,
            },
        )
        .unwrap();
        assert_eq!(
            total_balance_by_currency(&conn, None).unwrap(),
            vec![("EUR".to_string(), 281_000), ("USD".to_string(), 1_000)]
        );
    }

    #[test]
    fn transfer_pairs_need_opposite_amounts_on_two_accounts() {
        let (conn, a, b) = setup();
        let out_a = insert_transactions(
            &conn,
            a,
            None,
            &[NewTransaction::simple(
                d("2026-10-03"),
                "VIR EPARGNE",
                -20_000,
                builtin::INTERNAL_TRANSFER,
            )],
        )
        .unwrap();
        let out_b = insert_transactions(
            &conn,
            b,
            None,
            &[
                NewTransaction::simple(
                    d("2026-10-03"),
                    "VIR COURANT",
                    20_000,
                    builtin::INTERNAL_TRANSFER,
                ),
                NewTransaction::simple(d("2026-10-04"), "AUTRE", 19_999, builtin::INCOME),
            ],
        )
        .unwrap();
        let (debit, credit, other) = (out_a.inserted[0], out_b.inserted[0], out_b.inserted[1]);
        assert!(pair_transfer(
            &conn,
            debit,
            other,
            TransferStatus::Proposed,
            "2026-11-01T10:00:00"
        )
        .is_err());
        assert!(pair_transfer(
            &conn,
            credit,
            other,
            TransferStatus::Proposed,
            "2026-11-01T10:00:00"
        )
        .is_err());
        let pair = pair_transfer(
            &conn,
            debit,
            credit,
            TransferStatus::Confirmed,
            "2026-11-01T10:00:00",
        )
        .unwrap();
        let paired: i64 = conn
            .query_row(
                "SELECT count(*) FROM txn WHERE transfer_pair_id = ?1",
                [pair],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(paired, 2);
    }

    #[test]
    fn deleting_a_batch_removes_its_operations() {
        let (conn, a, _) = setup();
        let batch = insert_batch(&conn, a, Some("export.csv"), "2026-11-02T09:00:00").unwrap();
        let outcome = insert_transactions(
            &conn,
            a,
            Some(batch),
            &[NewTransaction::simple(
                d("2026-10-01"),
                "CAFE",
                -250,
                builtin::FOOD,
            )],
        )
        .unwrap();
        update_batch_counts(&conn, batch, &outcome).unwrap();
        assert_eq!(last_batch(&conn).unwrap().unwrap().rows_new, 1);
        conn.execute("DELETE FROM import_batch WHERE id = ?1", [batch])
            .unwrap();
        assert_eq!(transaction_count(&conn, None).unwrap(), 0);
    }
}
