//! What the scheduler hands other software: API keys, the apps connected with
//! OAuth, and webhooks with their deliveries.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::envelope::nullish;

/// The events a webhook can subscribe to, as the catalog lists them. The last
/// three are about recordings, which are not made; a webhook created on the web
/// may still carry them.
pub const SCHEDULING_WEBHOOK_EVENTS: [&str; 7] = [
    "booking.created",
    "booking.cancelled",
    "booking.rescheduled",
    "booking.reminder",
    "recording.completed",
    "transcript.ready",
    "notes.ready",
];

/// One API key, never with the key itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingApiKey {
    /// Its id.
    pub id: String,
    /// The name it was given.
    pub name: String,
    /// When it was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When it was last used, or `null` if never.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_used_at: Option<Option<String>>,
}

impl SchedulingApiKey {
    /// When it was last used, left out and `null` alike being never.
    pub fn last_used_at(&self) -> Option<&str> {
        self.last_used_at.as_ref().and_then(|v| v.as_deref())
    }
}

/// `apiKeys.create`: the new key, with its plain text, shown this once and
/// never again. A credential: `Debug` leaves it out.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingApiKeyCreated {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The key. Show it once, offer to copy it, and keep it nowhere.
    pub key: String,
    /// When it was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// The scheduler's own reminder that it is shown once, in English.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl fmt::Debug for SchedulingApiKeyCreated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedulingApiKeyCreated")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("key", &"<redacted>")
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

/// An app connected to the scheduler with OAuth.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingOauthConnection {
    /// Its id.
    pub id: String,
    /// The app's name.
    pub client_name: String,
    /// When it was connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// When it last used its access, or `null` if never.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_used_at: Option<Option<String>>,
    /// When its access runs out, or `null` if it does not.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub expires_at: Option<Option<String>>,
}

impl SchedulingOauthConnection {
    /// When it last used its access, left out and `null` alike being never.
    pub fn last_used_at(&self) -> Option<&str> {
        self.last_used_at.as_ref().and_then(|v| v.as_deref())
    }

    /// When its access runs out, the same way.
    pub fn expires_at(&self) -> Option<&str> {
        self.expires_at.as_ref().and_then(|v| v.as_deref())
    }
}

/// One webhook. The signing secret is never on the list: it is shown once,
/// when the webhook is made.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingWebhook {
    /// Its id.
    pub id: String,
    /// Where it posts.
    pub url: String,
    /// What it posts.
    pub events: Vec<String>,
    /// The booking fields each post carries.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub fields: Option<Option<Vec<String>>>,
    /// Whether it posts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    /// When it was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

impl SchedulingWebhook {
    /// The booking fields, left out and `null` alike being none.
    pub fn fields(&self) -> &[String] {
        self.fields
            .as_ref()
            .and_then(|value| value.as_deref())
            .unwrap_or_default()
    }
}

/// `webhooks.create`: the new webhook and its signing secret, shown this once.
/// A credential: `Debug` leaves the secret out.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingWebhookCreated {
    /// Its id.
    pub id: String,
    /// Where it posts.
    pub url: String,
    /// What it posts.
    pub events: Vec<String>,
    /// The booking fields each post carries.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub fields: Option<Option<Vec<String>>>,
    /// Whether it posts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_active: Option<bool>,
    /// When it was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// The secret its posts are signed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

impl fmt::Debug for SchedulingWebhookCreated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SchedulingWebhookCreated")
            .field("id", &self.id)
            .field("url", &self.url)
            .field("events", &self.events)
            .field("secret", &self.secret.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

/// One attempt to deliver a webhook's post.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingWebhookDelivery {
    /// Its id.
    pub id: String,
    /// The webhook's id.
    pub webhook_id: String,
    /// The event it posted.
    pub event: String,
    /// `success`, `failed` or `pending`, or a status added later.
    pub status: String,
    /// How many times it was tried.
    pub attempt_count: i64,
    /// The booking it was about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub booking_id: Option<String>,
    /// The receiver's HTTP status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_status: Option<i64>,
    /// When it was last tried.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempted_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_shown_once_is_never_printed() {
        let key = SchedulingApiKeyCreated {
            key: "cno_secret".into(),
            ..SchedulingApiKeyCreated::default()
        };
        assert!(!format!("{key:?}").contains("cno_secret"));
        let webhook = SchedulingWebhookCreated {
            secret: Some("whsec_secret".into()),
            ..SchedulingWebhookCreated::default()
        };
        let shown = format!("{webhook:?}");
        assert!(!shown.contains("whsec_secret"), "{shown}");
        assert!(shown.contains("<redacted>"), "{shown}");
    }

    #[test]
    fn never_used_reads_the_same_left_out_or_null() {
        let key: SchedulingApiKey =
            serde_json::from_str(r#"{"id":"k","name":"n","last_used_at":null}"#).unwrap();
        assert_eq!(key.last_used_at, Some(None));
        assert_eq!(key.last_used_at(), None);
        let app: SchedulingOauthConnection =
            serde_json::from_str(r#"{"id":"o","client_name":"c","expires_at":"2026"}"#).unwrap();
        assert_eq!(app.last_used_at(), None);
        assert_eq!(app.expires_at(), Some("2026"));
        let hook = SchedulingWebhook::default();
        assert!(hook.fields().is_empty());
        let used: SchedulingOauthConnection =
            serde_json::from_str(r#"{"id":"o","client_name":"c","last_used_at":"2026"}"#).unwrap();
        assert_eq!(used.last_used_at(), Some("2026"));
        let chosen: SchedulingWebhook =
            serde_json::from_str(r#"{"id":"w","url":"u","events":[],"fields":["id"]}"#).unwrap();
        assert_eq!(chosen.fields(), ["id"]);
    }
}
