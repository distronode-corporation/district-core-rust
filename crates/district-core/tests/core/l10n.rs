//! The core's words in the language the app chose: English exactly as it has
//! always been, and French from the catalog, which has French for every entry.
//! What these tests hold is which path each language takes: the service's
//! English never reaches a French screen, a known error code reads as the
//! catalog's sentence for it, and numbers are written the French way.
//!
//! Each test sets French for its own thread only ([`with_locale`]), so the rest
//! of the suite, which runs beside it in this process, stays in English.

use district_api::{ApiError, ErrorDetail};
use district_core::l10n::{Locale, api_error_text, with_locale};
use district_core::{
    AnalyticsReport, CAPABILITY_CATALOG, FailureText, NumberSearchForm, OwnedNumbers, SupportForm,
    call_duration_label, capability_label, format_amount, format_cents, format_duration,
    format_money, invoice_price, month_label, price_label, subscription_price,
};
use district_model::{AccountBillingResponse, AnalyticsRange, CallSummary};
use serde_json::Value;

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
    let ours = french(|| FailureText::from_api_error(&refused(None, None))).message;
    assert_ne!(
        ours,
        english(|| FailureText::from_api_error(&refused(None, None))).message
    );
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
        // English prices stay `$249.00` whatever the currency: a plan decision.
        assert_eq!(format_money(24_900, Some("cad")), "$249.00");
        assert_eq!(format_money(24_900, Some("usd")), "$249.00");
        assert_eq!(price_label(Some(1.15), None).unwrap(), "1.15");
    });
}

fn format_vectors(name: &str) -> Vec<Value> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/i18n/format-vectors.json");
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("format-vectors.json"))
            .expect("JSON");
    doc["categories"]
        .as_array()
        .expect("categories")
        .iter()
        .flat_map(|category| category["vectors"].as_array().cloned().unwrap_or_default())
        .filter(|vector| vector["fn"] == name)
        .collect()
}

#[test]
fn billing_amounts_name_their_currency_in_french_as_the_service_does() {
    // Every formatMoney vector in French with the default display, for the
    // currencies a subscription or an invoice can carry.
    let mut checked = 0;
    for vector in format_vectors("formatMoney") {
        let display = vector["options"]["display"].as_str();
        if vector["locale"] != "fr" || !matches!(display, None | Some("symbol")) {
            continue;
        }
        if vector["options"].get("disambiguateDollar").is_some() {
            continue;
        }
        let cents = vector["minorUnits"].as_i64().expect("minorUnits");
        let currency = vector["currency"].as_str().expect("currency");
        let output = vector["output"].as_str().expect("output");
        assert_eq!(
            french(|| format_money(cents, Some(currency))),
            output,
            "{vector}"
        );
        // English is the dollar amount it has always been.
        assert_eq!(
            english(|| format_money(cents, Some(currency))),
            english(|| format_cents(cents)),
            "{vector}"
        );
        checked += 1;
    }
    assert!(checked >= 6, "{checked} vectors");

    let billing: AccountBillingResponse = crate::support::fixture("district-billing.json");
    let subscription = &billing.subscriptions[0];
    let invoice = &billing.invoices[0];
    assert_eq!(subscription.currency.as_deref(), Some("usd"));
    assert_eq!(
        english(|| subscription_price(subscription)).as_deref(),
        Some("$249.00")
    );
    assert_eq!(
        french(|| subscription_price(subscription)).as_deref(),
        Some(format!("249,00{NBSP}${NBSP}US").as_str())
    );
    assert_eq!(english(|| invoice_price(invoice)), "$249.00");
    assert_eq!(
        french(|| invoice_price(invoice)),
        format!("249,00{NBSP}${NBSP}US")
    );
    let canadian = district_model::BillingInvoice {
        currency: Some("cad".to_owned()),
        ..invoice.clone()
    };
    assert_eq!(english(|| invoice_price(&canadian)), "$249.00");
    assert_eq!(
        french(|| invoice_price(&canadian)),
        format!("249,00{NBSP}${NBSP}CA")
    );
    let unnamed = district_model::BillingInvoice {
        currency: None,
        ..invoice.clone()
    };
    assert_eq!(french(|| invoice_price(&unnamed)), format!("249,00{NBSP}$"));
}

