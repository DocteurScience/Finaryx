//! French date formatting ("31 oct.", "2 nov. 2026", "octobre 2026").

use chrono::{Datelike, NaiveDate};

const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];
const MONTHS_SHORT: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];

/// "octobre"
pub fn month_name(month: u32) -> &'static str {
    MONTHS[(month as usize - 1) % 12]
}

/// "oct."
pub fn month_short(month: u32) -> &'static str {
    MONTHS_SHORT[(month as usize - 1) % 12]
}

/// "octobre 2026"
pub fn month_year(date: NaiveDate) -> String {
    format!("{} {}", month_name(date.month()), date.year())
}

/// "31 oct."
pub fn day_month(date: NaiveDate) -> String {
    format!("{} {}", date.day(), month_short(date.month()))
}

/// "1er oct.", "2 nov.": day with the French ordinal for the first.
pub fn day_month_ordinal(date: NaiveDate) -> String {
    if date.day() == 1 {
        format!("1er {}", month_short(date.month()))
    } else {
        day_month(date)
    }
}

/// "31 octobre 2026"
pub fn day_month_year_long(date: NaiveDate) -> String {
    format!(
        "{} {} {}",
        date.day(),
        month_name(date.month()),
        date.year()
    )
}

/// "2 nov. 2026"
pub fn day_month_year(date: NaiveDate) -> String {
    format!(
        "{} {} {}",
        date.day(),
        month_short(date.month()),
        date.year()
    )
}

/// Parses the date part of an ISO 8601 date or date-time.
pub fn parse_iso_date(s: &str) -> Option<NaiveDate> {
    s.get(..10)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn formats_like_the_mockup() {
        assert_eq!(day_month(d("2026-10-31")), "31 oct.");
        assert_eq!(day_month_year(d("2026-11-02")), "2 nov. 2026");
        assert_eq!(month_year(d("2026-10-01")), "octobre 2026");
        assert_eq!(day_month(d("2026-08-15")), "15 août");
        assert_eq!(month_short(2), "févr.");
        assert_eq!(day_month_ordinal(d("2026-10-01")), "1er oct.");
        assert_eq!(day_month_ordinal(d("2026-11-02")), "2 nov.");
        assert_eq!(day_month_year_long(d("2026-10-31")), "31 octobre 2026");
    }

    #[test]
    fn parses_iso_dates_and_date_times() {
        assert_eq!(parse_iso_date("2026-11-02T18:42:00"), Some(d("2026-11-02")));
        assert_eq!(parse_iso_date("2026-11-02"), Some(d("2026-11-02")));
        assert_eq!(parse_iso_date("02/11/2026"), None);
        assert_eq!(parse_iso_date(""), None);
    }
}
