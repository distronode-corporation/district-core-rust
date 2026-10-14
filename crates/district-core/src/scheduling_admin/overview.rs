//! The register: the booking desk at a glance (the booking page's links, the
//! calendars, the working hours, the event types, the next booking) and what is
//! coming up. A port of the web dashboard's `overview-summary.ts`, rule for
//! rule: the edge cases (a run of two days is a comma list, an unknown event
//! type's slug is shown as it is) are what an operator sees in the browser too.
//!
//! Each row is read on its own and fails on its own: a calendar outage says
//! nothing about the event types, so it blanks nothing but its row. The
//! profile's read only gives the zone, and a failed one falls back to UTC,
//! which the screen names. Nothing here writes.

use std::time::SystemTime;

use district_l10n::format;
use district_l10n::scheduling_strings as ss;
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingAvailabilityRule, SchedulingBooking,
    SchedulingBookingQuery, SchedulingCalendarStatus, SchedulingEventType,
    SchedulingStatusResponse,
};

use super::calendar::provider_label;
use super::{Answer, Out, Pending as AdminPending, SchedulingAdmin, SectionLoad, clock};
use crate::failure::FailureText;
use crate::model::Tickets;
use crate::text::l;
use crate::workflows::Tone;

/// How many upcoming bookings the register lists. The full feed is the
/// bookings section.
pub const COMING_UP_LIMIT: i64 = 10;

/// The register.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverviewSection {
    /// The event types.
    pub event_types: SectionLoad<Vec<SchedulingEventType>>,
    /// The member's calendars.
    pub calendar: SectionLoad<SchedulingCalendarStatus>,
    /// The weekly hours.
    pub rules: SectionLoad<Vec<SchedulingAvailabilityRule>>,
    /// The next bookings, soonest first.
    pub bookings: SectionLoad<Vec<SchedulingBooking>>,
    /// The zone every time is shown in: the profile's, or UTC until (or
    /// unless) it is read.
    pub timezone: String,
    /// The booking pages' public host, from the status, for the booking links.
    /// Never built from anything else.
    pub public_host: Option<String>,
    /// Whether the bookings are the whole tenancy's, from the status.
    pub can_manage: bool,
    /// Whether the status is still awaited, which the bookings read waits for.
    awaiting_status: bool,
}

impl Default for OverviewSection {
    fn default() -> Self {
        Self {
            event_types: SectionLoad::Loading,
            calendar: SectionLoad::Loading,
            rules: SectionLoad::Loading,
            bookings: SectionLoad::Loading,
            timezone: "UTC".to_owned(),
            public_host: None,
            can_manage: false,
            awaiting_status: true,
        }
    }
}

/// One row of the booking desk: its label, and what it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskRow {
    /// The row's label.
    pub label: &'static str,
    /// What it shows.
    pub content: DeskContent,
}

/// What a row of the booking desk shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeskContent {
    /// Its read is on its way.
    Loading,
    /// Its read failed: the sentence, in the danger tone.
    Failed(FailureText),
    /// Lines of value: the first is the row's value, any after it a caption
    /// (the event types' names) or further values (several links, several
    /// calendars).
    Lines(Vec<String>),
    /// Nothing set up yet: a hint, in the muted tone. An ordinary answer.
    Hint(&'static str),
}

/// One booking in "Coming up".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComingUpRow {
    /// The booking's id.
    pub id: String,
    /// When: "Today 2:30 p.m.", "Tomorrow 10:00 a.m." or the date and time.
    pub when: String,
    /// The row's second line: who booked, and what.
    pub subtitle: String,
    /// The status badge's label.
    pub status: String,
    /// The badge's tone.
    pub tone: Tone,
}

impl OverviewSection {
    /// "Times in America/Toronto".
    pub fn times_in(&self) -> String {
        clock::times_in(&self.timezone)
    }

