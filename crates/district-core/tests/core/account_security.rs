//! Two-step verification and deleting the account: the fresh re-authentication
//! in front of each, every step of each screen, and every refusal's sentence.

use district_api::{
    ApiError, Endpoint, ErrorDetail, TransportError, TransportKind, UnauthorizedReason,
};
use district_auth::device_hash;
use district_core::l10n::english as en;
use district_core::{
    DeleteAccountEvent, DeleteAccountStep, DeleteFailure, Effect, Event, Model, Notice,
    ReauthProof, ReauthProvider, ReauthStep, RecoveryCodes, Route, ServiceSignOut, SessionState,
    SignOutOutcome, SignOutScope, SigningOut, Tab, Ticket, TwoStepChange, TwoStepEvent,
    TwoStepNotice, TwoStepScreen, TwoStepSecret, TwoStepStep,
};
use district_model::{Credential, MfaEnrollment, MfaStatus, ReauthAction};

use crate::support::{AGENCY, THIS_DEVICE, fixture, last_ticket, loaded, server_error, signed_in};

// ── Helpers ────────────────────────────────────────────────────────────────

fn status(enrolled: bool) -> MfaStatus {
    MfaStatus {
        enrolled,
        enrolled_at: enrolled.then(|| "2026-10-01T12:00:00.000Z".to_owned()),
        backup_codes_remaining: if enrolled { 7 } else { 0 },
        can_disable: true,
    }
}

fn two_step(model: &Model) -> &TwoStepScreen {
    &signed_in(model).two_step
}

fn delete(model: &Model) -> &district_core::DeleteAccountScreen {
    &signed_in(model).delete_account
}

fn credential(value: &str) -> Credential {
    Credential::new(value)
}

fn detail(code: &str) -> ErrorDetail {
    ErrorDetail {
        message: Some(format!("The service says {code}.")),
        code: Some(code.to_owned()),
        ..ErrorDetail::default()
    }
}

fn envelope(status: u16, code: &str) -> ApiError {
    ApiError::Envelope {
        status,
        code: code.to_owned(),
        detail: detail(code),
    }
}

fn forbidden(code: &str) -> ApiError {
    ApiError::Forbidden(detail(code))
}

fn offline() -> ApiError {
    ApiError::Offline(TransportError {
        kind: TransportKind::Connect,
        message: "connection refused".to_owned(),
    })
}

fn not_json() -> ApiError {
    ApiError::Offline(TransportError {
        kind: TransportKind::NotJson {
            status: 502,
            content_type: "text/html".to_owned(),
        },
        message: "not json".to_owned(),
    })
}

fn unconfirmed() -> ApiError {
    ApiError::Unconfirmed {
        endpoint: Endpoint::MfaConfirm,
    }
}

fn rate_limited() -> ApiError {
    ApiError::RateLimited {
        retry_after: None,
        detail: ErrorDetail::default(),
    }
}

fn refused_not_retried() -> ApiError {
    ApiError::Unauthorized(UnauthorizedReason::RefusedNotRetried)
}

fn service_sentence(code: &str) -> String {
    district_core::l10n::api_error_text::<&str>(district_core::Locale::En, code, &[]).unwrap()
}

/// The page the effects open, as a URL.
fn opened(effects: &[Effect]) -> url::Url {
    match effects {
        [Effect::OpenOneTimeUrl { url }] => url::Url::parse(url.expose()).unwrap(),
        other => panic!("no page opened: {other:?}"),
    }
}

fn param(url: &url::Url, name: &str) -> String {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .unwrap_or_else(|| panic!("no {name}"))
}

/// The browser's answer to the page `effects` opened.
fn answer(effects: &[Effect], code: &str) -> Event {
    let state = param(&opened(effects), "state");
    Event::SignInCallback(format!("districtai://auth/?code={code}&state={state}"))
}

fn reauthenticated(effects: &[Effect], result: Result<i64, ApiError>) -> Event {
    match effects {
        [Effect::Reauthenticate { ticket, action, .. }] => Event::Reauthenticated {
            ticket: *ticket,
            action: *action,
            result,
        },
        other => panic!("no re-authentication: {other:?}"),
    }
}

