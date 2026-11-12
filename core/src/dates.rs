//! Date formatting in the active language (see `locale`): "31 oct.",
//! "2 nov. 2026", "octobre 2026" in French.

use chrono::{Datelike, NaiveDate};

use crate::locale::{self, DatePattern, Locale};

fn idx(month: u32) -> usize {
    (month as usize + 11) % 12
}

/// "octobre"
pub fn month_name(month: u32) -> &'static str {
    locale::current().months[idx(month)]
}

/// "oct."
pub fn month_short(month: u32) -> &'static str {
    locale::current().months_short[idx(month)]
}

/// "octobre 2026"
pub fn month_year(date: NaiveDate) -> String {
    month_year_in(locale::current(), date)
}

/// "31 oct."
pub fn day_month(date: NaiveDate) -> String {
    day_month_in(locale::current(), date)
}

/// "1er oct.", "2 nov.": with the French ordinal for the first.
pub fn day_month_ordinal(date: NaiveDate) -> String {
    day_month_ordinal_in(locale::current(), date)
}

/// "31 octobre 2026"
pub fn day_month_year_long(date: NaiveDate) -> String {
    day_month_year_long_in(locale::current(), date)
}

/// "2 nov. 2026"
pub fn day_month_year(date: NaiveDate) -> String {
    day_month_year_in(locale::current(), date)
}

pub fn month_year_in(l: &Locale, date: NaiveDate) -> String {
    let (m, y) = (l.months[idx(date.month())], date.year());
    match l.pattern {
        DatePattern::YearMonthDayCjk => format!("{y}年{}月", date.month()),
        DatePattern::DayDeMonth => format!("{m} de {y}"),
        _ => format!("{m} {y}"),
    }
}

fn day_month_with(l: &Locale, date: NaiveDate, day: &str) -> String {
    let mon = l.months_short[idx(date.month())];
    match l.pattern {
        DatePattern::DayMonth => format!("{day} {mon}"),
        DatePattern::DayDotMonth => format!("{day}. {mon}"),
        DatePattern::MonthDay => format!("{mon} {day}"),
        DatePattern::DayDeMonth => format!("{day} de {mon}"),
        DatePattern::Numeric => format!("{day}.{}.", date.month()),
        DatePattern::YearMonthDayCjk => format!("{}月{day}日", date.month()),
    }
}

pub fn day_month_in(l: &Locale, date: NaiveDate) -> String {
    day_month_with(l, date, &date.day().to_string())
}

pub fn day_month_ordinal_in(l: &Locale, date: NaiveDate) -> String {
    match l.first_day {
        Some(first) if date.day() == 1 => day_month_with(l, date, first),
        _ => day_month_in(l, date),
    }
}

pub fn day_month_year_in(l: &Locale, date: NaiveDate) -> String {
    let (d, y) = (date.day(), date.year());
    let mon = l.months_short[idx(date.month())];
    match l.pattern {
        DatePattern::DayMonth => format!("{d} {mon} {y}"),
        DatePattern::DayDotMonth => format!("{d}. {mon} {y}"),
        DatePattern::MonthDay => format!("{mon} {d}, {y}"),
        DatePattern::DayDeMonth => format!("{d} de {mon} de {y}"),
        DatePattern::Numeric => format!("{d}.{}.{y}", date.month()),
        DatePattern::YearMonthDayCjk => format!("{y}年{}月{d}日", date.month()),
    }
}

pub fn day_month_year_long_in(l: &Locale, date: NaiveDate) -> String {
    let (d, y) = (date.day(), date.year());
    let month = l.months_in_date[idx(date.month())];
    match l.pattern {
        DatePattern::DayMonth => format!("{d} {month} {y}"),
        DatePattern::DayDotMonth | DatePattern::Numeric => format!("{d}. {month} {y}"),
        DatePattern::MonthDay => format!("{month} {d}, {y}"),
        DatePattern::DayDeMonth => format!("{d} de {month} de {y}"),
        DatePattern::YearMonthDayCjk => format!("{y}年{}月{d}日", date.month()),
    }
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
    fn formats_of_other_languages() {
        let l = |c: &str| crate::locale::find(c).unwrap();
        let oct = d("2026-10-31");
        let first = d("2026-10-01");
        assert_eq!(day_month_in(l("en"), oct), "Oct 31");
        assert_eq!(day_month_year_in(l("en"), oct), "Oct 31, 2026");
        assert_eq!(day_month_year_long_in(l("en"), oct), "October 31, 2026");
        assert_eq!(month_year_in(l("en"), oct), "October 2026");
        assert_eq!(day_month_ordinal_in(l("en"), first), "Oct 1");
        assert_eq!(day_month_in(l("de"), oct), "31. Okt.");
        assert_eq!(day_month_year_long_in(l("de"), oct), "31. Oktober 2026");
        assert_eq!(month_year_in(l("es"), oct), "octubre de 2026");
        assert_eq!(day_month_year_in(l("pt_BR"), oct), "31 de out. de 2026");
        assert_eq!(day_month_year_long_in(l("pl"), oct), "31 października 2026");
        assert_eq!(month_year_in(l("pl"), oct), "październik 2026");
        assert_eq!(day_month_year_long_in(l("cs"), oct), "31. října 2026");
        assert_eq!(day_month_in(l("fi"), oct), "31.10.");
        assert_eq!(day_month_year_long_in(l("fi"), oct), "31. lokakuuta 2026");
        assert_eq!(day_month_year_in(l("ja"), oct), "2026年10月31日");
        assert_eq!(month_year_in(l("zh_CN"), oct), "2026年10月");
        assert_eq!(day_month_in(l("zh_CN"), oct), "10月31日");
    }

    #[test]
    fn parses_iso_dates_and_date_times() {
        assert_eq!(parse_iso_date("2026-11-02T18:42:00"), Some(d("2026-11-02")));
        assert_eq!(parse_iso_date("2026-11-02"), Some(d("2026-11-02")));
        assert_eq!(parse_iso_date("02/11/2026"), None);
        assert_eq!(parse_iso_date(""), None);
    }
}
