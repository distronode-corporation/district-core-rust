//! Bookings: the rows `bookings.*` answers.
//!
//! Only four fields of a booking are guaranteed (`id`, `start_at`, `end_at`
//! and `status`); the event type and the host may both be missing, as on a
//! row read from a sparse index, and a screen has to draw it anyway. The
//! scheduler's payment fields never cross the route.

use serde::{Deserialize, Serialize};

/// One person on a booking. A booking page that asked for neither a name nor an
/// address leaves both out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingBookingAttendee {
    /// Their name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Their email address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

/// A booking, as every `bookings.*` operation that answers one reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingBooking {
    /// The booking's id, which `bookings.answers` and the writes address it by.
    pub id: String,
    /// The event type's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type_id: Option<String>,
    /// The event type's slug, which the slots for a reschedule are asked for by.
    /// Without it a booking cannot be offered a new time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type_slug: Option<String>,
    /// The host's scheduler user id (not a member id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    /// The host's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_name: Option<String>,
    /// When it starts, RFC 3339.
    pub start_at: String,
    /// When it ends, RFC 3339. Whether it can still be acted on is judged on
    /// this, not on the start: a meeting under way can still be cancelled.
    pub end_at: String,
    /// `confirmed`, `cancelled` or `rescheduled` today, or a status added later,
    /// which is shown as it came.
    pub status: String,
    /// Why it was cancelled. Left out on a booking that was not, and on one
    /// cancelled without a reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancellation_reason: Option<String>,
    /// The phone number, address or meeting link it happens at: one field for
    /// every kind of location, so not safe to treat as a link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location_value: Option<String>,
    /// When it was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When it last changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Who booked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attendees: Option<Vec<SchedulingBookingAttendee>>,
}

/// The totals `bookings.list` reports beside its page: the whole collection's,
/// not the page's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingBookingCounts {
    /// Bookings still to come.
    pub upcoming: i64,
    /// Bookings in the past.
    pub past: i64,
}

/// One page of `bookings.list`. The scheduler clamps the page size without
/// saying so, so a pager moves by the rows that came back, never by what it
/// asked for; every number here may be missing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingBookingPage {
    /// The page's bookings.
    pub items: Vec<SchedulingBooking>,
    /// How many bookings match, in all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    /// The upcoming and past totals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counts: Option<SchedulingBookingCounts>,
    /// The page size the scheduler used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// Where the page starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
}

/// One answer a booker gave to one of the event type's questions. An optional
/// question left unanswered is `""`, never a missing row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingBookingAnswer {
    /// The question's id.
    pub question_id: String,
    /// The question, as the booker read it.
    pub label: String,
    /// The question's kind (`text`, `select`, `checkbox`), which decides how to
    /// label the answer, not how to read it: the answer is always text.
    #[serde(rename = "type")]
    pub kind: String,
    /// What they answered.
    pub value: String,
}
