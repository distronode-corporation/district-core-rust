//! The scheduler's times on the member's wall clock: an RFC 3339 instant read
//! in the IANA zone on their scheduler profile (not the computer's), then
//! written by `district_l10n::format`.
//!
//! An unknown zone reads as UTC rather than hiding the row: the instant is
//! still real, and the screen names the zone it used.

use std::time::SystemTime;

use district_l10n::Locale;
use district_l10n::format::{self, CivilDateTime, DateStyle};
use district_l10n::scheduling_strings as ss;
use jiff::Timestamp;
use jiff::civil::Date;
use jiff::tz::TimeZone;

use crate::text::l;

/// "Times in America/Toronto": named on every screen that shows a time.
pub fn times_in(zone: &str) -> String {
    ss::times_in(l(), zone)
}

/// A calendar day, as the scheduler names one: `YYYY-MM-DD`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SchedulingDay {
    /// The year.
    pub year: i16,
    /// The month, 1 to 12.
    pub month: i8,
    /// The day of the month.
    pub day: i8,
}

impl SchedulingDay {
    /// The day `now` falls on in `zone`.
    pub fn today(now: SystemTime, zone: &str) -> Self {
        let civil = wall_clock(timestamp(now), zone);
        Self {
            year: civil.year as i16,
            month: civil.month as i8,
            day: civil.day as i8,
        }
    }

    /// Reads `YYYY-MM-DD`, or `None` for anything that is not a real day.
    pub fn parse(key: &str) -> Option<Self> {
        let date: Date = key.parse().ok()?;
        Some(Self {
            year: date.year(),
            month: date.month(),
            day: date.day(),
        })
    }

    /// The day as the scheduler names it: `2026-09-14`, zero-padded.
    pub fn key(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// The day as a reader names it: `Sep 14, 2026`, `14 sept. 2026`.
    pub fn label(self) -> String {
        format::date(l(), self.civil(), DateStyle::Medium)
    }

    fn civil(self) -> CivilDateTime {
        CivilDateTime {
            year: i32::from(self.year),
            month: self.month as u8,
            day: self.day as u8,
            hour: 0,
            minute: 0,
        }
    }

    /// Whole days from `self` to `other`, by the calendar, not by elapsed time.
    pub(crate) fn days_until(self, other: Self) -> i64 {
        // Both were read from a real date, which jiff can hold.
        let date = |d: Self| Date::new(d.year, d.month, d.day).unwrap_or_default();
        date(self)
            .until(date(other))
            .map(|span| i64::from(span.get_days()))
            .unwrap_or_default()
    }
}

/// `now` as an instant. A clock past jiff's range (the year 9999) reads as the
/// epoch, which no screen shows as anything but a date long gone.
fn timestamp(now: SystemTime) -> Timestamp {
    Timestamp::try_from(now).unwrap_or_default()
}

/// The zone named `zone`, or UTC for one the database does not have.
fn zone_named(zone: &str) -> TimeZone {
    TimeZone::get(zone).unwrap_or(TimeZone::UTC)
}

/// The wall clock in `zone` at `instant`.
fn wall_clock(instant: Timestamp, zone: &str) -> CivilDateTime {
    let offset = zone_named(zone).to_offset(instant).seconds();
    CivilDateTime::from_unix(instant.as_second(), offset)
}

/// An RFC 3339 instant on the wall clock in `zone`, or `None` when it does not
/// parse.
pub(crate) fn civil(text: &str, zone: &str) -> Option<CivilDateTime> {
    let instant: Timestamp = text.parse().ok()?;
    Some(wall_clock(instant, zone))
}

/// The day an RFC 3339 instant falls on in `zone`.
pub(crate) fn day_of(text: &str, zone: &str) -> Option<SchedulingDay> {
    let at = civil(text, zone)?;
    Some(SchedulingDay {
        year: at.year as i16,
        month: at.month as i8,
        day: at.day as i8,
    })
}

/// Whether English reads the clock in `zone` as twelve hours: the service's
/// rule.
fn twelve_hour(zone: &str) -> bool {
    format::twelve_hour_clock(l(), zone)
}

/// `Sep 12, 2026, 10:00 a.m.`: a date and a time in `zone`, or `None` when the
/// instant does not parse.
pub(crate) fn date_time(text: &str, zone: &str) -> Option<String> {
    let at = civil(text, zone)?;
    Some(format::date_time(
        l(),
        at,
        DateStyle::Medium,
        twelve_hour(zone),
    ))
}

/// A date and time in `zone`, or the catalog's "Unknown" for an instant that
/// does not parse: a corrupt row, not one to hide.
pub(crate) fn date_time_or_unknown(text: &str, zone: &str) -> String {
    date_time(text, zone).unwrap_or_else(|| ss::unknown_time(l()).to_owned())
}

/// `Today 10:00 a.m.`, `Tomorrow 9:00 a.m.`, or the date and time: when
/// something starts, relative to `now`, for a glance. A day in the past reads
/// as its date: there is no "Yesterday".
pub(crate) fn relative(text: &str, zone: &str, now: SystemTime) -> Option<String> {
    let at = civil(text, zone)?;
    let clock = format::time(l(), at, twelve_hour(zone));
    let day = day_of(text, zone)?;
    Some(match SchedulingDay::today(now, zone).days_until(day) {
        0 => ss::overview_today(l(), &clock),
        1 => ss::overview_tomorrow(l(), &clock),
        _ => format::date_time(l(), at, DateStyle::Medium, twelve_hour(zone)),
    })
}

/// Why a time field cannot be saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeFieldError {
    /// It does not read as a time of day: "Enter a time as HH:MM.".
    Unreadable,
    /// It reads as one, but past 23:59 (`25:00`, `9:75`).
    OutOfRange,
}

