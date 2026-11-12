//! Import tests on invented files (`tests/fixtures/`, see the README
//! there). No real bank export is ever committed.

use finaryx_core::db::open_in_memory;
use finaryx_core::importers::format::{DateFormat, Encoding};
use finaryx_core::importers::mapping::{AmountMode, BankCategoryChoice, Field, RowProblem};
use finaryx_core::importers::plan::{self, CommitOptions, RowState};
use finaryx_core::importers::profiles;
use finaryx_core::models::{builtin, AccountKind, CategorySource, NewAccount};
use finaryx_core::repo;
use finaryx_core::rusqlite::Connection;

const BP_OCTOBRE: &[u8] = include_bytes!("fixtures/bp_octobre.csv");
const BP_NOVEMBRE: &[u8] = include_bytes!("fixtures/bp_novembre.csv");
const GENERIC: &[u8] = include_bytes!("fixtures/generic_us.csv");

fn setup() -> (Connection, i64) {
    let conn = open_in_memory().unwrap();
    let bank = repo::bank_id_or_insert(&conn, "Banque Populaire").unwrap();
    let account = repo::insert_account(
        &conn,
        &NewAccount {
            bank_id: Some(bank),
            name: "Compte courant".into(),
            number: None,
            kind: AccountKind::Checking,
            currency: "EUR".into(),
            opening_balance_cents: 100_000,
            color_slot: 1,
        },
    )
    .unwrap();
    (conn, account)
}

fn import(conn: &mut Connection, account: i64, bytes: &[u8], name: &str) -> plan::ImportSummary {
    let analysis = plan::analyze(conn, bytes).unwrap();
    let p = plan::plan(conn, &analysis.table, &analysis.mapping, account).unwrap();
    plan::commit(
        conn,
        account,
        name,
        &analysis.table,
        &analysis.mapping,
        &p,
        &CommitOptions::default(),
        "2026-11-11T10:00:00",
    )
    .unwrap()
}

#[test]
fn banque_populaire_is_recognised_and_read() {
    let (conn, account) = setup();
    let a = plan::analyze(&conn, BP_OCTOBRE).unwrap();
    assert_eq!(
        a.profile.as_ref().map(|p| p.name.as_str()),
        Some("Banque Populaire")
    );
    assert_eq!(a.mapping.encoding, Encoding::Latin1);
    assert_eq!(a.mapping.separator, ';');
    assert_eq!(a.mapping.amount_mode, AmountMode::DebitCredit);
    assert_eq!(a.table.headers.len(), 13);
    assert_eq!(a.table.rows.len(), 12);
    assert!(plan::validate(&a.table, &a.mapping).is_empty());

    let p = plan::plan(&conn, &a.table, &a.mapping, account).unwrap();
    assert_eq!(p.read(), 12);
    assert!(p.unreadable.is_empty());
    assert_eq!(p.count(RowState::Duplicate), 0);

    let first = &p.rows[0].txn;
    assert_eq!(first.label, "BOULANGERIE DES LILAS");
    assert_eq!(
        first.label_raw.as_deref(),
        Some("CB BOULANGERIE DES LILAS 31/10")
    );
    assert_eq!(first.booked_on.to_string(), "2026-10-31"); // "Date operation"
    assert_eq!(first.posted_on.unwrap().to_string(), "2026-11-02");
    assert_eq!(first.amount_cents, -420);
    assert_eq!(first.category_id, Some(builtin::FOOD));
    assert_eq!(first.category_source, CategorySource::Bank);

    let by_label = |l: &str| p.rows.iter().find(|r| r.txn.label == l).unwrap();
    assert_eq!(
        by_label("CAFÉ DES ARTS").txn.category_id,
        Some(builtin::LEISURE)
    );
    assert_eq!(by_label("SALAIRE").txn.amount_cents, 241_500);
    assert_eq!(by_label("SALAIRE").txn.category_id, Some(builtin::INCOME));
    assert_eq!(
        by_label("VIR EPARGNE").txn.category_id,
        Some(builtin::INTERNAL_TRANSFER)
    );
    // "Banque et assurance - autre" is flagged in the table.
    assert_eq!(by_label("ECHEANCE PRET").state, RowState::Review);
    assert_eq!(by_label("RETRAIT DAB").state, RowState::Review);
    // An unknown bank category falls back to keywords, then "Autres".
    assert_eq!(
        by_label("BOUTIQUE NOUVELLE").txn.category_id,
        Some(builtin::OTHERS)
    );
    assert_eq!(by_label("BOUTIQUE NOUVELLE").state, RowState::Review);
    assert_eq!(p.count(RowState::Review), 3);

    // The bank categories found, most frequent first.
    let cats = repo::list_categories(&conn).unwrap();
    let uses = plan::bank_category_uses(&a.table, &a.mapping, &cats);
    assert_eq!(uses[0].key, "Transaction exclue / Virement interne");
    assert_eq!(uses[0].count, 3);
    let unknown = uses.iter().find(|u| u.category == "Nouveautes").unwrap();
    assert!(!unknown.known);
    assert!(unknown.choice.needs_review);
}

