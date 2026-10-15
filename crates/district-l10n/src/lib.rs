//! The words the District AI desktop apps' core shows, in English and in
//! Canadian French, and the formats around them.
//!
//! - [`Locale`]: the two languages, and [`Locale::from_tags`], which picks one
//!   from the user's preferred languages as the operating system lists them.
//! - The language the core speaks in ([`current`]): one per process, set by the
//!   app through `district_core::CoreConfig::locale`. The core stays neutral
//!   about it; the app decides.
//! - [`strings`]: every sentence and label the core writes, generated from
//!   `catalog/core.toml` by `tools/l10n/l10n.py`. Each is a function of the
//!   locale, so a missing key fails the build rather than showing nothing.
//! - [`support_strings`]: the support area's, from `catalog/support.toml`.
//! - [`numbers_strings`]: managing phone numbers, from `catalog/numbers.toml`.
//! - [`scheduling_strings`]: the booking scheduler's administration, from
//!   `catalog/scheduling.toml`.
//! - [`api_error_text`]: the sentence for an error code the service sends, from
//!   `contracts/i18n/api-errors.json` through `catalog/api_errors.toml`.
//! - [`wire_label`] and [`placeholder_label`]: what a screen shows for a value the
//!   service stores in English (a call status, a role, a region), from
//!   `contracts/i18n/wire-vocabulary.json` through `catalog/wire.toml`. The value
//!   itself never changes; only its label does.
//! - [`support_status_label`]: a support request's status, from its name or its
//!   category, never the help desk's own word.
//! - [`format`](mod@format): money, dates, times, durations, numbers, percentages, lists
//!   and plural categories, held byte for byte to the service's own formatting
//!   vectors (`contracts/i18n/format-vectors.json`).
//!
//! Until the French is written, the French arm of every generated function
//! carries the English and [`FR_COMPLETE`] is false. The apps keep French off
//! until it is true.
//!
//! The generated files under `src/generated/` and `tests/accessors.rs` are
//! written by `python3 scripts/gen-l10n.py`; CI runs it with `--check`, which
//! fails when any of them differs from what the catalogs and the contracts
//! produce.

#![forbid(unsafe_code)]

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

mod errors;
pub mod format;
mod support_status;
mod wire;

pub use errors::{FALLBACK_CODE, api_error_text};
pub use support_status::support_status_label;
pub use wire::{WireCategory, placeholder_label, region_long_label, wire_label};

/// The core's own sentences and labels, from `catalog/core.toml`.
#[rustfmt::skip]
#[path = "generated/core.rs"]
pub mod strings;

/// The English of each sentence in `catalog/core.toml` that takes no
/// argument, as a constant named after its key (`failure.offline` is
/// `FAILURE_OFFLINE`).
///
/// For the public constants `district-core` has always had, which keep their
/// English so that nothing built on them changes. What a screen shows comes
/// from [`strings`], in the locale's language.
#[rustfmt::skip]
#[path = "generated/english.rs"]
pub mod english;

/// The service's error sentences, from `catalog/api_errors.toml`. Reached by
/// code through [`api_error_text`].
#[rustfmt::skip]
#[path = "generated/api_errors.rs"]
pub mod api_error_strings;

/// The wire values' labels, from `catalog/wire.toml`. Reached by value through
/// [`wire_label`] and [`placeholder_label`].
#[rustfmt::skip]
#[path = "generated/wire.rs"]
pub mod wire_strings;

/// The support area's sentences and labels, reporting AI-generated content
/// among them, from `catalog/support.toml`. The text a report sends to the
/// support desk is English in both languages: it is read by Distronode's staff.
#[rustfmt::skip]
#[path = "generated/support.rs"]
pub mod support_strings;

/// The phone numbers screen's sentences and labels for managing the numbers a
/// workspace holds and its carrier account, from `catalog/numbers.toml`.
#[rustfmt::skip]
#[path = "generated/numbers.rs"]
pub mod numbers_strings;

/// The booking scheduler's administration's sentences and labels, from
/// `catalog/scheduling.toml`: a catalog of its own, beside the core's.
#[rustfmt::skip]
#[path = "generated/scheduling.rs"]
pub mod scheduling_strings;

/// True when every catalog has French for every entry. Until then the French
/// arm of each generated function carries the English.
pub const FR_COMPLETE: bool = strings::FR_COMPLETE
    && api_error_strings::FR_COMPLETE
    && wire_strings::FR_COMPLETE
    && support_strings::FR_COMPLETE
    && numbers_strings::FR_COMPLETE
    && scheduling_strings::FR_COMPLETE;

/// A language the core writes in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Locale {
    /// English, the language every sentence is written in first.
    #[default]
    En,
    /// Canadian French.
    FrCa,
}

impl Locale {
    /// Both, in a stable order.
    pub const ALL: [Self; 2] = [Self::En, Self::FrCa];

    /// The locale for a user whose preferred languages are `tags`, most
    /// preferred first, as the operating system lists them: BCP 47 tags
    /// (`fr-CA`, `en-US`) or POSIX locale names (`fr_CA.UTF-8`).
    ///
    /// The first tag in a language the core writes decides: a tag whose primary
    /// language is `fr` (any region) gives [`FrCa`](Self::FrCa), and one whose
    /// primary language is `en` gives [`En`](Self::En). A tag in any other
    /// language is passed over, so `[de, fr]` is French and `[en, fr]` English.
    /// No tag in either language, or no tag at all, gives English.
    pub fn from_tags<S: AsRef<str>>(tags: &[S]) -> Self {
        tags.iter()
            .find_map(|tag| match primary_language(tag.as_ref()) {
                "fr" => Some(Self::FrCa),
                "en" => Some(Self::En),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// The BCP 47 tag: `en` or `fr-CA`.
    pub fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::FrCa => "fr-CA",
        }
    }
}

/// A tag's primary language subtag in lower case: what comes before the first
/// `-`, `_`, `.` or `@`.
fn primary_language(tag: &str) -> &str {
    let tag = tag.trim();
    let end = tag.find(['-', '_', '.', '@']).unwrap_or(tag.len());
    let primary = &tag[..end];
    if primary.eq_ignore_ascii_case("fr") {
        "fr"
    } else if primary.eq_ignore_ascii_case("en") {
        "en"
    } else {
        ""
    }
}

/// The process's language: false for English, true for French. One flag
/// rather than a lock: it is read on every sentence and written once.
static FRENCH: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// A language set for this thread only by [`with_locale`], over the
    /// process's.
    static OVERRIDE: Cell<Option<Locale>> = const { Cell::new(None) };
}

/// The language the core writes in on this thread: the one [`with_locale`] set
/// around the current call, else the process's ([`set_current`]), which is
/// English until the app says otherwise.
pub fn current() -> Locale {
    OVERRIDE.with(Cell::get).unwrap_or_else(|| {
        if FRENCH.load(Ordering::Relaxed) {
            Locale::FrCa
        } else {
            Locale::En
        }
    })
}

/// Sets the process's language. `district_core::Model` calls this with its
/// `CoreConfig::locale`; an app calls nothing here itself.
pub fn set_current(locale: Locale) {
    FRENCH.store(locale == Locale::FrCa, Ordering::Relaxed);
}

/// Runs `f` with `locale` as this thread's language, whatever the process's
/// is, and restores the previous one after, also when `f` panics. For tests
/// that render both languages side by side in one process.
pub fn with_locale<R>(locale: Locale, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Locale>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(OVERRIDE.with(|cell| cell.replace(Some(locale))));
    f()
}