#[test]
fn a_call_rows_length_is_the_services_in_english_and_read_from_its_seconds_in_french() {
    let calls: Vec<CallSummary> = crate::support::fixture("district-calls.json");
    assert_eq!(calls[0].duration, "1m 5s");
    assert_eq!(english(|| call_duration_label(&calls[0])), "1m 5s");
    assert_eq!(
        french(|| call_duration_label(&calls[0])),
        format!("1{NBSP}min 5{NBSP}s")
    );
    let call = |duration: &str, seconds: Option<i64>| CallSummary {
        duration: duration.to_owned(),
        duration_raw: seconds,
        ..calls[0].clone()
    };
    // The French reading of every formatMinutesSeconds vector, from the seconds,
    // and from the service's own text when it sent no seconds.
    let mut checked = 0;
    for vector in format_vectors("formatMinutesSeconds") {
        let seconds = vector["seconds"].as_i64().expect("seconds");
        let fr = vector["fr"].as_str().expect("fr");
        let wire = district_core::l10n::format::call_duration(seconds);
        assert_eq!(
            french(|| call_duration_label(&call(&wire, Some(seconds)))),
            fr
        );
        assert_eq!(french(|| call_duration_label(&call(&wire, None))), fr);
        assert_eq!(
            english(|| call_duration_label(&call(&wire, Some(seconds)))),
            wire
        );
        checked += 1;
    }
    assert!(checked >= 4, "{checked} vectors");
    assert_eq!(
        french(|| call_duration_label(&call("2m 34s", None))),
        format!("2{NBSP}min 34{NBSP}s")
    );
    // A length that cannot be read is shown as the service wrote it.
    assert_eq!(french(|| call_duration_label(&call("", None))), "");
    assert_eq!(
        french(|| call_duration_label(&call("a while", None))),
        "a while"
    );
}

#[test]
fn the_trends_days_are_the_services_in_english_and_written_from_iso_date_in_french() {
    let report = AnalyticsReport {
        range: AnalyticsRange::SevenDays,
        response: fixture("district-analytics.json"),
        refreshing: false,
    };
    let english_labels: Vec<String> = english(|| report.trend())
        .into_iter()
        .map(|bar| bar.label)
        .collect();
    let french_labels: Vec<String> = french(|| report.trend())
        .into_iter()
        .map(|bar| bar.label)
        .collect();
    let points = &report.response.engagement_trends;
    assert_eq!(english_labels[0], "Aug 9");
    assert_eq!(points[0].iso_date, "2026-08-09");
    assert_eq!(french_labels[0], "9 ao\u{fb}t");
    for (point, label) in points.iter().zip(&english_labels) {
        assert_eq!(&point.date, label);
    }
    let mut first = report.clone();
    first.response.engagement_trends[0].iso_date = "2026-09-01".to_owned();
    first.response.engagement_trends[1].iso_date = "not a day".to_owned();
    let labels: Vec<String> = french(|| first.trend())
        .into_iter()
        .map(|bar| bar.label)
        .collect();
    assert_eq!(labels[0], "1er sept.");
    assert_eq!(labels[1], "not a day");
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
    // The stored names are the vocabulary's English; each bar reads as its French
    // label, and the percentage takes its French space.
    assert_eq!(english_funnel.len(), french_funnel.len());
    assert!(!french_funnel.is_empty());
    for (english_bar, french_bar) in english_funnel.iter().zip(&french_funnel) {
        assert_ne!(english_bar.label, french_bar.label);
    }
    assert!(english_conversion.ends_with('%') && !english_conversion.contains(NBSP));
    assert!(
        french_conversion.ends_with(&format!("{NBSP}%")),
        "{french_conversion}"
    );
    let french_sentiment = french(|| report.sentiment());
    assert!(!french_sentiment.is_empty());
    // The label is translated; the name an app keys the band's colour on is
    // the service's, in either language.
    let english_sentiment = english(|| report.sentiment());
    for (english_band, french_band) in english_sentiment.iter().zip(&french_sentiment) {
        assert_ne!(english_band.label, french_band.label);
        assert_eq!(english_band.name, french_band.name);
    }
    assert_eq!(french_sentiment[0].name, "Positive Sentiment");
    assert_eq!(french_sentiment[0].label, "Sentiment positif");
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
    let french_note = french(|| numbers.partial_note()).unwrap();
    assert!(english_note.contains("telnyx"), "{english_note}");
    assert!(french_note.contains("telnyx"), "{french_note}");
    assert_ne!(french_note, english_note);
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
