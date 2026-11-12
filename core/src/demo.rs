//! Demonstration data built from the example figures of the mockup
//! (`design/`): 4 accounts, 3 banks, May to October 2026.
//!
//! Everything here is fictional: the mockup figures were replaced by an
//! invented data set. The figures reproduced exactly are:
//! - monthly spending per chart category, May–October (`Main`/`Cumul`),
//!   hence 15 973,76 € over six months and 2 624,90 € in October;
//! - October income 2 541,00 € (salary 2 415,00 € on the 1st, 57,00 € on
//!   the 11th, 69,00 € on the 23rd);
//! - total balance 12 052,48 € on 30 September and 11 968,58 € on
//!   31 October; end-of-month totals of June–October from the forecast
//!   chart (12 353,23, 12 112,31, 11 865,66, 12 052,48, 11 968,58 €);
//! - balance of each account on 31 October (2 871,46 €, 6 530,00 €,
//!   1 954,27 €, 612,85 €) and the Livret A month-end balances;
//! - the October operations listed in `Mois`, `Operations` and
//!   `CompteDetail` (labels, dates, accounts, amounts).
//!
//! The mockup pages are not consistent with each other on some points, so
//! these are only approximated: the day-by-day spending curve of October,
//! the month-end balances of the other accounts, the number of operations
//! per account, and the import dates of the joint and Revolut accounts.

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use crate::categories::CHART_GROUPS;
use crate::db::Result;
use crate::models::builtin::*;
use crate::models::*;
use crate::repo;

const YEAR: i32 = 2026;

/// "Today" in the demonstration: the day of its last import, so that the
/// views (import status, forecast) do not depend on the clock.
pub fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(YEAR, 11, 2).unwrap()
}
/// May to October.
const MONTHS: [u32; 6] = [5, 6, 7, 8, 9, 10];

/// Spending per chart category and month, in cents (mockup `Main.dc.html`,
/// `DATA`): Logement, Alimentation, Transports, Prêts et crédits, Shopping
/// et animaux, Abonnements et télécoms, Banque et assurances, Autres.
pub const MONTHLY_SPENDING: [[i64; 8]; 6] = [
    [73_500, 50_210, 18_840, 39_215, 15_630, 8_497, 8_850, 19_762],
    [
        73_500, 53_180, 23_690, 39_215, 20_470, 8_497, 12_140, 30_115,
    ],
    [73_500, 48_960, 34_220, 39_215, 26_890, 8_497, 5_480, 44_830],
    [73_500, 45_530, 37_160, 39_215, 35_940, 8_497, 7_720, 56_603],
    [
        73_500, 52_740, 19_310, 39_215, 22_180, 8_497, 18_630, 19_746,
    ],
    [
        73_500, 54_830, 21_140, 39_215, 28_760, 8_497, 10_230, 26_318,
    ],
];

/// A row of [`MONTHLY_SPENDING`] laid out as chart groups: the 7 coloured
/// categories in slots 1 to 7, colours 8 to 12 unused, then "Autres".
pub fn chart_row(row: &[i64; 8]) -> [i64; CHART_GROUPS] {
    let mut out = [0; CHART_GROUPS];
    out[..7].copy_from_slice(&row[..7]);
    out[CHART_GROUPS - 1] = row[7];
    out
}

/// Income besides the salary, per month: (day, label, cents).
const EXTRA_INCOME: [&[(u32, &str, i64)]; 6] = [
    &[(15, "PRIME TRIMESTRIELLE", 25_000)],
    &[(16, "REMBOURSEMENT MUTUELLE", 9_500)],
    &[(18, "VENTE EN LIGNE", 16_000)],
    &[(14, "PRIME DE VACANCES", 38_000)],
    &[(17, "REMBOURSEMENT IMPOTS", 31_000)],
    &[
        (11, "REMBOURSEMENT SANTE", 5_700),
        (23, "VENTE EN LIGNE", 6_900),
    ],
];
const SALARY: i64 = 241_500;

/// Monthly transfer from the current account to the Livret A.
const SAVINGS_TRANSFER: [i64; 6] = [15_000, 15_000, 15_000, 11_000, 11_000, 11_000];
const JOINT_TRANSFER: i64 = 62_000;
const REVOLUT_TRANSFER: i64 = 21_000;

/// Account balances on 31 October 2026 (mockup `Comptes.dc.html`).
pub const END_BALANCES: [i64; 4] = [287_146, 653_000, 195_427, 61_285];

