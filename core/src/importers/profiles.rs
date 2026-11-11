//! Import profiles ("Retenir ce format"): saved mappings, recognised by
//! the signature of the column names. Built-in profiles are the same
//! kind of mapping, shipped with the application.

use rusqlite::{params, Connection, OptionalExtension};

use super::bank_categories;
use super::format::{DateFormat, Encoding};
use super::mapping::{
    default_direction_values, signature, AmountMode, Field, Mapping, MAPPING_VERSION,
};
use crate::db::Result;
use crate::models::Id;
use crate::repo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// `None` for a built-in profile.
    pub id: Option<Id>,
    pub name: String,
    pub bank_name: Option<String>,
    pub signature: String,
    pub builtin: bool,
    pub mapping: Mapping,
}

/// Columns of the Banque Populaire export (verified format).
pub const BANQUE_POPULAIRE_COLUMNS: [(&str, Field); 13] = [
    ("Date de comptabilisation", Field::PostedOn),
    ("Libelle simplifie", Field::Label),
    ("Libelle operation", Field::Detail),
    ("Reference", Field::Reference),
    ("Informations complementaires", Field::Ignore),
    ("Type operation", Field::Ignore),
    ("Categorie", Field::BankCategory),
    ("Sous categorie", Field::BankSubcategory),
    ("Debit", Field::Debit),
    ("Credit", Field::Credit),
    ("Date operation", Field::BookedOn),
    ("Date de valeur", Field::ValueOn),
    ("Pointage operation", Field::Ignore),
];

/// Profiles shipped with the application. Only the Banque Populaire one is
/// verified on a real export; Boursorama and Revolut wait for anonymised
/// examples.
pub fn builtin_profiles() -> Vec<Profile> {
    let headers: Vec<String> = BANQUE_POPULAIRE_COLUMNS
        .iter()
        .map(|(c, _)| c.to_string())
        .collect();
    let (debit_values, credit_values) = default_direction_values();
    vec![Profile {
        id: None,
        name: "Banque Populaire".into(),
        bank_name: Some("Banque Populaire".into()),
        signature: signature(&headers),
        builtin: true,
        mapping: Mapping {
            version: MAPPING_VERSION,
            encoding: Encoding::Latin1,
            separator: ';',
            skip_records: 0,
            date_format: DateFormat::DayMonthYear,
            decimal: ',',
            amount_mode: AmountMode::DebitCredit,
            columns: BANQUE_POPULAIRE_COLUMNS
                .iter()
                .map(|(c, f)| (c.to_string(), *f))
                .collect(),
            debit_values,
            credit_values,
            bank_categories: bank_categories::banque_populaire(),
        },
    }]
}

fn from_row(
    r: &rusqlite::Row,
) -> rusqlite::Result<(Id, String, Option<String>, String, bool, String)> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
    ))
}

fn to_profile(row: (Id, String, Option<String>, String, bool, String)) -> Option<Profile> {
    let (id, name, bank_name, signature, builtin, json) = row;
    Some(Profile {
        id: Some(id),
        name,
        bank_name,
        signature,
        builtin,
        mapping: Mapping::from_json(&json)?,
    })
}

const SELECT: &str = "SELECT p.id, p.name, b.name, p.signature, p.is_builtin, p.mapping_json
                      FROM import_profile p LEFT JOIN bank b ON b.id = p.bank_id";

/// Profiles saved by the user.
pub fn list_profiles(conn: &Connection) -> Result<Vec<Profile>> {
    let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY p.name"))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.filter_map(|r| r.ok().and_then(to_profile)).collect())
}

/// The profile recognising these column names: a saved one first (it may
/// refine a built-in one), then a built-in one.
pub fn find_profile(conn: &Connection, headers: &[String]) -> Result<Option<Profile>> {
    let sig = signature(headers);
    let saved = conn
        .query_row(
            &format!("{SELECT} WHERE p.signature = ?1"),
            [&sig],
            from_row,
        )
        .optional()?
        .and_then(to_profile);
    Ok(saved.or_else(|| builtin_profiles().into_iter().find(|p| p.signature == sig)))
}

