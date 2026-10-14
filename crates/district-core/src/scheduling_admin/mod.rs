//! The booking scheduler's administration, natively: the overview, bookings,
//! event types, working hours, calendars, the team, the developer settings and
//! the booking pages' settings, each a section under the booking pages screen
//! ([`Route::Scheduling`](crate::Route::Scheduling)).
//!
//! Every read and every write is one operation of the service's catalog
//! ([`SchedulingAdminRequest`]), sent as [`Effect::SchedulingAdmin`] and
//! answered in [`Event::SchedulingAdminAnswered`](crate::Event::SchedulingAdminAnswered).
//! A section keeps its state while it, or a detail under it, shows: going from
//! the bookings to one booking and back finds the list as it was.
//!
//! # Who may do what
//!
//! Every section's reads admit a viewer. The writes need a member who may change
//! things ([`Capabilities::can_change`](crate::Capabilities::can_change)), except
//! the member's own: their scheduler profile, picture and notifications, and
//! their calendars, which a viewer may change too. Handing a booking to another
//! host is narrower again, and on another authority: the scheduler's own
//! administrator flag (`me.get`'s `is_admin`), never the workspace role. None of
//! this is a boundary; the service and the scheduler check every request.
//!
//! # Failures
//!
//! A failure is never shown as an empty section: every list here can be empty
//! for real. Every refusal reads as one of five sentences, the web dashboard's
//! own ([`admin_failure`]), and only the two a retry can fix offer one.
//!
//! # Time
//!
//! Every time is shown in the zone on the member's scheduler profile, not the
//! computer's, because that is the zone the customer was told; each screen that
//! shows one names it. Formats are `district_l10n::format`'s. A decision that
//! depends on the time (whether a booking can still be cancelled, which day is
//! today) takes it from the caller, so it is the app's clock and a test's.

/// The value in an answer of the variant named, as a closure for
/// [`overview::load`]: an answer of another variant (which the request's own
/// decoding rules out) reads as `None`.
macro_rules! data {
    ($variant:ident) => {
        |data: district_model::SchedulingAdminData| match data {
            district_model::SchedulingAdminData::$variant(value) => Some(value),
            _ => None,
        }
    };
}

mod bookings;
mod calendar;
mod clock;
mod developer;
mod event_types;
mod hours;
mod overview;
mod settings;
mod team;

use district_api::{ApiError, TransportKind};
use district_l10n::scheduling_strings as ss;
use district_model::{
    CODE_INVALID_PARAMS, CODE_SCHEDULING_NOT_READY, SchedulingAdminData, SchedulingAdminFailure,
    SchedulingAdminReply, SchedulingAdminRequest, SchedulingSecret,
};

pub use bookings::{
    BOOKINGS_PAGE_SIZE, BookingDetailSection, BookingDialog, BookingEvent, BookingRow,
    BookingsEvent, BookingsSection, BookingsView, CANCEL_REASON_MAX, CancelDialog, ReassignDialog,
    RescheduleDialog, is_actionable, status_label,
};
pub use calendar::{CalendarEvent, CalendarSection, provider_label};
pub use clock::{SchedulingDay, times_in};
pub use developer::{DeveloperEvent, DeveloperSection};
pub use event_types::{
    ActionsDialog, DEFAULT_LOCATION, EventTypeDialog, EventTypeEditor, EventTypeEvent,
    EventTypeForm, EventTypeRow, EventTypeSection, EventTypesEvent, EventTypesSection, HostRow,
    HostsDialog, OFFERED_LOCATIONS, QuestionForm, QuestionsDialog, location_label, minutes_label,
    slug_from_name, state_label, unique_slug,
};
pub use hours::{HoursEvent, HoursSection};
pub use overview::{COMING_UP_LIMIT, ComingUpRow, DeskContent, DeskRow, OverviewSection};
pub use settings::{SettingsEvent, SettingsSection};
pub use team::{TeamEvent, TeamSection};

use crate::failure::FailureText;
use crate::model::{CoreConfig, Effect, Slot, Ticket, Tickets};
use crate::route::Route;
use crate::scheduling::SchedulingStatus;
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::l;

