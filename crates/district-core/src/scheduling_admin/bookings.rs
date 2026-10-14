//! Bookings: every booking, a page at a time by view, and one booking's answers;
//! cancelling, moving and handing a booking to another host.
//!
//! The list is the one paged screen of the administration, by offset over live
//! data: a booking made between two pages can come twice, so rows are kept once
//! by id and the offset moves by the rows the scheduler sent. Before the first
//! page three things are read, because each changes how a row reads or what is
//! asked for: the profile (the zone, and whether the member administers the
//! scheduler), the event types (each row's name) and the status (whether to ask
//! for the whole tenancy's bookings, which the scheduler honours only for its
//! administrator).
//!
//! A booking can be acted on while it is confirmed and has not ended: a meeting
//! under way can still be cancelled. Moving one needs its event type's slug,
//! which the times are asked for by, so a booking without one is not offered a
//! move. Handing one to another host is offered only to the scheduler's own
//! administrator, the scheduler's flag and not the workspace role. Each write
//! answers the booking as it now is, and the list is read again.

use std::collections::BTreeSet;
use std::time::SystemTime;

use district_l10n::scheduling_strings as ss;
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingBooking, SchedulingBookingAnswer,
    SchedulingBookingQuery, SchedulingEventType, SchedulingSlot, SchedulingStatusResponse,
    SchedulingUser,
};

use super::clock::{self, SchedulingDay};
use super::overview::{booking_event_type_name, booking_who, load};
use super::{
    Answer, Out, Pending as AdminPending, SchedulingAdmin, SectionLoad, WriteState, is_slot_taken,
};
use crate::model::Tickets;
use crate::paging::Paging;
use crate::text::l;
use crate::workflows::Tone;

/// How many bookings a page asks for: a row is several lines, so a bigger page
/// buys waiting for rows nobody scrolls to. Far under the catalog's 200.
pub const BOOKINGS_PAGE_SIZE: i64 = 25;

/// The longest cancellation reason the catalog takes, in characters. Checked
/// here, because the service's refusal names the field and nothing a member
/// could act on.
pub const CANCEL_REASON_MAX: usize = 1000;

/// Which bookings the list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BookingsView {
    /// Confirmed and still to come, soonest first.
    #[default]
    Upcoming,
    /// Confirmed and over, newest first.
    Past,
    /// Cancelled, newest first.
    Cancelled,
    /// Every booking, newest first.
    All,
}

impl BookingsView {
    /// Every view, in the web's order.
    pub const ALL: [Self; 4] = [Self::Upcoming, Self::Past, Self::Cancelled, Self::All];

    /// The view's label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Upcoming => ss::booking_view_upcoming(l()),
            Self::Past => ss::booking_view_past(l()),
            Self::Cancelled => ss::booking_view_cancelled(l()),
            Self::All => ss::booking_view_all(l()),
        }
    }

    /// What the list says when the view has nothing: each view's own words.
    pub fn empty_body(self) -> &'static str {
        match self {
            Self::Upcoming | Self::All => ss::bookings_empty_upcoming(l()),
            Self::Past => ss::bookings_empty_past(l()),
            Self::Cancelled => ss::bookings_empty_cancelled(l()),
        }
    }

    /// The page's query. `all` narrows nothing, so it sends neither `when`
    /// nor `status`.
    fn query(self, offset: i64, all_hosts: bool) -> SchedulingBookingQuery {
        let (status, when) = match self {
            Self::Upcoming => (Some("confirmed"), Some("upcoming")),
            Self::Past => (Some("confirmed"), Some("past")),
            Self::Cancelled => (Some("cancelled"), None),
            Self::All => (None, None),
        };
        SchedulingBookingQuery {
            status: status.map(str::to_owned),
            when: when.map(str::to_owned),
            limit: Some(BOOKINGS_PAGE_SIZE),
            offset: Some(offset),
            all_hosts,
            order: Some(
                if self == Self::Upcoming {
                    "asc"
                } else {
                    "desc"
                }
                .to_owned(),
            ),
            ..SchedulingBookingQuery::default()
        }
    }
}

