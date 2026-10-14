//! The wire labels: every value in `contracts/i18n/wire-vocabulary.json`, the
//! English the desktop apps show for it today, and a generic label, never the
//! stored English, for a value this build does not know.

use std::fs;
use std::path::PathBuf;

use district_l10n::{Locale, WireCategory, placeholder_label, region_long_label, wire_label};
use serde_json::Value;

fn vocabulary() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/i18n/wire-vocabulary.json");
    serde_json::from_str(&fs::read_to_string(path).expect("wire-vocabulary.json")).expect("JSON")
}

/// What District AI for Linux shows for a value with no label (`humanize`).
fn humanize(raw: &str) -> String {
    let words = raw.trim().replace(['-', '_'], " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[test]
fn every_category_is_one_this_crate_names() {
    let vocabulary = vocabulary();
    let names: Vec<&str> = vocabulary["categories"]
        .as_object()
        .expect("categories")
        .keys()
        .map(String::as_str)
        .collect();
    let ours: Vec<&str> = WireCategory::ALL.iter().map(|c| c.name()).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    let mut ours_sorted = ours.clone();
    ours_sorted.sort_unstable();
    assert_eq!(sorted, ours_sorted);
    for category in WireCategory::ALL {
        assert_eq!(WireCategory::from_name(category.name()), Some(category));
    }
    assert_eq!(WireCategory::from_name("CallStatus"), None);
}

#[test]
fn every_value_reads_as_its_label_in_both_languages() {
    let vocabulary = vocabulary();
    let mut checked = 0;
    for (name, category) in vocabulary["categories"].as_object().expect("categories") {
        let wire = WireCategory::from_name(name).expect(name);
        for value in category["values"].as_array().expect("values") {
            let raw = value["value"].as_str().expect("value");
            let english = value["en"]
                .as_str()
                .map_or_else(|| humanize(raw), str::to_owned);
            for locale in Locale::ALL {
                assert_eq!(wire_label(wire, raw, locale), english, "{name} {raw}");
                // Matched trimmed and without regard to case.
                let shouted = format!("  {}  ", raw.to_uppercase());
                assert_eq!(wire_label(wire, &shouted, locale), english, "{name} {raw}");
            }
            if let Some(long) = value["enLong"].as_str() {
                assert_eq!(region_long_label(raw, Locale::FrCa), long);
            }
            checked += 1;
        }
    }
    assert!(checked > 40, "{checked} values");
}

#[test]
fn an_unknown_value_never_reads_as_its_english() {
    let fallback = vocabulary()["categories"]["transferReason"]["fallback"]["en"]
        .as_str()
        .expect("fallback")
        .to_owned();
    for locale in Locale::ALL {
        assert_eq!(
            wire_label(WireCategory::CallStatus, "queued-by-carrier", locale),
            "Other"
        );
        assert_eq!(
            wire_label(WireCategory::MemberRole, "owner", locale),
            "Other"
        );
        assert_eq!(
            wire_label(WireCategory::TransferReason, "sip_486", locale),
            fallback
        );
        assert_eq!(
            wire_label(WireCategory::TransferReason, "", locale),
            fallback
        );
        assert_eq!(wire_label(WireCategory::Region, " mx ", locale), "MX");
        assert_eq!(region_long_label("mx", locale), "MX");
    }
    // Every category with a fallback of its own reads an unknown value as it.
    let vocabulary = vocabulary();
    let mut with_fallback = 0;
    for (name, category) in vocabulary["categories"].as_object().expect("categories") {
        let Some(fallback) = category["fallback"]["en"].as_str() else {
            continue;
        };
        let wire = WireCategory::from_name(name).expect(name);
        assert_eq!(
            wire_label(wire, "not-a-value", Locale::En),
            fallback,
            "{name}"
        );
        with_fallback += 1;
    }
    assert!(
        with_fallback >= 3,
        "{with_fallback} categories with a fallback"
    );
}

#[test]
fn a_placeholder_reads_as_its_label_and_real_data_as_nothing() {
    let vocabulary = vocabulary();
    let mut checked = 0;
    let mut aliases = 0;
    for placeholder in vocabulary["placeholders"].as_array().expect("placeholders") {
        let value = placeholder["value"].as_str().expect("value");
        for locale in Locale::ALL {
            assert_eq!(placeholder_label(value, locale), Some(value), "{value}");
        }
        // An older spelling still in stored rows reads as the same placeholder.
        // The vendored copy writes its em dash as " - "; the stored row has the
        // dash itself.
        for alias in placeholder["aliases"].as_array().into_iter().flatten() {
            let alias = alias.as_str().expect("alias");
            let stored = alias.replace(" - ", " \u{2014} ");
            assert_ne!(stored, alias, "the alias names the dash it replaced");
            assert_eq!(placeholder_label(&stored, Locale::En), Some(value));
            assert_eq!(placeholder_label(alias, Locale::En), None, "{alias}");
            aliases += 1;
        }
        checked += 1;
    }
    assert!(checked > 10, "{checked} placeholders");
    assert!(aliases > 0, "no alias was checked");
    assert_eq!(placeholder_label("Ada Lovelace", Locale::FrCa), None);
    assert_eq!(placeholder_label("unknown", Locale::FrCa), None);
}
