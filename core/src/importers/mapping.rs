//! Column mapping: what each column of the file is used for. Columns are
//! always referred to by their **name**, never by their position.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use super::format::{DateFormat, Encoding};
use crate::i18n::N_;
use crate::normalize::label_norm;

/// What a column of the file is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Field {
    Ignore,
    /// Date de l'opération (required).
    BookedOn,
    /// Date de comptabilisation.
    PostedOn,
    /// Date de valeur.
    ValueOn,
    /// Libellé (required).
    Label,
    /// Détail / libellé brut, kept for searching.
    Detail,
    /// Signed amount.
    Amount,
    /// Debit (read as negative even if written positive).
    Debit,
    /// Credit.
    Credit,
    /// Direction of a positive amount ("D"/"C", "Debit"/"Credit").
    Direction,
    /// Bank reference (helps the duplicate check).
    Reference,
    /// Balance after the operation (control).
    Balance,
    Currency,
    BankCategory,
    BankSubcategory,
}

impl Field {
    /// Fields offered for a column, in menu order.
    pub const ALL: [Field; 15] = [
        Field::Ignore,
        Field::BookedOn,
        Field::PostedOn,
        Field::ValueOn,
        Field::Label,
        Field::Detail,
        Field::Amount,
        Field::Debit,
        Field::Credit,
        Field::Direction,
        Field::Reference,
        Field::Balance,
        Field::Currency,
        Field::BankCategory,
        Field::BankSubcategory,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Ignore => N_("Ignorer"),
            Field::BookedOn => N_("Date de l’opération"),
            Field::PostedOn => N_("Date de comptabilisation"),
            Field::ValueOn => N_("Date de valeur"),
            Field::Label => N_("Libellé"),
            Field::Detail => N_("Détail du libellé"),
            Field::Amount => N_("Montant"),
            Field::Debit => N_("Débit"),
            Field::Credit => N_("Crédit"),
            Field::Direction => N_("Sens (débit ou crédit)"),
            Field::Reference => N_("Référence de la banque"),
            Field::Balance => N_("Solde après l’opération"),
            Field::Currency => N_("Devise"),
            Field::BankCategory => N_("Catégorie de la banque"),
            Field::BankSubcategory => N_("Sous-catégorie de la banque"),
        }
    }
}

/// How the amount is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmountMode {
    /// One signed column.
    Signed,
    /// A debit column and a credit column, one of them filled per row.
    DebitCredit,
    /// A positive amount and a direction column.
    AmountAndDirection,
}

impl AmountMode {
    pub const ALL: [AmountMode; 3] = [
        AmountMode::Signed,
        AmountMode::DebitCredit,
        AmountMode::AmountAndDirection,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AmountMode::Signed => N_("Une colonne"),
            AmountMode::DebitCredit => N_("Débit et crédit"),
            AmountMode::AmountAndDirection => N_("Montant et sens"),
        }
    }
}

/// Where an operation goes, for a category (or category + sub-category)
/// of the bank.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BankCategoryChoice {
    /// Name of the Finaryx category.
    pub category: String,
    /// Operations get the "À vérifier" flag.
    pub needs_review: bool,
}

pub const MAPPING_VERSION: u32 = 1;

/// Everything needed to read a file of a given format. Stored as JSON in
/// `import_profile.mapping_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
    pub version: u32,
    pub encoding: Encoding,
    pub separator: char,
    /// Number of lines (records) before the header.
    pub skip_records: usize,
    pub date_format: DateFormat,
    pub decimal: char,
    pub amount_mode: AmountMode,
    /// Column name → field.
    pub columns: BTreeMap<String, Field>,
    /// Values of the direction column meaning a debit / a credit
    /// (compared case- and accent-insensitively).
    pub debit_values: Vec<String>,
    pub credit_values: Vec<String>,
    /// Bank category key (see [`bank_category_key`]) → Finaryx category.
    pub bank_categories: BTreeMap<String, BankCategoryChoice>,
}

impl Mapping {
    pub fn column_for(&self, field: Field) -> Option<&str> {
        self.columns
            .iter()
            .find(|(_, f)| **f == field)
            .map(|(c, _)| c.as_str())
    }

