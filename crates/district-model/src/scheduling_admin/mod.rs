//! The booking scheduler's administration: event types, availability,
//! bookings, calendars, the team, the developer settings and the pages'
//! branding, all through `POST /api/district/scheduling/admin` and its upload
//! route.
//!
//! These rows pass through the service from a scheduler that is not District's,
//! so they are snake_case on the wire where every other answer is camelCase.
//! Every answer comes in an envelope: `{ok: true, data}` when the scheduler did
//! the work, and `{ok: false, failure, status}`, still with a 200, when it
//! refused, failed or did not answer ([`SchedulingAdminFailure`]). The route's
//! own refusals (no booking pages, parameters the schema refused, a role the
//! operation needs) are ordinary error statuses.

mod availability;
mod bookings;
mod calendar;
mod developer;
mod envelope;

pub(crate) use envelope::nullish;
mod event_types;
mod op;
mod request;
mod settings;
mod team;

pub use availability::{
    SCHEDULING_OVERRIDE_REASONS, SchedulingAvailabilityOverride, SchedulingAvailabilityRule,
    SchedulingOverrideCreated, SchedulingOverrideGroup,
};
pub use bookings::{
    SchedulingBooking, SchedulingBookingAnswer, SchedulingBookingAttendee, SchedulingBookingCounts,
    SchedulingBookingPage,
};
pub use calendar::{
    SchedulingCaldavConnection, SchedulingCalendarConnection, SchedulingCalendarSelection,
    SchedulingCalendarSelections, SchedulingCalendarStatus, SchedulingZoomStatus,
};
pub use developer::{
    SCHEDULING_WEBHOOK_EVENTS, SchedulingApiKey, SchedulingApiKeyCreated,
    SchedulingOauthConnection, SchedulingWebhook, SchedulingWebhookCreated,
    SchedulingWebhookDelivery,
};
pub use envelope::{
    CODE_INVALID_PARAMS, CODE_SCHEDULING_NOT_READY, SchedulingAdminErrorBody,
    SchedulingAdminFailure, SchedulingAdminSuccess, SchedulingItems, SchedulingOk,
    SchedulingSecret,
};
pub use event_types::{
    SCHEDULING_LOCATION_TYPES, SchedulingEventType, SchedulingHost, SchedulingLocationValueKind,
    SchedulingQuestion, SchedulingSlot, SchedulingSlots, SchedulingTestEmail, location_value_kind,
};
pub use op::{SchedulingAdminOp, SchedulingAdminRole};
pub use request::{
    SchedulingAdminData, SchedulingAdminReply, SchedulingAdminRequest, SchedulingBookingQuery,
    SchedulingBrandingPatch, SchedulingEventTypeFields, SchedulingHostAssignment,
    SchedulingOverrideDraft, SchedulingProfilePatch, SchedulingQuestionFields, SchedulingUpload,
    SchedulingUploadTarget,
};
pub use settings::{
    SchedulingBranding, SchedulingLlmSettings, SchedulingLocaleOption, SchedulingMe,
    SchedulingUploadResult,
};
pub use team::{
    SchedulingTeam, SchedulingTeamMember, SchedulingUpcomingBooking, SchedulingUser,
    SchedulingUserArchived, SchedulingUserTeam,
};