fn opened_two_step() -> (Model, Ticket) {
    let (mut model, _) = loaded(AGENCY, "agency");
    let effects = model.update(Event::Navigate(Route::TwoStep));
    let [Effect::LoadTwoStep { ticket }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    (model, *ticket)
}

/// The two-step screen with the status read.
fn at_overview(read: MfaStatus) -> Model {
    let (mut model, ticket) = opened_two_step();
    model.update(Event::TwoStepLoaded {
        ticket,
        result: Ok(read),
    });
    model
}

/// The two-step screen at its password step, for `change`.
fn at_password(change: TwoStepChange) -> Model {
    let mut model = at_overview(status(change == TwoStepChange::TurnOff));
    model.update(Event::TwoStep(TwoStepEvent::Begin(change)));
    model.update(Event::TwoStep(TwoStepEvent::UsePassword));
    model
}

fn submit(model: &mut Model, password: &str, code: Option<&str>) -> Vec<Effect> {
    model.update(Event::TwoStep(TwoStepEvent::SubmitPassword {
        password: credential(password),
        code: code.map(credential),
    }))
}

/// The two-step screen with its secret showing.
fn at_scan() -> (Model, Vec<Effect>) {
    let mut model = at_password(TwoStepChange::TurnOn);
    let effects = submit(&mut model, "pw", None);
    let effects = model.update(reauthenticated(&effects, Ok(300)));
    let [Effect::StartTwoStep { ticket }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    let effects = model.update(Event::TwoStepStarted {
        ticket: *ticket,
        result: Ok(fixture("district-mfa-enroll.json")),
    });
    (model, effects)
}

fn confirmed(model: &mut Model, code: &str) -> Ticket {
    let effects = model.update(Event::TwoStep(TwoStepEvent::Confirm(credential(code))));
    let [Effect::ConfirmTwoStep { ticket, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    *ticket
}

/// The two-step screen with the proof standing for turning it off: the
/// ticket of the change.
fn turning_off() -> (Model, Ticket) {
    let mut model = at_password(TwoStepChange::TurnOff);
    let effects = submit(&mut model, "pw", None);
    let effects = model.update(reauthenticated(&effects, Ok(300)));
    let [Effect::TurnOffTwoStep { ticket }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    (model, *ticket)
}

/// The two-step screen with the proof standing for turning it on: the ticket
/// of the start.
fn starting() -> (Model, Ticket) {
    let mut model = at_password(TwoStepChange::TurnOn);
    let effects = submit(&mut model, "pw", None);
    let effects = model.update(reauthenticated(&effects, Ok(300)));
    (model, last_ticket(&effects))
}

fn opened_deletion() -> Model {
    let (mut model, _) = loaded(AGENCY, "agency");
    assert!(
        model
            .update(Event::Navigate(Route::DeleteAccount))
            .is_empty()
    );
    model
}

fn deletion_at_password() -> Model {
    let mut model = opened_deletion();
    model.update(Event::AccountDeletion(DeleteAccountEvent::UsePassword));
    model
}

fn delete_submit(model: &mut Model, password: &str, code: Option<&str>) -> Vec<Effect> {
    model.update(Event::AccountDeletion(DeleteAccountEvent::SubmitPassword {
        password: credential(password),
        code: code.map(credential),
    }))
}

/// The deletion screen with the proof standing.
fn deletion_confirming() -> Model {
    let mut model = deletion_at_password();
    let effects = delete_submit(&mut model, "pw", None);
    model.update(reauthenticated(&effects, Ok(300)));
    assert_eq!(delete(&model).step, DeleteAccountStep::Confirm);
    model
}

fn deleting(model: &mut Model, typed: &str) -> Ticket {
    let effects = model.update(Event::AccountDeletion(DeleteAccountEvent::Delete {
        confirmation: typed.to_owned(),
    }));
    let [Effect::DeleteAccount { ticket, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    *ticket
}

// ── The routes ─────────────────────────────────────────────────────────────

#[test]
fn both_screens_sit_under_the_account_and_need_no_workspace() {
    for route in [Route::TwoStep, Route::DeleteAccount] {
        assert_eq!(route.tab(), Tab::Account);
        assert_eq!(route.parent(), Some(Route::Account));
        assert!(!route.is_workspace_scoped());
    }
}

// ── Two-step verification: the status ─────────────────────────────────────

#[test]
fn opening_the_screen_reads_the_status_and_shows_it() {
    let (mut model, ticket) = opened_two_step();
    assert_eq!(two_step(&model).step, TwoStepStep::Loading);
    assert!(two_step(&model).busy);
    assert_eq!(two_step(&model).status_label(), None);
    assert!(two_step(&model).status_lines().is_empty());
    assert_eq!(two_step(&model).offered(), None);

    model.update(Event::TwoStepLoaded {
        ticket,
        result: Ok(fixture("district-mfa-status.json")),
    });
    let screen = two_step(&model);
    assert_eq!(screen.step, TwoStepStep::Overview);
    assert!(!screen.busy);
    assert_eq!(screen.status_label(), Some(en::TWO_STEP_STATUS_ON));
    assert_eq!(
        screen.status_lines(),
        ["On since Oct 1, 2026.", "7 recovery codes left."]
    );
    assert_eq!(screen.offered(), Some(TwoStepChange::TurnOff));
    assert!(!screen.signs_in_without_password());
}

#[test]
fn the_status_lines_say_what_each_state_means() {
    let lines = |read: MfaStatus| {
        TwoStepScreen {
            status: Some(read),
            ..TwoStepScreen::default()
        }
        .status_lines()
    };
    assert_eq!(lines(status(false)), [en::TWO_STEP_OFF_EXPLAIN]);
    let off = TwoStepScreen {
        status: Some(status(false)),
        ..TwoStepScreen::default()
    };
    assert_eq!(off.status_label(), Some(en::TWO_STEP_STATUS_OFF));
    assert_eq!(off.offered(), Some(TwoStepChange::TurnOn));

    let mut read = status(true);
    read.backup_codes_remaining = 1;
    read.enrolled_at = Some("not a date".to_owned());
    assert_eq!(lines(read.clone()), [en::TWO_STEP_CODES_LEFT_ONE]);
    read.backup_codes_remaining = 0;
    read.can_disable = false;
    assert_eq!(
        lines(read),
        [en::TWO_STEP_CODES_NONE, en::TWO_STEP_NO_PASSWORD_NOTE]
    );
    assert_eq!(
        TwoStepScreen::reauth_intro(TwoStepChange::TurnOn),
        en::TWO_STEP_REAUTH_INTRO_ON
    );
    assert_eq!(
        TwoStepScreen::reauth_intro(TwoStepChange::TurnOff),
        en::TWO_STEP_REAUTH_INTRO_OFF
    );
}

#[test]
fn a_first_read_that_fails_offers_a_retry_and_a_later_one_keeps_the_status() {
    let (mut model, ticket) = opened_two_step();
    model.update(Event::TwoStepLoaded {
        ticket,
        result: Err(offline()),
    });
    assert_eq!(two_step(&model).step, TwoStepStep::LoadFailed);
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(en::TWO_STEP_ERROR_OFFLINE_LOAD)
    );
    // Retry is the failed step's only way on.
    assert!(
        model
            .update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOn)))
            .is_empty()
    );
    let effects = model.update(Event::TwoStep(TwoStepEvent::Retry));
    assert_eq!(two_step(&model).step, TwoStepStep::Loading);
    assert_eq!(two_step(&model).failure, None);
    // A refresh while the read is on its way sends nothing more.
    assert!(model.update(Event::Refresh).is_empty());
    model.update(Event::TwoStepLoaded {
        ticket: last_ticket(&effects),
        result: Ok(status(false)),
    });

    let effects = model.update(Event::Refresh);
    let [Effect::LoadTwoStep { ticket }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    model.update(Event::TwoStepLoaded {
        ticket: *ticket,
        result: Err(server_error()),
    });
    assert_eq!(two_step(&model).step, TwoStepStep::Overview);
    assert_eq!(two_step(&model).status, Some(status(false)));
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(en::TWO_STEP_ERROR_LOAD)
    );
    // Retry does nothing once the status is shown.
    assert!(model.update(Event::TwoStep(TwoStepEvent::Retry)).is_empty());
}

#[test]
fn each_kind_of_failed_read_has_its_sentence() {
    for (error, sentence) in [
        (unconfirmed(), en::TWO_STEP_ERROR_LOAD.to_owned()),
        (not_json(), en::FAILURE_NOT_JSON.to_owned()),
        (
            forbidden("account_not_found"),
            "The service says account_not_found.".to_owned(),
        ),
    ] {
        let (mut model, ticket) = opened_two_step();
        model.update(Event::TwoStepLoaded {
            ticket,
            result: Err(error),
        });
        assert_eq!(two_step(&model).failure.as_deref(), Some(sentence.as_str()));
    }
}

#[test]
fn only_the_change_the_status_offers_can_start() {
    let mut model = at_overview(status(true));
    assert!(
        model
            .update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOn)))
            .is_empty()
    );
    assert_eq!(two_step(&model).step, TwoStepStep::Overview);
    model.update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOff)));
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOff,
            step: ReauthStep::Choose
        }
    );
}

