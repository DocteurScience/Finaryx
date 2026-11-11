//! Categorisation of imported operations (specification §3), in order:
//! a manual choice already made for the same normalised label → the
//! user's rules → the bank's category through the correspondence table →
//! built-in keywords → "Autres", flagged "À vérifier".

use std::collections::HashMap;

use regex::Regex;
use rusqlite::Connection;

use crate::db::Result;
use crate::importers::mapping::BankCategoryChoice;
use crate::models::{builtin, CategorySource, Id};
use crate::normalize::label_norm;

/// Built-in keywords (whole words of the normalised label).
const KEYWORDS: &[(&str, Id)] = &[
    ("ECHEANCE PRET", builtin::LOANS),
    ("REMBT PRET", builtin::LOANS),
    ("PRET", builtin::LOANS),
];

enum Matcher {
    Contains(String),
    Regex(Regex),
}

struct Rule {
    matcher: Matcher,
    category_id: Id,
}

/// The decision for one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub category_id: Id,
    pub source: CategorySource,
    pub needs_review: bool,
}

pub struct Categorizer {
    manual: HashMap<String, Id>,
    rules: Vec<Rule>,
    by_name: HashMap<String, Id>,
}

fn contains_words(haystack: &str, needle: &str) -> bool {
    let h = format!(" {haystack} ");
    h.contains(&format!(" {needle} "))
}

impl Categorizer {
    pub fn load(conn: &Connection) -> Result<Categorizer> {
        let mut manual = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT label_norm, category_id FROM txn
             WHERE category_source = 'manual' AND category_id IS NOT NULL
             ORDER BY booked_on, id",
        )?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Id>(1)?)))? {
            let (label, category) = row?;
            manual.insert(label, category); // the most recent wins
        }

        let mut rules = Vec::new();
        let mut stmt =
            conn.prepare("SELECT pattern, is_regex, category_id FROM rule ORDER BY priority, id")?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, bool>(1)?,
                r.get::<_, Id>(2)?,
            ))
        })? {
            let (pattern, is_regex, category_id) = row?;
            let matcher = if is_regex {
                match Regex::new(&pattern) {
                    Ok(re) => Matcher::Regex(re),
                    Err(_) => continue,
                }
            } else {
                Matcher::Contains(label_norm(&pattern))
            };
            rules.push(Rule {
                matcher,
                category_id,
            });
        }

        let mut by_name = HashMap::new();
        let mut stmt = conn.prepare("SELECT name, id FROM category")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Id>(1)?)))? {
            let (name, id) = row?;
            by_name.insert(label_norm(&name), id);
        }
        Ok(Categorizer {
            manual,
            rules,
            by_name,
        })
    }

    pub fn category_id(&self, name: &str) -> Option<Id> {
        self.by_name.get(&label_norm(name)).copied()
    }

    pub fn categorize(&self, label_norm: &str, bank: Option<&BankCategoryChoice>) -> Decision {
        if let Some(&id) = self.manual.get(label_norm) {
            return Decision {
                category_id: id,
                source: CategorySource::Manual,
                needs_review: false,
            };
        }
        for rule in &self.rules {
            let hit = match &rule.matcher {
                Matcher::Contains(p) => !p.is_empty() && contains_words(label_norm, p),
                Matcher::Regex(re) => re.is_match(label_norm),
            };
            if hit {
                return Decision {
                    category_id: rule.category_id,
                    source: CategorySource::Rule,
                    needs_review: false,
                };
            }
        }
        if let Some(choice) = bank {
            if let Some(id) = self.category_id(&choice.category) {
                return Decision {
                    category_id: id,
                    source: CategorySource::Bank,
                    needs_review: choice.needs_review,
                };
            }
        }
        for (keyword, id) in KEYWORDS {
            if contains_words(label_norm, keyword) {
                return Decision {
                    category_id: *id,
                    source: CategorySource::Auto,
                    needs_review: false,
                };
            }
        }
        Decision {
            category_id: builtin::OTHERS,
            source: CategorySource::Auto,
            needs_review: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::models::*;
    use crate::repo;

    #[test]
    fn order_of_the_sources() {
        let conn = open_in_memory().unwrap();
        let bank = repo::bank_id_or_insert(&conn, "Banque").unwrap();
        let account = repo::insert_account(
            &conn,
            &NewAccount {
                bank_id: Some(bank),
                name: "Courant".into(),
                number: None,
                kind: AccountKind::Checking,
                currency: "EUR".into(),
                opening_balance_cents: 0,
                color_slot: 1,
            },
        )
        .unwrap();
        let mut manual = NewTransaction::simple(
            "2026-10-01".parse().unwrap(),
            "CAFE DES ARTS",
            -350,
            builtin::LEISURE,
        );
        manual.category_source = CategorySource::Manual;
        repo::insert_transactions(&conn, account, None, &[manual]).unwrap();
        conn.execute(
            "INSERT INTO rule (pattern, category_id) VALUES ('supermarche', ?1)",
            [builtin::FOOD],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO rule (pattern, is_regex, category_id) VALUES ('^TRAIN', 1, ?1)",
            [builtin::TRANSPORT],
        )
        .unwrap();

        let c = Categorizer::load(&conn).unwrap();
        let bank_choice = BankCategoryChoice {
            category: "Santé".into(),
            needs_review: false,
        };
        let d = c.categorize("CAFE DES ARTS", Some(&bank_choice));
        assert_eq!(
            (d.category_id, d.source),
            (builtin::LEISURE, CategorySource::Manual)
        );
        let d = c.categorize("CB SUPERMARCHE CENTRE", Some(&bank_choice));
        assert_eq!(
            (d.category_id, d.source),
            (builtin::FOOD, CategorySource::Rule)
        );
        assert_eq!(
            c.categorize("TRAIN REGIONAL", None).category_id,
            builtin::TRANSPORT
        );
        let d = c.categorize("PHARMACIE", Some(&bank_choice));
        assert_eq!(
            (d.category_id, d.source, d.needs_review),
            (builtin::HEALTH, CategorySource::Bank, false)
        );
        let review = BankCategoryChoice {
            category: "Autres".into(),
            needs_review: true,
        };
        assert!(c.categorize("VIR EMIS", Some(&review)).needs_review);
        assert_eq!(
            c.categorize("ECHEANCE PRET IMMOBILIER", None).category_id,
            builtin::LOANS
        );
        // "PRET" must be a whole word.
        let d = c.categorize("PRETTY SHOP", None);
        assert_eq!((d.category_id, d.needs_review), (builtin::OTHERS, true));
    }
}