/// A section of the booking scheduler's administration, or a detail under one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SchedulingSection {
    /// The register: the booking desk at a glance, and what is coming up.
    Overview,
    /// The event types.
    EventTypes,
    /// One event type, addressed by its slug.
    EventType {
        /// The event type's slug, the scheduler's own key for it.
        slug: String,
    },
    /// The weekly hours, and the dated overrides.
    Hours,
    /// Every booking, a page at a time.
    Bookings,
    /// One booking, addressed by its id.
    Booking {
        /// The booking's id.
        id: String,
    },
    /// The member's own calendars.
    Calendar,
    /// The scheduler's users and teams.
    Team,
    /// The member's profile and notifications, the booking pages' branding and
    /// the booking assistant.
    Settings,
    /// API keys, connected apps and webhooks.
    Developer,
}

impl SchedulingSection {
    /// The sections the booking pages screen lists, in the web dashboard's
    /// order. The two details have no row: each needs an id the list holds.
    pub fn listed() -> [Self; 8] {
        [
            Self::Overview,
            Self::EventTypes,
            Self::Hours,
            Self::Bookings,
            Self::Calendar,
            Self::Team,
            Self::Settings,
            Self::Developer,
        ]
    }

    /// The section's title, the web dashboard's own name for it.
    pub fn title(&self) -> &'static str {
        match self {
            Self::Overview => ss::section_title_overview(l()),
            Self::EventTypes => ss::section_title_event_types(l()),
            Self::EventType { .. } => ss::section_title_event_type(l()),
            Self::Hours => ss::section_title_hours(l()),
            Self::Bookings => ss::section_title_bookings(l()),
            Self::Booking { .. } => ss::section_title_booking(l()),
            Self::Calendar => ss::section_title_calendar(l()),
            Self::Team => ss::section_title_team(l()),
            Self::Settings => ss::section_title_settings(l()),
            Self::Developer => ss::section_title_developer(l()),
        }
    }

    /// The row's second line on the booking pages screen: what the section
    /// holds. `None` for a detail, which has no row.
    pub fn subtitle(&self) -> Option<&'static str> {
        Some(match self {
            Self::Overview => ss::section_subtitle_overview(l()),
            Self::EventTypes => ss::section_subtitle_event_types(l()),
            Self::Hours => ss::section_subtitle_hours(l()),
            Self::Bookings => ss::section_subtitle_bookings(l()),
            Self::Calendar => ss::section_subtitle_calendar(l()),
            Self::Team => ss::section_subtitle_team(l()),
            Self::Settings => ss::section_subtitle_settings(l()),
            Self::Developer => ss::section_subtitle_developer(l()),
            Self::EventType { .. } | Self::Booking { .. } => return None,
        })
    }

    /// Where going back leads: a detail to its list, a section to the booking
    /// pages screen.
    pub fn parent(&self) -> Route {
        match self {
            Self::EventType { .. } => Route::SchedulingAdmin(Self::EventTypes),
            Self::Booking { .. } => Route::SchedulingAdmin(Self::Bookings),
            _ => Route::Scheduling,
        }
    }
}

/// What one read of a section stands at. A failed read is its own state and is
/// never shown as an empty list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SectionLoad<T> {
    /// Being read, with nothing to show yet.
    #[default]
    Loading,
    /// Read.
    Ready(T),
    /// The read failed.
    Failed(FailureText),
}

impl<T> SectionLoad<T> {
    /// The value, once read.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }

    /// Why the read failed, if it did.
    pub fn failure(&self) -> Option<&FailureText> {
        match self {
            Self::Failed(failure) => Some(failure),
            _ => None,
        }
    }

    fn from_result(result: Result<T, FailureText>) -> Self {
        match result {
            Ok(value) => Self::Ready(value),
            Err(failure) => Self::Failed(failure),
        }
    }
}

/// What a write (a dialog's confirm, a form's save) stands at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum WriteState {
    /// Nothing sent yet, or the last outcome dismissed.
    #[default]
    Idle,
    /// On its way. The confirm is disabled; the way out is not.
    Working,
    /// Done, with the sentence that says so.
    Done(String),
    /// Refused or failed. The edits are kept.
    Failed(FailureText),
}

