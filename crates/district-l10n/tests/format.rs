//! The formats beyond the service's vectors: the core's own English, which
//! must not change, and the edges the vectors do not reach.

use district_l10n::Locale;
use district_l10n::format::{self, CivilDateTime, DateStyle, MoneyDisplay, MoneyOptions};

const NBSP: char = '\u{a0}';

#[test]
fn a_count_is_plain_in_english_and_grouped_in_french() {
    assert_eq!(format::integer(Locale::En, 1_204), "1204");
    assert_eq!(format::integer(Locale::En, -7), "-7");
    assert_eq!(format::integer(Locale::FrCa, 1_204), "1\u{a0}204");
    assert_eq!(
        format::integer(Locale::FrCa, -1_000_000),
        "-1\u{a0}000\u{a0}000"
    );
    assert_eq!(format::integer(Locale::FrCa, 999), "999");
}

#[test]
fn decimals_round_to_their_places() {
    assert_eq!(format::decimal(Locale::En, 1_204.5, 2), "1,204.50");
    assert_eq!(format::decimal(Locale::FrCa, 1_204.5, 2), "1\u{a0}204,50");
    assert_eq!(format::decimal(Locale::En, 3.0, 0), "3");
    assert_eq!(format::decimal(Locale::En, -0.001, 2), "0.00");
    assert_eq!(format::number(Locale::En, 2.000_4), "2");
    assert_eq!(format::number(Locale::En, -0.000_1), "0");
}

#[test]
fn a_value_that_is_not_finite_writes_nothing() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for locale in Locale::ALL {
            assert_eq!(format::number(locale, value), "");
            assert_eq!(format::decimal(locale, value, 2), "");
            assert_eq!(format::percent(locale, value), "");
        }
    }
}

#[test]
fn the_billing_price_keeps_this_apps_english_without_a_currency() {
    assert_eq!(format::price(Locale::En, 24_900, None), "$249.00");
    assert_eq!(format::price(Locale::En, 120_425, None), "$1204.25");
    assert_eq!(format::price(Locale::En, -5, None), "-$0.05");
    assert_eq!(format::price(Locale::FrCa, 24_900, None), "249,00\u{a0}$");
    assert_eq!(
        format::price(Locale::FrCa, 120_425, None),
        "1\u{a0}204,25\u{a0}$"
    );
    assert_eq!(format::price(Locale::En, 24_900, Some("cad")), "CA$249.00");
    assert_eq!(
        format::price(Locale::FrCa, 24_900, Some("cad")),
        "249,00\u{a0}$\u{a0}CA"
    );
    assert_eq!(
        format::price(Locale::FrCa, 24_900, Some("usd")),
        "249,00\u{a0}$\u{a0}US"
    );
}

#[test]
fn money_in_other_currencies_and_below_zero() {
    let symbol = MoneyOptions::default();
    let narrow = MoneyOptions {
        display: MoneyDisplay::Narrow,
        disambiguate_dollar: false,
    };
    let code = MoneyOptions {
        display: MoneyDisplay::Code,
        disambiguate_dollar: true,
    };
    assert_eq!(
        format::money(Locale::En, 1_000, "gbp", symbol),
        "\u{a3}10.00"
    );
    assert_eq!(
        format::money(Locale::FrCa, 1_000, "GBP", symbol),
        "10,00\u{a0}\u{a3}"
    );
    assert_eq!(format::money(Locale::En, 1_000, "aud", symbol), "A$10.00");
    assert_eq!(
        format::money(Locale::FrCa, 1_000, "aud", symbol),
        "10,00\u{a0}$\u{a0}AU"
    );
    assert_eq!(format::money(Locale::En, 1_000, "aud", narrow), "$10.00");
    assert_eq!(
        format::money(Locale::En, 1_000, "jpy", narrow),
        "JPY\u{a0}10.00"
    );
    assert_eq!(
        format::money(Locale::En, 1_000, "usd", code),
        "USD\u{a0}10.00"
    );
    assert_eq!(format::money(Locale::En, -1_050, "usd", symbol), "-$10.50");
    assert_eq!(
        format::money(Locale::FrCa, -1_050, "eur", symbol),
        "-10,50\u{a0}\u{20ac}"
    );
    assert_eq!(
        format::money_or_bare(Locale::En, 5, Some("eur")),
        "\u{20ac}0.05"
    );
    assert_eq!(format::iso_amount(Locale::En, 2.0, "  "), "2.00");
    assert_eq!(
        format::iso_amount(Locale::FrCa, 2.0, "x"),
        format!("X{NBSP}2.00")
    );
}

