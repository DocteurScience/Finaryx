//! Formats of each interface language: month names, date patterns,
//! number and currency formatting, percentages (CLDR conventions,
//! simplified). `dates` and `money` format with the active language.
//!
//! The active language follows the messages actually shown: the interface
//! translates the msgid "fr" into its language code (each `.po` file does
//! it, see `build-aux/translations.py`) and calls [`set`]. Without a
//! translation, the interface stays in French and so do the formats.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Order of the parts of a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatePattern {
    /// "31 oct." / "31 oct. 2026" / "31 octobre 2026" / "octobre 2026".
    DayMonth,
    /// "31. Okt." / "31. Okt. 2026" / "31. Oktober 2026".
    DayDotMonth,
    /// "Oct 31" / "Oct 31, 2026" / "October 31, 2026".
    MonthDay,
    /// "31 de out." / "31 de out. de 2026" / "31 de outubro de 2026" /
    /// "outubro de 2026".
    DayDeMonth,
    /// "31.10." / "31.10.2026" / "31. lokakuuta 2026".
    Numeric,
    /// "10月31日" / "2026年10月31日" / "2026年10月".
    YearMonthDayCjk,
}

/// Where the currency symbol goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolPosition {
    /// "12,50 €" (with a no-break space).
    After,
    /// "€ 12,50" (with a no-break space).
    BeforeSpaced,
    /// "€12.50".
    Before,
}

