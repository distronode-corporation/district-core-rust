//! The account's own security: two-step verification, the fresh
//! re-authentication that guards a change to it or the account's deletion, and
//! the deletion itself.
//!
//! Three things here are credentials the service hands over once: the
//! authenticator secret ([`MfaEnrollment`]), the recovery codes
//! ([`MfaConfirmation`]) and what the person types to prove who they are
//! ([`Credential`]). Each one's `Debug` output is redacted, so no `{:?}` of a
//! response, an effect or a screen can put one in a log.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What a fresh re-authentication is for. The service binds each proof to one
/// action and refuses a proof made for another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReauthAction {
    /// Deleting the signed-in account.
    DeleteAccount,
    /// Turning two-step verification on or off.
    TwoStepChange,
}

impl ReauthAction {
    /// Both actions.
    pub const ALL: [Self; 2] = [Self::DeleteAccount, Self::TwoStepChange];

    /// The action as the service names it.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::DeleteAccount => "delete-account",
            Self::TwoStepChange => "2fa-change",
        }
    }
}

/// Something the person typed or a sign-in handed back that proves who they
/// are: a password, an authenticator code, a recovery code, the one-time code a
/// re-authentication sheet returned. Sent once and never kept.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Credential(String);

impl Credential {
    /// Wraps `value`.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value, for the one request that sends it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential(<redacted>)")
    }
}

/// `GET /api/auth/mfa`: whether the account has two-step verification, and how
/// many recovery codes are left. Counts and booleans only: the secret and the
/// codes leave the service once each, never here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct MfaStatus {
    /// Whether it is on.
    pub enrolled: bool,
    /// When it was turned on, as an ISO 8601 instant, or `None` while it is off.
    pub enrolled_at: Option<String>,
    /// How many unused recovery codes are left.
    #[serde(default)]
    pub backup_codes_remaining: i64,
    /// Whether the website would let this account turn it off: `false` for an
    /// account with no password, because a browser re-proves the password to do
    /// it. Not a limit on an app, whose fresh re-authentication stands in for
    /// the password.
    #[serde(default)]
    pub can_disable: bool,
}

impl MfaStatus {
    /// Whether the answer holds together: `enrolled` exactly when there is an
    /// `enrolled_at`. An answer that does not is treated as unreadable.
    pub fn is_consistent(&self) -> bool {
        self.enrolled == self.enrolled_at.is_some()
    }
}

/// `POST /api/auth/mfa/enroll`: a new authenticator secret, not on until it is
/// confirmed with one of its codes. Starting again replaces it.
///
/// All three fields are the secret: the URI carries it as a query item, and the
/// QR code is that URI as a picture. Its `Debug` output names none of them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct MfaEnrollment {
    /// The base32 key, for typing into an authenticator by hand.
    pub secret: String,
    /// The `otpauth://` URI an authenticator app opens.
    pub otpauth_uri: String,
    /// A `data:image/png;base64,...` QR code of the URI, drawn by the service.
    pub qr_data_url: String,
}

impl fmt::Debug for MfaEnrollment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MfaEnrollment(<redacted>)")
    }
}

/// `POST /api/auth/mfa/confirm`: two-step verification is on, and these are its
/// recovery codes, which the service never shows again. Its `Debug` output
/// gives their count only.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct MfaConfirmation {
    /// `true`: it is on.
    pub enrolled: bool,
    /// The recovery codes, each good for one sign-in.
    #[serde(default)]
    pub backup_codes: Vec<String>,
}

impl fmt::Debug for MfaConfirmation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MfaConfirmation {{ enrolled: {}, backup_codes: <{} redacted> }}",
            self.enrolled,
            self.backup_codes.len()
        )
    }
}

/// `POST /api/auth/mfa/disable`: two-step verification and its recovery codes
/// are gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct MfaDisableResponse {
    /// `false`: it is off.
    pub enrolled: bool,
}

/// `POST /api/auth/native/reauth/sso` and `/password`: a proof for one action
/// now stands on the service, bound to this installation, for
/// [`expires_in_seconds`](Self::expires_in_seconds).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct NativeReauthResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The action the proof is for, as the service names it
    /// ([`ReauthAction::wire`]).
    pub action: String,
    /// How long the proof stands.
    pub expires_in_seconds: i64,
}

/// `DELETE /api/auth/delete-account`: the account is gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct AccountDeleteResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// `true`: the account was deleted.
    #[serde(default)]
    pub account_deleted: bool,
    /// Whether support data a vendor holds is still being erased. The account
    /// itself is deleted either way.
    #[serde(default)]
    pub external_erasure_pending: bool,
    /// The service's own sentence about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The support-data erasure's counts, kept whole and not read: nothing on a
    /// device acts on them, and the service may add a count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub support_erasure: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_actions_have_the_service_names() {
        assert_eq!(
            ReauthAction::ALL.map(ReauthAction::wire),
            ["delete-account", "2fa-change"]
        );
    }

    #[test]
    fn credentials_never_reach_debug_output() {
        let credential = Credential::new("hunter2");
        assert_eq!(credential.expose(), "hunter2");
        assert_eq!(format!("{credential:?}"), "Credential(<redacted>)");
        assert_eq!(Credential::default().expose(), "");

        let enrollment = MfaEnrollment {
            secret: "JBSWY3DPEHPK3PXP".into(),
            otpauth_uri: "otpauth://totp/x?secret=JBSWY3DPEHPK3PXP".into(),
            qr_data_url: "data:image/png;base64,AAAA".into(),
        };
        assert_eq!(format!("{enrollment:?}"), "MfaEnrollment(<redacted>)");

        let confirmation = MfaConfirmation {
            enrolled: true,
            backup_codes: vec!["aaaa-bbbb".into(), "cccc-dddd".into()],
        };
        let shown = format!("{confirmation:?}");
        assert_eq!(
            shown,
            "MfaConfirmation { enrolled: true, backup_codes: <2 redacted> }"
        );
    }

    #[test]
    fn a_status_holds_together_only_when_enrolled_matches_its_date() {
        let status = |enrolled, at: Option<&str>| MfaStatus {
            enrolled,
            enrolled_at: at.map(str::to_owned),
            backup_codes_remaining: 0,
            can_disable: true,
        };
        assert!(status(true, Some("2026-10-01T12:00:00.000Z")).is_consistent());
        assert!(status(false, None).is_consistent());
        assert!(!status(true, None).is_consistent());
        assert!(!status(false, Some("2026-10-01T12:00:00.000Z")).is_consistent());
    }

    #[test]
    fn empty_answers_decode_to_the_defaults() {
        let delete: AccountDeleteResponse = serde_json::from_str("{}").unwrap();
        assert!(!delete.success && !delete.account_deleted && !delete.external_erasure_pending);
        assert_eq!(delete.message, None);
        assert_eq!(delete.support_erasure, None);
        let confirm: MfaConfirmation = serde_json::from_str(r#"{"enrolled":true}"#).unwrap();
        assert!(confirm.backup_codes.is_empty());
        let reauth: NativeReauthResponse =
            serde_json::from_str(r#"{"action":"2fa-change","expiresInSeconds":300}"#).unwrap();
        assert!(!reauth.success);
        let status: MfaStatus =
            serde_json::from_str(r#"{"enrolled":false,"enrolledAt":null}"#).unwrap();
        assert_eq!(status.backup_codes_remaining, 0);
        assert!(!status.can_disable);
    }
}
