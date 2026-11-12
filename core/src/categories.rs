//! Categories and rules (specification §4, "Catégories et règles").
//!
//! Charts never show more than 13 colours: 12 slots plus "Autres", in
//! grey. A category without a slot is grouped into "Autres" in charts
//! only. The number of categories is not limited.

use std::collections::HashMap;

use regex::Regex;
use rusqlite::{params, Connection};

use crate::db::{Error, Result};
use crate::i18n::{gettext, N_};
use crate::models::{builtin, Category, Id};
use crate::normalize::label_norm;
use crate::repo;

/// Number of chart colours that categories can take.
pub const COLOR_SLOTS: u8 = 12;

/// Chart group of "Autres" and of the categories without a colour: drawn
/// last, in grey (`finaryx_c13`). It is not stored in the database.
pub const OTHERS_GROUP: u8 = COLOR_SLOTS + 1;

/// Number of chart groups: the 12 colours, then "Autres".
pub const CHART_GROUPS: usize = OTHERS_GROUP as usize;

/// Names of the built-in categories as stored in the database (ids 1 to
/// 14, migration 1), marked for translation. They stay French in the
/// database (bank categories and rules match them); the interface shows
/// [`display_name`].
pub const BUILTIN_NAMES: [&str; 14] = [
    N_("Logement"),
    N_("Alimentation"),
    N_("Transports"),
    N_("Prêts et crédits"),
    N_("Shopping et animaux"),
    N_("Abonnements et télécoms"),
    N_("Banque et assurances"),
    N_("Autres"),
    N_("Sorties et loisirs"),
    N_("Santé"),
    N_("Impôts et taxes"),
    N_("Administratif et éducation"),
    N_("Revenus"),
    N_("Virement interne"),
];

/// Name of a category as shown: a built-in category that the user has
/// not renamed is translated; any other name is shown as written.
pub fn display_name(id: Id, stored: &str) -> String {
    match usize::try_from(id - 1)
        .ok()
        .and_then(|i| BUILTIN_NAMES.get(i))
    {
        Some(original) if *original == stored => gettext(*original),
        _ => stored.to_owned(),
    }
}

/// Categories in their fixed order, with the names shown to the user.
pub fn list_for_display(conn: &Connection) -> Result<Vec<Category>> {
    Ok(repo::list_categories(conn)?
        .into_iter()
        .map(|mut c| {
            c.name = display_name(c.id, &c.name);
            c
        })
        .collect())
}

/// A category with its number of operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryUse {
    pub category: Category,
    pub operations: i64,
}

/// Categories in their fixed order with their number of operations.
pub fn list_with_counts(conn: &Connection) -> Result<Vec<CategoryUse>> {
    let counts: HashMap<Id, i64> = conn
        .prepare("SELECT category_id, count(*) FROM txn WHERE category_id IS NOT NULL GROUP BY 1")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(list_for_display(conn)?
        .into_iter()
        .map(|c| CategoryUse {
            operations: counts.get(&c.id).copied().unwrap_or(0),
            category: c,
        })
        .collect())
}

/// Name of the category using each slot 1..=12 (index 0 = slot 1).
pub fn slot_owners(conn: &Connection) -> Result<[Option<(Id, String)>; COLOR_SLOTS as usize]> {
    let mut owners: [Option<(Id, String)>; COLOR_SLOTS as usize] = Default::default();
    for c in list_for_display(conn)? {
        if let Some(slot) = c.slot.filter(|s| (1..=COLOR_SLOTS).contains(s)) {
            owners[slot as usize - 1] = Some((c.id, c.name));
        }
    }
    Ok(owners)
}

/// Why a category change is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryError {
    EmptyName,
    /// Another category has this name (case and accents ignored).
    DuplicateName(String),
    /// The colour is used by another category.
    SlotTaken(String),
    /// "Autres", "Revenus" and "Virement interne" cannot be deleted or
    /// merged away.
    Protected,
}

