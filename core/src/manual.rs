//! Operations typed by hand ("Saisie manuelle"): for accounts without an
//! export (cash, a bank without CSV), or to stay up to date between two
//! imports. They carry `is_manual = 1`, no import batch, and a dedupe key
//! of their own, so that the bank's version of the same operation is never
//! skipped as a duplicate: the import proposes to replace them instead
//! (`reconciliations`), never without the user's confirmation.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::NaiveDate;
use rusqlite::{params, Connection, OptionalExtension};

use crate::categorize::{Categorizer, Decision};
use crate::db::{Error, Result};
use crate::models::{CategorySource, Id};
use crate::normalize::label_norm;

/// Imported operations within this many days of a manual one, for the
/// same amount, are proposed as its bank version.
pub const RECONCILE_DAYS: i64 = 7;

/// What the user types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualOperation {
    pub booked_on: NaiveDate,
    pub label: String,
    /// Negative for spending.
    pub amount_cents: i64,
    /// `None`: chosen like for an import (known corrections, rules,
    /// keywords).
    pub category_id: Option<Id>,
}

/// Category proposed for a label while the user types.
pub fn suggest_category(conn: &Connection, label: &str) -> Result<Decision> {
    Ok(Categorizer::load(conn)?.categorize(&label_norm(label), None))
}

fn validate(op: &ManualOperation) -> Result<String> {
    let label = op.label.trim();
    if label.is_empty() {
        return Err(Error::Invalid("the label is empty".into()));
    }
    if op.amount_cents == 0 {
        return Err(Error::Invalid("the amount is zero".into()));
    }
    Ok(label.to_owned())
}

/// Category, its source and the review flag of an operation.
fn decide(
    conn: &Connection,
    op: &ManualOperation,
    label: &str,
) -> Result<(Id, CategorySource, bool)> {
    Ok(match op.category_id {
        // Chosen by the user: remembered for the next operations with the
        // same label, like a correction in the list.
        Some(id) => (id, CategorySource::Manual, false),
        None => {
            let d = suggest_category(conn, label)?;
            (d.category_id, d.source, d.needs_review)
        }
    })
}

