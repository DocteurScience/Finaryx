//! Aggregates for the views (specification §4), computed in SQL.
//!
//! Every query works on a [`Scope`]: a set of accounts sharing one
//! currency (amounts of different currencies are never added up).
//! "Spending" is the opposite of the sum of the operations whose category
//! is neither income nor excluded from statistics ("Virement interne");
//! refunds in a spending category reduce it. "Income" is the sum of the
//! operations of the income categories.

use chrono::{Datelike, NaiveDate};
use rusqlite::{params_from_iter, Connection};

use crate::db::Result;
use crate::models::{builtin, Id};

/// Accounts taken into account by a view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub accounts: Vec<Id>,
    pub currency: String,
    /// Non-archived accounts left out because of their currency.
    pub excluded_accounts: usize,
}

/// The scope of "Tous les comptes" (`selected = None`): the non-archived
/// accounts of the most common currency; or one account.
pub fn scope(conn: &Connection, selected: Option<Id>) -> Result<Option<Scope>> {
    if let Some(id) = selected {
        let currency: Option<String> = conn
            .query_row("SELECT currency FROM account WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .ok();
        return Ok(currency.map(|currency| Scope {
            accounts: vec![id],
            currency,
            excluded_accounts: 0,
        }));
    }
    let currency: Option<String> = conn
        .query_row(
            "SELECT currency FROM account WHERE archived = 0
             GROUP BY currency ORDER BY count(*) DESC, currency = 'EUR' DESC, currency LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok();
    let Some(currency) = currency else {
        return Ok(None);
    };
    let mut stmt =
        conn.prepare("SELECT id FROM account WHERE archived = 0 AND currency = ?1 ORDER BY id")?;
    let accounts: Vec<Id> = stmt
        .query_map([&currency], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let total: i64 =
        conn.query_row("SELECT count(*) FROM account WHERE archived = 0", [], |r| {
            r.get(0)
        })?;
    Ok(Some(Scope {
        excluded_accounts: total as usize - accounts.len(),
        accounts,
        currency,
    }))
}

impl Scope {
    /// "account_id IN (?, ?, …)" and its parameters.
    fn accounts_sql(&self, column: &str) -> String {
        if self.accounts.is_empty() {
            return "0".into();
        }
        let ids: Vec<String> = self.accounts.iter().map(|id| id.to_string()).collect();
        format!("{column} IN ({})", ids.join(","))
    }
}

pub fn first_of_month(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap()
}

pub fn next_month(d: NaiveDate) -> NaiveDate {
    let d = first_of_month(d);
    if d.month() == 12 {
        NaiveDate::from_ymd_opt(d.year() + 1, 1, 1).unwrap()
    } else {
        NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1).unwrap()
    }
}

pub fn previous_month(d: NaiveDate) -> NaiveDate {
    first_of_month(first_of_month(d).pred_opt().unwrap())
}

pub fn days_in_month(d: NaiveDate) -> u32 {
    next_month(d).pred_opt().unwrap().day()
}

/// The `n` months ending with `last`, oldest first (first days).
pub fn months_ending(last: NaiveDate, n: usize) -> Vec<NaiveDate> {
    let mut months = vec![first_of_month(last)];
    while months.len() < n {
        months.push(previous_month(months[months.len() - 1]));
    }
    months.reverse();
    months
}

/// Month of the most recent operation of the scope.
pub fn latest_month(conn: &Connection, scope: &Scope) -> Result<Option<NaiveDate>> {
    let last: Option<NaiveDate> = conn.query_row(
        &format!(
            "SELECT max(booked_on) FROM txn WHERE {}",
            scope.accounts_sql("account_id")
        ),
        [],
        |r| r.get(0),
    )?;
    Ok(last.map(first_of_month))
}

/// Month of the oldest operation of the scope.
pub fn earliest_month(conn: &Connection, scope: &Scope) -> Result<Option<NaiveDate>> {
    let first: Option<NaiveDate> = conn.query_row(
        &format!(
            "SELECT min(booked_on) FROM txn WHERE {}",
            scope.accounts_sql("account_id")
        ),
        [],
        |r| r.get(0),
    )?;
    Ok(first.map(first_of_month))
}

/// Date of the most recent operation of the scope.
pub fn last_operation(conn: &Connection, scope: &Scope) -> Result<Option<NaiveDate>> {
    Ok(conn.query_row(
        &format!(
            "SELECT max(booked_on) FROM txn WHERE {}",
            scope.accounts_sql("account_id")
        ),
        [],
        |r| r.get(0),
    )?)
}

/// SQL expression: spending (positive) of a row joined with `category c`.
const SPENDING: &str =
    "CASE WHEN coalesce(c.is_income, 0) = 0 AND coalesce(c.exclude_from_stats, 0) = 0
                        THEN -t.amount_cents ELSE 0 END";
const INCOME: &str = "CASE WHEN c.is_income = 1 THEN t.amount_cents ELSE 0 END";

/// Totals of a period.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Totals {
    pub spending: i64,
    pub income: i64,
    pub operations: i64,
    /// Largest income operation: (label, amount).
    pub largest_income: Option<(String, i64)>,
}

/// Totals between `from` (included) and `to` (excluded).
pub fn totals(conn: &Connection, scope: &Scope, from: NaiveDate, to: NaiveDate) -> Result<Totals> {
    let (spending, income, operations): (i64, i64, i64) = conn.query_row(
        &format!(
            "SELECT coalesce(sum({SPENDING}), 0), coalesce(sum({INCOME}), 0), count(*)
             FROM txn t LEFT JOIN category c ON c.id = t.category_id
             WHERE {} AND t.booked_on >= ?1 AND t.booked_on < ?2",
            scope.accounts_sql("t.account_id")
        ),
        rusqlite::params![from, to],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let largest_income = conn
        .query_row(
            &format!(
                "SELECT t.label, t.amount_cents FROM txn t JOIN category c ON c.id = t.category_id
                 WHERE {} AND c.is_income = 1 AND t.booked_on >= ?1 AND t.booked_on < ?2
                 ORDER BY t.amount_cents DESC LIMIT 1",
                scope.accounts_sql("t.account_id")
            ),
            rusqlite::params![from, to],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    Ok(Totals {
        spending,
        income,
        operations,
        largest_income,
    })
}

pub fn month_totals(conn: &Connection, scope: &Scope, month: NaiveDate) -> Result<Totals> {
    totals(conn, scope, first_of_month(month), next_month(month))
}

/// Total balance of the scope at the end of `day` (or before any
/// operation if `None`).
pub fn balance_at(conn: &Connection, scope: &Scope, day: Option<NaiveDate>) -> Result<i64> {
    let opening: i64 = conn.query_row(
        &format!(
            "SELECT coalesce(sum(opening_balance_cents), 0) FROM account WHERE {}",
            scope.accounts_sql("id")
        ),
        [],
        |r| r.get(0),
    )?;
    let Some(day) = day else { return Ok(opening) };
    let ops: i64 = conn.query_row(
        &format!(
            "SELECT coalesce(sum(amount_cents), 0) FROM txn WHERE {} AND booked_on <= ?1",
            scope.accounts_sql("account_id")
        ),
        [day],
        |r| r.get(0),
    )?;
    Ok(opening + ops)
}

/// Day by day in a month: balance at the end of each day, and spending of
/// each day. Days after `until` are left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Daily {
    pub balances: Vec<i64>,
    pub spending: Vec<i64>,
    /// Balance on the eve of the month.
    pub start_balance: i64,
}

pub fn daily(
    conn: &Connection,
    scope: &Scope,
    month: NaiveDate,
    until: Option<NaiveDate>,
) -> Result<Daily> {
    let start = first_of_month(month);
    let end = next_month(month);
    let mut days = days_in_month(month) as usize;
    if let Some(until) = until {
        if until >= start && until < end {
            days = until.day() as usize;
        }
    }
    let start_balance = balance_at(conn, scope, start.pred_opt())?;
    let mut net = vec![0i64; days];
    let mut spending = vec![0i64; days];
    let mut stmt = conn.prepare(&format!(
        "SELECT CAST(strftime('%d', t.booked_on) AS INTEGER), sum(t.amount_cents), sum({SPENDING})
         FROM txn t LEFT JOIN category c ON c.id = t.category_id
         WHERE {} AND t.booked_on >= ?1 AND t.booked_on < ?2
         GROUP BY 1",
        scope.accounts_sql("t.account_id")
    ))?;
    let rows = stmt.query_map(rusqlite::params![start, end], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (day, n, s) = row?;
        let i = (day - 1) as usize;
        if i < days {
            net[i] = n;
            spending[i] = s;
        }
    }
    let mut balance = start_balance;
    let balances = net
        .iter()
        .map(|n| {
            balance += n;
            balance
        })
        .collect();
    Ok(Daily {
        balances,
        spending,
        start_balance,
    })
}

/// Spending of one category over a period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategorySpending {
    pub category_id: Id,
    pub name: String,
    /// Chart colour slot (`None`: grouped into "Autres" in charts).
    pub slot: Option<u8>,
    pub amount: i64,
    pub operations: i64,
}

/// Spending per category between `from` and `to` (excluded), largest
/// first. Categories without spending are left out. Operations without a
/// category count in "Autres".
pub fn spending_by_category(
    conn: &Connection,
    scope: &Scope,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Vec<CategorySpending>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT coalesce(c.id, {others}), coalesce(c.name, o.name), CASE WHEN c.id IS NULL THEN o.slot ELSE c.slot END,
                sum({SPENDING}), count(*)
         FROM txn t LEFT JOIN category c ON c.id = t.category_id
         JOIN category o ON o.id = {others}
         WHERE {acc} AND t.booked_on >= ?1 AND t.booked_on < ?2
           AND coalesce(c.is_income, 0) = 0 AND coalesce(c.exclude_from_stats, 0) = 0
         GROUP BY 1 HAVING sum({SPENDING}) <> 0
         ORDER BY 4 DESC, 1",
        others = builtin::OTHERS,
        acc = scope.accounts_sql("t.account_id")
    ))?;
    let rows = stmt.query_map(rusqlite::params![from, to], |r| {
        Ok(CategorySpending {
            category_id: r.get(0)?,
            name: r.get(1)?,
            slot: r.get(2)?,
            amount: r.get(3)?,
            operations: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// A part of a chart: one of the 8 colours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartPart {
    /// 1..=8 (8 = "Autres").
    pub slot: u8,
    pub name: String,
    pub amount: i64,
    pub operations: i64,
    /// Categories grouped in this part ("Autres": its own operations and
    /// the categories without a colour), largest first.
    pub members: Vec<CategorySpending>,
}

/// Groups categories for charts: never more than 8 colours. Categories
/// without a colour join "Autres". Parts are sorted by amount, largest
/// first; tables keep the categories separate.
pub fn chart_parts(categories: &[CategorySpending]) -> Vec<ChartPart> {
    let mut parts: Vec<ChartPart> = Vec::new();
    for c in categories {
        let slot = c.slot.unwrap_or(8);
        let name = if slot == 8 {
            "Autres".to_owned()
        } else {
            c.name.clone()
        };
        match parts.iter_mut().find(|p| p.slot == slot) {
            Some(p) => {
                p.amount += c.amount;
                p.operations += c.operations;
                p.members.push(c.clone());
            }
            None => parts.push(ChartPart {
                slot,
                name,
                amount: c.amount,
                operations: c.operations,
                members: vec![c.clone()],
            }),
        }
    }
    for p in &mut parts {
        p.members.sort_by_key(|m| std::cmp::Reverse(m.amount));
    }
    parts.retain(|p| p.amount > 0);
    parts.sort_by(|a, b| b.amount.cmp(&a.amount).then(a.slot.cmp(&b.slot)));
    parts
}

/// Spending per chart slot (index 0 = slot 1 … index 7 = "Autres") for
/// each month, oldest first.
pub fn monthly_by_slot(
    conn: &Connection,
    scope: &Scope,
    months: &[NaiveDate],
) -> Result<Vec<[i64; 8]>> {
    months
        .iter()
        .map(|m| {
            let mut row = [0i64; 8];
            for c in spending_by_category(conn, scope, first_of_month(*m), next_month(*m))? {
                row[c.slot.unwrap_or(8).clamp(1, 8) as usize - 1] += c.amount;
            }
            Ok(row)
        })
        .collect()
}

/// Spending of each category for each month (table of the cumulative
/// view), categories in their fixed order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryRow {
    pub category_id: Id,
    pub name: String,
    pub slot: Option<u8>,
    /// One amount per month.
    pub months: Vec<i64>,
    pub total: i64,
}

pub fn monthly_by_category(
    conn: &Connection,
    scope: &Scope,
    months: &[NaiveDate],
) -> Result<Vec<CategoryRow>> {
    let mut rows: Vec<CategoryRow> = Vec::new();
    for (i, m) in months.iter().enumerate() {
        for c in spending_by_category(conn, scope, first_of_month(*m), next_month(*m))? {
            let row = match rows.iter_mut().find(|r| r.category_id == c.category_id) {
                Some(r) => r,
                None => {
                    rows.push(CategoryRow {
                        category_id: c.category_id,
                        name: c.name.clone(),
                        slot: c.slot,
                        months: vec![0; months.len()],
                        total: 0,
                    });
                    rows.last_mut().unwrap()
                }
            };
            row.months[i] += c.amount;
            row.total += c.amount;
        }
    }
    // Fixed order: coloured slots first (1..8), then the others by id.
    rows.sort_by_key(|r| (r.slot.unwrap_or(99), r.category_id));
    Ok(rows)
}

/// Filters of the operations list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperationFilter {
    /// Words of the label (any order, accents and case ignored).
    pub search: String,
    pub category_id: Option<Id>,
    pub from: Option<NaiveDate>,
    /// Excluded.
    pub to: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationItem {
    pub id: Id,
    pub booked_on: NaiveDate,
    pub label: String,
    pub account_name: String,
    pub bank_name: Option<String>,
    pub category_id: Option<Id>,
    pub category_name: Option<String>,
    pub category_slot: Option<u8>,
    pub amount_cents: i64,
    pub needs_review: bool,
}

/// Operations matching the filter, most recent first: one page, the
/// number of matches and their total amount.
pub struct OperationPage {
    pub items: Vec<OperationItem>,
    pub count: i64,
    pub total: i64,
}

fn filter_sql(scope: &Scope, filter: &OperationFilter) -> (String, Vec<rusqlite::types::Value>) {
    use rusqlite::types::Value;
    let mut clauses = vec![scope.accounts_sql("t.account_id")];
    let mut params: Vec<Value> = Vec::new();
    for word in crate::normalize::label_norm(&filter.search)
        .split(' ')
        .filter(|w| !w.is_empty())
    {
        clauses.push("(t.label_norm LIKE ? OR upper(coalesce(t.label_raw, '')) LIKE ?)".into());
        params.push(Value::Text(format!("%{word}%")));
        params.push(Value::Text(format!("%{word}%")));
    }
    if let Some(c) = filter.category_id {
        if c == builtin::OTHERS {
            clauses.push("(t.category_id = ? OR t.category_id IS NULL)".into());
        } else {
            clauses.push("t.category_id = ?".into());
        }
        params.push(Value::Integer(c));
    }
    if let Some(from) = filter.from {
        clauses.push("t.booked_on >= ?".into());
        params.push(Value::Text(from.to_string()));
    }
    if let Some(to) = filter.to {
        clauses.push("t.booked_on < ?".into());
        params.push(Value::Text(to.to_string()));
    }
    (clauses.join(" AND "), params)
}

pub fn operations(
    conn: &Connection,
    scope: &Scope,
    filter: &OperationFilter,
    limit: usize,
    offset: usize,
) -> Result<OperationPage> {
    let (clause, params) = filter_sql(scope, filter);
    let (count, total): (i64, i64) = conn.query_row(
        &format!("SELECT count(*), coalesce(sum(t.amount_cents), 0) FROM txn t WHERE {clause}"),
        params_from_iter(params.iter()),
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let mut stmt = conn.prepare(&format!(
        "SELECT t.id, t.booked_on, t.label, a.name, b.name, t.category_id, c.name, c.slot, t.amount_cents,
                t.needs_review
         FROM txn t JOIN account a ON a.id = t.account_id LEFT JOIN bank b ON b.id = a.bank_id
         LEFT JOIN category c ON c.id = t.category_id
         WHERE {clause} ORDER BY t.booked_on DESC, t.id DESC LIMIT {limit} OFFSET {offset}"
    ))?;
    let items = stmt
        .query_map(params_from_iter(params.iter()), |r| {
            Ok(OperationItem {
                id: r.get(0)?,
                booked_on: r.get(1)?,
                label: r.get(2)?,
                account_name: r.get(3)?,
                bank_name: r.get(4)?,
                category_id: r.get(5)?,
                category_name: r.get(6)?,
                category_slot: r.get(7)?,
                amount_cents: r.get(8)?,
                needs_review: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(OperationPage {
        items,
        count,
        total,
    })
}

/// Sets the category of an operation by hand ("À vérifier" is cleared).
pub fn set_category(conn: &Connection, txn_id: Id, category_id: Id) -> Result<()> {
    conn.execute(
        "UPDATE txn SET category_id = ?2, category_source = 'manual', needs_review = 0 WHERE id = ?1",
        rusqlite::params![txn_id, category_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::demo;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn demo_scope() -> (Connection, Scope) {
        let mut conn = open_in_memory().unwrap();
        demo::load(&mut conn).unwrap();
        let scope = scope(&conn, None).unwrap().unwrap();
        (conn, scope)
    }

    #[test]
    fn scope_of_all_accounts_and_of_one() {
        let (conn, s) = demo_scope();
        assert_eq!(s.accounts.len(), 4);
        assert_eq!(s.currency, "EUR");
        assert_eq!(s.excluded_accounts, 0);
        let one = scope(&conn, Some(s.accounts[1])).unwrap().unwrap();
        assert_eq!(one.accounts, vec![s.accounts[1]]);
        assert_eq!(latest_month(&conn, &s).unwrap(), Some(d("2026-10-01")));
        assert_eq!(earliest_month(&conn, &s).unwrap(), Some(d("2026-05-01")));
    }

    #[test]
    fn october_totals_match_the_mockup() {
        let (conn, s) = demo_scope();
        let t = month_totals(&conn, &s, d("2026-10-01")).unwrap();
        assert_eq!(t.spending, 262_490);
        assert_eq!(t.income, 254_100);
        assert_eq!(t.largest_income, Some(("VIREMENT SALAIRE".into(), 241_500)));
        assert_eq!(
            balance_at(&conn, &s, Some(d("2026-10-31"))).unwrap(),
            1_196_858
        );
    }

    #[test]
    fn categories_sum_to_the_month_total() {
        let (conn, s) = demo_scope();
        let cats = spending_by_category(&conn, &s, d("2026-10-01"), d("2026-11-01")).unwrap();
        assert_eq!(cats.iter().map(|c| c.amount).sum::<i64>(), 262_490);
        // Categories without a colour are separate here…
        assert!(cats
            .iter()
            .any(|c| c.name == "Sorties et loisirs" && c.slot.is_none()));
        // …and grouped into "Autres" (slot 8) for charts.
        let parts = chart_parts(&cats);
        assert!(parts.len() <= 8);
        assert_eq!(parts.iter().map(|p| p.amount).sum::<i64>(), 262_490);
        let others = parts.iter().find(|p| p.slot == 8).unwrap();
        assert_eq!(others.amount, 26_318);
        assert!(others.members.len() > 1);
        assert_eq!(parts[0].name, "Logement");
    }

    #[test]
    fn monthly_aggregates_match_the_demo() {
        let (conn, s) = demo_scope();
        let months = months_ending(d("2026-10-15"), 6);
        assert_eq!(months[0], d("2026-05-01"));
        let by_slot = monthly_by_slot(&conn, &s, &months).unwrap();
        for (row, expected) in by_slot.iter().zip(demo::MONTHLY_SPENDING.iter()) {
            assert_eq!(row, expected);
        }
        let table = monthly_by_category(&conn, &s, &months).unwrap();
        assert_eq!(table[0].name, "Logement");
        let total: i64 = table.iter().map(|r| r.total).sum();
        assert_eq!(total, 1_597_376);
    }

    #[test]
    fn day_by_day() {
        let (conn, s) = demo_scope();
        let daily = daily(&conn, &s, d("2026-10-01"), None).unwrap();
        assert_eq!(daily.balances.len(), 31);
        assert_eq!(daily.start_balance, 1_205_248);
        assert_eq!(*daily.balances.last().unwrap(), 1_196_858);
        assert_eq!(daily.spending.iter().sum::<i64>(), 262_490);
        // Rent day.
        assert!(daily.spending[4] >= 69_000);
        let partial = daily_until(&conn, &s);
        assert_eq!(partial, 15);
    }

    fn daily_until(conn: &Connection, s: &Scope) -> usize {
        daily(conn, s, d("2026-10-01"), Some(d("2026-10-15")))
            .unwrap()
            .balances
            .len()
    }

    #[test]
    fn operations_list_with_filters_and_manual_category() {
        let (conn, s) = demo_scope();
        let oct = OperationFilter {
            from: Some(d("2026-10-01")),
            to: Some(d("2026-11-01")),
            ..Default::default()
        };
        let page = operations(&conn, &s, &oct, 10, 0).unwrap();
        assert_eq!(page.items.len(), 10);
        assert_eq!(page.items[0].booked_on, d("2026-10-31"));

        let food = OperationFilter {
            category_id: Some(builtin::FOOD),
            ..oct.clone()
        };
        let page = operations(&conn, &s, &food, 50, 0).unwrap();
        assert_eq!(page.count, 17);
        assert_eq!(page.total, -54_830);

        let search = OperationFilter {
            search: "épicerie bio".into(),
            ..oct.clone()
        };
        let page = operations(&conn, &s, &search, 50, 0).unwrap();
        assert!(page.count >= 1);
        assert!(page.items.iter().all(|i| i.label.contains("ÉPICERIE BIO")));

        let id = page.items[0].id;
        set_category(&conn, id, builtin::LEISURE).unwrap();
        let (source, review): (String, bool) = conn
            .query_row(
                "SELECT category_source, needs_review FROM txn WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((source.as_str(), review), ("manual", false));
    }

    #[test]
    fn fifty_thousand_operations_aggregate_quickly() {
        use crate::models::NewTransaction;
        let mut conn = open_in_memory().unwrap();
        let ids = demo::load(&mut conn).unwrap();
        let tx = conn.transaction().unwrap();
        let rows: Vec<NewTransaction> = (0..50_000)
            .map(|i| {
                let day = d("2022-01-01") + chrono::Days::new(i % 1_700);
                NewTransaction::simple(
                    day,
                    &format!("ACHAT {}", i % 300),
                    -(100 + (i as i64 % 9_000)),
                    1 + (i as i64 % 12),
                )
            })
            .collect();
        crate::repo::insert_transactions(&tx, ids[0], None, &rows).unwrap();
        tx.commit().unwrap();
        let s = scope(&conn, None).unwrap().unwrap();
        let start = std::time::Instant::now();
        let months = months_ending(d("2026-10-01"), 12);
        monthly_by_slot(&conn, &s, &months).unwrap();
        month_totals(&conn, &s, d("2026-10-01")).unwrap();
        daily(&conn, &s, d("2026-10-01"), None).unwrap();
        operations(&conn, &s, &OperationFilter::default(), 50, 0).unwrap();
        let elapsed = start.elapsed();
        // The specification asks for less than a second to open 50 000
        // operations; debug builds are slower, hence the margin.
        assert!(elapsed.as_millis() < 3_000, "{elapsed:?}");
    }
}