impl CategoryError {
    pub fn message(&self) -> String {
        match self {
            CategoryError::EmptyName => gettext("Donnez un nom à la catégorie."),
            CategoryError::DuplicateName(n) => {
                gettext("La catégorie « {name} » existe déjà.").replace("{name}", n)
            }
            CategoryError::SlotTaken(n) => {
                gettext("Cette couleur est déjà utilisée par « {name} ».").replace("{name}", n)
            }
            CategoryError::Protected => gettext(
                "Cette catégorie est nécessaire au fonctionnement de Finaryx et ne peut pas être supprimée.",
            ),
        }
    }
}

fn invalid(e: CategoryError) -> Error {
    Error::Invalid(e.message())
}

/// Checks a name: not empty, not used by another category (case and
/// accents ignored). Returns the trimmed name.
pub fn check_name(
    conn: &Connection,
    name: &str,
    except: Option<Id>,
) -> std::result::Result<String, CategoryError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CategoryError::EmptyName);
    }
    let wanted = label_norm(name);
    let existing = repo::list_categories(conn).map_err(|_| CategoryError::EmptyName)?;
    // Both the stored and the shown name are taken.
    if let Some(c) = existing.iter().find(|c| {
        Some(c.id) != except
            && (label_norm(&c.name) == wanted || label_norm(&display_name(c.id, &c.name)) == wanted)
    }) {
        return Err(CategoryError::DuplicateName(display_name(c.id, &c.name)));
    }
    Ok(name.to_owned())
}

fn check_slot(conn: &Connection, slot: Option<u8>, except: Option<Id>) -> Result<()> {
    let Some(slot) = slot else { return Ok(()) };
    if !(1..=COLOR_SLOTS).contains(&slot) {
        return Err(Error::Invalid(format!("invalid colour slot {slot}")));
    }
    if let Some((id, name)) = &slot_owners(conn)?[slot as usize - 1] {
        if Some(*id) != except {
            return Err(invalid(CategoryError::SlotTaken(name.clone())));
        }
    }
    Ok(())
}

