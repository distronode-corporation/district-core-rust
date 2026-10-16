//! Numbers, money, dates, times, durations, percentages, lists and plural
//! categories, in English and Canadian French.
//!
//! Hand-written tables, not the operating system's locale data: the same input
//! gives the same bytes on every system and every version of it. Each function
//! that has a counterpart on the service is held byte for byte to its vectors in
//! `contracts/i18n/format-vectors.json`, in both languages; the doc of each
//! names the counterpart. English follows the service's `en-CA` portal tag for
//! numbers, dates and times, and `en-US` for money, as the service does.
//!
//! The one space French uses inside a number, before a unit or a currency and
//! inside « » is the no-break space U+00A0 ([`NBSP`]).
//!
//! Two functions keep the core's own English instead of the service's, because
//! the core's text must not change where a catalog takes it over:
//! [`integer`], which the catalogs use for counts, writes English digits
//! without a thousands separator, and [`price`] with no currency writes the
//! `$249.00` the billing screen has always shown.

use crate::Locale;

/// The no-break space, U+00A0.
pub const NBSP: char = '\u{a0}';

// Numbers.

/// The digits of `n` with a separator between each group of three.
fn grouped(digits: &str, separator: char) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(separator);
        }
        out.push(digit);
    }
    out
}

/// The thousands separator and the decimal separator.
fn separators(locale: Locale) -> (char, char) {
    match locale {
        Locale::En => (',', '.'),
        Locale::FrCa => (NBSP, ','),
    }
}

/// `value`, already rounded and written with a `.` and no grouping (`-1204.25`),
/// in the locale's separators: `-1,204.25`, `-1 204,25`.
fn localize_plain(plain: &str, locale: Locale) -> String {
    let (group, decimal) = separators(locale);
    let (sign, unsigned) = match plain.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", plain),
    };
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (unsigned, None),
    };
    let mut out = format!("{sign}{}", grouped(whole, group));
    if let Some(fraction) = fraction {
        out.push(decimal);
        out.push_str(fraction);
    }
    out
}

/// `value` to exactly `digits` decimals, with a `.` and no grouping, and no
/// minus sign on a value that rounds to zero.
fn fixed(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    if text.starts_with('-') && text[1..].bytes().all(|b| b == b'0' || b == b'.') {
        text[1..].to_owned()
    } else {
        text
    }
}

/// `value` to at most `digits` decimals (one or more, so the text always has
/// a `.` to trim back to), trailing zeros dropped.
fn trimmed(value: f64, digits: usize) -> String {
    fixed(value, digits)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

/// A count, as the catalogs' `{count}` placeholders write it: `1204` in
/// English, which is how the core has always written a count, and `1 204` in
/// French, with a no-break space.
pub fn integer(locale: Locale, n: i64) -> String {
    match locale {
        Locale::En => n.to_string(),
        Locale::FrCa => localize_plain(&n.to_string(), locale),
    }
}

/// A number with up to three decimals: `1,204.25`, `1 204,25`. The service's
/// `formatNumber`. Not finite: the empty string.
pub fn number(locale: Locale, value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    localize_plain(&trimmed(value, 3), locale)
}

/// A number with exactly `digits` decimals: `1,204.50`, `1 204,50`. Not
/// finite: the empty string.
pub fn decimal(locale: Locale, value: f64, digits: usize) -> String {
    if !value.is_finite() {
        return String::new();
    }
    localize_plain(&fixed(value, digits), locale)
}

/// A ratio (0 to 1, not 0 to 100) as a percentage with up to one decimal:
/// `38%`, `38 %`, `99.9%`, `99,9 %`. The service's `formatPercent`. Not finite:
/// the empty string.
pub fn percent(locale: Locale, ratio: f64) -> String {
    if !ratio.is_finite() {
        return String::new();
    }
    let digits = localize_plain(&trimmed(ratio * 100.0, 1), locale);
    match locale {
        Locale::En => format!("{digits}%"),
        Locale::FrCa => format!("{digits}{NBSP}%"),
    }
}

// Money.

/// How a currency is written beside an amount. The service's `MoneyDisplay`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MoneyDisplay {
    /// The locale's own symbol, which marks a foreign currency: `$39` and
    /// `CA$59` in English, `39 $ US` and `59 $ CA` in French.
    #[default]
    Symbol,
    /// The bare glyph, `$` for every dollar. Only beside a printed currency code.
    Narrow,
    /// The ISO code: `USD 2.00`, `2,00 USD`.
    Code,
}

