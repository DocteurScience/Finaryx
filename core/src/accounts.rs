//! Accounts page logic: overview of each account, import status, share of
//! the total, account changes.

use chrono::{Datelike, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};

use crate::dates;
use crate::db::{Error, Result};
use crate::models::*;
use crate::repo;

/// Everything the accounts list shows about one account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountOverview {
    pub account: Account,
    pub bank_name: Option<String>,
    /// Balance after all its operations.
    pub balance_cents: i64,
    pub operation_count: i64,
    pub first_operation: Option<NaiveDate>,
    pub last_operation: Option<NaiveDate>,
    pub last_import: Option<ImportBatch>,
}

/// Overviews of the accounts, in creation order.
pub fn overviews(conn: &Connection, include_archived: bool) -> Result<Vec<AccountOverview>> {
    let accounts = repo::list_accounts(conn, include_archived)?;
    let mut out = Vec::with_capacity(accounts.len());
    let mut stats = conn.prepare_cached(
        "SELECT count(*), coalesce(sum(amount_cents), 0), min(booked_on), max(booked_on)
         FROM txn WHERE account_id = ?1",
    )?;
    let mut last_batch = conn.prepare_cached(
        "SELECT id, account_id, filename, imported_at, rows_read, rows_new, rows_duplicate, rows_review
         FROM import_batch WHERE account_id = ?1 ORDER BY imported_at DESC, id DESC LIMIT 1",
    )?;
    for account in accounts {
        let bank_name = match account.bank_id {
            Some(id) => conn
                .query_row("SELECT name FROM bank WHERE id = ?1", [id], |r| r.get(0))
                .optional()?,
            None => None,
        };
        let (count, sum, first, last): (i64, i64, Option<NaiveDate>, Option<NaiveDate>) = stats
            .query_row([account.id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
        let last_import = last_batch
            .query_row([account.id], |r| {
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
            })
            .optional()?;
        out.push(AccountOverview {
            balance_cents: account.opening_balance_cents + sum,
            account,
            bank_name,
            operation_count: count,
            first_operation: first,
            last_operation: last,
            last_import,
        });
    }
    Ok(out)
}

/// State of the last import of an account, shown as a badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportStatus {
    /// "Aucun import"
    NeverImported,
    /// "À jour"
    UpToDate,
    /// "À mettre à jour": last import more than [`STALE_AFTER_DAYS`] ago.
    Stale,
    /// "Octobre à importer": a whole past month has no operation yet. The
    /// date is the first day of the first missing month.
    MonthMissing(NaiveDate),
}

pub const STALE_AFTER_DAYS: i64 = 14;

fn first_of_month(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap()
}

fn next_month(d: NaiveDate) -> NaiveDate {
    let d = first_of_month(d);
    if d.month() == 12 {
        NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap()
    }
}

/// Status of an account's imports on `today`.
pub fn import_status(
    today: NaiveDate,
    last_operation: Option<NaiveDate>,
    last_import: Option<NaiveDate>,
) -> ImportStatus {
    let Some(imported) = last_import else {
        return ImportStatus::NeverImported;
    };
    if let Some(last_op) = last_operation {
        // The month after the last operation is complete and still empty.
        // (An import made after that month ended covered it: the account
        // simply had no operation then.)
        let missing = next_month(last_op);
        if next_month(missing) <= first_of_month(today) && imported < next_month(missing) {
            return ImportStatus::MonthMissing(missing);
        }
    }
    if (today - imported).num_days() > STALE_AFTER_DAYS {
        ImportStatus::Stale
    } else {
        ImportStatus::UpToDate
    }
}

/// Text of the badge, e.g. "Octobre à importer".
pub fn import_status_label(status: ImportStatus) -> String {
    match status {
        ImportStatus::NeverImported => "Aucun import".into(),
        ImportStatus::UpToDate => "À jour".into(),
        ImportStatus::Stale => "À mettre à jour".into(),
        ImportStatus::MonthMissing(month) => {
            let name = dates::month_name(month.month());
            let mut chars = name.chars();
            let first = chars
                .next()
                .map(|c| c.to_uppercase().collect::<String>())
                .unwrap_or_default();
            format!("{first}{} à importer", chars.as_str())
        }
    }
}

/// Shares of each balance in the total, in tenths of a percent, summing
/// to exactly 1000 (largest remainder). Negative balances count as zero.
pub fn shares_per_mille(balances: &[i64]) -> Vec<u32> {
    let positive: Vec<i128> = balances.iter().map(|&b| b.max(0) as i128).collect();
    let total: i128 = positive.iter().sum();
    if total == 0 {
        return vec![0; balances.len()];
    }
    let mut shares: Vec<u32> = positive
        .iter()
        .map(|&b| (b * 1000 / total) as u32)
        .collect();
    let mut remainders: Vec<(i128, usize)> = positive
        .iter()
        .enumerate()
        .map(|(i, &b)| (b * 1000 % total, i))
        .collect();
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let missing = 1000 - shares.iter().sum::<u32>();
    for &(_, i) in remainders.iter().take(missing as usize) {
        shares[i] += 1;
    }
    shares
}

/// "••7305": last four characters of the account number.
pub fn masked_number(number: &str) -> String {
    let digits: Vec<char> = number.chars().filter(|c| !c.is_whitespace()).collect();
    let tail: String = digits[digits.len().saturating_sub(4)..].iter().collect();
    format!("••{tail}")
}

/// Balance at the end of each of the `n` months ending with the month of
/// `last`, oldest first.
pub fn month_end_balances(
    conn: &Connection,
    account_id: Id,
    last: NaiveDate,
    n: usize,
) -> Result<Vec<(NaiveDate, i64)>> {
    let mut ends = Vec::with_capacity(n);
    let mut month = first_of_month(last);
    for _ in 0..n {
        ends.push(next_month(month).pred_opt().unwrap());
        month = first_of_month(month.pred_opt().unwrap());
    }
    ends.reverse();
    ends.into_iter()
        .map(|end| Ok((end, repo::account_balance(conn, account_id, Some(end))?)))
        .collect()
}

/// Balance at the end of each week (Sunday) from `from` to `to`, then at
/// `to` itself, oldest first.
pub fn weekly_balances(
    conn: &Connection,
    account_id: Id,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<(NaiveDate, i64)>> {
    let mut out = Vec::new();
    let mut day = from;
    while day < to {
        if day.weekday() == chrono::Weekday::Sun {
            out.push((day, repo::account_balance(conn, account_id, Some(day))?));
        }
        day = day.succ_opt().unwrap();
    }
    out.push((to, repo::account_balance(conn, account_id, Some(to))?));
    Ok(out)
}

/// First day of the month, `n - 1` months before the month of `d`.
pub fn months_back(d: NaiveDate, n: u32) -> NaiveDate {
    let mut month = first_of_month(d);
    for _ in 1..n {
        month = first_of_month(month.pred_opt().unwrap());
    }
    month
}

/// Fields of an account the user can change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountChanges {
    pub name: String,
    pub bank_name: String,
    pub number: Option<String>,
    pub kind: AccountKind,
    pub currency: String,
    pub opening_balance_cents: i64,
}

fn validate(changes: &AccountChanges) -> Result<()> {
    if changes.name.trim().is_empty() {
        return Err(Error::Invalid("account name is empty".into()));
    }
    if changes.currency.len() != 3 {
        return Err(Error::Invalid("invalid currency".into()));
    }
    Ok(())
}

fn bank_id(conn: &Connection, name: &str) -> Result<Option<Id>> {
    let name = name.trim();
    Ok(if name.is_empty() {
        None
    } else {
        Some(repo::bank_id_or_insert(conn, name)?)
    })
}

/// Creates an account. Its chart colour is the least used of the four.
pub fn create_account(conn: &Connection, changes: &AccountChanges) -> Result<Id> {
    validate(changes)?;
    let color_slot: u8 = conn.query_row(
        "SELECT s.slot FROM (SELECT 1 AS slot UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4) s
         LEFT JOIN account a ON a.color_slot = s.slot AND a.archived = 0
         GROUP BY s.slot ORDER BY count(a.id), s.slot LIMIT 1",
        [],
        |r| r.get(0),
    )?;
    let bank_id = bank_id(conn, &changes.bank_name)?;
    repo::insert_account(
        conn,
        &NewAccount {
            bank_id,
            name: changes.name.trim().to_owned(),
            number: changes.number.clone().filter(|n| !n.trim().is_empty()),
            kind: changes.kind,
            currency: changes.currency.clone(),
            opening_balance_cents: changes.opening_balance_cents,
            color_slot,
        },
    )
}

/// Updates an account; banks no longer used by any account are removed.
pub fn update_account(
    conn: &mut Connection,
    account_id: Id,
    changes: &AccountChanges,
) -> Result<()> {
    validate(changes)?;
    let tx = conn.transaction()?;
    let bank_id = bank_id(&tx, &changes.bank_name)?;
    tx.execute(
        "UPDATE account SET bank_id = ?2, name = ?3, number = ?4, kind = ?5, currency = ?6,
                            opening_balance_cents = ?7
         WHERE id = ?1",
        params![
            account_id,
            bank_id,
            changes.name.trim(),
            changes.number.clone().filter(|n| !n.trim().is_empty()),
            changes.kind.as_str(),
            changes.currency,
            changes.opening_balance_cents,
        ],
    )?;
    remove_unused_banks(&tx)?;
    tx.commit()?;
    Ok(())
}

pub fn set_archived(conn: &Connection, account_id: Id, archived: bool) -> Result<()> {
    conn.execute(
        "UPDATE account SET archived = ?2 WHERE id = ?1",
        params![account_id, archived],
    )?;
    Ok(())
}

fn remove_unused_banks(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM bank WHERE id NOT IN (SELECT bank_id FROM account WHERE bank_id IS NOT NULL)
                         AND id NOT IN (SELECT bank_id FROM import_profile WHERE bank_id IS NOT NULL)",
        [],
    )?;
    Ok(())
}