#[test]
fn reimporting_the_same_file_creates_nothing() {
    let (mut conn, account) = setup();
    let first = import(&mut conn, account, BP_OCTOBRE, "bp_octobre.csv");
    // Two identical purchases the same day stay two operations; serial
    // transfers sharing a reference stay three.
    assert_eq!(first.inserted, 12);
    assert_eq!(repo::transaction_count(&conn, Some(account)).unwrap(), 12);

    let a = plan::analyze(&conn, BP_OCTOBRE).unwrap();
    let p = plan::plan(&conn, &a.table, &a.mapping, account).unwrap();
    assert_eq!(p.count(RowState::Duplicate), 12);
    assert_eq!(p.to_import(), 0);
    let again = import(&mut conn, account, BP_OCTOBRE, "bp_octobre.csv");
    assert_eq!((again.inserted, again.duplicates), (0, 12));
    assert_eq!(repo::transaction_count(&conn, Some(account)).unwrap(), 12);

    // An overlapping file only adds what is new.
    let next = import(&mut conn, account, BP_NOVEMBRE, "bp_novembre.csv");
    assert_eq!((next.inserted, next.duplicates), (1, 3));
    assert_eq!(repo::transaction_count(&conn, Some(account)).unwrap(), 13);
}

#[test]
fn an_import_is_atomic() {
    let (mut conn, account) = setup();
    let a = plan::analyze(&conn, BP_OCTOBRE).unwrap();
    let p = plan::plan(&conn, &a.table, &a.mapping, account).unwrap();
    // Saving the format under an empty name fails at the end of the
    // transaction: nothing at all must remain.
    let options = CommitOptions {
        save_profile: Some(("  ".into(), None)),
        new_opening: None,
    };
    assert!(plan::commit(
        &mut conn,
        account,
        "bp.csv",
        &a.table,
        &a.mapping,
        &p,
        &options,
        "2026-11-11T10:00:00"
    )
    .is_err());
    assert_eq!(repo::transaction_count(&conn, None).unwrap(), 0);
    assert!(repo::last_batch(&conn).unwrap().is_none());
}

#[test]
fn unknown_format_with_metadata_lines_and_a_balance_column() {
    let (mut conn, account) = setup();
    let a = plan::analyze(&conn, GENERIC).unwrap();
    assert!(a.profile.is_none());
    assert_eq!(a.mapping.separator, ',');
    assert_eq!(a.mapping.skip_records, 2);
    assert_eq!(
        a.table.headers,
        ["Date", "Description", "Amount", "Balance"]
    );
    assert_eq!(a.mapping.decimal, '.');
    // 10/13/2026 proves the month comes first.
    assert_eq!(a.mapping.date_format, DateFormat::MonthDayYear);
    assert!(!a.date_ambiguous);
    assert_eq!(a.mapping.amount_mode, AmountMode::Signed);
    assert_eq!(a.mapping.columns["Description"], Field::Label);
    assert_eq!(a.mapping.columns["Balance"], Field::Balance);

    let p = plan::plan(&conn, &a.table, &a.mapping, account).unwrap();
    assert_eq!(p.rows.len(), 4);
    assert_eq!(p.rows[2].txn.label, "CAFE, CROISSANT");
    assert!(p.balance.inconsistent_lines.is_empty());
    // The file says 1 000,00 € before its first operation; so does the
    // account (opening balance 1 000,00 €): nothing to adjust.
    assert_eq!(p.balance.suggested_opening, None);

    // Remember the format: the next file is recognised.
    let options = CommitOptions {
        save_profile: Some(("Ma banque".into(), Some("Banque X".into()))),
        new_opening: None,
    };
    plan::commit(
        &mut conn,
        account,
        "generic_us.csv",
        &a.table,
        &a.mapping,
        &p,
        &options,
        "2026-11-11T10:00:00",
    )
    .unwrap();
    let again = plan::analyze(&conn, GENERIC).unwrap();
    assert_eq!(again.profile.unwrap().name, "Ma banque");
    assert_eq!(profiles::list_profiles(&conn).unwrap().len(), 1);
}

