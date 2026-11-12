//! From a file to an import: analysis (format, profile, proposed
//! mapping), preview of every row (Nouveau / Doublon / À vérifier), then
//! the import itself, in one SQL transaction.

use std::collections::{BTreeMap, HashSet};

use chrono::NaiveDate;
use rusqlite::Connection;

use super::bank_categories;
use super::format::{self, DateFormat, Encoding};
use super::mapping::{
    self, bank_category_key, default_direction_values, parse_row, BankCategoryChoice, Field,
    Mapping, ParsedRow, RowProblem, MAPPING_VERSION,
};
use super::profiles::{self, Profile};
use crate::categorize::Categorizer;
use crate::db::{Error, Result};
use crate::models::{Category, Id, NewTransaction};
use crate::repo;

/// The file once split into columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub headers: Vec<String>,
    /// Data rows: (line in the file, fields), padded to the header width.
    pub rows: Vec<(usize, Vec<String>)>,
}

impl Table {
    /// Values of a column (first `limit` non-empty ones).
    pub fn samples(&self, column: &str, limit: usize) -> Vec<&str> {
        let Some(i) = self.headers.iter().position(|h| h == column) else {
            return Vec::new();
        };
        self.rows
            .iter()
            .filter_map(|(_, r)| r.get(i))
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .take(limit)
            .collect()
    }

    /// First non-empty value of a column, for the "Exemple : …" text.
    pub fn example(&self, column: &str) -> Option<&str> {
        self.samples(column, 1).first().copied()
    }
}

/// Splits the file with the given format (encoding, separator, lines to
/// skip before the header).
pub fn read_table(bytes: &[u8], mapping: &Mapping) -> Table {
    let text = format::decode(bytes, mapping.encoding);
    let mut records = format::split(&text, mapping.separator)
        .into_iter()
        .skip(mapping.skip_records);
    let headers = records.next().map(|(_, h)| h).unwrap_or_default();
    let width = headers.len();
    let rows = records
        .map(|(line, mut fields)| {
            fields.resize(width.max(fields.len()), String::new());
            (line, fields)
        })
        .collect();
    Table { headers, rows }
}

/// What the importer understood of a file.
#[derive(Debug, Clone)]
pub struct Analysis {
    /// Format and mapping (from a profile, or proposed).
    pub mapping: Mapping,
    pub table: Table,
    /// Recognised profile, if any ("Modèle reconnu : …").
    pub profile: Option<Profile>,
    /// The dates could be JJ/MM or MM/JJ: the user must choose.
    pub date_ambiguous: bool,
}

/// Detects the format, looks for a profile and proposes a mapping.
pub fn analyze(conn: &Connection, bytes: &[u8]) -> Result<Analysis> {
    let encoding = format::detect_encoding(bytes);
    let text = format::decode(bytes, encoding);
    let separator = format::detect_separator(&text);
    let records = format::split(&text, separator);
    let skip_records = format::detect_header(&records);
    let (debit_values, credit_values) = default_direction_values();
    let mut mapping = Mapping {
        version: MAPPING_VERSION,
        encoding,
        separator,
        skip_records,
        date_format: DateFormat::DayMonthYear,
        decimal: ',',
        amount_mode: mapping::AmountMode::Signed,
        columns: BTreeMap::new(),
        debit_values,
        credit_values,
        bank_categories: BTreeMap::new(),
    };
    let table = read_table(bytes, &mapping);
    let profile = profiles::find_profile(conn, &table.headers)?;
    let mut date_ambiguous = false;
    match &profile {
        Some(p) => {
            // Encoding, separator and header line come from the file
            // itself; the rest from the profile.
            mapping = Mapping {
                encoding,
                separator,
                skip_records,
                ..p.mapping.clone()
            };
        }
        None => {
            let (columns, mode) = mapping::suggest(&table.headers);
            mapping.columns = columns;
            mapping.amount_mode = mode;
            let amount_samples: Vec<&str> =
                [Field::Amount, Field::Debit, Field::Credit, Field::Balance]
                    .iter()
                    .filter_map(|f| mapping.column_for(*f))
                    .flat_map(|c| table.samples(c, 50))
                    .collect();
            mapping.decimal = format::detect_decimal(&amount_samples);
            if let Some(column) = mapping.column_for(Field::BookedOn) {
                if let Some(det) = format::detect_date_format(&table.samples(column, 200)) {
                    mapping.date_format = det.format;
                    date_ambiguous = det.ambiguous;
                }
            }
        }
    }
    Ok(Analysis {
        mapping,
        table,
        profile,
        date_ambiguous,
    })
}