#[derive(Debug)]
pub struct Locale {
    /// gettext code: "fr", "en", "pt_BR"…
    pub code: &'static str,
    /// Name of the language in itself.
    pub name: &'static str,
    /// Month alone or with the year ("octobre 2026"), lower case if the
    /// language writes it so.
    pub months: [&'static str; 12],
    /// Month inside a full date ("31 października 2026"): genitive or
    /// partitive where the language needs it.
    pub months_in_date: [&'static str; 12],
    /// Short month: axis labels, "31 oct.".
    pub months_short: [&'static str; 12],
    pub pattern: DatePattern,
    /// Written instead of "1" before a month ("1er oct.").
    pub first_day: Option<&'static str>,
    pub decimal: char,
    /// Thousands separator.
    pub group: &'static str,
    pub symbol: SymbolPosition,
    /// "{n} %" with the language's spacing ("%{n}" in Turkish).
    pub percent: &'static str,
    /// Currency symbols of this language that differ from the
    /// international ones: (ISO code, symbol).
    pub symbols: &'static [(&'static str, &'static str)],
}

const NBSP: &str = "\u{a0}";
const NNBSP: &str = "\u{202f}";

pub static LOCALES: [Locale; 18] = [
    Locale {
        code: "fr",
        name: "Français",
        months: [
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
        ],
        months_in_date: [
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
        ],
        months_short: [
            "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.",
            "nov.", "déc.",
        ],
        pattern: DatePattern::DayMonth,
        first_day: Some("1er"),
        decimal: ',',
        group: NNBSP,
        symbol: SymbolPosition::After,
        percent: "{n}\u{a0}%",
        // Filled from `Currency::symbol` (French symbols: "$US", "£GB"…).
        symbols: &[],
    },
    Locale {
        code: "en",
        name: "English",
        months: [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ],
        months_in_date: [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ],
        months_short: [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ],
        pattern: DatePattern::MonthDay,
        first_day: None,
        decimal: '.',
        group: ",",
        symbol: SymbolPosition::Before,
        percent: "{n}%",
        symbols: &[("USD", "$")],
    },
    Locale {
        code: "de",
        name: "Deutsch",
        months: [
            "Januar",
            "Februar",
            "März",
            "April",
            "Mai",
            "Juni",
            "Juli",
            "August",
            "September",
            "Oktober",
            "November",
            "Dezember",
        ],
        months_in_date: [
            "Januar",
            "Februar",
            "März",
            "April",
            "Mai",
            "Juni",
            "Juli",
            "August",
            "September",
            "Oktober",
            "November",
            "Dezember",
        ],
        months_short: [
            "Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sept.", "Okt.", "Nov.",
            "Dez.",
        ],
        pattern: DatePattern::DayDotMonth,
        first_day: None,
        decimal: ',',
        group: ".",
        symbol: SymbolPosition::After,
        percent: "{n}\u{a0}%",
        symbols: &[],
    },
    Locale {
        code: "es",
        name: "Español",
        months: [
            "enero",
            "febrero",
            "marzo",
            "abril",
            "mayo",
            "junio",
            "julio",
            "agosto",
            "septiembre",
            "octubre",
            "noviembre",
            "diciembre",
        ],
        months_in_date: [
            "enero",
            "febrero",
            "marzo",
            "abril",
            "mayo",
            "junio",
            "julio",
            "agosto",
            "septiembre",
            "octubre",
            "noviembre",
            "diciembre",
        ],
        months_short: [
            "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
        ],
        pattern: DatePattern::DayDeMonth,
        first_day: None,
        decimal: ',',
        group: ".",
        symbol: SymbolPosition::After,
        percent: "{n}\u{a0}%",
        symbols: &[],
    },
    Locale {
        code: "it",
        name: "Italiano",
        months: [
            "gennaio",
            "febbraio",
            "marzo",
            "aprile",
            "maggio",
            "giugno",
            "luglio",
            "agosto",
            "settembre",
            "ottobre",
            "novembre",
            "dicembre",
        ],
        months_in_date: [
            "gennaio",
            "febbraio",
            "marzo",
            "aprile",
            "maggio",
            "giugno",
            "luglio",
            "agosto",
            "settembre",
            "ottobre",
            "novembre",
            "dicembre",
        ],
        months_short: [
            "gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic",
        ],
        pattern: DatePattern::DayMonth,
        first_day: None,
        decimal: ',',
        group: ".",
        symbol: SymbolPosition::After,
        percent: "{n}%",
        symbols: &[],
    },
    Locale {
        code: "pt_BR",
        name: "Português (Brasil)",
        months: [
            "janeiro",
            "fevereiro",
            "março",
            "abril",
            "maio",
            "junho",
            "julho",
            "agosto",
            "setembro",
            "outubro",
            "novembro",
            "dezembro",
        ],
        months_in_date: [
            "janeiro",
            "fevereiro",
            "março",
            "abril",
            "maio",
            "junho",
            "julho",
            "agosto",
            "setembro",
            "outubro",
            "novembro",
            "dezembro",
        ],
        months_short: [
            "jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.",
            "dez.",
        ],
        pattern: DatePattern::DayDeMonth,
        first_day: None,
        decimal: ',',
        group: ".",
        symbol: SymbolPosition::BeforeSpaced,
        percent: "{n}%",
        symbols: &[("BRL", "R$")],
    },
    Locale {
        code: "pt_PT",
        name: "Português (Portugal)",
        months: [
            "janeiro",
            "fevereiro",
            "março",
            "abril",
            "maio",
            "junho",
            "julho",
            "agosto",
            "setembro",
            "outubro",
            "novembro",
            "dezembro",
        ],
        months_in_date: [
            "janeiro",
            "fevereiro",
            "março",
            "abril",
            "maio",
            "junho",
            "julho",
            "agosto",
            "setembro",
            "outubro",
            "novembro",
            "dezembro",
        ],
        months_short: [
            "jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.", "nov.",
            "dez.",
        ],
        pattern: DatePattern::DayDeMonth,
        first_day: None,
        decimal: ',',
        group: NBSP,
        symbol: SymbolPosition::After,
        percent: "{n}%",
        symbols: &[],
    },
    Locale {
        code: "nl",
        name: "Nederlands",
        months: [
            "januari",
            "februari",
            "maart",
            "april",
            "mei",
            "juni",
            "juli",
            "augustus",
            "september",
            "oktober",
            "november",
            "december",
        ],
        months_in_date: [
            "januari",
            "februari",
            "maart",
            "april",
            "mei",
            "juni",
            "juli",
            "augustus",
            "september",
            "oktober",
            "november",
            "december",
        ],
        months_short: [
            "jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec",
        ],
        pattern: DatePattern::DayMonth,
        first_day: None,
        decimal: ',',
        group: ".",
        symbol: SymbolPosition::BeforeSpaced,
        percent: "{n}%",
        symbols: &[],
    },
    Locale {
        code: "pl",
        name: "Polski",
        months: [
            "styczeń",
            "luty",
            "marzec",
            "kwiecień",
            "maj",
            "czerwiec",
            "lipiec",
            "sierpień",
            "wrzesień",
            "październik",
            "listopad",
            "grudzień",
        ],
        months_in_date: [
            "stycznia",
            "lutego",
            "marca",
            "kwietnia",
            "maja",
            "czerwca",
            "lipca",
            "sierpnia",
            "września",
            "października",
            "listopada",
            "grudnia",
        ],
        months_short: [
            "sty", "lut", "mar", "kwi", "maj", "cze", "lip", "sie", "wrz", "paź", "lis", "gru",
        ],
        pattern: DatePattern::DayMonth,
        first_day: None,
        decimal: ',',
        group: NBSP,
        symbol: SymbolPosition::After,
        percent: "{n}%",
        symbols: &[("PLN", "zł")],
    },
    Locale {
        code: "cs",
        name: "Čeština",
        months: [
            "leden",
            "únor",
            "březen",
            "duben",
            "květen",
            "červen",
            "červenec",
            "srpen",
            "září",
            "říjen",
            "listopad",
            "prosinec",
        ],
        months_in_date: [
            "ledna",
            "února",
            "března",
            "dubna",
            "května",
            "června",
            "července",
            "srpna",
            "září",
            "října",
            "listopadu",
            "prosince",
        ],
        months_short: [
            "led", "úno", "bře", "dub", "kvě", "čvn", "čvc", "srp", "zář", "říj", "lis", "pro",
        ],
        pattern: DatePattern::DayDotMonth,
        first_day: None,
        decimal: ',',
        group: NBSP,
        symbol: SymbolPosition::After,
        percent: "{n}\u{a0}%",
        symbols: &[("CZK", "Kč")],
    },
    Locale {
        code: "sv",
        name: "Svenska",
        months: [
            "januari",
            "februari",
            "mars",
            "april",
            "maj",
            "juni",
            "juli",
            "augusti",
            "september",
            "oktober",
            "november",
            "december",
        ],
        months_in_date: [
            "januari",
            "februari",
            "mars",
            "april",
            "maj",
            "juni",
            "juli",
            "augusti",
            "september",
            "oktober",
            "november",
            "december",
        ],
        months_short: [
            "jan.", "feb.", "mars", "apr.", "maj", "juni", "juli", "aug.", "sep.", "okt.", "nov.",
            "dec.",
        ],
        pattern: DatePattern::DayMonth,
        first_day: None,
        decimal: ',',
        group: NBSP,
        symbol: SymbolPosition::After,
        percent: "{n}\u{a0}%",
        symbols: &[("SEK", "kr")],
    },
    Locale {
        code: "da",
        name: "Dansk",
        months: [
            "januar",
            "februar",
            "marts",
            "april",
            "maj",
            "juni",
            "juli",
            "august",
            "september",
            "oktober",
            "november",
            "december",
        ],
        months_in_date: [
            "januar",
            "februar",
            "marts",
            "april",
            "maj",
            "juni",
            "juli",
            "august",
            "september",
            "oktober",
            "november",
            "december",
        ],
        months_short: [
            "jan.", "feb.", "mar.", "apr.", "maj", "jun.", "jul.", "aug.", "sep.", "okt.", "nov.",
            "dec.",
        ],
        pattern: DatePattern::DayDotMonth,
        first_day: None,
        decimal: ',',
        group: ".",
        symbol: SymbolPosition::After,
        percent: "{n}\u{a0}%",
        symbols: &[("DKK", "kr.")],
    },
    Locale {
        code: "nb",
        name: "Norsk bokmål",
        months: [
            "januar",
            "februar",
            "mars",
            "april",
            "mai",
            "juni",
            "juli",
            "august",
            "september",
            "oktober",
            "november",
            "desember",
        ],
        months_in_date: [
            "januar",
            "februar",
            "mars",
            "april",
            "mai",
            "juni",
            "juli",
            "august",
            "september",
            "oktober",
            "november",
            "desember",
        ],
        months_short: [
            "jan.", "feb.", "mar.", "apr.", "mai", "jun.", "jul.", "aug.", "sep.", "okt.", "nov.",
            "des.",
        ],
        pattern: DatePattern::DayDotMonth,
        first_day: None,
        decimal: ',',
        group: NBSP,
        symbol: SymbolPosition::After,
        percent: "{n}\u{a0}%",
        symbols: &[("NOK", "kr")],
    },
    Locale {
        code: "fi",
        name: "Suomi",
        months: [
            "tammikuu",
            "helmikuu",
            "maaliskuu",
            "huhtikuu",
            "toukokuu",
            "kesäkuu",
            "heinäkuu",
            "elokuu",
            "syyskuu",
            "lokakuu",
            "marraskuu",
            "joulukuu",
        ],
        months_in_date: [
            "tammikuuta",
            "helmikuuta",
            "maaliskuuta",
            "huhtikuuta",
            "toukokuuta",
            "kesäkuuta",
            "heinäkuuta",
            "elokuuta",
            "syyskuuta",
            "lokakuuta",
            "marraskuuta",
            "joulukuuta",
        ],
        months_short: [
            "tammi", "helmi", "maalis", "huhti", "touko", "kesä", "heinä", "elo", "syys", "loka",
            "marras", "joulu",
        ],
        pattern: DatePattern::Numeric,
        first_day: None,
        decimal: ',',
        group: NBSP,
        symbol: SymbolPosition::After,
        percent: "{n}\u{a0}%",
        symbols: &[],
    },
    Locale {
        code: "uk",
        name: "Українська",
        months: [
            "січень",
            "лютий",
            "березень",
            "квітень",
            "травень",
            "червень",
            "липень",
            "серпень",
            "вересень",
            "жовтень",
            "листопад",
            "грудень",
        ],
        months_in_date: [
            "січня",
            "лютого",
            "березня",
            "квітня",
            "травня",
            "червня",
            "липня",
            "серпня",
            "вересня",
            "жовтня",
            "листопада",
            "грудня",
        ],
        months_short: [
            "січ.",
            "лют.",
            "бер.",
            "квіт.",
            "трав.",
            "черв.",
            "лип.",
            "серп.",
            "вер.",
            "жовт.",
            "лист.",
            "груд.",
        ],
        pattern: DatePattern::DayMonth,
        first_day: None,
        decimal: ',',
        group: NBSP,
        symbol: SymbolPosition::After,
        percent: "{n}%",
        symbols: &[("UAH", "₴")],
    },
    Locale {
        code: "tr",
        name: "Türkçe",
        months: [
            "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül",
            "Ekim", "Kasım", "Aralık",
        ],
        months_in_date: [
            "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül",
            "Ekim", "Kasım", "Aralık",
        ],
        months_short: [
            "Oca", "Şub", "Mar", "Nis", "May", "Haz", "Tem", "Ağu", "Eyl", "Eki", "Kas", "Ara",
        ],
        pattern: DatePattern::DayMonth,
        first_day: None,
        decimal: ',',
        group: ".",
        symbol: SymbolPosition::Before,
        percent: "%{n}",
        symbols: &[("TRY", "₺")],
    },
    Locale {
        code: "ja",
        name: "日本語",
        months: [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ],
        months_in_date: [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ],
        months_short: [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ],
        pattern: DatePattern::YearMonthDayCjk,
        first_day: None,
        decimal: '.',
        group: ",",
        symbol: SymbolPosition::Before,
        percent: "{n}%",
        symbols: &[("JPY", "￥")],
    },
    Locale {
        code: "zh_CN",
        name: "简体中文",
        months: [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ],
        months_in_date: [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ],
        months_short: [
            "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
        ],
        pattern: DatePattern::YearMonthDayCjk,
        first_day: None,
        decimal: '.',
        group: ",",
        symbol: SymbolPosition::Before,
        percent: "{n}%",
        symbols: &[("CNY", "¥")],
    },
];

static ACTIVE: AtomicUsize = AtomicUsize::new(0);

/// Finds a language: exact code ("pt_BR"), then the language part
/// ("de_AT" → "de"; "pt" → "pt_PT"; "zh" → "zh_CN"). Encoding and
/// modifier ("fr_FR.UTF-8@euro") are ignored.
pub fn find(code: &str) -> Option<&'static Locale> {
    let code = code
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .replace('-', "_");
    if let Some(l) = LOCALES.iter().find(|l| l.code.eq_ignore_ascii_case(&code)) {
        return Some(l);
    }
    let language = code.split('_').next().unwrap_or("");
    let fallback = match language {
        "pt" => "pt_PT",
        "zh" => "zh_CN",
        "no" | "nn" => "nb",
        other => other,
    };
    LOCALES.iter().find(|l| l.code == fallback)
}

/// Sets the language of the formats; an unknown code keeps French.
pub fn set(code: &str) {
    let index = find(code)
        .and_then(|l| LOCALES.iter().position(|x| std::ptr::eq(x, l)))
        .unwrap_or(0);
    ACTIVE.store(index, Ordering::Relaxed);
}

/// The active language (French until [`set`] is called).
pub fn current() -> &'static Locale {
    &LOCALES[ACTIVE.load(Ordering::Relaxed)]
}

impl Locale {
    /// "27,9 %" from a number already written with the right decimal mark.
    pub fn percent_of(&self, number: &str) -> String {
        self.percent.replace("{n}", number)
    }

