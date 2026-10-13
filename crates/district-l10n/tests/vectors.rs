//! The formats held byte for byte to the service's own, in both languages:
//! every vector in `contracts/i18n/format-vectors.json`, which the service
//! records from the functions its portal formats with.

use std::fs;
use std::path::PathBuf;

use district_l10n::Locale;
use district_l10n::format::{
    self, CivilDateTime, DateStyle, MoneyDisplay, MoneyOptions, PluralCategory,
};
use serde_json::Value;

fn vectors() -> Value {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/i18n/format-vectors.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_str(&text).expect("format-vectors.json is JSON")
}

fn category(name: &str) -> Value {
    vectors()["categories"]
        .as_array()
        .expect("categories")
        .iter()
        .find(|category| category["category"] == name)
        .unwrap_or_else(|| panic!("no category {name}"))
        .clone()
}

fn cases(name: &str) -> Vec<Value> {
    let cases = category(name)["vectors"]
        .as_array()
        .unwrap_or_else(|| panic!("{name} has no vectors"))
        .clone();
    assert!(!cases.is_empty(), "{name} has no vectors");
    cases
}

fn locale(case: &Value) -> Locale {
    match case["locale"].as_str().expect("locale") {
        "en" => Locale::En,
        "fr" => Locale::FrCa,
        other => panic!("locale {other}"),
    }
}

fn both(case: &Value) -> [(Locale, &str); 2] {
    [
        (Locale::En, case["en"].as_str().expect("en")),
        (Locale::FrCa, case["fr"].as_str().expect("fr")),
    ]
}

/// Unix seconds of an ISO 8601 UTC instant, `2026-08-01T12:00:00.000Z`.
fn unix(iso: &str) -> i64 {
    let number = |range: std::ops::Range<usize>| iso[range].parse::<i64>().expect("digits");
    let (year, month, day) = (number(0..4), number(5..7), number(8..10));
    let (hour, minute, second) = (number(11..13), number(14..16), number(17..19));
    assert!(iso.ends_with('Z'), "{iso} is UTC");
    // Howard Hinnant's days-from-civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + hour * 3_600 + minute * 60 + second
}

#[test]
fn money_matches_every_vector() {
    let mut checked = 0;
    for case in cases("money") {
        let locale = locale(&case);
        let options = &case["options"];
        let display = match options["display"].as_str() {
            None | Some("symbol") => MoneyDisplay::Symbol,
            Some("narrow") => MoneyDisplay::Narrow,
            Some("code") => MoneyDisplay::Code,
            Some(other) => panic!("display {other}"),
        };
        let options = MoneyOptions {
            display,
            disambiguate_dollar: options["disambiguateDollar"].as_bool().unwrap_or(false),
        };
        let actual = match case["fn"].as_str().expect("fn") {
            "formatMoney" => format::money(
                locale,
                case["minorUnits"].as_i64().expect("minorUnits"),
                case["currency"].as_str().expect("currency"),
                options,
            ),
            "formatMoneyOrBare" => format::money_or_bare(
                locale,
                case["minorUnits"].as_i64().expect("minorUnits"),
                case["currency"].as_str(),
            ),
            "formatIsoAmount" => format::iso_amount(
                locale,
                case["major"].as_f64().expect("major"),
                case["isoCode"].as_str().expect("isoCode"),
            ),
            other => panic!("money has a function this test does not know: {other}"),
        };
        assert_eq!(actual, case["output"].as_str().expect("output"), "{case}");
        checked += 1;
    }
    assert!(checked > 100, "{checked} money vectors");
}

#[test]
fn dates_match_every_vector() {
    for case in cases("dates") {
        assert_eq!(case["fn"], "formatDate");
        let input = case["input"].as_str().expect("input");
        // Every date vector is at noon UTC, the same calendar day in the zones
        // it names, so the date needs no zone.
        assert_eq!(&input[11..19], "12:00:00", "{case}");
        let when = CivilDateTime::from_unix(unix(input), 0);
        let style = match case["style"].as_str().expect("style") {
            "short" => DateStyle::Short,
            "medium" => DateStyle::Medium,
            "long" => DateStyle::Long,
            "full" => DateStyle::Full,
            other => panic!("style {other}"),
        };
        for (locale, expected) in both(&case) {
            assert_eq!(format::date(locale, when, style), expected, "{case}");
        }
    }
}

