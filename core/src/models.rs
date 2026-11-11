//! Data model (specification §2). Amounts are integers in the minor unit
//! of the account's currency; expenses are negative.

use chrono::NaiveDate;

pub type Id = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountKind {
    Checking,
    Savings,
}

impl AccountKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AccountKind::Checking => "checking",
            AccountKind::Savings => "savings",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "checking" => Some(AccountKind::Checking),
            "savings" => Some(AccountKind::Savings),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bank {
    pub id: Id,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: Id,
    pub bank_id: Option<Id>,
    pub name: String,
    /// Account number as given by the bank (shown masked: "••7305").
    pub number: Option<String>,
    pub kind: AccountKind,
    /// ISO 4217 code. Each account keeps its own currency; totals are
    /// computed per currency, never converted.
    pub currency: String,
    /// Balance before the first imported operation.
    pub opening_balance_cents: i64,
    /// Colour of the account in charts: 1..=4 (`finaryx_a1`…`finaryx_a4`).
    pub color_slot: u8,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAccount {
    pub bank_id: Option<Id>,
    pub name: String,
    pub number: Option<String>,
    pub kind: AccountKind,
    pub currency: String,
    pub opening_balance_cents: i64,
    pub color_slot: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Category {
    pub id: Id,
    pub name: String,
    /// Chart colour: 1..=7 (`finaryx_c1`…), 8 = "Autres" (grey), `None` =
    /// no colour of its own (grouped into "Autres" in charts only).
    pub slot: Option<u8>,
    pub is_income: bool,
    pub is_builtin: bool,
    /// Neither expense nor income ("Virement interne").
    pub exclude_from_stats: bool,
}

/// Ids of the built-in categories (fixed by the first migration).
pub mod builtin {
    use super::Id;
    pub const HOUSING: Id = 1;
    pub const FOOD: Id = 2;
    pub const TRANSPORT: Id = 3;
    pub const LOANS: Id = 4;
    pub const SHOPPING: Id = 5;
    pub const SUBSCRIPTIONS: Id = 6;
    pub const BANK_INSURANCE: Id = 7;
    pub const OTHERS: Id = 8;
    pub const LEISURE: Id = 9;
    pub const HEALTH: Id = 10;
    pub const TAXES: Id = 11;
    pub const ADMIN_EDUCATION: Id = 12;
    pub const INCOME: Id = 13;
    pub const INTERNAL_TRANSFER: Id = 14;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategorySource {
    Auto,
    Rule,
    Manual,
    Bank,
}

impl CategorySource {
    pub fn as_str(self) -> &'static str {
        match self {
            CategorySource::Auto => "auto",
            CategorySource::Rule => "rule",
            CategorySource::Manual => "manual",
            CategorySource::Bank => "bank",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(CategorySource::Auto),
            "rule" => Some(CategorySource::Rule),
            "manual" => Some(CategorySource::Manual),
            "bank" => Some(CategorySource::Bank),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBatch {
    pub id: Id,
    pub account_id: Id,
    pub filename: Option<String>,
    /// ISO 8601 date and time.
    pub imported_at: String,
    pub rows_read: i64,
    pub rows_new: i64,
    pub rows_duplicate: i64,
    pub rows_review: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub id: Id,
    pub account_id: Id,
    pub batch_id: Option<Id>,
    pub booked_on: NaiveDate,
    pub label: String,
    pub label_raw: Option<String>,
    pub posted_on: Option<NaiveDate>,
    pub value_on: Option<NaiveDate>,
    pub reference: Option<String>,
    pub bank_category: Option<String>,
    pub label_norm: String,
    pub amount_cents: i64,
    pub category_id: Option<Id>,
    pub category_source: CategorySource,
    pub needs_review: bool,
    pub transfer_pair_id: Option<Id>,
    pub dedupe_key: String,
}

/// An operation about to be stored; `label_norm` and `dedupe_key` are
/// computed on insertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTransaction {
    pub booked_on: NaiveDate,
    pub label: String,
    pub label_raw: Option<String>,
    pub posted_on: Option<NaiveDate>,
    pub value_on: Option<NaiveDate>,
    pub reference: Option<String>,
    pub bank_category: Option<String>,
    pub amount_cents: i64,
    pub category_id: Option<Id>,
    pub category_source: CategorySource,
    pub needs_review: bool,
}

impl NewTransaction {
    /// A minimal operation: date, label, amount and category.
    pub fn simple(booked_on: NaiveDate, label: &str, amount_cents: i64, category_id: Id) -> Self {
        NewTransaction {
            booked_on,
            label: label.to_owned(),
            label_raw: None,
            posted_on: None,
            value_on: None,
            reference: None,
            bank_category: None,
            amount_cents,
            category_id: Some(category_id),
            category_source: CategorySource::Auto,
            needs_review: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    /// Detected automatically, waiting for the user's confirmation.
    Proposed,
    Confirmed,
}

impl TransferStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TransferStatus::Proposed => "proposed",
            TransferStatus::Confirmed => "confirmed",
        }
    }
}