impl WriteState {
    /// Whether it is on its way: one at a time.
    pub fn busy(&self) -> bool {
        *self == Self::Working
    }

    /// The sentence for a write that landed.
    pub fn notice(&self) -> Option<&str> {
        match self {
            Self::Done(notice) => Some(notice),
            _ => None,
        }
    }

    /// Why it failed.
    pub fn failure(&self) -> Option<&FailureText> {
        match self {
            Self::Failed(failure) => Some(failure),
            _ => None,
        }
    }
}

/// The sentence for a refusal of the scheduling administration, in one of the
/// web dashboard's five: the scheduler did not answer (try again), the time was
/// taken, the role may not, there are no booking pages, or anything else (try
/// again). A request that never left is the network's sentence, an answer this
/// build cannot read says to update, and parameters the schema refused say
/// "that did not save" without offering the same request again.
pub fn admin_failure(error: &ApiError) -> FailureText {
    match error {
        ApiError::Offline(transport)
            if !matches!(transport.kind, TransportKind::NotJson { .. }) =>
        {
            FailureText::retryable(ss::failure_offline(l()))
        }
        ApiError::Decode { .. } | ApiError::Unconfirmed { .. } => {
            FailureText::final_(ss::failure_stale_build(l()))
        }
        ApiError::Forbidden(_) => FailureText::final_(ss::failure_forbidden(l())),
        error if error.code() == Some(CODE_SCHEDULING_NOT_READY) => {
            FailureText::final_(ss::failure_not_ready(l()))
        }
        error if error.code() == Some(CODE_INVALID_PARAMS) => {
            FailureText::final_(ss::failure_unknown(l()))
        }
        ApiError::RateLimited { .. }
        | ApiError::Server { .. }
        | ApiError::Rejected { status: 413, .. } => {
            FailureText::retryable(ss::failure_unavailable(l()))
        }
        ApiError::Envelope { status, .. } if *status >= 500 => {
            FailureText::retryable(ss::failure_unavailable(l()))
        }
        // The session's own failures read as they do everywhere: signing in
        // again, a token refused on a write (a press sends a fresh one), a
        // keyring that is locked.
        ApiError::Unauthorized(_) | ApiError::TokenUnavailable(_) => {
            FailureText::from_api_error(error)
        }
        _ => FailureText::retryable(ss::failure_unknown(l())),
    }
}

/// The sentence for the scheduler's own refusal (the route's 200 with `ok:
/// false`): its kind decides, and only two kinds have a sentence of their own.
pub fn refusal_failure(failure: &SchedulingAdminFailure) -> FailureText {
    if failure.is_unavailable() {
        FailureText::retryable(ss::failure_unavailable(l()))
    } else if failure.is_slot_taken() {
        FailureText::final_(ss::failure_slot_taken(l()))
    } else {
        FailureText::retryable(ss::failure_unknown(l()))
    }
}

/// An answer, as the data or the sentence for why there is none.
pub(crate) type Answer = Result<SchedulingAdminData, FailureText>;

/// Whether a failure was the scheduler refusing a time someone else took.
pub(crate) fn is_slot_taken(failure: &FailureText) -> bool {
    failure.message == ss::failure_slot_taken(l())
}

fn answer(result: Result<SchedulingAdminReply, ApiError>) -> Answer {
    match result {
        Ok(SchedulingAdminReply::Done(data)) => Ok(data),
        Ok(SchedulingAdminReply::Refused(failure)) => Err(refusal_failure(&failure)),
        Err(error) => Err(admin_failure(&error)),
    }
}

/// What the member does in the scheduling administration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedulingAdminEvent {
    /// On the bookings list.
    Bookings(BookingsEvent),
    /// On one booking.
    Booking(BookingEvent),
    /// On the event types list.
    EventTypes(EventTypesEvent),
    /// On one event type.
    EventType(EventTypeEvent),
    /// On the working hours.
    Hours(HoursEvent),
    /// On the calendars.
    Calendar(CalendarEvent),
    /// On the team.
    Team(TeamEvent),
    /// On the settings.
    Settings(SettingsEvent),
    /// On the developer settings.
    Developer(DeveloperEvent),
}

