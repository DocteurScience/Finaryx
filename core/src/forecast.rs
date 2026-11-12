//! Forecast from the user's habits (specification §5).
//!
//! Input: the operations of the last `N` complete months of a [`Scope`]
//! (the learning period), an horizon of `H` months. Output, for each
//! future month: expected spending and income, end-of-month balance and
//! the half-width of a probable range.
//!
//! 1. Recurring series: same normalised label and sign, at least 3
//!    occurrences, median interval of about a month, amounts within 15 %
//!    of their median. They are projected on their usual day with their
//!    median amount.
//! 2. Variable spending: per category, mean of the monthly amount without
//!    the recurring series; with 6 months or more, the highest and the
//!    lowest month are left out (an isolated exceptional expense is not
//!    projected).
//! 3. Seasonality (12 months of history or more): factor per calendar
//!    month, spending of that month / monthly mean, bounded to [0.8, 1.3],
//!    applied to the variable spending.
//! 4. Income: recurring series of the income categories; one-off income
//!    is the monthly mean, weighted at 50 %.
//! 5. Balance: current balance plus the expected flows (transfers between
//!    accounts are projected with their monthly mean: about zero for all
//!    the accounts, the savings transfer for one account).
//! 6. Range: root mean square of the monthly errors of the same model
//!    replayed on the learning period; half-width at month `h` =
//!    1.28 · σ · √h (about an 80 % interval).
//!
//! Amounts are minor units; ratios are computed in floating point and the
//! results rounded back to minor units.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use crate::db::Result;
use crate::models::Id;
use crate::stats::{self, first_of_month, next_month, previous_month, Scope};

/// Months of history needed for a forecast.
pub const MIN_MONTHS: usize = 3;
/// Choices of the learning period and of the horizon, in months.
pub const PERIODS: [usize; 3] = [3, 6, 12];
/// Months of history needed to take seasonality into account.
pub const SEASONAL_MONTHS: usize = 12;

/// Recurring series: median interval in days, and accepted gap around it
/// (28 to 32 days, ± 3).
const INTERVAL_MIN: i64 = 28 - 3;
const INTERVAL_MAX: i64 = 32 + 3;
/// A series whose last occurrence is older than this (in days before the
/// end of the learning period) has stopped.
const STALE_DAYS: i64 = 45;
/// The first month counts in the history only if its first operation is
/// on or before this day (an export starting mid-month is incomplete).
const FIRST_MONTH_LAST_START_DAY: u32 = 7;
/// z-score of an 80 % interval.
const Z80: f64 = 1.28;

/// A recurring series detected in the learning period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recurring {
    /// Label of its most recent operation.
    pub label: String,
    pub label_norm: String,
    /// Usual day of the month (median).
    pub day: u32,
    /// Median amount, signed (charges are negative).
    pub amount: i64,
    pub category_id: Option<Id>,
    /// Counted in income (income category) rather than in spending.
    pub income: bool,
    pub occurrences: usize,
}

/// A month of the forecast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastMonth {
    /// First day of the month.
    pub month: NaiveDate,
    /// Expected spending (positive), operations already booked included.
    pub spending: i64,
    /// Expected income, operations already booked included.
    pub income: i64,
    /// Expected balance at the end of the month.
    pub balance: i64,
    /// Half-width of the probable range around `balance`.
    pub half_width: i64,
    /// The month has started: part of it is real.
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Forecast {
    /// Months of the learning period actually used (at most the history).
    pub basis: usize,
    /// End-of-month balances of the learning period, oldest first.
    pub history: Vec<(NaiveDate, i64)>,
    /// Date of the most recent operation: the forecast starts there.
    pub as_of: NaiveDate,
    /// Balance after the most recent operation.
    pub start_balance: i64,
    pub months: Vec<ForecastMonth>,
    /// Recurring series, by usual day.
    pub recurring: Vec<Recurring>,
    /// Seasonality taken into account (12 months of history or more).
    pub seasonal: bool,
    /// Typical monthly error of the model on the learning period.
    pub sigma: i64,
}

impl Forecast {
    /// Recurring charges (spending side), by usual day.
    pub fn charges(&self) -> impl Iterator<Item = &Recurring> {
        self.recurring.iter().filter(|r| !r.income && r.amount < 0)
    }