/// A booking status's label and badge tone. A status this build does not know
/// is shown as it came: it still means something to the operator.
pub fn status_label(status: &str) -> (String, Tone) {
    match status {
        "confirmed" => (ss::booking_status_confirmed(l()).to_owned(), Tone::Success),
        "cancelled" => (ss::booking_status_cancelled(l()).to_owned(), Tone::Danger),
        "rescheduled" => (
            ss::booking_status_rescheduled(l()).to_owned(),
            Tone::Neutral,
        ),
        other => (other.to_owned(), Tone::Neutral),
    }
}

/// Whether `booking` can still be cancelled, moved or reassigned: confirmed,
/// and not yet ended at `now`. Judged on the end, not the start.
pub fn is_actionable(booking: &SchedulingBooking, now: SystemTime) -> bool {
    booking.status == "confirmed" && clock::is_after(&booking.end_at, now)
}

/// One row of the list, as the screen draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookingRow {
    /// The booking's id, which its detail is opened by.
    pub id: String,
    /// When it starts, a date and a time in the profile's zone.
    pub when: String,
    /// Who, what and with whom: "Dana Booker · Phone consultation · Sam".
    pub subtitle: String,
    /// The first attendee's address under it, or empty.
    pub email: String,
    /// The status badge's label.
    pub status: String,
    /// Its tone.
    pub tone: Tone,
    /// Whether to offer "Cancel booking".
    pub can_cancel: bool,
    /// Whether to offer "Move booking".
    pub can_reschedule: bool,
    /// Whether to offer "Change host".
    pub can_reassign: bool,
}

/// The bookings list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BookingsSection {
    /// The view showing.
    pub view: BookingsView,
    /// The bookings read, once the first page is in; rows from later pages are
    /// added to it.
    pub list: SectionLoad<Vec<SchedulingBooking>>,
    /// Where the next page stands.
    pub paging: Paging,
    /// The zone the times are shown in: the profile's, else UTC.
    pub timezone: String,
    /// Whether the whole tenancy's bookings were asked for.
    pub can_manage: bool,
    /// Whether the member administers the scheduler, which alone decides
    /// whether a booking can be handed to another host. False when the
    /// profile could not be read, the safe direction.
    pub scheduler_is_admin: bool,
    /// The dialog open over the list.
    pub dialog: Option<BookingDialog>,
    event_types: Vec<SchedulingEventType>,
    seen: BTreeSet<String>,
    awaiting_status: bool,
    awaiting_me: bool,
    awaiting_event_types: bool,
}

impl BookingsSection {
    /// "Times in America/Toronto".
    pub fn times_in(&self) -> String {
        clock::times_in(&self.timezone)
    }

    /// The rows, once read, with the actions `can_change` (the member's
    /// [`Capabilities::can_change`](crate::Capabilities::can_change)) and `now`
    /// allow on each.
    pub fn rows(&self, can_change: bool, now: SystemTime) -> Vec<BookingRow> {
        let Some(bookings) = self.list.value() else {
            return Vec::new();
        };
        bookings
            .iter()
            .map(|booking| {
                let (status, tone) = status_label(&booking.status);
                let actionable = can_change && is_actionable(booking, now);
                BookingRow {
                    id: booking.id.clone(),
                    when: clock::date_time_or_unknown(&booking.start_at, &self.timezone),
                    subtitle: ss::booking_subtitle(
                        l(),
                        &booking_who(booking),
                        &booking_event_type_name(booking, &self.event_types),
                        booking
                            .host_name
                            .as_deref()
                            .unwrap_or(ss::booking_no_host(l())),
                    ),
                    email: booking
                        .attendees
                        .as_ref()
                        .and_then(|a| a.first())
                        .and_then(|a| a.email.as_deref())
                        .map(str::trim)
                        .unwrap_or_default()
                        .to_owned(),
                    status,
                    tone,
                    can_cancel: actionable,
                    can_reschedule: actionable && booking.event_type_slug.is_some(),
                    can_reassign: actionable && self.scheduler_is_admin,
                }
            })
            .collect()
    }

    /// The heading of the empty list.
    pub fn empty_title() -> &'static str {
        ss::bookings_empty_title(l())
    }

    /// The button at the end of the list: "Load more", or "Loading…" while
    /// the next page is on its way.
    pub fn load_more_label(&self) -> &'static str {
        if self.paging.loading_more {
            ss::bookings_loading_more(l())
        } else {
            ss::bookings_load_more(l())
        }
    }

    fn booking(&self, id: &str) -> Option<&SchedulingBooking> {
        self.list.value()?.iter().find(|b| b.id == id)
    }
}