// ── Two-step verification: turning it on ──────────────────────────────────

#[test]
fn turning_on_through_a_provider_shows_the_secret_then_the_codes_once() {
    let mut model = at_overview(status(false));
    model.update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOn)));
    let effects = model.update(Event::TwoStep(TwoStepEvent::UseProvider(
        ReauthProvider::Google,
    )));
    let page = opened(&effects);
    assert_eq!(
        format!(
            "{}://{}{}",
            page.scheme(),
            page.host_str().unwrap(),
            page.path()
        ),
        "https://www.distronode.com/auth/native/reauth"
    );
    assert_eq!(param(&page, "provider"), "google");
    assert_eq!(param(&page, "action"), "2fa-change");
    assert_eq!(param(&page, "redirect_uri"), "districtai://auth");
    assert_eq!(param(&page, "device"), device_hash(THIS_DEVICE));
    assert!(!page.as_str().contains(THIS_DEVICE));
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOn,
            step: ReauthStep::Browser(ReauthProvider::Google)
        }
    );
    assert!(!two_step(&model).busy);

    let spent = model.update(answer(&effects, "one-time"));
    let [
        Effect::Reauthenticate {
            action: ReauthAction::TwoStepChange,
            proof: ReauthProof::Provider { code, verifier },
            ..
        },
    ] = spent.as_slice()
    else {
        panic!("{spent:?}")
    };
    assert_eq!(code.expose(), "one-time");
    assert_eq!(
        district_auth::challenge_for(verifier.expose()),
        param(&page, "code_challenge")
    );
    assert!(two_step(&model).busy);
    // The same answer twice is spent once.
    assert!(model.update(answer(&effects, "one-time")).is_empty());

    let effects = model.update(reauthenticated(&spent, Ok(300)));
    let [Effect::StartTwoStep { ticket }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    model.update(Event::TwoStepStarted {
        ticket: *ticket,
        result: Ok(fixture("district-mfa-enroll.json")),
    });
    let TwoStepStep::Scan(secret) = &two_step(&model).step else {
        panic!("{:?}", two_step(&model).step)
    };
    assert_eq!(secret.key, "JBSWY3DPEHPK3PXP");
    assert!(secret.otpauth_uri.starts_with("otpauth://totp/"));
    assert!(secret.qr_png.as_deref().unwrap().starts_with(b"\x89PNG"));
    assert!(!two_step(&model).busy);

    // Nothing to send without a code.
    assert!(
        model
            .update(Event::TwoStep(TwoStepEvent::Confirm(credential("   "))))
            .is_empty()
    );
    let effects = model.update(Event::TwoStep(TwoStepEvent::Confirm(credential(
        " 123456 ",
    ))));
    let [Effect::ConfirmTwoStep { ticket, code }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert_eq!(code.expose(), "123456");
    model.update(Event::TwoStepConfirmed {
        ticket: *ticket,
        result: Ok(fixture("district-mfa-confirm.json")),
    });
    let screen = two_step(&model);
    let TwoStepStep::RecoveryCodes(codes) = &screen.step else {
        panic!("{:?}", screen.step)
    };
    assert_eq!(codes.joined(), "aaaa-bbbb\ncccc-dddd");
    assert!(screen.status.as_ref().unwrap().enrolled);
    assert_eq!(screen.status.as_ref().unwrap().backup_codes_remaining, 2);
    assert!(screen.status.as_ref().unwrap().can_disable);

    let effects = model.update(Event::TwoStep(TwoStepEvent::FinishRecoveryCodes));
    assert!(matches!(effects.as_slice(), [Effect::LoadTwoStep { .. }]));
    let screen = two_step(&model);
    assert_eq!(screen.step, TwoStepStep::Overview);
    assert_eq!(screen.notice, Some(TwoStepNotice::TurnedOn));
    assert_eq!(TwoStepNotice::TurnedOn.message(), en::TWO_STEP_TURNED_ON);
    model.update(Event::TwoStep(TwoStepEvent::DismissNotices));
    assert_eq!(two_step(&model).notice, None);
}

#[test]
fn the_secret_and_the_codes_never_reach_debug_output() {
    let (model, _) = at_scan();
    let shown = format!("{:?}", model.session());
    assert!(!shown.contains("JBSWY3DPEHPK3PXP"), "{shown}");
    assert!(shown.contains("TwoStepSecret(<redacted>)"));
    let codes = RecoveryCodes(vec!["aaaa-bbbb".to_owned()]);
    assert_eq!(format!("{codes:?}"), "RecoveryCodes(<1 redacted>)");
    let effect = Effect::ConfirmTwoStep {
        ticket: two_step_ticket(),
        code: credential("123456"),
    };
    assert!(!format!("{effect:?}").contains("123456"));
}

fn two_step_ticket() -> Ticket {
    opened_two_step().1
}

#[test]
fn a_qr_code_the_app_cannot_read_leaves_the_key() {
    for picture in ["data:image/png;base64,***", "https://example.test/qr.png"] {
        let (mut model, ticket) = starting();
        let mut enrollment: MfaEnrollment = fixture("district-mfa-enroll.json");
        enrollment.qr_data_url = picture.to_owned();
        model.update(Event::TwoStepStarted {
            ticket,
            result: Ok(enrollment),
        });
        let TwoStepStep::Scan(TwoStepSecret { qr_png, key, .. }) = &two_step(&model).step else {
            panic!("{:?}", two_step(&model).step)
        };
        assert_eq!(qr_png, &None);
        assert_eq!(key, "JBSWY3DPEHPK3PXP");
    }
}

#[test]
fn back_inside_the_screen_steps_back_and_drops_what_it_held() {
    let mut model = at_password(TwoStepChange::TurnOn);
    model.update(Event::TwoStep(TwoStepEvent::Back));
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOn,
            step: ReauthStep::Choose
        }
    );
    model.update(Event::TwoStep(TwoStepEvent::Back));
    assert_eq!(two_step(&model).step, TwoStepStep::Overview);
    // Nothing further back inside the screen.
    model.update(Event::TwoStep(TwoStepEvent::Back));
    assert_eq!(two_step(&model).step, TwoStepStep::Overview);

    // Back from the browser drops the attempt: its answer is not spent.
    model.update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOn)));
    let effects = model.update(Event::TwoStep(TwoStepEvent::UseProvider(
        ReauthProvider::Apple,
    )));
    model.update(Event::TwoStep(TwoStepEvent::Back));
    assert_eq!(two_step(&model).step, TwoStepStep::Overview);
    assert!(model.update(answer(&effects, "c")).is_empty());

    let (mut model, _) = at_scan();
    model.update(Event::TwoStep(TwoStepEvent::Back));
    assert_eq!(two_step(&model).step, TwoStepStep::Overview);
}