#[test]
fn balance_column_suggests_a_new_opening_balance() {
    let (mut conn, account) = setup();
    repo::set_opening_balance(&conn, account, 0).unwrap();
    let a = plan::analyze(&conn, GENERIC).unwrap();
    let p = plan::plan(&conn, &a.table, &a.mapping, account).unwrap();
    assert_eq!(p.balance.suggested_opening, Some(100_000));
    let options = CommitOptions {
        save_profile: None,
        new_opening: p.balance.suggested_opening,
    };
    plan::commit(
        &mut conn,
        account,
        "generic_us.csv",
        &a.table,
        &a.mapping,
        &p,
        &options,
        "2026-11-11T10:00:00",
    )
    .unwrap();
    assert_eq!(
        repo::account_balance(&conn, account, None).unwrap(),
        267_930
    );

    // A broken balance column is reported line by line.
    let broken = String::from_utf8(GENERIC.to_vec())
        .unwrap()
        .replace("2679.30", "2600.00");
    let a = plan::analyze(&conn, broken.as_bytes()).unwrap();
    let p = plan::plan(&conn, &a.table, &a.mapping, account).unwrap();
    assert_eq!(p.balance.inconsistent_lines, vec![7]);
}

#[test]
fn unreadable_rows_are_reported_not_imported() {
    let (conn, account) = setup();
    let text =
        "Date;Libellé;Montant;Sens;Devise\n05/10/2026;LOYER;690,00;D;EUR\n06/10/2026;X;abc;D;EUR\n\
                32/10/2026;Y;1,00;C;EUR\n08/10/2026;Z;2,00;?;EUR\n09/10/2026;W;3,00;D;USD\n";
    let a = plan::analyze(&conn, text.as_bytes()).unwrap();
    assert_eq!(a.mapping.amount_mode, AmountMode::AmountAndDirection);
    assert_eq!(a.mapping.columns["Devise"], Field::Currency);
    let messages = plan::validate(&a.table, &a.mapping);
    assert!(
        messages.contains(&"1 ligne a un montant illisible\u{a0}: ligne 3".to_string()),
        "{messages:?}"
    );
    let p = plan::plan(&conn, &a.table, &a.mapping, account).unwrap();
    assert_eq!(p.rows.len(), 1);
    assert_eq!(p.rows[0].txn.amount_cents, -69_000);
    assert_eq!(p.unreadable[&RowProblem::UnreadableAmount], vec![3]);
    assert_eq!(p.unreadable[&RowProblem::UnreadableDate], vec![4]);
    assert_eq!(p.unreadable[&RowProblem::UnknownDirection], vec![5]);
    assert_eq!(p.unreadable[&RowProblem::OtherCurrency], vec![6]);
    assert_eq!(p.read(), 5);
}

#[test]
fn bank_category_choices_are_applied() {
    let (conn, account) = setup();
    let mut a = plan::analyze(&conn, BP_OCTOBRE).unwrap();
    a.mapping.bank_categories.insert(
        "Nouveautes / Inconnue".into(),
        BankCategoryChoice {
            category: "Shopping et animaux".into(),
            needs_review: false,
        },
    );
    let p = plan::plan(&conn, &a.table, &a.mapping, account).unwrap();
    let row = p
        .rows
        .iter()
        .find(|r| r.txn.label == "BOUTIQUE NOUVELLE")
        .unwrap();
    assert_eq!(row.txn.category_id, Some(builtin::SHOPPING));
    assert_eq!(row.state, RowState::New);
}