const COURANT: usize = 0;
const LIVRET: usize = 1;
const JOINT: usize = 2;
const REVOLUT: usize = 3;

struct DemoAccount {
    bank: &'static str,
    name: &'static str,
    number: &'static str,
    kind: AccountKind,
    filename: &'static str,
    imported_at: &'static str,
}

const ACCOUNTS: [DemoAccount; 4] = [
    DemoAccount {
        bank: "Banque Populaire",
        name: "Compte courant",
        number: "00000007305",
        kind: AccountKind::Checking,
        filename: "00000007305_01052026_31102026.csv",
        imported_at: "2026-11-02T18:42:00",
    },
    DemoAccount {
        bank: "Banque Populaire",
        name: "Livret A",
        number: "00000005162",
        kind: AccountKind::Savings,
        filename: "livret-a_2026.csv",
        imported_at: "2026-10-28T09:15:00",
    },
    DemoAccount {
        bank: "Boursorama",
        name: "Compte joint",
        number: "00000003948",
        kind: AccountKind::Checking,
        filename: "joint_octobre-2026.csv",
        imported_at: "2026-11-01T11:05:00",
    },
    DemoAccount {
        bank: "Revolut",
        name: "Revolut",
        number: "00000008810",
        kind: AccountKind::Checking,
        filename: "revolut_2026-10.csv",
        imported_at: "2026-11-01T11:20:00",
    },
];

/// October operations shown in the mockup: (day, label, account,
/// category, cents spent, "À vérifier").
const OCTOBER_LISTED: &[(u32, &str, usize, Id, i64, bool)] = &[
    (31, "BOULANGERIE DES LILAS", COURANT, FOOD, 1_840, false),
    (30, "LIBRAIRIE DU PASSAGE", JOINT, SHOPPING, 6_490, false),
    (
        29,
        "CLINIQUE VÉTO DES PINS",
        REVOLUT,
        SHOPPING,
        7_100,
        false,
    ),
    (27, "STATION DU PONT", COURANT, TRANSPORT, 6_845, false),
    (26, "SUPERMARCHÉ CENTRE", JOINT, FOOD, 4_125, false),
    (25, "HYPERMARCHÉ NORD", JOINT, FOOD, 18_670, false),
    (21, "ÉPICERIE BIO", COURANT, FOOD, 5_280, false),
    (17, "MARCHÉ DU SAMEDI", REVOLUT, FOOD, 2_850, true),
    (14, "SURGELÉS EXPRESS", COURANT, FOOD, 4_715, false),
    (12, "DISCOUNT ALIMENTAIRE", JOINT, FOOD, 5_830, false),
    (9, "SUPERMARCHÉ CENTRE", JOINT, FOOD, 2_640, false),
    (6, "BOULANGERIE DES LILAS", COURANT, FOOD, 1_280, false),
];

/// Pools of labels for the variable spending: (label, account).
const FOOD_POOL: &[(&str, usize)] = &[
    ("HYPERMARCHÉ NORD", JOINT),
    ("BOULANGERIE DES LILAS", COURANT),
    ("SUPERMARCHÉ CENTRE", JOINT),
    ("SURGELÉS EXPRESS", COURANT),
    ("DISCOUNT ALIMENTAIRE", JOINT),
    ("SUPÉRETTE DU QUARTIER", REVOLUT),
    ("ÉPICERIE BIO", COURANT),
];
const TRANSPORT_POOL: &[(&str, usize)] = &[
    ("STATION DU PONT", COURANT),
    ("TRAIN RÉGIONAL", COURANT),
    ("PARKING CENTRE", COURANT),
    ("VTC", REVOLUT),
];
const SHOPPING_POOL: &[(&str, usize)] = &[
    ("BOUTIQUE EN LIGNE", JOINT),
    ("ANIMALERIE", REVOLUT),
    ("MAGASIN DE SPORT", JOINT),
    ("LIBRAIRIE DU PASSAGE", JOINT),
];
const LEISURE_POOL: &[(&str, usize)] = &[
    ("CINÉMA DU CENTRE", REVOLUT),
    ("RESTAURANT LE PETIT JARDIN", REVOLUT),
    ("CAFÉ DES ARTS", REVOLUT),
];
const HEALTH_POOL: &[(&str, usize)] = &[("PHARMACIE DE LA GARE", COURANT)];
const TAXES_POOL: &[(&str, usize)] = &[("IMPOTS TAXE", COURANT)];
const ADMIN_POOL: &[(&str, usize)] = &[("BUREAU DE POSTE", COURANT)];
const OTHERS_POOL: &[(&str, usize)] = &[("RETRAIT DAB", COURANT)];