#[test]
fn a_unix_time_becomes_the_wall_clock_of_its_zone() {
    // 2026-03-01 04:30 UTC is still February in Toronto.
    let when = CivilDateTime::from_unix(1_772_339_400, -5 * 3_600);
    assert_eq!(
        when,
        CivilDateTime {
            year: 2026,
            month: 2,
            day: 28,
            hour: 23,
            minute: 30
        }
    );
    // Before 1970, and a leap day.
    let when = CivilDateTime::from_unix(-2_208_988_800 + 59 * 86_400, 0);
    assert_eq!((when.year, when.month, when.day), (1900, 3, 1));
    let leap = CivilDateTime::from_unix(951_782_400, 0);
    assert_eq!((leap.year, leap.month, leap.day), (2000, 2, 29));
}

#[test]
fn dates_in_every_style_and_month() {
    let day = |month: u8, day: u8| CivilDateTime {
        year: 2026,
        month,
        day,
        hour: 9,
        minute: 0,
    };
    assert_eq!(
        format::date(Locale::En, day(1, 5), DateStyle::Full),
        "Monday, January 5, 2026"
    );
    assert_eq!(
        format::date(Locale::FrCa, day(1, 4), DateStyle::Full),
        "dimanche 4 janvier 2026"
    );
    assert_eq!(
        format::date(Locale::FrCa, day(7, 1), DateStyle::Medium),
        "1er juill. 2026"
    );
    assert_eq!(
        format::date(Locale::FrCa, day(2, 3), DateStyle::Short),
        "2026-02-03"
    );
    assert_eq!(
        format::date_time(Locale::FrCa, day(9, 21), DateStyle::Long, false),
        "21 septembre 2026 \u{e0} 9 h"
    );
    assert_eq!(
        format::date_time(Locale::En, day(9, 21), DateStyle::Long, true),
        "September 21, 2026, 9:00 a.m."
    );
    // Out of range months read as the nearest, never a panic.
    assert_eq!(format::month_year(Locale::En, 2026, 0), "Jan 2026");
    assert_eq!(format::month_year(Locale::FrCa, 2026, 13), "d\u{e9}c. 2026");
    assert_eq!(format::month_year(Locale::FrCa, 2026, 8), "ao\u{fb}t 2026");
}

#[test]
fn noon_and_midnight_on_a_twelve_hour_clock() {
    let at = |hour: u8, minute: u8| CivilDateTime {
        year: 2026,
        month: 1,
        day: 1,
        hour,
        minute,
    };
    assert_eq!(format::time(Locale::En, at(12, 0), true), "12:00 p.m.");
    assert_eq!(format::time(Locale::En, at(0, 0), true), "12:00 a.m.");
    assert_eq!(format::time(Locale::FrCa, at(0, 0), false), "0 h");
    assert!(format::twelve_hour_clock(Locale::En, "Australia/Sydney"));
    assert!(!format::twelve_hour_clock(Locale::FrCa, "America/Toronto"));
    assert!(!format::twelve_hour_clock(Locale::En, "Europe/Paris"));
}

#[test]
fn a_negative_length_reads_as_none() {
    assert_eq!(format::minutes_seconds(Locale::En, -5), "0m");
    assert_eq!(
        format::minutes_seconds(Locale::FrCa, 7_265),
        "121\u{a0}min 5\u{a0}s"
    );
    assert_eq!(format::call_duration(-1), "0s");
}

#[test]
fn a_day_of_a_month_reads_as_an_axis_labels_it() {
    assert_eq!(format::day_month(Locale::En, 8, 9), "Aug 9");
    assert_eq!(format::day_month(Locale::FrCa, 8, 9), "9 ao\u{fb}t");
    assert_eq!(format::day_month(Locale::FrCa, 9, 1), "1er sept.");
    assert_eq!(format::day_month(Locale::En, 9, 1), "Sep 1");
}
