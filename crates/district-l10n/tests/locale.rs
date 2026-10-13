//! Choosing the language, and the language the core is in.

use district_l10n::{Locale, current, set_current, with_locale};

#[test]
fn the_first_tag_in_a_language_the_core_writes_decides() {
    assert_eq!(Locale::from_tags(&["fr-CA"]), Locale::FrCa);
    assert_eq!(Locale::from_tags(&["fr-FR", "en-US"]), Locale::FrCa);
    assert_eq!(Locale::from_tags(&["FR"]), Locale::FrCa);
    assert_eq!(Locale::from_tags(&["fr_CA.UTF-8"]), Locale::FrCa);
    assert_eq!(Locale::from_tags(&["fr@euro"]), Locale::FrCa);
    assert_eq!(Locale::from_tags(&[" fr-BE "]), Locale::FrCa);
    assert_eq!(Locale::from_tags(&["de-DE", "fr-CA"]), Locale::FrCa);
    assert_eq!(Locale::from_tags(&["en-US", "fr-CA"]), Locale::En);
    assert_eq!(Locale::from_tags(&["en_GB.UTF-8"]), Locale::En);
    assert_eq!(Locale::from_tags(&["de-DE", "es"]), Locale::En);
    assert_eq!(Locale::from_tags(&["C", "POSIX"]), Locale::En);
    assert_eq!(Locale::from_tags(&["fra", "french", ""]), Locale::En);
    assert_eq!(Locale::from_tags::<&str>(&[]), Locale::En);
    assert_eq!(Locale::from_tags(&[String::from("fr")]), Locale::FrCa);
}

#[test]
fn each_locale_has_its_tag() {
    assert_eq!(Locale::En.tag(), "en");
    assert_eq!(Locale::FrCa.tag(), "fr-CA");
    assert_eq!(Locale::default(), Locale::En);
}

#[test]
fn the_process_language_and_a_threads_own() {
    // The one test in this binary that sets the process's language.
    assert_eq!(current(), Locale::En);
    set_current(Locale::FrCa);
    assert_eq!(current(), Locale::FrCa);
    assert_eq!(with_locale(Locale::En, current), Locale::En);
    assert_eq!(current(), Locale::FrCa);
    set_current(Locale::En);
    assert_eq!(current(), Locale::En);
    let nested = with_locale(Locale::FrCa, || {
        (current(), with_locale(Locale::En, current), current())
    });
    assert_eq!(nested, (Locale::FrCa, Locale::En, Locale::FrCa));
    // Restored when the closure panics, too.
    let caught = std::panic::catch_unwind(|| with_locale(Locale::FrCa, || panic!("inside")));
    assert!(caught.is_err());
    assert_eq!(current(), Locale::En);
}