/// One generated operation, before insertion.
struct Planned {
    account: usize,
    txn: NewTransaction,
    /// Both sides of a transfer share the same key.
    transfer: Option<u32>,
}

/// Small deterministic generator (no randomness: the demo is identical on
/// every run).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// Weight in 60..=140.
    fn weight(&mut self) -> i64 {
        60 + (self.next() % 81) as i64
    }
}

/// Splits `total` cents into `n` positive parts with uneven weights.
fn split(total: i64, n: usize, rng: &mut Lcg) -> Vec<i64> {
    assert!(
        n > 0 && total >= n as i64,
        "cannot split {total} into {n} parts"
    );
    let weights: Vec<i64> = (0..n).map(|_| rng.weight()).collect();
    let sum: i64 = weights.iter().sum();
    let mut parts: Vec<i64> = weights.iter().map(|w| (total * w / sum).max(1)).collect();
    let mut diff = total - parts.iter().sum::<i64>();
    let mut i = 0;
    while diff != 0 {
        let step = diff.signum();
        if parts[i % n] + step >= 1 {
            parts[i % n] += step;
            diff -= step;
        }
        i += 1;
    }
    parts
}

fn days_in_month(month: u32) -> u32 {
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(YEAR + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(YEAR, month + 1, 1)
    };
    next.unwrap().pred_opt().unwrap().day()
}

fn date(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(YEAR, month, day.min(days_in_month(month))).unwrap()
}

struct Planner {
    planned: Vec<Planned>,
    rng: Lcg,
    next_transfer: u32,
}

impl Planner {
    fn spend(
        &mut self,
        account: usize,
        month: u32,
        day: u32,
        label: &str,
        cents: i64,
        category: Id,
    ) {
        self.planned.push(Planned {
            account,
            txn: NewTransaction::simple(date(month, day), label, -cents, category),
            transfer: None,
        });
    }

    fn earn(&mut self, account: usize, month: u32, day: u32, label: &str, cents: i64) {
        self.planned.push(Planned {
            account,
            txn: NewTransaction::simple(date(month, day), label, cents, INCOME),
            transfer: None,
        });
    }

    fn transfer(&mut self, from: usize, to: usize, month: u32, day: u32, cents: i64) {
        let key = self.next_transfer;
        self.next_transfer += 1;
        let out_label = match to {
            LIVRET => "VIR LIVRET A",
            JOINT => "VIR COMPTE JOINT",
            _ => "VIR REVOLUT",
        };
        self.planned.push(Planned {
            account: from,
            txn: NewTransaction::simple(date(month, day), out_label, -cents, INTERNAL_TRANSFER),
            transfer: Some(key),
        });
        self.planned.push(Planned {
            account: to,
            txn: NewTransaction::simple(
                date(month, day),
                "VIREMENT COMPTE COURANT",
                cents,
                INTERNAL_TRANSFER,
            ),
            transfer: Some(key),
        });
    }

    /// Spreads `total` over `n` operations taken from `pool`, on days
    /// spread across the month.
    fn spread(&mut self, month: u32, total: i64, n: usize, pool: &[(&str, usize)], category: Id) {
        if total <= 0 {
            return;
        }
        let n = n.min(total as usize).max(1);
        let parts = split(total, n, &mut self.rng);
        let dim = days_in_month(month);
        let offset = (self.rng.next() % 3) as u32;
        for (i, cents) in parts.into_iter().enumerate() {
            let (label, account) = pool[(i + month as usize) % pool.len()];
            let day = 1 + ((i as u32 * dim) / n as u32 + offset) % dim;
            let review = category == OTHERS;
            self.spend(account, month, day, label, cents, category);
            self.planned.last_mut().unwrap().txn.needs_review = review;
        }
    }
}