/// Transfer pairs with a single side left are dissolved.
fn remove_broken_transfer_pairs(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM transfer_pair WHERE id NOT IN
           (SELECT transfer_pair_id FROM txn WHERE transfer_pair_id IS NOT NULL
            GROUP BY transfer_pair_id HAVING count(*) = 2)",
        [],
    )?;
    Ok(())
}

/// Deletes an account with its imports and operations. Returns the number
/// of operations deleted.
pub fn delete_account(conn: &mut Connection, account_id: Id) -> Result<i64> {
    let tx = conn.transaction()?;
    let count = repo::transaction_count(&tx, Some(account_id))?;
    tx.execute("DELETE FROM account WHERE id = ?1", [account_id])?;
    remove_broken_transfer_pairs(&tx)?;
    remove_unused_banks(&tx)?;
    tx.commit()?;
    Ok(count)
}

/// "Annuler cet import": deletes the operations added by an import, and
/// the import itself. Returns the number of operations deleted.
pub fn undo_import(conn: &mut Connection, batch_id: Id) -> Result<i64> {
    let tx = conn.transaction()?;
    let count: i64 = tx.query_row(
        "SELECT count(*) FROM txn WHERE batch_id = ?1",
        [batch_id],
        |r| r.get(0),
    )?;
    tx.execute("DELETE FROM txn WHERE batch_id = ?1", [batch_id])?;
    tx.execute("DELETE FROM import_batch WHERE id = ?1", [batch_id])?;
    remove_broken_transfer_pairs(&tx)?;
    tx.commit()?;
    Ok(count)
}