    /// Fields that must be mapped for the current amount mode but are not,
    /// as names for the user.
    pub fn missing_fields(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.column_for(Field::BookedOn).is_none() {
            missing.push(Field::BookedOn.label());
        }
        if self.column_for(Field::Label).is_none() {
            missing.push(Field::Label.label());
        }
        match self.amount_mode {
            AmountMode::Signed => {
                if self.column_for(Field::Amount).is_none() {
                    missing.push(Field::Amount.label());
                }
            }
            AmountMode::DebitCredit => {
                if self.column_for(Field::Debit).is_none() {
                    missing.push(Field::Debit.label());
                }
                if self.column_for(Field::Credit).is_none() {
                    missing.push(Field::Credit.label());
                }
            }
            AmountMode::AmountAndDirection => {
                if self.column_for(Field::Amount).is_none() {
                    missing.push(Field::Amount.label());
                }
                if self.column_for(Field::Direction).is_none() {
                    missing.push(Field::Direction.label());
                }
            }
        }
        missing
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a mapping always serialises")
    }

    pub fn from_json(json: &str) -> Option<Mapping> {
        serde_json::from_str::<Mapping>(json)
            .ok()
            .filter(|m| m.version <= MAPPING_VERSION)
    }
}

/// Column name normalised for comparisons: lower case, no accents, single
/// spaces.
pub fn normalize_header(name: &str) -> String {
    label_norm(name).to_lowercase()
}

/// Signature of a format: hash of the normalised column names, order
/// ignored.
pub fn signature(headers: &[String]) -> String {
    let mut names: Vec<String> = headers.iter().map(|h| normalize_header(h)).collect();
    names.sort();
    let mut hasher = Sha1::new();
    hasher.update(names.join("\n").as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Key of a bank category in [`Mapping::bank_categories`]:
/// "Catégorie" or "Catégorie / Sous-catégorie".
pub fn bank_category_key(category: &str, subcategory: Option<&str>) -> String {
    match subcategory.filter(|s| !s.trim().is_empty()) {
        Some(sub) => format!("{} / {}", category.trim(), sub.trim()),
        None => category.trim().to_owned(),
    }
}

/// Proposes a field for each column from its name. Only a proposal: the
/// user confirms. Returns the columns and the amount mode.
pub fn suggest(headers: &[String]) -> (BTreeMap<String, Field>, AmountMode) {
    let mut columns = BTreeMap::new();
    let mut taken: Vec<Field> = Vec::new();
    for header in headers {
        let h = normalize_header(header);
        let has = |w: &str| h.split(' ').any(|t| t.starts_with(w));
        let candidate = if has("date") || h == "started date" || h == "completed date" {
            if has("valeur") || h.contains("value") {
                Field::ValueOn
            } else if has("compta") || h.contains("completed") || h.contains("posted") {
                Field::PostedOn
            } else {
                Field::BookedOn
            }
        } else if has("sous") || h.contains("subcategor") {
            Field::BankSubcategory
        } else if has("categ") {
            Field::BankCategory
        } else if has("debit") {
            Field::Debit
        } else if has("credit") {
            Field::Credit
        } else if has("montant") || has("amount") {
            Field::Amount
        } else if has("sens") || h == "type mouvement" {
            Field::Direction
        } else if has("solde") || has("balance") {
            Field::Balance
        } else if has("devise") || has("currency") {
            Field::Currency
        } else if has("ref") {
            Field::Reference
        } else if has("libel") || has("label") || has("description") || has("intitule") {
            if taken.contains(&Field::Label) {
                Field::Detail
            } else {
                Field::Label
            }
        } else {
            Field::Ignore
        };
        // One column per field (the first one wins), except "Ignorer".
        let field = if candidate != Field::Ignore && taken.contains(&candidate) {
            if candidate == Field::BookedOn && !taken.contains(&Field::PostedOn) {
                Field::PostedOn
            } else {
                Field::Ignore
            }
        } else {
            candidate
        };
        if field != Field::Ignore {
            taken.push(field);
        }
        columns.insert(header.clone(), field);
    }
    let mode = if taken.contains(&Field::Debit) && taken.contains(&Field::Credit) {
        AmountMode::DebitCredit
    } else if taken.contains(&Field::Direction) {
        AmountMode::AmountAndDirection
    } else {
        AmountMode::Signed
    };
    (columns, mode)
}

/// Parses an amount as written in the file, without floating point.
/// The decimal separator is given; the other one, spaces and apostrophes
/// group thousands. Currency symbols and codes are ignored.
pub fn parse_file_amount(text: &str, decimal: char) -> Option<i64> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let negative = t.starts_with('-')
        || t.starts_with('\u{2212}')
        || (t.starts_with('(') && t.ends_with(')'))
        || t.ends_with('-');
    let mut int_part = String::new();
    let mut frac_part = String::new();
    let mut in_frac = false;
    for c in t.chars() {
        if c.is_ascii_digit() {
            if in_frac {
                frac_part.push(c);
            } else {
                int_part.push(c);
            }
        } else if c == decimal {
            if in_frac {
                return None;
            }
            in_frac = true;
        } else if matches!(
            c,
            ',' | '.' | ' ' | '\'' | '\u{a0}' | '\u{202f}' | '-' | '+' | '\u{2212}' | '(' | ')'
        ) || c.is_alphabetic()
            || c == '€'
            || c == '$'
            || c == '£'
        {
            continue;
        } else {
            return None;
        }
    }
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if frac_part.len() > 2 {
        return None;
    }
    while frac_part.len() < 2 {
        frac_part.push('0');
    }
    let whole: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    let cents: i64 = frac_part.parse().ok()?;
    let value = whole.checked_mul(100)?.checked_add(cents)?;
    Some(if negative { -value } else { value })
}

/// A data row read with a mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRow {
    pub booked_on: NaiveDate,
    pub posted_on: Option<NaiveDate>,
    pub value_on: Option<NaiveDate>,
    pub label: String,
    pub detail: Option<String>,
    pub amount_cents: i64,
    pub reference: Option<String>,
    pub balance_cents: Option<i64>,
    pub currency: Option<String>,
    pub bank_category: Option<String>,
    pub bank_subcategory: Option<String>,
}

