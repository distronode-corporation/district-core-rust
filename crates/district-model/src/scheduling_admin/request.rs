//! What the scheduling administration route is asked, and what it answers.
//!
//! [`SchedulingAdminRequest`] is one operation with its parameters, typed. Its
//! [`params`](SchedulingAdminRequest::params) are sent as built, the keys the
//! service puts in the scheduler's path included: the catalog's schema checks
//! the whole object before the service takes those keys out, so a request
//! without one is refused. A parameter left as `None` is left out, never sent
//! as `null`, which the schema refuses. Nothing here checks a value: the
//! catalog's schema is the one validator, and a second one here would refuse
//! what the service accepts or accept what it refuses.
//!
//! [`SchedulingAdminRequest::decode`] reads the answer's `data` as the type the
//! operation answers, which is the one thing about an operation this side has
//! to know and the wire does not say.

use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::availability::{
    SchedulingAvailabilityOverride, SchedulingAvailabilityRule, SchedulingOverrideCreated,
    SchedulingOverrideGroup,
};
use super::bookings::{SchedulingBooking, SchedulingBookingAnswer, SchedulingBookingPage};
use super::calendar::{
    SchedulingCaldavConnection, SchedulingCalendarSelection, SchedulingCalendarSelections,
    SchedulingCalendarStatus, SchedulingZoomStatus,
};
use super::developer::{
    SchedulingApiKey, SchedulingApiKeyCreated, SchedulingOauthConnection, SchedulingWebhook,
    SchedulingWebhookCreated, SchedulingWebhookDelivery,
};
use super::envelope::{SchedulingAdminFailure, SchedulingItems, SchedulingOk, SchedulingSecret};
use super::event_types::{
    SchedulingEventType, SchedulingHost, SchedulingQuestion, SchedulingSlots, SchedulingTestEmail,
};
use super::op::SchedulingAdminOp;
use super::settings::{
    SchedulingBranding, SchedulingLlmSettings, SchedulingMe, SchedulingUploadResult,
};
use super::team::{
    SchedulingTeam, SchedulingUpcomingBooking, SchedulingUser, SchedulingUserArchived,
};

/// The fields `me.patch` may change. Each left as `None` is left alone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SchedulingProfilePatch {
    /// Their name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Their IANA zone. The wire key is `timezone`, not the column's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// `12h` or `24h`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_format: Option<String>,
    /// 0 to 6, Sunday first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub week_start: Option<i64>,
    /// `dmy`, `mdy` or `ymd`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_format: Option<String>,
    /// Email the booker a confirmation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_confirmation: Option<bool>,
    /// Email the booker a cancellation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_cancellation: Option<bool>,
    /// Email the booker a reschedule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_reschedule: Option<bool>,
    /// Email the booker a reminder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_reminder: Option<bool>,
    /// Email the member each new booking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_host_booking: Option<bool>,
    /// Email the member each cancellation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_host_cancel: Option<bool>,
    /// Email the member each reschedule.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify_host_reschedule: Option<bool>,
}

/// The fields of an event type `eventTypes.create` and `eventTypes.patch` send.
/// For a create, `slug`, `name` and `duration_minutes` are required by the
/// schema; a patch sends only what changed. The price fields the scheduler has
/// are not here: they are not settable from District.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SchedulingEventTypeFields {
    /// Its name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// What the booking page says about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How long a meeting lasts, in minutes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_minutes: Option<i64>,
    /// How far apart the start times are.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_interval_minutes: Option<i64>,
    /// Where it happens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_type: Option<String>,
    /// The phone number, address or link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_value: Option<String>,
    /// `fixed`, `round_robin` or `collective`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_mode: Option<String>,
    /// `even`, `soonest` or `priority`. A patch only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rr_strategy: Option<String>,
    /// Kept free before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_before_minutes: Option<i64>,
    /// Kept free after.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_after_minutes: Option<i64>,
    /// Minimum notice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_notice_minutes: Option<i64>,
    /// How far ahead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_future_days: Option<i64>,
    /// Bookings one person may hold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_active_bookings: Option<i64>,
    /// Whether it can be booked. A patch only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    /// Whether it is listed. A patch only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_public: Option<bool>,
    /// Whether the taken times show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_taken_slots: Option<bool>,
    /// Archived. A patch only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    /// The confirmation email's text. A patch only, as are the other seven.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_confirmation: Option<String>,
    /// The cancellation email's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_cancellation: Option<String>,
    /// The reschedule email's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_reschedule: Option<String>,
    /// The reminder email's text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_reminder: Option<String>,
    /// The greeting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub msg_greeting: Option<String>,
    /// The confirmation email's subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subj_confirmation: Option<String>,
    /// The cancellation email's subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subj_cancellation: Option<String>,
    /// The reschedule email's subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subj_reschedule: Option<String>,
    /// The reminder email's subject.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subj_reminder: Option<String>,
    /// When reminders go out, minutes before; five at most.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminders: Option<Vec<i64>>,
}