#[test]
fn leaving_the_screen_drops_the_secret_and_ignores_late_answers() {
    let (mut model, _) = at_scan();
    let ticket = confirmed(&mut model, "123456");
    model.update(Event::Navigate(Route::Account));
    assert_eq!(two_step(&model), &TwoStepScreen::default());
    assert!(
        model
            .update(Event::TwoStepConfirmed {
                ticket,
                result: Ok(fixture("district-mfa-confirm.json")),
            })
            .is_empty()
    );
    assert_eq!(two_step(&model), &TwoStepScreen::default());

    // Each answer of a screen that was left is dropped.
    let (mut model, ticket) = opened_two_step();
    model.update(Event::Back);
    model.update(Event::TwoStepLoaded {
        ticket,
        result: Ok(status(true)),
    });
    assert_eq!(two_step(&model).status, None);

    let (mut model, ticket) = starting();
    model.update(Event::Back);
    model.update(Event::TwoStepStarted {
        ticket,
        result: Ok(fixture("district-mfa-enroll.json")),
    });
    assert_eq!(two_step(&model).step, TwoStepStep::Loading);

    let (mut model, ticket) = turning_off();
    model.update(Event::Back);
    model.update(Event::TwoStepTurnedOff {
        ticket,
        result: Ok(()),
    });
    assert_eq!(two_step(&model).notice, None);

    let mut model = at_password(TwoStepChange::TurnOn);
    let effects = submit(&mut model, "pw", None);
    model.update(Event::Back);
    assert!(model.update(reauthenticated(&effects, Ok(300))).is_empty());
}

#[test]
fn a_busy_screen_ignores_every_click() {
    let mut model = at_password(TwoStepChange::TurnOn);
    assert_eq!(submit(&mut model, "pw", None).len(), 1);
    assert!(submit(&mut model, "pw", None).is_empty());
    assert!(model.update(Event::TwoStep(TwoStepEvent::Back)).is_empty());
    assert!(two_step(&model).busy);
}

// ── Two-step verification: turning it off ─────────────────────────────────

#[test]
fn turning_off_by_password_asks_for_the_code_once_the_service_does() {
    let mut model = at_password(TwoStepChange::TurnOff);
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOff,
            step: ReauthStep::Password { needs_code: false }
        }
    );
    assert!(submit(&mut model, "", None).is_empty());
    let effects = submit(&mut model, "pw", Some("ignored"));
    let [
        Effect::Reauthenticate {
            proof: ReauthProof::Password { password, code },
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}")
    };
    assert_eq!(password.expose(), "pw");
    assert_eq!(code, &None, "the code goes only once asked for");

    model.update(reauthenticated(
        &effects,
        Err(envelope(401, "MFA_REQUIRED")),
    ));
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOff,
            step: ReauthStep::Password { needs_code: true }
        }
    );
    assert_eq!(two_step(&model).failure, None);
    assert!(submit(&mut model, "pw", None).is_empty());
    assert!(submit(&mut model, "pw", Some("  ")).is_empty());
    let effects = submit(&mut model, "pw", Some(" 654321 "));
    let [
        Effect::Reauthenticate {
            proof: ReauthProof::Password {
                code: Some(code), ..
            },
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}")
    };
    assert_eq!(code.expose(), "654321");

    let effects = model.update(reauthenticated(&effects, Ok(300)));
    let [Effect::TurnOffTwoStep { ticket }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    let effects = model.update(Event::TwoStepTurnedOff {
        ticket: *ticket,
        result: Ok(()),
    });
    assert!(matches!(effects.as_slice(), [Effect::LoadTwoStep { .. }]));
    let screen = two_step(&model);
    assert_eq!(screen.step, TwoStepStep::Overview);
    assert_eq!(screen.notice, Some(TwoStepNotice::TurnedOff));
    assert_eq!(TwoStepNotice::TurnedOff.message(), en::TWO_STEP_TURNED_OFF);
    assert!(!screen.status.as_ref().unwrap().enrolled);
    assert!(screen.status.as_ref().unwrap().can_disable);
}

