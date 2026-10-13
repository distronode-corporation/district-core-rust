//! The signed-in member's own calendars: which are connected, which calendars
//! of an account are read for conflicts, and where new bookings are written.

use serde::{Deserialize, Serialize};

use super::envelope::nullish;

/// One connected calendar account.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingCalendarConnection {
    /// Its id. Recreated on every token refresh, so the account is named by
    /// the provider and the address beside it.
    pub id: String,
    /// `google`, `microsoft` or `caldav`.
    pub provider: String,
    /// The account's address.
    pub account_email: String,
    /// Whether new bookings are written to it.
    pub is_destination: bool,
    /// Whether it is read for conflicts.
    pub check_conflicts: bool,
}

/// `calendar.status`: the member's calendar connections.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingCalendarStatus {
    /// Whether any calendar is connected.
    pub connected: bool,
    /// Whether the scheduler can connect calendars at all.
    pub configured: bool,
    /// The providers that can be connected.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub providers: Option<Option<Vec<String>>>,
    /// The connections.
    pub connections: Vec<SchedulingCalendarConnection>,
    /// The providers the scheduler has no credentials for; `null` on every
    /// tenancy where both are set up.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub unconfigured_providers: Option<Option<Vec<String>>>,
    /// The provider of the first connection, for an older reader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

impl SchedulingCalendarStatus {
    /// The providers that can be connected, left out and `null` alike being
    /// none.
    pub fn providers(&self) -> &[String] {
        self.providers
            .as_ref()
            .and_then(|value| value.as_deref())
            .unwrap_or_default()
    }

    /// The providers the scheduler has no credentials for, the same way.
    pub fn unconfigured_providers(&self) -> &[String] {
        self.unconfigured_providers
            .as_ref()
            .and_then(|value| value.as_deref())
            .unwrap_or_default()
    }
}

/// `calendar.caldav.connect`: the CalDAV account that was connected.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingCaldavConnection {
    /// Whether it connected.
    pub connected: bool,
    /// The account's address.
    pub account_email: String,
}

/// One calendar of a connected account. Read back and sent back whole, so the
/// flags the scheduler left out stay left out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingCalendarSelection {
    /// Its id (often its address).
    pub id: String,
    /// Its name.
    pub name: String,
    /// Whether it is the account's main calendar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<bool>,
    /// Whether bookings can be written to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writable: Option<bool>,
    /// Whether it is read for conflicts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_conflicts: Option<bool>,
    /// Whether new bookings are written to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_destination: Option<bool>,
}

/// `calendar.connections.calendars.get`: an account's calendars.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingCalendarSelections {
    /// The calendars.
    pub calendars: Vec<SchedulingCalendarSelection>,
}

/// `zoom.status`: whether Zoom is set up and connected. Read only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingZoomStatus {
    /// Whether the scheduler can connect Zoom.
    pub configured: bool,
    /// Whether this member connected it.
    pub connected: bool,
}