/// Why a row cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RowProblem {
    UnreadableDate,
    MissingLabel,
    UnreadableAmount,
    UnknownDirection,
    /// The currency column names another currency than the account's.
    OtherCurrency,
}

impl RowProblem {
    /// "date illisible"
    pub fn label(self) -> &'static str {
        match self {
            RowProblem::UnreadableDate => N_("une date illisible"),
            RowProblem::MissingLabel => N_("un libellé vide"),
            RowProblem::UnreadableAmount => N_("un montant illisible"),
            RowProblem::UnknownDirection => N_("un sens (débit ou crédit) inconnu"),
            RowProblem::OtherCurrency => N_("une autre devise que celle du compte"),
        }
    }
}

fn same_word(a: &str, b: &str) -> bool {
    let (na, nb) = (normalize_header(a), normalize_header(b));
    if na.is_empty() || nb.is_empty() {
        // Symbols such as "+" and "-" are compared as written.
        a.trim() == b.trim()
    } else {
        na == nb
    }
}

/// Reads one data row. `headers` are the column names of the file.
pub fn parse_row(
    mapping: &Mapping,
    headers: &[String],
    fields: &[String],
) -> Result<ParsedRow, RowProblem> {
    let get = |field: Field| -> Option<&str> {
        let column = mapping.column_for(field)?;
        let index = headers.iter().position(|h| h == column)?;
        fields
            .get(index)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    };
    let date = |field: Field| get(field).and_then(|s| mapping.date_format.parse(s));

    let booked_on = date(Field::BookedOn).ok_or(RowProblem::UnreadableDate)?;
    let label = get(Field::Label)
        .ok_or(RowProblem::MissingLabel)?
        .to_owned();
    let amount = |field: Field| get(field).map(|s| parse_file_amount(s, mapping.decimal));
    let amount_cents = match mapping.amount_mode {
        AmountMode::Signed => amount(Field::Amount)
            .flatten()
            .ok_or(RowProblem::UnreadableAmount)?,
        AmountMode::DebitCredit => match (amount(Field::Debit), amount(Field::Credit)) {
            (Some(Some(d)), None) => -d.abs(),
            (None, Some(Some(c))) => c,
            // Both filled: the non-zero one.
            (Some(Some(d)), Some(Some(0))) => -d.abs(),
            (Some(Some(0)), Some(Some(c))) => c,
            _ => return Err(RowProblem::UnreadableAmount),
        },
        AmountMode::AmountAndDirection => {
            let value = amount(Field::Amount)
                .flatten()
                .ok_or(RowProblem::UnreadableAmount)?
                .abs();
            let direction = get(Field::Direction).ok_or(RowProblem::UnknownDirection)?;
            if mapping.debit_values.iter().any(|v| same_word(v, direction)) {
                -value
            } else if mapping
                .credit_values
                .iter()
                .any(|v| same_word(v, direction))
            {
                value
            } else {
                return Err(RowProblem::UnknownDirection);
            }
        }
    };
    Ok(ParsedRow {
        booked_on,
        posted_on: date(Field::PostedOn),
        value_on: date(Field::ValueOn),
        label,
        detail: get(Field::Detail).map(str::to_owned),
        amount_cents,
        reference: get(Field::Reference).map(str::to_owned),
        balance_cents: get(Field::Balance).and_then(|s| parse_file_amount(s, mapping.decimal)),
        currency: get(Field::Currency).map(str::to_owned),
        bank_category: get(Field::BankCategory).map(str::to_owned),
        bank_subcategory: get(Field::BankSubcategory).map(str::to_owned),
    })
}

