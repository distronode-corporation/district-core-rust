//! The account's own security: the two-step verification screen
//! ([`TwoStepScreen`]) and the account's deletion ([`DeleteAccountScreen`]),
//! each behind a fresh re-authentication ([`ReauthStep`]).
//!
//! The service enforces every step. A fresh re-authentication (a fresh sign-in
//! at the person's provider in the browser, or their password and, with
//! two-step verification on, its code) earns a short proof bound to this
//! installation and one action ([`ReauthAction`]); the change then spends it.
//! Nothing here decides who may do what: a refusal is the service's answer,
//! shown in a sentence that says what to do.
//!
//! Every change is single-flight: while one is on its way (`busy`), the
//! screen's events are ignored, so two clicks dispatched before a re-render
//! send one request.
//!
//! The authenticator secret and the recovery codes are shown once and dropped
//! when the screen is left; what the person types arrives in a [`Credential`]
//! and is sent, never kept. None of them reaches a `Debug` output.

use std::fmt;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use district_api::{ApiError, CODE_MFA_REQUIRED, TransportKind};
use district_auth::{LoginError, REAUTH_PATH, ReauthAttempt, ReauthProvider};
use district_l10n::api_error_text;
use district_l10n::format::{self, CivilDateTime, DateStyle};
use district_model::{Credential, MfaConfirmation, MfaEnrollment, MfaStatus, ReauthAction};
use jiff::Timestamp;

use crate::failure::FailureText;
use crate::model::{CoreConfig, Effect, Slot, Ticket, Tickets};
use crate::scheduling::OneTimeUrl;
use crate::session::SignOutScope;
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::{l, s};

/// How a screen is confirming it is the account's owner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReauthStep {
    /// Choosing a way: a provider ([`ReauthProvider::ALL`]) or the password.
    #[default]
    Choose,
    /// The browser is open at the provider's sign-in; its answer comes back
    /// through the app's own link.
    Browser(ReauthProvider),
    /// The password, and once the service has asked for it
    /// (`needs_code`), the authenticator or recovery code.
    Password {
        /// Whether the account has two-step verification and the code goes
        /// with the password.
        needs_code: bool,
    },
}

/// What a fresh re-authentication sends: the one-time code the browser handed
/// back with the attempt's PKCE verifier, or the password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReauthProof {
    /// A fresh sign-in at a provider.
    Provider {
        /// The one-time code from the re-authentication page.
        code: Credential,
        /// The PKCE verifier of the attempt that opened the page.
        verifier: Credential,
    },
    /// The account's password.
    Password {
        /// The password.
        password: Credential,
        /// The authenticator or recovery code, once the service asked for it.
        code: Option<Credential>,
    },
}

// ── Two-step verification ──────────────────────────────────────────────────

/// Which change a two-step screen is making.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TwoStepChange {
    /// Turning it on.
    TurnOn,
    /// Turning it off.
    TurnOff,
}

/// Where the two-step screen is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TwoStepStep {
    /// The status is being read for the first time.
    #[default]
    Loading,
    /// The first read failed: the failure, and a way to try again
    /// ([`TwoStepEvent::Retry`]).
    LoadFailed,
    /// The status, and the change it offers.
    Overview,
    /// Confirming it is you before `change`.
    Reauth {
        /// The change it is for.
        change: TwoStepChange,
        /// How far it has got.
        step: ReauthStep,
    },
    /// The new secret, to add to an authenticator app, and the field for its
    /// first code.
    Scan(TwoStepSecret),
    /// It is on: the recovery codes, shown this once.
    RecoveryCodes(RecoveryCodes),
}

/// A new authenticator secret, as the scan step shows it. Its `Debug` output
/// names none of it.
#[derive(Clone, PartialEq, Eq)]
pub struct TwoStepSecret {
    /// The base32 key, for typing into an authenticator by hand.
    pub key: String,
    /// The `otpauth://` link an authenticator app opens.
    pub otpauth_uri: String,
    /// The QR code of the link, as PNG bytes, or `None` when the service's
    /// picture could not be read (the key still works).
    pub qr_png: Option<Vec<u8>>,
}

impl TwoStepSecret {
    fn from_enrollment(enrollment: MfaEnrollment) -> Self {
        let qr_png = enrollment
            .qr_data_url
            .strip_prefix("data:image/png;base64,")
            .and_then(|data| STANDARD.decode(data).ok());
        Self {
            key: enrollment.secret,
            otpauth_uri: enrollment.otpauth_uri,
            qr_png,
        }
    }
}