/// One host of an event type, as `eventTypes.hosts.put` sets them: the whole
/// list at once, a replacement and not a change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SchedulingHostAssignment {
    /// The host's scheduler user id.
    pub user_id: String,
    /// `required`, `rotation` or `optional`.
    pub role: String,
    /// Their place in the rotation, from 0.
    pub priority: i64,
}

/// The fields of a booking question.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SchedulingQuestionFields {
    /// The question.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// `text`, `select` or `checkbox`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// A `select`'s choices.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    /// Whether it must be answered. Required by a create.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Its place, from 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
}

/// A dated exception to make: one day, or with `end_date` a run of days.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SchedulingOverrideDraft {
    /// The (first) day, `YYYY-MM-DD`.
    pub date: String,
    /// The last day of a run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    /// `day_off`, `out_of_office` or `custom_hours`.
    pub reason: String,
    /// For custom hours, when they open, `HH:MM`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,
    /// When they close.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
}

/// One page of `bookings.list`, as asked for. A filter left as `None` is left
/// out: an empty one is not "any" to the scheduler but a value matching nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SchedulingBookingQuery {
    /// `confirmed`, `cancelled` or `rescheduled`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// `upcoming` or `past`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// From this day, `YYYY-MM-DD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// To this day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// An event type's slug.
    #[serde(rename = "event_type", skip_serializing_if = "Option::is_none")]
    pub event_type_slug: Option<String>,
    /// A host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// A team.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<String>,
    /// The page size, 200 at most.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    /// Where the page starts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    /// The whole tenancy's bookings rather than the member's own: sends
    /// `scope: "all"`, which the scheduler honours only for its administrator.
    #[serde(
        rename = "scope",
        skip_serializing_if = "is_false",
        serialize_with = "all"
    )]
    pub all_hosts: bool,
    /// `asc` or `desc`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !value
}

fn all<S: serde::Serializer>(_: &bool, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str("all")
}

/// The booking pages' branding, all of it: the scheduler replaces every field,
/// so one left out would be cleared. Sent back as read, with the edits.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SchedulingBrandingPatch {
    /// The name the pages show.
    pub business_name: String,
    /// The logo's height in pixels.
    pub logo_height: i64,
    /// The logo's opacity.
    pub logo_opacity: i64,
    /// The banner's opacity.
    pub banner_opacity: i64,
    /// The privacy policy's link: an http(s) address, or empty.
    pub privacy_url: String,
    /// The terms' link, the same.
    pub terms_url: String,
    /// The fallback language.
    pub fallback_locale: String,
}

/// Which image an upload replaces. They do not share a permission: the logo and
/// the banner are the workspace's and need a member who may change things; the
/// picture is the member's own and a viewer may set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SchedulingUploadTarget {
    /// The booking pages' logo.
    Logo,
    /// Their banner.
    Banner,
    /// The member's own picture.
    Avatar,
}

impl SchedulingUploadTarget {
    /// The form field's value.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Logo => "logo",
            Self::Banner => "banner",
            Self::Avatar => "avatar",
        }
    }
}

