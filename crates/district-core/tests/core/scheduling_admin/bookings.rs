//! Bookings: the context the first page waits for, paging over live data, the
//! views, and cancelling, moving and handing over a booking, from the list and
//! from one booking.

use std::time::Duration;

use district_api::ApiError;
use district_core::scheduling_admin::{
    BOOKINGS_PAGE_SIZE, BookingDetailSection, BookingDialog, BookingEvent, BookingsEvent,
    BookingsSection, BookingsView, CANCEL_REASON_MAX, ReassignDialog, RescheduleDialog,
    SchedulingDay, SectionLoad, WriteState,
};
use district_core::{Effect, Event, Model, Route, SchedulingAdminEvent, SchedulingSection};
use district_model::{
    SchedulingAdminData, SchedulingAdminReply, SchedulingAdminRequest, SchedulingBooking,
    SchedulingBookingPage, SchedulingBookingQuery, SchedulingItems, SchedulingMe, SchedulingSlots,
    SchedulingUser,
};

use super::{
    act, admin, answer, data, done, now, open, ready_status, refused, sent, status_answer,
    ticket_for,
};
use crate::support::server_error;

fn section(model: &Model) -> &BookingsSection {
    admin(model)
        .bookings
        .as_ref()
        .expect("the bookings are open")
}

fn me() -> SchedulingMe {
    data("district-scheduling-me.json")
}

fn page() -> SchedulingBookingPage {
    data("district-scheduling-bookings.json")
}

fn bookings(model: &mut Model, event: BookingsEvent) -> Vec<Effect> {
    act(model, SchedulingAdminEvent::Bookings(event))
}

fn is_page(request: &SchedulingAdminRequest) -> bool {
    matches!(request, SchedulingAdminRequest::Bookings(_))
}

/// The bookings open as `role`, the context read, and the first page's request.
fn context_read(role: &str) -> (Model, Vec<Effect>) {
    let (mut model, effects) = open(role, SchedulingSection::Bookings);
    let me_ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::Me);
    let types_ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::EventTypes);
    assert!(answer(&mut model, me_ticket, done(SchedulingAdminData::Me(me()))).is_empty());
    let types = data::<SchedulingItems<_>>("district-scheduling-event-types.json").items;
    assert!(
        answer(
            &mut model,
            types_ticket,
            done(SchedulingAdminData::EventTypes(types))
        )
        .is_empty()
    );
    let effects = status_answer(&mut model, &effects, Ok(ready_status()));
    (model, effects)
}

/// The bookings open as an agency member, with the recorded page read.
fn listed() -> Model {
    let (mut model, effects) = context_read("agency");
    let ticket = ticket_for(&effects, is_page);
    answer(
        &mut model,
        ticket,
        done(SchedulingAdminData::BookingPage(page())),
    );
    model
}

fn booking(id: &str, status: &str, slug: Option<&str>) -> SchedulingBooking {
    SchedulingBooking {
        id: id.to_owned(),
        event_type_slug: slug.map(str::to_owned),
        host_id: Some("sched-user-contract".to_owned()),
        start_at: "2026-09-14T13:00:00Z".to_owned(),
        end_at: "2026-09-14T13:30:00Z".to_owned(),
        status: status.to_owned(),
        ..SchedulingBooking::default()
    }
}

#[test]
fn the_first_page_waits_for_the_profile_the_event_types_and_the_status() {
    let (model, effects) = context_read("agency");
    assert_eq!(
        sent(&effects)
            .into_iter()
            .map(|(_, r)| r)
            .collect::<Vec<_>>(),
        [SchedulingAdminRequest::Bookings(SchedulingBookingQuery {
            status: Some("confirmed".into()),
            when: Some("upcoming".into()),
            limit: Some(BOOKINGS_PAGE_SIZE),
            offset: Some(0),
            all_hosts: true,
            order: Some("asc".into()),
            ..SchedulingBookingQuery::default()
        })]
    );
    let section = section(&model);
    assert_eq!(section.list, SectionLoad::Loading);
    assert!(section.can_manage && section.scheduler_is_admin);
    assert_eq!(section.times_in(), "Times in America/Toronto");
}