#[test]
fn an_account_without_a_password_turns_it_off_at_its_provider() {
    let mut read = status(true);
    read.can_disable = false;
    let mut model = at_overview(read);
    assert!(two_step(&model).signs_in_without_password());
    model.update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOff)));
    model.update(Event::TwoStep(TwoStepEvent::UsePassword));
    let effects = submit(&mut model, "guess", None);
    model.update(reauthenticated(
        &effects,
        Err(envelope(409, "NO_PASSWORD_SET")),
    ));
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOff,
            step: ReauthStep::Choose
        }
    );
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(en::REAUTH_ERROR_NO_PASSWORD)
    );

    let effects = model.update(Event::TwoStep(TwoStepEvent::UseProvider(
        ReauthProvider::Microsoft,
    )));
    let spent = model.update(answer(&effects, "c"));
    let effects = model.update(reauthenticated(&spent, Ok(300)));
    model.update(Event::TwoStepTurnedOff {
        ticket: last_ticket(&effects),
        result: Ok(()),
    });
    // What the status said stays: the next read says the rest.
    assert!(!two_step(&model).status.as_ref().unwrap().can_disable);
}

// ── The browser's answer ───────────────────────────────────────────────────

#[test]
fn only_this_attempts_answer_is_spent() {
    let mut model = at_overview(status(false));
    model.update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOn)));
    let first = model.update(Event::TwoStep(TwoStepEvent::UseProvider(
        ReauthProvider::Google,
    )));
    // A link that is not the sign-in answer leaves the attempt waiting.
    assert!(
        model
            .update(Event::SignInCallback(
                "districtai://other?code=c".to_owned()
            ))
            .is_empty()
    );
    // A second provider replaces the first attempt.
    let second = model.update(Event::TwoStep(TwoStepEvent::UseProvider(
        ReauthProvider::Apple,
    )));
    assert!(model.update(answer(&first, "c")).is_empty());
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(en::REAUTH_ERROR_SIGN_IN_NOT_VERIFIED)
    );
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOn,
            step: ReauthStep::Choose
        }
    );
    // The refused answer dropped the attempt, so the right one comes too late.
    assert!(model.update(answer(&second, "c")).is_empty());

    // No attempt at all: a stale link is ignored.
    assert!(
        model
            .update(Event::SignInCallback(
                "districtai://auth?code=c&state=s".to_owned()
            ))
            .is_empty()
    );
}

#[test]
fn choosing_the_password_drops_the_browser_attempt() {
    let mut model = at_overview(status(false));
    model.update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOn)));
    let effects = model.update(Event::TwoStep(TwoStepEvent::UseProvider(
        ReauthProvider::Google,
    )));
    model.update(Event::TwoStep(TwoStepEvent::UsePassword));
    assert!(model.update(answer(&effects, "c")).is_empty());
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOn,
            step: ReauthStep::Password { needs_code: false }
        }
    );
}

#[test]
fn a_browser_that_does_not_open_says_so_on_the_screen() {
    let mut model = at_overview(status(false));
    model.update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOn)));
    model.update(Event::TwoStep(TwoStepEvent::UseProvider(
        ReauthProvider::Google,
    )));
    model.update(Event::UrlOpenFailed);
    assert_eq!(signed_in(&model).notice, Some(Notice::NoBrowser));
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(en::REAUTH_ERROR_BROWSER_DID_NOT_OPEN)
    );
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOn,
            step: ReauthStep::Choose
        }
    );

    let mut model = opened_deletion();
    model.update(Event::AccountDeletion(DeleteAccountEvent::UseProvider(
        ReauthProvider::Google,
    )));
    model.update(Event::UrlOpenFailed);
    assert_eq!(
        delete(&model).failure,
        Some(DeleteFailure {
            title: None,
            message: en::REAUTH_ERROR_BROWSER_DID_NOT_OPEN.to_owned()
        })
    );
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Choose)
    );
}

#[test]
fn every_refused_reauthentication_has_its_sentence() {
    let cases = [
        (
            forbidden("incorrect_password"),
            en::REAUTH_ERROR_WRONG_PASSWORD.to_owned(),
        ),
        (
            envelope(400, "mfa_code_invalid"),
            en::REAUTH_ERROR_WRONG_CODE.to_owned(),
        ),
        (
            forbidden("reauth_failed"),
            en::REAUTH_ERROR_NOT_CONFIRMED.to_owned(),
        ),
        (
            rate_limited(),
            en::REAUTH_ERROR_TOO_MANY_ATTEMPTS.to_owned(),
        ),
        (
            envelope(429, "rate_limited"),
            en::REAUTH_ERROR_TOO_MANY_ATTEMPTS.to_owned(),
        ),
        (offline(), en::REAUTH_ERROR_OFFLINE_NOT_CONFIRMED.to_owned()),
        (server_error(), en::REAUTH_ERROR_NOT_CONFIRMED.to_owned()),
        (
            envelope(500, "server_error"),
            en::REAUTH_ERROR_NOT_CONFIRMED.to_owned(),
        ),
        (
            envelope(503, "service_unavailable"),
            en::REAUTH_ERROR_NOT_CONFIRMED.to_owned(),
        ),
        (unconfirmed(), en::REAUTH_ERROR_NOT_CONFIRMED.to_owned()),
        (
            forbidden("forbidden"),
            "The service says forbidden.".to_owned(),
        ),
    ];
    for (error, sentence) in cases {
        let mut model = at_password(TwoStepChange::TurnOn);
        let effects = submit(&mut model, "pw", None);
        model.update(reauthenticated(&effects, Err(error.clone())));
        assert_eq!(
            two_step(&model).failure.as_deref(),
            Some(sentence.as_str()),
            "{error:?}"
        );
        assert_eq!(
            two_step(&model).step,
            TwoStepStep::Reauth {
                change: TwoStepChange::TurnOn,
                step: ReauthStep::Password { needs_code: false }
            }
        );
        assert!(!two_step(&model).busy);
    }

    // Through the browser, a refused code is most often another account's.
    let mut model = at_overview(status(false));
    model.update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOn)));
    let effects = model.update(Event::TwoStep(TwoStepEvent::UseProvider(
        ReauthProvider::Google,
    )));
    let spent = model.update(answer(&effects, "c"));
    model.update(reauthenticated(&spent, Err(forbidden("reauth_failed"))));
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(en::REAUTH_ERROR_WRONG_ACCOUNT)
    );
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOn,
            step: ReauthStep::Choose
        }
    );
}

