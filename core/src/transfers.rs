//! Transfers between the user's own accounts: neither spending nor
//! income. Two sources of proposals, always confirmed by the user:
//! - a debit and a credit of the same amount, on two accounts of the same
//!   currency, at most 3 days apart;
//! - operations whose label contains one of the household names.

use std::collections::HashSet;

use chrono::NaiveDate;
use rusqlite::{params, Connection};

use crate::categories;
use crate::db::{Error, Result};
use crate::models::{builtin, Id, TransferStatus};
use crate::normalize::label_norm;
use crate::repo;

/// Maximum number of days between the two sides of a transfer.
pub const MAX_DAYS: i64 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Side {
    pub id: Id,
    pub account_name: String,
    pub booked_on: NaiveDate,
    pub label: String,
}

/// A debit and a credit that look like one transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub debit: Side,
    pub credit: Side,
    /// Positive amount moved.
    pub amount_cents: i64,
    pub currency: String,
}

/// Unpaired debit/credit couples of opposite amounts on two accounts of
/// the same currency, at most [`MAX_DAYS`] apart, not rejected before.
/// Each operation is used at most once (closest dates first).
pub fn proposals(conn: &Connection) -> Result<Vec<Proposal>> {
    let mut stmt = conn.prepare(
        "SELECT d.id, da.name, d.booked_on, d.label, c.id, ca.name, c.booked_on, c.label, c.amount_cents, ca.currency,
                abs(julianday(d.booked_on) - julianday(c.booked_on)) AS gap
         FROM txn d JOIN account da ON da.id = d.account_id
         JOIN txn c ON c.amount_cents = -d.amount_cents AND c.account_id <> d.account_id
         JOIN account ca ON ca.id = c.account_id AND ca.currency = da.currency
         WHERE d.amount_cents < 0 AND d.transfer_pair_id IS NULL AND c.transfer_pair_id IS NULL
           AND abs(julianday(d.booked_on) - julianday(c.booked_on)) <= ?1
           AND NOT EXISTS (SELECT 1 FROM transfer_rejection r WHERE r.debit_id = d.id AND r.credit_id = c.id)
         ORDER BY gap, d.booked_on, d.id, c.id",
    )?;
    let rows = stmt.query_map([MAX_DAYS], |r| {
        Ok(Proposal {
            debit: Side {
                id: r.get(0)?,
                account_name: r.get(1)?,
                booked_on: r.get(2)?,
                label: r.get(3)?,
            },
            credit: Side {
                id: r.get(4)?,
                account_name: r.get(5)?,
                booked_on: r.get(6)?,
                label: r.get(7)?,
            },
            amount_cents: r.get(8)?,
            currency: r.get(9)?,
        })
    })?;
    let mut used = HashSet::new();
    let mut out = Vec::new();
    for p in rows {
        let p = p?;
        if used.contains(&p.debit.id) || used.contains(&p.credit.id) {
            continue;
        }
        used.insert(p.debit.id);
        used.insert(p.credit.id);
        out.push(p);
    }
    out.sort_by_key(|p| std::cmp::Reverse(p.debit.booked_on));
    Ok(out)
}

/// Confirms a transfer: both operations are paired and classified as
/// "Virement interne" (a manual choice: rules never override it).
pub fn confirm(conn: &mut Connection, debit: Id, credit: Id, now: &str) -> Result<Id> {
    let tx = conn.transaction()?;
    let pair = repo::pair_transfer(&tx, debit, credit, TransferStatus::Confirmed, now)?;
    tx.execute(
        "UPDATE txn SET category_id = ?3, category_source = 'manual', needs_review = 0 WHERE id IN (?1, ?2)",
        params![debit, credit, builtin::INTERNAL_TRANSFER],
    )?;
    tx.commit()?;
    Ok(pair)
}

/// "Ce n'est pas un virement": the couple will not be proposed again.
pub fn reject(conn: &Connection, debit: Id, credit: Id) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO transfer_rejection (debit_id, credit_id) VALUES (?1, ?2)",
        params![debit, credit],
    )?;
    Ok(())
}

// ----- Household names ---------------------------------------------------------

pub fn household_names(conn: &Connection) -> Result<Vec<(Id, String)>> {
    let mut stmt = conn.prepare("SELECT id, name FROM household_name ORDER BY name")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn add_household_name(conn: &Connection, name: &str) -> Result<Id> {
    let name = name.trim();
    if label_norm(name).is_empty() {
        return Err(Error::Invalid("Le nom est vide.".into()));
    }
    conn.execute(
        "INSERT OR IGNORE INTO household_name (name) VALUES (?1)",
        [name],
    )?;
    Ok(conn.query_row(
        "SELECT id FROM household_name WHERE name = ?1",
        [name],
        |r| r.get(0),
    )?)
}

pub fn remove_household_name(conn: &Connection, id: Id) -> Result<()> {
    conn.execute("DELETE FROM household_name WHERE id = ?1", [id])?;
    Ok(())
}

/// A household name found in operations not yet classified as internal
/// transfers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HouseholdMatch {
    pub name: String,
    /// Operations a "Virement interne" rule would change.
    pub operations: usize,
}