#[test]
fn rows_read_in_the_profiles_zone_with_the_actions_the_member_may_take() {
    let model = listed();
    let rows = section(&model).rows(true, now());
    assert_eq!(rows.len(), 2);
    let first = &rows[0];
    assert_eq!(first.when, "Sep 14, 2026, 9:00 a.m.");
    assert_eq!(
        first.subtitle,
        "Dana Booker \u{b7} Phone consultation \u{b7} Contract Member"
    );
    assert_eq!(first.email, "dana@example.com");
    assert_eq!(first.status, "Confirmed");
    assert!(first.can_cancel && first.can_reschedule && first.can_reassign);
    // The sparse row: no attendee, no event type, no host, and cancelled.
    let second = &rows[1];
    assert_eq!(second.subtitle, "Someone \u{b7} Booking \u{b7} No host");
    assert_eq!(
        (second.email.as_str(), second.status.as_str()),
        ("", "Cancelled")
    );
    assert!(!second.can_cancel && !second.can_reschedule && !second.can_reassign);
    // A viewer, and a booking that has ended, are offered nothing.
    assert!(!section(&model).rows(false, now())[0].can_cancel);
    let later = now() + Duration::from_secs(3 * 3600);
    assert!(!section(&model).rows(true, later)[0].can_cancel);
    assert!(
        section(&model).paging.end_reached,
        "two rows of a page of 25"
    );
}

#[test]
fn without_the_context_the_list_is_the_members_own_in_utc_with_no_reassign() {
    let (mut model, effects) = open("agency", SchedulingSection::Bookings);
    for wanted in [
        SchedulingAdminRequest::Me,
        SchedulingAdminRequest::EventTypes,
    ] {
        let ticket = ticket_for(&effects, |r| *r == wanted);
        answer(&mut model, ticket, Err(server_error()));
    }
    let effects = status_answer(&mut model, &effects, Err(server_error()));
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::Bookings(query))] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(!query.all_hosts);
    answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::BookingPage(page())),
    );
    let row = &section(&model).rows(true, now())[0];
    assert_eq!(row.when, "Sep 14, 2026, 13:00");
    assert_eq!(
        row.subtitle,
        "Dana Booker \u{b7} phone-consultation \u{b7} Contract Member"
    );
    assert!(row.can_cancel && !row.can_reassign);
}

fn full_page(ids: impl Iterator<Item = usize>, total: Option<i64>) -> SchedulingAdminData {
    SchedulingAdminData::BookingPage(SchedulingBookingPage {
        items: ids
            .map(|i| booking(&format!("b{i}"), "confirmed", Some("intro")))
            .collect(),
        total,
        ..SchedulingBookingPage::default()
    })
}

#[test]
fn paging_moves_by_what_came_and_keeps_each_booking_once() {
    let (mut model, effects) = context_read("agency");
    answer(
        &mut model,
        ticket_for(&effects, is_page),
        done(full_page(0..25, Some(29))),
    );
    assert!(!section(&model).paging.end_reached);
    let effects = bookings(&mut model, BookingsEvent::LoadMore);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::Bookings(query))] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(query.offset, Some(25));
    assert!(section(&model).paging.loading_more);
    assert!(
        bookings(&mut model, BookingsEvent::LoadMore).is_empty(),
        "one at a time"
    );
    // A booking made meanwhile pushed b24 onto the next page.
    answer(&mut model, *ticket, done(full_page(24..28, Some(29))));
    let rows = section(&model).list.value().unwrap();
    assert_eq!(rows.len(), 28);
    assert!(
        section(&model).paging.end_reached,
        "25 + 4 reached the total"
    );
    assert!(bookings(&mut model, BookingsEvent::LoadMore).is_empty());
}

#[test]
fn a_later_page_that_fails_keeps_the_rows_and_a_first_one_fails_the_list() {
    let (mut model, effects) = context_read("agency");
    answer(
        &mut model,
        ticket_for(&effects, is_page),
        done(full_page(0..25, None)),
    );
    let effects = bookings(&mut model, BookingsEvent::LoadMore);
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    let section_now = section(&model);
    assert_eq!(section_now.list.value().unwrap().len(), 25);
    assert_eq!(
        section_now.paging.more_failure.as_ref().unwrap().message,
        "The booking system did not answer. Try again in a minute."
    );
    assert!(!section_now.paging.loading_more);

    let (mut model, effects) = context_read("agency");
    answer(
        &mut model,
        ticket_for(&effects, is_page),
        refused("unavailable"),
    );
    assert!(section(&model).list.failure().unwrap().retryable);
    assert!(section(&model).rows(true, now()).is_empty());
    assert!(bookings(&mut model, BookingsEvent::LoadMore).is_empty());
}

