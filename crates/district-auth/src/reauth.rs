// SPDX-FileCopyrightText: Distronode Corporation
//
// SPDX-License-Identifier: Apache-2.0

//! A fresh re-authentication at the person's provider, before a sensitive
//! change: deleting the account, or turning two-step verification on or off.
//!
//! The same two legs as signing in, with the same guards:
//!
//! 1. The app opens [`REAUTH_PATH`] in the user's browser
//!    ([`ReauthAttempt::sheet_query`]). The page sends the person to their
//!    provider for a sign-in that takes the credential again, then hands a
//!    one-time code back through `districtai://auth`, with this attempt's
//!    `state`.
//! 2. The app checks the `state` ([`ReauthAttempt::answer`]) and spends the
//!    code with this attempt's PKCE verifier
//!    (`ApiClient::native_reauth_sso`), which earns a short proof for the one
//!    action.
//!
//! The service binds the code four ways: to this account, to this installation
//! (by the SHA-256 of its id, which is all the page is given), to the action,
//! and to the verifier. The app adds the `state` check; a code that fails it is
//! never spent.

use std::fmt;

use district_model::{Credential, ReauthAction};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use url::Url;
use url::form_urlencoded::Serializer;

use crate::login::{LoginError, Param, REDIRECT_URI, is_app_link, single_param};
use crate::pkce::{Pkce, PkceVerifier, new_state};

/// The re-authentication page, below the service's origin. A page, not an API
/// route, like the sign-in page.
pub const REAUTH_PATH: &str = "/auth/native/reauth";

/// The providers a fresh re-authentication can go through. The app does not
/// know which ones an account has linked, so it offers all three and the
/// service decides: a provider the account does not use is refused on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReauthProvider {
    /// Google.
    Google,
    /// Microsoft.
    Microsoft,
    /// Apple.
    Apple,
}

impl ReauthProvider {
    /// Every provider, in the order a screen offers them.
    pub const ALL: [Self; 3] = [Self::Google, Self::Microsoft, Self::Apple];

    /// The provider as the service names it.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "azure-ad",
            Self::Apple => "apple",
        }
    }

    /// The provider's name, a proper noun in every language.
    pub const fn display(self) -> &'static str {
        match self {
            Self::Google => "Google",
            Self::Microsoft => "Microsoft",
            Self::Apple => "Apple",
        }
    }
}

/// One re-authentication at a provider, from opening the page to its answer.
/// Its `Debug` output names the action and the provider only: the `state` and
/// the verifier never reach a log.
#[derive(Clone)]
pub struct ReauthAttempt {
    action: ReauthAction,
    provider: ReauthProvider,
    pkce: Pkce,
    state: String,
}

impl ReauthAttempt {
    /// A new attempt, with a fresh PKCE pair and `state`.
    pub fn new(action: ReauthAction, provider: ReauthProvider) -> Self {
        Self {
            action,
            provider,
            pkce: Pkce::generate(),
            state: new_state(),
        }
    }

    /// What it is for.
    pub fn action(&self) -> ReauthAction {
        self.action
    }

    /// Where the person signs in again.
    pub fn provider(&self) -> ReauthProvider {
        self.provider
    }

    /// The verifier, sent only with the code.
    pub fn verifier(&self) -> &PkceVerifier {
        self.pkce.verifier()
    }

    /// The page's query, for [`REAUTH_PATH`] on the service's origin. The
    /// installation is named by the SHA-256 of `device_id`, in hex, never by
    /// the id itself.
    pub fn sheet_query(&self, device_id: &str) -> String {
        Serializer::new(String::new())
            .append_pair("code_challenge", self.pkce.challenge())
            .append_pair("state", &self.state)
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("provider", self.provider.wire())
            .append_pair("device", &device_hash(device_id))
            .append_pair("action", self.action.wire())
            .finish()
    }

    /// The one-time code in `callback`, once it is this attempt's answer: the
    /// app's `districtai://auth` link with exactly this attempt's `state` and
    /// one non-empty `code`. A link that is not the app's is
    /// [`LoginError::NotOurRedirect`]; one with another `state`,
    /// [`LoginError::StateMismatch`], and its code is never spent.
    pub fn answer(&self, callback: &str) -> Result<Credential, LoginError> {
        let callback = Url::parse(callback).map_err(|_| LoginError::NotOurRedirect)?;
        if !is_app_link(&callback, "auth") {
            return Err(LoginError::NotOurRedirect);
        }
        let Param::One(state) = single_param(&callback, "state") else {
            return Err(LoginError::StateMismatch);
        };
        if !bool::from(state.as_bytes().ct_eq(self.state.as_bytes())) {
            return Err(LoginError::StateMismatch);
        }
        match single_param(&callback, "code") {
            Param::One(code) if !code.is_empty() => Ok(Credential::new(code)),
            Param::One(_) | Param::Absent => Err(LoginError::MissingCode),
            Param::Many => Err(LoginError::MalformedCallback),
        }
    }
}