/// An image to upload: JPEG, PNG, GIF or WebP (never SVG), 5 MiB at most.
#[derive(Clone, PartialEq, Eq)]
pub struct SchedulingUpload {
    /// Which image it replaces.
    pub target: SchedulingUploadTarget,
    /// The file's name.
    pub file_name: String,
    /// Its MIME type.
    pub mime_type: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

impl fmt::Debug for SchedulingUpload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedulingUpload")
            .field("target", &self.target)
            .field("file_name", &self.file_name)
            .field("mime_type", &self.mime_type)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// One request of the booking scheduler's administration: an operation of
/// `POST /api/district/scheduling/admin` with its parameters, or an image for
/// `POST /api/district/scheduling/admin/upload`.
///
/// Every id here is the scheduler's. Event types are addressed by slug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedulingAdminRequest {
    /// `me.get`
    Me,
    /// `me.patch`
    PatchMe(SchedulingProfilePatch),
    /// `me.avatar.delete`
    DeleteAvatar,
    /// `eventTypes.list`
    EventTypes,
    /// `eventTypes.get`
    EventType {
        /// The event type's slug.
        slug: String,
    },
    /// `eventTypes.create`
    CreateEventType {
        /// Its slug.
        slug: String,
        /// Its fields; `name` and `duration_minutes` are required.
        fields: SchedulingEventTypeFields,
    },
    /// `eventTypes.patch`
    PatchEventType {
        /// The event type's slug.
        slug: String,
        /// What changed.
        fields: SchedulingEventTypeFields,
    },
    /// `eventTypes.delete`
    DeleteEventType {
        /// The event type's slug.
        slug: String,
    },
    /// `eventTypes.hosts.get`
    Hosts {
        /// The event type's slug.
        slug: String,
    },
    /// `eventTypes.hosts.put`
    SetHosts {
        /// The event type's slug.
        slug: String,
        /// Every host: the list replaces the one there. One at least.
        hosts: Vec<SchedulingHostAssignment>,
    },
    /// `eventTypes.testEmail`
    SendTestEmail {
        /// The event type's slug.
        slug: String,
        /// `confirmation`, `cancellation`, `reschedule` or `reminder`.
        kind: String,
    },
    /// `eventTypes.questions.list`
    Questions {
        /// The event type's slug.
        slug: String,
    },
    /// `eventTypes.questions.create`
    CreateQuestion {
        /// The event type's slug.
        slug: String,
        /// The question; `label`, `kind` and `required` are required.
        fields: SchedulingQuestionFields,
    },
    /// `eventTypes.questions.patch`
    PatchQuestion {
        /// The event type's slug.
        slug: String,
        /// The question's id.
        id: String,
        /// What changed.
        fields: SchedulingQuestionFields,
    },
    /// `eventTypes.questions.delete`
    DeleteQuestion {
        /// The event type's slug.
        slug: String,
        /// The question's id.
        id: String,
    },
    /// `eventTypes.slots`
    Slots {
        /// The event type's slug.
        slug: String,
        /// The first day, `YYYY-MM-DD`.
        from: Option<String>,
        /// The last day.
        to: Option<String>,
        /// The IANA zone the times are computed in. Left out, the scheduler
        /// decides, so always sent from a screen.
        tz: Option<String>,
    },
    /// `availability.rules.list`
    Rules {
        /// Only this event type's own rules (the ones for every event type are
        /// not among them).
        event_type_id: Option<String>,
    },
    /// `availability.rules.create`
    CreateRule {
        /// The event type it is for, or `None` for every event type.
        event_type_id: Option<String>,
        /// The day, 0 to 6, Sunday first.
        day_of_week: i64,
        /// `HH:MM`, zero-padded.
        start_time: String,
        /// `HH:MM`, zero-padded.
        end_time: String,
    },
    /// `availability.rules.patch`
    PatchRule {
        /// The rule's id.
        id: String,
        /// The day.
        day_of_week: Option<i64>,
        /// When it opens.
        start_time: Option<String>,
        /// When it closes.
        end_time: Option<String>,
    },
    /// `availability.rules.delete`
    DeleteRule {
        /// The rule's id.
        id: String,
    },
    /// `availability.overrides.list`
    Overrides,
    /// `availability.overrides.create`
    CreateOverride(SchedulingOverrideDraft),
    /// `availability.overrides.patch`
    PatchOverride {
        /// The override's id.
        id: String,
        /// Its reason.
        reason: Option<String>,
        /// When its hours open.
        start_time: Option<String>,
        /// When they close.
        end_time: Option<String>,
    },
    /// `availability.overrides.delete`: one day, even of a run.
    DeleteOverride {
        /// The override's id.
        id: String,
    },
    /// `availability.overrides.deleteGroup`: every day of a run.
    DeleteOverrideGroup {
        /// The run's id (sent as `groupId`, the one camelCase key there is).
        group_id: String,
    },
    /// `bookings.list`
    Bookings(SchedulingBookingQuery),
    /// `bookings.answers`
    BookingAnswers {
        /// The booking's id.
        id: String,
    },
    /// `bookings.cancel`
    CancelBooking {
        /// The booking's id.
        id: String,
        /// Why, for the attendee's email; left out when blank.
        reason: Option<String>,
    },
    /// `bookings.reschedule`. No end: the scheduler works it out from the
    /// event type.
    RescheduleBooking {
        /// The booking's id.
        id: String,
        /// The new start, a slot's `start` as it came.
        start_at: String,
    },
    /// `bookings.reassign`
    ReassignBooking {
        /// The booking's id.
        id: String,
        /// The new host's scheduler user id.
        host_id: String,
    },
    /// `calendar.status`
    CalendarStatus,
    /// `calendar.caldav.connect`
    ConnectCaldav {
        /// A server preset's name.
        preset: Option<String>,
        /// The server, an https address with a host name.
        server_url: Option<String>,
        /// The account's user name.
        username: String,
        /// Its app password (the wire key is `app_password`).
        app_password: SchedulingSecret,
    },
    /// `calendar.connections.calendars.get`
    Calendars {
        /// The connection's id.
        id: String,
        /// Its provider: what names the account.
        provider: String,
        /// Its address (`account` in this query).
        account: Option<String>,
    },
    /// `calendar.connections.calendars.put`
    SetCalendars {
        /// The connection's id.
        id: String,
        /// Its provider.
        provider: String,
        /// Its address (`account_email` in this body).
        account_email: Option<String>,
        /// Every calendar, as read, with the edits.
        calendars: Vec<SchedulingCalendarSelection>,
    },
    /// `calendar.connections.destination`
    SetDestination {
        /// The connection's id.
        id: String,
        /// Its provider.
        provider: String,
        /// Its address.
        account_email: Option<String>,
    },
    /// `calendar.connections.delete`
    DeleteConnection {
        /// The connection's id.
        id: String,
        /// Its provider.
        provider: String,
        /// Its address (`account` here).
        account: Option<String>,
    },
    /// `zoom.status`
    ZoomStatus,
    /// `users.list`
    Users {
        /// Whether to include archived users.
        include_archived: Option<bool>,
    },
    /// `users.archive`
    ArchiveUser {
        /// The user's id.
        id: String,
    },
    /// `users.upcomingBookings`
    UpcomingBookings {
        /// The user's id.
        id: String,
    },
    /// `teams.list`
    Teams,
    /// `teams.get`
    Team {
        /// The team's id.
        id: String,
    },
    /// `teams.create`
    CreateTeam {
        /// Its name.
        name: String,
        /// Its slug; the scheduler makes one when left out.
        slug: Option<String>,
    },
    /// `teams.patch`
    PatchTeam {
        /// The team's id.
        id: String,
        /// Its name.
        name: Option<String>,
        /// Its slug.
        slug: Option<String>,
    },
    /// `teams.delete`
    DeleteTeam {
        /// The team's id.
        id: String,
    },
    /// `teams.members.add`
    AddTeamMember {
        /// The team's id.
        id: String,
        /// The user's id.
        user_id: String,
        /// Their place in the rotation.
        routing_priority: Option<i64>,
    },
    /// `teams.members.patch`
    PatchTeamMember {
        /// The team's id.
        id: String,
        /// The user's id (sent as `userId`).
        user_id: String,
        /// Their place in the rotation.
        routing_priority: i64,
    },
    /// `teams.members.remove`
    RemoveTeamMember {
        /// The team's id.
        id: String,
        /// The user's id (sent as `userId`).
        user_id: String,
    },
    /// `settings.branding.get`
    Branding,
    /// `settings.branding.patch`
    PatchBranding(SchedulingBrandingPatch),
    /// `settings.branding.logo.delete`
    DeleteLogo,
    /// `settings.branding.banner.delete`
    DeleteBanner,
    /// `settings.llm.get`
    Llm,
    /// `settings.llm.patch`
    PatchLlm {
        /// Whether meetings are summarised.
        enabled: Option<bool>,
        /// The extra instructions, 4000 characters at most.
        extra_instructions: Option<String>,
    },
    /// `apiKeys.list`
    ApiKeys,
    /// `apiKeys.create`
    CreateApiKey {
        /// Its name.
        name: String,
    },
    /// `apiKeys.delete`
    DeleteApiKey {
        /// The key's id.
        id: String,
    },
    /// `oauth.connections.list`
    OauthConnections,
    /// `oauth.connections.delete`
    DeleteOauthConnection {
        /// The connection's id.
        id: String,
    },
    /// `webhooks.list`
    Webhooks,
    /// `webhooks.create`
    CreateWebhook {
        /// Where it posts: an https address.
        url: String,
        /// What it posts: one at least.
        events: Vec<String>,
        /// The booking fields each post carries.
        fields: Option<Vec<String>>,
    },
    /// `webhooks.patch`
    PatchWebhook {
        /// The webhook's id.
        id: String,
        /// What it posts.
        events: Option<Vec<String>>,
        /// The booking fields each post carries.
        fields: Option<Vec<String>>,
    },
    /// `webhooks.delete`
    DeleteWebhook {
        /// The webhook's id.
        id: String,
    },
    /// `webhooks.deliveries`
    WebhookDeliveries {
        /// The webhook's id.
        id: String,
    },
    /// An image, through the upload route rather than an operation.
    Upload(SchedulingUpload),
}

