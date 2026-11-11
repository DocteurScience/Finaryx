//! Reading a CSV file whose format is unknown: encoding, column
//! separator, header line, decimal separator and date format are detected,
//! and each can be overridden by the user.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Text encoding of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoding {
    Utf8,
    /// ISO-8859-1 (Latin-1).
    Latin1,
    /// Windows-1252, a superset of Latin-1 (€, œ, curly quotes…).
    Windows1252,
}

impl Encoding {
    pub const ALL: [Encoding; 3] = [Encoding::Utf8, Encoding::Latin1, Encoding::Windows1252];

    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Latin1 => "ISO-8859-1",
            Encoding::Windows1252 => "Windows-1252",
        }
    }
}

/// Characters 0x80–0x9F of Windows-1252 (undefined ones map to U+FFFD).
const CP1252_HIGH: [char; 32] = [
    '€', '\u{FFFD}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{FFFD}', 'Ž',
    '\u{FFFD}', '\u{FFFD}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{FFFD}',
    'ž', 'Ÿ',
];

/// UTF-8 (with or without BOM) when the bytes are valid UTF-8; otherwise
/// Windows-1252 if a byte of 0x80–0x9F appears, else ISO-8859-1.
pub fn detect_encoding(bytes: &[u8]) -> Encoding {
    if std::str::from_utf8(bytes).is_ok() {
        Encoding::Utf8
    } else if bytes.iter().any(|b| (0x80..=0x9F).contains(b)) {
        Encoding::Windows1252
    } else {
        Encoding::Latin1
    }
}

/// Decodes the file. Invalid UTF-8 sequences become U+FFFD; a BOM is
/// removed.
pub fn decode(bytes: &[u8], encoding: Encoding) -> String {
    match encoding {
        Encoding::Utf8 => {
            let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
            String::from_utf8_lossy(bytes).into_owned()
        }
        Encoding::Latin1 => bytes.iter().map(|&b| b as char).collect(),
        Encoding::Windows1252 => bytes
            .iter()
            .map(|&b| {
                if (0x80..=0x9F).contains(&b) {
                    CP1252_HIGH[(b - 0x80) as usize]
                } else {
                    b as char
                }
            })
            .collect(),
    }
}

pub const SEPARATORS: [char; 4] = [';', ',', '\t', '|'];

pub fn separator_label(sep: char) -> &'static str {
    match sep {
        ';' => "Point-virgule ( ; )",
        ',' => "Virgule ( , )",
        '\t' => "Tabulation",
        '|' => "Barre verticale ( | )",
        _ => "?",
    }
}

/// Splits the text into records (quotes are honoured). Each record keeps
/// its 1-based line number in the file.
pub fn split(text: &str, separator: char) -> Vec<(usize, Vec<String>)> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(separator as u8)
        .from_reader(text.as_bytes());
    let mut out = Vec::new();
    for result in reader.records() {
        let Ok(record) = result else { continue };
        let line = record.position().map_or(0, |p| p.line() as usize);
        let fields: Vec<String> = record.iter().map(|f| f.trim().to_owned()).collect();
        if fields.iter().all(|f| f.is_empty()) {
            continue;
        }
        out.push((line, fields));
    }
    out
}