#[test]
fn each_view_asks_for_its_own_first_page() {
    let mut model = listed();
    let mut queries = Vec::new();
    for view in [
        BookingsView::Past,
        BookingsView::Cancelled,
        BookingsView::All,
    ] {
        let effects = bookings(&mut model, BookingsEvent::SelectView(view));
        let requests = sent(&effects);
        let [(_, SchedulingAdminRequest::Bookings(query))] = requests.as_slice() else {
            panic!("{effects:?}");
        };
        assert_eq!(section(&model).list, SectionLoad::Loading);
        queries.push((
            query.status.clone(),
            query.when.clone(),
            query.order.clone(),
        ));
    }
    let text = |s: &str| Some(s.to_owned());
    assert_eq!(
        queries,
        [
            (text("confirmed"), text("past"), text("desc")),
            (text("cancelled"), None, text("desc")),
            (None, None, text("desc")),
        ]
    );
    assert!(bookings(&mut model, BookingsEvent::SelectView(BookingsView::All)).is_empty());
    let labels: Vec<_> = BookingsView::ALL.iter().map(|v| v.label()).collect();
    assert_eq!(labels, ["Upcoming", "Past", "Cancelled", "All"]);
    let empties: Vec<_> = BookingsView::ALL.iter().map(|v| v.empty_body()).collect();
    assert_eq!(
        empties,
        [
            "Nothing is booked yet. Share your booking page and bookings appear here.",
            "No past bookings.",
            "No cancelled bookings.",
            "Nothing is booked yet. Share your booking page and bookings appear here.",
        ]
    );
}

#[test]
fn a_page_for_a_view_left_is_dropped() {
    let (mut model, effects) = context_read("agency");
    let old = ticket_for(&effects, is_page);
    bookings(&mut model, BookingsEvent::SelectView(BookingsView::Past));
    assert!(
        answer(
            &mut model,
            old,
            done(SchedulingAdminData::BookingPage(page()))
        )
        .is_empty()
    );
    assert_eq!(section(&model).list, SectionLoad::Loading);
}

fn dialog(model: &Model) -> &BookingDialog {
    section(model).dialog.as_ref().expect("a dialog")
}

fn open_cancel(model: &mut Model, id: &str) -> Vec<Effect> {
    bookings(
        model,
        BookingsEvent::OpenCancel {
            booking_id: id.to_owned(),
            now: now(),
        },
    )
}

#[test]
fn cancelling_needs_a_member_who_may_change_things_and_a_booking_still_on() {
    let mut model = listed();
    open_cancel(&mut model, "bk-contract-2");
    assert!(section(&model).dialog.is_none(), "already cancelled");
    open_cancel(&mut model, "nobody");
    assert!(section(&model).dialog.is_none());
    bookings(
        &mut model,
        BookingsEvent::OpenCancel {
            booking_id: "bk-contract-1".into(),
            now: now() + Duration::from_secs(7200),
        },
    );
    assert!(section(&model).dialog.is_none(), "over by then");

    let (mut viewer, effects) = context_read("viewer");
    answer(
        &mut viewer,
        ticket_for(&effects, is_page),
        done(SchedulingAdminData::BookingPage(page())),
    );
    open_cancel(&mut viewer, "bk-contract-1");
    assert!(section(&viewer).dialog.is_none());
}