/// What an operation answered, typed by the operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedulingAdminData {
    /// `me.get`, `me.patch`
    Me(SchedulingMe),
    /// `eventTypes.list`
    EventTypes(Vec<SchedulingEventType>),
    /// `eventTypes.get`, `.create`, `.patch`
    EventType(Box<SchedulingEventType>),
    /// `eventTypes.hosts.get`, `.put`
    Hosts(Vec<SchedulingHost>),
    /// `eventTypes.testEmail`
    TestEmail(SchedulingTestEmail),
    /// `eventTypes.questions.list`
    Questions(Vec<SchedulingQuestion>),
    /// `eventTypes.questions.create`, `.patch`
    Question(SchedulingQuestion),
    /// `eventTypes.slots`
    Slots(SchedulingSlots),
    /// `availability.rules.list`
    Rules(Vec<SchedulingAvailabilityRule>),
    /// `availability.rules.create`, `.patch`
    Rule(SchedulingAvailabilityRule),
    /// `availability.overrides.list`
    Overrides(Vec<SchedulingAvailabilityOverride>),
    /// `availability.overrides.patch`
    Override(SchedulingAvailabilityOverride),
    /// `availability.overrides.create`
    OverrideCreated(SchedulingOverrideCreated),
    /// `bookings.list`
    BookingPage(SchedulingBookingPage),
    /// `bookings.answers`
    Answers(Vec<SchedulingBookingAnswer>),
    /// `bookings.cancel`, `.reschedule`, `.reassign`: the booking as it now is.
    Booking(Box<SchedulingBooking>),
    /// `calendar.status`
    CalendarStatus(SchedulingCalendarStatus),
    /// `calendar.caldav.connect`
    CaldavConnected(SchedulingCaldavConnection),
    /// `calendar.connections.calendars.get`
    Calendars(Vec<SchedulingCalendarSelection>),
    /// `zoom.status`
    ZoomStatus(SchedulingZoomStatus),
    /// `users.list`
    Users(Vec<SchedulingUser>),
    /// `users.archive`
    UserArchived(SchedulingUserArchived),
    /// `users.upcomingBookings`
    UpcomingBookings(Vec<SchedulingUpcomingBooking>),
    /// `teams.list`
    Teams(Vec<SchedulingTeam>),
    /// `teams.get`, `.create`, `.patch`, `.members.add`, `.members.patch`
    Team(SchedulingTeam),
    /// Every operation that answers nothing, and `teams.delete` and
    /// `teams.members.remove`, which answer `{ok}`.
    Done(SchedulingOk),
    /// `settings.branding.get`, `.patch`
    Branding(SchedulingBranding),
    /// `settings.llm.get`, `.patch`
    Llm(SchedulingLlmSettings),
    /// `apiKeys.list`
    ApiKeys(Vec<SchedulingApiKey>),
    /// `apiKeys.create`
    ApiKeyCreated(SchedulingApiKeyCreated),
    /// `oauth.connections.list`
    OauthConnections(Vec<SchedulingOauthConnection>),
    /// `webhooks.list`
    Webhooks(Vec<SchedulingWebhook>),
    /// `webhooks.create`
    WebhookCreated(SchedulingWebhookCreated),
    /// `webhooks.deliveries`
    Deliveries(Vec<SchedulingWebhookDelivery>),
    /// The upload route.
    Uploaded(SchedulingUploadResult),
}