    /// The booking desk's five rows: the booking page, the calendars, the
    /// hours, the event types, the next booking.
    pub fn desk(&self, now: SystemTime) -> Vec<DeskRow> {
        vec![
            row(ss::desk_booking_page(l()), &self.event_types, |_| {
                let links = self.booking_links();
                if links.is_empty() {
                    Err(match self.public_host {
                        None => ss::desk_no_host(l()),
                        Some(_) => ss::desk_no_event_types(l()),
                    })
                } else {
                    Ok(links)
                }
            }),
            row(ss::desk_calendar(l()), &self.calendar, |status| {
                let lines: Vec<String> = status
                    .connections
                    .iter()
                    .map(|c| ss::calendar_line(l(), &provider_label(&c.provider), &c.account_email))
                    .collect();
                if lines.is_empty() {
                    Err(ss::desk_no_calendar(l()))
                } else {
                    Ok(lines)
                }
            }),
            row(ss::desk_hours(l()), &self.rules, |rules| {
                summarize_working_hours(rules, &self.timezone)
                    .map(|summary| vec![summary])
                    .ok_or(ss::desk_no_hours(l()))
            }),
            row(ss::desk_event_types(l()), &self.event_types, |items| {
                let summary = summarize_event_types(items).ok_or(ss::desk_no_event_types(l()))?;
                let names: Vec<&str> = items
                    .iter()
                    .filter(|item| item.is_active != Some(false))
                    .map(|item| item.name.as_str())
                    .collect();
                let mut lines = vec![summary];
                if !names.is_empty() {
                    lines.push(names.join(", "));
                }
                Ok(lines)
            }),
            row(ss::desk_next_booking(l()), &self.bookings, |bookings| {
                let booking = bookings.first().ok_or(ss::desk_no_bookings(l()))?;
                let when = clock::relative(&booking.start_at, &self.timezone, now)
                    .unwrap_or_else(|| ss::unknown_time(l()).to_owned());
                Ok(vec![ss::next_booking_line(
                    l(),
                    &when,
                    &booking_who(booking),
                    &self.event_type_name(booking),
                )])
            }),
        ]
    }

    /// The bookable event types' public links, `https://<host>/book/<slug>`.
    /// None while the host is not known: a link with no origin is no link.
    pub fn booking_links(&self) -> Vec<String> {
        let (Some(host), Some(items)) = (
            self.public_host.as_deref().filter(|h| !h.is_empty()),
            self.event_types.value(),
        ) else {
            return Vec::new();
        };
        bookable_event_types(items)
            .map(|item| booking_url_for(host, &item.slug))
            .collect()
    }

    /// "Coming up": the next bookings, once read.
    pub fn coming_up(&self, now: SystemTime) -> Vec<ComingUpRow> {
        let Some(bookings) = self.bookings.value() else {
            return Vec::new();
        };
        bookings
            .iter()
            .map(|booking| {
                let (status, tone) = super::bookings::status_label(&booking.status);
                ComingUpRow {
                    id: booking.id.clone(),
                    when: clock::relative(&booking.start_at, &self.timezone, now)
                        .unwrap_or_else(|| ss::unknown_time(l()).to_owned()),
                    subtitle: ss::coming_up_subtitle(
                        l(),
                        &booking_who(booking),
                        &self.event_type_name(booking),
                    ),
                    status,
                    tone,
                }
            })
            .collect()
    }

    fn event_type_name(&self, booking: &SchedulingBooking) -> String {
        booking_event_type_name(booking, self.event_types.value().map_or(&[], Vec::as_slice))
    }
}

/// A row from its read: loading, failed, or what `content` makes of the value
/// (lines, or a hint for nothing set up).
fn row<T>(
    label: &'static str,
    load: &SectionLoad<T>,
    content: impl FnOnce(&T) -> Result<Vec<String>, &'static str>,
) -> DeskRow {
    let content = match load {
        SectionLoad::Loading => DeskContent::Loading,
        SectionLoad::Failed(failure) => DeskContent::Failed(failure.clone()),
        SectionLoad::Ready(value) => match content(value) {
            Ok(lines) => DeskContent::Lines(lines),
            Err(hint) => DeskContent::Hint(hint),
        },
    };
    DeskRow { label, content }
}

/// What an answer the register awaits is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    Me,
    EventTypes,
    Calendar,
    Rules,
    Bookings,
}

fn send(pending: Pending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::Overview(pending), Box::new(request))
}

