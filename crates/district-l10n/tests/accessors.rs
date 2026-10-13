//! Every generated accessor, called in both languages and held to the text its
//! catalog gives it: the English as written, and the French as written or,
//! while an entry has none, the English again. A string argument is passed as
//! its own placeholder, so the expected text is the catalog's.
//!
//! The assertions are generated with the accessors, by `scripts/gen-l10n.py`,
//! and are not formatted, like the accessors themselves.

#[rustfmt::skip]
#[path = "generated/accessors.rs"]
mod generated;
