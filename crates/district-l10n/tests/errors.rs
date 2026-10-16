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

/// French this catalog already words as the service is about to: (code, the
/// contract's words, the catalog's words). « enregistrement » is banned by the
/// glossary (the product records nothing), and the service's French moves to
/// « entrées DNS » too. Once `contracts/i18n/` is synced past that change the
/// contract no longer holds the old words, and
/// `the_french_ahead_of_the_contract_still_is` says to remove the row.
const FRENCH_AHEAD_OF_THE_CONTRACT: &[(&str, &str, &str)] = &[(
    "sending_domain_provider_changed",
    "les nouveaux enregistrements DNS",
    "les nouvelles entrées DNS",
)];

fn french_as_the_catalog_words_it(code: &str, text: &str) -> String {
    FRENCH_AHEAD_OF_THE_CONTRACT
        .iter()
        .filter(|(ahead, _, _)| *ahead == code)
        .fold(text.to_owned(), |text, (_, contract, catalog)| {
            text.replace(contract, catalog)
        })
}

#[test]
fn the_french_ahead_of_the_contract_still_is() {
    let contract = contract();
    for (code, words, _) in FRENCH_AHEAD_OF_THE_CONTRACT {
        let french = contract["codes"][code]["fr"].as_str().expect(code);
        assert!(
            french.contains(words),
            "{code}: the contract no longer says {words:?}; remove its row from FRENCH_AHEAD_OF_THE_CONTRACT"
        );
    }
}

#[test]
fn every_code_reads_as_the_services_sentence() {
    let contract = contract();
    let codes = contract["codes"].as_object().expect("codes");
    assert!(codes.len() >= 140, "{} codes", codes.len());
    let placeholder = contract["itemsPlaceholder"].as_str().expect("placeholder");
    let separator = contract["listSeparator"].as_str().expect("separator");
    for (code, sentence) in codes {
        for locale in Locale::ALL {
            let (field, without) = match locale {
                Locale::En => ("en", "enWithoutList"),
                Locale::FrCa => ("fr", "frWithoutList"),
            };
            let text = sentence[field].as_str().expect(field);
            let text = &match locale {
                Locale::En => text.to_owned(),
                Locale::FrCa => french_as_the_catalog_words_it(code, text),
            };
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
                    Some(text.as_str()),
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