/// The options of [`money`]. The service's `MoneyOptions`, without the number
/// of decimals, which is always two here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MoneyOptions {
    /// How the currency is written.
    pub display: MoneyDisplay,
    /// US dollars as `US$39` in English, for a line that quotes several
    /// dollars at once. French already writes `39 $ US`.
    pub disambiguate_dollar: bool,
}

/// The symbol for an upper-case ISO code: (English symbol, French symbol,
/// narrow symbol). `None` for a currency written by its code.
fn symbols(code: &str) -> Option<(&'static str, &'static str, &'static str)> {
    match code {
        "USD" => Some(("$", "$\u{a0}US", "$")),
        // French Canadian dollars are `$ CA`, the glossary's form, not ICU's
        // bare `$`.
        "CAD" => Some(("CA$", "$\u{a0}CA", "$")),
        "EUR" => Some(("\u{20ac}", "\u{20ac}", "\u{20ac}")),
        "GBP" => Some(("\u{a3}", "\u{a3}", "\u{a3}")),
        "AUD" => Some(("A$", "$\u{a0}AU", "$")),
        _ => None,
    }
}

/// `amount` (already two decimals, `.`, no grouping, maybe negative) with its
/// currency `code` (three ASCII letters, upper case).
fn with_currency(locale: Locale, plain: &str, code: &str, options: MoneyOptions) -> String {
    let symbol = match (options.display, symbols(code)) {
        (MoneyDisplay::Code, _) | (_, None) => code,
        (MoneyDisplay::Narrow, Some((_, _, narrow))) => narrow,
        (MoneyDisplay::Symbol, Some((english, french, _))) => match locale {
            Locale::En if options.disambiguate_dollar && code == "USD" => "US$",
            Locale::En => english,
            Locale::FrCa => french,
        },
    };
    let (sign, unsigned) = match plain.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", plain),
    };
    let amount = localize_plain(unsigned, locale);
    match locale {
        // A symbol that ends in a letter (a code) is set off by a no-break space.
        Locale::En if symbol.ends_with(|c: char| c.is_ascii_alphabetic()) => {
            format!("{sign}{symbol}{NBSP}{amount}")
        }
        Locale::En => format!("{sign}{symbol}{amount}"),
        Locale::FrCa => format!("{sign}{amount}{NBSP}{symbol}"),
    }
}

/// `value` in a currency named by `code`, with two decimals. A code that is not
/// three ASCII letters is written as it came, upper-cased, beside the plain
/// amount (`BOGUS 249.00`), as the service does.
fn currency_amount(locale: Locale, value: f64, code: &str, options: MoneyOptions) -> String {
    let plain = fixed(value, 2);
    if code.len() == 3 && code.bytes().all(|b| b.is_ascii_alphabetic()) {
        with_currency(locale, &plain, &code.to_ascii_uppercase(), options)
    } else {
        [code.trim().to_uppercase(), plain]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\u{a0}")
    }
}

/// Minor units (cents) in `currency` (`usd`, `cad`, `eur`, any case): `$249.00`,
/// `249,00 $ US`, `CA$249.00`, `249,00 $ CA`. The service's `formatMoney`.
pub fn money(locale: Locale, minor_units: i64, currency: &str, options: MoneyOptions) -> String {
    currency_amount(locale, minor_units as f64 / 100.0, currency, options)
}