/// The answer to a request that got one: the scheduler's work, or its refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedulingAdminReply {
    /// Done, with what the operation answered.
    Done(SchedulingAdminData),
    /// The scheduler refused, failed or did not answer: a 200 from the route.
    Refused(SchedulingAdminFailure),
}

/// An object of the pairs with a value; a `None` leaves its key out.
fn object(pairs: impl IntoIterator<Item = (&'static str, Option<Value>)>) -> Value {
    Value::Object(
        pairs
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value)))
            .collect::<Map<String, Value>>(),
    )
}

fn text(value: &str) -> Option<Value> {
    Some(Value::String(value.to_owned()))
}

fn opt_text(value: &Option<String>) -> Option<Value> {
    value.as_deref().and_then(text)
}

fn json<T: Serialize>(value: &T) -> Option<Value> {
    // Every type serialized here is plain data with string keys, which serde_json
    // always encodes.
    serde_json::to_value(value).ok()
}

/// `fields`'s object with `extra` added.
fn with<T: Serialize>(
    fields: &T,
    extra: impl IntoIterator<Item = (&'static str, Option<Value>)>,
) -> Value {
    // Every `T` here is a struct of plain fields, which encodes as an object.
    let mut map = json(fields)
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    map.extend(
        extra
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value))),
    );
    Value::Object(map)
}

fn items<T: DeserializeOwned>(data: Value) -> Result<Vec<T>, serde_json::Error> {
    serde_json::from_value::<SchedulingItems<T>>(data).map(|list| list.items)
}