#[test]
fn a_cancel_sends_the_trimmed_reason_or_none_and_reads_the_list_again() {
    let mut model = listed();
    open_cancel(&mut model, "bk-contract-1");
    let BookingDialog::Cancel(cancel) = dialog(&model) else {
        panic!()
    };
    assert_eq!(cancel.write, WriteState::Idle);
    bookings(
        &mut model,
        BookingsEvent::EditCancelReason("x".repeat(CANCEL_REASON_MAX + 1)),
    );
    assert!(bookings(&mut model, BookingsEvent::Confirm).is_empty());
    let BookingDialog::Cancel(cancel) = dialog(&model) else {
        panic!()
    };
    assert_eq!(
        cancel.reason_rejected,
        Some("That reason is longer than 1000 characters.")
    );
    bookings(
        &mut model,
        BookingsEvent::EditCancelReason("  Sick  ".into()),
    );
    let effects = bookings(&mut model, BookingsEvent::Confirm);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::CancelBooking {
            id: "bk-contract-1".into(),
            reason: Some("Sick".into()),
        }
    );
    assert!(
        bookings(&mut model, BookingsEvent::Confirm).is_empty(),
        "one at a time"
    );
    let cancelled = data::<SchedulingBooking>("district-scheduling-booking.json");
    let effects = answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::Booking(cancelled)),
    );
    assert!(
        sent(&effects).iter().any(|(_, r)| is_page(r)),
        "the list is read again"
    );
    let BookingDialog::Cancel(cancel) = dialog(&model) else {
        panic!()
    };
    assert_eq!(cancel.write.notice(), Some("Booking cancelled"));
}

#[test]
fn a_refused_cancel_keeps_the_reason_until_it_is_edited() {
    let mut model = listed();
    open_cancel(&mut model, "bk-contract-1");
    let effects = bookings(&mut model, BookingsEvent::Confirm);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::CancelBooking { reason: None, .. })] =
        requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    let effects = answer(&mut model, *ticket, refused("rejected"));
    assert!(effects.is_empty(), "nothing read again");
    let BookingDialog::Cancel(cancel) = dialog(&model) else {
        panic!()
    };
    assert_eq!(
        cancel.write.failure().unwrap().message,
        "That did not save. Try again."
    );
    bookings(&mut model, BookingsEvent::EditCancelReason("Away".into()));
    let BookingDialog::Cancel(cancel) = dialog(&model) else {
        panic!()
    };
    assert_eq!(cancel.write, WriteState::Idle);
    bookings(&mut model, BookingsEvent::CloseDialog);
    assert!(section(&model).dialog.is_none());
}

fn reschedule(model: &Model) -> &RescheduleDialog {
    match dialog(model) {
        BookingDialog::Reschedule(dialog) => dialog,
        other => panic!("{other:?}"),
    }
}

fn open_reschedule(model: &mut Model, id: &str) -> Vec<Effect> {
    bookings(
        model,
        BookingsEvent::OpenReschedule {
            booking_id: id.to_owned(),
            now: now(),
        },
    )
}

fn slots() -> Result<SchedulingAdminReply, ApiError> {
    done(SchedulingAdminData::Slots(data::<SchedulingSlots>(
        "district-scheduling-slots.json",
    )))
}

#[test]
fn a_move_reads_one_days_times_in_the_profiles_zone() {
    let mut model = listed();
    let effects = open_reschedule(&mut model, "bk-contract-1");
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::Slots {
            slug: "phone-consultation".into(),
            from: Some("2026-09-14".into()),
            to: Some("2026-09-14".into()),
            tz: Some("America/Toronto".into()),
        }
    );
    assert!(!reschedule(&model).can_confirm());
    bookings(
        &mut model,
        BookingsEvent::PickSlot("2026-09-14T13:00:00Z".into()),
    );
    assert_eq!(
        reschedule(&model).selected,
        None,
        "not a time on the list yet"
    );
    answer(&mut model, *ticket, slots());
    assert_eq!(reschedule(&model).slots.value().unwrap().len(), 2);
    assert!(bookings(&mut model, BookingsEvent::Confirm).is_empty());
    assert_eq!(reschedule(&model).selection_rejected, Some("Pick a time."));
    bookings(
        &mut model,
        BookingsEvent::PickSlot("2026-09-14T13:00:00Z".into()),
    );
    let dialog = reschedule(&model);
    assert!(dialog.can_confirm() && dialog.selection_rejected.is_none());
    assert_eq!(
        dialog.slot_label("2026-09-14T13:00:00Z"),
        "Sep 14, 2026, 9:00 a.m."
    );
    assert_eq!(dialog.slot_label("whenever"), "whenever");
    // The same day again reads nothing; another day reads its times and
    // forgets the choice.
    let day = SchedulingDay::parse("2026-09-14").unwrap();
    assert!(bookings(&mut model, BookingsEvent::PickDay(day)).is_empty());
    let next = SchedulingDay::parse("2026-09-15").unwrap();
    let effects = bookings(&mut model, BookingsEvent::PickDay(next));
    let requests = sent(&effects);
    let [(_, SchedulingAdminRequest::Slots { from, .. })] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(from.as_deref(), Some("2026-09-15"));
    assert_eq!(reschedule(&model).selected, None);
}