/// Opens the register: the four reads that need nothing, at once; the
/// bookings wait for the status, which scopes them.
pub(crate) fn enter(admin: &mut SchedulingAdmin, tickets: &mut Tickets) -> Vec<Out> {
    admin.forget(tickets, |pending| {
        matches!(pending, AdminPending::Overview(_))
    });
    admin.overview = Some(OverviewSection::default());
    vec![
        send(Pending::Me, SchedulingAdminRequest::Me),
        send(Pending::EventTypes, SchedulingAdminRequest::EventTypes),
        send(Pending::Calendar, SchedulingAdminRequest::CalendarStatus),
        // No event type: these are the member's own hours, which a filter would
        // leave out.
        send(
            Pending::Rules,
            SchedulingAdminRequest::Rules {
                event_type_id: None,
            },
        ),
    ]
}

/// The status arrived (or failed: then the bookings are the member's own, the
/// safe direction, and no host is known).
pub(crate) fn status_arrived(
    admin: &mut SchedulingAdmin,
    status: Option<&SchedulingStatusResponse>,
) -> Vec<Out> {
    let Some(section) = admin.overview.as_mut().filter(|s| s.awaiting_status) else {
        return Vec::new();
    };
    section.awaiting_status = false;
    section.can_manage = status.is_some_and(|s| s.can_manage);
    section.public_host = status
        .and_then(|s| s.tenant.as_ref())
        .map(|t| t.public_host.clone());
    vec![send(
        Pending::Bookings,
        SchedulingAdminRequest::Bookings(SchedulingBookingQuery {
            when: Some("upcoming".to_owned()),
            limit: Some(COMING_UP_LIMIT),
            all_hosts: section.can_manage,
            order: Some("asc".to_owned()),
            ..SchedulingBookingQuery::default()
        }),
    )]
}

pub(crate) fn answered(admin: &mut SchedulingAdmin, pending: Pending, answer: Answer) -> Vec<Out> {
    // The register's answers are forgotten when it closes, so it is open.
    if let Some(section) = admin.overview.as_mut() {
        match pending {
            // The profile gives the zone and nothing else: its failure is no
            // row, and the zone stays UTC.
            Pending::Me => {
                if let Ok(SchedulingAdminData::Me(me)) = answer {
                    section.timezone = me.display_timezone().to_owned();
                }
            }
            Pending::EventTypes => section.event_types = load(answer, data!(EventTypes)),
            Pending::Calendar => section.calendar = load(answer, data!(CalendarStatus)),
            Pending::Rules => section.rules = load(answer, data!(Rules)),
            Pending::Bookings => {
                section.bookings = load(answer, |data| data!(BookingPage)(data).map(|p| p.items));
            }
        }
    }
    Vec::new()
}

/// A read's answer as a section's state: the value `pick` finds in it, or the
/// failure. An answer of another type than the request's cannot come (the
/// request decodes it), and reads as a response this build cannot read.
pub(crate) fn load<T>(
    answer: Answer,
    pick: impl FnOnce(SchedulingAdminData) -> Option<T>,
) -> SectionLoad<T> {
    SectionLoad::from_result(answer.and_then(|data| pick(data).ok_or_else(FailureText::unexpected)))
}

// The summaries, as overview-summary.ts writes them.

/// The event types a customer could book: a flag the scheduler left out counts
/// as set.
pub(crate) fn bookable_event_types(
    items: &[SchedulingEventType],
) -> impl Iterator<Item = &SchedulingEventType> {
    items
        .iter()
        .filter(|item| item.is_active != Some(false) && item.is_public != Some(false))
}

/// "3 event types, 2 active", or `None` when there are none. The active count
/// is the switch the operator threw (`is_active`), not whether it is bookable.
pub(crate) fn summarize_event_types(items: &[SchedulingEventType]) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let active = items.iter().filter(|i| i.is_active != Some(false)).count();
    Some(ss::overview_event_types(
        l(),
        items.len() as i64,
        &format::integer(l(), active as i64),
    ))
}

