//! The signed-in member's scheduler profile, the booking pages' branding, and
//! the meeting summariser's settings.

use serde::{Deserialize, Serialize};

/// `me.get`: the signed-in member's own scheduler user.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingMe {
    /// Their scheduler id.
    pub id: String,
    /// Their address.
    pub email: String,
    /// Their name.
    pub name: String,
    /// The IANA zone every scheduling time is shown in for them: their
    /// profile's, not the computer's. Empty reads as UTC.
    pub timezone: String,
    /// `12h` or `24h`.
    pub time_format: String,
    /// The first day of the week, 0 to 6, Sunday first.
    pub week_start: i64,
    /// `dmy`, `mdy` or `ymd`.
    pub date_format: String,
    /// Whether they administer the scheduler: the only thing that decides
    /// whether a booking can be handed to another host.
    pub is_admin: bool,
    /// Whether they own it.
    pub is_owner: bool,
    /// `owner`, `admin` or `member`.
    pub role: String,
    /// Email the booker a confirmation.
    pub notify_confirmation: bool,
    /// Email the booker a cancellation.
    pub notify_cancellation: bool,
    /// Email the booker a reschedule.
    pub notify_reschedule: bool,
    /// Email the booker a reminder.
    pub notify_reminder: bool,
    /// Email them each new booking.
    pub notify_host_booking: bool,
    /// Email them each cancellation.
    pub notify_host_cancel: bool,
    /// Email them each reschedule.
    pub notify_host_reschedule: bool,
    /// Their picture, left out when they have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

impl SchedulingMe {
    /// The zone their times are shown in: their profile's, or UTC for an empty
    /// one. The one place that says so, so no two screens show one booking at
    /// two different hours.
    pub fn display_timezone(&self) -> &str {
        if self.timezone.is_empty() {
            "UTC"
        } else {
            &self.timezone
        }
    }
}

/// A language the booking pages can be shown in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingLocaleOption {
    /// Its code, `en`.
    pub code: String,
    /// Its name, in itself.
    pub name: String,
}

/// `settings.branding.get`: how the booking pages look.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingBranding {
    /// The name the pages show.
    pub business_name: String,
    /// The logo, or empty.
    pub logo_url: String,
    /// The logo's height in pixels.
    pub logo_height: i64,
    /// The logo's opacity, 0 to 100.
    pub logo_opacity: i64,
    /// The banner, or empty.
    pub banner_url: String,
    /// The banner's opacity, 0 to 100.
    pub banner_opacity: i64,
    /// The privacy policy's link, or empty.
    pub privacy_url: String,
    /// The terms' link, or empty.
    pub terms_url: String,
    /// The language a booker's own is replaced by when the pages lack it.
    pub fallback_locale: String,
    /// The languages the pages can be shown in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supported_locales: Option<Vec<SchedulingLocaleOption>>,
}

/// `settings.llm.get`: whether meetings are summarised, and the summariser's
/// extra instructions. Nothing else about the model crosses the route.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingLlmSettings {
    /// Whether it is on.
    pub enabled: bool,
    /// The instructions added to its prompt.
    pub extra_instructions: String,
}

/// What the upload route answers: the published image's address, under the
/// key of the image that was replaced.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingUploadResult {
    /// The new logo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    /// The new banner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
    /// The new picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
}

impl SchedulingUploadResult {
    /// The published address, whichever image it was.
    pub fn published_url(&self) -> Option<&str> {
        self.logo_url
            .as_deref()
            .or(self.banner_url.as_deref())
            .or(self.avatar_url.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_zone_is_utc() {
        let mut me = SchedulingMe::default();
        assert_eq!(me.display_timezone(), "UTC");
        me.timezone = "America/Toronto".into();
        assert_eq!(me.display_timezone(), "America/Toronto");
    }

    #[test]
    fn the_published_address_is_whichever_came() {
        let answer = |logo: Option<&str>, banner: Option<&str>, avatar: Option<&str>| {
            SchedulingUploadResult {
                logo_url: logo.map(Into::into),
                banner_url: banner.map(Into::into),
                avatar_url: avatar.map(Into::into),
            }
        };
        assert_eq!(answer(Some("l"), None, None).published_url(), Some("l"));
        assert_eq!(answer(None, Some("b"), None).published_url(), Some("b"));
        assert_eq!(answer(None, None, Some("a")).published_url(), Some("a"));
        assert_eq!(answer(None, None, None).published_url(), None);
    }
}