// ── Two-step verification: refused changes ────────────────────────────────

#[test]
fn a_start_that_fails_stays_on_the_reauthentication() {
    let cases = [
        (
            offline(),
            en::TWO_STEP_ERROR_OFFLINE_NOTHING_CHANGED.to_owned(),
        ),
        (unconfirmed(), en::TWO_STEP_ERROR_TRY_AGAIN.to_owned()),
        (server_error(), en::TWO_STEP_ERROR_TRY_AGAIN.to_owned()),
        (
            refused_not_retried(),
            en::FAILURE_REFUSED_NOT_RETRIED.to_owned(),
        ),
        (
            forbidden("browser_session_required"),
            service_sentence("browser_session_required"),
        ),
    ];
    for (error, sentence) in cases {
        let (mut model, ticket) = starting();
        assert!(
            model
                .update(Event::TwoStepStarted {
                    ticket,
                    result: Err(error.clone()),
                })
                .is_empty()
        );
        assert_eq!(
            two_step(&model).failure.as_deref(),
            Some(sentence.as_str()),
            "{error:?}"
        );
        assert_eq!(
            two_step(&model).step,
            TwoStepStep::Reauth {
                change: TwoStepChange::TurnOn,
                step: ReauthStep::Password { needs_code: false }
            }
        );
    }
}

#[test]
fn a_spent_or_expired_proof_asks_to_confirm_again() {
    let (mut model, ticket) = starting();
    model.update(Event::TwoStepStarted {
        ticket,
        result: Err(forbidden("reauth_required")),
    });
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOn,
            step: ReauthStep::Choose
        }
    );
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(en::TWO_STEP_ERROR_PROOF_EXPIRED)
    );

    let (mut model, _) = at_scan();
    let ticket = confirmed(&mut model, "123456");
    model.update(Event::TwoStepConfirmed {
        ticket,
        result: Err(forbidden("reauth_required")),
    });
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOn,
            step: ReauthStep::Choose
        }
    );

    let (mut model, ticket) = turning_off();
    model.update(Event::TwoStepTurnedOff {
        ticket,
        result: Err(forbidden("reauth_required")),
    });
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOff,
            step: ReauthStep::Choose
        }
    );
}

#[test]
fn a_change_that_may_have_happened_reads_the_status_again() {
    let reread = |model: &mut Model, effects: Vec<Effect>, sentence: &str, read: MfaStatus| {
        let [Effect::LoadTwoStep { ticket }] = effects.as_slice() else {
            panic!("{effects:?}")
        };
        assert_eq!(two_step(model).step, TwoStepStep::Overview);
        assert_eq!(two_step(model).failure.as_deref(), Some(sentence));
        model.update(Event::TwoStepLoaded {
            ticket: *ticket,
            result: Ok(read.clone()),
        });
        // The refusal stays over the status it led to.
        assert_eq!(two_step(model).failure.as_deref(), Some(sentence));
        assert_eq!(two_step(model).status, Some(read));
    };

    for error in [offline(), unconfirmed()] {
        let (mut model, _) = at_scan();
        let ticket = confirmed(&mut model, "123456");
        let effects = model.update(Event::TwoStepConfirmed {
            ticket,
            result: Err(error),
        });
        reread(
            &mut model,
            effects,
            en::TWO_STEP_ERROR_OUTCOME_UNKNOWN_ON,
            status(true),
        );

        let (mut model, ticket) = turning_off();
        let effects = model.update(Event::TwoStepTurnedOff {
            ticket,
            result: Err(offline()),
        });
        reread(
            &mut model,
            effects,
            en::TWO_STEP_ERROR_OUTCOME_UNKNOWN_OFF,
            status(false),
        );
    }

    for code in ["MFA_ALREADY_ENABLED", "MFA_NOT_STARTED", "MFA_NOT_PENDING"] {
        let (mut model, ticket) = starting();
        let effects = model.update(Event::TwoStepStarted {
            ticket,
            result: Err(ApiError::Conflict(detail(code))),
        });
        reread(&mut model, effects, &service_sentence(code), status(true));
    }

    // A read that fails after the refusal says so instead.
    let (mut model, ticket) = turning_off();
    let effects = model.update(Event::TwoStepTurnedOff {
        ticket,
        result: Err(offline()),
    });
    model.update(Event::TwoStepLoaded {
        ticket: last_ticket(&effects),
        result: Err(offline()),
    });
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(en::TWO_STEP_ERROR_OFFLINE_LOAD)
    );
    assert_eq!(two_step(&model).step, TwoStepStep::Overview);
}

#[test]
fn a_refused_code_or_confirm_stays_on_the_secret() {
    let cases = [
        (
            envelope(400, "mfa_code_invalid"),
            service_sentence("mfa_code_invalid"),
        ),
        (server_error(), en::TWO_STEP_ERROR_TRY_AGAIN.to_owned()),
        (
            refused_not_retried(),
            en::FAILURE_REFUSED_NOT_RETRIED.to_owned(),
        ),
    ];
    for (error, sentence) in cases {
        let (mut model, _) = at_scan();
        let ticket = confirmed(&mut model, "123456");
        assert!(
            model
                .update(Event::TwoStepConfirmed {
                    ticket,
                    result: Err(error.clone()),
                })
                .is_empty()
        );
        assert!(matches!(two_step(&model).step, TwoStepStep::Scan(_)));
        assert_eq!(
            two_step(&model).failure.as_deref(),
            Some(sentence.as_str()),
            "{error:?}"
        );
        assert!(!two_step(&model).busy);
    }

    let (mut model, ticket) = turning_off();
    model.update(Event::TwoStepTurnedOff {
        ticket,
        result: Err(forbidden("browser_session_required")),
    });
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(service_sentence("browser_session_required").as_str())
    );
}

// ── Deleting the account ───────────────────────────────────────────────────