impl SchedulingAdminRequest {
    /// The operation, or `None` for an upload, which is not one.
    pub fn op(&self) -> Option<SchedulingAdminOp> {
        use SchedulingAdminOp as Op;
        Some(match self {
            Self::Me => Op::MeGet,
            Self::PatchMe(_) => Op::MePatch,
            Self::DeleteAvatar => Op::MeAvatarDelete,
            Self::EventTypes => Op::EventTypesList,
            Self::EventType { .. } => Op::EventTypesGet,
            Self::CreateEventType { .. } => Op::EventTypesCreate,
            Self::PatchEventType { .. } => Op::EventTypesPatch,
            Self::DeleteEventType { .. } => Op::EventTypesDelete,
            Self::Hosts { .. } => Op::EventTypesHostsGet,
            Self::SetHosts { .. } => Op::EventTypesHostsPut,
            Self::SendTestEmail { .. } => Op::EventTypesTestEmail,
            Self::Questions { .. } => Op::EventTypesQuestionsList,
            Self::CreateQuestion { .. } => Op::EventTypesQuestionsCreate,
            Self::PatchQuestion { .. } => Op::EventTypesQuestionsPatch,
            Self::DeleteQuestion { .. } => Op::EventTypesQuestionsDelete,
            Self::Slots { .. } => Op::EventTypesSlots,
            Self::Rules { .. } => Op::AvailabilityRulesList,
            Self::CreateRule { .. } => Op::AvailabilityRulesCreate,
            Self::PatchRule { .. } => Op::AvailabilityRulesPatch,
            Self::DeleteRule { .. } => Op::AvailabilityRulesDelete,
            Self::Overrides => Op::AvailabilityOverridesList,
            Self::CreateOverride(_) => Op::AvailabilityOverridesCreate,
            Self::PatchOverride { .. } => Op::AvailabilityOverridesPatch,
            Self::DeleteOverride { .. } => Op::AvailabilityOverridesDelete,
            Self::DeleteOverrideGroup { .. } => Op::AvailabilityOverridesDeleteGroup,
            Self::Bookings(_) => Op::BookingsList,
            Self::BookingAnswers { .. } => Op::BookingsAnswers,
            Self::CancelBooking { .. } => Op::BookingsCancel,
            Self::RescheduleBooking { .. } => Op::BookingsReschedule,
            Self::ReassignBooking { .. } => Op::BookingsReassign,
            Self::CalendarStatus => Op::CalendarStatus,
            Self::ConnectCaldav { .. } => Op::CalendarCaldavConnect,
            Self::Calendars { .. } => Op::CalendarConnectionsCalendarsGet,
            Self::SetCalendars { .. } => Op::CalendarConnectionsCalendarsPut,
            Self::SetDestination { .. } => Op::CalendarConnectionsDestination,
            Self::DeleteConnection { .. } => Op::CalendarConnectionsDelete,
            Self::ZoomStatus => Op::ZoomStatus,
            Self::Users { .. } => Op::UsersList,
            Self::ArchiveUser { .. } => Op::UsersArchive,
            Self::UpcomingBookings { .. } => Op::UsersUpcomingBookings,
            Self::Teams => Op::TeamsList,
            Self::Team { .. } => Op::TeamsGet,
            Self::CreateTeam { .. } => Op::TeamsCreate,
            Self::PatchTeam { .. } => Op::TeamsPatch,
            Self::DeleteTeam { .. } => Op::TeamsDelete,
            Self::AddTeamMember { .. } => Op::TeamsMembersAdd,
            Self::PatchTeamMember { .. } => Op::TeamsMembersPatch,
            Self::RemoveTeamMember { .. } => Op::TeamsMembersRemove,
            Self::Branding => Op::SettingsBrandingGet,
            Self::PatchBranding(_) => Op::SettingsBrandingPatch,
            Self::DeleteLogo => Op::SettingsBrandingLogoDelete,
            Self::DeleteBanner => Op::SettingsBrandingBannerDelete,
            Self::Llm => Op::SettingsLlmGet,
            Self::PatchLlm { .. } => Op::SettingsLlmPatch,
            Self::ApiKeys => Op::ApiKeysList,
            Self::CreateApiKey { .. } => Op::ApiKeysCreate,
            Self::DeleteApiKey { .. } => Op::ApiKeysDelete,
            Self::OauthConnections => Op::OauthConnectionsList,
            Self::DeleteOauthConnection { .. } => Op::OauthConnectionsDelete,
            Self::Webhooks => Op::WebhooksList,
            Self::CreateWebhook { .. } => Op::WebhooksCreate,
            Self::PatchWebhook { .. } => Op::WebhooksPatch,
            Self::DeleteWebhook { .. } => Op::WebhooksDelete,
            Self::WebhookDeliveries { .. } => Op::WebhooksDeliveries,
            Self::Upload(_) => return None,
        })
    }