/// A dialog over the bookings list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookingDialog {
    /// Cancelling a booking.
    Cancel(CancelDialog),
    /// Moving one to another time.
    Reschedule(RescheduleDialog),
    /// Handing one to another host.
    Reassign(ReassignDialog),
}

/// Cancelling one booking, with an optional reason for the attendee's email.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelDialog {
    /// The booking's id.
    pub booking_id: String,
    /// The reason, as typed.
    pub reason: String,
    /// Why the reason cannot be sent, before anything was sent.
    pub reason_rejected: Option<&'static str>,
    /// The cancel's state.
    pub write: WriteState,
}

impl CancelDialog {
    fn new(booking_id: String) -> Self {
        Self {
            booking_id,
            reason: String::new(),
            reason_rejected: None,
            write: WriteState::Idle,
        }
    }

    /// "Cancel this booking?"
    pub fn title() -> &'static str {
        ss::cancel_title(l())
    }

    /// What the attendee gets: a cancellation email.
    pub fn body() -> &'static str {
        ss::cancel_body(l())
    }

    /// The reason field's label.
    pub fn reason_label() -> &'static str {
        ss::cancel_reason_label(l())
    }

    /// Under the field: optional, and it goes in the email.
    pub fn reason_hint() -> &'static str {
        ss::cancel_reason_hint(l())
    }

    /// The confirm button: "Cancel booking". Destructive.
    pub fn confirm_label() -> &'static str {
        ss::cancel_confirm(l())
    }

    /// The way out: "Keep it".
    pub fn keep_label() -> &'static str {
        ss::cancel_keep(l())
    }

    fn edit(&mut self, reason: String) {
        self.reason = reason;
        self.reason_rejected = None;
        if self.write.failure().is_some() {
            self.write = WriteState::Idle;
        }
    }

    /// The request, or `None` while one is on its way or the reason is too
    /// long (then saying so). A blank reason is left out, not sent empty.
    fn submit(&mut self) -> Option<SchedulingAdminRequest> {
        if self.write.busy() {
            return None;
        }
        let reason = self.reason.trim();
        if reason.chars().count() > CANCEL_REASON_MAX {
            self.reason_rejected = Some(ss::cancel_reason_too_long(l()));
            return None;
        }
        self.write = WriteState::Working;
        Some(SchedulingAdminRequest::CancelBooking {
            id: self.booking_id.clone(),
            reason: Some(reason.to_owned()).filter(|r| !r.is_empty()),
        })
    }

    fn answered(&mut self, answer: &Answer) {
        self.write = match answer {
            Ok(_) => WriteState::Done(ss::cancel_done(l()).to_owned()),
            Err(failure) => WriteState::Failed(failure.clone()),
        };
    }
}

/// Moving one booking to another free time on a day: the times are the ones
/// the booking page offers, read for the one day in the profile's zone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RescheduleDialog {
    /// The booking's id.
    pub booking_id: String,
    /// Its event type's slug, which the times are read by.
    pub slug: String,
    /// The zone the day and the times are in.
    pub timezone: String,
    /// The day whose times show.
    pub day: SchedulingDay,
    /// Its free times.
    pub slots: SectionLoad<Vec<SchedulingSlot>>,
    /// The time chosen: a slot's start as it came, which is what is sent.
    pub selected: Option<String>,
    /// "Pick a time.", for a confirm with none chosen.
    pub selection_rejected: Option<&'static str>,
    /// The move's state.
    pub write: WriteState,
}

