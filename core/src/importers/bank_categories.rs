//! Categories given by the bank, and their Finaryx counterparts.

use std::collections::BTreeMap;

use super::mapping::{bank_category_key, normalize_header, BankCategoryChoice};
use crate::models::Category;

/// Name used in the table for "Virement interne" (excluded from stats).
pub const INTERNAL_TRANSFER: &str = "Virement interne";
/// Fallback category.
pub const OTHERS: &str = "Autres";

/// Correspondence table of the Banque Populaire (category + sub-category
/// couples), shipped with the application.
const BANQUE_POPULAIRE_CSV: &str = include_str!("../../../data/banque_populaire_category_map.csv");

/// The Banque Populaire table: key → choice.
pub fn banque_populaire() -> BTreeMap<String, BankCategoryChoice> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(BANQUE_POPULAIRE_CSV.as_bytes());
    let mut out = BTreeMap::new();
    for record in reader.records().flatten() {
        let (cat, sub, target, review) = (&record[0], &record[1], &record[2], &record[3]);
        let category = if target == "(exclue)" {
            INTERNAL_TRANSFER
        } else {
            target
        };
        out.insert(
            bank_category_key(cat, Some(sub)),
            BankCategoryChoice {
                category: category.to_owned(),
                needs_review: review.trim() == "1",
            },
        );
    }
    out
}

fn words(s: &str) -> Vec<String> {
    normalize_header(s)
        .split(' ')
        .filter(|w| w.len() > 3)
        .map(|w| w.trim_end_matches('s').to_owned())
        .collect()
}

/// Proposes a Finaryx category for a bank category: same name first, then
/// a shared word, otherwise "Autres" flagged "À vérifier".
pub fn propose(
    category: &str,
    subcategory: Option<&str>,
    categories: &[Category],
) -> BankCategoryChoice {
    let names = [subcategory.unwrap_or(""), category];
    // Exact name (sub-category first: it is more precise).
    for name in names {
        if let Some(c) = categories
            .iter()
            .find(|c| normalize_header(&c.name) == normalize_header(name))
        {
            return BankCategoryChoice {
                category: c.name.clone(),
                needs_review: false,
            };
        }
    }
    // A shared word ("Transports en commun" → "Transports").
    for name in names {
        let wanted = words(name);
        if wanted.is_empty() {
            continue;
        }
        if let Some(c) = categories
            .iter()
            .find(|c| words(&c.name).iter().any(|w| wanted.contains(w)))
        {
            return BankCategoryChoice {
                category: c.name.clone(),
                needs_review: false,
            };
        }
    }
    BankCategoryChoice {
        category: OTHERS.to_owned(),
        needs_review: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::repo;

    #[test]
    fn banque_populaire_table() {
        let table = banque_populaire();
        assert_eq!(table.len(), 42);
        let get = |k: &str| table.get(k).unwrap().clone();
        assert_eq!(
            get("Alimentation / Restaurant").category,
            "Sorties et loisirs"
        );
        assert_eq!(
            get("Logement - maison / Internet et telephonie").category,
            "Abonnements et télécoms"
        );
        assert!(get("Banque et assurances / Banque et assurance - autre").needs_review);
        assert_eq!(
            get("Transaction exclue / Virement interne").category,
            INTERNAL_TRANSFER
        );
        // Every target exists among the built-in categories.
        let conn = open_in_memory().unwrap();
        let names: Vec<String> = repo::list_categories(&conn)
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        for choice in table.values() {
            assert!(names.contains(&choice.category), "{}", choice.category);
        }
    }

    #[test]
    fn proposals_by_name() {
        let conn = open_in_memory().unwrap();
        let cats = repo::list_categories(&conn).unwrap();
        assert_eq!(propose("Santé", None, &cats).category, "Santé");
        assert_eq!(
            propose("Transports en commun", None, &cats).category,
            "Transports"
        );
        assert_eq!(
            propose("Divers", Some("Alimentation"), &cats).category,
            "Alimentation"
        );
        let unknown = propose("Xyz", None, &cats);
        assert_eq!(unknown.category, OTHERS);
        assert!(unknown.needs_review);
    }
}