/// [`money`] when the currency is known, the bare amount when it is not:
/// `249.00`, `249,00`. The service's `formatMoneyOrBare`.
pub fn money_or_bare(locale: Locale, minor_units: i64, currency: Option<&str>) -> String {
    match currency {
        Some(currency) => money(locale, minor_units, currency, MoneyOptions::default()),
        None => localize_plain(&fixed(minor_units as f64 / 100.0, 2), locale),
    }
}

/// Major units in any ISO 4217 code a carrier returned: `XYZ 249.00`,
/// `249,00 XYZ`. The service's `formatIsoAmount`.
pub fn iso_amount(locale: Locale, major: f64, iso_code: &str) -> String {
    currency_amount(locale, major, iso_code, MoneyOptions::default())
}

/// An amount the billing screens show, in minor units, in `currency` when the
/// service names one. With no currency, English is the `$249.00` this app has
/// always shown (no thousands separator) and French the bare dollar, `249,00 $`.
pub fn price(locale: Locale, minor_units: i64, currency: Option<&str>) -> String {
    match (currency, locale) {
        (Some(currency), _) => money(locale, minor_units, currency, MoneyOptions::default()),
        (None, Locale::En) => {
            let sign = if minor_units < 0 { "-" } else { "" };
            let cents = minor_units.unsigned_abs();
            format!("{sign}${}.{:02}", cents / 100, cents % 100)
        }
        (None, Locale::FrCa) => {
            let options = MoneyOptions {
                display: MoneyDisplay::Narrow,
                disambiguate_dollar: false,
            };
            money(locale, minor_units, "usd", options)
        }
    }
}

// Durations.

/// A length as minutes and seconds: `1m 5s`, `1 min 5 s`; whole minutes drop
/// the seconds, under a minute reads seconds alone, and zero or less reads
/// `0m`. The service's `formatMinutesSeconds`. French never breaks a length:
/// a no-break space (U+00A0) sits between each number and its unit and
/// between the minutes and the seconds.
pub fn minutes_seconds(locale: Locale, seconds: i64) -> String {
    let unit = |n: i64, english: &str, french: &str| match locale {
        Locale::En => format!("{n}{english}"),
        Locale::FrCa => format!("{}{NBSP}{french}", integer(locale, n)),
    };
    if seconds <= 0 {
        return unit(0, "m", "min");
    }
    match (seconds / 60, seconds % 60) {
        (0, rest) => unit(rest, "s", "s"),
        (minutes, 0) => unit(minutes, "m", "min"),
        (minutes, rest) => {
            let between = match locale {
                Locale::En => ' ',
                Locale::FrCa => NBSP,
            };
            format!(
                "{}{between}{}",
                unit(minutes, "m", "min"),
                unit(rest, "s", "s")
            )
        }
    }
}

/// The `duration` string the service puts on a call row, the same in every
/// language: `0s`, `0m 5s`, `62m 5s`. The service's `formatCallDuration`.
pub fn call_duration(seconds: i64) -> String {
    if seconds <= 0 {
        "0s".to_owned()
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

// Dates and times.

/// A calendar day and a time of day on a wall clock, with no zone. The app
/// knows the user's zone; [`CivilDateTime::from_unix`] turns an instant and the
/// zone's offset at that instant into one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CivilDateTime {
    /// The year.
    pub year: i32,
    /// The month, 1 to 12.
    pub month: u8,
    /// The day of the month, 1 to 31.
    pub day: u8,
    /// The hour, 0 to 23.
    pub hour: u8,
    /// The minute, 0 to 59.
    pub minute: u8,
}

impl CivilDateTime {
    /// The wall clock at Unix time `seconds` in a zone `utc_offset_seconds`
    /// ahead of UTC at that instant (negative west of Greenwich).
    pub fn from_unix(seconds: i64, utc_offset_seconds: i32) -> Self {
        let local = seconds + i64::from(utc_offset_seconds);
        let days = local.div_euclid(86_400);
        let of_day = local.rem_euclid(86_400);
        // Howard Hinnant's days-from-civil, inverted.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Self {
            year: year as i32,
            month: month as u8,
            day: day as u8,
            hour: (of_day / 3_600) as u8,
            minute: (of_day % 3_600 / 60) as u8,
        }
    }

    /// The day of the week, 0 for Sunday to 6 for Saturday.
    fn weekday(self) -> usize {
        // Sakamoto's method.
        const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let year = if self.month < 3 {
            self.year - 1
        } else {
            self.year
        };
        let index = usize::from(self.month.clamp(1, 12) - 1);
        let weekday = year + year / 4 - year / 100 + year / 400 + T[index] + i32::from(self.day);
        weekday.rem_euclid(7) as usize
    }
}

