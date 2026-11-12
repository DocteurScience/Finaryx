//! Translation helpers. The interface is written in French (source
//! language); `xgettext` extracts the strings marked with [`N_`] and the
//! arguments of `gettext`/`ngettext`. Dates and amounts keep the `fr-FR`
//! format (see `dates` and `money`).

pub use gettextrs::{gettext, ngettext};

/// Marks a string for extraction without translating it: the caller
/// translates it when it is shown (`gettext(field.label())`).
#[allow(non_snake_case)]
pub const fn N_(s: &'static str) -> &'static str {
    s
}

/// `ngettext` with a count given as any integer (clamped).
pub fn ngettext_n(singular: &str, plural: &str, n: impl TryInto<u32>) -> String {
    ngettext(singular, plural, n.try_into().unwrap_or(u32::MAX))
}