fn plan() -> Vec<Planned> {
    let mut p = Planner {
        planned: Vec::new(),
        rng: Lcg(0x5EED_F1AA),
        next_transfer: 0,
    };

    for (m, &month) in MONTHS.iter().enumerate() {
        let [housing, food, transport, loans, shopping, subscriptions, bank, others] =
            MONTHLY_SPENDING[m];
        let october = month == 10;

        // Income.
        p.earn(COURANT, month, 1, "VIREMENT SALAIRE", SALARY);
        for &(day, label, cents) in EXTRA_INCOME[m] {
            p.earn(COURANT, month, day, label, cents);
        }

        // Transfers between the user's own accounts.
        p.transfer(COURANT, JOINT, month, 2, JOINT_TRANSFER);
        p.transfer(COURANT, REVOLUT, month, 2, REVOLUT_TRANSFER);
        p.transfer(COURANT, LIVRET, month, 3, SAVINGS_TRANSFER[m]);

        // Fixed charges.
        p.spend(COURANT, month, 5, "LOYER", 69_000, HOUSING);
        p.spend(
            COURANT,
            month,
            12,
            "FOURNISSEUR ELECTRICITE",
            housing - 69_000,
            HOUSING,
        );
        p.spend(COURANT, month, 10, "ECHEANCE PRET IMMOBILIER", loans, LOANS);
        let fixed_subscriptions = [
            (3, "OPERATEUR MOBILE", 1_299),
            (8, "BOX INTERNET", 3_499),
            (15, "SERVICE MUSIQUE", 999),
            (24, "SERVICE DE STREAMING", 1_199),
        ];
        for (day, label, cents) in fixed_subscriptions {
            p.spend(COURANT, month, day, label, cents, SUBSCRIPTIONS);
        }
        let rest: i64 = fixed_subscriptions.iter().map(|s| s.2).sum();
        p.spend(
            COURANT,
            month,
            18,
            "STOCKAGE EN LIGNE",
            subscriptions - rest,
            SUBSCRIPTIONS,
        );
        p.spend(
            COURANT,
            month,
            28,
            "ASSURANCE HABITAT PLUS",
            3_360,
            BANK_INSURANCE,
        );
        let bank_rest = bank - 3_360;
        let bank_label = if bank_rest > 10_000 {
            "ASSURANCE AUTO"
        } else {
            "FRAIS BANCAIRES"
        };
        p.spend(COURANT, month, 6, bank_label, bank_rest, BANK_INSURANCE);

        // Operations listed in the mockup (October), then variable spending.
        let mut listed = [0i64; 15];
        if october {
            for &(day, label, account, category, cents, review) in OCTOBER_LISTED {
                p.spend(account, month, day, label, cents, category);
                p.planned.last_mut().unwrap().txn.needs_review = review;
                listed[category as usize] += cents;
            }
        }
        let food_count = if october { 8 } else { 16 };
        p.spread(
            month,
            food - listed[FOOD as usize],
            food_count,
            FOOD_POOL,
            FOOD,
        );
        p.spread(
            month,
            transport - listed[TRANSPORT as usize],
            4,
            TRANSPORT_POOL,
            TRANSPORT,
        );
        p.spread(
            month,
            shopping - listed[SHOPPING as usize],
            3,
            SHOPPING_POOL,
            SHOPPING,
        );

        // "Autres" in the charts groups the categories without a colour.
        let leisure = others * 45 / 100;
        let health = others * 20 / 100;
        let taxes = others * 15 / 100;
        let admin = others * 5 / 100;
        let rest = others - leisure - health - taxes - admin;
        p.spread(month, leisure, 3, LEISURE_POOL, LEISURE);
        p.spread(month, health, 1, HEALTH_POOL, HEALTH);
        p.spread(month, taxes, 1, TAXES_POOL, TAXES);
        p.spread(month, admin, 1, ADMIN_POOL, ADMIN_EDUCATION);
        p.spread(month, rest, 1, OTHERS_POOL, OTHERS);
    }
    p.planned
}

