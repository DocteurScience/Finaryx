//! Money amounts and their formatting in the active language (`fr-FR`
//! by default, see `locale`).
//!
//! Amounts are always stored as integers in the currency's minor unit
//! (cents for the euro). Floating point is never used, neither for storage
//! nor for formatting.

use crate::locale::{self, Locale, SymbolPosition};

/// Narrow no-break space (U+202F), used by `fr-FR` as the thousands
/// separator and before the currency symbol.
pub const NNBSP: char = '\u{202F}';
/// Typographic minus sign (U+2212), as in the mockup ("−214,88 €").
pub const MINUS: char = '\u{2212}';

/// A currency known to the formatter.
///
/// Unknown ISO codes are still accepted: they are displayed with their code
/// as symbol and two decimals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Currency {
    /// ISO 4217 code, e.g. `"EUR"`.
    pub code: &'static str,
    /// Symbol used in French, e.g. `"€"` or `"$US"`.
    pub symbol: &'static str,
    /// Symbol used by the other languages, e.g. `"US$"` (a language may
    /// have its own, see `locale::Locale::symbols`).
    pub intl: &'static str,
    /// Number of decimals of the minor unit (2 for the euro, 0 for the yen).
    pub exponent: u32,
}

const KNOWN: &[Currency] = &[
    Currency {
        code: "EUR",
        symbol: "€",
        intl: "€",
        exponent: 2,
    },
    Currency {
        code: "USD",
        symbol: "$US",
        intl: "US$",
        exponent: 2,
    },
    Currency {
        code: "GBP",
        symbol: "£GB",
        intl: "£",
        exponent: 2,
    },
    Currency {
        code: "CHF",
        symbol: "CHF",
        intl: "CHF",
        exponent: 2,
    },
    Currency {
        code: "CAD",
        symbol: "$CA",
        intl: "CA$",
        exponent: 2,
    },
    Currency {
        code: "AUD",
        symbol: "$AU",
        intl: "A$",
        exponent: 2,
    },
    Currency {
        code: "SEK",
        symbol: "SEK",
        intl: "SEK",
        exponent: 2,
    },
    Currency {
        code: "NOK",
        symbol: "NOK",
        intl: "NOK",
        exponent: 2,
    },
    Currency {
        code: "DKK",
        symbol: "DKK",
        intl: "DKK",
        exponent: 2,
    },
    Currency {
        code: "PLN",
        symbol: "PLN",
        intl: "PLN",
        exponent: 2,
    },
    Currency {
        code: "CZK",
        symbol: "CZK",
        intl: "CZK",
        exponent: 2,
    },
    Currency {
        code: "MAD",
        symbol: "MAD",
        intl: "MAD",
        exponent: 2,
    },
    Currency {
        code: "XOF",
        symbol: "F\u{202F}CFA",
        intl: "F\u{202F}CFA",
        exponent: 0,
    },
    Currency {
        code: "JPY",
        symbol: "JPY",
        intl: "¥",
        exponent: 0,
    },
    Currency {
        code: "KRW",
        symbol: "KRW",
        intl: "₩",
        exponent: 0,
    },
];

impl Currency {
    pub const EUR: Currency = KNOWN[0];

    /// Looks a currency up by ISO code (case-insensitive).
    pub fn from_code(code: &str) -> Option<Currency> {
        KNOWN
            .iter()
            .copied()
            .find(|c| c.code.eq_ignore_ascii_case(code))
    }

    /// All currencies with a dedicated symbol, in display order.
    pub fn known() -> &'static [Currency] {
        KNOWN
    }
}

/// How the sign of an amount is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    /// Only negative amounts get a sign ("−12,00 €").
    Negative,
    /// Positive amounts get a "+" too, for differences ("+22,40 €").
    Always,
}

/// Groups the digits of `n` by three with `separator`.
fn group_thousands(n: u64, separator: &str) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 * 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push_str(separator);
        }
        out.push(ch);
    }
    out
}

/// Symbol of `currency` in language `l`.
pub fn symbol_in(l: &Locale, currency: &Currency) -> &'static str {
    if l.code == "fr" {
        return currency.symbol;
    }
    l.symbols
        .iter()
        .find(|(code, _)| *code == currency.code)
        .map_or(currency.intl, |(_, s)| s)
}

fn sign_prefix(negative: bool, zero: bool, sign: Sign) -> &'static str {
    match (negative, sign) {
        (true, _) => "\u{2212}",
        (false, Sign::Always) if !zero => "+",
        _ => "",
    }
}

/// Formats an amount in minor units, e.g. `1196858` EUR → `"11 968,58 €"`.
pub fn format_amount(minor: i64, currency: &Currency, sign: Sign) -> String {
    format_amount_in(locale::current(), minor, currency, sign)
}