impl fmt::Debug for TwoStepSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TwoStepSecret(<redacted>)")
    }
}

/// The recovery codes, shown once. Its `Debug` output gives their count only.
#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryCodes(pub Vec<String>);

impl RecoveryCodes {
    /// Every code, one per line, for "Copy all".
    pub fn joined(&self) -> String {
        self.0.join("\n")
    }
}

impl fmt::Debug for RecoveryCodes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RecoveryCodes(<{} redacted>)", self.0.len())
    }
}

/// What a finished change says.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TwoStepNotice {
    /// It was turned on.
    TurnedOn,
    /// It was turned off.
    TurnedOff,
}

impl TwoStepNotice {
    /// The sentence.
    pub fn message(self) -> &'static str {
        match self {
            Self::TurnedOn => s::two_step_turned_on(l()),
            Self::TurnedOff => s::two_step_turned_off(l()),
        }
    }
}

/// The two-step verification screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TwoStepScreen {
    /// Where it is.
    pub step: TwoStepStep,
    /// The status last read, or written by a change the service confirmed.
    pub status: Option<MfaStatus>,
    /// Whether a read or a change is on its way. One at a time.
    pub busy: bool,
    /// What went wrong last, as a sentence that says what to do.
    pub failure: Option<String>,
    /// What the last finished change did.
    pub notice: Option<TwoStepNotice>,
}

impl TwoStepScreen {
    /// "On" or "Off", once the status is known.
    pub fn status_label(&self) -> Option<&'static str> {
        self.status.as_ref().map(|status| {
            if status.enrolled {
                s::two_step_status_on(l())
            } else {
                s::two_step_status_off(l())
            }
        })
    }

    /// What the status says under its label: since when it is on and how many
    /// recovery codes are left, or what it does while it is off.
    pub fn status_lines(&self) -> Vec<String> {
        let Some(status) = &self.status else {
            return Vec::new();
        };
        if !status.enrolled {
            return vec![s::two_step_off_explain(l()).to_owned()];
        }
        let mut lines = Vec::new();
        if let Some(day) = status.enrolled_at.as_deref().and_then(day_of) {
            lines.push(s::two_step_on_since(l(), &day));
        }
        lines.push(match status.backup_codes_remaining {
            0 => s::two_step_codes_none(l()).to_owned(),
            1 => s::two_step_codes_left_one(l()).to_owned(),
            count => s::two_step_codes_left_many(l(), count),
        });
        if self.signs_in_without_password() {
            lines.push(s::two_step_no_password_note(l()).to_owned());
        }
        lines
    }

    /// The change the overview offers: on when it is off, off when it is on.
    pub fn offered(&self) -> Option<TwoStepChange> {
        self.status.as_ref().map(|status| {
            if status.enrolled {
                TwoStepChange::TurnOff
            } else {
                TwoStepChange::TurnOn
            }
        })
    }

    /// Whether the account has no password, so turning it off confirms it is
    /// you at a provider: the website cannot do that, an app can.
    pub fn signs_in_without_password(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(|status| status.enrolled && !status.can_disable)
    }

    /// The sentence over the ways to confirm it is you, for `change`.
    pub fn reauth_intro(change: TwoStepChange) -> &'static str {
        match change {
            TwoStepChange::TurnOn => s::two_step_reauth_intro_on(l()),
            TwoStepChange::TurnOff => s::two_step_reauth_intro_off(l()),
        }
    }
}

/// The day an ISO 8601 instant falls on, as the app writes dates.
fn day_of(when: &str) -> Option<String> {
    let instant = when.parse::<Timestamp>().ok()?;
    let day = CivilDateTime::from_unix(instant.as_second(), 0);
    Some(format::date(l(), day, DateStyle::Medium))
}

/// Something on the two-step screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TwoStepEvent {
    /// Read the status again after a first read that failed.
    Retry,
    /// Start a change: the one [`TwoStepScreen::offered`] names.
    Begin(TwoStepChange),
    /// Confirm it is you by signing in again at `provider`, in the browser.
    UseProvider(ReauthProvider),
    /// Confirm it is you with the password instead.
    UsePassword,
    /// Send the password, and the code once asked for.
    SubmitPassword {
        /// The password.
        password: Credential,
        /// The authenticator or recovery code; sent only once the service
        /// asked for it.
        code: Option<Credential>,
    },
    /// Turn it on with the authenticator's first code.
    Confirm(Credential),
    /// The recovery codes are saved: drop them and show the status.
    FinishRecoveryCodes,
    /// One step back inside the screen: from the password to the ways to
    /// confirm, from those or the secret to the status.
    Back,
    /// Dismiss the notice and the failure.
    DismissNotices,
}