impl RescheduleDialog {
    /// "Reschedule this booking?"
    pub fn title() -> &'static str {
        ss::reschedule_title(l())
    }

    /// What the attendee gets: an email with the new time.
    pub fn body() -> &'static str {
        ss::reschedule_body(l())
    }

    /// The day picker's label.
    pub fn day_label() -> &'static str {
        ss::reschedule_day_label(l())
    }

    /// The times' label.
    pub fn time_label() -> &'static str {
        ss::reschedule_time_label(l())
    }

    /// Said when the day has no free time.
    pub fn no_slots() -> &'static str {
        ss::reschedule_no_slots(l())
    }

    /// The confirm button: "Move booking".
    pub fn confirm_label() -> &'static str {
        ss::reschedule_confirm(l())
    }

    /// The way out.
    pub fn cancel_label() -> &'static str {
        ss::reschedule_cancel(l())
    }

    /// A time's label: its date and time in the dialog's zone, or the value as
    /// it came when it does not parse, which still tells two apart.
    pub fn slot_label(&self, start: &str) -> String {
        clock::date_time(start, &self.timezone).unwrap_or_else(|| start.to_owned())
    }

    /// Whether the confirm can be pressed: a time is chosen and nothing is on
    /// its way.
    pub fn can_confirm(&self) -> bool {
        self.selected.is_some() && !self.write.busy()
    }

    fn slots_request(&self) -> SchedulingAdminRequest {
        // One day, `from` and `to` the same, as the web's table asks; and the
        // zone always, or the scheduler would choose one.
        SchedulingAdminRequest::Slots {
            slug: self.slug.clone(),
            from: Some(self.day.key()),
            to: Some(self.day.key()),
            tz: Some(self.timezone.clone()),
        }
    }
}

/// Handing one booking to another host: the scheduler's users who are not
/// archived and not its host now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReassignDialog {
    /// The booking's id.
    pub booking_id: String,
    /// Its host now, left out of the candidates.
    pub current_host: Option<String>,
    /// The candidates.
    pub hosts: SectionLoad<Vec<SchedulingUser>>,
    /// The candidate chosen.
    pub selected: Option<String>,
    /// The hand-over's state.
    pub write: WriteState,
}

impl ReassignDialog {
    /// "Reassign this booking?"
    pub fn title() -> &'static str {
        ss::reassign_title(l())
    }

    /// The booking keeps its time; both hosts get an email.
    pub fn body() -> &'static str {
        ss::reassign_body(l())
    }

    /// The host list's label.
    pub fn host_label() -> &'static str {
        ss::reassign_host_label(l())
    }

    /// Said when nobody else can take it: a fact, not a failure.
    pub fn no_hosts() -> &'static str {
        ss::reassign_no_hosts(l())
    }

    /// The confirm button: "Change host".
    pub fn confirm_label() -> &'static str {
        ss::reassign_confirm(l())
    }

    /// The way out.
    pub fn cancel_label() -> &'static str {
        ss::reassign_cancel(l())
    }

    /// A candidate's label: their name, or their address for a blank one.
    pub fn host_name(user: &SchedulingUser) -> &str {
        if user.name.is_empty() {
            &user.email
        } else {
            &user.name
        }
    }

    /// Whether the confirm can be pressed.
    pub fn can_confirm(&self) -> bool {
        self.selected.is_some() && !self.write.busy()
    }
}

/// What the member does on the bookings list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookingsEvent {
    /// Show another view, read from its first page.
    SelectView(BookingsView),
    /// Read the next page.
    LoadMore,
    /// Open the cancel dialog for a booking, if it can be acted on at `now`.
    OpenCancel {
        /// The booking's id.
        booking_id: String,
        /// The time now.
        now: SystemTime,
    },
    /// Open the move dialog, on the day `now` falls on in the profile's zone.
    OpenReschedule {
        /// The booking's id.
        booking_id: String,
        /// The time now.
        now: SystemTime,
    },
    /// Open the host dialog.
    OpenReassign {
        /// The booking's id.
        booking_id: String,
        /// The time now.
        now: SystemTime,
    },
    /// The cancel dialog's reason changed.
    EditCancelReason(String),
    /// The move dialog's day changed: its times are read again.
    PickDay(SchedulingDay),
    /// A time was chosen, by its start.
    PickSlot(String),
    /// A host was chosen, by their scheduler user id.
    PickHost(String),
    /// Read the dialog's times or hosts again, after a failure.
    RetryDialogRead,
    /// The dialog's confirm.
    Confirm,
    /// Close the dialog. A write on its way still lands, and the list is read
    /// again when it does.
    CloseDialog,
}

/// What the member does on one booking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookingEvent {
    /// Open the cancel dialog.
    OpenCancel,
    /// Its reason changed.
    EditCancelReason(String),
    /// Its confirm.
    Confirm,
    /// Close it.
    CloseDialog,
}

/// One booking: what the booker answered, and the cancel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookingDetailSection {
    /// The booking's id.
    pub id: String,
    /// The answers to the event type's questions.
    pub answers: SectionLoad<Vec<SchedulingBookingAnswer>>,
    /// The cancel dialog, while it is open.
    pub cancel: Option<CancelDialog>,
}

