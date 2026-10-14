//! The error table: every code in `contracts/i18n/api-errors.json`, with its
//! English and its French, and nothing for a code it does not have.

use std::fs;
use std::path::PathBuf;

use district_l10n::{FALLBACK_CODE, Locale, api_error_text};
use serde_json::Value;

fn contract() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/i18n/api-errors.json");
    serde_json::from_str(&fs::read_to_string(path).expect("api-errors.json")).expect("JSON")
}

/// Codes whose reviewed French says « disponible » where the website's says
/// « offerte »: the W3 review chose it, and the website follows later.
const FRENCH_NOT_THE_WEBSITES: [&str; 2] = ["direct_dial_disabled", "setup_not_available"];

#[test]
fn every_code_reads_as_the_services_sentence() {
    let contract = contract();
    let codes = contract["codes"].as_object().expect("codes");
    assert!(codes.len() >= 140, "{} codes", codes.len());
    let placeholder = contract["itemsPlaceholder"].as_str().expect("placeholder");
    let separator = contract["listSeparator"].as_str().expect("separator");
    for (code, sentence) in codes {
        let en = sentence["en"].as_str().expect("en");
        for locale in Locale::ALL {
            let (field, without) = match locale {
                Locale::En => ("en", "enWithoutList"),
                Locale::FrCa => ("fr", "frWithoutList"),
            };
            if locale == Locale::FrCa && FRENCH_NOT_THE_WEBSITES.contains(&code.as_str()) {
                let text = api_error_text(locale, code, &["ignored"]).expect(code);
                assert!(text.contains("disponible"), "{code}: {text}");
                assert_ne!(text, en, "{code}");
                continue;
            }
            let text = sentence[field].as_str().expect(field);
            if sentence.get("listField").is_some() {
                let items = ["eu", "apac"];
                let listed = text.replace(placeholder, &items.join(separator));
                assert_eq!(
                    api_error_text(locale, code, &items).as_deref(),
                    Some(listed.as_str())
                );
                assert_eq!(
                    api_error_text::<&str>(locale, code, &[]).as_deref(),
                    sentence[without].as_str(),
                    "{code}"
                );
            } else {
                assert_eq!(
                    api_error_text(locale, code, &["ignored"]).as_deref(),
                    Some(text),
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