/// Most frequent field count among the records, ignoring 1-field lines.
fn stable_width(records: &[(usize, Vec<String>)]) -> Option<usize> {
    let mut counts = std::collections::BTreeMap::new();
    for (_, r) in records.iter().take(200) {
        if r.len() > 1 {
            *counts.entry(r.len()).or_insert(0usize) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(width, n)| (n, width))
        .map(|(w, _)| w)
}

/// The separator giving the most records with the same, largest field
/// count.
pub fn detect_separator(text: &str) -> char {
    let sample: String = text.lines().take(60).collect::<Vec<_>>().join("\n");
    SEPARATORS
        .iter()
        .copied()
        .max_by_key(|&sep| {
            let records = split(&sample, sep);
            match stable_width(&records) {
                Some(w) => {
                    let n = records.iter().filter(|(_, r)| r.len() == w).count();
                    (n, w)
                }
                None => (0, 0),
            }
        })
        .unwrap_or(';')
}

/// Index (in `records`) of the header: the first record whose field count
/// is the stable one and stays so for the following records. Banks that
/// add metadata lines before the table are handled this way.
pub fn detect_header(records: &[(usize, Vec<String>)]) -> usize {
    let Some(width) = stable_width(records) else {
        return 0;
    };
    for i in 0..records.len() {
        if records[i].1.len() == width
            && records[i + 1..]
                .iter()
                .take(5)
                .all(|(_, r)| r.len() == width)
        {
            return i;
        }
    }
    0
}

/// Decimal separator of the amounts: `,` or `.`. The last `,` or `.` of
/// a number followed by one or two digits is its decimal separator.
pub fn detect_decimal(samples: &[&str]) -> char {
    let (mut comma, mut dot) = (0, 0);
    for s in samples {
        let s: String = s
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == ',' || *c == '.')
            .collect();
        let Some(pos) = s.rfind([',', '.']) else {
            continue;
        };
        let after = s.len() - pos - 1;
        if (1..=2).contains(&after) {
            if s.as_bytes()[pos] == b',' {
                comma += 1;
            } else {
                dot += 1;
            }
        }
    }
    if dot > comma {
        '.'
    } else {
        ','
    }
}

/// Date formats understood by the importer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateFormat {
    /// JJ/MM/AAAA
    DayMonthYear,
    /// AAAA-MM-JJ (a time after the date is ignored)
    Iso,
    /// JJ-MM-AA
    DayMonthShortYear,
    /// MM/JJ/AAAA
    MonthDayYear,
}

impl DateFormat {
    pub const ALL: [DateFormat; 4] = [
        DateFormat::DayMonthYear,
        DateFormat::Iso,
        DateFormat::DayMonthShortYear,
        DateFormat::MonthDayYear,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DateFormat::DayMonthYear => "JJ/MM/AAAA",
            DateFormat::Iso => "AAAA-MM-JJ",
            DateFormat::DayMonthShortYear => "JJ-MM-AA",
            DateFormat::MonthDayYear => "MM/JJ/AAAA",
        }
    }

    pub fn parse(self, text: &str) -> Option<NaiveDate> {
        let t = text.trim();
        let nums: Vec<&str> = t
            .split(['/', '-', '.', ' ', 'T'])
            .filter(|p| !p.is_empty())
            .collect();
        if nums.len() < 3 {
            return None;
        }
        let n = |i: usize| nums[i].parse::<i32>().ok();
        let (y, m, d) = match self {
            DateFormat::DayMonthYear => {
                (nums[2].len() == 4).then_some(())?;
                (n(2)?, n(1)?, n(0)?)
            }
            DateFormat::Iso => {
                (nums[0].len() == 4).then_some(())?;
                (n(0)?, n(1)?, n(2)?)
            }
            DateFormat::DayMonthShortYear => {
                (nums[2].len() == 2).then_some(())?;
                (2000 + n(2)?, n(1)?, n(0)?)
            }
            DateFormat::MonthDayYear => {
                (nums[2].len() == 4).then_some(())?;
                (n(2)?, n(0)?, n(1)?)
            }
        };
        NaiveDate::from_ymd_opt(y, u32::try_from(m).ok()?, u32::try_from(d).ok()?)
    }
}

/// Result of the date detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateDetection {
    pub format: DateFormat,
    /// Both JJ/MM and MM/JJ read every sample: the user must choose.
    pub ambiguous: bool,
}