/// [`format_amount`] in language `l`: "11 968,58 €", "€11,968.58",
/// "€ 11.968,58"…
pub fn format_amount_in(l: &Locale, minor: i64, currency: &Currency, sign: Sign) -> String {
    let negative = minor < 0;
    let abs = minor.unsigned_abs();
    let mut number = String::new();
    if currency.exponent == 0 {
        number.push_str(&group_thousands(abs, l.group));
    } else {
        let unit = 10u64.pow(currency.exponent);
        number.push_str(&group_thousands(abs / unit, l.group));
        number.push(l.decimal);
        number.push_str(&format!(
            "{:0width$}",
            abs % unit,
            width = currency.exponent as usize
        ));
    }
    let prefix = sign_prefix(negative, abs == 0, sign);
    let symbol = symbol_in(l, currency);
    let space = if l.code == "fr" { NNBSP } else { '\u{a0}' };
    match l.symbol {
        SymbolPosition::After => format!("{prefix}{number}{space}{symbol}"),
        SymbolPosition::BeforeSpaced => format!("{prefix}{symbol}{space}{number}"),
        SymbolPosition::Before => format!("{prefix}{symbol}{number}"),
    }
}

/// Formats an amount rounded to whole units (half away from zero),
/// e.g. `1196858` EUR → `"11 969 €"`. Used for chart labels.
pub fn format_amount_whole(minor: i64, currency: &Currency, sign: Sign) -> String {
    format_amount_whole_in(locale::current(), minor, currency, sign)
}

pub fn format_amount_whole_in(l: &Locale, minor: i64, currency: &Currency, sign: Sign) -> String {
    let unit = 10i64.pow(currency.exponent);
    let half = unit / 2;
    let rounded = if currency.exponent == 0 {
        minor
    } else if minor >= 0 {
        (minor + half) / unit
    } else {
        (minor - half) / unit
    };
    let whole = Currency {
        exponent: 0,
        ..*currency
    };
    format_amount_in(l, rounded, &whole, sign)
}

/// Why a typed amount was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseAmountError {
    Empty,
    Invalid,
    TooManyDecimals,
    TooLarge,
}