impl fmt::Debug for ReauthAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReauthAttempt")
            .field("action", &self.action)
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

impl PartialEq for ReauthAttempt {
    /// Two attempts are the same only when their `state` is.
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
    }
}

impl Eq for ReauthAttempt {}

/// The SHA-256 of `device_id`, in lowercase hex.
pub fn device_hash(device_id: &str) -> String {
    Sha256::digest(device_id.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(attempt: &ReauthAttempt, device: &str) -> Vec<(String, String)> {
        url::form_urlencoded::parse(attempt.sheet_query(device).as_bytes())
            .into_owned()
            .collect()
    }

    fn state_of(attempt: &ReauthAttempt) -> String {
        query(attempt, "d")
            .into_iter()
            .find(|(key, _)| key == "state")
            .map(|(_, value)| value)
            .unwrap()
    }

    #[test]
    fn the_providers_have_the_service_names() {
        assert_eq!(
            ReauthProvider::ALL.map(ReauthProvider::wire),
            ["google", "azure-ad", "apple"]
        );
        assert_eq!(
            ReauthProvider::ALL.map(ReauthProvider::display),
            ["Google", "Microsoft", "Apple"]
        );
    }

    #[test]
    fn the_page_is_given_the_device_hash_and_never_the_id() {
        let attempt = ReauthAttempt::new(ReauthAction::DeleteAccount, ReauthProvider::Microsoft);
        let pairs = query(&attempt, "device-123");
        let keys: Vec<&str> = pairs.iter().map(|(key, _)| key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "code_challenge",
                "state",
                "redirect_uri",
                "provider",
                "device",
                "action"
            ]
        );
        let value = |key: &str| pairs.iter().find(|(k, _)| k == key).unwrap().1.clone();
        assert_eq!(
            value("code_challenge"),
            crate::challenge_for(attempt.verifier().as_str())
        );
        assert_eq!(value("redirect_uri"), "districtai://auth");
        assert_eq!(value("provider"), "azure-ad");
        assert_eq!(value("action"), "delete-account");
        assert_eq!(value("device"), device_hash("device-123"));
        assert!(!attempt.sheet_query("device-123").contains("device-123"));
        assert_eq!(attempt.action(), ReauthAction::DeleteAccount);
        assert_eq!(attempt.provider(), ReauthProvider::Microsoft);
    }

    #[test]
    fn the_device_hash_is_sha256_in_hex() {
        assert_eq!(
            device_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn only_this_attempts_answer_gives_its_code() {
        let attempt = ReauthAttempt::new(ReauthAction::TwoStepChange, ReauthProvider::Google);
        let state = state_of(&attempt);
        let link = |query: &str| format!("districtai://auth?{query}");

        let code = attempt
            .answer(&link(&format!("code=c0de&state={state}")))
            .unwrap();
        assert_eq!(code.expose(), "c0de");
        // GLib hands the link over with a slash after the host.
        let code = attempt
            .answer(&format!("districtai://auth/?code=c0de&state={state}"))
            .unwrap();
        assert_eq!(code.expose(), "c0de");

        assert_eq!(
            attempt.answer(&link("code=c0de&state=other")),
            Err(LoginError::StateMismatch)
        );
        assert_eq!(
            attempt.answer(&link("code=c0de")),
            Err(LoginError::StateMismatch)
        );
        assert_eq!(
            attempt.answer(&link(&format!("state={state}"))),
            Err(LoginError::MissingCode)
        );
        assert_eq!(
            attempt.answer(&link(&format!("code=&state={state}"))),
            Err(LoginError::MissingCode)
        );
        assert_eq!(
            attempt.answer(&link(&format!("code=a&code=b&state={state}"))),
            Err(LoginError::MalformedCallback)
        );
        assert_eq!(
            attempt.answer(&format!("districtai://handoff?code=c0de&state={state}")),
            Err(LoginError::NotOurRedirect)
        );
        assert_eq!(
            attempt.answer("not a link"),
            Err(LoginError::NotOurRedirect)
        );
    }

    #[test]
    fn an_attempt_keeps_its_secrets_out_of_debug_output() {
        let attempt = ReauthAttempt::new(ReauthAction::TwoStepChange, ReauthProvider::Apple);
        let shown = format!("{attempt:?}");
        assert_eq!(
            shown,
            "ReauthAttempt { action: TwoStepChange, provider: Apple, .. }"
        );
        assert!(!shown.contains(&state_of(&attempt)));
    }

    #[test]
    fn attempts_are_equal_only_to_themselves() {
        let attempt = ReauthAttempt::new(ReauthAction::DeleteAccount, ReauthProvider::Google);
        assert_eq!(attempt, attempt.clone());
        assert_ne!(
            attempt,
            ReauthAttempt::new(ReauthAction::DeleteAccount, ReauthProvider::Google)
        );
    }
}