impl BookingDetailSection {
    /// The heading over the cancel button.
    pub fn manage_heading() -> &'static str {
        ss::booking_manage(l())
    }

    /// The answers' heading.
    pub fn answers_heading() -> &'static str {
        ss::answers_eyebrow(l())
    }

    /// Said when the event type asks nothing.
    pub fn no_questions() -> &'static str {
        ss::event_type_no_questions(l())
    }

    /// An answer as shown: a blank one says so, rather than looking lost.
    pub fn answer_value(answer: &SchedulingBookingAnswer) -> &str {
        if answer.value.trim().is_empty() {
            ss::booking_answer_blank(l())
        } else {
            &answer.value
        }
    }
}

/// What an answer the bookings await is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    Me,
    EventTypes,
    Page { first: bool },
    Slots,
    Users,
    Write,
}

/// What an answer one booking awaits is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DetailPending {
    Answers,
    Cancel,
}

fn send(pending: Pending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::Bookings(pending), Box::new(request))
}

fn send_detail(pending: DetailPending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::Booking(pending), Box::new(request))
}

/// Opens the list: a fresh one, its profile and event types read, and the
/// status awaited; the first page waits for all three.
pub(crate) fn enter(admin: &mut SchedulingAdmin, tickets: &mut Tickets) -> Vec<Out> {
    admin.forget(tickets, |p| matches!(p, AdminPending::Bookings(_)));
    admin.bookings = Some(BookingsSection {
        timezone: "UTC".to_owned(),
        awaiting_status: true,
        awaiting_me: true,
        awaiting_event_types: true,
        ..BookingsSection::default()
    });
    vec![
        send(Pending::Me, SchedulingAdminRequest::Me),
        send(Pending::EventTypes, SchedulingAdminRequest::EventTypes),
    ]
}

/// The first page, if every read it waits for is in.
fn first_page(section: &mut BookingsSection) -> Vec<Out> {
    if section.awaiting_status || section.awaiting_me || section.awaiting_event_types {
        return Vec::new();
    }
    section.list = SectionLoad::Loading;
    section.paging = Paging::default();
    section.seen.clear();
    vec![send(
        Pending::Page { first: true },
        SchedulingAdminRequest::Bookings(section.view.query(0, section.can_manage)),
    )]
}

/// The list read from its first page again, after a write or a change of view,
/// any page on its way forgotten.
fn reload(admin: &mut SchedulingAdmin, tickets: &mut Tickets) -> Vec<Out> {
    admin.forget(tickets, |p| {
        matches!(p, AdminPending::Bookings(Pending::Page { .. }))
    });
    admin.bookings.as_mut().map(first_page).unwrap_or_default()
}

pub(crate) fn status_arrived(
    admin: &mut SchedulingAdmin,
    status: Option<&SchedulingStatusResponse>,
) -> Vec<Out> {
    let Some(section) = admin.bookings.as_mut().filter(|s| s.awaiting_status) else {
        return Vec::new();
    };
    section.awaiting_status = false;
    section.can_manage = status.is_some_and(|s| s.can_manage);
    first_page(section)
}