/// Parses an amount typed by the user, without floating point:
/// `"1 234,56"`, `"-12.5"`, `"−12,00 €"`, `"+3"`, `"1.234,56"`. A comma or
/// a dot followed by one or two digits (or as many as the currency has)
/// is the decimal separator; other dots, spaces and apostrophes group
/// thousands.
pub fn parse_amount(text: &str, currency: &Currency) -> Result<i64, ParseAmountError> {
    let mut s: String = text
        .trim()
        .trim_end_matches(currency.symbol)
        .trim_end_matches(currency.code)
        .chars()
        .filter(|c| !c.is_whitespace() && *c != NNBSP && *c != '\'')
        .collect();
    if s.is_empty() {
        return Err(ParseAmountError::Empty);
    }
    let negative = s.starts_with('-') || s.starts_with(MINUS);
    if negative || s.starts_with('+') {
        s = s.chars().skip(1).collect();
    }
    // Decimal separator: the last ',' or '.', if followed by 1..=exponent digits.
    let (int_part, frac_part) = match s.rfind([',', '.']) {
        Some(pos) => {
            let frac = &s[pos + 1..];
            let is_decimal = currency.exponent > 0
                && !frac.is_empty()
                && frac.len() <= currency.exponent as usize
                && (s[pos..].starts_with(',') || frac.len() != 3);
            if is_decimal {
                (s[..pos].to_string(), frac.to_string())
            } else if frac.len() == 3 {
                (s.clone(), String::new())
            } else {
                return Err(ParseAmountError::TooManyDecimals);
            }
        }
        None => (s.clone(), String::new()),
    };
    let int_digits: String = int_part
        .chars()
        .filter(|c| *c != '.' && *c != ',')
        .collect();
    if int_digits.is_empty() && frac_part.is_empty() {
        return Err(ParseAmountError::Invalid);
    }
    if !int_digits.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return Err(ParseAmountError::Invalid);
    }
    let unit = 10i64.pow(currency.exponent);
    let whole: i64 = if int_digits.is_empty() {
        0
    } else {
        int_digits.parse().map_err(|_| ParseAmountError::TooLarge)?
    };
    let mut frac = frac_part;
    while frac.len() < currency.exponent as usize {
        frac.push('0');
    }
    let cents: i64 = if frac.is_empty() {
        0
    } else {
        frac.parse().map_err(|_| ParseAmountError::Invalid)?
    };
    let value = whole
        .checked_mul(unit)
        .and_then(|v| v.checked_add(cents))
        .ok_or(ParseAmountError::TooLarge)?;
    Ok(if negative { -value } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_in_other_languages() {
        let l = |c: &str| crate::locale::find(c).unwrap();
        let eur = Currency::EUR;
        let usd = Currency::from_code("USD").unwrap();
        assert_eq!(
            format_amount_in(l("en"), -123_456, &eur, Sign::Negative),
            "\u{2212}€1,234.56"
        );
        assert_eq!(format_amount_in(l("en"), 500, &usd, Sign::Always), "+$5.00");
        assert_eq!(
            format_amount_in(l("de"), 123_456, &eur, Sign::Negative),
            "1.234,56\u{a0}€"
        );
        assert_eq!(
            format_amount_in(l("nl"), 123_456, &eur, Sign::Negative),
            "€\u{a0}1.234,56"
        );
        assert_eq!(
            format_amount_in(l("pl"), 123_456, &eur, Sign::Negative),
            "1\u{a0}234,56\u{a0}€"
        );
        assert_eq!(
            format_amount_in(l("de"), 500, &usd, Sign::Negative),
            "5,00\u{a0}US$"
        );
        assert_eq!(
            format_amount_in(l("fr"), 500, &usd, Sign::Negative),
            "5,00\u{202f}$US"
        );
        assert_eq!(
            format_amount_whole_in(l("ja"), 123_456, &eur, Sign::Negative),
            "€1,235"
        );
        let jpy = Currency::from_code("JPY").unwrap();
        assert_eq!(
            format_amount_in(l("ja"), 1500, &jpy, Sign::Negative),
            "￥1,500"
        );
    }

    fn eur(minor: i64) -> String {
        format_amount(minor, &Currency::EUR, Sign::Negative)
    }

    #[test]
    fn formats_mockup_values() {
        assert_eq!(eur(1_196_858), "11\u{202F}968,58\u{202F}€");
        assert_eq!(eur(262_490), "2\u{202F}624,90\u{202F}€");
        assert_eq!(eur(4_400), "44,00\u{202F}€");
        assert_eq!(eur(-21_488), "\u{2212}214,88\u{202F}€");
    }

    #[test]
    fn small_and_zero_amounts() {
        assert_eq!(eur(0), "0,00\u{202F}€");
        assert_eq!(eur(5), "0,05\u{202F}€");
        assert_eq!(eur(-99), "\u{2212}0,99\u{202F}€");
        assert_eq!(
            format_amount(0, &Currency::EUR, Sign::Always),
            "0,00\u{202F}€"
        );
    }

    #[test]
    fn explicit_sign_for_differences() {
        assert_eq!(
            format_amount(2_240, &Currency::EUR, Sign::Always),
            "+22,40\u{202F}€"
        );
        assert_eq!(
            format_amount(-28_000, &Currency::EUR, Sign::Always),
            "\u{2212}280,00\u{202F}€"
        );
    }

    #[test]
    fn thousands_grouping() {
        assert_eq!(eur(100_000_000), "1\u{202F}000\u{202F}000,00\u{202F}€");
        assert_eq!(eur(99_999), "999,99\u{202F}€");
        assert_eq!(
            eur(i64::MIN),
            "\u{2212}92\u{202F}233\u{202F}720\u{202F}368\u{202F}547\u{202F}758,08\u{202F}€"
        );
    }

    #[test]
    fn other_currencies() {
        let usd = Currency::from_code("usd").unwrap();
        assert_eq!(
            format_amount(123_456, &usd, Sign::Negative),
            "1\u{202F}234,56\u{202F}$US"
        );
        let jpy = Currency::from_code("JPY").unwrap();
        assert_eq!(
            format_amount(12_345, &jpy, Sign::Negative),
            "12\u{202F}345\u{202F}JPY"
        );
        assert!(Currency::from_code("XXX").is_none());
    }

    #[test]
    fn parses_typed_amounts() {
        let p = |s| parse_amount(s, &Currency::EUR);
        assert_eq!(p("4 410,00"), Ok(441_000));
        assert_eq!(p("4410"), Ok(441_000));
        assert_eq!(p("4\u{202F}410,5\u{202F}€"), Ok(441_050));
        assert_eq!(p("-12.5"), Ok(-1_250));
        assert_eq!(p("\u{2212}214,88 €"), Ok(-21_488));
        assert_eq!(p("+3"), Ok(300));
        assert_eq!(p("1.234,56"), Ok(123_456));
        assert_eq!(p("1,234.56"), Ok(123_456));
        assert_eq!(p("1.234"), Ok(123_400));
        assert_eq!(p(",5"), Ok(50));
        assert_eq!(p("0"), Ok(0));
        assert_eq!(p(""), Err(ParseAmountError::Empty));
        assert_eq!(p("abc"), Err(ParseAmountError::Invalid));
        assert_eq!(p("12,3456"), Err(ParseAmountError::TooManyDecimals));
        assert_eq!(p("-"), Err(ParseAmountError::Invalid));
        assert_eq!(p("99999999999999999999"), Err(ParseAmountError::TooLarge));
        let jpy = Currency::from_code("JPY").unwrap();
        assert_eq!(parse_amount("12 345", &jpy), Ok(12_345));
    }

    #[test]
    fn whole_units_round_half_away_from_zero() {
        let f = |m| format_amount_whole(m, &Currency::EUR, Sign::Negative);
        assert_eq!(f(1_196_858), "11\u{202F}969\u{202F}€");
        assert_eq!(f(1_131_858), "11\u{202F}319\u{202F}€");
        assert_eq!(f(50), "1\u{202F}€");
        assert_eq!(f(49), "0\u{202F}€");
        assert_eq!(f(-50), "\u{2212}1\u{202F}€");
    }
}
