//! A model built in French: the core writes in the language its configuration
//! names. A binary of its own, because the language is the process's and every
//! other test runs in English.
//!
//! There is no French yet, so the French arm of every sentence carries the
//! English and `FR_COMPLETE` is false; what this holds is that French is the
//! arm taken.

use district_api::{ApiError, ErrorDetail};
use district_core::l10n::{self, Locale, api_error_text, strings};
use district_core::{CoreConfig, FailureText, Model};

fn config(locale: Locale) -> CoreConfig {
    CoreConfig {
        web_base_url: "https://www.distronode.com".to_owned(),
        app_version: "0.0.0-test".to_owned(),
        calls_available: false,
        in_app_purchases: false,
        locale,
    }
}

#[test]
fn a_french_model_writes_in_french_until_told_otherwise() {
    assert!(
        !std::hint::black_box(l10n::FR_COMPLETE),
        "French is still being written"
    );
    let (mut model, _) = Model::new(config(Locale::FrCa));
    assert_eq!(model.config().locale, Locale::FrCa);
    assert_eq!(l10n::current(), Locale::FrCa);

    // The French arm is the one taken: the sentence for a known code, never the
    // service's English.
    let refusal = ApiError::NotFound(ErrorDetail {
        message: Some("No such thing, in English.".to_owned()),
        code: Some("contact_not_found".to_owned()),
        degraded_regions: Vec::new(),
    });
    let text = FailureText::from_api_error(&refusal);
    assert_eq!(
        Some(text.message),
        api_error_text::<&str>(Locale::FrCa, "contact_not_found", &[])
    );
    assert_eq!(
        FailureText::from_retry_reason(district_api::RetryReason::Offline).message,
        strings::failure_offline(Locale::FrCa)
    );

    model.set_locale(Locale::En);
    assert_eq!(model.config().locale, Locale::En);
    assert_eq!(l10n::current(), Locale::En);
    assert_eq!(
        FailureText::from_api_error(&refusal).message,
        "No such thing, in English."
    );
}

#[test]
fn the_default_configuration_is_english() {
    assert_eq!(Locale::default(), Locale::En);
}