/// Picks the format that reads every non-empty sample. JJ/MM/AAAA is
/// preferred to MM/JJ/AAAA unless a day above 12 proves the latter.
pub fn detect_date_format(samples: &[&str]) -> Option<DateDetection> {
    let samples: Vec<&str> = samples
        .iter()
        .copied()
        .filter(|s| !s.trim().is_empty())
        .collect();
    if samples.is_empty() {
        return None;
    }
    let reads_all = |f: DateFormat| samples.iter().all(|s| f.parse(s).is_some());
    let candidates: Vec<DateFormat> = DateFormat::ALL
        .iter()
        .copied()
        .filter(|f| reads_all(*f))
        .collect();
    let dmy = candidates.contains(&DateFormat::DayMonthYear);
    let mdy = candidates.contains(&DateFormat::MonthDayYear);
    let format = *candidates.first()?;
    Some(DateDetection {
        format,
        ambiguous: dmy && mdy,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings() {
        assert_eq!(detect_encoding("Libellé".as_bytes()), Encoding::Utf8);
        assert_eq!(detect_encoding(b"Libell\xE9"), Encoding::Latin1);
        assert_eq!(detect_encoding(b"\x80 Libell\xE9"), Encoding::Windows1252);
        assert_eq!(decode(b"Libell\xE9", Encoding::Latin1), "Libellé");
        assert_eq!(decode(b"\x80\x9C", Encoding::Windows1252), "€œ");
        assert_eq!(
            decode("\u{FEFF}Date;Montant".as_bytes(), Encoding::Utf8),
            "Date;Montant"
        );
    }

    #[test]
    fn separators_and_metadata_lines() {
        let text = "Compte n° 123\nExport du 01/11/2026\nDate,Libellé,Montant\n01/10/2026,\"CAFÉ, CROISSANT\",-3.20\n02/10/2026,LOYER,-690.00\n03/10/2026,SALAIRE,2415.00\n";
        assert_eq!(detect_separator(text), ',');
        let records = split(text, ',');
        let header = detect_header(&records);
        assert_eq!(records[header].1, ["Date", "Libellé", "Montant"]);
        assert_eq!(records[header].0, 3);
        assert_eq!(records[header + 1].1[1], "CAFÉ, CROISSANT");
        assert_eq!(detect_separator("a;b;c\n1;2;3\n4;5;6\n"), ';');
        assert_eq!(detect_separator("a\tb\tc\n1\t2\t3\n"), '\t');
    }

    #[test]
    fn decimal_separators() {
        assert_eq!(detect_decimal(&["-12,50", "1 234,56", ""]), ',');
        assert_eq!(detect_decimal(&["-12.50", "1,234.56", "3"]), '.');
        assert_eq!(detect_decimal(&["12", "40"]), ',');
    }

    #[test]
    fn date_formats() {
        let d = |s: &str| s.parse::<NaiveDate>().unwrap();
        assert_eq!(
            DateFormat::DayMonthYear.parse("05/10/2026"),
            Some(d("2026-10-05"))
        );
        assert_eq!(
            DateFormat::Iso.parse("2026-10-05 14:03:11"),
            Some(d("2026-10-05"))
        );
        assert_eq!(
            DateFormat::DayMonthShortYear.parse("05-10-26"),
            Some(d("2026-10-05"))
        );
        assert_eq!(
            DateFormat::MonthDayYear.parse("10/05/2026"),
            Some(d("2026-10-05"))
        );
        assert_eq!(DateFormat::DayMonthYear.parse("31/02/2026"), None);
        assert_eq!(DateFormat::DayMonthYear.parse("2026-10-05"), None);

        let det = detect_date_format(&["05/10/2026", "28/10/2026"]).unwrap();
        assert_eq!(
            det,
            DateDetection {
                format: DateFormat::DayMonthYear,
                ambiguous: false
            }
        );
        let det = detect_date_format(&["10/28/2026", "10/05/2026"]).unwrap();
        assert_eq!(
            det,
            DateDetection {
                format: DateFormat::MonthDayYear,
                ambiguous: false
            }
        );
        let det = detect_date_format(&["05/10/2026", "06/10/2026"]).unwrap();
        assert!(det.ambiguous);
        assert_eq!(det.format, DateFormat::DayMonthYear);
        assert_eq!(
            detect_date_format(&["2026-10-05"]).unwrap().format,
            DateFormat::Iso
        );
        assert_eq!(detect_date_format(&["hier"]), None);
    }
}