/// Which change a refusal came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TwoStepWrite {
    Enroll,
    Confirm,
    Disable,
}

/// What a refusal leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterRefusal {
    /// Stay where the screen is.
    Stay,
    /// The proof is gone: confirm it is you again.
    Reauth,
    /// The change may or may not have happened: read the status.
    Reread,
}

// ── Deleting the account ───────────────────────────────────────────────────

/// Where the account deletion screen is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeleteAccountStep {
    /// What is deleted and kept, and confirming it is you.
    Explain(ReauthStep),
    /// The proof stands: the typed email address and the final button.
    Confirm,
}

impl Default for DeleteAccountStep {
    fn default() -> Self {
        Self::Explain(ReauthStep::Choose)
    }
}

/// A refusal on the deletion screen: a headline when nothing was deleted, and
/// the sentence that says why or what to do.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeleteFailure {
    /// "Your account was not deleted.", when that is certain.
    pub title: Option<&'static str>,
    /// What went wrong, and what to do.
    pub message: String,
}

impl DeleteFailure {
    fn plain(message: impl Into<String>) -> Self {
        Self {
            title: None,
            message: message.into(),
        }
    }

    fn not_deleted(message: impl Into<String>) -> Self {
        Self {
            title: Some(s::delete_account_not_deleted(l())),
            message: message.into(),
        }
    }
}

/// The account deletion screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeleteAccountScreen {
    /// Where it is.
    pub step: DeleteAccountStep,
    /// Whether a re-authentication or the deletion is on its way. One at a
    /// time.
    pub busy: bool,
    /// What went wrong last.
    pub failure: Option<DeleteFailure>,
}

/// Something on the account deletion screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteAccountEvent {
    /// Confirm it is you by signing in again at `provider`, in the browser.
    UseProvider(ReauthProvider),
    /// Confirm it is you with the password instead.
    UsePassword,
    /// Send the password, and the code once asked for.
    SubmitPassword {
        /// The password.
        password: Credential,
        /// The authenticator or recovery code; sent only once the service
        /// asked for it.
        code: Option<Credential>,
    },
    /// Delete the account. `confirmation` is its email address, as typed.
    Delete {
        /// The email address.
        confirmation: String,
    },
    /// From the password, or the browser's sign-in, back to the ways to
    /// confirm it is you.
    Back,
}

// ── The signed-in state's part ─────────────────────────────────────────────

impl SignedIn {
    /// Opens the two-step screen afresh and reads the status.
    pub(crate) fn enter_two_step(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.two_step = TwoStepScreen::default();
        self.load_two_step(tickets)
    }

    /// Opens the deletion screen afresh. A deletion still on its way keeps it
    /// busy, so its answer lands on the new visit.
    pub(crate) fn enter_delete_account(&mut self) {
        self.delete_account = DeleteAccountScreen {
            busy: self.delete_account.busy,
            ..DeleteAccountScreen::default()
        };
    }

    /// Leaves either screen: the secret, the codes and a re-authentication
    /// under way are dropped. A deletion already sent is still awaited, so its
    /// answer is never lost.
    pub(crate) fn close_account_security(&mut self, tickets: &mut Tickets) {
        tickets.cancel(Slot::TwoStep);
        tickets.cancel(Slot::TwoStepWrite);
        tickets.cancel(Slot::Reauth);
        self.reauth = None;
        self.two_step = TwoStepScreen::default();
        self.enter_delete_account();
    }

    /// The two-step screen's refresh: a read, unless something is on its way.
    pub(crate) fn refresh_two_step(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if self.two_step.busy {
            return Vec::new();
        }
        self.load_two_step(tickets)
    }