impl TimeFieldError {
    /// The sentence the field shows.
    pub fn message(self) -> &'static str {
        match self {
            Self::Unreadable => ss::time_format_error(l()),
            Self::OutOfRange => ss::time_range_error(l()),
        }
    }
}

/// The hour and the minute digits of `text`, both up to two digits, unchecked.
/// Reads `09:30`, `9:30`, `9 h 30`, `9h30`, `9 h` and `9h`, with any space
/// (ordinary or no-break) around the `h`, in either language.
fn time_field_parts(text: &str) -> Option<(u32, u32)> {
    let text = text.trim();
    let digits = |s: &str, max: usize| {
        (!s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u32>().ok())
            .flatten()
    };
    let (hour, minute) = if let Some((hour, minute)) = text.split_once(':') {
        (hour, Some(minute))
    } else {
        let (hour, minute) = text.split_once(['h', 'H'])?;
        let spaces: &[char] = &[' ', '\u{a0}', '\u{202f}'];
        let minute = minute.trim_matches(spaces);
        (
            hour.trim_end_matches(spaces),
            (!minute.is_empty()).then_some(minute),
        )
    };
    let hour = digits(hour, 2)?;
    let minute = match minute {
        // After a colon the minutes are two digits; after an `h` too, or none.
        Some(minute) if minute.len() == 2 => digits(minute, 2)?,
        Some(_) => return None,
        None => 0,
    };
    Some((hour, minute))
}

/// What a member typed in a time field, as the scheduler takes it: `HH:MM`,
/// zero-padded (`9 h 30`, `9h30`, `9:30` and `09:30` are all `09:30`). The
/// scheduler refuses anything else, so this is what a save sends.
pub fn time_field_wire(text: &str) -> Result<String, TimeFieldError> {
    let (hour, minute) = time_field_parts(text).ok_or(TimeFieldError::Unreadable)?;
    if hour > 23 || minute > 59 {
        return Err(TimeFieldError::OutOfRange);
    }
    Ok(format!("{hour:02}:{minute:02}"))
}