    /// The operation's `params`, `{}` for one that takes none. Empty for an
    /// upload, which sends a form instead.
    pub fn params(&self) -> Value {
        match self {
            Self::Me
            | Self::DeleteAvatar
            | Self::EventTypes
            | Self::Overrides
            | Self::CalendarStatus
            | Self::ZoomStatus
            | Self::Teams
            | Self::Branding
            | Self::DeleteLogo
            | Self::DeleteBanner
            | Self::Llm
            | Self::ApiKeys
            | Self::OauthConnections
            | Self::Webhooks
            | Self::Upload(_) => object([]),
            Self::PatchMe(patch) => with(patch, []),
            Self::EventType { slug }
            | Self::DeleteEventType { slug }
            | Self::Hosts { slug }
            | Self::Questions { slug } => object([("slug", text(slug))]),
            Self::CreateEventType { slug, fields } | Self::PatchEventType { slug, fields } => {
                with(fields, [("slug", text(slug))])
            }
            Self::SetHosts { slug, hosts } => {
                object([("slug", text(slug)), ("hosts", json(hosts))])
            }
            Self::SendTestEmail { slug, kind } => {
                object([("slug", text(slug)), ("type", text(kind))])
            }
            Self::CreateQuestion { slug, fields } => with(fields, [("slug", text(slug))]),
            Self::PatchQuestion { slug, id, fields } => {
                with(fields, [("slug", text(slug)), ("id", text(id))])
            }
            Self::DeleteQuestion { slug, id } => object([("slug", text(slug)), ("id", text(id))]),
            Self::Slots { slug, from, to, tz } => object([
                ("slug", text(slug)),
                ("from", opt_text(from)),
                ("to", opt_text(to)),
                ("tz", opt_text(tz)),
            ]),
            Self::Rules { event_type_id } => object([("event_type_id", opt_text(event_type_id))]),
            Self::CreateRule {
                event_type_id,
                day_of_week,
                start_time,
                end_time,
            } => object([
                ("event_type_id", opt_text(event_type_id)),
                ("day_of_week", json(day_of_week)),
                ("start_time", text(start_time)),
                ("end_time", text(end_time)),
            ]),
            Self::PatchRule {
                id,
                day_of_week,
                start_time,
                end_time,
            } => object([
                ("id", text(id)),
                ("day_of_week", day_of_week.as_ref().and_then(json)),
                ("start_time", opt_text(start_time)),
                ("end_time", opt_text(end_time)),
            ]),
            Self::DeleteRule { id }
            | Self::DeleteOverride { id }
            | Self::BookingAnswers { id }
            | Self::ArchiveUser { id }
            | Self::UpcomingBookings { id }
            | Self::Team { id }
            | Self::DeleteTeam { id }
            | Self::DeleteApiKey { id }
            | Self::DeleteOauthConnection { id }
            | Self::DeleteWebhook { id }
            | Self::WebhookDeliveries { id } => object([("id", text(id))]),
            Self::CreateOverride(draft) => with(draft, []),
            Self::PatchOverride {
                id,
                reason,
                start_time,
                end_time,
            } => object([
                ("id", text(id)),
                ("reason", opt_text(reason)),
                ("start_time", opt_text(start_time)),
                ("end_time", opt_text(end_time)),
            ]),
            Self::DeleteOverrideGroup { group_id } => object([("groupId", text(group_id))]),
            Self::Bookings(query) => with(query, []),
            Self::CancelBooking { id, reason } => {
                object([("id", text(id)), ("reason", opt_text(reason))])
            }
            Self::RescheduleBooking { id, start_at } => {
                object([("id", text(id)), ("start_at", text(start_at))])
            }
            Self::ReassignBooking { id, host_id } => {
                object([("id", text(id)), ("host_id", text(host_id))])
            }
            Self::ConnectCaldav {
                preset,
                server_url,
                username,
                app_password,
            } => object([
                ("preset", opt_text(preset)),
                ("server_url", opt_text(server_url)),
                ("username", text(username)),
                ("app_password", text(app_password.expose())),
            ]),
            Self::Calendars {
                id,
                provider,
                account,
            }
            | Self::DeleteConnection {
                id,
                provider,
                account,
            } => object([
                ("id", text(id)),
                ("provider", text(provider)),
                ("account", opt_text(account)),
            ]),
            Self::SetCalendars {
                id,
                provider,
                account_email,
                calendars,
            } => object([
                ("id", text(id)),
                ("provider", text(provider)),
                ("account_email", opt_text(account_email)),
                ("calendars", json(calendars)),
            ]),
            Self::SetDestination {
                id,
                provider,
                account_email,
            } => object([
                ("id", text(id)),
                ("provider", text(provider)),
                ("account_email", opt_text(account_email)),
            ]),
            Self::Users { include_archived } => {
                object([("include_archived", include_archived.as_ref().and_then(json))])
            }
            Self::CreateTeam { name, slug } => {
                object([("name", text(name)), ("slug", opt_text(slug))])
            }
            Self::PatchTeam { id, name, slug } => object([
                ("id", text(id)),
                ("name", opt_text(name)),
                ("slug", opt_text(slug)),
            ]),
            Self::AddTeamMember {
                id,
                user_id,
                routing_priority,
            } => object([
                ("id", text(id)),
                ("user_id", text(user_id)),
                ("routing_priority", routing_priority.as_ref().and_then(json)),
            ]),
            Self::PatchTeamMember {
                id,
                user_id,
                routing_priority,
            } => object([
                ("id", text(id)),
                ("userId", text(user_id)),
                ("routing_priority", json(routing_priority)),
            ]),
            Self::RemoveTeamMember { id, user_id } => {
                object([("id", text(id)), ("userId", text(user_id))])
            }
            Self::PatchBranding(patch) => with(patch, []),
            Self::PatchLlm {
                enabled,
                extra_instructions,
            } => object([
                ("enabled", enabled.as_ref().and_then(json)),
                ("extra_instructions", opt_text(extra_instructions)),
            ]),
            Self::CreateApiKey { name } => object([("name", text(name))]),
            Self::CreateWebhook {
                url,
                events,
                fields,
            } => object([
                ("url", text(url)),
                ("events", json(events)),
                ("fields", fields.as_ref().and_then(json)),
            ]),
            Self::PatchWebhook { id, events, fields } => object([
                ("id", text(id)),
                ("events", events.as_ref().and_then(json)),
                ("fields", fields.as_ref().and_then(json)),
            ]),
        }
    }