pub(crate) fn event(
    admin: &mut SchedulingAdmin,
    tickets: &mut Tickets,
    event: BookingsEvent,
    can_change: bool,
) -> Vec<Out> {
    let Some(section) = admin.bookings.as_mut() else {
        return Vec::new();
    };
    match event {
        BookingsEvent::SelectView(view) if view != section.view => {
            section.view = view;
            reload(admin, tickets)
        }
        BookingsEvent::LoadMore
            if section.list.value().is_some() && section.paging.can_load_more() =>
        {
            let offset = section.paging.start_more();
            vec![send(
                Pending::Page { first: false },
                SchedulingAdminRequest::Bookings(
                    section.view.query(i64::from(offset), section.can_manage),
                ),
            )]
        }
        BookingsEvent::OpenCancel { booking_id, now } => {
            if open_allowed(section, &booking_id, can_change, now).is_some() {
                section.dialog = Some(BookingDialog::Cancel(CancelDialog::new(booking_id)));
            }
            Vec::new()
        }
        BookingsEvent::OpenReschedule { booking_id, now } => {
            let Some(slug) = open_allowed(section, &booking_id, can_change, now)
                .and_then(|b| b.event_type_slug.clone())
            else {
                return Vec::new();
            };
            let dialog = RescheduleDialog {
                booking_id,
                slug,
                day: SchedulingDay::today(now, &section.timezone),
                timezone: section.timezone.clone(),
                slots: SectionLoad::Loading,
                selected: None,
                selection_rejected: None,
                write: WriteState::Idle,
            };
            let request = dialog.slots_request();
            section.dialog = Some(BookingDialog::Reschedule(dialog));
            vec![send(Pending::Slots, request)]
        }
        BookingsEvent::OpenReassign { booking_id, now } => {
            let Some(booking) = open_allowed(section, &booking_id, can_change, now)
                .filter(|_| section.scheduler_is_admin)
            else {
                return Vec::new();
            };
            section.dialog = Some(BookingDialog::Reassign(ReassignDialog {
                current_host: booking.host_id.clone(),
                booking_id,
                hosts: SectionLoad::Loading,
                selected: None,
                write: WriteState::Idle,
            }));
            vec![users()]
        }
        BookingsEvent::EditCancelReason(reason) => {
            if let Some(BookingDialog::Cancel(dialog)) = &mut section.dialog {
                dialog.edit(reason);
            }
            Vec::new()
        }
        BookingsEvent::PickDay(day) => match &mut section.dialog {
            Some(BookingDialog::Reschedule(dialog)) if dialog.day != day => {
                dialog.day = day;
                dialog.selected = None;
                dialog.selection_rejected = None;
                dialog.slots = SectionLoad::Loading;
                let request = dialog.slots_request();
                admin.forget(tickets, |p| {
                    matches!(p, AdminPending::Bookings(Pending::Slots))
                });
                vec![send(Pending::Slots, request)]
            }
            _ => Vec::new(),
        },
        BookingsEvent::PickSlot(start) => {
            if let Some(BookingDialog::Reschedule(dialog)) = &mut section.dialog
                && dialog
                    .slots
                    .value()
                    .is_some_and(|slots| slots.iter().any(|s| s.start == start))
            {
                dialog.selected = Some(start);
                dialog.selection_rejected = None;
            }
            Vec::new()
        }
        BookingsEvent::PickHost(id) => {
            if let Some(BookingDialog::Reassign(dialog)) = &mut section.dialog
                && dialog
                    .hosts
                    .value()
                    .is_some_and(|hosts| hosts.iter().any(|h| h.id == id))
            {
                dialog.selected = Some(id);
                if dialog.write.failure().is_some() {
                    dialog.write = WriteState::Idle;
                }
            }
            Vec::new()
        }
        BookingsEvent::RetryDialogRead => match &mut section.dialog {
            Some(BookingDialog::Reschedule(dialog)) if dialog.slots.failure().is_some() => {
                dialog.slots = SectionLoad::Loading;
                vec![send(Pending::Slots, dialog.slots_request())]
            }
            Some(BookingDialog::Reassign(dialog)) if dialog.hosts.failure().is_some() => {
                dialog.hosts = SectionLoad::Loading;
                vec![users()]
            }
            _ => Vec::new(),
        },
        BookingsEvent::Confirm => confirm(section),
        BookingsEvent::CloseDialog => {
            section.dialog = None;
            admin.forget(tickets, |p| {
                matches!(p, AdminPending::Bookings(Pending::Slots | Pending::Users))
            });
            Vec::new()
        }
        BookingsEvent::SelectView(_) | BookingsEvent::LoadMore => Vec::new(),
    }
}

fn users() -> Out {
    send(
        Pending::Users,
        SchedulingAdminRequest::Users {
            include_archived: None,
        },
    )
}

/// The booking `id`, if the member may act on it: they may change things, it
/// is listed, and it is confirmed and not over.
fn open_allowed<'a>(
    section: &'a BookingsSection,
    id: &str,
    can_change: bool,
    now: SystemTime,
) -> Option<&'a SchedulingBooking> {
    section
        .booking(id)
        .filter(|booking| can_change && is_actionable(booking, now))
}

