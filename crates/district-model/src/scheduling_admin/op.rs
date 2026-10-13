//! The operations the booking scheduler's administration may be asked for, by
//! name.

/// One operation of `POST /api/district/scheduling/admin`, by the name the
/// service's catalog gives it.
///
/// The route takes an operation's name and never a path, so this client can
/// reach exactly the scheduler routes the catalog names and no other. The names
/// are the service's verbatim, dots and all; a name the service renamed or
/// withdrew is a 400 at run time, not a compile error, which is why the tests
/// spell every name out a second time rather than reading them back from here.
///
/// Sixty-four operations: 29 a viewer may send and 35 that need a member who may
/// change things, 23 reads and 41 writes. Meetings are not recorded, so the
/// operations that listed, deleted or summarised recordings are gone, as they are
/// from the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SchedulingAdminOp {
    /// `me.get`
    MeGet,
    /// `me.patch`
    MePatch,
    /// `me.avatar.delete`
    MeAvatarDelete,
    /// `eventTypes.list`
    EventTypesList,
    /// `eventTypes.get`
    EventTypesGet,
    /// `eventTypes.create`
    EventTypesCreate,
    /// `eventTypes.patch`
    EventTypesPatch,
    /// `eventTypes.delete`
    EventTypesDelete,
    /// `eventTypes.hosts.get`
    EventTypesHostsGet,
    /// `eventTypes.hosts.put`
    EventTypesHostsPut,
    /// `eventTypes.testEmail`
    EventTypesTestEmail,
    /// `eventTypes.questions.list`
    EventTypesQuestionsList,
    /// `eventTypes.questions.create`
    EventTypesQuestionsCreate,
    /// `eventTypes.questions.patch`
    EventTypesQuestionsPatch,
    /// `eventTypes.questions.delete`
    EventTypesQuestionsDelete,
    /// `eventTypes.slots`
    EventTypesSlots,
    /// `availability.rules.list`
    AvailabilityRulesList,
    /// `availability.rules.create`
    AvailabilityRulesCreate,
    /// `availability.rules.patch`
    AvailabilityRulesPatch,
    /// `availability.rules.delete`
    AvailabilityRulesDelete,
    /// `availability.overrides.list`
    AvailabilityOverridesList,
    /// `availability.overrides.create`
    AvailabilityOverridesCreate,
    /// `availability.overrides.patch`
    AvailabilityOverridesPatch,
    /// `availability.overrides.delete`
    AvailabilityOverridesDelete,
    /// `availability.overrides.deleteGroup`
    AvailabilityOverridesDeleteGroup,
    /// `bookings.list`
    BookingsList,
    /// `bookings.answers`
    BookingsAnswers,
    /// `bookings.cancel`
    BookingsCancel,
    /// `bookings.reschedule`
    BookingsReschedule,
    /// `bookings.reassign`
    BookingsReassign,
    /// `calendar.status`
    CalendarStatus,
    /// `calendar.caldav.connect`
    CalendarCaldavConnect,
    /// `calendar.connections.calendars.get`
    CalendarConnectionsCalendarsGet,
    /// `calendar.connections.calendars.put`
    CalendarConnectionsCalendarsPut,
    /// `calendar.connections.destination`
    CalendarConnectionsDestination,
    /// `calendar.connections.delete`
    CalendarConnectionsDelete,
    /// `zoom.status`
    ZoomStatus,
    /// `users.list`
    UsersList,
    /// `users.archive`
    UsersArchive,
    /// `users.upcomingBookings`
    UsersUpcomingBookings,
    /// `teams.list`
    TeamsList,
    /// `teams.get`
    TeamsGet,
    /// `teams.create`
    TeamsCreate,
    /// `teams.patch`
    TeamsPatch,
    /// `teams.delete`
    TeamsDelete,
    /// `teams.members.add`
    TeamsMembersAdd,
    /// `teams.members.patch`
    TeamsMembersPatch,
    /// `teams.members.remove`
    TeamsMembersRemove,
    /// `settings.branding.get`
    SettingsBrandingGet,
    /// `settings.branding.patch`
    SettingsBrandingPatch,
    /// `settings.branding.logo.delete`
    SettingsBrandingLogoDelete,
    /// `settings.branding.banner.delete`
    SettingsBrandingBannerDelete,
    /// `settings.llm.get`
    SettingsLlmGet,
    /// `settings.llm.patch`
    SettingsLlmPatch,
    /// `apiKeys.list`
    ApiKeysList,
    /// `apiKeys.create`
    ApiKeysCreate,
    /// `apiKeys.delete`
    ApiKeysDelete,
    /// `oauth.connections.list`
    OauthConnectionsList,
    /// `oauth.connections.delete`
    OauthConnectionsDelete,
    /// `webhooks.list`
    WebhooksList,
    /// `webhooks.create`
    WebhooksCreate,
    /// `webhooks.patch`
    WebhooksPatch,
    /// `webhooks.delete`
    WebhooksDelete,
    /// `webhooks.deliveries`
    WebhooksDeliveries,
}