/// Fills an empty database with the demonstration data, atomically.
/// Returns the ids of the four accounts.
pub fn load(conn: &mut Connection) -> Result<[Id; 4]> {
    let tx = conn.transaction()?;
    let planned = plan();

    // Opening balance = balance on 31 October − sum of the operations.
    let mut sums = [0i64; 4];
    for p in &planned {
        sums[p.account] += p.txn.amount_cents;
    }

    let mut ids = [0; 4];
    for (i, a) in ACCOUNTS.iter().enumerate() {
        let bank_id = repo::bank_id_or_insert(&tx, a.bank)?;
        ids[i] = repo::insert_account(
            &tx,
            &NewAccount {
                bank_id: Some(bank_id),
                name: a.name.into(),
                number: Some(a.number.into()),
                kind: a.kind,
                currency: "EUR".into(),
                opening_balance_cents: END_BALANCES[i] - sums[i],
                color_slot: i as u8 + 1,
            },
        )?;
    }

    // One import batch per account; remember the id of each planned row.
    let mut row_ids = vec![0; planned.len()];
    for (i, a) in ACCOUNTS.iter().enumerate() {
        let indices: Vec<usize> = (0..planned.len())
            .filter(|&k| planned[k].account == i)
            .collect();
        let mut rows: Vec<NewTransaction> =
            indices.iter().map(|&k| planned[k].txn.clone()).collect();
        // Banks list the most recent operations first.
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by(|&x, &y| rows[y].booked_on.cmp(&rows[x].booked_on).then(y.cmp(&x)));
        rows = order.iter().map(|&o| rows[o].clone()).collect();

        let batch = repo::insert_batch(&tx, ids[i], Some(a.filename), a.imported_at)?;
        let outcome = repo::insert_transactions(&tx, ids[i], Some(batch), &rows)?;
        if outcome.duplicates != 0 {
            return Err(crate::db::Error::Invalid(
                "demo data contains duplicates".into(),
            ));
        }
        repo::update_batch_counts(&tx, batch, &outcome)?;
        for (pos, &o) in order.iter().enumerate() {
            row_ids[indices[o]] = outcome.inserted[pos];
        }
    }

    // Pair both sides of every transfer.
    let mut sides: std::collections::BTreeMap<u32, Vec<(i64, Id)>> = Default::default();
    for (k, p) in planned.iter().enumerate() {
        if let Some(key) = p.transfer {
            sides
                .entry(key)
                .or_default()
                .push((p.txn.amount_cents, row_ids[k]));
        }
    }
    for pair in sides.values() {
        let debit = pair.iter().find(|s| s.0 < 0).unwrap().1;
        let credit = pair.iter().find(|s| s.0 > 0).unwrap().1;
        repo::pair_transfer(
            &tx,
            debit,
            credit,
            TransferStatus::Confirmed,
            "2026-11-02T18:42:00",
        )?;
    }

    tx.commit()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn loaded() -> (Connection, [Id; 4]) {
        let mut conn = open_in_memory().unwrap();
        let ids = load(&mut conn).unwrap();
        (conn, ids)
    }

    /// Spending of a month per chart group (categories without a colour
    /// count in "Autres"), from SQL.
    fn spending_by_slot(conn: &Connection, month: &str) -> [i64; CHART_GROUPS] {
        let mut out = [0; CHART_GROUPS];
        let mut stmt = conn
            .prepare(
                "SELECT coalesce(c.slot, 13), -sum(t.amount_cents)
                 FROM txn t JOIN category c ON c.id = t.category_id
                 WHERE c.is_income = 0 AND c.exclude_from_stats = 0
                   AND substr(t.booked_on, 1, 7) = ?1
                 GROUP BY 1",
            )
            .unwrap();
        let rows = stmt
            .query_map([month], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))
            .unwrap();
        for row in rows {
            let (slot, cents) = row.unwrap();
            out[slot as usize - 1] = cents;
        }
        out
    }

    fn sum_where(conn: &Connection, sql_where: &str, month: &str) -> i64 {
        conn.query_row(
            &format!(
                "SELECT coalesce(sum(t.amount_cents), 0) FROM txn t JOIN category c ON c.id = t.category_id
                 WHERE {sql_where} AND substr(t.booked_on, 1, 7) = ?1"
            ),
            [month],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn monthly_spending_matches_the_mockup() {
        let (conn, _) = loaded();
        let mut total = 0;
        for (m, month) in MONTHS.iter().enumerate() {
            let key = format!("2026-{month:02}");
            assert_eq!(
                spending_by_slot(&conn, &key),
                chart_row(&MONTHLY_SPENDING[m]),
                "month {key}"
            );
            total += MONTHLY_SPENDING[m].iter().sum::<i64>();
        }
        assert_eq!(total, 1_597_376);
        assert_eq!(MONTHLY_SPENDING[5].iter().sum::<i64>(), 262_490);
        assert_eq!(MONTHLY_SPENDING[3].iter().sum::<i64>(), 304_165);
    }

    #[test]
    fn october_income_and_balances_match_the_mockup() {
        let (conn, ids) = loaded();
        assert_eq!(sum_where(&conn, "c.is_income = 1", "2026-10"), 254_100);
        let total = |on: &str| repo::total_balance_by_currency(&conn, Some(d(on))).unwrap();
        assert_eq!(total("2026-10-31"), vec![("EUR".into(), 1_196_858)]);
        assert_eq!(total("2026-09-30"), vec![("EUR".into(), 1_205_248)]);
        assert_eq!(total("2026-08-31"), vec![("EUR".into(), 1_186_566)]);
        assert_eq!(total("2026-07-31"), vec![("EUR".into(), 1_211_231)]);
        assert_eq!(total("2026-06-30"), vec![("EUR".into(), 1_235_323)]);
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(
                repo::account_balance(&conn, *id, None).unwrap(),
                END_BALANCES[i]
            );
        }
        // Livret A month-end balances (Comptes.dc.html).
        let livret: Vec<i64> = [
            "2026-05-31",
            "2026-06-30",
            "2026-07-31",
            "2026-08-31",
            "2026-09-30",
            "2026-10-31",
        ]
        .iter()
        .map(|on| repo::account_balance(&conn, ids[LIVRET], Some(d(on))).unwrap())
        .collect();
        assert_eq!(
            livret,
            [590_000, 605_000, 620_000, 631_000, 642_000, 653_000]
        );
    }

    #[test]
    fn accounts_banks_and_openings_are_plausible() {
        let (conn, _) = loaded();
        let accounts = repo::list_accounts(&conn, false).unwrap();
        assert_eq!(accounts.len(), 4);
        assert_eq!(repo::list_banks(&conn).unwrap().len(), 3);
        for a in &accounts {
            assert!(
                a.opening_balance_cents >= 0,
                "{} opens at {}",
                a.name,
                a.opening_balance_cents
            );
        }
        let last = repo::last_batch(&conn).unwrap().unwrap();
        assert_eq!(last.imported_at, "2026-11-02T18:42:00");
    }

    #[test]
    fn listed_october_operations_are_present() {
        let (conn, _) = loaded();
        for &(day, label, _, _, cents, review) in OCTOBER_LISTED {
            let found: i64 = conn
                .query_row(
                    "SELECT count(*) FROM txn WHERE booked_on = ?1 AND label = ?2 AND amount_cents = ?3
                     AND needs_review = ?4",
                    rusqlite::params![d(&format!("2026-10-{day:02}")), label, -cents, review],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(found, 1, "{label} on {day}");
        }
        let food_ops: i64 = conn
            .query_row(
                "SELECT count(*) FROM txn WHERE category_id = ?1 AND booked_on LIKE '2026-10-%'",
                [FOOD],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(food_ops, 17); // "17 opérations" (reference/tooltips.png)
    }

    #[test]
    fn transfers_are_paired_and_neutral() {
        let (conn, _) = loaded();
        let (pairs, unpaired, net): (i64, i64, i64) = conn
            .query_row(
                "SELECT count(DISTINCT transfer_pair_id),
                        sum(transfer_pair_id IS NULL),
                        sum(amount_cents)
                 FROM txn WHERE category_id = ?1",
                [INTERNAL_TRANSFER],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(pairs, 18);
        assert_eq!(unpaired, 0);
        assert_eq!(net, 0);
    }

    #[test]
    fn loading_twice_into_the_same_accounts_adds_nothing() {
        let (conn, ids) = loaded();
        let before = repo::transaction_count(&conn, None).unwrap();
        for (i, id) in ids.iter().enumerate() {
            let rows: Vec<NewTransaction> = plan()
                .into_iter()
                .filter(|p| p.account == i)
                .map(|p| p.txn)
                .collect();
            let outcome = repo::insert_transactions(&conn, *id, None, &rows).unwrap();
            assert!(outcome.inserted.is_empty());
        }
        assert_eq!(repo::transaction_count(&conn, None).unwrap(), before);
    }

    #[test]
    fn split_is_exact_and_positive() {
        let mut rng = Lcg(1);
        for (total, n) in [(7_350, 10), (10, 10), (123_457, 7), (1, 1)] {
            let parts = split(total, n, &mut rng);
            assert_eq!(parts.iter().sum::<i64>(), total);
            assert!(parts.iter().all(|&p| p >= 1));
        }
    }
}
