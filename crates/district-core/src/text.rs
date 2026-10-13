//! Where the core's words come from: `district-l10n`'s catalog, in the
//! language the app chose (`CoreConfig::locale`). Every sentence and label a
//! screen shows is one of its functions, called with [`l`].
//!
//! The helpers here keep the core's English exactly as it has always been
//! where a number or a stored value is written into it, and write French the
//! way `district-l10n`'s formats do.

use district_l10n::{Locale, WireCategory, format, wire_label};

pub(crate) use district_l10n::strings as s;

/// The language the core writes in now.
pub(crate) fn l() -> Locale {
    district_l10n::current()
}

/// A number the core writes as Rust prints it in English (`12.5`, `12`), and
/// in French as French writes it (`12,5`).
pub(crate) fn number(value: f64) -> String {
    match l() {
        Locale::En => value.to_string(),
        Locale::FrCa => format::number(Locale::FrCa, value),
    }
}

/// A percentage given out of 100: `38%`, and `38 %` in French.
pub(crate) fn percent(value: f64) -> String {
    match l() {
        Locale::En => format!("{value}%"),
        Locale::FrCa => format::percent(Locale::FrCa, value / 100.0),
    }
}

/// A value the service stores in English (a status, a stage's name), shown
/// as it is in English, as the core always has, and by its label otherwise.
pub(crate) fn wire(category: WireCategory, value: &str) -> String {
    match l() {
        Locale::En => value.to_owned(),
        Locale::FrCa => wire_label(category, value, Locale::FrCa),
    }
}

/// A sentence the service wrote itself, in English: kept in English and
/// dropped in French, where the screen says its own words instead.
pub(crate) fn english_only(theirs: Option<String>) -> Option<String> {
    theirs.filter(|_| l() == Locale::En)
}