#[test]
fn a_taken_time_reads_the_times_again_and_a_move_reads_the_list_again() {
    let mut model = listed();
    let effects = open_reschedule(&mut model, "bk-contract-1");
    answer(&mut model, sent(&effects)[0].0, slots());
    bookings(
        &mut model,
        BookingsEvent::PickSlot("2026-09-14T14:00:00Z".into()),
    );
    let effects = bookings(&mut model, BookingsEvent::Confirm);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::RescheduleBooking {
            id: "bk-contract-1".into(),
            start_at: "2026-09-14T14:00:00Z".into(),
        }
    );
    assert!(
        bookings(&mut model, BookingsEvent::Confirm).is_empty(),
        "one at a time"
    );
    let effects = answer(&mut model, *ticket, refused("slot_taken"));
    assert!(matches!(
        sent(&effects).as_slice(),
        [(_, SchedulingAdminRequest::Slots { .. })]
    ));
    let dialog = reschedule(&model);
    assert_eq!(
        dialog.write.failure().unwrap().message,
        "That time was just taken. Pick another."
    );
    assert!(!dialog.write.failure().unwrap().retryable);
    assert_eq!(
        (dialog.slots.value(), dialog.selected.as_ref()),
        (None, None)
    );

    answer(&mut model, sent(&effects)[0].0, slots());
    bookings(
        &mut model,
        BookingsEvent::PickSlot("2026-09-14T13:00:00Z".into()),
    );
    let effects = bookings(&mut model, BookingsEvent::Confirm);
    let mut moved = booking("bk-contract-1", "confirmed", Some("phone-consultation"));
    moved.start_at = "2026-09-14T14:00:00Z".into();
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Booking(moved)),
    );
    assert!(sent(&effects).iter().any(|(_, r)| is_page(r)));
    assert_eq!(
        reschedule(&model).write.notice(),
        Some("Booking moved to Sep 14, 2026, 10:00 a.m.")
    );
}

#[test]
fn a_move_whose_new_time_cannot_be_read_still_says_it_moved() {
    let mut model = listed();
    let effects = open_reschedule(&mut model, "bk-contract-1");
    answer(&mut model, sent(&effects)[0].0, slots());
    bookings(
        &mut model,
        BookingsEvent::PickSlot("2026-09-14T13:00:00Z".into()),
    );
    let effects = bookings(&mut model, BookingsEvent::Confirm);
    let mut moved = booking("bk-contract-1", "confirmed", None);
    moved.start_at = "soon".into();
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Booking(moved)),
    );
    assert_eq!(reschedule(&model).write.notice(), Some("Booking moved"));
}

#[test]
fn the_times_can_be_read_again_after_a_failure_only() {
    let mut model = listed();
    let effects = open_reschedule(&mut model, "bk-contract-1");
    assert!(bookings(&mut model, BookingsEvent::RetryDialogRead).is_empty());
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(reschedule(&model).slots.failure().is_some());
    let effects = bookings(&mut model, BookingsEvent::RetryDialogRead);
    assert!(matches!(
        sent(&effects).as_slice(),
        [(_, SchedulingAdminRequest::Slots { .. })]
    ));
    assert_eq!(reschedule(&model).slots, SectionLoad::Loading);
    assert_eq!(RescheduleDialog::title(), "Reschedule this booking?");
    assert_eq!(RescheduleDialog::confirm_label(), "Move booking");
}