/// A scheduler time (`09:00`) as a time field shows it to edit: unchanged in
/// English, and `9 h`, `9 h 30` in French. [`time_field_wire`] reads either
/// back. A value that is not a time comes back as it was.
pub fn time_field_text(wire: &str) -> String {
    match (l(), time_field_wire(wire)) {
        (Locale::FrCa, Ok(normal)) => hour_minute(&normal, "UTC"),
        _ => wire.to_owned(),
    }
}

/// `09:00` as a reader reads it in `zone`'s convention: `9:00 a.m.`, `09:00`
/// or `9 h`. A value that is not `HH:MM` comes back as it was.
pub(crate) fn hour_minute(value: &str, zone: &str) -> String {
    let parsed = value
        .split_once(':')
        .and_then(|(hour, minute)| Some((hour.parse::<u8>().ok()?, minute.parse::<u8>().ok()?)))
        .filter(|(hour, minute)| *hour < 24 && *minute < 60);
    match parsed {
        Some((hour, minute)) => format::time(
            l(),
            CivilDateTime {
                year: 2000,
                month: 1,
                day: 1,
                hour,
                minute,
            },
            twelve_hour(zone),
        ),
        None => value.to_owned(),
    }
}

/// Whether the instant `text` is after `now`. An instant that does not parse
/// is not.
pub(crate) fn is_after(text: &str, now: SystemTime) -> bool {
    text.parse::<Timestamp>()
        .is_ok_and(|instant| instant > timestamp(now))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use district_l10n::with_locale;

    use super::*;

    /// 2026-09-14T13:00:00Z.
    fn at(seconds: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds)
    }
    const SEPT_14_13H: u64 = 1_789_390_800;

    #[test]
    fn an_instant_reads_in_the_profiles_zone_and_an_unknown_zone_is_utc() {
        with_locale(Locale::En, || {
            let text = "2026-09-14T13:00:00Z";
            assert_eq!(
                date_time(text, "America/Toronto").as_deref(),
                Some("Sep 14, 2026, 9:00 a.m.")
            );
            assert_eq!(
                date_time(text, "Europe/Paris").as_deref(),
                Some("Sep 14, 2026, 15:00")
            );
            assert_eq!(
                date_time(text, "Mars/Olympus").as_deref(),
                Some("Sep 14, 2026, 13:00")
            );
            assert_eq!(date_time("yesterday", "UTC"), None);
            assert_eq!(date_time_or_unknown("yesterday", "UTC"), "Unknown");
            assert_eq!(times_in("UTC"), "Times in UTC");
        });
        with_locale(Locale::FrCa, || {
            assert_eq!(
                date_time("2026-09-14T13:00:00Z", "America/Toronto").as_deref(),
                Some("14 sept. 2026, 9 h")
            );
        });
    }

    #[test]
    fn today_and_tomorrow_are_calendar_days_in_the_zone() {
        with_locale(Locale::En, || {
            let now = at(SEPT_14_13H);
            let zone = "America/Toronto";
            assert_eq!(
                relative("2026-09-14T20:00:00Z", zone, now).as_deref(),
                Some("Today 4:00 p.m.")
            );
            // 01:30 UTC on the 15th is still the 14th in Toronto.
            assert_eq!(
                relative("2026-09-15T01:30:00Z", zone, now).as_deref(),
                Some("Today 9:30 p.m.")
            );
            assert_eq!(
                relative("2026-09-15T14:00:00Z", zone, now).as_deref(),
                Some("Tomorrow 10:00 a.m.")
            );
            assert_eq!(
                relative("2026-09-20T14:00:00Z", zone, now).as_deref(),
                Some("Sep 20, 2026, 10:00 a.m.")
            );
            assert_eq!(
                relative("2026-09-10T14:00:00Z", zone, now).as_deref(),
                Some("Sep 10, 2026, 10:00 a.m.")
            );
            assert_eq!(relative("soon", zone, now), None);
        });
    }

    #[test]
    fn a_day_has_a_key_a_label_and_a_distance() {
        with_locale(Locale::En, || {
            let today = SchedulingDay::today(at(SEPT_14_13H), "Pacific/Auckland");
            assert_eq!(today.key(), "2026-09-15", "already tomorrow in Auckland");
            assert_eq!(today.label(), "Sep 15, 2026");
            let parsed = SchedulingDay::parse("2026-12-31").unwrap();
            assert_eq!(today.days_until(parsed), 107);
            assert_eq!(SchedulingDay::parse("2026-02-30"), None);
            assert_eq!(SchedulingDay::parse("soon"), None);
            assert_eq!(
                day_of("2026-09-15T01:30:00Z", "America/Toronto").map(SchedulingDay::key),
                Some("2026-09-14".to_owned())
            );
            assert_eq!(day_of("x", "UTC"), None);
            assert_eq!(
                SchedulingDay::today(UNIX_EPOCH - Duration::from_secs(1), "UTC").key(),
                "1969-12-31"
            );
            let beyond = UNIX_EPOCH + Duration::from_secs(400_000_000_000);
            assert_eq!(SchedulingDay::today(beyond, "UTC").key(), "1970-01-01");
        });
    }

    #[test]
    fn a_time_field_reads_every_way_of_writing_a_time_and_saves_hh_mm() {
        for locale in [Locale::En, Locale::FrCa] {
            with_locale(locale, || {
                for (typed, wire) in [
                    ("09:30", "09:30"),
                    ("9:30", "09:30"),
                    ("9 h 30", "09:30"),
                    ("9h30", "09:30"),
                    ("9H30", "09:30"),
                    ("9\u{a0}h\u{a0}30", "09:30"),
                    ("9\u{202f}h\u{202f}30", "09:30"),
                    (" 9 h ", "09:00"),
                    ("9h", "09:00"),
                    ("17 h", "17:00"),
                    ("0 h 15", "00:15"),
                    ("00:00", "00:00"),
                    ("23:59", "23:59"),
                    ("23 h 59", "23:59"),
                ] {
                    assert_eq!(time_field_wire(typed).as_deref(), Ok(wire), "{typed:?}");
                }
                for typed in ["25:00", "24:00", "9:75", "9 h 60", "99h"] {
                    assert_eq!(
                        time_field_wire(typed),
                        Err(TimeFieldError::OutOfRange),
                        "{typed:?}"
                    );
                }
                for typed in [
                    "", "9", "nine", "9:3", "9:300", "09-30", "9 h 5", "9:", "h30", "123:00",
                    "9 h 30 x", "9.30", "-9:30", "+9:30",
                ] {
                    assert_eq!(
                        time_field_wire(typed),
                        Err(TimeFieldError::Unreadable),
                        "{typed:?}"
                    );
                }
            });
        }
    }

    #[test]
    fn a_time_field_shows_nine_h_in_french_and_the_wire_in_english() {
        with_locale(Locale::En, || {
            for wire in ["09:00", "09:30", "17:00", "00:15", "not a time"] {
                assert_eq!(time_field_text(wire), wire);
            }
            assert_eq!(
                TimeFieldError::Unreadable.message(),
                "Enter a time as HH:MM."
            );
            assert_eq!(
                TimeFieldError::OutOfRange.message(),
                "Enter a time from 00:00 to 23:59."
            );
        });
        with_locale(Locale::FrCa, || {
            for (wire, shown) in [
                ("09:00", "9 h"),
                ("09:30", "9 h 30"),
                ("17:00", "17 h"),
                ("00:15", "0 h 15"),
                ("25:00", "25:00"),
                ("not a time", "not a time"),
            ] {
                assert_eq!(time_field_text(wire), shown);
                assert!(!time_field_text(wire).contains(['\u{a0}', '\u{202f}']));
                if let Ok(back) = time_field_wire(&time_field_text(wire)) {
                    assert_eq!(back, wire, "what French shows reads back as it was");
                }
            }
            assert_eq!(
                TimeFieldError::Unreadable.message(),
                "Entrez une heure comme 9 h ou 9 h 30."
            );
            assert_eq!(
                TimeFieldError::OutOfRange.message(),
                "Entrez une heure de 0 h \u{e0} 23 h 59."
            );
        });
    }

    /// French shows a time to edit byte for byte as the service's formatTime
    /// vectors write it, ordinary spaces (U+0020) around the `h` included.
    #[test]
    fn a_french_time_field_matches_the_services_vectors_byte_for_byte() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/i18n/format-vectors.json"
        );
        let vectors: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("the vectors"))
                .expect("JSON");
        let times = vectors["categories"]
            .as_array()
            .expect("categories")
            .iter()
            .find(|c| c["category"] == "times")
            .expect("the times");
        let mut checked = 0;
        with_locale(Locale::FrCa, || {
            for case in times["vectors"].as_array().expect("vectors") {
                if case["fn"] != "formatTime" {
                    continue;
                }
                let local = case["local"].as_str().expect("local");
                let french = case["fr"].as_str().expect("fr");
                let shown = time_field_text(local);
                assert_eq!(shown.as_bytes(), french.as_bytes(), "{case}");
                assert!(!shown.contains(['\u{a0}', '\u{202f}']), "{shown:?}");
                assert_eq!(time_field_wire(&shown).as_deref(), Ok(local));
                checked += 1;
            }
        });
        assert!(checked >= 4, "only {checked} vectors");
    }

    #[test]
    fn a_window_reads_in_the_zones_convention() {
        with_locale(Locale::En, || {
            assert_eq!(hour_minute("09:00", "America/Toronto"), "9:00 a.m.");
            assert_eq!(hour_minute("17:30", "Europe/London"), "17:30");
            assert_eq!(hour_minute("nine", "UTC"), "nine");
            assert_eq!(hour_minute("25:00", "UTC"), "25:00");
        });
        with_locale(Locale::FrCa, || {
            assert_eq!(hour_minute("09:00", "UTC"), "9 h")
        });
    }

    /// French reads the first of a month `1er`, a 24-hour clock with no minutes on
    /// the hour, and a range keeps both years, in every formatter here.
    #[test]
    fn french_reads_the_first_and_the_clock_as_the_service_does() {
        with_locale(Locale::FrCa, || {
            let first = SchedulingDay::parse("2026-08-01").unwrap();
            assert_eq!(first.label(), "1er ao\u{fb}t 2026");
            assert_eq!(
                date_time("2026-08-01T13:00:00Z", "America/Toronto").as_deref(),
                Some("1er ao\u{fb}t 2026, 9 h")
            );
            assert_eq!(
                date_time("2026-09-12T19:04:00Z", "America/Toronto").as_deref(),
                Some("12 sept. 2026, 15 h 04")
            );
            let now = at(SEPT_14_13H);
            let zone = "America/Toronto";
            assert_eq!(
                relative("2026-09-14T20:00:00Z", zone, now).as_deref(),
                Some("Aujourd'hui \u{e0} 16 h")
            );
            assert_eq!(
                relative("2026-10-01T13:30:00Z", zone, now).as_deref(),
                Some("1er oct. 2026, 9 h 30")
            );
            assert_eq!(hour_minute("13:30", zone), "13 h 30");
        });
    }

    #[test]
    fn after_now_is_judged_on_the_instant() {
        let now = at(SEPT_14_13H);
        assert!(is_after("2026-09-14T13:00:01Z", now));
        assert!(!is_after("2026-09-14T13:00:00Z", now));
        assert!(!is_after("later", now));
    }
}