/// What an answer awaited is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    Overview(overview::Pending),
    Bookings(bookings::Pending),
    Booking(bookings::DetailPending),
    EventTypes(event_types::Pending),
    EventType(event_types::DetailPending),
    Hours(hours::Pending),
    Calendar(calendar::Pending),
    Team(team::Pending),
    Settings(settings::Pending),
    Developer(developer::Pending),
}

/// The scheduling administration's sections, each while it (or a detail under
/// it) shows, and the answers awaited.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchedulingAdmin {
    /// The register.
    pub overview: Option<OverviewSection>,
    /// The bookings list.
    pub bookings: Option<BookingsSection>,
    /// One booking.
    pub booking: Option<BookingDetailSection>,
    /// The event types list.
    pub event_types: Option<EventTypesSection>,
    /// One event type.
    pub event_type: Option<EventTypeSection>,
    /// The working hours.
    pub hours: Option<HoursSection>,
    /// The calendars.
    pub calendar: Option<CalendarSection>,
    /// The team.
    pub team: Option<TeamSection>,
    /// The settings.
    pub settings: Option<SettingsSection>,
    /// The developer settings.
    pub developer: Option<DeveloperSection>,
    pending: Vec<(Ticket, Pending)>,
}

/// What a section asks for: a request whose answer it awaits, the scheduler
/// sign-in a calendar connection starts with, or an effect that reports
/// nothing back.
pub(crate) enum Out {
    Send(Pending, SchedulingAdminRequest),
    SignIn(Pending, String),
    Effect(Effect),
    /// Show another screen: a deleted event type's list.
    Show(Route),
}

impl SchedulingAdmin {
    /// The effects for `outs`, with a ticket for each answer awaited.
    fn send(&mut self, outs: Vec<Out>, workspace_id: &str, tickets: &mut Tickets) -> Vec<Effect> {
        let workspace_id = workspace_id.to_owned();
        outs.into_iter()
            .filter_map(|out| match out {
                Out::Effect(effect) => Some(effect),
                // Taken out by `SignedIn::scheduling_admin_send`.
                Out::Show(_) => None,
                Out::Send(pending, request) => {
                    let ticket = tickets.issue_keyed(Slot::SchedulingAdmin, "");
                    self.pending.push((ticket, pending));
                    Some(Effect::SchedulingAdmin {
                        ticket,
                        workspace_id: workspace_id.clone(),
                        request,
                    })
                }
                Out::SignIn(pending, next) => {
                    let ticket = tickets.issue_keyed(Slot::SchedulingAdmin, "");
                    self.pending.push((ticket, pending));
                    Some(Effect::SchedulingSignIn {
                        ticket,
                        workspace_id: workspace_id.clone(),
                        next,
                    })
                }
            })
            .collect()
    }

    /// Stops waiting for every answer `drop` names: a read superseded by a new
    /// one, or a section closed.
    fn forget(&mut self, tickets: &mut Tickets, drop: impl Fn(&Pending) -> bool) {
        self.pending.retain(|(ticket, pending)| {
            let keep = !drop(pending);
            if !keep {
                tickets.forget(Slot::SchedulingAdmin, *ticket);
            }
            keep
        });
    }

    /// What `ticket`'s answer was for, if it is still awaited.
    fn take(&mut self, ticket: Ticket) -> Option<Pending> {
        let index = self.pending.iter().position(|(t, _)| *t == ticket)?;
        Some(self.pending.remove(index).1)
    }

    /// Whether `route`'s section was never read here, so going back to it reads
    /// it.
    pub(crate) fn never_read(&self, section: &SchedulingSection) -> bool {
        match section {
            SchedulingSection::Bookings => self.bookings.is_none(),
            SchedulingSection::EventTypes => self.event_types.is_none(),
            _ => false,
        }
    }
}