pub fn create_category(conn: &Connection, name: &str, slot: Option<u8>) -> Result<Id> {
    let name = check_name(conn, name, None).map_err(invalid)?;
    check_slot(conn, slot, None)?;
    conn.execute(
        "INSERT INTO category (name, slot) VALUES (?1, ?2)",
        params![name, slot],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_category(conn: &Connection, id: Id, name: &str) -> Result<()> {
    let name = check_name(conn, name, Some(id)).map_err(invalid)?;
    conn.execute(
        "UPDATE category SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    Ok(())
}

/// Gives a colour to a category or removes it. Freeing a colour never
/// recolours another category. "Autres" keeps its grey.
pub fn set_slot(conn: &Connection, id: Id, slot: Option<u8>) -> Result<()> {
    if id == builtin::OTHERS {
        return Err(Error::Invalid(gettext(
            "« Autres » garde toujours le gris.",
        )));
    }
    check_slot(conn, slot, Some(id))?;
    conn.execute(
        "UPDATE category SET slot = ?2 WHERE id = ?1",
        params![id, slot],
    )?;
    Ok(())
}

fn protected(id: Id) -> bool {
    matches!(
        id,
        builtin::OTHERS | builtin::INCOME | builtin::INTERNAL_TRANSFER
    )
}

/// Moves the operations and rules of `from` into `into`, then deletes
/// `from`. Totals are unchanged. Returns the number of operations moved.
pub fn merge_category(conn: &mut Connection, from: Id, into: Id) -> Result<i64> {
    if protected(from) {
        return Err(invalid(CategoryError::Protected));
    }
    if from == into {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    let moved = tx.execute(
        "UPDATE txn SET category_id = ?2 WHERE category_id = ?1",
        params![from, into],
    )? as i64;
    tx.execute(
        "UPDATE rule SET category_id = ?2 WHERE category_id = ?1",
        params![from, into],
    )?;
    tx.execute("DELETE FROM category WHERE id = ?1", [from])?;
    tx.commit()?;
    Ok(moved)
}

/// Deletes a category: its operations go to "Autres", flagged "À
/// vérifier", and its rules are deleted. Returns the number of operations
/// moved.
pub fn delete_category(conn: &mut Connection, id: Id) -> Result<i64> {
    if protected(id) {
        return Err(invalid(CategoryError::Protected));
    }
    let tx = conn.transaction()?;
    let moved = tx.execute(
        "UPDATE txn SET category_id = ?2, category_source = 'auto', needs_review = 1 WHERE category_id = ?1",
        params![id, builtin::OTHERS],
    )? as i64;
    tx.execute("DELETE FROM rule WHERE category_id = ?1", [id])?;
    tx.execute("DELETE FROM category WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(moved)
}

// ----- Rules -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: Id,
    pub pattern: String,
    pub is_regex: bool,
    pub category_id: Id,
    pub category_name: String,
    pub category_slot: Option<u8>,
    pub priority: i64,
}

/// Does a rule match a normalised label? A plain pattern matches whole
/// words of the label (accents and case ignored).
pub fn rule_matches(pattern: &str, is_regex: bool, label_norm_text: &str) -> bool {
    if is_regex {
        Regex::new(pattern).is_ok_and(|re| re.is_match(label_norm_text))
    } else {
        let p = label_norm(pattern);
        !p.is_empty() && format!(" {label_norm_text} ").contains(&format!(" {p} "))
    }
}

pub fn list_rules(conn: &Connection) -> Result<Vec<Rule>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.pattern, r.is_regex, r.category_id, c.name, c.slot, r.priority
         FROM rule r JOIN category c ON c.id = r.category_id ORDER BY r.priority, r.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Rule {
            id: r.get(0)?,
            pattern: r.get(1)?,
            is_regex: r.get(2)?,
            category_id: r.get(3)?,
            category_name: display_name(r.get(3)?, &r.get::<_, String>(4)?),
            category_slot: r.get(5)?,
            priority: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Operations matching a pattern: all of them, and those a rule may
/// change (not corrected by hand, not already in `category_id`).
pub fn matching_operations(
    conn: &Connection,
    pattern: &str,
    is_regex: bool,
    category_id: Option<Id>,
) -> Result<(Vec<Id>, Vec<Id>)> {
    let mut stmt = conn.prepare("SELECT id, label_norm, category_source, category_id FROM txn")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Id>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<Id>>(3)?,
        ))
    })?;
    let (mut all, mut changeable) = (Vec::new(), Vec::new());
    for row in rows {
        let (id, label, source, current) = row?;
        if rule_matches(pattern, is_regex, &label) {
            all.push(id);
            if source != "manual" && (category_id.is_none() || current != category_id) {
                changeable.push(id);
            }
        }
    }
    Ok((all, changeable))
}

/// Creates a rule; with `apply_existing`, the matching operations not
/// corrected by hand take its category. Returns (rule id, operations
/// changed).
pub fn create_rule(
    conn: &mut Connection,
    pattern: &str,
    category_id: Id,
    apply_existing: bool,
) -> Result<(Id, usize)> {
    let pattern = pattern.trim();
    if label_norm(pattern).is_empty() {
        return Err(Error::Invalid(gettext("Le motif de la règle est vide.")));
    }
    let tx = conn.transaction()?;
    // A newer rule for the same words replaces the older one.
    // (Compared once normalised: SQLite's upper() ignores accents.)
    let wanted = label_norm(pattern);
    let same: Vec<Id> = list_rules(&tx)?
        .into_iter()
        .filter(|r| !r.is_regex && label_norm(&r.pattern) == wanted)
        .map(|r| r.id)
        .collect();
    for id in same {
        tx.execute("DELETE FROM rule WHERE id = ?1", [id])?;
    }
    tx.execute(
        "INSERT INTO rule (pattern, is_regex, category_id) VALUES (?1, 0, ?2)",
        params![pattern, category_id],
    )?;
    let rule_id = tx.last_insert_rowid();
    let mut changed = 0;
    if apply_existing {
        let (_, changeable) = matching_operations(&tx, pattern, false, Some(category_id))?;
        let mut stmt = tx.prepare(
            "UPDATE txn SET category_id = ?2, category_source = 'rule', needs_review = 0 WHERE id = ?1",
        )?;
        for id in &changeable {
            stmt.execute(params![id, category_id])?;
        }
        changed = changeable.len();
    }
    tx.commit()?;
    Ok((rule_id, changed))
}

/// Deletes a rule; operations already classified are not changed.
pub fn delete_rule(conn: &Connection, id: Id) -> Result<()> {
    conn.execute("DELETE FROM rule WHERE id = ?1", [id])?;
    Ok(())
}

/// Pattern proposed for a rule from the label of an operation: its
/// normalised words ("CB SUPERMARCHÉ CENTRE 12/10" → "CB SUPERMARCHE
/// CENTRE"), with the usual card or transfer prefixes removed.
pub fn pattern_from_label(label: &str) -> String {
    const PREFIXES: [&str; 6] = ["CB", "CARTE", "PRLV", "SEPA", "VIR", "PAIEMENT"];
    let words: Vec<&str> = label.split_whitespace().collect();
    let norm = label_norm(label);
    let mut tokens: Vec<&str> = norm.split(' ').filter(|w| !w.is_empty()).collect();
    while tokens.len() > 1 && PREFIXES.contains(&tokens[0]) {
        tokens.remove(0);
    }
    // Keep the original spelling (accents) when the label is simple.
    let simple =
        words.len() == tokens.len() && words.iter().zip(&tokens).all(|(w, t)| label_norm(w) == *t);
    if simple {
        words.join(" ").to_uppercase()
    } else {
        tokens.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::demo;
    use crate::models::NewTransaction;
    use crate::stats;

    fn d(s: &str) -> chrono::NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn create_with_unique_names_and_colours() {
        let conn = open_in_memory().unwrap();
        let id = create_category(&conn, "  Voyages ", None).unwrap();
        assert_eq!(
            repo::list_categories(&conn).unwrap().last().unwrap().name,
            "Voyages"
        );
        // Case and accents are ignored.
        assert!(create_category(&conn, "voyages", None).is_err());
        assert!(create_category(&conn, "SANTE", None).is_err());
        assert!(create_category(&conn, "   ", None).is_err());
        // Colours 1 to 7 are taken by the built-in categories, 8 to 12 are free.
        assert!(create_category(&conn, "Vacances", Some(3)).is_err());
        let owners = slot_owners(&conn).unwrap();
        assert!(owners[..7].iter().all(Option::is_some));
        assert!(owners[7..].iter().all(Option::is_none));
        assert!(create_category(&conn, "Vacances", Some(13)).is_err());
        assert!(create_category(&conn, "Vacances", Some(0)).is_err());
        let holidays = create_category(&conn, "Vacances", Some(12)).unwrap();
        assert!(create_category(&conn, "Week-ends", Some(12)).is_err());
        set_slot(&conn, holidays, None).unwrap();
        // Freeing a colour does not recolour anybody.
        set_slot(&conn, builtin::BANK_INSURANCE, None).unwrap();
        assert!(slot_owners(&conn).unwrap()[6].is_none());
        set_slot(&conn, id, Some(7)).unwrap();
        assert_eq!(
            slot_owners(&conn).unwrap()[6].as_ref().unwrap().1,
            "Voyages"
        );
        assert!(set_slot(&conn, builtin::OTHERS, Some(1)).is_err());
        rename_category(&conn, id, "Grands voyages").unwrap();
        assert!(rename_category(&conn, id, "Logement").is_err());
    }

    #[test]
    fn merge_keeps_totals_and_protected_categories_stay() {
        let mut conn = open_in_memory().unwrap();
        demo::load(&mut conn).unwrap();
        let scope = stats::scope(&conn, None).unwrap().unwrap();
        let before = stats::month_totals(&conn, &scope, d("2026-10-01"))
            .unwrap()
            .spending;
        conn.execute(
            "INSERT INTO rule (pattern, category_id) VALUES ('CINEMA', ?1)",
            [builtin::LEISURE],
        )
        .unwrap();
        let moved = merge_category(&mut conn, builtin::LEISURE, builtin::OTHERS).unwrap();
        assert!(moved > 0);
        assert_eq!(
            stats::month_totals(&conn, &scope, d("2026-10-01"))
                .unwrap()
                .spending,
            before
        );
        assert_eq!(list_rules(&conn).unwrap()[0].category_id, builtin::OTHERS);
        assert!(repo::list_categories(&conn)
            .unwrap()
            .iter()
            .all(|c| c.id != builtin::LEISURE));

        assert!(delete_category(&mut conn, builtin::OTHERS).is_err());
        assert!(merge_category(&mut conn, builtin::INCOME, builtin::OTHERS).is_err());
        let health = list_with_counts(&conn)
            .unwrap()
            .into_iter()
            .find(|u| u.category.id == builtin::HEALTH)
            .unwrap();
        assert_eq!(
            delete_category(&mut conn, builtin::HEALTH).unwrap(),
            health.operations
        );
        assert_eq!(
            stats::month_totals(&conn, &scope, d("2026-10-01"))
                .unwrap()
                .spending,
            before
        );
    }

    #[test]
    fn rules_apply_to_old_operations_except_manual_ones() {
        let mut conn = open_in_memory().unwrap();
        let ids = demo::load(&mut conn).unwrap();
        let rows = vec![
            NewTransaction::simple(
                d("2026-11-01"),
                "CB SUPERMARCHÉ CENTRE",
                -1_000,
                builtin::OTHERS,
            ),
            NewTransaction::simple(
                d("2026-11-02"),
                "CB SUPERMARCHÉ CENTRE",
                -2_000,
                builtin::OTHERS,
            ),
        ];
        let out = repo::insert_transactions(&conn, ids[0], None, &rows).unwrap();
        stats::set_category(&conn, out.inserted[1], builtin::LEISURE).unwrap();

        let (all, changeable) =
            matching_operations(&conn, "SUPERMARCHÉ CENTRE", false, Some(builtin::FOOD)).unwrap();
        assert!(all.len() > changeable.len());
        assert!(changeable.contains(&out.inserted[0]));
        assert!(!changeable.contains(&out.inserted[1]));

        let (_, changed) =
            create_rule(&mut conn, "SUPERMARCHÉ CENTRE", builtin::FOOD, true).unwrap();
        assert_eq!(changed, changeable.len());
        let cat = |conn: &Connection, id: Id| -> (Id, String) {
            conn.query_row(
                "SELECT category_id, category_source FROM txn WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(cat(&conn, out.inserted[0]), (builtin::FOOD, "rule".into()));
        assert_eq!(
            cat(&conn, out.inserted[1]),
            (builtin::LEISURE, "manual".into())
        );

        // Same words again: the rule is replaced, not duplicated.
        create_rule(&mut conn, "supermarché centre", builtin::SHOPPING, false).unwrap();
        assert_eq!(list_rules(&conn).unwrap().len(), 1);
        // Deleting a rule does not change classified operations.
        delete_rule(&conn, list_rules(&conn).unwrap()[0].id).unwrap();
        assert_eq!(cat(&conn, out.inserted[0]).0, builtin::FOOD);
    }

    #[test]
    fn patterns_and_matching() {
        assert_eq!(
            pattern_from_label("CB SUPERMARCHÉ CENTRE"),
            "SUPERMARCHE CENTRE"
        );
        assert_eq!(
            pattern_from_label("SUPERMARCHÉ CENTRE"),
            "SUPERMARCHÉ CENTRE"
        );
        assert_eq!(
            pattern_from_label("PRLV SEPA OPERATEUR MOBILE 12/10"),
            "OPERATEUR MOBILE"
        );
        assert!(rule_matches("supermarché", false, "CB SUPERMARCHE CENTRE"));
        assert!(!rule_matches("MARCHE", false, "CB SUPERMARCHE CENTRE"));
        assert!(rule_matches("^CB ", true, "CB SUPERMARCHE CENTRE"));
    }
}
