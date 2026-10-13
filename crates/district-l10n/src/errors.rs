//! The sentence for an error code the service sends.
//!
//! The service names most refusals with a machine-readable code, in the body
//! or in the `X-Distronode-Error-Code` header, beside an English sentence.
//! The code is what a screen translates: its sentence here is the one the
//! service's own portal shows for it. Each code's entry in
//! `catalog/api_errors.toml` is generated from `contracts/i18n/api-errors.json`.

use crate::Locale;

#[path = "generated/api_error_table.rs"]
#[rustfmt::skip]
mod table;

/// The code the service uses for a failure it gives no code of its own.
pub const FALLBACK_CODE: &str = "unknown";

/// What the separator between the items of a listing sentence is.
const LIST_SEPARATOR: &str = ", ";

/// The sentence for `code` in `locale`, or `None` for a code the service's
/// contract does not have, for which the screen says its own words.
///
/// A sentence that names what was refused (the fields of a request, the
/// regions that did not answer) lists `items`, joined by commas; with no items
/// it reads without the list.
pub fn api_error_text<S: AsRef<str>>(locale: Locale, code: &str, items: &[S]) -> Option<String> {
    let items: Vec<&str> = items.iter().map(AsRef::as_ref).collect();
    let items = (!items.is_empty()).then(|| items.join(LIST_SEPARATOR));
    table::sentence(locale, code, items.as_deref())
}
