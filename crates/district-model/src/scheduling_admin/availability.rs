//! Availability: weekly rules, and dated overrides of them.
//!
//! Two shapes, not layers of one: a rule is a weekly window that repeats; an
//! override is an exception on one date (or a run of dates). Whether a member
//! is free on a day is both, and the times an event type offers
//! (`eventTypes.slots`) are what composes them.

use serde::{Deserialize, Serialize};

/// The three reasons an override may give.
pub const SCHEDULING_OVERRIDE_REASONS: [&str; 3] = ["day_off", "out_of_office", "custom_hours"];

/// One weekly window.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingAvailabilityRule {
    /// Its id.
    pub id: String,
    /// The event type it is for, or `None` for every event type. Always sent.
    pub event_type_id: Option<String>,
    /// The day, 0 to 6, Sunday first.
    pub day_of_week: i64,
    /// When it opens, `HH:MM`.
    pub start_time: String,
    /// When it closes, `HH:MM`.
    pub end_time: String,
}

/// One dated exception.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingAvailabilityOverride {
    /// Its id.
    pub id: String,
    /// The day, `YYYY-MM-DD`.
    pub date: String,
    /// Whether the day has hours (`custom_hours`) or none.
    pub is_available: bool,
    /// One of [`SCHEDULING_OVERRIDE_REASONS`], or a reason added later.
    pub reason: String,
    /// When the day's hours open, for custom hours. Always sent.
    pub start_time: Option<String>,
    /// When they close. Always sent.
    pub end_time: Option<String>,
    /// The run of days it was made with, if it was made as one. Deleting one
    /// day leaves the rest of the run in place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
}

/// What making a run of days answers: a summary, not the rows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingOverrideGroup {
    /// The run's id.
    pub group_id: String,
    /// Its reason.
    pub reason: String,
    /// Its first day.
    pub start: String,
    /// Its last day.
    pub end: String,
    /// How many days it covers.
    pub days: i64,
}

/// What `availability.overrides.create` answers: one day's row, or, when an end
/// date was sent, the run's summary. Read by
/// [`SchedulingAdminRequest::decode`](crate::SchedulingAdminRequest::decode),
/// which tries the row first, as each type decodes strictly in the tests.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum SchedulingOverrideCreated {
    /// One day.
    Day(SchedulingAvailabilityOverride),
    /// A run of days.
    Range(SchedulingOverrideGroup),
}