impl SignedIn {
    /// Opens `section`: a fresh state and its reads.
    pub(crate) fn enter_scheduling_admin(
        &mut self,
        section: SchedulingSection,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let admin = &mut self.scheduling_admin;
        let scoped = matches!(
            section,
            SchedulingSection::Overview
                | SchedulingSection::Bookings
                | SchedulingSection::EventTypes
        );
        let outs = match section {
            SchedulingSection::Overview => overview::enter(admin, tickets),
            SchedulingSection::Bookings => bookings::enter(admin, tickets),
            SchedulingSection::Booking { id } => bookings::enter_detail(admin, tickets, id),
            SchedulingSection::EventTypes => event_types::enter(admin, tickets),
            SchedulingSection::EventType { slug } => {
                event_types::enter_detail(admin, tickets, slug)
            }
            SchedulingSection::Hours => hours::enter(admin, tickets),
            SchedulingSection::Calendar => calendar::enter(admin, tickets),
            SchedulingSection::Team => team::enter(admin, tickets),
            SchedulingSection::Settings => settings::enter(admin, tickets),
            SchedulingSection::Developer => developer::enter(admin, tickets),
        };
        let mut effects = self.scheduling_admin_send(outs, tickets);
        // The overview and the bookings scope their reads by whether this
        // member may manage the booking pages, which the status says, read
        // afresh: the section has to fill itself without the screen above it.
        if scoped {
            effects.extend(self.enter_scheduling(tickets));
        }
        effects
    }

    fn scheduling_admin_send(&mut self, outs: Vec<Out>, tickets: &mut Tickets) -> Vec<Effect> {
        let workspace_id = self.workspace_id();
        let show = outs.iter().find_map(|out| match out {
            Out::Show(route) => Some(route.clone()),
            _ => None,
        });
        let mut effects = self.scheduling_admin.send(outs, &workspace_id, tickets);
        if let Some(route) = show {
            effects.extend(self.show(route, tickets));
        }
        effects
    }

    /// Closes every section `next` does not show (a section stays while a
    /// detail under it shows), and forgets their answers.
    pub(crate) fn close_scheduling_admin(&mut self, next: &Route, tickets: &mut Tickets) {
        let shown = match next {
            Route::SchedulingAdmin(section) => Some(section),
            _ => None,
        };
        let shows = |section: SchedulingSection| shown == Some(&section);
        let shows_bookings = shows(SchedulingSection::Bookings)
            || matches!(shown, Some(SchedulingSection::Booking { .. }));
        let shows_event_types = shows(SchedulingSection::EventTypes)
            || matches!(shown, Some(SchedulingSection::EventType { .. }));
        let admin = &mut self.scheduling_admin;
        let keep_booking = admin
            .booking
            .as_ref()
            .is_some_and(|b| shows(SchedulingSection::Booking { id: b.id.clone() }));
        let keep_event_type = admin.event_type.as_ref().is_some_and(|e| {
            shows(SchedulingSection::EventType {
                slug: e.slug.clone(),
            })
        });
        let keeps = |pending: &Pending| match pending {
            Pending::Overview(_) => shows(SchedulingSection::Overview),
            Pending::Bookings(_) => shows_bookings,
            Pending::Booking(_) => keep_booking,
            Pending::EventTypes(_) => shows_event_types,
            Pending::EventType(_) => keep_event_type,
            Pending::Hours(_) => shows(SchedulingSection::Hours),
            Pending::Calendar(_) => shows(SchedulingSection::Calendar),
            Pending::Team(_) => shows(SchedulingSection::Team),
            Pending::Settings(_) => shows(SchedulingSection::Settings),
            Pending::Developer(_) => shows(SchedulingSection::Developer),
        };
        admin.forget(tickets, |pending| !keeps(pending));
        keep_if(&mut admin.overview, shows(SchedulingSection::Overview));
        keep_if(&mut admin.bookings, shows_bookings);
        keep_if(&mut admin.booking, keep_booking);
        keep_if(&mut admin.event_types, shows_event_types);
        keep_if(&mut admin.event_type, keep_event_type);
        keep_if(&mut admin.hours, shows(SchedulingSection::Hours));
        keep_if(&mut admin.calendar, shows(SchedulingSection::Calendar));
        keep_if(&mut admin.team, shows(SchedulingSection::Team));
        keep_if(&mut admin.settings, shows(SchedulingSection::Settings));
        keep_if(&mut admin.developer, shows(SchedulingSection::Developer));
    }