    /// Total of the recurring charges per month (positive).
    pub fn charges_total(&self) -> i64 {
        -self.charges().map(|r| r.amount).sum::<i64>()
    }

    /// Recurring income series, largest first.
    pub fn regular_income(&self) -> Vec<&Recurring> {
        let mut v: Vec<&Recurring> = self
            .recurring
            .iter()
            .filter(|r| r.income && r.amount > 0)
            .collect();
        v.sort_by_key(|r| std::cmp::Reverse(r.amount));
        v
    }

    /// Mean of the expected spending over the horizon.
    pub fn mean_spending(&self) -> i64 {
        mean(&self.months.iter().map(|m| m.spending).collect::<Vec<_>>())
    }

    /// Month of a clear spending peak (at least 5 % above the mean of the
    /// full months of the horizon), if any.
    pub fn peak(&self) -> Option<&ForecastMonth> {
        let full: Vec<&ForecastMonth> = self.months.iter().filter(|m| !m.partial).collect();
        let avg = mean(&full.iter().map(|m| m.spending).collect::<Vec<_>>());
        let top = full
            .iter()
            .max_by_key(|m| (m.spending, std::cmp::Reverse(m.month)))?;
        (avg > 0 && top.spending * 100 >= avg * 105).then_some(*top)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Fewer than [`MIN_MONTHS`] complete months of history.
    NotEnoughData {
        months: usize,
    },
    Ready(Forecast),
}

/// An operation as the model sees it.
#[derive(Debug, Clone)]
struct Op {
    date: NaiveDate,
    amount: i64,
    label: String,
    label_norm: String,
    category_id: Option<Id>,
    income: bool,
    /// Transfer between the user's accounts ("Virement interne").
    transfer: bool,
}

impl Op {
    fn key(&self) -> (String, bool) {
        (self.label_norm.clone(), self.amount < 0)
    }
}

fn load_ops(conn: &Connection, scope: &Scope, from: NaiveDate, to: NaiveDate) -> Result<Vec<Op>> {
    if scope.accounts.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = scope.accounts.iter().map(|id| id.to_string()).collect();
    let mut stmt = conn.prepare(&format!(
        "SELECT t.booked_on, t.amount_cents, t.label, t.label_norm, t.category_id,
                coalesce(c.is_income, 0), coalesce(c.exclude_from_stats, 0)
         FROM txn t LEFT JOIN category c ON c.id = t.category_id
         WHERE t.account_id IN ({}) AND t.booked_on >= ?1 AND t.booked_on < ?2
         ORDER BY t.booked_on, t.id",
        ids.join(",")
    ))?;
    let rows = stmt.query_map(rusqlite::params![from, to], |r| {
        Ok(Op {
            date: r.get(0)?,
            amount: r.get(1)?,
            label: r.get(2)?,
            label_norm: r.get(3)?,
            category_id: r.get(4)?,
            income: r.get(5)?,
            transfer: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn median(values: &mut [i64]) -> i64 {
    values.sort_unstable();
    let n = values.len();
    if n == 0 {
        0
    } else if n % 2 == 1 {
        values[n / 2]
    } else {
        // Rounded half away from zero, in integers.
        let s = values[n / 2 - 1] + values[n / 2];
        (s + s.signum()) / 2
    }
}

fn mean(values: &[i64]) -> i64 {
    if values.is_empty() {
        return 0;
    }
    let n = values.len() as i64;
    let s: i64 = values.iter().sum();
    (s + s.signum() * n / 2) / n
}

/// Mean without the highest and the lowest value when there are 6 or more.
fn trimmed_mean(values: &[i64]) -> i64 {
    let mut v = values.to_vec();
    v.sort_unstable();
    if v.len() >= 6 {
        v.remove(v.len() - 1);
        v.remove(0);
    }
    mean(&v)
}

/// Index of a month in `months` (first days).
fn month_index(months: &[NaiveDate], d: NaiveDate) -> Option<usize> {
    months.iter().position(|m| *m == first_of_month(d))
}

/// Detects the recurring series among `ops` (transfers left out).
fn detect_recurring(ops: &[Op], period_end: NaiveDate) -> Vec<Recurring> {
    let mut groups: HashMap<(String, bool), Vec<&Op>> = HashMap::new();
    for op in ops.iter().filter(|o| !o.transfer && o.amount != 0) {
        groups.entry(op.key()).or_default().push(op);
    }
    let mut found: Vec<Recurring> = groups
        .into_values()
        .filter_map(|group| {
            if group.len() < 3 {
                return None;
            }
            let last = group.last()?;
            if (period_end - last.date).num_days() > STALE_DAYS {
                return None;
            }
            let mut intervals: Vec<i64> = group
                .windows(2)
                .map(|w| (w[1].date - w[0].date).num_days())
                .collect();
            if !(INTERVAL_MIN..=INTERVAL_MAX).contains(&median(&mut intervals)) {
                return None;
            }
            let mut amounts: Vec<i64> = group.iter().map(|o| o.amount).collect();
            let m = median(&mut amounts);
            if group
                .iter()
                .any(|o| (o.amount - m).abs() * 100 >= m.abs() * 15)
            {
                return None;
            }
            let mut days: Vec<i64> = group.iter().map(|o| o.date.day() as i64).collect();
            Some(Recurring {
                label: last.label.clone(),
                label_norm: last.label_norm.clone(),
                day: median(&mut days).clamp(1, 31) as u32,
                amount: m,
                category_id: last.category_id,
                income: last.income,
                occurrences: group.len(),
            })
        })
        .collect();
    found.sort_by(|a, b| (a.day, a.amount, &a.label_norm).cmp(&(b.day, b.amount, &b.label_norm)));
    found
}

/// The parts of the model learnt on a period.
struct Model {
    recurring: Vec<Recurring>,
    /// Expected variable spending per month (before seasonality).
    variable: i64,
    /// Expected one-off income per month (already weighted).
    one_off_income: i64,
    /// Mean monthly net of the transfers between accounts.
    transfers: i64,
}

impl Model {
    fn recurring_spending(&self) -> i64 {
        -self
            .recurring
            .iter()
            .filter(|r| !r.income)
            .map(|r| r.amount)
            .sum::<i64>()
    }

    fn recurring_income(&self) -> i64 {
        self.recurring
            .iter()
            .filter(|r| r.income)
            .map(|r| r.amount)
            .sum()
    }
}

/// Monthly sums of the parts of `ops` over `months`.
struct Monthly {
    variable: HashMap<Option<Id>, Vec<i64>>,
    one_off_income: Vec<i64>,
    transfers: Vec<i64>,
    /// Net flow of every operation.
    net: Vec<i64>,
    /// Spending as the statistics count it (recurring included).
    spending: Vec<i64>,
}

fn monthly(ops: &[Op], months: &[NaiveDate], recurring: &HashSet<(String, bool)>) -> Monthly {
    let n = months.len();
    let mut m = Monthly {
        variable: HashMap::new(),
        one_off_income: vec![0; n],
        transfers: vec![0; n],
        net: vec![0; n],
        spending: vec![0; n],
    };
    for op in ops {
        let Some(i) = month_index(months, op.date) else {
            continue;
        };
        m.net[i] += op.amount;
        if op.transfer {
            m.transfers[i] += op.amount;
            continue;
        }
        if !op.income {
            m.spending[i] -= op.amount;
        }
        if recurring.contains(&op.key()) {
            continue;
        }
        if op.income {
            m.one_off_income[i] += op.amount;
        } else {
            m.variable
                .entry(op.category_id)
                .or_insert_with(|| vec![0; n])[i] -= op.amount;
        }
    }
    m
}

fn learn(ops: &[Op], months: &[NaiveDate]) -> (Model, Monthly) {
    let period_end = next_month(*months.last().unwrap()).pred_opt().unwrap();
    let recurring = detect_recurring(ops, period_end);
    let keys: HashSet<(String, bool)> = recurring
        .iter()
        .map(|r| (r.label_norm.clone(), r.amount < 0))
        .collect();
    let sums = monthly(ops, months, &keys);
    let model = Model {
        variable: sums.variable.values().map(|v| trimmed_mean(v)).sum(),
        one_off_income: mean(&sums.one_off_income) / 2,
        transfers: mean(&sums.transfers),
        recurring,
    };
    (model, sums)
}

/// Seasonal factor per calendar month (index 0 = January), from the 12
/// months ending with `last`.
fn seasonal_factors(conn: &Connection, scope: &Scope, last: NaiveDate) -> Result<[f64; 12]> {
    let months = stats::months_ending(last, SEASONAL_MONTHS);
    let spending: Vec<i64> = months
        .iter()
        .map(|m| stats::month_totals(conn, scope, *m).map(|t| t.spending))
        .collect::<Result<_>>()?;
    let avg = spending.iter().sum::<i64>() as f64 / spending.len() as f64;
    let mut factors = [1.0; 12];
    if avg > 0.0 {
        for (m, s) in months.iter().zip(&spending) {
            factors[m.month0() as usize] = (*s as f64 / avg).clamp(0.8, 1.3);
        }
    }
    Ok(factors)
}

fn scaled(amount: i64, factor: f64) -> i64 {
    (amount as f64 * factor).round() as i64
}

/// Complete months of history ending with `end` (the first month counts
/// only if it starts early enough).
fn history_months(conn: &Connection, scope: &Scope, end: NaiveDate) -> Result<usize> {
    let first: Option<NaiveDate> = load_ops(
        conn,
        scope,
        NaiveDate::from_ymd_opt(1, 1, 1).unwrap(),
        next_month(end),
    )?
    .first()
    .map(|o| o.date);
    let Some(first) = first else { return Ok(0) };
    let mut start = first_of_month(first);
    if first.day() > FIRST_MONTH_LAST_START_DAY {
        start = next_month(start);
    }
    let months = (end.year() - start.year()) * 12 + end.month() as i32 - start.month() as i32 + 1;
    Ok(months.max(0) as usize)
}

/// Forecast for `scope`, learnt on `basis` months, over `horizon` months.
/// `today` decides whether the month of the last operation is over.
pub fn forecast(
    conn: &Connection,
    scope: &Scope,
    today: NaiveDate,
    basis: usize,
    horizon: usize,
) -> Result<Outcome> {
    let Some(as_of) = stats::last_operation(conn, scope)? else {
        return Ok(Outcome::NotEnoughData { months: 0 });
    };
    let current = first_of_month(as_of);
    let complete = today >= next_month(current);
    let end = if complete {
        current
    } else {
        previous_month(current)
    };
    let available = history_months(conn, scope, end)?;
    if available < MIN_MONTHS {
        return Ok(Outcome::NotEnoughData { months: available });
    }
    let basis = basis
        .clamp(MIN_MONTHS, available.max(MIN_MONTHS))
        .min(available);
    let months = stats::months_ending(end, basis);
    let ops = load_ops(conn, scope, months[0], next_month(end))?;
    let (model, sums) = learn(&ops, &months);

    let seasonal = available >= SEASONAL_MONTHS;
    let factors = if seasonal {
        seasonal_factors(conn, scope, end)?
    } else {
        [1.0; 12]
    };
    let factor = |d: NaiveDate| factors[d.month0() as usize];

    // Replay on the learning period: monthly errors of the net flow.
    let errors: Vec<f64> = months
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let predicted = model.recurring_income()
                - model.recurring_spending()
                - scaled(model.variable, factor(*m))
                + model.one_off_income
                + model.transfers;
            (sums.net[i] - predicted) as f64
        })
        .collect();
    let sigma = (errors.iter().map(|e| e * e).sum::<f64>() / errors.len() as f64).sqrt();

    // Operations already booked in the month under way.
    let keys: HashSet<(String, bool)> = model
        .recurring
        .iter()
        .map(|r| (r.label_norm.clone(), r.amount < 0))
        .collect();
    let booked = if complete {
        Vec::new()
    } else {
        load_ops(conn, scope, current, next_month(current))?
    };
    let seen: HashSet<(String, bool)> = booked.iter().map(Op::key).collect();
    let mut real = (0i64, 0i64, 0i64, 0i64, 0i64); // spending, income, variable, one-off, transfers
    for op in &booked {
        if op.transfer {
            real.4 += op.amount;
            continue;
        }
        if op.income {
            real.1 += op.amount;
        } else {
            real.0 -= op.amount;
        }
        if !keys.contains(&op.key()) {
            if op.income {
                real.3 += op.amount;
            } else {
                real.2 -= op.amount;
            }
        }
    }

    let start_balance = stats::balance_at(conn, scope, Some(as_of))?;
    let mut balance = start_balance;
    let first = if complete {
        next_month(current)
    } else {
        current
    };
    let mut out = Vec::with_capacity(horizon);
    let mut month = first;
    for h in 1..=horizon {
        let partial = !complete && month == current;
        let mut spending = 0i64;
        let mut income = 0i64;
        let mut flow = 0i64;
        for r in &model.recurring {
            if partial && seen.contains(&(r.label_norm.clone(), r.amount < 0)) {
                continue;
            }
            flow += r.amount;
            if r.income {
                income += r.amount;
            } else {
                spending -= r.amount;
            }
        }
        let mut variable = scaled(model.variable, factor(month));
        let mut one_off = model.one_off_income;
        let mut transfers = model.transfers;
        if partial {
            variable = (variable - real.2).max(0);
            one_off = (one_off - real.3).max(0);
            transfers = if transfers < 0 {
                (transfers - real.4).min(0)
            } else {
                (transfers - real.4).max(0)
            };
            spending += real.0;
            income += real.1;
        }
        spending += variable;
        income += one_off;
        flow += one_off - variable + transfers;
        balance += flow;
        out.push(ForecastMonth {
            month,
            spending,
            income,
            balance,
            half_width: (Z80 * sigma * (h as f64).sqrt()).round() as i64,
            partial,
        });
        month = next_month(month);
    }

    let history = months
        .iter()
        .map(|m| {
            let last_day = next_month(*m).pred_opt().unwrap();
            Ok((*m, stats::balance_at(conn, scope, Some(last_day))?))
        })
        .collect::<Result<_>>()?;

    Ok(Outcome::Ready(Forecast {
        basis,
        history,
        as_of,
        start_balance,
        months: out,
        recurring: model.recurring,
        seasonal,
        sigma: sigma.round() as i64,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::models::builtin;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    /// One account; returns its scope.
    fn setup(conn: &Connection, opening: i64) -> Scope {
        conn.execute_batch(&format!(
            "INSERT INTO bank (id, name) VALUES (1, 'Banque A');
             INSERT INTO account (id, bank_id, name, opening_balance_cents)
               VALUES (1, 1, 'Courant', {opening});"
        ))
        .unwrap();
        stats::scope(conn, None).unwrap().unwrap()
    }

    fn add(conn: &Connection, day: NaiveDate, label: &str, cents: i64, category: Id) {
        let norm = crate::normalize::label_norm(label);
        conn.execute(
            "INSERT INTO txn (account_id, booked_on, label, label_norm, amount_cents, category_id, dedupe_key)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?1 || ?2 || ?4 || random())",
            rusqlite::params![day, label, norm, cents, category],
        )
        .unwrap();
    }

    fn month(i: u32) -> NaiveDate {
        // Months from January 2026.
        NaiveDate::from_ymd_opt(2026 + (i / 12) as i32, i % 12 + 1, 1).unwrap()
    }

    /// Salary, rent, subscription and groceries, identical every month.
    fn perfect_month(conn: &Connection, m: NaiveDate, n: u32) {
        let day = |x: u32| m.with_day(x).unwrap();
        add(conn, day(1), "VIREMENT SALAIRE", 200_000, builtin::INCOME);
        add(conn, day(5), "LOYER", -70_000, builtin::HOUSING);
        add(
            conn,
            day(12),
            "OPERATEUR MOBILE",
            -1_500,
            builtin::SUBSCRIPTIONS,
        );
        // Groceries: different shops each month, same total.
        const SHOPS: [&str; 6] = [
            "MARCHE",
            "EPICERIE",
            "PRIMEUR",
            "FROMAGERIE",
            "SUPERETTE",
            "HALLES",
        ];
        let shop = |k: u32| SHOPS[(k % 6) as usize];
        add(conn, day(8), shop(2 * n), -12_000, builtin::FOOD);
        add(conn, day(20), shop(2 * n + 1), -18_000, builtin::FOOD);
    }

    fn ready(o: Outcome) -> Forecast {
        match o {
            Outcome::Ready(f) => f,
            other => panic!("expected a forecast, got {other:?}"),
        }
    }

    #[test]
    fn perfect_monthly_series_is_forecast_exactly() {
        let conn = open_in_memory().unwrap();
        let scope = setup(&conn, 100_000);
        for i in 0..6 {
            perfect_month(&conn, month(i), i);
        }
        let f = ready(forecast(&conn, &scope, d("2026-07-15"), 6, 6).unwrap());
        assert_eq!(f.basis, 6);
        assert_eq!(f.as_of, d("2026-06-20"));
        assert_eq!(f.start_balance, 100_000 + 6 * 98_500);
        assert_eq!(f.sigma, 0);
        assert!(!f.seasonal);
        assert_eq!(f.months.len(), 6);
        assert_eq!(f.months[0].month, d("2026-07-01"));
        for (h, m) in f.months.iter().enumerate() {
            assert_eq!(m.spending, 101_500);
            assert_eq!(m.income, 200_000);
            assert_eq!(m.balance, f.start_balance + (h as i64 + 1) * 98_500);
            assert_eq!(m.half_width, 0);
            assert!(!m.partial);
        }
        // Rent, subscription and salary are recurring; groceries are not.
        let labels: Vec<&str> = f.recurring.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["VIREMENT SALAIRE", "LOYER", "OPERATEUR MOBILE"]);
        assert_eq!(f.recurring[1].day, 5);
        assert_eq!(f.charges_total(), 71_500);
        assert_eq!(f.regular_income()[0].amount, 200_000);
        assert_eq!(f.peak(), None);
        assert_eq!(f.history.len(), 6);
        assert_eq!(f.history[5], (d("2026-06-01"), f.start_balance));
    }

    #[test]
    fn isolated_exceptional_expense_is_not_projected() {
        let conn = open_in_memory().unwrap();
        let scope = setup(&conn, 100_000);
        for i in 0..6 {
            perfect_month(&conn, month(i), i);
        }
        add(
            &conn,
            d("2026-03-14"),
            "TELEVISEUR",
            -150_000,
            builtin::SHOPPING,
        );
        let f = ready(forecast(&conn, &scope, d("2026-07-15"), 6, 3).unwrap());
        assert!(f.recurring.iter().all(|r| r.label != "TELEVISEUR"));
        for m in &f.months {
            assert_eq!(m.spending, 101_500);
        }
        // The range reflects that the past was not that regular.
        assert!(f.sigma > 0);
        assert!(f.months[2].half_width > f.months[0].half_width);
    }

    #[test]
    fn fewer_than_three_months_is_not_enough() {
        let conn = open_in_memory().unwrap();
        let scope = setup(&conn, 0);
        assert_eq!(
            forecast(&conn, &scope, d("2026-07-15"), 6, 6).unwrap(),
            Outcome::NotEnoughData { months: 0 }
        );
        perfect_month(&conn, month(0), 0);
        perfect_month(&conn, month(1), 1);
        assert_eq!(
            forecast(&conn, &scope, d("2026-07-15"), 6, 6).unwrap(),
            Outcome::NotEnoughData { months: 2 }
        );
        // A month under way does not count yet.
        perfect_month(&conn, month(2), 2);
        assert_eq!(
            forecast(&conn, &scope, d("2026-03-25"), 6, 6).unwrap(),
            Outcome::NotEnoughData { months: 2 }
        );
        let f = ready(forecast(&conn, &scope, d("2026-04-02"), 6, 6).unwrap());
        assert_eq!(f.basis, 3);
    }

    #[test]
    fn first_month_starting_late_does_not_count() {
        let conn = open_in_memory().unwrap();
        let scope = setup(&conn, 0);
        add(&conn, d("2026-01-25"), "LOYER", -70_000, builtin::HOUSING);
        for i in 1..3 {
            perfect_month(&conn, month(i), i);
        }
        assert_eq!(
            forecast(&conn, &scope, d("2026-04-02"), 6, 6).unwrap(),
            Outcome::NotEnoughData { months: 2 }
        );
    }

    #[test]
    fn month_under_way_keeps_what_is_booked() {
        let conn = open_in_memory().unwrap();
        let scope = setup(&conn, 100_000);
        for i in 0..6 {
            perfect_month(&conn, month(i), i);
        }
        // July has started: salary, rent and one shop are booked.
        let july = month(6);
        add(
            &conn,
            d("2026-07-01"),
            "VIREMENT SALAIRE",
            200_000,
            builtin::INCOME,
        );
        add(&conn, d("2026-07-05"), "LOYER", -70_000, builtin::HOUSING);
        add(&conn, d("2026-07-08"), "MARCHE", -12_000, builtin::FOOD);
        let f = ready(forecast(&conn, &scope, d("2026-07-10"), 6, 2).unwrap());
        assert_eq!(f.as_of, d("2026-07-08"));
        let m = &f.months[0];
        assert_eq!(m.month, july);
        assert!(m.partial);
        // Same totals as a full month: nothing is counted twice.
        assert_eq!(m.spending, 101_500);
        assert_eq!(m.income, 200_000);
        assert_eq!(m.balance, 100_000 + 7 * 98_500);
        assert_eq!(f.start_balance, 100_000 + 6 * 98_500 + 118_000);
        assert_eq!(f.months[1].balance, m.balance + 98_500);
        // The learning period ends with June.
        assert_eq!(f.history.last().unwrap().0, d("2026-06-01"));
    }

    #[test]
    fn transfers_are_projected_with_their_mean() {
        let conn = open_in_memory().unwrap();
        let scope = setup(&conn, 0);
        for i in 0..4 {
            perfect_month(&conn, month(i), i);
            add(
                &conn,
                month(i).with_day(3).unwrap(),
                "VIR LIVRET A",
                -20_000,
                builtin::INTERNAL_TRANSFER,
            );
        }
        let f = ready(forecast(&conn, &scope, d("2026-05-15"), 6, 2).unwrap());
        assert_eq!(f.basis, 4);
        // Not a recurring charge, not spending, but the balance follows it.
        assert!(f.recurring.iter().all(|r| r.label != "VIR LIVRET A"));
        assert_eq!(f.months[0].spending, 101_500);
        assert_eq!(f.months[0].balance, f.start_balance + 98_500 - 20_000);
        assert_eq!(f.sigma, 0);
    }

    #[test]
    fn seasonality_with_twelve_months() {
        let conn = open_in_memory().unwrap();
        let scope = setup(&conn, 0);
        for i in 0..12 {
            perfect_month(&conn, month(i), i);
        }
        // December 2026: gifts.
        add(
            &conn,
            d("2026-12-15"),
            "CADEAUX",
            -60_000,
            builtin::SHOPPING,
        );
        let f = ready(forecast(&conn, &scope, d("2027-01-10"), 12, 12).unwrap());
        assert!(f.seasonal);
        let dec = f.months.iter().find(|m| m.month.month() == 12).unwrap();
        let jan = f.months.iter().find(|m| m.month.month() == 1).unwrap();
        assert!(dec.spending > jan.spending);
        assert_eq!(f.peak().map(|m| m.month), Some(dec.month));
        // The gifts are not projected as such (trimmed), but December's
        // factor (bounded to 1.3) applies to the 300 € of groceries.
        assert_eq!(dec.spending, 71_500 + 39_000);
    }

    #[test]
    fn demo_forecast() {
        let mut conn = open_in_memory().unwrap();
        crate::demo::load(&mut conn).unwrap();
        let scope = stats::scope(&conn, None).unwrap().unwrap();
        let f = ready(forecast(&conn, &scope, d("2026-11-12"), 6, 6).unwrap());
        assert_eq!(f.basis, 6);
        assert_eq!(f.months[0].month, d("2026-11-01"));
        let charges: Vec<&str> = f.charges().map(|r| r.label.as_str()).collect();
        for label in [
            "LOYER",
            "ECHEANCE PRET IMMOBILIER",
            "OPERATEUR MOBILE",
            "BOX INTERNET",
        ] {
            assert!(charges.contains(&label), "{label} missing in {charges:?}");
        }
        assert_eq!(f.regular_income()[0].label, "VIREMENT SALAIRE");
        // Expected spending stays close to the past months (2 345 to 3 042 €).
        let s = f.mean_spending();
        assert!((230_000..=300_000).contains(&s), "{s}");
        assert!(f.months[5].half_width > f.months[0].half_width);
    }
}
