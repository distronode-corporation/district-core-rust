// SPDX-FileCopyrightText: Distronode Corporation
//
// SPDX-License-Identifier: Apache-2.0

//! The account's own security: two-step verification, the fresh
//! re-authentication that guards a change to it or the account's deletion, and
//! the deletion.
//!
//! Every change here needs a fresh proof first, from
//! [`native_reauth_sso`](ApiClient::native_reauth_sso) or
//! [`native_reauth_password`](ApiClient::native_reauth_password), for the
//! action it is ([`ReauthAction`]). The proof lives on the service, bound to
//! this installation's session, so no request carries it; without one the
//! service answers 403 `reauth_required`. A refusal keeps the service's `code`
//! (`MFA_REQUIRED`, `incorrect_password`, `mfa_code_invalid`, `reauth_failed`,
//! `confirmation_mismatch` and the others) for the caller to act on.
//!
//! Each change is sent once and never repeated automatically: a proof is spent
//! by the first request that reaches the service.

use district_model::{
    AccountDeleteResponse, Credential, MfaConfirmation, MfaDisableResponse, MfaEnrollment,
    MfaStatus, NativeReauthResponse, ReauthAction,
};

use crate::client::ApiClient;
use crate::endpoints::Endpoint;
use crate::error::ApiError;
use crate::methods::confirm;
use crate::token::TokenSource;

impl<S: TokenSource> ApiClient<S> {
    /// Whether two-step verification is on, and how many recovery codes are
    /// left. Needs no proof. An answer whose `enrolled` disagrees with its
    /// `enrolledAt` is [`ApiError::Unconfirmed`].
    pub async fn mfa_status(&self) -> Result<MfaStatus, ApiError> {
        let status: MfaStatus = self.request(Endpoint::MfaStatus).send().await?;
        confirm(Endpoint::MfaStatus, status.is_consistent())?;
        Ok(status)
    }

    /// Starts two-step verification: a new secret, its `otpauth://` URI and a
    /// QR code of it. Nothing is on until [`mfa_confirm`](Self::mfa_confirm);
    /// starting again replaces a secret not yet confirmed. Checks the
    /// `2fa-change` proof without spending it.
    pub async fn mfa_enroll(&self) -> Result<MfaEnrollment, ApiError> {
        self.request(Endpoint::MfaEnroll).send().await
    }

    /// Turns two-step verification on with `code`, one of the authenticator's
    /// codes, and answers the recovery codes, which the service never shows
    /// again. Spends the `2fa-change` proof. An answer that is not on, or has
    /// no codes, is [`ApiError::Unconfirmed`].
    pub async fn mfa_confirm(&self, code: &Credential) -> Result<MfaConfirmation, ApiError> {
        let confirmed: MfaConfirmation = self
            .request(Endpoint::MfaConfirm)
            .field("totp", code.expose())
            .send()
            .await?;
        // On, and with its codes: an answer without them would leave the
        // person nothing to save, so it reads as one this build cannot trust.
        confirm(
            Endpoint::MfaConfirm,
            confirmed.enrolled && !confirmed.backup_codes.is_empty(),
        )?;
        Ok(confirmed)
    }

    /// Turns two-step verification off and drops its recovery codes. Spends
    /// the `2fa-change` proof, which stands in for the password, so an account
    /// with no password can turn it off too.
    pub async fn mfa_disable(&self) -> Result<(), ApiError> {
        let answer: MfaDisableResponse = self.request(Endpoint::MfaDisable).send().await?;
        confirm(Endpoint::MfaDisable, !answer.enrolled)
    }

    /// A fresh re-authentication for `action` from `code`, the one-time code
    /// the re-authentication page handed back to `redirect_uri`, with the PKCE
    /// verifier of the attempt that opened the page. Answers how many seconds
    /// the proof stands. A proof the service made for another action is
    /// [`ApiError::Unconfirmed`].
    pub async fn native_reauth_sso(
        &self,
        action: ReauthAction,
        code: &Credential,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<i64, ApiError> {
        let answer: NativeReauthResponse = self
            .request(Endpoint::NativeReauthSso)
            .field("action", action.wire())
            .field("code", code.expose())
            .field("codeVerifier", code_verifier)
            .field("redirectUri", redirect_uri)
            .send()
            .await?;
        proof(Endpoint::NativeReauthSso, action, &answer)
    }

    /// A fresh re-authentication for `action` from the account's `password`,
    /// with `code` (an authenticator or recovery code) once the service has
    /// asked for it. Without a code, an account with two-step verification is
    /// answered `MFA_REQUIRED`, a 401 that is the service asking for the code,
    /// not refusing the session: an [`ApiError::Envelope`] with status 401
    /// ([`CODE_MFA_REQUIRED`](crate::CODE_MFA_REQUIRED)).
    pub async fn native_reauth_password(
        &self,
        action: ReauthAction,
        password: &Credential,
        code: Option<&Credential>,
    ) -> Result<i64, ApiError> {
        let answer: NativeReauthResponse = self
            .request(Endpoint::NativeReauthPassword)
            .field("action", action.wire())
            .field("password", password.expose())
            .optional_field("code", code.map(Credential::expose))
            .send()
            .await?;
        proof(Endpoint::NativeReauthPassword, action, &answer)
    }

    /// Deletes the signed-in account. `confirmation` is its email address as
    /// the person typed it; the service compares it without case before it
    /// spends the `delete-account` proof, so a typo costs nothing
    /// (`confirmation_mismatch`). Answers whether support data a vendor holds
    /// is still being erased; the account itself is gone either way.
    pub async fn delete_account(&self, confirmation: &str) -> Result<bool, ApiError> {
        let answer: AccountDeleteResponse = self
            .request(Endpoint::DeleteAccount)
            .field("confirmation", confirmation)
            .send()
            .await?;
        confirm(
            Endpoint::DeleteAccount,
            answer.success && answer.account_deleted,
        )?;
        Ok(answer.external_erasure_pending)
    }
}

/// The seconds a proof stands, once it is a success for `action`.
fn proof(
    endpoint: Endpoint,
    action: ReauthAction,
    answer: &NativeReauthResponse,
) -> Result<i64, ApiError> {
    confirm(endpoint, answer.success && answer.action == action.wire())?;
    Ok(answer.expires_in_seconds)
}