/// Saves a profile under `name` for these columns, replacing the one with
/// the same signature (or the same name).
pub fn save_profile(
    conn: &Connection,
    name: &str,
    bank_name: Option<&str>,
    headers: &[String],
    mapping: &Mapping,
) -> Result<Id> {
    let sig = signature(headers);
    let bank_id = match bank_name.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => Some(repo::bank_id_or_insert(conn, b)?),
        None => None,
    };
    conn.execute(
        "DELETE FROM import_profile WHERE signature = ?1 OR name = ?2",
        params![sig, name.trim()],
    )?;
    conn.execute(
        "INSERT INTO import_profile (name, bank_id, signature, is_builtin, mapping_json) VALUES (?1, ?2, ?3, 0, ?4)",
        params![name.trim(), bank_id, sig, mapping.to_json()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn rename_profile(conn: &Connection, id: Id, name: &str) -> Result<()> {
    conn.execute(
        "UPDATE import_profile SET name = ?2 WHERE id = ?1",
        params![id, name.trim()],
    )?;
    Ok(())
}

pub fn delete_profile(conn: &Connection, id: Id) -> Result<()> {
    conn.execute("DELETE FROM import_profile WHERE id = ?1", [id])?;
    Ok(())
}

/// Profile name proposed from the file name: "export_banque_2026.csv" →
/// "export banque".
pub fn name_from_filename(filename: &str) -> String {
    let stem = filename.rsplit_once('.').map_or(filename, |(s, _)| s);
    let words: Vec<&str> = stem
        .split(['_', '-', ' ', '.'])
        .filter(|w| !w.is_empty() && !w.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let name = words.join(" ");
    if name.is_empty() {
        "Mon format".into()
    } else {
        let mut chars = name.chars();
        chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn banque_populaire_is_recognised_whatever_the_column_order() {
        let conn = open_in_memory().unwrap();
        let mut headers: Vec<String> = BANQUE_POPULAIRE_COLUMNS
            .iter()
            .map(|(c, _)| c.to_string())
            .collect();
        headers.reverse();
        let p = find_profile(&conn, &headers).unwrap().unwrap();
        assert!(p.builtin);
        assert_eq!(p.name, "Banque Populaire");
        assert!(p.mapping.missing_fields().is_empty());
        assert!(find_profile(&conn, &["Date".into(), "Montant".into()])
            .unwrap()
            .is_none());
    }

    #[test]
    fn saved_profiles_take_precedence_and_can_be_managed() {
        let conn = open_in_memory().unwrap();
        let headers: Vec<String> = BANQUE_POPULAIRE_COLUMNS
            .iter()
            .map(|(c, _)| c.to_string())
            .collect();
        let mut mapping = builtin_profiles()[0].mapping.clone();
        mapping
            .columns
            .insert("Informations complementaires".into(), Field::Detail);
        let id = save_profile(
            &conn,
            "Mon BP",
            Some("Banque Populaire"),
            &headers,
            &mapping,
        )
        .unwrap();
        let p = find_profile(&conn, &headers).unwrap().unwrap();
        assert_eq!(p.id, Some(id));
        assert!(!p.builtin);
        assert_eq!(p.bank_name.as_deref(), Some("Banque Populaire"));
        assert_eq!(
            p.mapping.columns["Informations complementaires"],
            Field::Detail
        );

        // Saving again replaces it.
        save_profile(&conn, "Mon BP 2", None, &headers, &mapping).unwrap();
        assert_eq!(list_profiles(&conn).unwrap().len(), 1);
        let id = list_profiles(&conn).unwrap()[0].id.unwrap();
        rename_profile(&conn, id, "Courant BP").unwrap();
        assert_eq!(list_profiles(&conn).unwrap()[0].name, "Courant BP");
        delete_profile(&conn, id).unwrap();
        assert!(find_profile(&conn, &headers).unwrap().unwrap().builtin);
    }

    #[test]
    fn names_from_file_names() {
        assert_eq!(
            name_from_filename("export_banque_2026-10.csv"),
            "Export banque"
        );
        assert_eq!(name_from_filename("00012_01052026.csv"), "Mon format");
        assert_eq!(name_from_filename("revolut.csv"), "Revolut");
    }
}
