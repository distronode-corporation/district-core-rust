//! What to tell the user about a failed request.
//!
//! One mapping, shared by every screen. The rule it holds is that a failure is
//! never shown as an absence of data: "we could not look" and "there is nothing"
//! are different answers, and showing the second when the first is true reads as
//! losing the account. Each screen still decides its own state (a 404 means "no
//! workspace" on the overview), but the words and whether to offer a retry come
//! from here.

use district_api::{
    ApiError, CODE_REGIONS_DEGRADED, ErrorDetail, RetryReason, TransportKind, UnauthorizedReason,
};
use district_l10n::{Locale, api_error_text};
use district_live::LiveError;
use district_model::{CODE_LAST_AGENCY_MEMBER, CODE_MEMBER_EXISTS};

use crate::session::SessionEnd;
use crate::text::{l, s};

/// What to tell the user about one failure.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FailureText {
    /// The sentence to show, in the language the core writes in. In English,
    /// the service's own wording where it wrote one for a person to read (a
    /// 4xx), this app's otherwise. In French, the sentence for the service's
    /// error code where it sent one this build knows, this app's otherwise:
    /// never the service's wording, which is English. Never the body of a 5xx,
    /// which can carry internal detail nobody can act on.
    pub message: String,
    /// The regions that did not answer, when that was the cause, so the screen
    /// can name them. Empty otherwise.
    pub degraded_regions: Vec<String>,
    /// Set when the session is over and signing in again is the only way on.
    pub session_ended: Option<SessionEnd>,
    /// Whether offering "Try again" is honest. False when a retry can only fail
    /// the same way: a role refusal, a missing record, a response this build
    /// cannot read, an ended session.
    pub retryable: bool,
}

impl FailureText {
    /// The text for a failed call.
    pub fn from_api_error(error: &ApiError) -> Self {
        match error {
            ApiError::Unauthorized(UnauthorizedReason::RefusedNotRetried) => {
                Self::retryable(s::failure_refused_not_retried(l()))
            }
            ApiError::Unauthorized(_) => {
                let end = SessionEnd::from_api_error(error);
                Self {
                    session_ended: end,
                    ..Self::final_(s::failure_signed_out(l()))
                }
            }
            ApiError::Forbidden(detail) => Self::final_(or_ours(detail, s::failure_forbidden(l()))),
            ApiError::NotFound(detail) => Self::final_(or_ours(detail, s::failure_not_found(l()))),
            ApiError::Conflict(detail) | ApiError::Rejected { detail, .. } => {
                Self::retryable(or_ours(detail, s::failure_fallback(l())))
            }
            ApiError::RateLimited { detail, .. } => {
                Self::retryable(or_ours(detail, s::failure_rate_limited(l())))
            }
            ApiError::TokenUnavailable(reason) => Self::from_retry_reason(*reason),
            ApiError::Envelope { code, detail, .. } if code == CODE_REGIONS_DEGRADED => Self {
                degraded_regions: detail.degraded_regions.clone(),
                ..Self::retryable(s::failure_regions_degraded(l()))
            },
            ApiError::Envelope { status, detail, .. } if *status < 500 => {
                Self::retryable(or_ours(detail, s::failure_fallback(l())))
            }
            ApiError::Envelope { .. } | ApiError::Server { .. } => {
                Self::retryable(s::failure_server(l()))
            }
            ApiError::Redirect { .. } => Self::retryable(s::failure_redirected(l())),
            ApiError::Offline(transport) => match transport.kind {
                TransportKind::NotJson { .. } => Self::retryable(s::failure_not_json(l())),
                TransportKind::Connect | TransportKind::Timeout | TransportKind::Other => {
                    Self::retryable(s::failure_offline(l()))
                }
            },
            ApiError::Decode { .. } | ApiError::Unconfirmed { .. } => {
                Self::final_(s::failure_unexpected_response(l()))
            }
            ApiError::InvalidRequest(_) => Self::final_(s::failure_app_bug(l())),
        }
    }