#[test]
fn a_booking_without_its_event_type_is_not_offered_a_move() {
    let (mut model, effects) = context_read("agency");
    answer(
        &mut model,
        ticket_for(&effects, is_page),
        done(SchedulingAdminData::BookingPage(SchedulingBookingPage {
            items: vec![booking("b1", "confirmed", None)],
            ..SchedulingBookingPage::default()
        })),
    );
    assert!(!section(&model).rows(true, now())[0].can_reschedule);
    assert!(open_reschedule(&mut model, "b1").is_empty());
    assert!(section(&model).dialog.is_none());
}

fn reassign(model: &Model) -> &ReassignDialog {
    match dialog(model) {
        BookingDialog::Reassign(dialog) => dialog,
        other => panic!("{other:?}"),
    }
}

fn user(id: &str, name: &str, archived: bool) -> SchedulingUser {
    SchedulingUser {
        id: id.to_owned(),
        email: format!("{id}@example.com"),
        name: name.to_owned(),
        archived,
        ..SchedulingUser::default()
    }
}

fn open_reassign(model: &mut Model) -> Vec<Effect> {
    bookings(
        model,
        BookingsEvent::OpenReassign {
            booking_id: "bk-contract-1".into(),
            now: now(),
        },
    )
}

#[test]
fn a_booking_goes_to_a_host_who_is_neither_archived_nor_its_host_now() {
    let mut model = listed();
    let effects = open_reassign(&mut model);
    let requests = sent(&effects);
    let [
        (
            ticket,
            SchedulingAdminRequest::Users {
                include_archived: None,
            },
        ),
    ] = requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::Users(vec![
            user("sched-user-contract", "Contract Member", false),
            user("gone", "Gone", true),
            user("sam", "", false),
        ])),
    );
    let hosts = reassign(&model).hosts.value().unwrap();
    assert_eq!(hosts.len(), 1);
    assert_eq!(ReassignDialog::host_name(&hosts[0]), "sam@example.com");
    assert_eq!(ReassignDialog::host_name(&user("x", "Ann", false)), "Ann");
    assert!(
        bookings(&mut model, BookingsEvent::Confirm).is_empty(),
        "nobody chosen"
    );
    bookings(&mut model, BookingsEvent::PickHost("gone".into()));
    assert_eq!(reassign(&model).selected, None);
    bookings(&mut model, BookingsEvent::PickHost("sam".into()));
    assert!(reassign(&model).can_confirm());
    let effects = bookings(&mut model, BookingsEvent::Confirm);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::ReassignBooking {
            id: "bk-contract-1".into(),
            host_id: "sam".into(),
        }
    );
    answer(&mut model, *ticket, Err(server_error()));
    assert!(reassign(&model).write.failure().is_some());
    bookings(&mut model, BookingsEvent::PickHost("sam".into()));
    assert_eq!(
        reassign(&model).write,
        WriteState::Idle,
        "a new choice clears it"
    );
    let effects = bookings(&mut model, BookingsEvent::Confirm);
    let booking = booking("bk-contract-1", "confirmed", None);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Booking(booking)),
    );
    assert_eq!(reassign(&model).write.notice(), Some("Host changed"));
}

#[test]
fn handing_over_is_for_the_schedulers_administrator_alone() {
    let (mut model, effects) = open("agency", SchedulingSection::Bookings);
    let mut profile = me();
    profile.is_admin = false;
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::Me),
        done(SchedulingAdminData::Me(profile)),
    );
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::EventTypes),
        done(SchedulingAdminData::EventTypes(Vec::new())),
    );
    let effects = status_answer(&mut model, &effects, Ok(ready_status()));
    answer(
        &mut model,
        ticket_for(&effects, is_page),
        done(SchedulingAdminData::BookingPage(page())),
    );
    assert!(!section(&model).rows(true, now())[0].can_reassign);
    assert!(open_reassign(&mut model).is_empty());
}

#[test]
fn the_hosts_can_be_read_again_after_a_failure() {
    let mut model = listed();
    let effects = open_reassign(&mut model);
    answer(&mut model, sent(&effects)[0].0, refused("unavailable"));
    let effects = bookings(&mut model, BookingsEvent::RetryDialogRead);
    assert!(matches!(
        sent(&effects).as_slice(),
        [(_, SchedulingAdminRequest::Users { .. })]
    ));
    assert_eq!(ReassignDialog::title(), "Reassign this booking?");
    assert_eq!(ReassignDialog::no_hosts(), "No other host can take it.");
}