/// "Mon to Fri, 9:00 a.m. to 5:00 p.m.; Sat, 10:00 a.m. to 2:00 p.m.", or
/// `None` when no rule is for every event type. A rule for one event type is
/// not the member's working hours. Days with the same windows share a clause,
/// in the order their first day comes, Monday first.
pub(crate) fn summarize_working_hours(
    rules: &[SchedulingAvailabilityRule],
    zone: &str,
) -> Option<String> {
    let mut by_day: Vec<Vec<(&str, &str)>> = vec![Vec::new(); 7];
    for rule in rules.iter().filter(|r| r.event_type_id.is_none()) {
        by_day[display_index(rule.day_of_week)].push((&rule.start_time, &rule.end_time));
    }
    let mut phrases: Vec<(String, Vec<usize>)> = Vec::new();
    for (day, windows) in by_day.iter_mut().enumerate().filter(|(_, w)| !w.is_empty()) {
        windows.sort_by(|a, b| a.0.cmp(b.0));
        let phrase = windows_phrase(windows, zone);
        match phrases.iter_mut().find(|(p, _)| *p == phrase) {
            Some((_, days)) => days.push(day),
            None => phrases.push((phrase, vec![day])),
        }
    }
    if phrases.is_empty() {
        return None;
    }
    Some(
        phrases
            .iter()
            .map(|(phrase, days)| ss::overview_hours_clause(l(), &days_phrase(days), phrase))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

/// The wire's day (0 Sunday) as the display's (0 Monday). Any whole number
/// folds into the week, as the web's does.
pub(crate) fn display_index(day_of_week: i64) -> usize {
    (day_of_week.rem_euclid(7) + 6).rem_euclid(7) as usize
}

/// A day's short name, Monday first.
pub(crate) fn weekday_short(display_index: usize) -> &'static str {
    match display_index {
        0 => ss::weekday_short_mon(l()),
        1 => ss::weekday_short_tue(l()),
        2 => ss::weekday_short_wed(l()),
        3 => ss::weekday_short_thu(l()),
        4 => ss::weekday_short_fri(l()),
        5 => ss::weekday_short_sat(l()),
        _ => ss::weekday_short_sun(l()),
    }
}

/// "9:00 a.m. to 12:00 p.m. and 1:00 p.m. to 5:00 p.m.".
fn windows_phrase(windows: &[(&str, &str)], zone: &str) -> String {
    let parts: Vec<String> = windows
        .iter()
        .map(|(start, end)| {
            ss::overview_window(
                l(),
                &clock::hour_minute(start, zone),
                &clock::hour_minute(end, zone),
            )
        })
        .collect();
    format::list(l(), &parts)
}

/// "Mon to Fri" for three or more days in a row, else "Mon, Wed": exactly two
/// stays a list, as on the web.
fn days_phrase(days: &[usize]) -> String {
    let consecutive = days.windows(2).all(|pair| pair[1] == pair[0] + 1);
    match (days.first(), days.last()) {
        (Some(first), Some(last)) if days.len() >= 3 && consecutive => {
            ss::overview_days_range(l(), weekday_short(*first), weekday_short(*last))
        }
        _ => days
            .iter()
            .map(|day| weekday_short(*day))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// `https://<host>/book/<slug>`, the slug encoded as `encodeURIComponent` does:
/// it is the customer's text, and a `/` in it must not reach another path.
pub(crate) fn booking_url_for(host: &str, slug: &str) -> String {
    format!("https://{host}/book/{}", encode_uri_component(slug))
}

fn encode_uri_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => char::from(byte).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Who booked: the first attendee's name, else their address (a blank name
/// falls through), else "Someone".
pub(crate) fn booking_who(booking: &SchedulingBooking) -> String {
    let first = booking.attendees.as_ref().and_then(|a| a.first());
    let trimmed =
        |value: Option<&String>| value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
    first
        .and_then(|a| trimmed(a.name.as_ref()).or_else(|| trimmed(a.email.as_ref())))
        .unwrap_or_else(|| ss::overview_someone(l()).to_owned())
}

/// The event type's name, else its slug as it came (it may have been deleted
/// since), else "Booking" for a booking that names none.
pub(crate) fn booking_event_type_name(
    booking: &SchedulingBooking,
    event_types: &[SchedulingEventType],
) -> String {
    match booking.event_type_slug.as_deref().filter(|s| !s.is_empty()) {
        None => ss::overview_booking(l()).to_owned(),
        Some(slug) => event_types
            .iter()
            .find(|t| t.slug == slug)
            .map_or(slug, |t| t.name.as_str())
            .to_owned(),
    }
}