/// A category (or category + sub-category) of the bank found in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankCategoryUse {
    pub key: String,
    pub category: String,
    pub subcategory: Option<String>,
    pub count: usize,
    pub choice: BankCategoryChoice,
    /// The choice comes from the profile (already known), not from a
    /// proposal.
    pub known: bool,
}

/// Bank categories of the file, most frequent first, with the current
/// choice for each.
pub fn bank_category_uses(
    table: &Table,
    mapping: &Mapping,
    categories: &[Category],
) -> Vec<BankCategoryUse> {
    let Some(cat_col) = mapping.column_for(Field::BankCategory) else {
        return Vec::new();
    };
    let cat_i = table.headers.iter().position(|h| h == cat_col);
    let sub_i = mapping
        .column_for(Field::BankSubcategory)
        .and_then(|c| table.headers.iter().position(|h| h == c));
    let mut uses: Vec<BankCategoryUse> = Vec::new();
    for (_, row) in &table.rows {
        let Some(category) = cat_i.and_then(|i| row.get(i)).filter(|c| !c.is_empty()) else {
            continue;
        };
        let subcategory = sub_i.and_then(|i| row.get(i)).filter(|s| !s.is_empty());
        let key = bank_category_key(category, subcategory.map(String::as_str));
        if let Some(u) = uses.iter_mut().find(|u| u.key == key) {
            u.count += 1;
            continue;
        }
        let known = mapping.bank_categories.get(&key).cloned();
        uses.push(BankCategoryUse {
            choice: known.clone().unwrap_or_else(|| {
                bank_categories::propose(category, subcategory.map(String::as_str), categories)
            }),
            known: known.is_some(),
            key,
            category: category.clone(),
            subcategory: subcategory.cloned(),
            count: 1,
        });
    }
    uses.sort_by(|a, b| b.count.cmp(&a.count).then(a.key.cmp(&b.key)));
    uses
}

/// Checks that the mapping can read the file: required fields mapped, and
/// the first 20 rows readable. Returns messages for the user.
pub fn validate(table: &Table, mapping: &Mapping) -> Vec<String> {
    let missing = mapping.missing_fields();
    if !missing.is_empty() {
        return vec![format!("Associez une colonne à : {}.", missing.join(", "))];
    }
    let mut problems: BTreeMap<RowProblem, Vec<usize>> = BTreeMap::new();
    for (line, row) in table.rows.iter().take(20) {
        if let Err(p) = parse_row(mapping, &table.headers, row) {
            problems.entry(p).or_default().push(*line);
        }
    }
    problems
        .iter()
        .map(|(p, lines)| problem_message(*p, lines))
        .collect()
}