    /// Reads the answer's `data` as the type this request's operation answers.
    pub fn decode(&self, data: Value) -> Result<SchedulingAdminData, serde_json::Error> {
        use SchedulingAdminData as D;
        Ok(match self {
            Self::Me | Self::PatchMe(_) => D::Me(serde_json::from_value(data)?),
            Self::EventTypes => D::EventTypes(items(data)?),
            Self::EventType { .. } | Self::CreateEventType { .. } | Self::PatchEventType { .. } => {
                D::EventType(Box::new(serde_json::from_value(data)?))
            }
            Self::Hosts { .. } | Self::SetHosts { .. } => D::Hosts(items(data)?),
            Self::SendTestEmail { .. } => D::TestEmail(serde_json::from_value(data)?),
            Self::Questions { .. } => D::Questions(items(data)?),
            Self::CreateQuestion { .. } | Self::PatchQuestion { .. } => {
                D::Question(serde_json::from_value(data)?)
            }
            Self::Slots { .. } => D::Slots(serde_json::from_value(data)?),
            Self::Rules { .. } => D::Rules(items(data)?),
            Self::CreateRule { .. } | Self::PatchRule { .. } => {
                D::Rule(serde_json::from_value(data)?)
            }
            Self::Overrides => D::Overrides(items(data)?),
            Self::PatchOverride { .. } => D::Override(serde_json::from_value(data)?),
            Self::CreateOverride(_) => D::OverrideCreated(
                match serde_json::from_value::<SchedulingAvailabilityOverride>(data.clone()) {
                    Ok(day) => SchedulingOverrideCreated::Day(day),
                    Err(_) => SchedulingOverrideCreated::Range(serde_json::from_value::<
                        SchedulingOverrideGroup,
                    >(data)?),
                },
            ),
            Self::Bookings(_) => D::BookingPage(serde_json::from_value(data)?),
            Self::BookingAnswers { .. } => D::Answers(items(data)?),
            Self::CancelBooking { .. }
            | Self::RescheduleBooking { .. }
            | Self::ReassignBooking { .. } => D::Booking(Box::new(serde_json::from_value(data)?)),
            Self::CalendarStatus => D::CalendarStatus(serde_json::from_value(data)?),
            Self::ConnectCaldav { .. } => D::CaldavConnected(serde_json::from_value(data)?),
            Self::Calendars { .. } => D::Calendars(
                serde_json::from_value::<SchedulingCalendarSelections>(data)?.calendars,
            ),
            Self::ZoomStatus => D::ZoomStatus(serde_json::from_value(data)?),
            Self::Users { .. } => D::Users(serde_json::from_value(data)?),
            Self::ArchiveUser { .. } => D::UserArchived(serde_json::from_value(data)?),
            Self::UpcomingBookings { .. } => D::UpcomingBookings(items(data)?),
            Self::Teams => D::Teams(items(data)?),
            Self::Team { .. }
            | Self::CreateTeam { .. }
            | Self::PatchTeam { .. }
            | Self::AddTeamMember { .. }
            | Self::PatchTeamMember { .. } => D::Team(serde_json::from_value(data)?),
            Self::DeleteAvatar
            | Self::DeleteEventType { .. }
            | Self::DeleteQuestion { .. }
            | Self::DeleteRule { .. }
            | Self::DeleteOverride { .. }
            | Self::DeleteOverrideGroup { .. }
            | Self::SetCalendars { .. }
            | Self::SetDestination { .. }
            | Self::DeleteConnection { .. }
            | Self::DeleteTeam { .. }
            | Self::RemoveTeamMember { .. }
            | Self::DeleteLogo
            | Self::DeleteBanner
            | Self::DeleteApiKey { .. }
            | Self::DeleteOauthConnection { .. }
            | Self::PatchWebhook { .. }
            | Self::DeleteWebhook { .. } => D::Done(serde_json::from_value(data)?),
            Self::Branding | Self::PatchBranding(_) => D::Branding(serde_json::from_value(data)?),
            Self::Llm | Self::PatchLlm { .. } => D::Llm(serde_json::from_value(data)?),
            Self::ApiKeys => D::ApiKeys(items(data)?),
            Self::CreateApiKey { .. } => D::ApiKeyCreated(serde_json::from_value(data)?),
            Self::OauthConnections => D::OauthConnections(items(data)?),
            Self::Webhooks => D::Webhooks(items(data)?),
            Self::CreateWebhook { .. } => D::WebhookCreated(serde_json::from_value(data)?),
            Self::WebhookDeliveries { .. } => D::Deliveries(items(data)?),
            Self::Upload(_) => D::Uploaded(serde_json::from_value(data)?),
        })
    }
}