    pub(crate) fn scheduling_admin_event(
        &mut self,
        event: SchedulingAdminEvent,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Next {
        let can_change = self.capabilities().can_change;
        let admin = &mut self.scheduling_admin;
        let outs = match event {
            SchedulingAdminEvent::Bookings(event) => {
                bookings::event(admin, tickets, event, can_change)
            }
            SchedulingAdminEvent::Booking(event) => {
                bookings::detail_event(admin, tickets, event, can_change)
            }
            SchedulingAdminEvent::EventTypes(event) => {
                event_types::event(admin, tickets, event, can_change)
            }
            SchedulingAdminEvent::EventType(event) => {
                event_types::detail_event(admin, tickets, event, can_change)
            }
            SchedulingAdminEvent::Hours(event) => hours::event(admin, tickets, event, can_change),
            SchedulingAdminEvent::Calendar(event) => calendar::event(admin, tickets, event, config),
            SchedulingAdminEvent::Team(event) => team::event(admin, tickets, event, can_change),
            SchedulingAdminEvent::Settings(event) => {
                settings::event(admin, tickets, event, can_change)
            }
            SchedulingAdminEvent::Developer(event) => {
                developer::event(admin, tickets, event, can_change)
            }
        };
        Next::Stay(self.scheduling_admin_send(outs, tickets))
    }

    pub(crate) fn scheduling_admin_answered(
        &mut self,
        ticket: Ticket,
        result: Result<SchedulingAdminReply, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        let admin = &mut self.scheduling_admin;
        let Some(pending) = tickets
            .accept_keyed(Slot::SchedulingAdmin, ticket)
            .and_then(|_| admin.take(ticket))
        else {
            return stay();
        };
        let answer = answer(result);
        let outs = match pending {
            Pending::Overview(pending) => overview::answered(admin, pending, answer),
            Pending::Bookings(pending) => bookings::answered(admin, tickets, pending, answer),
            Pending::Booking(pending) => bookings::detail_answered(admin, pending, answer),
            Pending::EventTypes(pending) => event_types::answered(admin, tickets, pending, answer),
            Pending::EventType(pending) => {
                event_types::detail_answered(admin, tickets, pending, answer)
            }
            Pending::Hours(pending) => hours::answered(admin, tickets, pending, answer),
            Pending::Calendar(pending) => calendar::answered(admin, tickets, pending, answer),
            Pending::Team(pending) => team::answered(admin, tickets, pending, answer),
            Pending::Settings(pending) => settings::answered(admin, tickets, pending, answer),
            Pending::Developer(pending) => developer::answered(admin, tickets, pending, answer),
        };
        Next::Stay(self.scheduling_admin_send(outs, tickets))
    }

    pub(crate) fn scheduling_sign_in_ready(
        &mut self,
        ticket: Ticket,
        result: Result<SchedulingSecret, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        let admin = &mut self.scheduling_admin;
        let Some(pending) = tickets
            .accept_keyed(Slot::SchedulingAdmin, ticket)
            .and_then(|_| admin.take(ticket))
        else {
            return stay();
        };
        let outs = calendar::signed_in(admin, pending, result);
        Next::Stay(self.scheduling_admin_send(outs, tickets))
    }

    /// The status the booking pages screen read, or failed to: the overview and
    /// the bookings wait for it, and scope their reads by it.
    pub(crate) fn scheduling_status_arrived(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let status = match &self.scheduling.status {
            SchedulingStatus::Ready { status, .. } => Some(status.clone()),
            _ => None,
        };
        let admin = &mut self.scheduling_admin;
        let mut outs = overview::status_arrived(admin, status.as_ref());
        outs.extend(bookings::status_arrived(admin, status.as_ref()));
        event_types::status_arrived(admin, status.as_ref());
        self.scheduling_admin_send(outs, tickets)
    }
}

fn keep_if<T>(section: &mut Option<T>, keep: bool) {
    if !keep {
        *section = None;
    }
}