/// Default values of a direction column.
pub fn default_direction_values() -> (Vec<String>, Vec<String>) {
    (
        ["D", "Debit", "Débit", "DBIT", "-"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        ["C", "Credit", "Crédit", "CRDT", "+"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn signature_ignores_order_case_and_accents() {
        let a = signature(&h(&["Date", "Libellé", "Montant"]));
        assert_eq!(a, signature(&h(&["montant", " LIBELLE", "date "])));
        assert_ne!(a, signature(&h(&["Date", "Libellé", "Débit"])));
    }

    #[test]
    fn suggestions_from_column_names() {
        let headers = h(&[
            "Date de comptabilisation",
            "Libelle simplifie",
            "Libelle operation",
            "Reference",
            "Informations complementaires",
            "Type operation",
            "Categorie",
            "Sous categorie",
            "Debit",
            "Credit",
            "Date operation",
            "Date de valeur",
            "Pointage operation",
        ]);
        let (cols, mode) = suggest(&headers);
        assert_eq!(mode, AmountMode::DebitCredit);
        assert_eq!(cols["Date de comptabilisation"], Field::PostedOn);
        assert_eq!(cols["Date operation"], Field::BookedOn);
        assert_eq!(cols["Date de valeur"], Field::ValueOn);
        assert_eq!(cols["Libelle simplifie"], Field::Label);
        assert_eq!(cols["Libelle operation"], Field::Detail);
        assert_eq!(cols["Categorie"], Field::BankCategory);
        assert_eq!(cols["Sous categorie"], Field::BankSubcategory);
        assert_eq!(cols["Informations complementaires"], Field::Ignore);
        assert_eq!(cols["Pointage operation"], Field::Ignore);

        let (cols, mode) = suggest(&h(&["Date", "Description", "Amount", "Balance"]));
        assert_eq!(mode, AmountMode::Signed);
        assert_eq!(cols["Date"], Field::BookedOn);
        assert_eq!(cols["Amount"], Field::Amount);
        assert_eq!(cols["Balance"], Field::Balance);

        let (_, mode) = suggest(&h(&["Date", "Libellé", "Montant", "Sens"]));
        assert_eq!(mode, AmountMode::AmountAndDirection);
    }

    #[test]
    fn amounts_in_files() {
        assert_eq!(parse_file_amount("-12,50", ','), Some(-1_250));
        assert_eq!(parse_file_amount("1 234,5", ','), Some(123_450));
        assert_eq!(parse_file_amount("1.234,56 €", ','), Some(123_456));
        assert_eq!(parse_file_amount("1,234.56", '.'), Some(123_456));
        assert_eq!(parse_file_amount("+3", ','), Some(300));
        assert_eq!(parse_file_amount("(42.10)", '.'), Some(-4_210));
        assert_eq!(parse_file_amount("12,345", ','), None);
        assert_eq!(parse_file_amount("", ','), None);
        assert_eq!(parse_file_amount("n/a", ','), None);
    }

    fn mapping(mode: AmountMode, columns: &[(&str, Field)]) -> Mapping {
        let (debit_values, credit_values) = default_direction_values();
        Mapping {
            version: MAPPING_VERSION,
            encoding: Encoding::Utf8,
            separator: ';',
            skip_records: 0,
            date_format: DateFormat::DayMonthYear,
            decimal: ',',
            amount_mode: mode,
            columns: columns.iter().map(|(c, f)| (c.to_string(), *f)).collect(),
            debit_values,
            credit_values,
            bank_categories: BTreeMap::new(),
        }
    }

    #[test]
    fn rows_in_the_three_amount_modes() {
        let headers = h(&["Date", "Libellé", "Débit", "Crédit"]);
        let m = mapping(
            AmountMode::DebitCredit,
            &[
                ("Date", Field::BookedOn),
                ("Libellé", Field::Label),
                ("Débit", Field::Debit),
                ("Crédit", Field::Credit),
            ],
        );
        let row = |f: &[&str]| parse_row(&m, &headers, &h(f));
        assert_eq!(
            row(&["05/10/2026", "LOYER", "690,00", ""])
                .unwrap()
                .amount_cents,
            -69_000
        );
        assert_eq!(
            row(&["05/10/2026", "LOYER", "-690,00", ""])
                .unwrap()
                .amount_cents,
            -69_000
        );
        assert_eq!(
            row(&["01/10/2026", "SALAIRE", "", "2415,00"])
                .unwrap()
                .amount_cents,
            241_500
        );
        assert_eq!(
            row(&["32/10/2026", "X", "1", ""]),
            Err(RowProblem::UnreadableDate)
        );
        assert_eq!(
            row(&["01/10/2026", "", "1", ""]),
            Err(RowProblem::MissingLabel)
        );
        assert_eq!(
            row(&["01/10/2026", "X", "", ""]),
            Err(RowProblem::UnreadableAmount)
        );

        let headers = h(&["Date", "Libellé", "Montant", "Sens"]);
        let m = mapping(
            AmountMode::AmountAndDirection,
            &[
                ("Date", Field::BookedOn),
                ("Libellé", Field::Label),
                ("Montant", Field::Amount),
                ("Sens", Field::Direction),
            ],
        );
        let row = |f: &[&str]| parse_row(&m, &headers, &h(f));
        assert_eq!(
            row(&["05/10/2026", "LOYER", "690,00", "débit"])
                .unwrap()
                .amount_cents,
            -69_000
        );
        assert_eq!(
            row(&["05/10/2026", "SALAIRE", "2415", "C"])
                .unwrap()
                .amount_cents,
            241_500
        );
        assert_eq!(
            row(&["05/10/2026", "X", "1", "?"]),
            Err(RowProblem::UnknownDirection)
        );

        let mut missing = mapping(AmountMode::Signed, &[("Date", Field::BookedOn)]);
        assert_eq!(missing.missing_fields(), vec!["Libellé", "Montant"]);
        missing.amount_mode = AmountMode::DebitCredit;
        assert_eq!(missing.missing_fields(), vec!["Libellé", "Débit", "Crédit"]);
    }

    #[test]
    fn mapping_round_trips_through_json() {
        let m = mapping(AmountMode::Signed, &[("Date", Field::BookedOn)]);
        assert_eq!(Mapping::from_json(&m.to_json()), Some(m));
        assert_eq!(Mapping::from_json("{}"), None);
    }
}
