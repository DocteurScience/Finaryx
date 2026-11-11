//! Label normalisation and duplicate keys.

use sha1::{Digest, Sha1};

/// Folds a character to an unaccented upper-case ASCII letter when it is a
/// Latin letter, e.g. `é` → `E`, `œ` → `OE`. Other characters are returned
/// upper-cased.
fn fold(c: char, out: &mut String) {
    let folded = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => "A",
        'ç' | 'Ç' => "C",
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => "E",
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => "I",
        'ñ' | 'Ñ' => "N",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => "O",
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => "U",
        'ý' | 'ÿ' | 'Ý' | 'Ÿ' => "Y",
        'œ' | 'Œ' => "OE",
        'æ' | 'Æ' => "AE",
        'ß' => "SS",
        _ => {
            out.extend(c.to_uppercase());
            return;
        }
    };
    out.push_str(folded);
}

/// Normalised label used for grouping, rules and duplicate detection:
/// upper case, no accents, no digits (hence no dates, card numbers or
/// references), punctuation replaced by spaces, single spaces.
///
/// `"CB****1234 Supermarche 12/10"` → `"CB SUPERMARCHE"`.
pub fn label_norm(label: &str) -> String {
    let mut folded = String::with_capacity(label.len());
    for c in label.chars() {
        fold(c, &mut folded);
    }
    let mut out = String::with_capacity(folded.len());
    for word in folded.split(|c: char| !c.is_alphabetic()) {
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// Duplicate key of an operation (specification §3):
/// `sha1(date | amount_cents | label_norm | reference | n)` where `n` is the
/// rank of the same (date, amount, label, reference) in the file, so that
/// two identical purchases on the same day stay two operations.
pub fn dedupe_key(
    booked_on: &str,
    amount_cents: i64,
    label_norm: &str,
    reference: Option<&str>,
    occurrence: u32,
) -> String {
    let mut hasher = Sha1::new();
    hasher.update(booked_on.as_bytes());
    hasher.update(b"|");
    hasher.update(amount_cents.to_string().as_bytes());
    hasher.update(b"|");
    hasher.update(label_norm.as_bytes());
    hasher.update(b"|");
    hasher.update(reference.unwrap_or("").as_bytes());
    hasher.update(b"|");
    hasher.update(occurrence.to_string().as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Assigns occurrence ranks to a sequence of operation identities, in
/// order: the first `(date, amount, label_norm, reference)` gets 0, the
/// second identical one 1, and so on.
#[derive(Debug, Default)]
pub struct OccurrenceCounter {
    seen: std::collections::HashMap<(String, i64, String, String), u32>,
}

impl OccurrenceCounter {
    pub fn next(
        &mut self,
        booked_on: &str,
        amount_cents: i64,
        label_norm: &str,
        reference: Option<&str>,
    ) -> u32 {
        let key = (
            booked_on.to_owned(),
            amount_cents,
            label_norm.to_owned(),
            reference.unwrap_or("").to_owned(),
        );
        let n = self.seen.entry(key).or_insert(0);
        let current = *n;
        *n += 1;
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_labels() {
        assert_eq!(label_norm("CB****1234 Supermarche 12/10"), "CB SUPERMARCHE");
        assert_eq!(label_norm("Marché Saint-Germain"), "MARCHE SAINT GERMAIN");
        assert_eq!(label_norm("  CLINIQUE   des pins  "), "CLINIQUE DES PINS");
        assert_eq!(label_norm("Œuvre & cœur"), "OEUVRE COEUR");
        assert_eq!(
            label_norm("PRLV SEPA 2026-10-05 REF:AB12CD"),
            "PRLV SEPA REF AB CD"
        );
        assert_eq!(label_norm("1234 / 56"), "");
    }

    #[test]
    fn dedupe_key_is_stable_and_discriminating() {
        let a = dedupe_key("2026-10-31", -2005, "BOULANGERIE DES LILAS", None, 0);
        assert_eq!(
            a,
            dedupe_key("2026-10-31", -2005, "BOULANGERIE DES LILAS", Some(""), 0)
        );
        assert_eq!(a.len(), 40);
        assert_ne!(
            a,
            dedupe_key("2026-10-31", -2005, "BOULANGERIE DES LILAS", None, 1)
        );
        assert_ne!(
            a,
            dedupe_key("2026-10-30", -2005, "BOULANGERIE DES LILAS", None, 0)
        );
        assert_ne!(
            a,
            dedupe_key("2026-10-31", 2005, "BOULANGERIE DES LILAS", None, 0)
        );
        assert_ne!(
            a,
            dedupe_key("2026-10-31", -2005, "BOULANGERIE DES LILAS", Some("R1"), 0)
        );
    }

    #[test]
    fn identical_operations_get_increasing_ranks() {
        let mut counter = OccurrenceCounter::default();
        assert_eq!(counter.next("2026-10-31", -250, "CAFE", None), 0);
        assert_eq!(counter.next("2026-10-31", -250, "CAFE", None), 1);
        assert_eq!(counter.next("2026-10-31", -250, "CAFE", Some("X")), 0);
        assert_eq!(counter.next("2026-10-30", -250, "CAFE", None), 0);
        assert_eq!(counter.next("2026-10-31", -250, "CAFE", None), 2);
    }
}