/// The lowest workspace role the service accepts an operation from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SchedulingAdminRole {
    /// Every member, viewers included.
    Viewer,
    /// A member who may change things: `client` or `agency`.
    Client,
}

impl SchedulingAdminOp {
    /// Every operation, in the catalog's order.
    pub const ALL: [Self; 64] = [
        Self::MeGet,
        Self::MePatch,
        Self::MeAvatarDelete,
        Self::EventTypesList,
        Self::EventTypesGet,
        Self::EventTypesCreate,
        Self::EventTypesPatch,
        Self::EventTypesDelete,
        Self::EventTypesHostsGet,
        Self::EventTypesHostsPut,
        Self::EventTypesTestEmail,
        Self::EventTypesQuestionsList,
        Self::EventTypesQuestionsCreate,
        Self::EventTypesQuestionsPatch,
        Self::EventTypesQuestionsDelete,
        Self::EventTypesSlots,
        Self::AvailabilityRulesList,
        Self::AvailabilityRulesCreate,
        Self::AvailabilityRulesPatch,
        Self::AvailabilityRulesDelete,
        Self::AvailabilityOverridesList,
        Self::AvailabilityOverridesCreate,
        Self::AvailabilityOverridesPatch,
        Self::AvailabilityOverridesDelete,
        Self::AvailabilityOverridesDeleteGroup,
        Self::BookingsList,
        Self::BookingsAnswers,
        Self::BookingsCancel,
        Self::BookingsReschedule,
        Self::BookingsReassign,
        Self::CalendarStatus,
        Self::CalendarCaldavConnect,
        Self::CalendarConnectionsCalendarsGet,
        Self::CalendarConnectionsCalendarsPut,
        Self::CalendarConnectionsDestination,
        Self::CalendarConnectionsDelete,
        Self::ZoomStatus,
        Self::UsersList,
        Self::UsersArchive,
        Self::UsersUpcomingBookings,
        Self::TeamsList,
        Self::TeamsGet,
        Self::TeamsCreate,
        Self::TeamsPatch,
        Self::TeamsDelete,
        Self::TeamsMembersAdd,
        Self::TeamsMembersPatch,
        Self::TeamsMembersRemove,
        Self::SettingsBrandingGet,
        Self::SettingsBrandingPatch,
        Self::SettingsBrandingLogoDelete,
        Self::SettingsBrandingBannerDelete,
        Self::SettingsLlmGet,
        Self::SettingsLlmPatch,
        Self::ApiKeysList,
        Self::ApiKeysCreate,
        Self::ApiKeysDelete,
        Self::OauthConnectionsList,
        Self::OauthConnectionsDelete,
        Self::WebhooksList,
        Self::WebhooksCreate,
        Self::WebhooksPatch,
        Self::WebhooksDelete,
        Self::WebhooksDeliveries,
    ];

