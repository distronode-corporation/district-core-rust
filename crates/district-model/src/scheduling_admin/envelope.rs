//! The envelope every answer of the scheduling administration route comes in,
//! and the helpers its row types share.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

/// The code of the 409 for a workspace whose booking pages are not set up: no
/// operation can reach a scheduler that does not exist. The code travels in the
/// `X-Distronode-Error-Code` header, because the body's shape is pinned.
pub const CODE_SCHEDULING_NOT_READY: &str = "scheduling_not_ready";

/// The code of the 400 for parameters the catalog's schema refused. The body
/// names the fields, never what was wrong with them.
pub const CODE_INVALID_PARAMS: &str = "invalid_params";

/// An answer the route sent, the scheduler having done the work: `{ok: true,
/// data}`. `ok` is modelled although the reader has already branched on it,
/// so that the contract tests' round trip keeps the key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingAdminSuccess<T> {
    /// Always true here.
    pub ok: bool,
    /// What the operation answered.
    pub data: T,
}

/// A refusal by the scheduler itself, which the route answers with a 200:
/// `{ok: false, failure, status}`. The request reached the service, was
/// allowed and was valid; the scheduler behind it refused, failed or did not
/// answer. Its own words never cross: only a kind and a number.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingAdminFailure {
    /// Always false here.
    pub ok: bool,
    /// What went wrong: `unavailable`, `conflict`, `not_found` and `rejected`
    /// from the scheduler's client, `instance_unavailable` and `slot_taken`
    /// from the calendar's failure modes, or a kind added later.
    pub failure: String,
    /// The scheduler's own HTTP status, or 502 when it never answered. For a
    /// log line, never for a screen.
    pub status: i64,
}

impl SchedulingAdminFailure {
    /// The kind for a scheduler that did not answer, in either vocabulary.
    pub fn is_unavailable(&self) -> bool {
        matches!(
            self.failure.as_str(),
            "unavailable" | "instance_unavailable"
        )
    }

    /// The kind for a time someone else booked between the list and the
    /// choice: the one refusal whose remedy is to choose again.
    pub fn is_slot_taken(&self) -> bool {
        self.failure == "slot_taken"
    }
}

/// The body of the route's own refusals: `{error}`, and `{error, fields}` for
/// [`CODE_INVALID_PARAMS`]. The fields are names to look up, never prose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingAdminErrorBody {
    /// The refusal's code.
    pub error: String,
    /// The fields the schema refused, for an `invalid_params`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<Vec<String>>,
}

/// `{items}`: the list envelope most of the catalog's reads answer in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingItems<T> {
    /// The rows.
    pub items: Vec<T>,
}

/// `{ok: true}`: what an operation that answers nothing answers (the service
/// rewrites an empty answer into it), and what deleting a team or one of its
/// members answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingOk {
    /// Whether it was done.
    pub ok: bool,
}

/// A credential this client sends: a CalDAV app password. Its `Debug` output is
/// redacted, so no `{:?}` of a request, an effect or a screen can put it in a
/// log. (The credentials the service shows once, an API key and a webhook's
/// signing secret, arrive as plain fields of types whose `Debug` is redacted.)
#[derive(Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SchedulingSecret(String);

impl SchedulingSecret {
    /// Wraps `secret`.
    pub fn new(secret: impl Into<String>) -> Self {
        Self(secret.into())
    }

    /// The secret, for the one place that sends or shows it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SchedulingSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Reads a key the scheduler may leave out or send as `null`, keeping which:
/// `None` when it was left out, `Some(None)` for `null`. With
/// `skip_serializing_if = "Option::is_none"` beside it, a row goes back exactly
/// as it came.
pub(crate) fn nullish<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_is_never_printed() {
        let secret = SchedulingSecret::new("whsec_123");
        assert_eq!(format!("{secret:?}"), "<redacted>");
        assert_eq!(secret.expose(), "whsec_123");
        assert_eq!(serde_json::to_string(&secret).unwrap(), "\"whsec_123\"");
    }

    #[test]
    fn the_two_spellings_of_a_scheduler_that_did_not_answer() {
        let failure = |kind: &str| SchedulingAdminFailure {
            ok: false,
            failure: kind.to_owned(),
            status: 502,
        };
        assert!(failure("unavailable").is_unavailable());
        assert!(failure("instance_unavailable").is_unavailable());
        assert!(!failure("rejected").is_unavailable());
        assert!(failure("slot_taken").is_slot_taken());
        assert!(!failure("conflict").is_slot_taken());
    }
}