#[test]
fn a_write_that_lands_after_its_dialog_closed_still_reads_the_list_again() {
    let mut model = listed();
    let effects = open_reschedule(&mut model, "bk-contract-1");
    let slots_ticket = sent(&effects)[0].0;
    bookings(&mut model, BookingsEvent::CloseDialog);
    assert!(
        answer(&mut model, slots_ticket, slots()).is_empty(),
        "forgotten"
    );

    open_cancel(&mut model, "bk-contract-1");
    let effects = bookings(&mut model, BookingsEvent::Confirm);
    bookings(&mut model, BookingsEvent::CloseDialog);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Booking(booking(
            "bk-contract-1",
            "cancelled",
            None,
        ))),
    );
    assert!(sent(&effects).iter().any(|(_, r)| is_page(r)));
}

#[test]
fn the_dialogs_ignore_what_is_not_theirs() {
    let mut model = listed();
    // No dialog open.
    for event in [
        BookingsEvent::EditCancelReason("x".into()),
        BookingsEvent::PickDay(SchedulingDay::parse("2026-09-20").unwrap()),
        BookingsEvent::PickSlot("x".into()),
        BookingsEvent::PickHost("x".into()),
        BookingsEvent::Confirm,
    ] {
        assert!(bookings(&mut model, event).is_empty());
    }
    assert!(section(&model).dialog.is_none());
    // An answer for a dialog that has gone is kept nowhere.
    let effects = open_reassign(&mut model);
    open_cancel(&mut model, "bk-contract-1");
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Users(Vec::new())),
    );
    assert!(matches!(dialog(&model), BookingDialog::Cancel(_)));
}

#[test]
fn an_event_for_the_list_while_it_is_closed_does_nothing() {
    let (mut model, _) = open("agency", SchedulingSection::Overview);
    assert!(bookings(&mut model, BookingsEvent::LoadMore).is_empty());
    assert!(
        act(
            &mut model,
            SchedulingAdminEvent::Booking(BookingEvent::Confirm)
        )
        .is_empty()
    );
}

// One booking.

fn detail(model: &Model) -> &BookingDetailSection {
    admin(model).booking.as_ref().expect("a booking is open")
}

fn one(model: &mut Model, event: BookingEvent) -> Vec<Effect> {
    act(model, SchedulingAdminEvent::Booking(event))
}

fn answers() -> Result<SchedulingAdminReply, ApiError> {
    done(SchedulingAdminData::Answers(
        data::<SchedulingItems<_>>("district-scheduling-booking-answers.json").items,
    ))
}

#[test]
fn a_booking_shows_what_the_booker_answered_and_a_blank_says_so() {
    let (mut model, effects) = open(
        "agency",
        SchedulingSection::Booking {
            id: "bk-contract-1".into(),
        },
    );
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::BookingAnswers {
            id: "bk-contract-1".into()
        }
    );
    answer(&mut model, *ticket, answers());
    let rows = detail(&model).answers.value().unwrap();
    let values: Vec<&str> = rows
        .iter()
        .map(BookingDetailSection::answer_value)
        .collect();
    assert_eq!(values, ["District", "No answer"]);
    assert_eq!(BookingDetailSection::answers_heading(), "Answers");
    assert_eq!(
        BookingDetailSection::no_questions(),
        "This event type asks no questions."
    );
}