    /// The name the service's catalog gives the operation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MeGet => "me.get",
            Self::MePatch => "me.patch",
            Self::MeAvatarDelete => "me.avatar.delete",
            Self::EventTypesList => "eventTypes.list",
            Self::EventTypesGet => "eventTypes.get",
            Self::EventTypesCreate => "eventTypes.create",
            Self::EventTypesPatch => "eventTypes.patch",
            Self::EventTypesDelete => "eventTypes.delete",
            Self::EventTypesHostsGet => "eventTypes.hosts.get",
            Self::EventTypesHostsPut => "eventTypes.hosts.put",
            Self::EventTypesTestEmail => "eventTypes.testEmail",
            Self::EventTypesQuestionsList => "eventTypes.questions.list",
            Self::EventTypesQuestionsCreate => "eventTypes.questions.create",
            Self::EventTypesQuestionsPatch => "eventTypes.questions.patch",
            Self::EventTypesQuestionsDelete => "eventTypes.questions.delete",
            Self::EventTypesSlots => "eventTypes.slots",
            Self::AvailabilityRulesList => "availability.rules.list",
            Self::AvailabilityRulesCreate => "availability.rules.create",
            Self::AvailabilityRulesPatch => "availability.rules.patch",
            Self::AvailabilityRulesDelete => "availability.rules.delete",
            Self::AvailabilityOverridesList => "availability.overrides.list",
            Self::AvailabilityOverridesCreate => "availability.overrides.create",
            Self::AvailabilityOverridesPatch => "availability.overrides.patch",
            Self::AvailabilityOverridesDelete => "availability.overrides.delete",
            Self::AvailabilityOverridesDeleteGroup => "availability.overrides.deleteGroup",
            Self::BookingsList => "bookings.list",
            Self::BookingsAnswers => "bookings.answers",
            Self::BookingsCancel => "bookings.cancel",
            Self::BookingsReschedule => "bookings.reschedule",
            Self::BookingsReassign => "bookings.reassign",
            Self::CalendarStatus => "calendar.status",
            Self::CalendarCaldavConnect => "calendar.caldav.connect",
            Self::CalendarConnectionsCalendarsGet => "calendar.connections.calendars.get",
            Self::CalendarConnectionsCalendarsPut => "calendar.connections.calendars.put",
            Self::CalendarConnectionsDestination => "calendar.connections.destination",
            Self::CalendarConnectionsDelete => "calendar.connections.delete",
            Self::ZoomStatus => "zoom.status",
            Self::UsersList => "users.list",
            Self::UsersArchive => "users.archive",
            Self::UsersUpcomingBookings => "users.upcomingBookings",
            Self::TeamsList => "teams.list",
            Self::TeamsGet => "teams.get",
            Self::TeamsCreate => "teams.create",
            Self::TeamsPatch => "teams.patch",
            Self::TeamsDelete => "teams.delete",
            Self::TeamsMembersAdd => "teams.members.add",
            Self::TeamsMembersPatch => "teams.members.patch",
            Self::TeamsMembersRemove => "teams.members.remove",
            Self::SettingsBrandingGet => "settings.branding.get",
            Self::SettingsBrandingPatch => "settings.branding.patch",
            Self::SettingsBrandingLogoDelete => "settings.branding.logo.delete",
            Self::SettingsBrandingBannerDelete => "settings.branding.banner.delete",
            Self::SettingsLlmGet => "settings.llm.get",
            Self::SettingsLlmPatch => "settings.llm.patch",
            Self::ApiKeysList => "apiKeys.list",
            Self::ApiKeysCreate => "apiKeys.create",
            Self::ApiKeysDelete => "apiKeys.delete",
            Self::OauthConnectionsList => "oauth.connections.list",
            Self::OauthConnectionsDelete => "oauth.connections.delete",
            Self::WebhooksList => "webhooks.list",
            Self::WebhooksCreate => "webhooks.create",
            Self::WebhooksPatch => "webhooks.patch",
            Self::WebhooksDelete => "webhooks.delete",
            Self::WebhooksDeliveries => "webhooks.deliveries",
        }
    }

    /// The lowest role the service accepts the operation from.
    ///
    /// Reads are a viewer's and writes need a member who may change things, with
    /// one exception that is not a relaxation: `me.*` and `calendar.*` touch only
    /// the caller's own scheduler user (their name, time zone, notifications,
    /// picture and calendars), so a viewer may write them. A viewer who could not
    /// set their own time zone would be offered every window in the wrong hours.
    ///
    /// This decides which controls to offer and nothing more. The scheduler
    /// checks the member's own scheduler role too, so an operation this allows
    /// can still be refused.
    pub const fn min_role(self) -> SchedulingAdminRole {
        match self {
            Self::MeGet
            | Self::MePatch
            | Self::MeAvatarDelete
            | Self::EventTypesList
            | Self::EventTypesGet
            | Self::EventTypesHostsGet
            | Self::EventTypesQuestionsList
            | Self::EventTypesSlots
            | Self::AvailabilityRulesList
            | Self::AvailabilityOverridesList
            | Self::BookingsList
            | Self::BookingsAnswers
            | Self::CalendarStatus
            | Self::CalendarCaldavConnect
            | Self::CalendarConnectionsCalendarsGet
            | Self::CalendarConnectionsCalendarsPut
            | Self::CalendarConnectionsDestination
            | Self::CalendarConnectionsDelete
            | Self::ZoomStatus
            | Self::UsersList
            | Self::UsersUpcomingBookings
            | Self::TeamsList
            | Self::TeamsGet
            | Self::SettingsBrandingGet
            | Self::SettingsLlmGet
            | Self::ApiKeysList
            | Self::OauthConnectionsList
            | Self::WebhooksList
            | Self::WebhooksDeliveries => SchedulingAdminRole::Viewer,
            Self::EventTypesCreate
            | Self::EventTypesPatch
            | Self::EventTypesDelete
            | Self::EventTypesHostsPut
            | Self::EventTypesTestEmail
            | Self::EventTypesQuestionsCreate
            | Self::EventTypesQuestionsPatch
            | Self::EventTypesQuestionsDelete
            | Self::AvailabilityRulesCreate
            | Self::AvailabilityRulesPatch
            | Self::AvailabilityRulesDelete
            | Self::AvailabilityOverridesCreate
            | Self::AvailabilityOverridesPatch
            | Self::AvailabilityOverridesDelete
            | Self::AvailabilityOverridesDeleteGroup
            | Self::BookingsCancel
            | Self::BookingsReschedule
            | Self::BookingsReassign
            | Self::UsersArchive
            | Self::TeamsCreate
            | Self::TeamsPatch
            | Self::TeamsDelete
            | Self::TeamsMembersAdd
            | Self::TeamsMembersPatch
            | Self::TeamsMembersRemove
            | Self::SettingsBrandingPatch
            | Self::SettingsBrandingLogoDelete
            | Self::SettingsBrandingBannerDelete
            | Self::SettingsLlmPatch
            | Self::ApiKeysCreate
            | Self::ApiKeysDelete
            | Self::OauthConnectionsDelete
            | Self::WebhooksCreate
            | Self::WebhooksPatch
            | Self::WebhooksDelete => SchedulingAdminRole::Client,
        }
    }

    /// Whether sending the operation spends one of the workspace's (or, for a
    /// viewer's own writes, the member's) hourly writes: the service's own test,
    /// "the scheduler method is not GET". Not the same as needing
    /// [`SchedulingAdminRole::Client`]: the viewer-level writes are writes.
    pub const fn is_write(self) -> bool {
        !matches!(
            self,
            Self::MeGet
                | Self::EventTypesList
                | Self::EventTypesGet
                | Self::EventTypesHostsGet
                | Self::EventTypesQuestionsList
                | Self::EventTypesSlots
                | Self::AvailabilityRulesList
                | Self::AvailabilityOverridesList
                | Self::BookingsList
                | Self::BookingsAnswers
                | Self::CalendarStatus
                | Self::CalendarConnectionsCalendarsGet
                | Self::ZoomStatus
                | Self::UsersList
                | Self::UsersUpcomingBookings
                | Self::TeamsList
                | Self::TeamsGet
                | Self::SettingsBrandingGet
                | Self::SettingsLlmGet
                | Self::ApiKeysList
                | Self::OauthConnectionsList
                | Self::WebhooksList
                | Self::WebhooksDeliveries
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The catalog's names, typed out a second time. A test that read them back
    /// from `as_str` would pass through any rename.
    const NAMES: [&str; 64] = [
        "me.get",
        "me.patch",
        "me.avatar.delete",
        "eventTypes.list",
        "eventTypes.get",
        "eventTypes.create",
        "eventTypes.patch",
        "eventTypes.delete",
        "eventTypes.hosts.get",
        "eventTypes.hosts.put",
        "eventTypes.testEmail",
        "eventTypes.questions.list",
        "eventTypes.questions.create",
        "eventTypes.questions.patch",
        "eventTypes.questions.delete",
        "eventTypes.slots",
        "availability.rules.list",
        "availability.rules.create",
        "availability.rules.patch",
        "availability.rules.delete",
        "availability.overrides.list",
        "availability.overrides.create",
        "availability.overrides.patch",
        "availability.overrides.delete",
        "availability.overrides.deleteGroup",
        "bookings.list",
        "bookings.answers",
        "bookings.cancel",
        "bookings.reschedule",
        "bookings.reassign",
        "calendar.status",
        "calendar.caldav.connect",
        "calendar.connections.calendars.get",
        "calendar.connections.calendars.put",
        "calendar.connections.destination",
        "calendar.connections.delete",
        "zoom.status",
        "users.list",
        "users.archive",
        "users.upcomingBookings",
        "teams.list",
        "teams.get",
        "teams.create",
        "teams.patch",
        "teams.delete",
        "teams.members.add",
        "teams.members.patch",
        "teams.members.remove",
        "settings.branding.get",
        "settings.branding.patch",
        "settings.branding.logo.delete",
        "settings.branding.banner.delete",
        "settings.llm.get",
        "settings.llm.patch",
        "apiKeys.list",
        "apiKeys.create",
        "apiKeys.delete",
        "oauth.connections.list",
        "oauth.connections.delete",
        "webhooks.list",
        "webhooks.create",
        "webhooks.patch",
        "webhooks.delete",
        "webhooks.deliveries",
    ];

    #[test]
    fn every_name_is_the_catalogs() {
        let names: Vec<&str> = SchedulingAdminOp::ALL
            .iter()
            .map(|op| op.as_str())
            .collect();
        assert_eq!(names, NAMES);
    }

    #[test]
    fn the_split_is_counted_not_assumed() {
        let viewer = SchedulingAdminOp::ALL
            .iter()
            .filter(|op| op.min_role() == SchedulingAdminRole::Viewer)
            .count();
        let writes = SchedulingAdminOp::ALL
            .iter()
            .filter(|op| op.is_write())
            .count();
        assert_eq!((viewer, 64 - viewer), (29, 35));
        assert_eq!((64 - writes, writes), (23, 41));
    }

    #[test]
    fn the_viewer_writes_are_the_callers_own() {
        let viewer_writes: Vec<&str> = SchedulingAdminOp::ALL
            .iter()
            .filter(|op| op.is_write() && op.min_role() == SchedulingAdminRole::Viewer)
            .map(|op| op.as_str())
            .collect();
        assert_eq!(
            viewer_writes,
            [
                "me.patch",
                "me.avatar.delete",
                "calendar.caldav.connect",
                "calendar.connections.calendars.put",
                "calendar.connections.destination",
                "calendar.connections.delete",
            ]
        );
    }
}