    pub(crate) fn two_step_event(
        &mut self,
        event: TwoStepEvent,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Next {
        let screen = &mut self.two_step;
        // Dismissing sends nothing, so it is never in the way of a change.
        if event == TwoStepEvent::DismissNotices {
            screen.failure = None;
            screen.notice = None;
            return stay();
        }
        if screen.busy {
            return stay();
        }
        match (event, screen.step.clone()) {
            (TwoStepEvent::Retry, TwoStepStep::LoadFailed) => {
                screen.step = TwoStepStep::Loading;
                screen.failure = None;
                return Next::Stay(self.load_two_step(tickets));
            }
            (TwoStepEvent::Begin(change), TwoStepStep::Overview)
                if screen.offered() == Some(change) =>
            {
                screen.failure = None;
                screen.notice = None;
                screen.step = TwoStepStep::Reauth {
                    change,
                    step: ReauthStep::Choose,
                };
            }
            (
                TwoStepEvent::UseProvider(provider),
                TwoStepStep::Reauth {
                    change,
                    step: ReauthStep::Choose | ReauthStep::Browser(_),
                },
            ) => {
                screen.failure = None;
                screen.step = TwoStepStep::Reauth {
                    change,
                    step: ReauthStep::Browser(provider),
                };
                return Next::Stay(self.open_reauth(ReauthAction::TwoStepChange, provider, config));
            }
            (
                TwoStepEvent::UsePassword,
                TwoStepStep::Reauth {
                    change,
                    step: ReauthStep::Choose | ReauthStep::Browser(_),
                },
            ) => {
                self.reauth = None;
                screen.failure = None;
                screen.step = TwoStepStep::Reauth {
                    change,
                    step: ReauthStep::Password { needs_code: false },
                };
            }
            (
                TwoStepEvent::SubmitPassword { password, code },
                TwoStepStep::Reauth {
                    step: ReauthStep::Password { needs_code },
                    ..
                },
            ) => {
                let Some(proof) = password_proof(password, code, needs_code) else {
                    return stay();
                };
                screen.busy = true;
                screen.failure = None;
                return Next::Stay(vec![reauthenticate(
                    ReauthAction::TwoStepChange,
                    proof,
                    tickets,
                )]);
            }
            (TwoStepEvent::Confirm(code), TwoStepStep::Scan(_)) => {
                let code = code.expose().trim();
                if code.is_empty() {
                    return stay();
                }
                screen.busy = true;
                screen.failure = None;
                return Next::Stay(vec![Effect::ConfirmTwoStep {
                    ticket: tickets.issue(Slot::TwoStepWrite),
                    code: Credential::new(code),
                }]);
            }
            (TwoStepEvent::FinishRecoveryCodes, TwoStepStep::RecoveryCodes(_)) => {
                screen.step = TwoStepStep::Overview;
                screen.notice = Some(TwoStepNotice::TurnedOn);
                return Next::Stay(self.load_two_step(tickets));
            }
            (TwoStepEvent::Back, TwoStepStep::Reauth { change, step }) => {
                screen.failure = None;
                screen.step = match step {
                    ReauthStep::Password { .. } => TwoStepStep::Reauth {
                        change,
                        step: ReauthStep::Choose,
                    },
                    ReauthStep::Choose | ReauthStep::Browser(_) => TwoStepStep::Overview,
                };
                self.reauth = None;
            }
            (TwoStepEvent::Back, TwoStepStep::Scan(_)) => {
                screen.failure = None;
                screen.step = TwoStepStep::Overview;
            }
            // Anything else is a stale click on a step that has moved on.
            _ => {}
        }
        stay()
    }

    pub(crate) fn delete_account_event(
        &mut self,
        event: DeleteAccountEvent,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Next {
        if self.delete_account.busy {
            return stay();
        }
        let screen = &mut self.delete_account;
        match (event, screen.step) {
            (
                DeleteAccountEvent::UseProvider(provider),
                DeleteAccountStep::Explain(ReauthStep::Choose | ReauthStep::Browser(_)),
            ) => {
                screen.failure = None;
                screen.step = DeleteAccountStep::Explain(ReauthStep::Browser(provider));
                return Next::Stay(self.open_reauth(ReauthAction::DeleteAccount, provider, config));
            }
            (
                DeleteAccountEvent::UsePassword,
                DeleteAccountStep::Explain(ReauthStep::Choose | ReauthStep::Browser(_)),
            ) => {
                self.reauth = None;
                screen.failure = None;
                screen.step =
                    DeleteAccountStep::Explain(ReauthStep::Password { needs_code: false });
            }
            (
                DeleteAccountEvent::SubmitPassword { password, code },
                DeleteAccountStep::Explain(ReauthStep::Password { needs_code }),
            ) => {
                let Some(proof) = password_proof(password, code, needs_code) else {
                    return stay();
                };
                screen.busy = true;
                screen.failure = None;
                return Next::Stay(vec![reauthenticate(
                    ReauthAction::DeleteAccount,
                    proof,
                    tickets,
                )]);
            }
            (DeleteAccountEvent::Delete { confirmation }, DeleteAccountStep::Confirm) => {
                let confirmation = confirmation.trim();
                if confirmation.is_empty() {
                    return stay();
                }
                screen.busy = true;
                screen.failure = None;
                return Next::Stay(vec![Effect::DeleteAccount {
                    ticket: tickets.issue(Slot::AccountDelete),
                    confirmation: confirmation.to_owned(),
                }]);
            }
            (
                DeleteAccountEvent::Back,
                DeleteAccountStep::Explain(ReauthStep::Password { .. } | ReauthStep::Browser(_)),
            ) => {
                self.reauth = None;
                screen.failure = None;
                screen.step = DeleteAccountStep::Explain(ReauthStep::Choose);
            }
            _ => {}
        }
        stay()
    }

    /// A `districtai://auth` link while signed in: the answer to the
    /// re-authentication under way, if it is one. Anything else is ignored, as
    /// a stale sign-in link always was.
    pub(crate) fn reauth_callback(&mut self, callback: &str, tickets: &mut Tickets) -> Next {
        let Some(attempt) = self.reauth.take() else {
            return stay();
        };
        // An attempt is held only while its screen waits for the browser:
        // every step away from it drops the attempt.
        let action = attempt.action();
        match attempt.answer(callback) {
            Ok(code) => {
                let proof = ReauthProof::Provider {
                    code,
                    verifier: Credential::new(attempt.verifier().as_str()),
                };
                self.set_reauth_busy(action);
                Next::Stay(vec![reauthenticate(action, proof, tickets)])
            }
            // Not the app's sign-in link at all: the attempt is still waiting.
            Err(LoginError::NotOurRedirect) => {
                self.reauth = Some(attempt);
                stay()
            }
            // Another attempt's answer, or none: never spent.
            Err(_) => {
                self.reauth_refused(action, s::reauth_error_sign_in_not_verified(l()).to_owned());
                stay()
            }
        }
    }

    /// No browser opened the re-authentication page.
    pub(crate) fn reauth_unopened(&mut self) {
        if let Some(attempt) = self.reauth.take() {
            self.reauth_refused(
                attempt.action(),
                s::reauth_error_browser_did_not_open(l()).to_owned(),
            );
        }
    }

    pub(crate) fn reauthenticated(
        &mut self,
        ticket: Ticket,
        action: ReauthAction,
        result: Result<i64, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::Reauth, ticket) {
            return stay();
        }
        match (action, result) {
            (ReauthAction::TwoStepChange, Ok(_)) => self.change_two_step(tickets),
            (ReauthAction::DeleteAccount, Ok(_)) => {
                self.delete_account.busy = false;
                self.delete_account.step = DeleteAccountStep::Confirm;
                stay()
            }
            (action, Err(error)) => {
                self.reauth_failed(action, &error);
                stay()
            }
        }
    }

    pub(crate) fn two_step_loaded(
        &mut self,
        ticket: Ticket,
        result: Result<MfaStatus, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::TwoStep, ticket) {
            return stay();
        }
        let screen = &mut self.two_step;
        screen.busy = false;
        match result {
            Ok(status) => {
                screen.status = Some(status);
                screen.step = TwoStepStep::Overview;
            }
            // A status already shown stays under the failure; only a first
            // read fails to the step that offers a retry.
            Err(error) => {
                screen.failure = Some(load_failure(&error));
                screen.step = if screen.status.is_some() {
                    TwoStepStep::Overview
                } else {
                    TwoStepStep::LoadFailed
                };
            }
        }
        stay()
    }

