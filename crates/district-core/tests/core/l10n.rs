//! The core's words in the language the app chose: English exactly as it has
//! always been, and French where the catalog has it. There is no French yet, so
//! the French arm of every sentence is the English; what these tests hold is
//! which path each language takes: the service's English never reaches a French
//! screen, a known error code reads as the catalog's sentence for it, and numbers
//! are written the French way.
//!
//! Each test sets French for its own thread only ([`with_locale`]), so the rest
//! of the suite, which runs beside it in this process, stays in English.

use district_api::{ApiError, ErrorDetail};
use district_core::l10n::{Locale, api_error_text, with_locale};
use district_core::{
    AnalyticsReport, CAPABILITY_CATALOG, FailureText, NumberSearchForm, OwnedNumbers, SupportForm,
    capability_label, format_amount, format_cents, format_duration, format_money, month_label,
    price_label,
};
use district_model::AnalyticsRange;

use crate::support::fixture;

const NBSP: char = '\u{a0}';

fn french<T>(f: impl FnOnce() -> T) -> T {
    with_locale(Locale::FrCa, f)
}

fn english<T>(f: impl FnOnce() -> T) -> T {
    with_locale(Locale::En, f)
}

fn refused(code: Option<&str>, message: Option<&str>) -> ApiError {
    ApiError::Forbidden(ErrorDetail {
        message: message.map(str::to_owned),
        code: code.map(str::to_owned),
        degraded_regions: Vec::new(),
    })
}

#[test]
fn english_keeps_the_services_own_sentence() {
    let error = refused(Some("forbidden"), Some("Only the owner may do that."));
    let text = english(|| FailureText::from_api_error(&error));
    assert_eq!(text.message, "Only the owner may do that.");
}

#[test]
fn french_reads_a_known_code_as_the_catalog_sentence_never_the_services_english() {
    let error = refused(
        Some("workspace_owner_only"),
        Some("Only the owner may do that."),
    );
    let text = french(|| FailureText::from_api_error(&error));
    let expected = api_error_text::<&str>(Locale::FrCa, "workspace_owner_only", &[]).unwrap();
    assert_eq!(text.message, expected);
    assert_ne!(text.message, "Only the owner may do that.");
}

#[test]
fn french_reads_an_unknown_code_or_none_as_the_screens_own_words() {
    let ours = english(|| FailureText::from_api_error(&refused(None, None))).message;
    for error in [
        refused(Some("SOMETHING_NEW"), Some("A sentence in English.")),
        refused(None, Some("A sentence in English.")),
    ] {
        let text = french(|| FailureText::from_api_error(&error));
        assert_eq!(text.message, ours);
    }
}

#[test]
fn french_lists_the_regions_a_listing_code_names() {
    let error = ApiError::Rejected {
        status: 400,
        detail: ErrorDetail {
            message: Some("English.".to_owned()),
            code: Some("REGIONS_DEGRADED".to_owned()),
            degraded_regions: vec!["eu".to_owned(), "apac".to_owned()],
        },
    };
    let text = french(|| FailureText::from_api_error(&error));
    let expected = api_error_text(Locale::FrCa, "REGIONS_DEGRADED", &["EU", "APAC"]).unwrap();
    assert_eq!(text.message, expected);
    assert!(text.message.contains("EU, APAC"), "{}", text.message);
}

#[test]
fn numbers_are_written_the_french_way() {
    french(|| {
        assert_eq!(format_amount(1204.25), format!("1{NBSP}204,25"));
        assert_eq!(format_amount(412.0), "412");
        assert_eq!(format_duration(125), format!("2{NBSP}min 5{NBSP}s"));
        assert_eq!(format_duration(0), format!("0{NBSP}min"));
        assert_eq!(month_label("2026-08"), "ao\u{fb}t 2026");
        assert_eq!(month_label("2026-13"), "2026-13");
        assert_eq!(format_cents(24_900), format!("249,00{NBSP}$"));
        assert_eq!(
            format_money(24_900, Some("cad")),
            format!("249,00{NBSP}${NBSP}CA")
        );
        assert_eq!(price_label(Some(1.15), None).unwrap(), "1,15");
        assert_eq!(
            price_label(Some(1.15), Some("USD")).unwrap(),
            format!("1,15{NBSP}${NBSP}US")
        );
    });
    english(|| {
        assert_eq!(format_money(24_900, None), "$249.00");
        assert_eq!(format_money(24_900, Some("cad")), "CA$249.00");
        assert_eq!(price_label(Some(1.15), None).unwrap(), "1.15");
    });
}

#[test]
fn chart_labels_and_percentages_follow_the_language() {
    let report = AnalyticsReport {
        range: AnalyticsRange::SevenDays,
        response: fixture("district-analytics.json"),
        refreshing: false,
    };
    let (english_funnel, english_conversion) = english(|| (report.funnel(), report.conversion()));
    let (french_funnel, french_conversion) = french(|| (report.funnel(), report.conversion()));
    // The stored names are the vocabulary's English, so the labels read the same
    // until the French is written; the percentage takes its French space now.
    assert_eq!(
        english_funnel
            .iter()
            .map(|bar| &bar.label)
            .collect::<Vec<_>>(),
        french_funnel
            .iter()
            .map(|bar| &bar.label)
            .collect::<Vec<_>>()
    );
    assert!(english_conversion.ends_with('%') && !english_conversion.contains(NBSP));
    assert!(
        french_conversion.ends_with(&format!("{NBSP}%")),
        "{french_conversion}"
    );
    let french_sentiment = french(|| report.sentiment());
    assert!(!french_sentiment.is_empty());
}

#[test]
fn a_short_numbers_list_names_its_carriers_in_either_language() {
    let numbers = OwnedNumbers {
        numbers: Vec::new(),
        partial: true,
        failed_providers: vec!["telnyx".to_owned()],
        refreshing: false,
    };
    let english_note = english(|| numbers.partial_note()).unwrap();
    assert_eq!(french(|| numbers.partial_note()).unwrap(), english_note);
}

#[test]
fn the_localized_lists_match_their_english_constants() {
    english(|| {
        assert_eq!(
            NumberSearchForm::number_types(),
            NumberSearchForm::NUMBER_TYPES
        );
        assert_eq!(SupportForm::kinds(), SupportForm::KINDS);
        for (id, label) in CAPABILITY_CATALOG {
            assert_eq!(capability_label(id), Some(label), "{id}");
        }
    });
    assert_eq!(capability_label("a_tool_from_later"), None);
}
