//! The account screen: who is signed in on this device, and what the account
//! allows here (sign out, see the devices, two-step verification, delete the
//! account).
//!
//! It holds no state of its own. Signing out is a session change
//! ([`SessionState::SigningOut`](crate::SessionState::SigningOut)), its outcome
//! is shown on the signed-out screen, and two-step verification and the
//! deletion are screens of their own ([`Route::TwoStep`](crate::Route::TwoStep),
//! [`Route::DeleteAccount`](crate::Route::DeleteAccount)).

/// The web page that explains how to delete an account, below the service's
/// origin. The app deletes the account on its own screen; this address stays
/// the data-deletion address the store listings name.
pub const ACCOUNT_DELETION_PATH: &str = "/privacy/account-deletion";

/// What the account screen shows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AccountView {
    /// This build's version.
    pub app_version: String,
    /// The installation id this session was issued to.
    pub device_id: String,
    /// The signed-in user's id.
    pub user_id: String,
}

impl AccountView {
    /// The sign-out row's caption.
    pub const SIGN_OUT_CAPTION: &'static str = district_l10n::english::ACCOUNT_SIGN_OUT_CAPTION;
    /// The devices row's caption.
    pub const DEVICES_CAPTION: &'static str = district_l10n::english::ACCOUNT_DEVICES_CAPTION;
    /// The deletion row's caption. It says the person confirms it is them
    /// first, because deletion cannot be undone.
    pub const DELETE_ACCOUNT_CAPTION: &'static str =
        district_l10n::english::ACCOUNT_DELETE_ACCOUNT_CAPTION;
    /// The two-step verification row's caption.
    pub const TWO_STEP_CAPTION: &'static str = district_l10n::english::ACCOUNT_TWO_STEP_CAPTION;
}