    pub(crate) fn two_step_started(
        &mut self,
        ticket: Ticket,
        result: Result<MfaEnrollment, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::TwoStepWrite, ticket) {
            return stay();
        }
        self.two_step.busy = false;
        match result {
            Ok(enrollment) => {
                self.two_step.step = TwoStepStep::Scan(TwoStepSecret::from_enrollment(enrollment));
                stay()
            }
            Err(error) => self.two_step_refused(&error, TwoStepWrite::Enroll, tickets),
        }
    }

    pub(crate) fn two_step_confirmed(
        &mut self,
        ticket: Ticket,
        result: Result<MfaConfirmation, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::TwoStepWrite, ticket) {
            return stay();
        }
        self.two_step.busy = false;
        match result {
            Ok(confirmed) => {
                let screen = &mut self.two_step;
                // On now, whatever a later read says: a read that fails after
                // this must not show it off.
                screen.status = Some(MfaStatus {
                    enrolled: true,
                    enrolled_at: Some(Timestamp::now().to_string()),
                    backup_codes_remaining: i64::try_from(confirmed.backup_codes.len())
                        .unwrap_or(i64::MAX),
                    can_disable: screen.status.as_ref().is_none_or(|s| s.can_disable),
                });
                screen.step = TwoStepStep::RecoveryCodes(RecoveryCodes(confirmed.backup_codes));
                stay()
            }
            Err(error) if error.code() == Some("mfa_code_invalid") => {
                self.two_step.failure = Some(service_sentence(&error));
                stay()
            }
            Err(error) => self.two_step_refused(&error, TwoStepWrite::Confirm, tickets),
        }
    }

    pub(crate) fn two_step_turned_off(
        &mut self,
        ticket: Ticket,
        result: Result<(), ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::TwoStepWrite, ticket) {
            return stay();
        }
        self.two_step.busy = false;
        match result {
            Ok(()) => {
                let screen = &mut self.two_step;
                // Off now, whatever a later read says, as after a confirm.
                screen.status = Some(MfaStatus {
                    enrolled: false,
                    enrolled_at: None,
                    backup_codes_remaining: 0,
                    can_disable: screen.status.as_ref().is_none_or(|s| s.can_disable),
                });
                screen.step = TwoStepStep::Overview;
                screen.notice = Some(TwoStepNotice::TurnedOff);
                Next::Stay(self.load_two_step(tickets))
            }
            Err(error) => self.two_step_refused(&error, TwoStepWrite::Disable, tickets),
        }
    }

    pub(crate) fn account_deleted(
        &mut self,
        ticket: Ticket,
        result: Result<bool, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::AccountDelete, ticket) {
            return stay();
        }
        self.delete_account.busy = false;
        match result {
            // The account is gone, and with it this session: sign out here,
            // which forgets the session, the workspace and the rest.
            Ok(erasure_pending) => Next::SignOut(SignOutScope::AccountDeleted { erasure_pending }),
            Err(error) if error.code() == Some("reauth_required") => {
                // The proof is gone (it ran out, or was spent): back to the
                // start, said plainly.
                self.delete_account.step = DeleteAccountStep::Explain(ReauthStep::Choose);
                self.delete_account.failure = Some(DeleteFailure::plain(
                    s::delete_account_error_proof_expired(l()),
                ));
                stay()
            }
            Err(error) => {
                self.delete_account.failure = Some(delete_failure(&error));
                stay()
            }
        }
    }

    // ── Pieces ─────────────────────────────────────────────────────────────

    fn load_two_step(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.two_step.busy = true;
        vec![Effect::LoadTwoStep {
            ticket: tickets.issue(Slot::TwoStep),
        }]
    }

    /// A new attempt at `provider`, and the page that starts it.
    fn open_reauth(
        &mut self,
        action: ReauthAction,
        provider: ReauthProvider,
        config: &CoreConfig,
    ) -> Vec<Effect> {
        let attempt = ReauthAttempt::new(action, provider);
        let url = format!(
            "{}?{}",
            config.web_url(REAUTH_PATH),
            attempt.sheet_query(&self.identity.device_id)
        );
        // A newer attempt replaces one still waiting, whose answer is then
        // refused as another attempt's.
        self.reauth = Some(attempt);
        vec![Effect::OpenOneTimeUrl {
            url: OneTimeUrl::new(url),
        }]
    }

    fn set_reauth_busy(&mut self, action: ReauthAction) {
        match action {
            ReauthAction::TwoStepChange => self.two_step.busy = true,
            ReauthAction::DeleteAccount => self.delete_account.busy = true,
        }
    }

    /// The re-authentication for `action` did not stand: `message`, and back
    /// to the ways to confirm when the browser was the way.
    fn reauth_refused(&mut self, action: ReauthAction, message: String) {
        match action {
            ReauthAction::TwoStepChange => {
                let screen = &mut self.two_step;
                screen.busy = false;
                if let TwoStepStep::Reauth { change, step } = screen.step.clone() {
                    screen.step = TwoStepStep::Reauth {
                        change,
                        step: after_refusal(step),
                    };
                }
                screen.failure = Some(message);
            }
            ReauthAction::DeleteAccount => {
                let screen = &mut self.delete_account;
                screen.busy = false;
                if let DeleteAccountStep::Explain(step) = screen.step {
                    screen.step = DeleteAccountStep::Explain(after_refusal(step));
                }
                screen.failure = Some(DeleteFailure::plain(message));
            }
        }
    }

    fn reauth_failed(&mut self, action: ReauthAction, error: &ApiError) {
        match error.code() {
            // Not a failure: the account has two-step verification, and its
            // code goes with the password on the next try.
            Some(CODE_MFA_REQUIRED) => {
                self.set_reauth_step(action, ReauthStep::Password { needs_code: true });
            }
            Some("NO_PASSWORD_SET") => {
                self.reauth_refused(action, s::reauth_error_no_password(l()).to_owned());
                self.set_reauth_step(action, ReauthStep::Choose);
            }
            _ => {
                let message = reauth_failure(error, self.reauth_in_browser(action));
                self.reauth_refused(action, message);
            }
        }
    }

    /// Whether the re-authentication for `action` went through the browser.
    fn reauth_in_browser(&self, action: ReauthAction) -> bool {
        match action {
            ReauthAction::TwoStepChange => matches!(
                self.two_step.step,
                TwoStepStep::Reauth {
                    step: ReauthStep::Browser(_),
                    ..
                }
            ),
            ReauthAction::DeleteAccount => matches!(
                self.delete_account.step,
                DeleteAccountStep::Explain(ReauthStep::Browser(_))
            ),
        }
    }

    fn set_reauth_step(&mut self, action: ReauthAction, step: ReauthStep) {
        match action {
            ReauthAction::TwoStepChange => {
                self.two_step.busy = false;
                if let TwoStepStep::Reauth { change, .. } = self.two_step.step {
                    self.two_step.step = TwoStepStep::Reauth { change, step };
                }
            }
            ReauthAction::DeleteAccount => {
                self.delete_account.busy = false;
                self.delete_account.step = DeleteAccountStep::Explain(step);
            }
        }
    }

    /// The proof stands: the change it was for. The screen is still on its
    /// re-authentication: while it was busy nothing could move it.
    fn change_two_step(&mut self, tickets: &mut Tickets) -> Next {
        let ticket = tickets.issue(Slot::TwoStepWrite);
        let turn_on = matches!(
            self.two_step.step,
            TwoStepStep::Reauth {
                change: TwoStepChange::TurnOn,
                ..
            }
        );
        Next::Stay(vec![if turn_on {
            Effect::StartTwoStep { ticket }
        } else {
            Effect::TurnOffTwoStep { ticket }
        }])
    }

    fn two_step_refused(
        &mut self,
        error: &ApiError,
        write: TwoStepWrite,
        tickets: &mut Tickets,
    ) -> Next {
        let (message, after) = two_step_refusal(error, write);
        let change = match write {
            TwoStepWrite::Disable => TwoStepChange::TurnOff,
            TwoStepWrite::Enroll | TwoStepWrite::Confirm => TwoStepChange::TurnOn,
        };
        let screen = &mut self.two_step;
        screen.failure = Some(message);
        match after {
            AfterRefusal::Stay => {
                if let TwoStepStep::Reauth { change, step } = screen.step.clone() {
                    screen.step = TwoStepStep::Reauth {
                        change,
                        step: after_refusal(step),
                    };
                }
                stay()
            }
            // A new proof means a new secret for turning on: the old one is
            // dropped.
            AfterRefusal::Reauth => {
                screen.step = TwoStepStep::Reauth {
                    change,
                    step: ReauthStep::Choose,
                };
                stay()
            }
            AfterRefusal::Reread => {
                screen.step = TwoStepStep::Overview;
                Next::Stay(self.load_two_step(tickets))
            }
        }
    }
}

