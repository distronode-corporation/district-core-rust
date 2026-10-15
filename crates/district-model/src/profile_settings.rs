//! The account's own settings, `/api/settings`: who the member is, and the
//! language their account speaks.
//!
//! The language is the one thing a desktop app writes here. The service reads
//! it for whatever it says to the member away from the app (an email, a text
//! message), so an app that shows French tells the account so.

use serde::{Deserialize, Serialize};

/// [`SettingsProfile::locale`] for French.
pub const SETTINGS_LANGUAGE_FRENCH: &str = "fr";

/// [`SettingsProfile::locale`] for English.
pub const SETTINGS_LANGUAGE_ENGLISH: &str = "en";

/// `GET /api/settings`: the account's profile, the first workspace's plan and
/// its data region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SettingsResponse {
    /// The account's profile.
    pub profile: SettingsProfile,
    /// The first workspace's plan, or `Free` and `none` without one.
    pub subscription: SettingsSubscription,
    /// The first workspace's data region, `us` without one.
    pub region: String,
}

/// The account's profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SettingsProfile {
    /// The account's email address.
    pub email: String,
    /// The name the member chose, or empty.
    pub display_name: String,
    /// An IANA time zone.
    pub timezone: String,
    /// [`SETTINGS_LANGUAGE_FRENCH`] or [`SETTINGS_LANGUAGE_ENGLISH`], or `None` for an
    /// account that never chose, which the service sends as `null`.
    pub locale: Option<String>,
}

/// The first workspace's plan, as [`SettingsResponse`] carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SettingsSubscription {
    /// The plan's tier as stored (`VoicePro`), or `Free`.
    pub tier: String,
    /// Its status (`active`), or `none`.
    pub status: String,
    /// When the workspace last changed, as an ISO 8601 instant. Left out with no
    /// workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// `POST /api/settings`: the body that sets the account's language, and
/// nothing else. A field the body leaves out is kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanguageSave {
    /// The one part of the profile written.
    pub profile: LanguageSaveProfile,
}

/// The profile part of [`LanguageSave`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanguageSaveProfile {
    /// [`SETTINGS_LANGUAGE_FRENCH`] or [`SETTINGS_LANGUAGE_ENGLISH`].
    pub locale: String,
}

impl LanguageSave {
    /// The body that sets the account's language to `locale`.
    pub fn new(locale: &str) -> Self {
        Self {
            profile: LanguageSaveProfile {
                locale: locale.to_owned(),
            },
        }
    }
}

/// `POST /api/settings`: the save's answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SettingsSaveResponse {
    /// Whether the settings were saved.
    pub success: bool,
    /// The service's words for it, in English.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_locale_body_writes_the_locale_alone() {
        let body = serde_json::to_value(LanguageSave::new(SETTINGS_LANGUAGE_FRENCH)).unwrap();
        assert_eq!(body, serde_json::json!({"profile": {"locale": "fr"}}));
        assert_eq!(SETTINGS_LANGUAGE_ENGLISH, "en");
    }
}
