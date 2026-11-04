//! The WhatsApp composer's reads: the customer-service window and the message
//! templates.

use serde::{Deserialize, Serialize};

/// `GET /api/district/workspace/whatsapp/window`: whether free-form WhatsApp text
/// may go from a sender to a customer now, and until when. The composer's
/// preview only: the send route enforces the same window itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppWindowResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The window.
    pub window: WhatsAppWindow,
    /// The sender whose window this is: the one asked about, or else the one
    /// the send route would choose.
    pub sender_id: Option<String>,
    /// The number of ours the customer's latest WhatsApp message reached, or
    /// `None` when they never wrote. It labels the thread; it never names the
    /// sender.
    pub received_on: Option<String>,
}

/// The 24-hour customer-service window. When it is closed, only a template can
/// be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppWindow {
    /// Whether free-form text may be sent now.
    pub open: bool,
    /// When it closes, as an ISO 8601 instant; `None` when closed.
    pub expires_at: Option<String>,
}

/// `GET /api/district/workspace/whatsapp/templates`: the workspace's WhatsApp
/// message templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppTemplatesResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The templates.
    #[serde(default)]
    pub templates: Vec<WhatsAppTemplate>,
}

/// One WhatsApp message template. Only an `approved` one can be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppTemplate {
    /// The template's id, which the send route takes as `templateId`.
    pub id: String,
    /// The sender it belongs to, or `None` for one not tied to a sender.
    pub sender_id: Option<String>,
    /// The template's name at the provider.
    pub name: String,
    /// Its language, for example `en_US`.
    pub language: String,
    /// The provider's category, for example `UTILITY` or `MARKETING`.
    pub category: String,
    /// `approved`, `pending` or another provider status.
    pub status: String,
    /// The body text, with its `{{n}}` or `{{name}}` variables.
    pub body: String,
    /// How many positional `{{n}}` variables the body has, or `None` when it uses
    /// named ones or has no body text.
    pub variable_count: Option<i64>,
}