/// The step a refused re-authentication returns to: the ways to confirm when
/// the browser was the way, the same step otherwise.
fn after_refusal(step: ReauthStep) -> ReauthStep {
    match step {
        ReauthStep::Browser(_) => ReauthStep::Choose,
        step => step,
    }
}

/// The password proof, when there is something to send: a password, and a
/// code when one is needed. The code goes only once the service asked for it.
fn password_proof(
    password: Credential,
    code: Option<Credential>,
    needs_code: bool,
) -> Option<ReauthProof> {
    if password.expose().is_empty() {
        return None;
    }
    let code = if needs_code {
        let code = code?;
        let trimmed = code.expose().trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(Credential::new(trimmed))
    } else {
        None
    };
    Some(ReauthProof::Password { password, code })
}

fn reauthenticate(action: ReauthAction, proof: ReauthProof, tickets: &mut Tickets) -> Effect {
    Effect::Reauthenticate {
        ticket: tickets.issue(Slot::Reauth),
        action,
        proof,
    }
}

/// The service's sentence for the error's code, in the app's language, or the
/// general one.
fn service_sentence(error: &ApiError) -> String {
    error
        .code()
        .and_then(|code| api_error_text::<&str>(l(), code, &[]))
        .unwrap_or_else(|| FailureText::from_api_error(error).message)
}