/// How much of a date to write. The service's `DateStyle`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DateStyle {
    /// `2026-08-15`, in both languages.
    Short,
    /// `Aug 15, 2026`, `15 août 2026`.
    #[default]
    Medium,
    /// `August 15, 2026`, `15 août 2026`.
    Long,
    /// `Saturday, August 15, 2026`, `samedi 15 août 2026`.
    Full,
}

const EN_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const EN_MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const FR_MONTHS: [&str; 12] = [
    "janvier",
    "f\u{e9}vrier",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "ao\u{fb}t",
    "septembre",
    "octobre",
    "novembre",
    "d\u{e9}cembre",
];
const FR_MONTHS_SHORT: [&str; 12] = [
    "janv.",
    "f\u{e9}vr.",
    "mars",
    "avr.",
    "mai",
    "juin",
    "juill.",
    "ao\u{fb}t",
    "sept.",
    "oct.",
    "nov.",
    "d\u{e9}c.",
];
const EN_WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const FR_WEEKDAYS: [&str; 7] = [
    "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi",
];

/// Month `month` (1 to 12) from `names`; out of range reads as the nearest.
fn month_name(names: &[&'static str; 12], month: u8) -> &'static str {
    names[usize::from(month.clamp(1, 12) - 1)]
}

/// A month and year: `Aug 2026`, `août 2026`. The abbreviated month in
/// English, as the core's charts have always labelled their months.
pub fn month_year(locale: Locale, year: i32, month: u8) -> String {
    match locale {
        Locale::En => format!("{} {year}", month_name(&EN_MONTHS_SHORT, month)),
        Locale::FrCa => format!("{} {year}", month_name(&FR_MONTHS_SHORT, month)),
    }
}

/// A day of a month with no year, as a chart's axis labels it: `Aug 9`,
/// `9 août`, and `1er août` for the first, as [`date`] writes it.
pub fn day_month(locale: Locale, month: u8, day: u8) -> String {
    match locale {
        Locale::En => format!("{} {day}", month_name(&EN_MONTHS_SHORT, month)),
        Locale::FrCa if day == 1 => format!("1er {}", month_name(&FR_MONTHS_SHORT, month)),
        Locale::FrCa => format!("{day} {}", month_name(&FR_MONTHS_SHORT, month)),
    }
}

/// A date: see [`DateStyle`]. French writes the first of a month `1er`. The
/// service's `formatDate`.
pub fn date(locale: Locale, when: CivilDateTime, style: DateStyle) -> String {
    let CivilDateTime {
        year, month, day, ..
    } = when;
    match (locale, style) {
        (_, DateStyle::Short) => format!("{year:04}-{month:02}-{day:02}"),
        (Locale::En, DateStyle::Medium) => {
            format!("{} {day}, {year}", month_name(&EN_MONTHS_SHORT, month))
        }
        (Locale::En, DateStyle::Long) => format!("{} {day}, {year}", month_name(&EN_MONTHS, month)),
        (Locale::En, DateStyle::Full) => format!(
            "{}, {} {day}, {year}",
            EN_WEEKDAYS[when.weekday()],
            month_name(&EN_MONTHS, month)
        ),
        (Locale::FrCa, _) => {
            let day = if day == 1 {
                "1er".to_owned()
            } else {
                day.to_string()
            };
            let month = match style {
                DateStyle::Medium => month_name(&FR_MONTHS_SHORT, month),
                _ => month_name(&FR_MONTHS, month),
            };
            match style {
                DateStyle::Full => format!("{} {day} {month} {year}", FR_WEEKDAYS[when.weekday()]),
                _ => format!("{day} {month} {year}"),
            }
        }
    }
}

/// Whether English reads the clock in `timezone` (an IANA zone) as twelve
/// hours: only in an `America/` or `Australia/` zone. French never does. The
/// service's `hourCycleFor`.
pub fn twelve_hour_clock(locale: Locale, timezone: &str) -> bool {
    locale == Locale::En && (timezone.starts_with("America/") || timezone.starts_with("Australia/"))
}

/// A time of day: `3:04 p.m.` or `15:04` in English (see
/// [`twelve_hour_clock`]), `15 h 04` and `9 h` in French. The service's
/// `formatTime` with its default short style.
pub fn time(locale: Locale, when: CivilDateTime, twelve_hour: bool) -> String {
    let CivilDateTime { hour, minute, .. } = when;
    match locale {
        Locale::En if twelve_hour => {
            let meridiem = if hour < 12 { "a.m." } else { "p.m." };
            let hour = match hour % 12 {
                0 => 12,
                other => other,
            };
            format!("{hour}:{minute:02} {meridiem}")
        }
        Locale::En => format!("{hour:02}:{minute:02}"),
        Locale::FrCa if minute == 0 => format!("{hour} h"),
        Locale::FrCa => format!("{hour} h {minute:02}"),
    }
}

/// A date and a time of day, joined as each language joins them: `, ` in
/// English, and in French `, ` after a short or medium date and ` à ` after a
/// long or full one. The service's `formatDateTime` with a short time.
pub fn date_time(
    locale: Locale,
    when: CivilDateTime,
    style: DateStyle,
    twelve_hour: bool,
) -> String {
    let joiner = match (locale, style) {
        (Locale::FrCa, DateStyle::Long | DateStyle::Full) => " \u{e0} ",
        _ => ", ",
    };
    format!(
        "{}{joiner}{}",
        date(locale, when, style),
        time(locale, when, twelve_hour)
    )
}

// Lists and plurals.

/// A list joined by "and": `A, B, and C` in English, `A, B et C` in French.
pub fn list<S: AsRef<str>>(locale: Locale, items: &[S]) -> String {
    let items: Vec<&str> = items.iter().map(AsRef::as_ref).collect();
    let and = match locale {
        Locale::En => "and",
        Locale::FrCa => "et",
    };
    match items.as_slice() {
        [] => String::new(),
        [only] => (*only).to_owned(),
        [first, second] => format!("{first} {and} {second}"),
        [init @ .., last] => {
            let comma = match locale {
                Locale::En => ",",
                Locale::FrCa => "",
            };
            format!("{}{comma} {and} {last}", init.join(", "))
        }
    }
}

/// A plural category, as the catalogs choose between their forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PluralCategory {
    /// English 1; French 0 and 1.
    One,
    /// French only: a non-zero multiple of a million (`1 000 000 d'appels`).
    Many,
    /// Everything else.
    Other,
}

/// The plural category of the count `n`.
pub fn plural_category(locale: Locale, n: i64) -> PluralCategory {
    match locale {
        Locale::En if n == 1 => PluralCategory::One,
        Locale::En => PluralCategory::Other,
        Locale::FrCa if n == 0 || n == 1 => PluralCategory::One,
        Locale::FrCa if n % 1_000_000 == 0 => PluralCategory::Many,
        Locale::FrCa => PluralCategory::Other,
    }
}