    /// Tenths of a percent: 279 → "27,9 %" / "27.9%".
    pub fn percent_tenths(&self, tenths: i64) -> String {
        let sign = if tenths < 0 { "\u{2212}" } else { "" };
        let t = tenths.unsigned_abs();
        self.percent_of(&format!("{sign}{}{}{}", t / 10, self.decimal, t % 10))
    }

    /// Whole percent: 28 → "28 %" / "28%".
    pub fn percent_whole(&self, value: i64) -> String {
        let sign = if value < 0 { "\u{2212}" } else { "" };
        self.percent_of(&format!("{sign}{}", value.unsigned_abs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_are_found_from_environment_values() {
        assert_eq!(find("fr_FR.UTF-8").unwrap().code, "fr");
        assert_eq!(find("en_GB").unwrap().code, "en");
        assert_eq!(find("pt_BR.UTF-8").unwrap().code, "pt_BR");
        assert_eq!(find("pt").unwrap().code, "pt_PT");
        assert_eq!(find("zh_TW").unwrap().code, "zh_CN");
        assert_eq!(find("de-AT").unwrap().code, "de");
        assert_eq!(find("nn_NO").unwrap().code, "nb");
        assert!(find("xx").is_none());
        assert!(find("").is_none());
    }

    #[test]
    fn codes_are_unique_and_tables_complete() {
        let mut codes: Vec<&str> = LOCALES.iter().map(|l| l.code).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), LOCALES.len());
        for l in &LOCALES {
            for m in l
                .months
                .iter()
                .chain(&l.months_in_date)
                .chain(&l.months_short)
            {
                assert!(!m.is_empty(), "{}", l.code);
            }
            assert!(l.percent.contains("{n}"), "{}", l.code);
        }
    }

    #[test]
    fn percentages() {
        let fr = find("fr").unwrap();
        assert_eq!(fr.percent_tenths(279), "27,9\u{a0}%");
        assert_eq!(fr.percent_whole(-4), "\u{2212}4\u{a0}%");
        assert_eq!(find("en").unwrap().percent_tenths(279), "27.9%");
        assert_eq!(find("tr").unwrap().percent_whole(28), "%28");
    }
}