fn is_offline(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::Offline(transport) if !matches!(transport.kind, TransportKind::NotJson { .. })
    )
}

fn is_rate_limited(error: &ApiError) -> bool {
    matches!(error, ApiError::RateLimited { .. }) || error.code() == Some("rate_limited")
}

fn is_server_side(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::Server { .. } | ApiError::Envelope { status: 500.., .. }
    ) || error.code() == Some("server_error")
}

fn is_unreadable(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::Decode { .. } | ApiError::Unconfirmed { .. }
    )
}

/// Why a re-authentication did not stand. Branched on the code, never on the
/// message; a code this build does not know shows the service's sentence.
fn reauth_failure(error: &ApiError, provider: bool) -> String {
    if is_offline(error) {
        return s::reauth_error_offline_not_confirmed(l()).to_owned();
    }
    if is_rate_limited(error) {
        return s::reauth_error_too_many_attempts(l()).to_owned();
    }
    if is_unreadable(error) || is_server_side(error) {
        return s::reauth_error_not_confirmed(l()).to_owned();
    }
    match error.code() {
        Some("incorrect_password") => s::reauth_error_wrong_password(l()).to_owned(),
        Some("mfa_code_invalid") => s::reauth_error_wrong_code(l()).to_owned(),
        Some("reauth_failed") if provider => s::reauth_error_wrong_account(l()).to_owned(),
        Some("reauth_failed") => s::reauth_error_not_confirmed(l()).to_owned(),
        _ => FailureText::from_api_error(error).message,
    }
}