fn confirm(section: &mut BookingsSection) -> Vec<Out> {
    let request = match &mut section.dialog {
        Some(BookingDialog::Cancel(dialog)) => dialog.submit(),
        Some(BookingDialog::Reschedule(dialog)) if !dialog.write.busy() => {
            match dialog.selected.clone() {
                None => {
                    dialog.selection_rejected = Some(ss::reschedule_pick_time(l()));
                    None
                }
                Some(start_at) => {
                    dialog.write = WriteState::Working;
                    Some(SchedulingAdminRequest::RescheduleBooking {
                        id: dialog.booking_id.clone(),
                        start_at,
                    })
                }
            }
        }
        Some(BookingDialog::Reassign(dialog)) if dialog.can_confirm() => {
            dialog.write = WriteState::Working;
            dialog
                .selected
                .clone()
                .map(|host_id| SchedulingAdminRequest::ReassignBooking {
                    id: dialog.booking_id.clone(),
                    host_id,
                })
        }
        _ => None,
    };
    request
        .map(|request| vec![send(Pending::Write, request)])
        .unwrap_or_default()
}

pub(crate) fn answered(
    admin: &mut SchedulingAdmin,
    tickets: &mut Tickets,
    pending: Pending,
    answer: Answer,
) -> Vec<Out> {
    match pending {
        Pending::Write => write_answered(admin, tickets, answer),
        // A read's answers are forgotten when the list closes.
        read => admin
            .bookings
            .as_mut()
            .map(|section| read_answered(section, read, answer))
            .unwrap_or_default(),
    }
}

fn read_answered(section: &mut BookingsSection, pending: Pending, answer: Answer) -> Vec<Out> {
    match pending {
        // The profile and the event types change how rows read; neither's
        // failure stops the list (UTC, slugs as they came, no reassign).
        Pending::Me => {
            section.awaiting_me = false;
            if let Ok(SchedulingAdminData::Me(me)) = answer {
                section.timezone = me.display_timezone().to_owned();
                section.scheduler_is_admin = me.is_admin;
            }
            first_page(section)
        }
        Pending::EventTypes => {
            section.awaiting_event_types = false;
            if let Ok(SchedulingAdminData::EventTypes(items)) = answer {
                section.event_types = items;
            }
            first_page(section)
        }
        Pending::Page { first } => {
            page_answered(section, first, answer);
            Vec::new()
        }
        Pending::Slots => {
            if let Some(BookingDialog::Reschedule(dialog)) = &mut section.dialog {
                dialog.slots = load(answer, |data| data!(Slots)(data).map(|s| s.slots));
            }
            Vec::new()
        }
        // `Users`, and `Write`, which `answered` takes first.
        _ => {
            if let Some(BookingDialog::Reassign(dialog)) = &mut section.dialog {
                let current = dialog.current_host.clone();
                dialog.hosts = load(answer, |data| {
                    data!(Users)(data).map(|users| {
                        users
                            .into_iter()
                            .filter(|u| !u.archived && Some(&u.id) != current.as_ref())
                            .collect()
                    })
                });
            }
            Vec::new()
        }
    }
}

/// A page arrived. A failed first page is the list's failure; a failed later
/// page keeps the rows read, with why at their end.
fn page_answered(section: &mut BookingsSection, first: bool, answer: Answer) {
    let page = answer.and_then(|data| {
        data!(BookingPage)(data).ok_or_else(crate::failure::FailureText::unexpected)
    });
    section.paging.loading_more = false;
    let page = match page {
        Ok(page) => page,
        Err(failure) if first => {
            section.list = SectionLoad::Failed(failure);
            return;
        }
        Err(failure) => {
            section.paging.more_failure = Some(failure);
            return;
        }
    };
    let received = page.items.len() as i64;
    let fresh: Vec<SchedulingBooking> = page
        .items
        .into_iter()
        .filter(|b| section.seen.insert(b.id.clone()))
        .collect();
    // The offset moves by what the scheduler sent, not by what was kept.
    let offset = i64::from(section.paging.next_offset) + received;
    section.paging.next_offset = u32::try_from(offset).unwrap_or(u32::MAX);
    section.paging.end_reached =
        received < BOOKINGS_PAGE_SIZE || page.total.is_some_and(|total| offset >= total);
    match &mut section.list {
        SectionLoad::Ready(rows) if !first => rows.extend(fresh),
        list => *list = SectionLoad::Ready(fresh),
    }
}