#[test]
fn a_booking_can_be_cancelled_from_its_page_and_its_answers_are_read_again() {
    let (mut model, effects) = open(
        "agency",
        SchedulingSection::Booking {
            id: "bk-contract-1".into(),
        },
    );
    answer(&mut model, sent(&effects)[0].0, answers());
    assert!(
        one(&mut model, BookingEvent::Confirm).is_empty(),
        "no dialog"
    );
    one(&mut model, BookingEvent::EditCancelReason("x".into()));
    one(&mut model, BookingEvent::OpenCancel);
    one(&mut model, BookingEvent::EditCancelReason("Away".into()));
    let effects = one(&mut model, BookingEvent::Confirm);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::CancelBooking { reason, .. })] = requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(reason.as_deref(), Some("Away"));
    answer(&mut model, *ticket, refused("conflict"));
    assert!(
        detail(&model)
            .cancel
            .as_ref()
            .unwrap()
            .write
            .failure()
            .is_some()
    );
    let effects = one(&mut model, BookingEvent::Confirm);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Booking(booking(
            "bk-contract-1",
            "cancelled",
            None,
        ))),
    );
    assert!(matches!(
        sent(&effects).as_slice(),
        [(_, SchedulingAdminRequest::BookingAnswers { .. })]
    ));
    assert_eq!(detail(&model).answers, SectionLoad::Loading);
    one(&mut model, BookingEvent::CloseDialog);
    assert!(detail(&model).cancel.is_none());
}

#[test]
fn a_viewer_reads_a_booking_and_is_offered_no_cancel() {
    let (mut model, _) = open(
        "viewer",
        SchedulingSection::Booking {
            id: "bk-contract-1".into(),
        },
    );
    one(&mut model, BookingEvent::OpenCancel);
    assert!(detail(&model).cancel.is_none());
}

#[test]
fn a_cancel_that_lands_after_its_dialog_closed_still_reads_the_answers_again() {
    let (mut model, _) = open(
        "agency",
        SchedulingSection::Booking {
            id: "bk-contract-1".into(),
        },
    );
    one(&mut model, BookingEvent::OpenCancel);
    let effects = one(&mut model, BookingEvent::Confirm);
    one(&mut model, BookingEvent::CloseDialog);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Booking(booking(
            "bk-contract-1",
            "cancelled",
            None,
        ))),
    );
    assert_eq!(sent(&effects).len(), 1);
}

#[test]
fn a_booking_and_its_list_keep_each_other_as_the_member_goes_back_and_forth() {
    let mut model = listed();
    model.update(Event::Navigate(Route::SchedulingAdmin(
        SchedulingSection::Booking {
            id: "bk-contract-1".into(),
        },
    )));
    assert!(admin(&model).bookings.is_some() && admin(&model).booking.is_some());
    let effects = model.update(Event::Back);
    assert!(effects.is_empty(), "the list is shown as it was");
    assert!(admin(&model).booking.is_none());
    assert_eq!(section(&model).list.value().unwrap().len(), 2);
    model.update(Event::Back);
    assert!(admin(&model).bookings.is_none());

    // Straight to a booking, then back: the list was never read, so it is.
    let (mut model, _) = open(
        "agency",
        SchedulingSection::Booking {
            id: "bk-contract-1".into(),
        },
    );
    let effects = model.update(Event::Back);
    assert!(
        sent(&effects)
            .iter()
            .any(|(_, r)| *r == SchedulingAdminRequest::Me)
    );
}

#[test]
fn the_dialogs_say_what_the_attendee_gets() {
    use district_core::scheduling_admin::CancelDialog;
    assert_eq!(CancelDialog::title(), "Cancel this booking?");
    assert_eq!(
        CancelDialog::body(),
        "The attendee gets a cancellation email."
    );
    assert_eq!(CancelDialog::reason_label(), "Reason");
    assert_eq!(
        CancelDialog::reason_hint(),
        "Optional. It goes in the email."
    );
    assert_eq!(CancelDialog::confirm_label(), "Cancel booking");
    assert_eq!(CancelDialog::keep_label(), "Keep it");
    assert_eq!(
        RescheduleDialog::body(),
        "The attendee gets an email with the new time."
    );
    assert_eq!(RescheduleDialog::day_label(), "Day");
    assert_eq!(RescheduleDialog::time_label(), "Time");
    assert_eq!(
        RescheduleDialog::no_slots(),
        "Nothing free that day. Try another."
    );
    assert_eq!(RescheduleDialog::cancel_label(), "Cancel");
    assert_eq!(
        ReassignDialog::body(),
        "The booking keeps its time. Both hosts get an email about the change."
    );
    assert_eq!(ReassignDialog::host_label(), "New host");
    assert_eq!(ReassignDialog::confirm_label(), "Change host");
    assert_eq!(ReassignDialog::cancel_label(), "Cancel");
}
