//! The error table: every code in `contracts/i18n/api-errors.json`, with its
//! English, and nothing for a code it does not have.

use std::fs;
use std::path::PathBuf;

use district_l10n::{FALLBACK_CODE, Locale, api_error_text};
use serde_json::Value;

fn contract() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/i18n/api-errors.json");
    serde_json::from_str(&fs::read_to_string(path).expect("api-errors.json")).expect("JSON")
}

#[test]
fn every_code_reads_as_the_services_english() {
    let contract = contract();
    let codes = contract["codes"].as_object().expect("codes");
    assert!(codes.len() >= 140, "{} codes", codes.len());
    let placeholder = contract["itemsPlaceholder"].as_str().expect("placeholder");
    let separator = contract["listSeparator"].as_str().expect("separator");
    for (code, sentence) in codes {
        let en = sentence["en"].as_str().expect("en");
        for locale in Locale::ALL {
            // No French yet: the French arm carries the English.
            if sentence.get("listField").is_some() {
                let items = ["eu", "apac"];
                let listed = en.replace(placeholder, &items.join(separator));
                assert_eq!(
                    api_error_text(locale, code, &items).as_deref(),
                    Some(listed.as_str())
                );
                assert_eq!(
                    api_error_text::<&str>(locale, code, &[]).as_deref(),
                    sentence["enWithoutList"].as_str(),
                    "{code}"
                );
            } else {
                assert_eq!(
                    api_error_text(locale, code, &["ignored"]).as_deref(),
                    Some(en),
                    "{code}"
                );
            }
        }
    }
    assert_eq!(contract["fallbackCode"], FALLBACK_CODE);
    assert!(api_error_text::<&str>(Locale::En, FALLBACK_CODE, &[]).is_some());
}

#[test]
fn a_code_the_contract_does_not_have_reads_as_nothing() {
    for code in ["", "NOT_A_CODE", "Forbidden", " forbidden"] {
        assert_eq!(
            api_error_text::<&str>(Locale::FrCa, code, &[]),
            None,
            "{code:?}"
        );
    }
    // Case matters: the service sends both of these, with their own sentences.
    assert!(api_error_text::<&str>(Locale::En, "forbidden", &[]).is_some());
    assert!(api_error_text::<&str>(Locale::En, "FORBIDDEN", &[]).is_some());
}