/// The dialog open over the list says what came of its write.
fn dialog_answered(section: &mut BookingsSection, answer: &Answer) -> Vec<Out> {
    let mut outs = Vec::new();
    match &mut section.dialog {
        Some(BookingDialog::Cancel(dialog)) => dialog.answered(answer),
        Some(BookingDialog::Reschedule(dialog)) => {
            let when = match answer {
                Ok(SchedulingAdminData::Booking(booking)) => {
                    clock::date_time(&booking.start_at, &dialog.timezone)
                }
                _ => None,
            };
            dialog.write = match answer {
                Ok(_) => WriteState::Done(when.map_or_else(
                    || ss::reschedule_done(l()).to_owned(),
                    |when| ss::reschedule_done_at(l(), &when),
                )),
                Err(failure) => WriteState::Failed(failure.clone()),
            };
            if let Err(failure) = answer
                && is_slot_taken(failure)
            {
                dialog.slots = SectionLoad::Loading;
                dialog.selected = None;
                outs.push(send(Pending::Slots, dialog.slots_request()));
            }
        }
        Some(BookingDialog::Reassign(dialog)) => {
            dialog.write = match answer {
                Ok(_) => WriteState::Done(ss::reassign_done(l()).to_owned()),
                Err(failure) => WriteState::Failed(failure.clone()),
            };
        }
        None => {}
    }
    outs
}

/// A write landed or failed: the dialog says so, and on success the list is
/// read again (the dialog may have been closed meanwhile). A time someone else
/// took means the times shown are stale: they are read again.
fn write_answered(admin: &mut SchedulingAdmin, tickets: &mut Tickets, answer: Answer) -> Vec<Out> {
    // The list is open: leaving it stops the write being awaited.
    let mut outs = admin
        .bookings
        .as_mut()
        .map(|section| dialog_answered(section, &answer))
        .unwrap_or_default();
    if answer.is_ok() {
        outs.extend(reload(admin, tickets));
    }
    outs
}

/// Opens one booking: its answers read.
pub(crate) fn enter_detail(
    admin: &mut SchedulingAdmin,
    tickets: &mut Tickets,
    id: String,
) -> Vec<Out> {
    admin.forget(tickets, |p| {
        matches!(p, AdminPending::Booking(DetailPending::Answers))
    });
    let request = SchedulingAdminRequest::BookingAnswers { id: id.clone() };
    admin.booking = Some(BookingDetailSection {
        id,
        answers: SectionLoad::Loading,
        cancel: None,
    });
    vec![send_detail(DetailPending::Answers, request)]
}

pub(crate) fn detail_event(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: BookingEvent,
    can_change: bool,
) -> Vec<Out> {
    let Some(section) = admin.booking.as_mut() else {
        return Vec::new();
    };
    match event {
        BookingEvent::OpenCancel if can_change => {
            section.cancel = Some(CancelDialog::new(section.id.clone()));
            Vec::new()
        }
        BookingEvent::EditCancelReason(reason) => {
            if let Some(dialog) = &mut section.cancel {
                dialog.edit(reason);
            }
            Vec::new()
        }
        BookingEvent::Confirm => section
            .cancel
            .as_mut()
            .and_then(CancelDialog::submit)
            .map(|request| vec![send_detail(DetailPending::Cancel, request)])
            .unwrap_or_default(),
        BookingEvent::CloseDialog => {
            section.cancel = None;
            Vec::new()
        }
        BookingEvent::OpenCancel => Vec::new(),
    }
}

pub(crate) fn detail_answered(
    admin: &mut SchedulingAdmin,
    pending: DetailPending,
    answer: Answer,
) -> Vec<Out> {
    // The booking's answers are forgotten when it closes.
    admin
        .booking
        .as_mut()
        .map(|section| match pending {
            DetailPending::Answers => {
                section.answers = load(answer, data!(Answers));
                Vec::new()
            }
            DetailPending::Cancel => {
                let done = answer.is_ok();
                if let Some(dialog) = &mut section.cancel {
                    dialog.answered(&answer);
                }
                if done {
                    section.answers = SectionLoad::Loading;
                    vec![send_detail(
                        DetailPending::Answers,
                        SchedulingAdminRequest::BookingAnswers {
                            id: section.id.clone(),
                        },
                    )]
                } else {
                    Vec::new()
                }
            }
        })
        .unwrap_or_default()
}