#[test]
fn times_match_every_vector() {
    for case in cases("times") {
        let input = case["input"].as_str().expect("input");
        let timezone = case["timezone"].as_str().expect("timezone");
        // The zone's offset at that instant, from the wall clock the vector
        // records beside it.
        let local = case["local"].as_str().expect("local");
        let local_minutes = local[..2].parse::<i32>().expect("hour") * 60
            + local[3..].parse::<i32>().expect("minute");
        let utc_minutes = input[11..13].parse::<i32>().expect("hour") * 60
            + input[14..16].parse::<i32>().expect("minute");
        let offset_minutes = (local_minutes - utc_minutes + 12 * 60).rem_euclid(24 * 60) - 12 * 60;
        let when = CivilDateTime::from_unix(unix(input), offset_minutes * 60);
        assert_eq!(format!("{:02}:{:02}", when.hour, when.minute), local);
        for (locale, expected) in both(&case) {
            let twelve = format::twelve_hour_clock(locale, timezone);
            let actual = match case["fn"].as_str().expect("fn") {
                "formatTime" => format::time(locale, when, twelve),
                "formatDateTime" => format::date_time(locale, when, DateStyle::Medium, twelve),
                other => panic!("times has a function this test does not know: {other}"),
            };
            assert_eq!(actual, expected, "{case}");
        }
    }
}

#[test]
fn durations_match_every_vector() {
    for case in cases("durations") {
        let seconds = case["seconds"].as_i64().expect("seconds");
        match case["fn"].as_str().expect("fn") {
            "formatMinutesSeconds" => {
                for (locale, expected) in both(&case) {
                    assert_eq!(format::minutes_seconds(locale, seconds), expected, "{case}");
                }
            }
            "formatCallDuration" => {
                assert_eq!(format::call_duration(seconds), case["output"], "{case}");
            }
            other => panic!("durations has a function this test does not know: {other}"),
        }
    }
}

#[test]
fn numbers_match_every_vector() {
    for case in cases("numbers") {
        assert_eq!(case["fn"], "formatNumber");
        let value = case["value"].as_f64().expect("value");
        for (locale, expected) in both(&case) {
            assert_eq!(format::number(locale, value), expected, "{case}");
        }
    }
}

#[test]
fn percentages_match_every_vector() {
    for case in cases("percentages") {
        assert_eq!(case["fn"], "formatPercent");
        let ratio = case["ratio"].as_f64().expect("ratio");
        for (locale, expected) in both(&case) {
            assert_eq!(format::percent(locale, ratio), expected, "{case}");
        }
    }
}

#[test]
fn lists_and_plurals_have_no_vectors_and_follow_the_toolkit_rules() {
    // The service has no function for either, so the contract records them with
    // no source; the rules are the native apps' toolkit's (i18n/README.md).
    for name in ["lists", "plurals"] {
        assert!(
            category(name)["source"].is_null(),
            "{name} now has a source"
        );
    }
    let three = ["A", "B", "C"];
    assert_eq!(format::list(Locale::FrCa, &three), "A, B et C");
    assert_eq!(format::list(Locale::En, &three), "A, B, and C");
    assert_eq!(format::list(Locale::FrCa, &["A", "B"]), "A et B");
    assert_eq!(format::list(Locale::En, &["A", "B"]), "A and B");
    assert_eq!(format::list(Locale::En, &["A"]), "A");
    assert_eq!(format::list::<&str>(Locale::FrCa, &[]), "");

    assert_eq!(
        format::plural_category(Locale::FrCa, 0),
        PluralCategory::One
    );
    assert_eq!(
        format::plural_category(Locale::FrCa, 1),
        PluralCategory::One
    );
    assert_eq!(
        format::plural_category(Locale::FrCa, 2),
        PluralCategory::Other
    );
    assert_eq!(
        format::plural_category(Locale::FrCa, 2_000_000),
        PluralCategory::Many
    );
    assert_eq!(
        format::plural_category(Locale::En, 0),
        PluralCategory::Other
    );
    assert_eq!(format::plural_category(Locale::En, 1), PluralCategory::One);
    assert_eq!(
        format::plural_category(Locale::En, 1_000_000),
        PluralCategory::Other
    );
}