/// "3 lignes ont une date illisible : ligne 12, 40, 41"
pub fn problem_message(problem: RowProblem, lines: &[usize]) -> String {
    let shown: Vec<String> = lines.iter().take(10).map(|l| l.to_string()).collect();
    let more = if lines.len() > 10 { " …" } else { "" };
    if lines.len() == 1 {
        format!("1 ligne a {} : ligne {}", problem.label(), shown[0])
    } else {
        format!(
            "{} lignes ont {} : ligne {}{more}",
            lines.len(),
            problem.label(),
            shown.join(", ")
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    New,
    Duplicate,
    /// New, but the category is uncertain.
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRow {
    pub line: usize,
    pub state: RowState,
    pub txn: NewTransaction,
}

/// Balance column check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BalanceCheck {
    /// Lines where balance ≠ previous balance + amount.
    pub inconsistent_lines: Vec<usize>,
    /// Opening balance that makes the account match the file, when it
    /// differs from the current one.
    pub suggested_opening: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// Readable rows in file order.
    pub rows: Vec<PlannedRow>,
    /// Unreadable rows, by problem (not imported, always reported).
    pub unreadable: BTreeMap<RowProblem, Vec<usize>>,
    pub balance: BalanceCheck,
}

impl Plan {
    pub fn read(&self) -> usize {
        self.rows.len() + self.unreadable.values().map(Vec::len).sum::<usize>()
    }
    pub fn count(&self, state: RowState) -> usize {
        self.rows.iter().filter(|r| r.state == state).count()
    }
    /// Operations that the import will add.
    pub fn to_import(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| r.state != RowState::Duplicate)
            .count()
    }
    pub fn unreadable_messages(&self) -> Vec<String> {
        self.unreadable
            .iter()
            .map(|(p, lines)| problem_message(*p, lines))
            .collect()
    }
}

fn to_transaction(row: &ParsedRow, categorizer: &Categorizer, mapping: &Mapping) -> NewTransaction {
    let bank_key = row
        .bank_category
        .as_deref()
        .map(|c| bank_category_key(c, row.bank_subcategory.as_deref()));
    let choice = bank_key
        .as_ref()
        .and_then(|k| mapping.bank_categories.get(k));
    let decision = categorizer.categorize(&crate::normalize::label_norm(&row.label), choice);
    NewTransaction {
        booked_on: row.booked_on,
        label: row.label.clone(),
        label_raw: row.detail.clone(),
        posted_on: row.posted_on,
        value_on: row.value_on,
        reference: row.reference.clone(),
        bank_category: bank_key,
        amount_cents: row.amount_cents,
        category_id: Some(decision.category_id),
        category_source: decision.source,
        needs_review: decision.needs_review,
    }
}

/// Reads every row for `account_id` and classifies it.
pub fn plan(conn: &Connection, table: &Table, mapping: &Mapping, account_id: Id) -> Result<Plan> {
    let categorizer = Categorizer::load(conn)?;
    let currency: String = conn.query_row(
        "SELECT currency FROM account WHERE id = ?1",
        [account_id],
        |r| r.get(0),
    )?;
    let mut plan = Plan::default();
    let mut parsed: Vec<(usize, ParsedRow)> = Vec::new();
    for (line, fields) in &table.rows {
        match parse_row(mapping, &table.headers, fields) {
            Ok(row) => parsed.push((*line, row)),
            Err(p) => plan.unreadable.entry(p).or_default().push(*line),
        }
    }
    // Rows in another currency do not belong to this account.
    parsed.retain(|(line, row)| match &row.currency {
        Some(c) if !c.eq_ignore_ascii_case(&currency) => {
            plan.unreadable
                .entry(RowProblem::OtherCurrency)
                .or_default()
                .push(*line);
            false
        }
        _ => true,
    });

    let txns: Vec<NewTransaction> = parsed
        .iter()
        .map(|(_, r)| to_transaction(r, &categorizer, mapping))
        .collect();
    for ((line, _), (txn, (_, key))) in parsed.iter().zip(txns.iter().zip(repo::dedupe_keys(&txns)))
    {
        let state = if repo::has_dedupe_key(conn, account_id, &key)? {
            RowState::Duplicate
        } else if txn.needs_review {
            RowState::Review
        } else {
            RowState::New
        };
        plan.rows.push(PlannedRow {
            line: *line,
            state,
            txn: txn.clone(),
        });
    }
    plan.balance = check_balance(conn, account_id, &parsed)?;
    Ok(plan)
}

/// Chronological walk of the balance column.
fn check_balance(
    conn: &Connection,
    account_id: Id,
    parsed: &[(usize, ParsedRow)],
) -> Result<BalanceCheck> {
    let with_balance: Vec<&(usize, ParsedRow)> = parsed
        .iter()
        .filter(|(_, r)| r.balance_cents.is_some())
        .collect();
    if with_balance.len() != parsed.len() || parsed.is_empty() {
        return Ok(BalanceCheck::default());
    }
    // Files are sorted one way or the other; same-day rows keep the file's
    // relative order.
    let mut rows: Vec<&(usize, ParsedRow)> = with_balance;
    let descending = rows.first().unwrap().1.booked_on > rows.last().unwrap().1.booked_on;
    if descending {
        rows.reverse();
    }
    let mut check = BalanceCheck::default();
    for pair in rows.windows(2) {
        let (prev, cur) = (&pair[0].1, &pair[1].1);
        if prev.balance_cents.unwrap() + cur.amount_cents != cur.balance_cents.unwrap() {
            check.inconsistent_lines.push(pair[1].0);
        }
    }
    let first = &rows[0].1;
    let before_file = first.balance_cents.unwrap() - first.amount_cents;
    let day_before = first.booked_on.pred_opt().unwrap_or(first.booked_on);
    let current = repo::account_balance(conn, account_id, Some(day_before))?;
    if current != before_file {
        let opening: i64 = conn.query_row(
            "SELECT opening_balance_cents FROM account WHERE id = ?1",
            [account_id],
            |r| r.get(0),
        )?;
        check.suggested_opening = Some(opening + before_file - current);
    }
    Ok(check)
}

/// What to do besides inserting the operations.
#[derive(Debug, Clone, Default)]
pub struct CommitOptions {
    /// "Retenir ce format": profile name and bank.
    pub save_profile: Option<(String, Option<String>)>,
    /// New opening balance, after the user's confirmation.
    pub new_opening: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportSummary {
    pub batch_id: Id,
    pub inserted: usize,
    pub duplicates: usize,
    pub review: usize,
}

/// Imports the plan in a single transaction: either everything is
/// written, or nothing is.
#[allow(clippy::too_many_arguments)]
pub fn commit(
    conn: &mut Connection,
    account_id: Id,
    filename: &str,
    table: &Table,
    mapping: &Mapping,
    plan: &Plan,
    options: &CommitOptions,
    now: &str,
) -> Result<ImportSummary> {
    let tx = conn.transaction()?;
    let batch_id = repo::insert_batch(&tx, account_id, Some(filename), now)?;
    // All readable rows go through the insertion: those already present
    // are skipped by the unique key (a file may have changed meanwhile).
    let txns: Vec<NewTransaction> = plan.rows.iter().map(|r| r.txn.clone()).collect();
    let outcome = repo::insert_transactions(&tx, account_id, Some(batch_id), &txns)?;
    tx.execute(
        "UPDATE import_batch SET rows_read = ?2, rows_new = ?3, rows_duplicate = ?4, rows_review = ?5
         WHERE id = ?1",
        rusqlite::params![
            batch_id,
            plan.read() as i64,
            outcome.inserted.len() as i64,
            outcome.duplicates as i64,
            outcome.review as i64
        ],
    )?;
    if let Some((name, bank)) = &options.save_profile {
        if name.trim().is_empty() {
            return Err(Error::Invalid("the format name is empty".into()));
        }
        profiles::save_profile(&tx, name, bank.as_deref(), &table.headers, mapping)?;
    }
    if let Some(opening) = options.new_opening {
        repo::set_opening_balance(&tx, account_id, opening)?;
    }
    tx.commit()?;
    Ok(ImportSummary {
        batch_id,
        inserted: outcome.inserted.len(),
        duplicates: outcome.duplicates,
        review: outcome.review,
    })
}

/// Dates covered by a plan (first, last).
pub fn period(plan: &Plan) -> Option<(NaiveDate, NaiveDate)> {
    let dates: HashSet<NaiveDate> = plan.rows.iter().map(|r| r.txn.booked_on).collect();
    Some((*dates.iter().min()?, *dates.iter().max()?))
}

/// Encodings offered to the user.
pub fn encodings() -> [Encoding; 3] {
    Encoding::ALL
}
