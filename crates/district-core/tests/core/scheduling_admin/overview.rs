//! The register: its reads, each row on its own, and the bookings scoped by
//! the status.

use district_core::scheduling_admin::{DeskContent, DeskRow, OverviewSection, SectionLoad};
use district_core::{Model, SchedulingSection};
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingBookingPage, SchedulingBookingQuery,
    SchedulingCalendarStatus, SchedulingItems, SchedulingOk,
};

use super::{admin, answer, data, done, now, open, ready_status, sent, status_answer, ticket_for};
use crate::support::server_error;

fn section(model: &Model) -> &OverviewSection {
    admin(model)
        .overview
        .as_ref()
        .expect("the overview is open")
}

fn rows(model: &Model) -> Vec<DeskRow> {
    section(model).desk(now())
}

fn content(model: &Model, index: usize) -> DeskContent {
    rows(model)[index].content.clone()
}

/// The register open, every read answered with its recorded fixture.
fn read() -> Model {
    let (mut model, effects) = open("agency", SchedulingSection::Overview);
    let requests: Vec<_> = sent(&effects).into_iter().map(|(_, r)| r).collect();
    assert_eq!(
        requests,
        [
            SchedulingAdminRequest::Me,
            SchedulingAdminRequest::EventTypes,
            SchedulingAdminRequest::CalendarStatus,
            SchedulingAdminRequest::Rules {
                event_type_id: None
            },
        ]
    );
    let answers = [
        (
            SchedulingAdminRequest::Me,
            SchedulingAdminData::Me(data("district-scheduling-me.json")),
        ),
        (
            SchedulingAdminRequest::EventTypes,
            SchedulingAdminData::EventTypes(
                data::<SchedulingItems<_>>("district-scheduling-event-types.json").items,
            ),
        ),
        (
            SchedulingAdminRequest::CalendarStatus,
            SchedulingAdminData::CalendarStatus(data("district-scheduling-calendar-status.json")),
        ),
        (
            SchedulingAdminRequest::Rules {
                event_type_id: None,
            },
            SchedulingAdminData::Rules(
                data::<SchedulingItems<_>>("district-scheduling-rules.json").items,
            ),
        ),
    ];
    for (request, reply) in answers {
        let ticket = ticket_for(&effects, |r| *r == request);
        assert!(answer(&mut model, ticket, done(reply)).is_empty());
    }
    assert_eq!(
        content(&model, 4),
        DeskContent::Loading,
        "waits for the status"
    );
    let effects = status_answer(&mut model, &effects, Ok(ready_status()));
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::Bookings(SchedulingBookingQuery {
            when: Some("upcoming".into()),
            limit: Some(10),
            all_hosts: true,
            order: Some("asc".into()),
            ..SchedulingBookingQuery::default()
        })
    );
    answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::BookingPage(data(
            "district-scheduling-bookings.json",
        ))),
    );
    model
}

#[test]
fn the_desk_summarises_each_section_in_the_profiles_zone() {
    let model = read();
    let section = section(&model);
    assert_eq!(section.times_in(), "Times in America/Toronto");
    let labels: Vec<&str> = rows(&model).iter().map(|r| r.label).collect();
    assert_eq!(
        labels,
        [
            "Booking page",
            "Calendar",
            "Working hours",
            "Event types",
            "Next booking"
        ]
    );
    let lines = |text: &[&str]| DeskContent::Lines(text.iter().map(|t| (*t).to_owned()).collect());
    assert_eq!(
        content(&model, 0),
        lines(&["https://book.example.com/book/phone-consultation"])
    );
    assert_eq!(
        content(&model, 1),
        lines(&[
            "Google Calendar, contract@example.com",
            "CalDAV, caldav@example.com"
        ])
    );
    // The rule for one event type is not the member's hours.
    assert_eq!(content(&model, 2), lines(&["Wed, 10:30 a.m. to 3:00 p.m."]));
    assert_eq!(
        content(&model, 3),
        lines(&["2 event types, 1 active", "Phone consultation"])
    );
    assert_eq!(
        content(&model, 4),
        lines(&["Today 9:00 a.m., Dana Booker, Phone consultation"])
    );
    let coming = section.coming_up(now());
    assert_eq!(coming.len(), 2);
    assert_eq!(coming[0].when, "Today 9:00 a.m.");
    assert_eq!(coming[0].subtitle, "Dana Booker \u{b7} Phone consultation");
    assert_eq!(coming[1].status, "Cancelled");
    assert_eq!(coming[1].when, "Tomorrow 2:00 p.m.");
}