/// A key no imported operation can have: two identical manual operations
/// (two coffees the same day) are both kept.
fn manual_key() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "manual:{nanos:x}:{:x}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Adds a manual operation to an account.
pub fn add(conn: &Connection, account_id: Id, op: &ManualOperation) -> Result<Id> {
    let label = validate(op)?;
    let (category, source, review) = decide(conn, op, &label)?;
    conn.execute(
        "INSERT INTO txn (account_id, batch_id, booked_on, label, label_norm, amount_cents,
                          category_id, category_source, needs_review, dedupe_key, is_manual)
         VALUES (?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)",
        params![
            account_id,
            op.booked_on,
            label,
            label_norm(&label),
            op.amount_cents,
            category,
            source.as_str(),
            review,
            manual_key(),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// A manual operation as stored, with its account (`None` if `txn_id` is
/// not a manual operation). `category_id` is set only when the user chose
/// it; otherwise the category stays automatic.
pub fn get(conn: &Connection, txn_id: Id) -> Result<Option<(Id, ManualOperation)>> {
    Ok(conn
        .query_row(
            "SELECT account_id, booked_on, label, amount_cents,
                    CASE WHEN category_source = 'manual' THEN category_id END
             FROM txn WHERE id = ?1 AND is_manual = 1",
            [txn_id],
            |r| {
                Ok((
                    r.get(0)?,
                    ManualOperation {
                        booked_on: r.get(1)?,
                        label: r.get(2)?,
                        amount_cents: r.get(3)?,
                        category_id: r.get(4)?,
                    },
                ))
            },
        )
        .optional()?)
}

fn ensure_manual(conn: &Connection, txn_id: Id) -> Result<()> {
    if get(conn, txn_id)?.is_none() {
        return Err(Error::Invalid(
            "only manual operations can be changed".into(),
        ));
    }
    Ok(())
}

/// Removes the transfer pairing of an operation, if any (both sides).
fn unpair(conn: &Connection, txn_id: Id) -> Result<()> {
    conn.execute(
        "DELETE FROM transfer_pair WHERE id = (SELECT transfer_pair_id FROM txn WHERE id = ?1)",
        [txn_id],
    )?;
    Ok(())
}

/// Changes a manual operation; imported operations cannot be changed here.
pub fn update(conn: &Connection, txn_id: Id, op: &ManualOperation) -> Result<()> {
    ensure_manual(conn, txn_id)?;
    let label = validate(op)?;
    let (category, source, review) = decide(conn, op, &label)?;
    let amount: i64 = conn.query_row(
        "SELECT amount_cents FROM txn WHERE id = ?1",
        [txn_id],
        |r| r.get(0),
    )?;
    if amount != op.amount_cents {
        // The amounts of a transfer must stay opposite.
        unpair(conn, txn_id)?;
    }
    conn.execute(
        "UPDATE txn SET booked_on = ?2, label = ?3, label_norm = ?4, amount_cents = ?5,
                        category_id = ?6, category_source = ?7, needs_review = ?8
         WHERE id = ?1",
        params![
            txn_id,
            op.booked_on,
            label,
            label_norm(&label),
            op.amount_cents,
            category,
            source.as_str(),
            review,
        ],
    )?;
    Ok(())
}

/// Deletes a manual operation.
pub fn delete(conn: &Connection, txn_id: Id) -> Result<()> {
    ensure_manual(conn, txn_id)?;
    unpair(conn, txn_id)?;
    conn.execute("DELETE FROM txn WHERE id = ?1", [txn_id])?;
    Ok(())
}

/// A manual operation found again in a file being imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reconciliation {
    pub manual_id: Id,
    pub manual_label: String,
    pub manual_date: NaiveDate,
    pub amount_cents: i64,
    /// Index of the file's operation among the candidates given.
    pub row: usize,
}

/// Pairs the account's manual operations with new operations of a file
/// (`(date, amount)`, `None` for rows that will not be imported): same
/// amount, at most `RECONCILE_DAYS` apart, the nearest date first; each
/// side is used once.
pub fn reconciliations(
    conn: &Connection,
    account_id: Id,
    rows: &[Option<(NaiveDate, i64)>],
) -> Result<Vec<Reconciliation>> {
    let mut stmt = conn.prepare(
        "SELECT id, label, booked_on, amount_cents FROM txn
         WHERE account_id = ?1 AND is_manual = 1 ORDER BY booked_on, id",
    )?;
    let manual: Vec<(Id, String, NaiveDate, i64)> = stmt
        .query_map([account_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut used = vec![false; rows.len()];
    let mut out = Vec::new();
    for (id, label, date, amount) in manual {
        let best = rows
            .iter()
            .enumerate()
            .filter(|(i, _)| !used[*i])
            .filter_map(|(i, r)| r.map(|(d, a)| (i, d, a)))
            .filter(|(_, d, a)| *a == amount && (*d - date).num_days().abs() <= RECONCILE_DAYS)
            .min_by_key(|(i, d, _)| ((*d - date).num_days().abs(), *i));
        if let Some((i, _, _)) = best {
            used[i] = true;
            out.push(Reconciliation {
                manual_id: id,
                manual_label: label,
                manual_date: date,
                amount_cents: amount,
                row: i,
            });
        }
    }
    Ok(out)
}

/// Category the bank's version takes over from a manual operation, and
/// removal of the manual one (inside the import's transaction).
pub(crate) fn take_over(conn: &Connection, manual_id: Id) -> Result<Option<Id>> {
    let category = get(conn, manual_id)?.and_then(|(_, op)| op.category_id);
    delete(conn, manual_id)?;
    Ok(category)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::models::{builtin, AccountKind, NewAccount};
    use crate::repo;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn account(conn: &Connection) -> Id {
        repo::insert_account(
            conn,
            &NewAccount {
                bank_id: None,
                name: "Espèces".into(),
                number: None,
                kind: AccountKind::Checking,
                currency: "EUR".into(),
                opening_balance_cents: 5_000,
                color_slot: 1,
            },
        )
        .unwrap()
    }

    fn op(date: &str, label: &str, cents: i64, category: Option<Id>) -> ManualOperation {
        ManualOperation {
            booked_on: d(date),
            label: label.into(),
            amount_cents: cents,
            category_id: category,
        }
    }

    #[test]
    fn add_update_delete() {
        let conn = open_in_memory().unwrap();
        let a = account(&conn);
        // Two identical operations the same day are both kept.
        let first = add(&conn, a, &op("2026-11-02", "Café", -250, None)).unwrap();
        // Automatic category: not presented as the user's choice.
        assert_eq!(get(&conn, first).unwrap().unwrap().1.category_id, None);
        let second = add(&conn, a, &op("2026-11-02", "Café", -250, None)).unwrap();
        assert_ne!(first, second);
        assert_eq!(repo::account_balance(&conn, a, None).unwrap(), 4_500);

        // Chosen category: remembered as a manual correction.
        update(
            &conn,
            first,
            &op(
                "2026-11-03",
                "  Café du coin ",
                -300,
                Some(builtin::LEISURE),
            ),
        )
        .unwrap();
        let (account_id, stored) = get(&conn, first).unwrap().unwrap();
        assert_eq!(account_id, a);
        assert_eq!(stored.label, "Café du coin");
        assert_eq!(stored.category_id, Some(builtin::LEISURE));
        let source: String = conn
            .query_row(
                "SELECT category_source FROM txn WHERE id = ?1",
                [first],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(source, "manual");
        assert_eq!(
            suggest_category(&conn, "CAFE DU COIN").unwrap().category_id,
            builtin::LEISURE
        );

        assert!(add(&conn, a, &op("2026-11-02", " ", -1, None)).is_err());
        assert!(add(&conn, a, &op("2026-11-02", "Rien", 0, None)).is_err());

        delete(&conn, second).unwrap();
        assert!(get(&conn, second).unwrap().is_none());
        assert_eq!(repo::account_balance(&conn, a, None).unwrap(), 4_700);
    }

    #[test]
    fn imported_operations_are_not_changed_here() {
        let conn = open_in_memory().unwrap();
        let a = account(&conn);
        let imported = repo::insert_transactions(
            &conn,
            a,
            None,
            &[crate::models::NewTransaction::simple(
                d("2026-11-02"),
                "CB BOULANGERIE",
                -180,
                builtin::FOOD,
            )],
        )
        .unwrap()
        .inserted[0];
        assert!(get(&conn, imported).unwrap().is_none());
        assert!(update(&conn, imported, &op("2026-11-02", "X", -1, None)).is_err());
        assert!(delete(&conn, imported).is_err());
    }

    #[test]
    fn reconciliations_pair_the_nearest_date_once() {
        let conn = open_in_memory().unwrap();
        let a = account(&conn);
        let lunch = add(&conn, a, &op("2026-11-02", "Déjeuner", -1_250, None)).unwrap();
        let fuel = add(&conn, a, &op("2026-11-03", "Essence", -6_000, None)).unwrap();
        let far = add(&conn, a, &op("2026-10-01", "Ancien", -1_250, None)).unwrap();
        let rows = [
            Some((d("2026-11-09"), -1_250)), // 7 days after the lunch
            Some((d("2026-11-04"), -1_250)), // nearer: chosen
            None,                            // duplicate, not imported
            Some((d("2026-11-04"), -6_000)),
            Some((d("2026-11-04"), -6_001)),
        ];
        let found = reconciliations(&conn, a, &rows).unwrap();
        let pairs: Vec<(Id, usize)> = found.iter().map(|r| (r.manual_id, r.row)).collect();
        assert_eq!(pairs, vec![(lunch, 1), (fuel, 3)]);
        assert!(!pairs.iter().any(|(id, _)| *id == far));
        assert_eq!(found[0].manual_label, "Déjeuner");
    }
}