/// What a refused two-step change says, and what it leads to.
fn two_step_refusal(error: &ApiError, write: TwoStepWrite) -> (String, AfterRefusal) {
    // A confirm or a disable lost in transport, or answered in a way this
    // build cannot read, may have happened: never claim either way.
    if is_offline(error) || is_unreadable(error) {
        let message = match write {
            TwoStepWrite::Enroll if is_offline(error) => {
                return (
                    s::two_step_error_offline_nothing_changed(l()).to_owned(),
                    AfterRefusal::Stay,
                );
            }
            TwoStepWrite::Enroll => {
                return (
                    s::two_step_error_try_again(l()).to_owned(),
                    AfterRefusal::Stay,
                );
            }
            TwoStepWrite::Confirm => s::two_step_error_outcome_unknown_on(l()),
            TwoStepWrite::Disable => s::two_step_error_outcome_unknown_off(l()),
        };
        return (message.to_owned(), AfterRefusal::Reread);
    }
    match error.code() {
        Some("reauth_required") => (
            s::two_step_error_proof_expired(l()).to_owned(),
            AfterRefusal::Reauth,
        ),
        Some("MFA_ALREADY_ENABLED" | "MFA_NOT_STARTED" | "MFA_NOT_PENDING") => {
            (service_sentence(error), AfterRefusal::Reread)
        }
        _ if is_server_side(error) => (
            s::two_step_error_try_again(l()).to_owned(),
            AfterRefusal::Stay,
        ),
        _ if matches!(error, ApiError::Unauthorized(_)) => (
            FailureText::from_api_error(error).message,
            AfterRefusal::Stay,
        ),
        _ => (service_sentence(error), AfterRefusal::Stay),
    }
}

/// Why the status could not be read.
fn load_failure(error: &ApiError) -> String {
    if is_offline(error) {
        return s::two_step_error_offline_load(l()).to_owned();
    }
    if is_unreadable(error) || is_server_side(error) {
        return s::two_step_error_load(l()).to_owned();
    }
    FailureText::from_api_error(error).message
}

/// Why the account was not deleted, with a headline only when that is certain.
fn delete_failure(error: &ApiError) -> DeleteFailure {
    // A dropped connection can come after the service deleted the account, and
    // an answer this build cannot read may be a deletion: never claim either
    // way.
    if is_offline(error) {
        return DeleteFailure::plain(s::delete_account_error_offline_outcome_unknown(l()));
    }
    if is_unreadable(error) {
        return DeleteFailure::plain(s::delete_account_error_outcome_unknown(l()));
    }
    // No headline either: a retry after a deletion lost in transport is
    // answered 401 when the first one did delete the account.
    if matches!(error, ApiError::Unauthorized(_)) {
        return DeleteFailure::plain(FailureText::from_api_error(error).message);
    }
    if error.code() == Some("confirmation_mismatch") {
        return DeleteFailure::plain(s::delete_account_error_email_mismatch(l()));
    }
    if is_rate_limited(error) {
        return DeleteFailure::not_deleted(s::reauth_error_too_many_attempts(l()));
    }
    if is_server_side(error) {
        return DeleteFailure::not_deleted(s::delete_account_error_try_again_shortly(l()));
    }
    DeleteFailure::not_deleted(service_sentence(error))
}