#[test]
fn nothing_set_up_is_a_hint_and_each_failure_stays_in_its_row() {
    let (mut model, effects) = open("agency", SchedulingSection::Overview);
    assert!(
        rows(&model)
            .iter()
            .all(|row| row.content == DeskContent::Loading)
    );
    assert!(section(&model).coming_up(now()).is_empty());
    let empty = [
        (
            SchedulingAdminRequest::EventTypes,
            done(SchedulingAdminData::EventTypes(Vec::new())),
        ),
        (
            SchedulingAdminRequest::CalendarStatus,
            done(SchedulingAdminData::CalendarStatus(
                SchedulingCalendarStatus::default(),
            )),
        ),
        (
            SchedulingAdminRequest::Rules {
                event_type_id: None,
            },
            Err(server_error()),
        ),
        (SchedulingAdminRequest::Me, Err(server_error())),
    ];
    for (request, reply) in empty {
        let ticket = ticket_for(&effects, |r| *r == request);
        answer(&mut model, ticket, reply);
    }
    let effects = status_answer(&mut model, &effects, Err(server_error()));
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::Bookings(query))] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(!query.all_hosts, "the member's own, the safe direction");
    answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::BookingPage(
            SchedulingBookingPage::default(),
        )),
    );
    let section = section(&model);
    assert_eq!(section.timezone, "UTC");
    assert_eq!(
        content(&model, 0),
        DeskContent::Hint("The booking address is not ready yet.")
    );
    assert_eq!(
        content(&model, 1),
        DeskContent::Hint("No calendar is connected, so nothing is checked for conflicts.")
    );
    assert!(matches!(content(&model, 2), DeskContent::Failed(_)));
    assert_eq!(
        content(&model, 3),
        DeskContent::Hint("No event types yet, so there is nothing to book.")
    );
    assert_eq!(content(&model, 4), DeskContent::Hint("None yet."));
    assert!(section.booking_links().is_empty());
    assert!(section.coming_up(now()).is_empty());
}

#[test]
fn with_a_host_and_no_bookable_event_type_the_page_row_says_there_is_nothing_to_book() {
    let (mut model, effects) = open("agency", SchedulingSection::Overview);
    status_answer(&mut model, &effects, Ok(ready_status()));
    let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::EventTypes);
    answer(
        &mut model,
        ticket,
        done(SchedulingAdminData::EventTypes(Vec::new())),
    );
    assert_eq!(
        content(&model, 0),
        DeskContent::Hint("No event types yet, so there is nothing to book.")
    );
}

#[test]
fn an_answer_of_another_kind_than_asked_reads_as_unreadable() {
    let (mut model, effects) = open("agency", SchedulingSection::Overview);
    let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::CalendarStatus);
    answer(
        &mut model,
        ticket,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    let SectionLoad::Failed(failure) = &section(&model).calendar else {
        panic!("{:?}", section(&model).calendar);
    };
    assert!(!failure.retryable);
}

#[test]
fn a_booking_whose_start_cannot_be_read_says_unknown() {
    let (mut model, effects) = open("agency", SchedulingSection::Overview);
    let effects = status_answer(&mut model, &effects, Ok(ready_status()));
    let mut page: SchedulingBookingPage = data("district-scheduling-bookings.json");
    page.items[0].start_at = "soon".into();
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::BookingPage(page)),
    );
    let DeskContent::Lines(lines) = content(&model, 4) else {
        panic!()
    };
    assert_eq!(lines, ["Unknown, Dana Booker, phone-consultation"]);
    assert_eq!(section(&model).coming_up(now())[0].when, "Unknown");
}

#[test]
fn event_types_all_switched_off_are_counted_and_none_named() {
    let (mut model, effects) = open("agency", SchedulingSection::Overview);
    let mut types: Vec<district_model::SchedulingEventType> =
        data::<SchedulingItems<_>>("district-scheduling-event-types.json").items;
    for t in &mut types {
        t.is_active = Some(false);
    }
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::EventTypes),
        done(SchedulingAdminData::EventTypes(types)),
    );
    let DeskContent::Lines(lines) = content(&model, 3) else {
        panic!("{:?}", rows(&model));
    };
    assert_eq!(lines.len(), 1, "{lines:?}");
}
