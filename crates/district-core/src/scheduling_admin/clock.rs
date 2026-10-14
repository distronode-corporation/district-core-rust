//! The scheduler's times on the member's wall clock: an RFC 3339 instant read
//! in the IANA zone on their scheduler profile (not the computer's), then
//! written by `district_l10n::format`.
//!
//! An unknown zone reads as UTC rather than hiding the row: the instant is
//! still real, and the screen names the zone it used.

use std::time::SystemTime;

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

    use district_l10n::{Locale, with_locale};

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

    #[test]
    fn after_now_is_judged_on_the_instant() {
        let now = at(SEPT_14_13H);
        assert!(is_after("2026-09-14T13:00:01Z", now));
        assert!(!is_after("2026-09-14T13:00:00Z", now));
        assert!(!is_after("later", now));
    }
}