/// For each household name, the operations it would turn into internal
/// transfers (corrections made by hand are left alone).
pub fn household_matches(conn: &Connection) -> Result<Vec<HouseholdMatch>> {
    let mut out = Vec::new();
    for (_, name) in household_names(conn)? {
        let (_, changeable) =
            categories::matching_operations(conn, &name, false, Some(builtin::INTERNAL_TRANSFER))?;
        if !changeable.is_empty() {
            out.push(HouseholdMatch {
                name,
                operations: changeable.len(),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::models::{AccountKind, NewAccount, NewTransaction};

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn account(conn: &Connection, name: &str, currency: &str) -> Id {
        repo::insert_account(
            conn,
            &NewAccount {
                bank_id: None,
                name: name.into(),
                number: None,
                kind: AccountKind::Checking,
                currency: currency.into(),
                opening_balance_cents: 0,
                color_slot: 1,
            },
        )
        .unwrap()
    }

    fn op(conn: &Connection, account: Id, date: &str, label: &str, cents: i64) -> Id {
        repo::insert_transactions(
            conn,
            account,
            None,
            &[NewTransaction::simple(
                d(date),
                label,
                cents,
                builtin::OTHERS,
            )],
        )
        .unwrap()
        .inserted[0]
    }

    #[test]
    fn opposite_amounts_within_three_days_are_proposed() {
        let mut conn = open_in_memory().unwrap();
        let (a, b, usd) = (
            account(&conn, "Courant", "EUR"),
            account(&conn, "Livret", "EUR"),
            account(&conn, "Dollars", "USD"),
        );
        let debit = op(&conn, a, "2026-10-03", "VIR VERS LIVRET", -15_000);
        let credit = op(&conn, b, "2026-10-05", "VIR RECU", 15_000);
        op(&conn, b, "2026-10-12", "TROP TARD", 15_000); // 9 days
        op(&conn, a, "2026-10-04", "MEME COMPTE", 15_000); // same account
        op(&conn, usd, "2026-10-03", "AUTRE DEVISE", 15_000);
        let p = proposals(&conn).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(
            (p[0].debit.id, p[0].credit.id, p[0].amount_cents),
            (debit, credit, 15_000)
        );

        confirm(&mut conn, debit, credit, "2026-11-11T10:00:00").unwrap();
        assert!(proposals(&conn).unwrap().is_empty());
        let (cat, source): (Id, String) = conn
            .query_row(
                "SELECT category_id, category_source FROM txn WHERE id = ?1",
                [credit],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            (cat, source.as_str()),
            (builtin::INTERNAL_TRANSFER, "manual")
        );
    }

    #[test]
    fn rejected_couples_are_not_proposed_again() {
        let conn = open_in_memory().unwrap();
        let (a, b) = (
            account(&conn, "Courant", "EUR"),
            account(&conn, "Joint", "EUR"),
        );
        let debit = op(&conn, a, "2026-10-03", "CHEQUE", -5_000);
        let credit = op(&conn, b, "2026-10-03", "REMISE", 5_000);
        assert_eq!(proposals(&conn).unwrap().len(), 1);
        reject(&conn, debit, credit).unwrap();
        assert!(proposals(&conn).unwrap().is_empty());
    }

    #[test]
    fn household_names_propose_internal_transfers() {
        let mut conn = open_in_memory().unwrap();
        let a = account(&conn, "Courant", "EUR");
        op(&conn, a, "2026-10-03", "VIR M CLAUDE MARTIN", 30_000);
        op(&conn, a, "2026-10-04", "VIR MARTINEZ", 10_000);
        add_household_name(&conn, "Claude Martin").unwrap();
        add_household_name(&conn, "claude martin").unwrap();
        assert_eq!(household_names(&conn).unwrap().len(), 1);
        let m = household_matches(&conn).unwrap();
        assert_eq!(
            m,
            vec![HouseholdMatch {
                name: "Claude Martin".into(),
                operations: 1
            }]
        );
        categories::create_rule(&mut conn, "Claude Martin", builtin::INTERNAL_TRANSFER, true)
            .unwrap();
        assert!(household_matches(&conn).unwrap().is_empty());
        let id = household_names(&conn).unwrap()[0].0;
        remove_household_name(&conn, id).unwrap();
        assert!(household_names(&conn).unwrap().is_empty());
    }
}