#[test]
fn deleting_through_a_provider_signs_out_and_says_the_account_is_gone() {
    let mut model = opened_deletion();
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Choose)
    );
    // Nothing to delete before the proof.
    assert!(
        model
            .update(Event::AccountDeletion(DeleteAccountEvent::Delete {
                confirmation: "ada@example.com".to_owned()
            }))
            .is_empty()
    );
    let effects = model.update(Event::AccountDeletion(DeleteAccountEvent::UseProvider(
        ReauthProvider::Microsoft,
    )));
    let page = opened(&effects);
    assert_eq!(param(&page, "provider"), "azure-ad");
    assert_eq!(param(&page, "action"), "delete-account");
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Browser(ReauthProvider::Microsoft))
    );

    let spent = model.update(answer(&effects, "c"));
    assert!(matches!(
        spent.as_slice(),
        [Effect::Reauthenticate {
            action: ReauthAction::DeleteAccount,
            proof: ReauthProof::Provider { .. },
            ..
        }]
    ));
    assert!(delete(&model).busy);
    model.update(reauthenticated(&spent, Ok(300)));
    assert_eq!(delete(&model).step, DeleteAccountStep::Confirm);
    assert!(!delete(&model).busy);
    // Back keeps the proof.
    model.update(Event::AccountDeletion(DeleteAccountEvent::Back));
    assert_eq!(delete(&model).step, DeleteAccountStep::Confirm);

    assert!(
        model
            .update(Event::AccountDeletion(DeleteAccountEvent::Delete {
                confirmation: "  ".to_owned()
            }))
            .is_empty()
    );
    let effects = model.update(Event::AccountDeletion(DeleteAccountEvent::Delete {
        confirmation: " Ada@example.com ".to_owned(),
    }));
    let [
        Effect::DeleteAccount {
            ticket,
            confirmation,
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}")
    };
    assert_eq!(confirmation, "Ada@example.com");
    assert!(delete(&model).busy);
    let effects = model.update(Event::AccountDeleted {
        ticket: *ticket,
        result: Ok(false),
    });
    assert!(effects.iter().any(|e| matches!(e, Effect::SignOut { .. })));
    assert_eq!(
        model.session(),
        &SessionState::SigningOut(SigningOut {
            scope: SignOutScope::AccountDeleted {
                erasure_pending: false
            }
        })
    );
}

#[test]
fn the_signed_out_screen_says_the_account_was_deleted() {
    for (pending, headline) in [
        (false, en::DELETE_ACCOUNT_DELETED),
        (true, en::DELETE_ACCOUNT_DELETED_ERASURE_PENDING),
    ] {
        let outcome = SignOutOutcome {
            scope: SignOutScope::AccountDeleted {
                erasure_pending: pending,
            },
            service: ServiceSignOut::Deferred,
            removed: Ok(()),
        };
        assert_eq!(outcome.headline(), headline);
        // What became of the session on the service does not matter: the
        // account is gone.
        assert!(outcome.details().is_empty());
    }
}

#[test]
fn deleting_by_password_asks_for_the_code_once_the_service_does() {
    let mut model = deletion_at_password();
    assert!(delete_submit(&mut model, "", None).is_empty());
    let effects = delete_submit(&mut model, "pw", None);
    assert!(delete(&model).busy);
    assert!(delete_submit(&mut model, "pw", None).is_empty());
    model.update(reauthenticated(
        &effects,
        Err(envelope(401, "MFA_REQUIRED")),
    ));
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Password { needs_code: true })
    );
    assert!(delete_submit(&mut model, "pw", None).is_empty());
    let effects = delete_submit(&mut model, "pw", Some("123456"));
    model.update(reauthenticated(
        &effects,
        Err(forbidden("incorrect_password")),
    ));
    assert_eq!(
        delete(&model).failure,
        Some(DeleteFailure {
            title: None,
            message: en::REAUTH_ERROR_WRONG_PASSWORD.to_owned()
        })
    );
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Password { needs_code: true })
    );

    let effects = delete_submit(&mut model, "pw", Some("123456"));
    model.update(reauthenticated(
        &effects,
        Err(envelope(409, "NO_PASSWORD_SET")),
    ));
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Choose)
    );
    assert_eq!(
        delete(&model).failure.as_ref().unwrap().message,
        en::REAUTH_ERROR_NO_PASSWORD
    );
}

#[test]
fn back_on_the_deletion_screen_returns_to_the_ways_to_confirm() {
    let mut model = deletion_at_password();
    model.update(Event::AccountDeletion(DeleteAccountEvent::Back));
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Choose)
    );
    let effects = model.update(Event::AccountDeletion(DeleteAccountEvent::UseProvider(
        ReauthProvider::Google,
    )));
    model.update(Event::AccountDeletion(DeleteAccountEvent::Back));
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Choose)
    );
    assert!(model.update(answer(&effects, "c")).is_empty());

    // The password from the browser step drops that attempt too.
    let effects = model.update(Event::AccountDeletion(DeleteAccountEvent::UseProvider(
        ReauthProvider::Google,
    )));
    model.update(Event::AccountDeletion(DeleteAccountEvent::UsePassword));
    assert!(model.update(answer(&effects, "c")).is_empty());
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Password { needs_code: false })
    );
}

#[test]
fn a_refused_browser_answer_on_the_deletion_screen_says_so() {
    let mut model = opened_deletion();
    let effects = model.update(Event::AccountDeletion(DeleteAccountEvent::UseProvider(
        ReauthProvider::Google,
    )));
    let spent = model.update(answer(&effects, "c"));
    model.update(reauthenticated(&spent, Err(forbidden("reauth_failed"))));
    assert_eq!(
        delete(&model).failure.as_ref().unwrap().message,
        en::REAUTH_ERROR_WRONG_ACCOUNT
    );
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Choose)
    );

    let effects = model.update(Event::AccountDeletion(DeleteAccountEvent::UseProvider(
        ReauthProvider::Google,
    )));
    assert!(
        model
            .update(Event::SignInCallback(
                "districtai://auth?code=c&state=forged".to_owned()
            ))
            .is_empty()
    );
    assert_eq!(
        delete(&model).failure.as_ref().unwrap().message,
        en::REAUTH_ERROR_SIGN_IN_NOT_VERIFIED
    );
    assert!(model.update(answer(&effects, "c")).is_empty());
}