/// One line of the import history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportHistoryEntry {
    pub batch: ImportBatch,
    pub account_name: String,
    pub bank_name: Option<String>,
    /// Operations of this import still in the database.
    pub remaining: i64,
}

/// Imports, most recent first.
pub fn import_history(conn: &Connection) -> Result<Vec<ImportHistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT b.id, b.account_id, b.filename, b.imported_at, b.rows_read, b.rows_new,
                b.rows_duplicate, b.rows_review, a.name, k.name,
                (SELECT count(*) FROM txn t WHERE t.batch_id = b.id)
         FROM import_batch b JOIN account a ON a.id = b.account_id
         LEFT JOIN bank k ON k.id = a.bank_id
         ORDER BY b.imported_at DESC, b.id DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ImportHistoryEntry {
            batch: ImportBatch {
                id: r.get(0)?,
                account_id: r.get(1)?,
                filename: r.get(2)?,
                imported_at: r.get(3)?,
                rows_read: r.get(4)?,
                rows_new: r.get(5)?,
                rows_duplicate: r.get(6)?,
                rows_review: r.get(7)?,
            },
            account_name: r.get(8)?,
            bank_name: r.get(9)?,
            remaining: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// An operation as listed on the account page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRow {
    pub id: Id,
    pub booked_on: NaiveDate,
    pub label: String,
    pub amount_cents: i64,
    pub category_name: Option<String>,
    /// Chart colour of the category (`None`: grouped in "Autres").
    pub category_slot: Option<u8>,
    pub needs_review: bool,
}

