//! The Messenger and Instagram composer's read: the reply window.

use serde::{Deserialize, Serialize};

/// `GET /api/district/messages/send?workspaceId&channel&to`: the Messenger or
/// Instagram reply window for one customer. The composer's preview only: the
/// send route enforces the same window itself, and refuses a reply outside it
/// with `messenger_window_closed` (for Instagram too).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct MetaWindowResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The window.
    pub window: MetaWindow,
}

/// Where the reply window stands. Meta lets a business reply freely for 24 hours
/// after the customer's last message; from then until 7 days only a person may
/// reply, tagged as a human agent, and only when the page or account has that
/// switched on; after that nothing goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct MetaWindow {
    /// [`META_WINDOW_OPEN`], [`META_WINDOW_HUMAN_AGENT`] or
    /// [`META_WINDOW_CLOSED`]. Kept as text, so a state this build does not know
    /// still decodes; the web composer treats one as blocking nothing.
    pub state: String,
    /// When the current state ends, as an ISO 8601 instant; `None` when closed.
    pub until: Option<String>,
}

/// Any reply may go, a person's or the receptionist's.
pub const META_WINDOW_OPEN: &str = "open";
/// Only a person may reply (tagged as a human agent); the receptionist may not.
pub const META_WINDOW_HUMAN_AGENT: &str = "human_agent";
/// Nothing may be sent until the customer writes again.
pub const META_WINDOW_CLOSED: &str = "closed";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_this_build_does_not_know_still_decodes() {
        let response: MetaWindowResponse = serde_json::from_str(
            r#"{"success":true,"window":{"state":"ad_window","until":"2026-08-22T14:25:00.000Z"}}"#,
        )
        .expect("an unknown state is text, not a decode error");
        assert_eq!(response.window.state, "ad_window");
        assert_eq!(
            response.window.until.as_deref(),
            Some("2026-08-22T14:25:00.000Z")
        );
    }
}