    /// The text for a failed change of a member. The two refusals the service
    /// names (an address that is already a member, and a change that would
    /// leave the workspace with no agency member) are shown in its words and
    /// offer no retry: pressing again gets the same answer. Anything else reads
    /// as it does everywhere.
    pub fn from_member_error(error: &ApiError) -> Self {
        match error {
            ApiError::Conflict(detail) if detail.code.as_deref() == Some(CODE_MEMBER_EXISTS) => {
                Self::final_(or_ours(detail, s::failure_member_exists(l())))
            }
            ApiError::Conflict(detail)
                if detail.code.as_deref() == Some(CODE_LAST_AGENCY_MEMBER) =>
            {
                Self::final_(or_ours(detail, s::failure_last_agency_member(l())))
            }
            other => Self::from_api_error(other),
        }
    }

    /// The text for a contact that could not be added or changed. A conflict
    /// is another contact already holding that phone number or email address:
    /// English shows the service's sentence (it says which), any other
    /// language the catalog's, and neither offers a retry, which would get the
    /// same answer (visual QA LB7). Anything else reads as it does everywhere.
    pub fn from_contact_error(error: &ApiError) -> Self {
        match error {
            ApiError::Conflict(detail) => Self::final_(match l() {
                Locale::En => detail
                    .message
                    .clone()
                    .unwrap_or_else(|| s::failure_contact_exists(l()).to_owned()),
                Locale::FrCa => s::failure_contact_exists(l()).to_owned(),
            }),
            other => Self::from_api_error(other),
        }
    }

    /// The text for a session that is intact but has no token right now. Each
    /// reason has its own remedy, and the sentence names it.
    pub fn from_retry_reason(reason: RetryReason) -> Self {
        Self::retryable(match reason {
            RetryReason::RateLimited => s::failure_rate_limited(l()),
            RetryReason::Offline => s::failure_offline(l()),
            RetryReason::SecretStoreUnavailable => s::failure_keyring_unavailable(l()),
            RetryReason::SecretStoreLocked => s::failure_keyring_locked(l()),
            RetryReason::StorageFailed => s::failure_storage_failed(l()),
        })
    }

    /// The text for live updates that stopped for good. Only the credential's
    /// own failure can say more than "refresh by hand": the others need an update
    /// of the app or a change of membership, which no retry brings about.
    pub fn from_live_error(error: &LiveError) -> Self {
        match error {
            LiveError::Mint(error) => Self::from_api_error(error),
            LiveError::Forbidden { .. } => Self::final_(s::failure_live_forbidden(l())),
            LiveError::InvalidGrant | LiveError::Endpoint(_) | LiveError::Protocol => {
                Self::final_(s::failure_live_unusable(l()))
            }
        }
    }

    /// "Affected regions: EU, APAC", or `None` when no region was named.
    pub fn regions_line(&self) -> Option<String> {
        (!self.degraded_regions.is_empty())
            .then(|| s::failure_affected_regions(l(), &regions(&self.degraded_regions)))
    }

    /// A failure a retry may fix.
    pub(crate) fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            degraded_regions: Vec::new(),
            session_ended: None,
            retryable: true,
        }
    }

    /// A success that did not carry what a success carries: a call read with no
    /// call in it, a create with no new id. The service and this build disagree
    /// about the answer's shape.
    pub(crate) fn unexpected() -> Self {
        Self::final_(s::failure_unexpected_response(l()))
    }

    /// A failure a retry cannot fix.
    pub(crate) fn final_(message: impl Into<String>) -> Self {
        Self {
            retryable: false,
            ..Self::retryable(message)
        }
    }
}

/// What to say about a refusal the service may have worded itself. In English,
/// its own sentence when it wrote one, else `ours`. In French never its
/// sentence, which is English: the catalog's sentence for its code when the
/// code is one the service's contract has, else `ours`.
fn or_ours(detail: &ErrorDetail, ours: &str) -> String {
    let theirs = match l() {
        Locale::En => detail.message.clone(),
        Locale::FrCa => detail.code.as_deref().and_then(|code| {
            let regions: Vec<String> = detail
                .degraded_regions
                .iter()
                .map(|r| r.to_uppercase())
                .collect();
            api_error_text(Locale::FrCa, code, &regions)
        }),
    };
    theirs.unwrap_or_else(|| ours.to_owned())
}

/// Region codes as a reader sees them: `EU, APAC`.
pub(crate) fn regions(codes: &[String]) -> String {
    codes
        .iter()
        .map(|code| code.to_uppercase())
        .collect::<Vec<_>>()
        .join(", ")
}