#[test]
fn every_refused_deletion_says_whether_anything_was_deleted() {
    let not_deleted = Some(en::DELETE_ACCOUNT_NOT_DELETED);
    let cases = [
        (
            offline(),
            None,
            en::DELETE_ACCOUNT_ERROR_OFFLINE_OUTCOME_UNKNOWN.to_owned(),
        ),
        (
            unconfirmed(),
            None,
            en::DELETE_ACCOUNT_ERROR_OUTCOME_UNKNOWN.to_owned(),
        ),
        (
            refused_not_retried(),
            None,
            en::FAILURE_REFUSED_NOT_RETRIED.to_owned(),
        ),
        (
            forbidden("confirmation_mismatch"),
            None,
            en::DELETE_ACCOUNT_ERROR_EMAIL_MISMATCH.to_owned(),
        ),
        (
            rate_limited(),
            not_deleted,
            en::REAUTH_ERROR_TOO_MANY_ATTEMPTS.to_owned(),
        ),
        (
            server_error(),
            not_deleted,
            en::DELETE_ACCOUNT_ERROR_TRY_AGAIN_SHORTLY.to_owned(),
        ),
        (
            ApiError::Conflict(detail("workspace_owner_only")),
            not_deleted,
            service_sentence("workspace_owner_only"),
        ),
    ];
    for (error, title, message) in cases {
        let mut model = deletion_confirming();
        let ticket = deleting(&mut model, "ada@example.com");
        assert!(
            model
                .update(Event::AccountDeleted {
                    ticket,
                    result: Err(error.clone()),
                })
                .is_empty()
        );
        assert_eq!(
            delete(&model).failure,
            Some(DeleteFailure { title, message }),
            "{error:?}"
        );
        assert_eq!(delete(&model).step, DeleteAccountStep::Confirm);
        assert!(!delete(&model).busy);
    }

    // The proof ran out: back to the start.
    let mut model = deletion_confirming();
    let ticket = deleting(&mut model, "ada@example.com");
    model.update(Event::AccountDeleted {
        ticket,
        result: Err(forbidden("reauth_required")),
    });
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Choose)
    );
    assert_eq!(
        delete(&model).failure.as_ref().unwrap().message,
        en::DELETE_ACCOUNT_ERROR_PROOF_EXPIRED
    );
}

#[test]
fn a_deletion_already_sent_is_still_answered_after_leaving_the_screen() {
    let mut model = deletion_confirming();
    let ticket = deleting(&mut model, "ada@example.com");
    // Busy: the screen takes no other click.
    assert!(
        model
            .update(Event::AccountDeletion(DeleteAccountEvent::Back))
            .is_empty()
    );
    model.update(Event::Navigate(Route::Account));
    model.update(Event::Navigate(Route::DeleteAccount));
    assert!(delete(&model).busy);
    model.update(Event::AccountDeleted {
        ticket,
        result: Ok(true),
    });
    assert_eq!(
        model.session(),
        &SessionState::SigningOut(SigningOut {
            scope: SignOutScope::AccountDeleted {
                erasure_pending: true
            }
        })
    );

    // A stale answer of a deletion never sent is ignored.
    let mut model = deletion_confirming();
    assert!(
        model
            .update(Event::AccountDeleted {
                ticket,
                result: Ok(true),
            })
            .is_empty()
    );
}

#[test]
fn a_reauthentication_answer_after_leaving_the_deletion_screen_is_dropped() {
    let mut model = deletion_at_password();
    let effects = delete_submit(&mut model, "pw", None);
    model.update(Event::Back);
    assert!(model.update(reauthenticated(&effects, Ok(300))).is_empty());
    assert_eq!(
        delete(&model).step,
        DeleteAccountStep::Explain(ReauthStep::Choose)
    );
}

#[test]
fn every_new_effect_names_its_ticket() {
    let ticket = two_step_ticket();
    for effect in [
        Effect::LoadTwoStep { ticket },
        Effect::StartTwoStep { ticket },
        Effect::ConfirmTwoStep {
            ticket,
            code: credential("1"),
        },
        Effect::TurnOffTwoStep { ticket },
        Effect::Reauthenticate {
            ticket,
            action: ReauthAction::TwoStepChange,
            proof: ReauthProof::Password {
                password: credential("pw"),
                code: None,
            },
        },
        Effect::DeleteAccount {
            ticket,
            confirmation: "ada@example.com".to_owned(),
        },
    ] {
        assert_eq!(effect.ticket(), Some(ticket), "{effect:?}");
    }
}

#[test]
fn a_code_this_build_does_not_know_reads_as_the_service_wrote_it() {
    let (mut model, ticket) = starting();
    model.update(Event::TwoStepStarted {
        ticket,
        result: Err(forbidden("a_code_from_the_future")),
    });
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some("The service says a_code_from_the_future.")
    );

    let mut model = deletion_confirming();
    let ticket = deleting(&mut model, "ada@example.com");
    model.update(Event::AccountDeleted {
        ticket,
        result: Err(forbidden("a_code_from_the_future")),
    });
    assert_eq!(
        delete(&model).failure,
        Some(DeleteFailure {
            title: Some(en::DELETE_ACCOUNT_NOT_DELETED),
            message: "The service says a_code_from_the_future.".to_owned()
        })
    );
}

#[test]
fn a_start_refused_after_a_provider_sign_in_goes_back_to_the_ways_to_confirm() {
    let mut model = at_overview(status(false));
    model.update(Event::TwoStep(TwoStepEvent::Begin(TwoStepChange::TurnOn)));
    let effects = model.update(Event::TwoStep(TwoStepEvent::UseProvider(
        ReauthProvider::Google,
    )));
    let spent = model.update(answer(&effects, "c"));
    let effects = model.update(reauthenticated(&spent, Ok(300)));
    model.update(Event::TwoStepStarted {
        ticket: last_ticket(&effects),
        result: Err(server_error()),
    });
    assert_eq!(
        two_step(&model).step,
        TwoStepStep::Reauth {
            change: TwoStepChange::TurnOn,
            step: ReauthStep::Choose
        }
    );
    assert_eq!(
        two_step(&model).failure.as_deref(),
        Some(en::TWO_STEP_ERROR_TRY_AGAIN)
    );
}