/// Most recent operations of an account.
pub fn recent_operations(
    conn: &Connection,
    account_id: Id,
    limit: usize,
) -> Result<Vec<OperationRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.booked_on, t.label, t.amount_cents, c.name, c.slot, t.needs_review
         FROM txn t LEFT JOIN category c ON c.id = t.category_id
         WHERE t.account_id = ?1
         ORDER BY t.booked_on DESC, t.id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![account_id, limit as i64], |r| {
        Ok(OperationRow {
            id: r.get(0)?,
            booked_on: r.get(1)?,
            label: r.get(2)?,
            amount_cents: r.get(3)?,
            category_name: r.get(4)?,
            category_slot: r.get(5)?,
            needs_review: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Sum of the operations of an account in the month of `day`.
pub fn month_change(conn: &Connection, account_id: Id, day: NaiveDate) -> Result<i64> {
    let start = first_of_month(day);
    let end = next_month(day);
    Ok(conn.query_row(
        "SELECT coalesce(sum(amount_cents), 0) FROM txn
         WHERE account_id = ?1 AND booked_on >= ?2 AND booked_on < ?3",
        params![account_id, start, end],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::demo;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn import_status_follows_the_mockup_cases() {
        let today = d("2026-11-02");
        // Imported on 2 Nov, operations up to 31 Oct.
        assert_eq!(
            import_status(today, Some(d("2026-10-31")), Some(d("2026-11-02"))),
            ImportStatus::UpToDate
        );
        // Savings account imported on 28 Oct with few operations.
        assert_eq!(
            import_status(today, Some(d("2026-10-03")), Some(d("2026-10-28"))),
            ImportStatus::UpToDate
        );
        // Imported on 1 Oct, September only: October is missing.
        assert_eq!(
            import_status(today, Some(d("2026-09-30")), Some(d("2026-10-01"))),
            ImportStatus::MonthMissing(d("2026-10-01"))
        );
        // Imported on 12 Oct: October is incomplete.
        assert_eq!(
            import_status(today, Some(d("2026-10-11")), Some(d("2026-10-12"))),
            ImportStatus::Stale
        );
        assert_eq!(
            import_status(today, None, None),
            ImportStatus::NeverImported
        );
        // A quiet savings account imported after the empty month ended.
        assert_eq!(
            import_status(today, Some(d("2026-08-31")), Some(d("2026-11-01"))),
            ImportStatus::UpToDate
        );
        assert_eq!(
            import_status_label(ImportStatus::MonthMissing(d("2026-10-01"))),
            "Octobre à importer"
        );
        assert_eq!(
            import_status_label(ImportStatus::MonthMissing(d("2026-08-01"))),
            "Août à importer"
        );
    }

    #[test]
    fn shares_sum_to_exactly_one_thousand() {
        let shares = shares_per_mille(&[287_146, 653_000, 195_427, 61_285]);
        assert_eq!(shares, vec![240, 546, 163, 51]);
        assert_eq!(shares.iter().sum::<u32>(), 1000);
        assert_eq!(shares_per_mille(&[1, 1, 1]).iter().sum::<u32>(), 1000);
        assert_eq!(shares_per_mille(&[100, -50]), vec![1000, 0]);
        assert_eq!(shares_per_mille(&[0, 0]), vec![0, 0]);
    }

    #[test]
    fn masks_account_numbers() {
        assert_eq!(masked_number("00000007305"), "••7305");
        assert_eq!(masked_number("12"), "••12");
        assert_eq!(masked_number("FR76 1234 5678"), "••5678");
    }

    #[test]
    fn overviews_of_the_demo() {
        let mut conn = open_in_memory().unwrap();
        demo::load(&mut conn).unwrap();
        let list = overviews(&conn, false).unwrap();
        assert_eq!(list.len(), 4);
        let total: i64 = list.iter().map(|o| o.balance_cents).sum();
        assert_eq!(total, 1_196_858);
        assert_eq!(list[0].bank_name.as_deref(), Some("Banque Populaire"));
        assert_eq!(list[0].last_operation, Some(d("2026-10-31")));
        assert!(list.iter().all(|o| o.last_import.is_some()));

        let livret = month_end_balances(&conn, list[1].account.id, d("2026-10-31"), 6).unwrap();
        assert_eq!(livret.first().unwrap(), &(d("2026-05-31"), 590_000));
        assert_eq!(livret.last().unwrap(), &(d("2026-10-31"), 653_000));

        let weeks =
            weekly_balances(&conn, list[0].account.id, d("2026-05-01"), d("2026-10-31")).unwrap();
        assert_eq!(weeks.len(), 27);
        assert_eq!(weeks.last().unwrap().1, 287_146);
        assert_eq!(months_back(d("2026-10-31"), 6), d("2026-05-01"));
        assert_eq!(months_back(d("2026-02-10"), 3), d("2025-12-01"));

        let recent = recent_operations(&conn, list[0].account.id, 5).unwrap();
        assert_eq!(recent.len(), 5);
        assert_eq!(recent[0].label, "BOULANGERIE DES LILAS");
        assert_eq!(recent[0].category_slot, Some(2));
    }

    #[test]
    fn create_update_archive_delete() {
        let mut conn = open_in_memory().unwrap();
        let changes = AccountChanges {
            name: "  Compte en dollars ".into(),
            bank_name: "Banque X".into(),
            number: Some("".into()),
            kind: AccountKind::Checking,
            currency: "USD".into(),
            opening_balance_cents: 12_345,
        };
        let id = create_account(&conn, &changes).unwrap();
        let account = &repo::list_accounts(&conn, true).unwrap()[0];
        assert_eq!(account.name, "Compte en dollars");
        assert_eq!(account.number, None);
        assert_eq!(account.color_slot, 1);
        let second = create_account(
            &conn,
            &AccountChanges {
                name: "B".into(),
                ..changes.clone()
            },
        )
        .unwrap();
        assert_eq!(repo::list_accounts(&conn, true).unwrap()[1].color_slot, 2);

        assert!(create_account(
            &conn,
            &AccountChanges {
                name: " ".into(),
                ..changes.clone()
            }
        )
        .is_err());

        update_account(
            &mut conn,
            id,
            &AccountChanges {
                bank_name: "Banque Y".into(),
                kind: AccountKind::Savings,
                ..changes.clone()
            },
        )
        .unwrap();
        // "Banque X" is still used by the second account.
        assert_eq!(repo::list_banks(&conn).unwrap().len(), 2);
        update_account(
            &mut conn,
            second,
            &AccountChanges {
                bank_name: "Banque Y".into(),
                ..changes.clone()
            },
        )
        .unwrap();
        assert_eq!(repo::list_banks(&conn).unwrap().len(), 1);

        set_archived(&conn, id, true).unwrap();
        assert_eq!(repo::list_accounts(&conn, false).unwrap().len(), 1);
        assert_eq!(repo::list_accounts(&conn, true).unwrap().len(), 2);
    }

    #[test]
    fn undo_import_and_delete_account_clean_up_transfers() {
        let mut conn = open_in_memory().unwrap();
        let ids = demo::load(&mut conn).unwrap();
        let before = repo::transaction_count(&conn, None).unwrap();
        let history = import_history(&conn).unwrap();
        assert_eq!(history.len(), 4);
        assert_eq!(history[0].account_name, "Compte courant");

        // Undo the Livret A import: its transfers lose their other side.
        let livret_batch = history
            .iter()
            .find(|h| h.batch.account_id == ids[1])
            .unwrap();
        let removed = undo_import(&mut conn, livret_batch.batch.id).unwrap();
        assert_eq!(removed, livret_batch.remaining);
        assert_eq!(
            repo::transaction_count(&conn, None).unwrap(),
            before - removed
        );
        let dangling: i64 = conn
            .query_row(
                "SELECT count(*) FROM txn t WHERE t.transfer_pair_id IS NOT NULL
                 AND (SELECT count(*) FROM txn u WHERE u.transfer_pair_id = t.transfer_pair_id) < 2",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dangling, 0);

        let count = repo::transaction_count(&conn, Some(ids[3])).unwrap();
        assert_eq!(delete_account(&mut conn, ids[3]).unwrap(), count);
        assert_eq!(repo::list_accounts(&conn, true).unwrap().len(), 3);
        // Revolut was the only account of its bank.
        assert_eq!(repo::list_banks(&conn).unwrap().len(), 2);
        assert_eq!(import_history(&conn).unwrap().len(), 2);
    }

    #[test]
    fn month_change_of_the_current_account() {
        let mut conn = open_in_memory().unwrap();
        let ids = demo::load(&mut conn).unwrap();
        let change = month_change(&conn, ids[0], d("2026-10-15")).unwrap();
        let start = repo::account_balance(&conn, ids[0], Some(d("2026-09-30"))).unwrap();
        assert_eq!(start + change, 287_146);
    }
}
