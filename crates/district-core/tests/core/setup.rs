// SPDX-FileCopyrightText: Distronode Corporation
//
// SPDX-License-Identifier: Apache-2.0

//! The setup wizard in the app: the shell (the first open step, the single
//! step write, Back and Skip, the hand-off re-read, the refusals), and each of
//! its six steps, against the service's recorded answers.

use district_api::{ApiError, ErrorDetail};
use district_core::l10n::{self, Locale};
use district_core::setup::facts::{
    self, AddressDraft, DayState, FaqDraft, Interval, ServiceDraft, Weekday,
};
use district_core::setup::{
    FORWARDING_POLL_INTERVAL, FORWARDING_POLL_LIMIT, first_open, next_step, places, previous_step,
};
use district_core::{
    BusinessEvent, BusinessNotice, CallersEvent, Effect, Event, GoLiveEvent, Model,
    ReceptionistEvent, Route, SetupCallsEvent, SetupEvent, SetupHandOff, SetupLanguage, SetupPhase,
    SetupRefusal, SetupWizard, StepPhase, TestCallStatus, Ticket,
};
use district_model::{
    CallHandlingMode, CallHandlingResponse, ConnectionsResponse, ForwardingCheckMode,
    ForwardingCheckResponse, PersonaOptionsResponse, SetupLookupResponse, SetupResponse, SetupStep,
    SetupStepState, WorkspaceConfigResponse,
};
use serde_json::{Value, json};

use crate::support::{
    AGENCY, fixture, last_ticket, loaded, overview, restored, server_error, signed_in, ticket,
    with_purchases, workspace_list,
};

const ALL_TODO: [&str; 6] = ["todo"; 6];

fn wizard(model: &Model) -> &SetupWizard {
    &signed_in(model).setup
}

fn send(model: &mut Model, event: SetupEvent) -> Vec<Effect> {
    model.update(Event::Setup(event))
}

/// The wizard's state with these step states, in the wizard's order.
fn state(steps: [&str; 6], extra: Value) -> SetupResponse {
    let mut value = json!({
        "setupProgress": {"steps": {
            "business": steps[0], "number": steps[1], "receptionist": steps[2],
            "callers": steps[3], "calls": steps[4], "golive": steps[5],
        }},
        "businessFacts": null,
        "region": "ca",
        "tier": "VoicePro",
        "includedNumbers": 3,
        "numbersHeld": 1,
    });
    if let (Value::Object(base), Value::Object(more)) = (&mut value, extra) {
        base.extend(more);
    }
    serde_json::from_value(value).unwrap()
}

fn finished() -> SetupResponse {
    let mut done = state(["done"; 6], json!({}));
    done.setup_progress.as_mut().unwrap().completed_at = Some("2026-09-24T10:00:00.000Z".into());
    done
}

fn config_with(persona: Value) -> WorkspaceConfigResponse {
    let mut config: Value = fixture("district-workspace-config.json");
    config["config"]["aiPersona"] = persona;
    serde_json::from_value(config).unwrap()
}

fn workspace_config() -> WorkspaceConfigResponse {
    fixture("district-workspace-config.json")
}

/// The fixture's configuration on Voice Studio, a plan that includes caller
/// profiles (the fixture's own tier is a placeholder no plan is named).
fn studio_config(tier: &str) -> WorkspaceConfigResponse {
    let mut config = workspace_config();
    config.config.subscription_tier = Some(tier.to_owned());
    config
}

fn options() -> PersonaOptionsResponse {
    fixture("district-persona-options.json")
}

fn conflict(code: &str) -> ApiError {
    ApiError::Conflict(ErrorDetail {
        code: Some(code.to_owned()),
        ..ErrorDetail::default()
    })
}

fn forbidden() -> ApiError {
    ApiError::Forbidden(ErrorDetail::default())
}

fn find(effects: &[Effect], wanted: fn(&Effect) -> bool) -> Ticket {
    ticket(
        effects
            .iter()
            .find(|effect| wanted(effect))
            .unwrap_or_else(|| panic!("{effects:?}")),
    )
}

/// The wizard opened from the overview's card, nothing answered yet: the
/// ticket of the wizard's read and of the persona's.
fn opened() -> (Model, Ticket, Ticket) {
    let (mut model, _) = loaded(AGENCY, "agency");
    let effects = model.update(Event::OpenFinishSetup);
    assert_eq!(signed_in(&model).route, Route::Setup);
    let [
        Effect::LoadSetupWizard {
            ticket: load,
            workspace_id,
        },
        Effect::LoadWorkspaceConfig { ticket: config, .. },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(workspace_id, AGENCY);
    (model, *load, *config)
}

/// The wizard read with `response` and the persona's `language`, and what the
/// step it opened on asked for.
fn ready(response: SetupResponse, language: Option<&str>) -> (Model, Vec<Effect>) {
    let (mut model, load, config) = opened();
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config,
        result: Ok(config_with(json!({"language": language}))),
    });
    let effects = model.update(Event::SetupWizardLoaded {
        ticket: load,
        result: Ok(response),
    });
    (model, effects)
}

/// A step write sent through the shell, and its answer.
fn complete(model: &mut Model, effects: &[Effect], answer: SetupResponse) -> Vec<Effect> {
    let [
        Effect::SaveSetupWizard {
            ticket,
            steps,
            business_facts: None,
            ..
        },
    ] = effects
    else {
        panic!("{effects:?}");
    };
    assert_eq!(steps.len(), 1);
    model.update(Event::SetupWizardSaved {
        ticket: *ticket,
        result: Ok(answer),
    })
}

// ── The shell ─────────────────────────────────────────────────────────────

#[test]
fn the_steps_are_the_webs_in_its_order() {
    assert_eq!(previous_step(SetupStep::Business), None);
    assert_eq!(previous_step(SetupStep::Number), Some(SetupStep::Business));
    assert_eq!(next_step(SetupStep::Calls), Some(SetupStep::Golive));
    assert_eq!(next_step(SetupStep::Golive), None);
    assert_eq!(first_open(None), SetupStep::Business);
    assert_eq!(
        first_open(Some(&state(["done"; 6], json!({})))),
        SetupStep::Business
    );
    assert_eq!(
        first_open(Some(&state(
            ["done", "skipped", "later", "todo", "todo", "todo"],
            json!({})
        ))),
        SetupStep::Callers,
        "a state this build does not know is not todo"
    );
    let titles: Vec<&str> = SetupStep::ALL
        .iter()
        .map(|step| SetupWizard::title(*step))
        .collect();
    assert_eq!(
        titles,
        [
            "Find your business",
            "Your number",
            "Your receptionist",
            "What callers should know",
            "Calls, people and bookings",
            "Test and go live",
        ]
    );
    for step in SetupStep::ALL {
        assert!(!SetupWizard::description(step).is_empty());
        assert_eq!(SetupStep::ALL[SetupWizard::position(step)], step);
    }
    for page in [
        SetupHandOff::SetupPage,
        SetupHandOff::Skills,
        SetupHandOff::Integrations,
    ] {
        assert!(page.path().starts_with("/dashboard/district"));
    }
}

#[test]
fn the_wizard_opens_on_the_first_step_still_to_do() {
    let (model, effects) = ready(
        state(["done", "done", "todo", "todo", "todo", "todo"], json!({})),
        None,
    );
    let shown = wizard(&model);
    assert_eq!(shown.phase, SetupPhase::Ready);
    assert_eq!(shown.active, Some(SetupStep::Receptionist));
    assert_eq!(shown.state(SetupStep::Business), Some(SetupStepState::Done));
    assert!(shown.can_go_back());
    assert!(!shown.is_moving());
    assert_eq!(shown.workspace_name, "Zulu Agency");
    assert!(matches!(
        effects.as_slice(),
        [
            Effect::LoadWorkspaceConfig { .. },
            Effect::LoadPersonaOptions { .. }
        ]
    ));
}

#[test]
fn a_member_is_told_only_the_owner_sets_up_and_an_old_workspace_has_no_wizard() {
    for (error, refusal) in [
        (forbidden(), SetupRefusal::NotOwner),
        (conflict("setup_not_available"), SetupRefusal::NotAvailable),
    ] {
        let (mut model, load, _) = opened();
        model.update(Event::SetupWizardLoaded {
            ticket: load,
            result: Err(error),
        });
        assert_eq!(wizard(&model).phase, SetupPhase::Refused(refusal));
        assert!(!refusal.title().is_empty());
        assert!(!refusal.message().is_empty());
        assert!(
            send(&mut model, SetupEvent::Retry).is_empty(),
            "a refusal is not retried"
        );
        assert!(send(&mut model, SetupEvent::HandOffReturned).is_empty());
    }
    let (mut model, load, _) = opened();
    model.update(Event::SetupWizardLoaded {
        ticket: load,
        result: Ok(serde_json::from_value(json!({})).unwrap()),
    });
    assert_eq!(
        wizard(&model).phase,
        SetupPhase::Refused(SetupRefusal::NotAvailable)
    );
}

#[test]
fn a_failed_read_offers_a_retry_and_a_finished_setup_says_so() {
    let (mut model, load, _) = opened();
    model.update(Event::SetupWizardLoaded {
        ticket: load,
        result: Err(conflict("other")),
    });
    assert!(matches!(wizard(&model).phase, SetupPhase::Failed(_)));
    let effects = send(&mut model, SetupEvent::Retry);
    assert!(matches!(
        effects.as_slice(),
        [
            Effect::LoadSetupWizard { .. },
            Effect::LoadWorkspaceConfig { .. }
        ]
    ));
    assert_eq!(wizard(&model).phase, SetupPhase::Loading);
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&effects[0]),
        result: Ok(finished()),
    });
    assert_eq!(wizard(&model).phase, SetupPhase::Finished);
    assert!(
        send(&mut model, SetupEvent::Skip).is_empty(),
        "no step on screen"
    );
}

#[test]
fn the_window_refresh_retries_a_failed_read_and_rereads_a_wizard_on_screen() {
    let (mut model, load, _) = opened();
    model.update(Event::SetupWizardLoaded {
        ticket: load,
        result: Err(server_error()),
    });
    let effects = model.update(Event::Refresh);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadSetupWizard { .. }))
    );
    let (mut model, _) = ready(
        state(["done", "todo", "todo", "todo", "todo", "todo"], json!({})),
        None,
    );
    let effects = model.update(Event::Refresh);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::LoadSetupWizard { .. }))
    );
}

#[test]
fn skip_marks_the_step_and_moves_on_one_write_at_a_time() {
    let (mut model, _) = ready(
        state(["done", "todo", "todo", "todo", "todo", "todo"], json!({})),
        None,
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
    let effects = send(&mut model, SetupEvent::Skip);
    let [Effect::SaveSetupWizard { steps, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(steps, &[(SetupStep::Number, SetupStepState::Skipped)]);
    assert!(wizard(&model).is_moving());
    assert!(
        send(&mut model, SetupEvent::Skip).is_empty(),
        "single flight"
    );
    assert!(
        send(&mut model, SetupEvent::Back).is_empty(),
        "no move while writing"
    );
    assert!(send(&mut model, SetupEvent::KeepNumber).is_empty());
    let next = complete(
        &mut model,
        &effects,
        state(
            ["done", "skipped", "todo", "todo", "todo", "todo"],
            json!({}),
        ),
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Receptionist));
    assert_eq!(next.len(), 2, "step 3 reads the persona and its options");
}

#[test]
fn keeping_a_number_is_only_for_the_number_step() {
    let (mut model, _) = ready(state(ALL_TODO, json!({})), None);
    assert_eq!(wizard(&model).active, Some(SetupStep::Business));
    assert!(send(&mut model, SetupEvent::KeepNumber).is_empty());
    assert_eq!(wizard(&model).active, Some(SetupStep::Business));
    assert!(!wizard(&model).is_moving());
}

#[test]
fn a_failed_step_write_stays_and_says_why_until_dismissed() {
    let (mut model, _) = ready(
        state(["done", "todo", "todo", "todo", "todo", "todo"], json!({})),
        None,
    );
    let effects = send(&mut model, SetupEvent::KeepNumber);
    let [Effect::SaveSetupWizard { ticket, steps, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(steps, &[(SetupStep::Number, SetupStepState::Done)]);
    model.update(Event::SetupWizardSaved {
        ticket: *ticket,
        result: Err(server_error()),
    });
    assert!(wizard(&model).step_error.is_some());
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
    send(&mut model, SetupEvent::DismissError);
    assert_eq!(wizard(&model).step_error, None);
}

#[test]
fn the_last_step_done_with_one_still_open_goes_to_the_open_one_and_a_completion_finishes() {
    let (mut model, _) = ready(
        state(["done", "done", "done", "done", "done", "todo"], json!({})),
        None,
    );
    let effects = send(&mut model, SetupEvent::Back);
    assert_eq!(wizard(&model).active, Some(SetupStep::Calls));
    assert_eq!(effects.len(), 3);
    for _ in 0..4 {
        send(&mut model, SetupEvent::Back);
    }
    assert_eq!(wizard(&model).active, Some(SetupStep::Business));
    assert!(!wizard(&model).can_go_back());
    assert!(
        send(&mut model, SetupEvent::Back).is_empty(),
        "nothing before the first step"
    );
    // Business skipped: the next step is number.
    let effects = send(&mut model, SetupEvent::Skip);
    complete(
        &mut model,
        &effects,
        state(
            ["skipped", "done", "done", "done", "done", "todo"],
            json!({}),
        ),
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
    let (mut model, _) = ready(
        state(["done", "todo", "done", "done", "done", "todo"], json!({})),
        None,
    );
    let effects = send(&mut model, SetupEvent::KeepNumber);
    complete(
        &mut model,
        &effects,
        state(["done", "done", "done", "done", "done", "todo"], json!({})),
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Receptionist));
    let (mut model, _) = ready(
        state(["done", "done", "done", "done", "done", "todo"], json!({})),
        None,
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Golive));
    let effects = send(&mut model, SetupEvent::GoLive(GoLiveEvent::Finish));
    // A second Finish while the first is on its way writes nothing.
    assert!(send(&mut model, SetupEvent::GoLive(GoLiveEvent::Finish)).is_empty());
    complete(
        &mut model,
        &effects,
        state(["done", "todo", "done", "done", "done", "done"], json!({})),
    );
    assert_eq!(
        wizard(&model).active,
        Some(SetupStep::Number),
        "past the last, the open one"
    );
    let effects = send(&mut model, SetupEvent::KeepNumber);
    complete(&mut model, &effects, finished());
    assert_eq!(wizard(&model).phase, SetupPhase::Finished);
}

#[test]
fn a_hand_off_opens_the_web_and_its_return_moves_on_only_from_a_step_it_finished() {
    let (mut model, _) = ready(
        state(
            ["done", "todo", "todo", "todo", "todo", "todo"],
            json!({"numbersHeld": 0}),
        ),
        None,
    );
    let shown = wizard(&model);
    assert_eq!(shown.number_hand_off(), SetupHandOff::SetupPage);
    assert!(
        send(&mut model, SetupEvent::KeepNumber).is_empty(),
        "no number to keep"
    );
    let effects = send(&mut model, SetupEvent::OpenHandOff(SetupHandOff::SetupPage));
    let [Effect::OpenUrl { url }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(url, "https://www.distronode.com/dashboard/district");
    // A failed re-read keeps the step.
    let effects = send(&mut model, SetupEvent::HandOffReturned);
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&effects[0]),
        result: Err(server_error()),
    });
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
    // Unchanged: stays.
    let effects = send(&mut model, SetupEvent::HandOffReturned);
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&effects[0]),
        result: Ok(state(
            ["done", "todo", "todo", "todo", "todo", "todo"],
            json!({}),
        )),
    });
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
    // A number bought there: moves on.
    let effects = send(&mut model, SetupEvent::HandOffReturned);
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&effects[0]),
        result: Ok(state(
            ["done", "done", "todo", "todo", "todo", "todo"],
            json!({}),
        )),
    });
    assert_eq!(wizard(&model).active, Some(SetupStep::Receptionist));
    // Finished there.
    let effects = send(&mut model, SetupEvent::HandOffReturned);
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&effects[0]),
        result: Ok(finished()),
    });
    assert_eq!(wizard(&model).phase, SetupPhase::Finished);
    // A step already done, reached by Back, stays.
    let (mut model, _) = ready(
        state(["done", "done", "todo", "todo", "todo", "todo"], json!({})),
        None,
    );
    send(&mut model, SetupEvent::Back);
    let effects = send(&mut model, SetupEvent::HandOffReturned);
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&effects[0]),
        result: Ok(state(
            ["done", "done", "done", "todo", "todo", "todo"],
            json!({}),
        )),
    });
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
}

#[test]
fn step_two_says_where_calls_are_answered_and_what_the_plan_includes() {
    for (region, expected) in [
        ("us", Some("Calls are answered in the United States.")),
        ("ca", Some("Calls are answered in Montréal.")),
        ("eu", Some("Calls are answered in Frankfurt.")),
        ("apac", Some("Calls are answered in Singapore.")),
        ("xx", None),
    ] {
        let (model, _) = ready(state(ALL_TODO, json!({"region": region})), None);
        assert_eq!(wizard(&model).answered_in(), expected);
    }
    let (model, _) = ready(state(ALL_TODO, json!({})), None);
    assert_eq!(
        wizard(&model).numbers_line().as_deref(),
        Some("Included in your plan (1 of 3)")
    );
    let (model, _) = ready(state(ALL_TODO, json!({"includedNumbers": null})), None);
    assert_eq!(
        wizard(&model).numbers_line().as_deref(),
        Some("Numbers you hold: 1")
    );
    assert_eq!(SetupWizard::default().numbers_line(), None);
    assert_eq!(SetupWizard::default().answered_in(), None);
    assert_eq!(SetupWizard::default().numbers_held(), 0);
}

#[test]
fn the_number_step_takes_the_web_hand_off_whether_or_not_the_app_buys_in_the_app() {
    for purchases in [false, true] {
        let run = || {
            let (model, _) = ready(state(ALL_TODO, json!({"numbersHeld": 0})), None);
            assert_eq!(wizard(&model).buys_in_app, purchases);
            assert_eq!(wizard(&model).number_hand_off(), SetupHandOff::SetupPage);
        };
        if purchases {
            with_purchases(run)
        } else {
            run()
        }
    }
}

#[test]
fn leaving_the_wizard_drops_what_it_had_on_its_way_and_the_draft_comes_back() {
    let (mut model, _) = ready(state(ALL_TODO, json!({})), Some("fr-CA"));
    assert_eq!(wizard(&model).business.language, SetupLanguage::Fr);
    let effects = send(&mut model, SetupEvent::Skip);
    model.update(Event::Navigate(Route::Overview));
    assert!(
        complete(&mut model, &effects, state(ALL_TODO, json!({}))).is_empty(),
        "dropped"
    );
    let effects = model.update(Event::OpenFinishSetup);
    // The persona's language read for this visit arrives late; the draft
    // kept from the first visit wins.
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&effects[0]),
        result: Ok(state(ALL_TODO, json!({}))),
    });
    model.update(Event::WorkspaceConfigLoaded {
        ticket: ticket(&effects[1]),
        result: Ok(config_with(json!({"language": "en-US"}))),
    });
    assert_eq!(wizard(&model).business.language, SetupLanguage::Fr);
    // Another workspace starts afresh.
    model.update(Event::SelectWorkspace("ws-contract-client".into()));
    assert_eq!(wizard(&model).phase, SetupPhase::Loading);
    assert_eq!(wizard(&model).data, None);
}

#[test]
fn a_re_read_that_lands_before_a_step_write_never_skips_a_step() {
    let (mut model, _) = ready(state(ALL_TODO, json!({})), None);
    assert_eq!(wizard(&model).active, Some(SetupStep::Business));
    let skip = send(&mut model, SetupEvent::Skip);
    // The window comes back while Skip's write is on its way, and the re-read
    // it asks for lands first, with step 1 already skipped.
    let skipped = || {
        state(
            ["skipped", "todo", "todo", "todo", "todo", "todo"],
            json!({}),
        )
    };
    let read = send(&mut model, SetupEvent::HandOffReturned);
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&read[0]),
        result: Ok(skipped()),
    });
    assert_eq!(
        wizard(&model).active,
        Some(SetupStep::Business),
        "the write moves on, not the read"
    );
    complete(&mut model, &skip, skipped());
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
}

#[test]
fn a_re_read_asked_before_a_write_landed_is_dropped_and_one_with_no_progress_keeps_the_step() {
    let (mut model, _) = ready(state(ALL_TODO, json!({})), None);
    let read = send(&mut model, SetupEvent::HandOffReturned);
    let skip = send(&mut model, SetupEvent::Skip);
    let saved = state(
        ["skipped", "todo", "todo", "todo", "todo", "todo"],
        json!({"businessFacts": {"faqs": [{"question": "Parking?", "answer": "Yes.", "enabled": true}]}}),
    );
    complete(&mut model, &skip, saved.clone());
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
    // The re-read asked before the write landed holds the older state.
    let late = model.update(Event::SetupWizardLoaded {
        ticket: ticket(&read[0]),
        result: Ok(state(ALL_TODO, json!({}))),
    });
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(wizard(&model).data, Some(saved.clone()));
    // A re-read with no progress says nothing about the steps.
    let read = send(&mut model, SetupEvent::HandOffReturned);
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&read[0]),
        result: Ok(state(ALL_TODO, json!({"setupProgress": null}))),
    });
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
    assert_eq!(wizard(&model).data, Some(saved));
}

#[test]
fn a_website_hours_read_from_step_four_left_and_reopened_brings_no_draft() {
    let mut model = on_callers(json!({}));
    callers(
        &mut model,
        CallersEvent::SetWebsite("https://example.com/".into()),
    );
    let read = callers(&mut model, CallersEvent::FillHoursFromWebsite);
    // Back to step 3, then on to step 4 again while the read is out.
    send(&mut model, SetupEvent::Back);
    assert_eq!(wizard(&model).active, Some(SetupStep::Receptionist));
    let skip = send(&mut model, SetupEvent::Skip);
    complete(
        &mut model,
        &skip,
        state(
            ["done", "done", "skipped", "todo", "todo", "todo"],
            json!({}),
        ),
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Callers));
    let late = model.update(Event::SetupWebsiteRead {
        ticket: ticket(&read[0]),
        result: Ok(fixture("district-setup-lookup.json")),
    });
    assert!(late.is_empty(), "{late:?}");
    assert!(
        !wizard(&model).callers.hours_draft_pending,
        "the reopened step asked for nothing"
    );
}

#[test]
fn step_six_left_mid_call_or_mid_check_drops_their_answers() {
    let (mut model, _) = on_golive("ca", 1);
    golive(&mut model, GoLiveEvent::SetMobile("+14165550142".into()));
    let call = golive(&mut model, GoLiveEvent::CallMe);
    golive(
        &mut model,
        GoLiveEvent::SetBusinessLine("+14165550123".into()),
    );
    let start = golive(&mut model, GoLiveEvent::StartCheck);
    send(&mut model, SetupEvent::Back);
    assert_eq!(wizard(&model).active, Some(SetupStep::Calls));
    // A pending check's answer would start the poll: off step 6 it starts nothing.
    let late = model.update(Event::SetupForwardingRead {
        ticket: ticket(&start[0]),
        result: Ok(forwarding("district-forwarding-check-start.json")),
    });
    assert!(late.is_empty(), "{late:?}");
    let late = model.update(Event::SetupTestCallPlaced {
        ticket: ticket(&call[0]),
        result: Ok(fixture("district-setup-test-call.json")),
    });
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(wizard(&model).golive.forwarding, None);
    assert_eq!(wizard(&model).golive.call_status, None);
}

#[test]
fn answers_that_arrive_after_the_wizard_was_left_change_nothing() {
    let (mut model, load, _) = opened();
    model.update(Event::Navigate(Route::Overview));
    let late = model.update(Event::SetupWizardLoaded {
        ticket: load,
        result: Ok(state(ALL_TODO, json!({}))),
    });
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(wizard(&model).phase, SetupPhase::Loading);
    assert_eq!(wizard(&model).data, None);
    // Step 6's forwarding read, after the wizard was left.
    let (mut model, effects) = on_golive("ca", 1);
    let read = find(&effects, |e| {
        matches!(e, Effect::LoadSetupForwardingCheck { .. })
    });
    model.update(Event::Navigate(Route::Overview));
    let late = model.update(Event::SetupForwardingRead {
        ticket: read,
        result: Ok(forwarding("district-forwarding-check.json")),
    });
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(wizard(&model).golive.forwarding, None);
    // Step 6's test call, after the wizard was left.
    let (mut model, _) = on_golive("ca", 1);
    golive(&mut model, GoLiveEvent::SetMobile("+14165550142".into()));
    let call = golive(&mut model, GoLiveEvent::CallMe);
    model.update(Event::Navigate(Route::Overview));
    let late = model.update(Event::SetupTestCallPlaced {
        ticket: ticket(&call[0]),
        result: Ok(fixture("district-setup-test-call.json")),
    });
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(wizard(&model).golive.call_status, None);
    // Step 5's connections read, after the wizard was left.
    let (mut model, effects) = ready(
        state(["done", "done", "done", "done", "todo", "todo"], json!({})),
        None,
    );
    let linked = find(&effects, |e| {
        matches!(e, Effect::LoadSetupConnections { .. })
    });
    model.update(Event::Navigate(Route::Overview));
    let late = model.update(Event::SetupConnectionsLoaded {
        ticket: linked,
        result: Ok(fixture("district-connections.json")),
    });
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(wizard(&model).calls.calendar_name, None);
    // Step 4's hours read from the website, after the wizard was left.
    let mut model = on_callers(json!({}));
    callers(
        &mut model,
        CallersEvent::SetWebsite("https://example.com/".into()),
    );
    let read = callers(&mut model, CallersEvent::FillHoursFromWebsite);
    model.update(Event::Navigate(Route::Overview));
    let late = model.update(Event::SetupWebsiteRead {
        ticket: ticket(&read[0]),
        result: Ok(fixture("district-setup-lookup.json")),
    });
    assert!(late.is_empty(), "{late:?}");
    assert!(!wizard(&model).callers.hours_draft_pending);
}

#[test]
fn an_empty_step_one_writes_only_its_empty_address() {
    let step = district_core::BusinessStep::default();
    assert_eq!(
        Value::Object(step.facts_patch(None)),
        json!({"address": {"city": "", "country": ""}})
    );
}

#[test]
fn clearing_what_you_do_when_it_is_the_only_question_clears_the_questions() {
    let what = district_core::setup::wire::what_question(SetupLanguage::En);
    let stored = json!({"faqs": [{"question": what, "answer": "Teeth.", "enabled": true}]});
    let step = district_core::BusinessStep::default();
    let patch = step.facts_patch(stored.as_object());
    assert_eq!(
        patch["faqs"],
        Value::Null,
        "an empty section is sent as null"
    );
    // With another question stored, that one is kept.
    let stored = json!({"faqs": [
        {"question": what, "answer": "Teeth.", "enabled": true},
        {"question": "Parking?", "answer": "Yes.", "enabled": true},
    ]});
    let patch = step.facts_patch(stored.as_object());
    assert_eq!(
        patch["faqs"],
        json!([{"question": "Parking?", "answer": "Yes.", "enabled": true}])
    );
}

#[test]
fn a_provisioned_workspace_name_opens_step_one_with_no_name() {
    let (mut model, restore) = restored();
    let mut list = workspace_list();
    list.workspaces[0].name = "Sam's Communications Workspace".to_owned();
    let effects = model.update(Event::WorkspacesLoaded {
        ticket: restore,
        remembered: Some(AGENCY.to_owned()),
        result: Ok(list),
    });
    model.update(Event::OverviewLoaded {
        ticket: last_ticket(&effects),
        result: Ok(overview(AGENCY, "agency")),
    });
    let effects = model.update(Event::OpenFinishSetup);
    model.update(Event::WorkspaceConfigLoaded {
        ticket: ticket(&effects[1]),
        result: Ok(config_with(json!({}))),
    });
    model.update(Event::SetupWizardLoaded {
        ticket: ticket(&effects[0]),
        result: Ok(state(ALL_TODO, json!({}))),
    });
    assert_eq!(wizard(&model).active, Some(SetupStep::Business));
    assert_eq!(wizard(&model).business.business_name, "");
    assert!(wizard(&model).business.name_error().is_some());
}

#[test]
fn the_persona_language_arriving_after_the_wizard_opens_step_one_on_it() {
    let (mut model, load, config) = opened();
    model.update(Event::SetupWizardLoaded {
        ticket: load,
        result: Ok(state(ALL_TODO, json!({}))),
    });
    assert_eq!(
        wizard(&model).business.language,
        SetupLanguage::En,
        "the app's until then"
    );
    // What the owner typed meanwhile stays.
    business(&mut model, BusinessEvent::SetName("Zulu Dental".into()));
    business(&mut model, BusinessEvent::SetCity("Montreal".into()));
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config,
        result: Ok(config_with(json!({"language": "fr-CA"}))),
    });
    let step = &wizard(&model).business;
    assert_eq!(step.language, SetupLanguage::Fr);
    assert_eq!(
        (step.business_name.as_str(), step.city.as_str()),
        ("Zulu Dental", "Montreal")
    );
    // A language the owner chose stays too.
    let (mut model, load, config) = opened();
    model.update(Event::SetupWizardLoaded {
        ticket: load,
        result: Ok(state(ALL_TODO, json!({}))),
    });
    business(&mut model, BusinessEvent::SetLanguage(SetupLanguage::Fr));
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config,
        result: Ok(config_with(json!({"language": "en-US"}))),
    });
    assert_eq!(wizard(&model).business.language, SetupLanguage::Fr);
    // A failed persona read opens on the app's language, here French.
    l10n::with_locale(Locale::FrCa, || {
        let (mut model, load, config) = opened();
        model.update(Event::WorkspaceConfigLoaded {
            ticket: config,
            result: Err(server_error()),
        });
        model.update(Event::SetupWizardLoaded {
            ticket: load,
            result: Ok(state(ALL_TODO, json!({}))),
        });
        assert_eq!(wizard(&model).business.language, SetupLanguage::Fr);
    });
    // On a step other than the first, the draft is made and the step stays.
    let (mut model, load, config) = opened();
    model.update(Event::SetupWizardLoaded {
        ticket: load,
        result: Ok(state(
            ["done", "todo", "todo", "todo", "todo", "todo"],
            json!({}),
        )),
    });
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config,
        result: Ok(workspace_config()),
    });
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
    assert!(wizard(&model).draft.is_some());
}

// ── Step 1 ────────────────────────────────────────────────────────────────

fn business(model: &mut Model, event: BusinessEvent) -> Vec<Effect> {
    send(model, SetupEvent::Business(event))
}

fn facts_save(effects: &[Effect]) -> (Ticket, serde_json::Map<String, Value>) {
    let [
        Effect::SaveSetupWizard {
            ticket,
            steps,
            business_facts: Some(facts),
            ..
        },
    ] = effects
    else {
        panic!("{effects:?}");
    };
    assert!(steps.is_empty());
    (*ticket, facts.clone())
}

#[test]
fn step_one_opens_on_the_draft_and_the_stored_facts() {
    let facts = json!({"businessFacts": {
        "address": {"line1": "100 Example Street", "city": "Toronto", "region": "on", "country": "CA"},
        "faqs": [{"question": "Que fait l'entreprise?", "answer": "Des dents.", "enabled": false}],
    }});
    let (model, _) = ready(state(ALL_TODO, facts), Some("fr-CA"));
    let step = &wizard(&model).business;
    assert_eq!(step.business_name, "Zulu Agency");
    assert_eq!(step.what, "Des dents.");
    assert_eq!(
        (step.country.as_str(), step.subdivision.as_str()),
        ("CA", "ON")
    );
    assert_eq!(step.street, "100 Example Street");
    assert_eq!(step.subdivision_label(), "Province or territory");
    assert!(step.subdivisions().is_some());
    assert!(step.is_valid());
    // No stored address: the region's country, nothing else.
    let (model, _) = ready(state(ALL_TODO, json!({"region": "us"})), None);
    let step = &wizard(&model).business;
    assert_eq!(step.country, "US");
    assert_eq!(step.subdivision_label(), "State");
    assert_eq!(
        step.subdivision_error(),
        Some("Choose your province or state.")
    );
    assert!(step.city_error().is_some());
    assert!(!step.is_valid());
}

#[test]
fn a_provisioned_name_opens_empty_and_every_field_has_its_error() {
    let (mut model, _) = ready(state(ALL_TODO, json!({"region": "eu"})), None);
    business(
        &mut model,
        BusinessEvent::SetName("Sam's Communications Workspace".into()),
    );
    let step = &wizard(&model).business;
    assert!(step.country_error().is_some());
    assert_eq!(step.subdivision_label(), "Region");
    assert_eq!(step.zone(), None);
    assert_eq!(
        step.timezone_line(None),
        "Choose your country and province or state to set it."
    );
    assert!(step.name_error().is_none());
    business(&mut model, BusinessEvent::SetName(" ".into()));
    assert!(wizard(&model).business.name_error().is_some());
    business(&mut model, BusinessEvent::ChooseCountry("FR".into()));
    let step = &wizard(&model).business;
    assert_eq!(step.subdivision_error(), None);
    assert_eq!(
        step.timezone_line(None),
        "Europe/Paris. You can change it in step 4."
    );
}

#[test]
fn the_time_zone_is_the_stored_one_while_the_place_is_the_stored_place() {
    let facts = json!({"businessFacts": {
        "timezone": "America/Winnipeg",
        "address": {"city": "Toronto", "region": "ON", "country": "CA"},
    }});
    let (mut model, _) = ready(state(ALL_TODO, facts), None);
    let stored = wizard(&model).facts().cloned();
    let step = &wizard(&model).business;
    assert_eq!(
        step.timezone(stored.as_ref()).as_deref(),
        Some("America/Winnipeg")
    );
    assert_eq!(
        step.timezone_line(stored.as_ref()),
        "America/Winnipeg. You can change it in step 4."
    );
    business(&mut model, BusinessEvent::SetCity("Ottawa".into()));
    let step = &wizard(&model).business;
    assert_eq!(
        step.timezone(stored.as_ref()).as_deref(),
        Some("America/Toronto")
    );
    assert_eq!(
        step.timezone_line(stored.as_ref()),
        "America/Toronto. Your area has more than one time zone, so check it in step 4."
    );
    business(&mut model, BusinessEvent::ChooseCountry("CA".into()));
    assert_eq!(
        wizard(&model).business.subdivision,
        "ON",
        "the same country keeps it"
    );
    business(&mut model, BusinessEvent::ChooseCountry("US".into()));
    assert_eq!(wizard(&model).business.subdivision, "");
    business(&mut model, BusinessEvent::SetSubdivision("NY".into()));
    business(&mut model, BusinessEvent::SetStreet("1 Main".into()));
    business(&mut model, BusinessEvent::SetLanguage(SetupLanguage::Fr));
    business(&mut model, BusinessEvent::SetWhat("Teeth".into()));
    let step = &wizard(&model).business;
    assert_eq!(
        (step.street.as_str(), step.language, step.what.as_str()),
        ("1 Main", SetupLanguage::Fr, "Teeth")
    );
}

#[test]
fn reading_the_website_saves_its_services_and_questions_as_drafts_and_never_its_hours() {
    let stored = json!({"businessFacts": {
        "services": [{"name": "cleaning", "enabled": true}],
        "faqs": [{"question": "Parking?", "answer": "Yes.", "enabled": true}],
    }});
    let (mut model, _) = ready(state(ALL_TODO, stored), None);
    assert!(
        business(&mut model, BusinessEvent::ReadWebsite).is_empty(),
        "no address"
    );
    business(
        &mut model,
        BusinessEvent::SetWebsite(" https://example.com/ ".into()),
    );
    business(&mut model, BusinessEvent::SetCity(String::new()));
    business(&mut model, BusinessEvent::SetStreet(String::new()));
    business(&mut model, BusinessEvent::ChooseCountry(String::new()));
    let effects = business(&mut model, BusinessEvent::ReadWebsite);
    let [
        Effect::ReadSetupWebsite {
            ticket: read,
            website_url,
            locale,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(
        (website_url.as_str(), *locale),
        ("https://example.com/", Locale::En)
    );
    assert!(wizard(&model).business.is_busy());
    assert!(wizard(&model).is_moving());
    assert!(
        business(&mut model, BusinessEvent::ReadWebsite).is_empty(),
        "single flight"
    );
    assert!(
        business(&mut model, BusinessEvent::Save).is_empty(),
        "never beside a read"
    );
    let effects = model.update(Event::SetupWebsiteRead {
        ticket: *read,
        result: Ok(fixture("district-setup-lookup.json")),
    });
    let step = &wizard(&model).business;
    assert_eq!(
        (
            step.city.as_str(),
            step.country.as_str(),
            step.subdivision.as_str()
        ),
        ("Toronto", "CA", "ON")
    );
    assert_eq!(step.street, "100 Example Street");
    let (save, facts) = facts_save(&effects);
    assert!(!facts.contains_key("weeklyHours"));
    assert_eq!(
        facts["services"],
        json!([{"name": "cleaning", "enabled": true}, {"name": "Whitening", "enabled": false}])
    );
    assert_eq!(facts["faqs"].as_array().unwrap().len(), 2);
    model.update(Event::SetupWizardSaved {
        ticket: save,
        result: Ok(state(ALL_TODO, json!({}))),
    });
    let step = &wizard(&model).business;
    assert!(!step.is_busy());
    assert_eq!(
        step.notice.as_ref().map(BusinessNotice::message).as_deref(),
        Some(
            "Found 2 services and 1 question. They are saved as drafts for step 4. We found opening hours on your website; confirm them in step 4."
        )
    );
    assert!(matches!(step.notice, Some(BusinessNotice::Found(_))));
    business(&mut model, BusinessEvent::DismissNotice);
    assert_eq!(wizard(&model).business.notice, None);
}

#[test]
fn a_read_that_finds_nothing_only_an_address_or_fails_says_so() {
    let lookup = |draft: Value| -> SetupLookupResponse {
        serde_json::from_value(
            json!({"draft": draft, "source": {"kind": "website", "url": "https://example.com/"}}),
        )
        .unwrap()
    };
    let read = |model: &mut Model| {
        business(model, BusinessEvent::SetWebsite("example.com".into()));
        ticket(&business(model, BusinessEvent::ReadWebsite)[0])
    };
    let (mut model, _) = ready(state(ALL_TODO, json!({})), None);
    let t = read(&mut model);
    model.update(Event::SetupWebsiteRead {
        ticket: t,
        result: Ok(lookup(json!({"weeklyHours": {}}))),
    });
    assert_eq!(
        wizard(&model).business.notice,
        Some(BusinessNotice::Nothing)
    );
    assert!(!BusinessNotice::Nothing.message().is_empty());
    let t = read(&mut model);
    model.update(Event::SetupWebsiteRead {
        ticket: t,
        result: Ok(lookup(
            json!({"address": {"city": "Paris", "country": "FR", "region": "IDF"}}),
        )),
    });
    let step = &wizard(&model).business;
    assert_eq!(
        step.notice,
        Some(BusinessNotice::Found(
            "We filled in your address from your website. Check it below.".into()
        ))
    );
    assert_eq!(step.country, "CA", "a country already chosen stays");
    let t = read(&mut model);
    model.update(Event::SetupWebsiteRead {
        ticket: t,
        result: Err(server_error()),
    });
    assert!(matches!(
        wizard(&model).business.notice,
        Some(BusinessNotice::Failed(_))
    ));
    // Hours alone: nothing to save, and the sentence says hours were found.
    let t = read(&mut model);
    model.update(Event::SetupWebsiteRead {
        ticket: t,
        result: Ok(lookup(
            json!({"weeklyHours": {"mon": [{"open": "09:00", "close": "17:00"}]},
                                  "address": {"country": "CA", "region": "qc"}}),
        )),
    });
    let step = &wizard(&model).business;
    assert_eq!(
        step.subdivision, "QC",
        "an empty province is filled for the same country"
    );
    assert!(!step.is_busy());
    // The drafts' save fails.
    let t = read(&mut model);
    let effects = model.update(Event::SetupWebsiteRead {
        ticket: t,
        result: Ok(fixture("district-setup-lookup.json")),
    });
    let (save, _) = facts_save(&effects);
    model.update(Event::SetupWizardSaved {
        ticket: save,
        result: Err(server_error()),
    });
    assert!(matches!(
        wizard(&model).business.notice,
        Some(BusinessNotice::Failed(_))
    ));
    assert!(!wizard(&model).is_moving());
}

#[test]
fn the_found_sentence_counts_in_both_languages() {
    let (mut model, _) = ready(state(ALL_TODO, json!({})), None);
    let lookup = |services: usize, faqs: usize| -> SetupLookupResponse {
        let services: Vec<Value> = (0..services)
            .map(|n| json!({"name": format!("S{n}"), "enabled": false}))
            .collect();
        let faqs: Vec<Value> = (0..faqs)
            .map(|n| json!({"question": format!("Q{n}"), "answer": "A", "enabled": false}))
            .collect();
        serde_json::from_value(json!({"draft": {"services": services, "faqs": faqs}, "source": {"kind": "website", "url": "u"}})).unwrap()
    };
    let mut said = Vec::new();
    for locale in [Locale::En, Locale::FrCa] {
        for (services, faqs) in [(1, 0), (2, 1), (0, 2)] {
            l10n::with_locale(locale, || {
                business(&mut model, BusinessEvent::SetWebsite("u".into()));
                let t = ticket(&business(&mut model, BusinessEvent::ReadWebsite)[0]);
                let effects = model.update(Event::SetupWebsiteRead {
                    ticket: t,
                    result: Ok(lookup(services, faqs)),
                });
                let (save, _) = facts_save(&effects);
                model.update(Event::SetupWizardSaved {
                    ticket: save,
                    result: Ok(state(ALL_TODO, json!({}))),
                });
                said.push(wizard(&model).business.notice.as_ref().unwrap().message());
            });
        }
    }
    assert!(
        said[0].contains("1 service") && said[0].contains("0 questions"),
        "{said:?}"
    );
    assert!(
        said[1].contains("2 services") && said[1].contains("1 question"),
        "{said:?}"
    );
    assert!(
        said[3].contains("1 service") && said[3].contains("0 question "),
        "{said:?}"
    );
}

#[test]
fn saving_step_one_writes_the_facts_renames_when_the_name_changed_and_moves_on() {
    let stored = json!({"businessFacts": {
        "address": {"city": "Toronto", "region": "ON", "country": "CA", "postalCode": "M5V 1A1"},
        "faqs": [
            {"question": "What does the business do?", "answer": "Old.", "enabled": false},
            {"question": "Parking?", "answer": "Yes.", "enabled": true},
        ],
    }});
    let (mut model, _) = ready(state(ALL_TODO, stored), None);
    business(&mut model, BusinessEvent::SetWhat("Teeth.".into()));
    business(&mut model, BusinessEvent::SetName("  Acme Dental ".into()));
    business(&mut model, BusinessEvent::SetStreet("1 Main".into()));
    let effects = business(&mut model, BusinessEvent::Save);
    let (save, facts) = facts_save(&effects);
    assert_eq!(
        Value::Object(facts),
        json!({
            "timezone": "America/Toronto",
            "address": {"city": "Toronto", "country": "CA", "line1": "1 Main", "region": "ON", "postalCode": "M5V 1A1"},
            "faqs": [
                {"question": "What does the business do?", "answer": "Teeth.", "enabled": false},
                {"question": "Parking?", "answer": "Yes.", "enabled": true},
            ],
        })
    );
    assert!(
        business(&mut model, BusinessEvent::Save).is_empty(),
        "single flight"
    );
    let effects = model.update(Event::SetupWizardSaved {
        ticket: save,
        result: Ok(state(ALL_TODO, json!({}))),
    });
    let [
        Effect::RenameWorkspace {
            ticket: rename,
            name,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(name, "Acme Dental");
    let effects = model.update(Event::WorkspaceRenamed {
        ticket: *rename,
        result: Ok(fixture("district-rename.json")),
    });
    assert_eq!(wizard(&model).workspace_name, "Renamed Workspace");
    let draft = wizard(&model).draft.clone().unwrap();
    assert_eq!(draft.business_name, "Acme Dental");
    complete(
        &mut model,
        &effects,
        state(["done", "todo", "todo", "todo", "todo", "todo"], json!({})),
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Number));
}

#[test]
fn step_one_without_a_rename_or_a_what_and_its_failures() {
    let stored = json!({"businessFacts": {
        "address": {"city": "Toronto", "region": "ON", "country": "CA"},
        "faqs": [{"question": "Que fait l'entreprise?", "answer": "Old.", "enabled": true},
                 {"question": "Parking?", "answer": "Yes.", "enabled": true}],
    }});
    let (mut model, _) = ready(state(ALL_TODO, stored.clone()), None);
    business(&mut model, BusinessEvent::SetWhat(" ".into()));
    let (save, facts) = facts_save(&business(&mut model, BusinessEvent::Save));
    assert_eq!(
        facts["faqs"],
        json!([{"question": "Parking?", "answer": "Yes.", "enabled": true}])
    );
    let effects = model.update(Event::SetupWizardSaved {
        ticket: save,
        result: Ok(state(ALL_TODO, json!({}))),
    });
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::SaveSetupWizard {
                business_facts: None,
                ..
            }]
        ),
        "no rename"
    );
    // A failed facts save, and a failed rename.
    let (mut model, _) = ready(state(ALL_TODO, stored), None);
    let (save, _) = facts_save(&business(&mut model, BusinessEvent::Save));
    model.update(Event::SetupWizardSaved {
        ticket: save,
        result: Err(server_error()),
    });
    assert!(wizard(&model).business.save_error.is_some());
    assert!(!wizard(&model).is_moving());
    business(&mut model, BusinessEvent::SetName("Other".into()));
    let (save, _) = facts_save(&business(&mut model, BusinessEvent::Save));
    let effects = model.update(Event::SetupWizardSaved {
        ticket: save,
        result: Ok(state(ALL_TODO, json!({}))),
    });
    model.update(Event::WorkspaceRenamed {
        ticket: ticket(&effects[0]),
        result: Err(server_error()),
    });
    assert!(wizard(&model).business.save_error.is_some());
    // An invalid form sends nothing and shows its errors.
    business(&mut model, BusinessEvent::SetCity(String::new()));
    assert!(business(&mut model, BusinessEvent::Save).is_empty());
    assert!(wizard(&model).business.show_errors);
}

#[test]
fn what_you_do_is_refused_at_the_question_cap_and_a_new_one_starts_on() {
    let faqs: Vec<Value> = (0..50)
        .map(|n| json!({"question": format!("Q{n}"), "answer": "A", "enabled": true}))
        .collect();
    let (mut model, _) = ready(
        state(
            ALL_TODO,
            json!({"businessFacts": {"address": {"city": "Toronto", "region": "ON", "country": "CA"}, "faqs": faqs}}),
        ),
        None,
    );
    business(&mut model, BusinessEvent::SetWhat("Teeth".into()));
    assert!(business(&mut model, BusinessEvent::Save).is_empty());
    assert!(wizard(&model).business.save_error.is_some());
    let (mut model, _) = ready(
        state(
            ALL_TODO,
            json!({"businessFacts": {"address": {"city": "Toronto", "region": "ON", "country": "CA"}}}),
        ),
        None,
    );
    business(&mut model, BusinessEvent::SetWhat("x".repeat(1200)));
    business(&mut model, BusinessEvent::SetLanguage(SetupLanguage::Fr));
    let (_, facts) = facts_save(&business(&mut model, BusinessEvent::Save));
    let what = &facts["faqs"][0];
    assert_eq!(what["question"], "Que fait l'entreprise?");
    assert_eq!(what["enabled"], true);
    assert_eq!(what["answer"].as_str().unwrap().len(), 1000);
}

#[test]
fn a_step_one_without_a_name_is_not_saved() {
    let stored = json!({"businessFacts": {
        "address": {"city": "Toronto", "region": "ON", "country": "CA"},
    }});
    let (mut model, _) = ready(state(ALL_TODO, stored), None);
    assert!(wizard(&model).business.is_valid());
    business(&mut model, BusinessEvent::SetName("  ".into()));
    assert!(!wizard(&model).business.is_valid());
    assert!(business(&mut model, BusinessEvent::Save).is_empty());
    assert!(wizard(&model).business.show_errors);
    assert!(!wizard(&model).is_moving());
}

#[test]
fn another_province_in_the_stored_country_takes_its_own_time_zone() {
    let facts = json!({"businessFacts": {
        "timezone": "America/Winnipeg",
        "address": {"city": "Toronto", "region": "ON", "country": "CA"},
    }});
    let (mut model, _) = ready(state(ALL_TODO, facts), None);
    let stored = wizard(&model).facts().cloned();
    business(&mut model, BusinessEvent::SetSubdivision("QC".into()));
    assert_eq!(
        wizard(&model).business.timezone(stored.as_ref()).as_deref(),
        Some("America/Toronto")
    );
}

#[test]
fn a_website_address_fills_only_what_is_empty_and_a_country_this_app_knows() {
    let (mut model, _) = ready(state(ALL_TODO, json!({})), None);
    business(&mut model, BusinessEvent::SetStreet("1 Main".into()));
    business(&mut model, BusinessEvent::SetCity(String::new()));
    business(&mut model, BusinessEvent::ChooseCountry(String::new()));
    business(&mut model, BusinessEvent::SetWebsite("example.com".into()));
    let read = ticket(&business(&mut model, BusinessEvent::ReadWebsite)[0]);
    model.update(Event::SetupWebsiteRead {
        ticket: read,
        result: Ok(serde_json::from_value(json!({
            "draft": {"address": {"line1": "9 Other Road", "country": "ZZ", "region": "Nowhere"}},
            "source": {"kind": "website", "url": "https://example.com/"},
        }))
        .unwrap()),
    });
    let step = &wizard(&model).business;
    assert_eq!(step.street, "1 Main", "a typed street stays");
    assert_eq!(step.city, "", "no city to fill it with");
    assert_eq!(
        (step.country.as_str(), step.subdivision.as_str()),
        ("", ""),
        "a country this app does not list is not chosen"
    );
    assert!(matches!(step.notice, Some(BusinessNotice::Found(_))));
}

// ── Step 3 ────────────────────────────────────────────────────────────────

fn receptionist(model: &mut Model, event: ReceptionistEvent) -> Vec<Effect> {
    send(model, SetupEvent::Receptionist(event))
}

/// Step 3 open, its two reads answered.
fn on_receptionist(
    persona: Value,
    options: Result<PersonaOptionsResponse, ApiError>,
    region: &str,
) -> Model {
    let (mut model, effects) = ready(
        state(
            ["done", "done", "todo", "todo", "todo", "todo"],
            json!({"region": region}),
        ),
        None,
    );
    let config = find(&effects, |e| {
        matches!(e, Effect::LoadWorkspaceConfig { .. })
    });
    let listed = find(&effects, |e| matches!(e, Effect::LoadPersonaOptions { .. }));
    model.update(Event::PersonaOptionsLoaded {
        ticket: listed,
        result: options,
    });
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config,
        result: Ok(config_with(persona)),
    });
    model
}

#[test]
fn step_three_replaces_a_starter_greeting_and_offers_the_engines_voices() {
    let model = on_receptionist(
        json!({"modelId": "deepgram-pipeline", "voice": "aura-2-luna-en", "language": "en-US",
               "greeting": "Thanks for calling. This is Ava. How can I help you today?"}),
        Ok(options()),
        "eu",
    );
    let step = &wizard(&model).receptionist;
    assert_eq!(step.phase, StepPhase::Ready);
    assert_eq!(step.name, "Ava");
    assert_eq!(
        step.greeting,
        "Thanks for calling Zulu Agency, this is Ava. How can I help?"
    );
    assert!(!step.greeting_edited);
    assert_eq!(step.voice.as_deref(), Some("aura-2-luna-en"));
    assert_eq!(step.voice_groups.len(), 2);
    assert_eq!(
        wizard(&model).disclosure_line(),
        "This is required in Europe."
    );
    let other = on_receptionist(json!({}), Ok(options()), "ca");
    assert_eq!(
        wizard(&other).disclosure_line(),
        "Your receptionist tells callers it is an AI assistant at the start of every call."
    );
}

#[test]
fn step_three_keeps_an_owners_greeting_and_falls_back_on_the_default_voice() {
    let model = on_receptionist(
        json!({"name": "Léa", "modelId": "deepgram-pipeline", "voice": "made-up", "language": "en-US",
               "greeting": "Hello from us."}),
        Ok(options()),
        "ca",
    );
    let step = &wizard(&model).receptionist;
    assert_eq!(step.greeting, "Hello from us.");
    assert!(step.greeting_edited);
    assert_eq!(step.voice.as_deref(), Some("aura-2-asteria-en"));
    // No engine, or no options: no voices.
    for (persona, options) in [
        (json!({"language": "en-US"}), Ok(options())),
        (json!({"modelId": "deepgram-pipeline"}), Err(server_error())),
    ] {
        let model = on_receptionist(persona, options, "ca");
        let step = &wizard(&model).receptionist;
        assert!(step.voice_groups.is_empty());
        assert_eq!(step.voice, None);
    }
    // An engine with voices but no default: the first offered.
    let mut no_default = options();
    no_default.defaults.voice_by_engine.clear();
    let model = on_receptionist(
        json!({"modelId": "aws-pipeline", "language": "en-US"}),
        Ok(no_default),
        "ca",
    );
    assert_eq!(wizard(&model).receptionist.voice.as_deref(), Some("Joanna"));
    let mut none = options();
    none.defaults.voice_by_engine.clear();
    none.voices
        .retain(|catalog| catalog.engine != "aws-pipeline");
    let model = on_receptionist(
        json!({"modelId": "aws-pipeline", "language": "en-US"}),
        Ok(none),
        "ca",
    );
    assert_eq!(wizard(&model).receptionist.voice, None);
}

#[test]
fn the_greeting_follows_the_name_until_the_owner_writes_it_and_the_save_sends_only_its_fields() {
    let mut model = on_receptionist(
        json!({"modelId": "deepgram-pipeline", "language": "hi-IN"}),
        Ok(options()),
        "ca",
    );
    receptionist(&mut model, ReceptionistEvent::SetName("Max".into()));
    assert_eq!(
        wizard(&model).receptionist.greeting,
        "Thanks for calling Zulu Agency, this is Max. How can I help?"
    );
    receptionist(&mut model, ReceptionistEvent::SetName(" ".into()));
    assert!(wizard(&model).receptionist.greeting.contains("this is Ava"));
    assert!(wizard(&model).receptionist.name_error().is_some());
    assert!(receptionist(&mut model, ReceptionistEvent::Save).is_empty());
    receptionist(&mut model, ReceptionistEvent::SetName("Max".into()));
    receptionist(&mut model, ReceptionistEvent::SetGreeting(" ".into()));
    assert!(wizard(&model).receptionist.greeting_error().is_some());
    receptionist(&mut model, ReceptionistEvent::SetName("Moe".into()));
    assert_eq!(
        wizard(&model).receptionist.greeting,
        " ",
        "written by the owner now"
    );
    receptionist(&mut model, ReceptionistEvent::SetGreeting("Hi.".into()));
    receptionist(&mut model, ReceptionistEvent::SetVoice("x".into()));
    let effects = receptionist(&mut model, ReceptionistEvent::Save);
    let [
        Effect::SavePersona {
            ticket: save,
            patch,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(patch.name.as_deref(), Some("Moe"));
    assert_eq!(patch.greeting.as_deref(), Some("Hi."));
    assert_eq!(
        patch.voice, None,
        "the engine offers no hi-IN voices, so none is sent"
    );
    assert_eq!(
        patch.language.as_deref(),
        Some("hi-IN"),
        "a language outside both families is kept"
    );
    assert!(
        receptionist(&mut model, ReceptionistEvent::Save).is_empty(),
        "single flight"
    );
    model.update(Event::SettingsWritten {
        ticket: *save,
        result: Err(server_error()),
    });
    assert!(wizard(&model).receptionist.save_error.is_some());
    let effects = receptionist(&mut model, ReceptionistEvent::Save);
    let effects = model.update(Event::SettingsWritten {
        ticket: ticket(&effects[0]),
        result: Ok(()),
    });
    complete(
        &mut model,
        &effects,
        state(["done", "done", "done", "todo", "todo", "todo"], json!({})),
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Callers));
}

#[test]
fn the_voice_is_sent_when_voices_are_offered_and_a_failed_read_retries() {
    let mut model = on_receptionist(
        json!({"modelId": "deepgram-pipeline", "language": "en-US"}),
        Ok(options()),
        "ca",
    );
    receptionist(
        &mut model,
        ReceptionistEvent::SetVoice("aura-2-luna-en".into()),
    );
    let effects = receptionist(&mut model, ReceptionistEvent::Save);
    let [Effect::SavePersona { patch, .. }] = effects.as_slice() else {
        panic!()
    };
    assert_eq!(patch.voice.as_deref(), Some("aura-2-luna-en"));
    let (mut model, effects) = ready(
        state(["done", "done", "todo", "todo", "todo", "todo"], json!({})),
        None,
    );
    assert!(
        receptionist(&mut model, ReceptionistEvent::Retry).is_empty(),
        "nothing failed"
    );
    assert!(
        receptionist(&mut model, ReceptionistEvent::Save).is_empty(),
        "not read yet"
    );
    let config = find(&effects, |e| {
        matches!(e, Effect::LoadWorkspaceConfig { .. })
    });
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config,
        result: Err(server_error()),
    });
    assert!(matches!(
        wizard(&model).receptionist.phase,
        StepPhase::Failed(_)
    ));
    let listed = find(&effects, |e| matches!(e, Effect::LoadPersonaOptions { .. }));
    model.update(Event::PersonaOptionsLoaded {
        ticket: listed,
        result: Ok(options()),
    });
    assert!(
        matches!(wizard(&model).receptionist.phase, StepPhase::Failed(_)),
        "a late answer keeps the failure"
    );
    assert_eq!(receptionist(&mut model, ReceptionistEvent::Retry).len(), 2);
}

#[test]
fn a_starter_greeting_is_recognised_in_both_languages() {
    use district_core::ReceptionistStep;
    assert!(ReceptionistStep::is_starter("", "Ava", "Acme"));
    assert!(ReceptionistStep::is_starter(
        "Merci de votre appel. Ici Ava. Comment puis-je vous aider aujourd'hui?",
        "Ava",
        "Acme"
    ));
    assert!(ReceptionistStep::is_starter(
        "Merci d'avoir appelé, ici Léa. Comment puis-je vous aider?",
        "Léa",
        "Acme"
    ));
    assert!(ReceptionistStep::is_starter(
        "Thanks for calling Acme, this is Léa. How can I help?",
        "Léa",
        "Acme"
    ));
    assert!(!ReceptionistStep::is_starter("Hello.", "Léa", "Acme"));
}

#[test]
fn step_three_is_not_saved_before_its_reads_land() {
    let (mut model, _) = ready(
        state(["done", "done", "todo", "todo", "todo", "todo"], json!({})),
        None,
    );
    receptionist(&mut model, ReceptionistEvent::SetName("Ava".into()));
    receptionist(&mut model, ReceptionistEvent::SetGreeting("Hello.".into()));
    assert!(receptionist(&mut model, ReceptionistEvent::Save).is_empty());
    let step = &wizard(&model).receptionist;
    assert_eq!(step.phase, StepPhase::Loading);
    assert!(!step.saving);
}

// ── Step 4 ────────────────────────────────────────────────────────────────

fn callers(model: &mut Model, event: CallersEvent) -> Vec<Effect> {
    send(model, SetupEvent::Callers(event))
}

fn on_callers(facts: Value) -> Model {
    ready(
        state(
            ["done", "done", "done", "todo", "todo", "todo"],
            json!({"businessFacts": facts}),
        ),
        None,
    )
    .0
}

#[test]
fn step_four_opens_on_the_stored_facts_and_offers_the_countrys_zones() {
    let model = on_callers(json!({
        "timezone": "America/Halifax",
        "address": {"city": "Toronto", "country": "ca"},
        "weeklyHours": {"mon": [{"open": "09:00", "close": "17:00"}], "sun": "closed"},
        "services": [{"name": "Cleaning", "enabled": false}],
        "faqs": [],
    }));
    let step = &wizard(&model).callers;
    assert_eq!(step.timezone, "America/Halifax");
    assert_eq!(step.hours[0].state, DayState::Open);
    assert_eq!(step.hours[6].state, DayState::Closed);
    assert!(step.has_drafts());
    assert!(step.is_valid());
    let zones = signed_in(&model).callers_zones();
    assert!(
        zones.contains(&"America/Halifax".to_owned())
            && zones.contains(&"America/Vancouver".to_owned())
    );
    // A question switched off is a draft too.
    let model = on_callers(json!({
        "services": [{"name": "Cleaning", "enabled": true}],
        "faqs": [{"question": "Parking?", "answer": "Behind the building.", "enabled": false}],
    }));
    assert!(wizard(&model).callers.has_drafts());
    // No address: the draft's country, the workspace's region.
    let model = on_callers(json!({}));
    assert!(!wizard(&model).callers.has_drafts());
    assert_eq!(
        signed_in(&model).callers_zones(),
        places::zones_with("CA", &wizard(&model).callers.timezone)
    );
}

#[test]
fn days_periods_services_and_questions_are_edited_within_the_limits() {
    let mut model = on_callers(json!({}));
    callers(
        &mut model,
        CallersEvent::SetDayState(Weekday::Tue, DayState::Open),
    );
    assert_eq!(
        wizard(&model).callers.hours[1].intervals,
        [Interval::STANDARD]
    );
    callers(
        &mut model,
        CallersEvent::SetDayState(Weekday::Tue, DayState::Closed),
    );
    callers(
        &mut model,
        CallersEvent::SetDayState(Weekday::Tue, DayState::Open),
    );
    assert_eq!(
        wizard(&model).callers.hours[1].intervals.len(),
        1,
        "kept, not doubled"
    );
    callers(&mut model, CallersEvent::AddPeriod(Weekday::Mon));
    assert!(
        wizard(&model).callers.hours[0].intervals.is_empty(),
        "a day not open takes no period"
    );
    for _ in 0..5 {
        callers(&mut model, CallersEvent::AddPeriod(Weekday::Tue));
    }
    let tuesday = &wizard(&model).callers.hours[1];
    assert_eq!(tuesday.intervals.len(), 4);
    assert_eq!(
        tuesday.intervals[1],
        Interval {
            open: 18 * 60,
            close: 19 * 60
        }
    );
    callers(
        &mut model,
        CallersEvent::SetPeriod(
            Weekday::Tue,
            3,
            Interval {
                open: 1380,
                close: 1440,
            },
        ),
    );
    callers(
        &mut model,
        CallersEvent::SetPeriod(Weekday::Tue, 9, Interval::STANDARD),
    );
    callers(&mut model, CallersEvent::AddPeriod(Weekday::Wed));
    for index in [9, 3, 2, 1, 0] {
        callers(&mut model, CallersEvent::RemovePeriod(Weekday::Tue, index));
    }
    assert_eq!(wizard(&model).callers.hours[1].state, DayState::NotSet);
    // A late day's next period starts when the last closes and stops at
    // midnight; a day open until midnight takes none.
    callers(
        &mut model,
        CallersEvent::SetDayState(Weekday::Fri, DayState::Open),
    );
    callers(
        &mut model,
        CallersEvent::SetPeriod(
            Weekday::Fri,
            0,
            Interval {
                open: 1260,
                close: 1410,
            },
        ),
    );
    callers(&mut model, CallersEvent::AddPeriod(Weekday::Fri));
    let friday = &wizard(&model).callers.hours[4];
    assert_eq!(
        friday.intervals[1],
        Interval {
            open: 1410,
            close: 1440
        }
    );
    assert!(facts::day_is_valid(friday), "no overlap");
    callers(&mut model, CallersEvent::AddPeriod(Weekday::Fri));
    assert_eq!(wizard(&model).callers.hours[4].intervals.len(), 2);
    for _ in 0..51 {
        callers(&mut model, CallersEvent::AddService);
        callers(&mut model, CallersEvent::AddFaq);
    }
    assert_eq!(wizard(&model).callers.services.len(), 50);
    assert_eq!(wizard(&model).callers.faqs.len(), 50);
    assert!(wizard(&model).callers.services.iter().all(|s| s.enabled));
    callers(
        &mut model,
        CallersEvent::SetService(
            0,
            ServiceDraft {
                name: "Cut".into(),
                ..ServiceDraft::default()
            },
        ),
    );
    callers(
        &mut model,
        CallersEvent::SetService(99, ServiceDraft::default()),
    );
    callers(
        &mut model,
        CallersEvent::SetFaq(
            0,
            FaqDraft {
                question: "Q".into(),
                answer: "A".into(),
                enabled: false,
            },
        ),
    );
    callers(&mut model, CallersEvent::SetFaq(99, FaqDraft::default()));
    for _ in 0..51 {
        callers(&mut model, CallersEvent::RemoveService(0));
        callers(&mut model, CallersEvent::RemoveFaq(0));
    }
    assert!(wizard(&model).callers.services.is_empty() && wizard(&model).callers.faqs.is_empty());
    callers(
        &mut model,
        CallersEvent::SetTimezone("America/Regina".into()),
    );
    callers(
        &mut model,
        CallersEvent::SetAddress(AddressDraft {
            city: "Regina".into(),
            country: "CA".into(),
            ..AddressDraft::default()
        }),
    );
    assert_eq!(wizard(&model).callers.timezone, "America/Regina");
}

#[test]
fn hours_from_the_website_are_a_draft_until_used_and_continue_waits_for_them() {
    let mut model = on_callers(json!({"weeklyHours": {"sat": "closed"}}));
    assert!(
        callers(&mut model, CallersEvent::FillHoursFromWebsite).is_empty(),
        "no address"
    );
    callers(&mut model, CallersEvent::SetWebsite("example.com".into()));
    let effects = callers(&mut model, CallersEvent::FillHoursFromWebsite);
    let [Effect::ReadSetupWebsite { ticket: read, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert!(
        callers(&mut model, CallersEvent::FillHoursFromWebsite).is_empty(),
        "single flight"
    );
    model.update(Event::SetupWebsiteRead {
        ticket: *read,
        result: Ok(fixture("district-setup-lookup.json")),
    });
    let step = &wizard(&model).callers;
    assert!(step.hours_draft_pending);
    assert_eq!(step.hours[0].state, DayState::Open);
    assert!(
        !step.patch().contains_key("weeklyHours"),
        "a draft is not saved"
    );
    assert!(callers(&mut model, CallersEvent::SaveAndContinue).is_empty());
    assert!(wizard(&model).callers.save_error.is_some());
    callers(&mut model, CallersEvent::DiscardDraftHours);
    assert_eq!(
        wizard(&model).callers.hours[5].state,
        DayState::Closed,
        "put back"
    );
    callers(&mut model, CallersEvent::DiscardDraftHours);
    // Read twice: the first week is what comes back.
    for _ in 0..2 {
        let t = ticket(&callers(&mut model, CallersEvent::FillHoursFromWebsite)[0]);
        model.update(Event::SetupWebsiteRead {
            ticket: t,
            result: Ok(fixture("district-setup-lookup.json")),
        });
    }
    callers(&mut model, CallersEvent::DiscardDraftHours);
    assert_eq!(wizard(&model).callers.hours[5].state, DayState::Closed);
    let t = ticket(&callers(&mut model, CallersEvent::FillHoursFromWebsite)[0]);
    model.update(Event::SetupWebsiteRead {
        ticket: t,
        result: Ok(fixture("district-setup-lookup.json")),
    });
    callers(&mut model, CallersEvent::UseDraftHours);
    assert!(wizard(&model).callers.patch().contains_key("weeklyHours"));
    // None found, and a failed read.
    let t = ticket(&callers(&mut model, CallersEvent::FillHoursFromWebsite)[0]);
    model.update(Event::SetupWebsiteRead {
        ticket: t,
        result: Ok(serde_json::from_value(
            json!({"draft": {}, "source": {"kind": "website", "url": "u"}}),
        )
        .unwrap()),
    });
    assert_eq!(
        wizard(&model).callers.hours_notice.as_deref(),
        Some("We did not find opening hours on that website. Enter them below.")
    );
    let t = ticket(&callers(&mut model, CallersEvent::FillHoursFromWebsite)[0]);
    model.update(Event::SetupWebsiteRead {
        ticket: t,
        result: Err(server_error()),
    });
    assert!(wizard(&model).callers.hours_notice.is_some());
    // Editing a day confirms a draft.
    let t = ticket(&callers(&mut model, CallersEvent::FillHoursFromWebsite)[0]);
    model.update(Event::SetupWebsiteRead {
        ticket: t,
        result: Ok(fixture("district-setup-lookup.json")),
    });
    callers(
        &mut model,
        CallersEvent::SetDayState(Weekday::Sun, DayState::Closed),
    );
    assert!(!wizard(&model).callers.hours_draft_pending);
}

#[test]
fn step_four_saves_all_five_sections_and_continue_moves_on() {
    let mut model = on_callers(json!({"timezone": "America/Toronto"}));
    let effects = callers(&mut model, CallersEvent::Save);
    let (save, facts) = facts_save(&effects);
    assert_eq!(
        Value::Object(facts),
        json!({"address": null, "services": null, "faqs": null, "timezone": "America/Toronto", "weeklyHours": null})
    );
    assert!(
        callers(&mut model, CallersEvent::Save).is_empty(),
        "single flight"
    );
    assert!(
        callers(&mut model, CallersEvent::SaveAndContinue).is_empty(),
        "single flight"
    );
    let effects = model.update(Event::SetupWizardSaved {
        ticket: save,
        result: Ok(state(
            ["done", "done", "done", "todo", "todo", "todo"],
            json!({}),
        )),
    });
    assert!(effects.is_empty(), "Save stays");
    assert!(wizard(&model).callers.saved);
    let (save, _) = facts_save(&callers(&mut model, CallersEvent::SaveAndContinue));
    model.update(Event::SetupWizardSaved {
        ticket: save,
        result: Err(server_error()),
    });
    assert!(wizard(&model).callers.save_error.is_some());
    let (save, _) = facts_save(&callers(&mut model, CallersEvent::SaveAndContinue));
    let effects = model.update(Event::SetupWizardSaved {
        ticket: save,
        result: Ok(state(
            ["done", "done", "done", "todo", "todo", "todo"],
            json!({}),
        )),
    });
    complete(
        &mut model,
        &effects,
        state(["done", "done", "done", "done", "todo", "todo"], json!({})),
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Calls));
    // An invalid form is refused with its sentence.
    let mut model = on_callers(json!({}));
    callers(&mut model, CallersEvent::AddService);
    assert!(callers(&mut model, CallersEvent::Save).is_empty());
    assert!(wizard(&model).callers.save_error.is_some());
}

#[test]
fn an_address_without_a_city_or_a_day_closing_before_it_opens_is_refused() {
    let mut model = on_callers(json!({}));
    callers(
        &mut model,
        CallersEvent::SetAddress(AddressDraft {
            country: "CA".into(),
            ..AddressDraft::default()
        }),
    );
    assert!(!wizard(&model).callers.is_valid());
    assert!(callers(&mut model, CallersEvent::Save).is_empty());
    assert!(wizard(&model).callers.save_error.is_some());

    callers(
        &mut model,
        CallersEvent::SetAddress(AddressDraft {
            city: "Regina".into(),
            country: "CA".into(),
            ..AddressDraft::default()
        }),
    );
    assert!(wizard(&model).callers.is_valid());
    callers(
        &mut model,
        CallersEvent::SetDayState(Weekday::Mon, DayState::Open),
    );
    callers(
        &mut model,
        CallersEvent::SetPeriod(
            Weekday::Mon,
            0,
            Interval {
                open: 17 * 60,
                close: 9 * 60,
            },
        ),
    );
    assert!(!wizard(&model).callers.is_valid());
    assert!(callers(&mut model, CallersEvent::Save).is_empty());
}

// ── Step 5 ────────────────────────────────────────────────────────────────

fn calls(model: &mut Model, event: SetupCallsEvent) -> Vec<Effect> {
    send(model, SetupEvent::Calls(event))
}

fn handling(mode: &str, seconds: i64) -> CallHandlingResponse {
    CallHandlingResponse {
        success: true,
        call_handling: mode.into(),
        app_ring_seconds: seconds,
    }
}

fn on_calls(
    mode: &str,
    config: Result<WorkspaceConfigResponse, ApiError>,
    connections: Result<ConnectionsResponse, ApiError>,
) -> (Model, Vec<Effect>) {
    let (mut model, effects) = ready(
        state(["done", "done", "done", "done", "todo", "todo"], json!({})),
        None,
    );
    let read = find(&effects, |e| matches!(e, Effect::LoadCallHandling { .. }));
    let config_read = find(&effects, |e| {
        matches!(e, Effect::LoadWorkspaceConfig { .. })
    });
    let linked = find(&effects, |e| {
        matches!(e, Effect::LoadSetupConnections { .. })
    });
    model.update(Event::SetupConnectionsLoaded {
        ticket: linked,
        result: connections,
    });
    model.update(Event::CallHandlingLoaded {
        ticket: read,
        result: Ok(handling(mode, 20)),
    });
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config_read,
        result: config,
    });
    (model, effects)
}

#[test]
fn step_five_reads_who_answers_the_transfer_number_the_calendar_and_profiles() {
    let (model, _) = on_calls(
        "ai_then_app",
        Ok(studio_config("VoiceStudio")),
        Ok(fixture("district-connections.json")),
    );
    let step = &wizard(&model).calls;
    assert_eq!(step.phase, StepPhase::Ready);
    assert_eq!(step.modes(), ["ai_first", "app_first", "ai_then_app"]);
    assert_eq!(step.transfer_number.as_deref(), Some("+14165550123"));
    assert_eq!(step.calendar_name, Some("Google Calendar"));
    assert!(step.profiles_offered && step.profiles_on);
    assert!(!step.answering_changed());
    let (model, _) = on_calls(
        "ai_first",
        Ok(fixture("district-workspace-config-sparse.json")),
        Ok(fixture("district-connections-viewer.json")),
    );
    let step = &wizard(&model).calls;
    assert_eq!(step.modes(), ["ai_first", "app_first"]);
    assert_eq!(step.transfer_number, None);
    assert_eq!(step.calendar_name, None);
    assert!(!step.profiles_offered && !step.profiles_on);
    let (model, _) = on_calls("ai_first", Ok(workspace_config()), Err(server_error()));
    assert_eq!(wizard(&model).calls.calendar_name, None);
    let mut resolved: ConnectionsResponse = fixture("district-connections.json");
    resolved.calendar.as_mut().unwrap().active_provider = Some("microsoft".into());
    let (model, _) = on_calls("ai_first", Ok(workspace_config()), Ok(resolved));
    assert_eq!(
        wizard(&model).calls.calendar_name,
        None,
        "the Microsoft row is in error"
    );
}

#[test]
fn a_failed_read_fails_the_step_and_retries() {
    let (mut model, _) = on_calls(
        "ai_first",
        Err(server_error()),
        Ok(fixture("district-connections.json")),
    );
    assert!(matches!(wizard(&model).calls.phase, StepPhase::Failed(_)));
    assert!(calls(&mut model, SetupCallsEvent::Continue).is_empty());
    assert_eq!(calls(&mut model, SetupCallsEvent::Retry).len(), 3);
    assert!(
        calls(&mut model, SetupCallsEvent::Retry).is_empty(),
        "not failed now"
    );
    let (mut model, effects) = ready(
        state(["done", "done", "done", "done", "todo", "todo"], json!({})),
        None,
    );
    let read = find(&effects, |e| matches!(e, Effect::LoadCallHandling { .. }));
    model.update(Event::CallHandlingLoaded {
        ticket: read,
        result: Err(server_error()),
    });
    let config_read = find(&effects, |e| {
        matches!(e, Effect::LoadWorkspaceConfig { .. })
    });
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config_read,
        result: Ok(workspace_config()),
    });
    assert!(matches!(wizard(&model).calls.phase, StepPhase::Failed(_)));
}

#[test]
fn who_answers_is_saved_with_only_what_changed_and_continue_waits_for_it() {
    let (mut model, _) = on_calls(
        "ai_first",
        Ok(workspace_config()),
        Ok(fixture("district-connections.json")),
    );
    assert!(
        calls(&mut model, SetupCallsEvent::SaveAnswering).is_empty(),
        "nothing changed"
    );
    calls(&mut model, SetupCallsEvent::SetMode("app_first".into()));
    calls(&mut model, SetupCallsEvent::SetRing(90));
    let effects = calls(&mut model, SetupCallsEvent::SaveAnswering);
    let [
        Effect::SaveCallHandling {
            ticket: save,
            patch,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}")
    };
    assert_eq!(patch.call_handling, Some(CallHandlingMode::AppFirst));
    assert_eq!(patch.app_ring_seconds, Some(30));
    assert!(
        calls(&mut model, SetupCallsEvent::SaveAnswering).is_empty(),
        "single flight"
    );
    model.update(Event::CallHandlingLoaded {
        ticket: *save,
        result: Err(server_error()),
    });
    assert!(wizard(&model).calls.error.is_some());
    let effects = calls(&mut model, SetupCallsEvent::Continue);
    let save = ticket(&effects[0]);
    assert!(
        calls(&mut model, SetupCallsEvent::Continue).is_empty(),
        "single flight"
    );
    let effects = model.update(Event::CallHandlingLoaded {
        ticket: save,
        result: Ok(handling("app_first", 30)),
    });
    assert!(wizard(&model).calls.answering_saved);
    complete(
        &mut model,
        &effects,
        state(["done", "done", "done", "done", "done", "todo"], json!({})),
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Golive));
    // A kept mode with a changed ring sends the ring alone; a stored
    // ai_then_app chosen again sends nothing; Continue with nothing changed.
    let (mut model, _) = on_calls(
        "app_first",
        Ok(workspace_config()),
        Ok(fixture("district-connections.json")),
    );
    calls(&mut model, SetupCallsEvent::SetRing(10));
    let effects = calls(&mut model, SetupCallsEvent::SaveAnswering);
    let [
        Effect::SaveCallHandling {
            ticket: save,
            patch,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!()
    };
    assert_eq!(
        (patch.call_handling, patch.app_ring_seconds),
        (None, Some(10))
    );
    model.update(Event::CallHandlingLoaded {
        ticket: *save,
        result: Ok(handling("app_first", 10)),
    });
    let (mut model, _) = on_calls(
        "ai_then_app",
        Ok(workspace_config()),
        Ok(fixture("district-connections.json")),
    );
    calls(&mut model, SetupCallsEvent::SetMode("ai_first".into()));
    calls(&mut model, SetupCallsEvent::SetMode("ai_then_app".into()));
    let effects = calls(&mut model, SetupCallsEvent::Continue);
    assert!(matches!(
        effects.as_slice(),
        [Effect::SaveSetupWizard { .. }]
    ));
    let (mut model, _) = on_calls(
        "app_first",
        Ok(workspace_config()),
        Ok(fixture("district-connections.json")),
    );
    calls(&mut model, SetupCallsEvent::SetMode("ai_then_app".into()));
    let effects = calls(&mut model, SetupCallsEvent::SaveAnswering);
    let [Effect::SaveCallHandling { patch, .. }] = effects.as_slice() else {
        panic!()
    };
    assert_eq!(patch.call_handling, Some(CallHandlingMode::AiThenApp));
    // The receptionist first, from the app first: the mode alone.
    let (mut model, _) = on_calls(
        "app_first",
        Ok(workspace_config()),
        Ok(fixture("district-connections.json")),
    );
    calls(&mut model, SetupCallsEvent::SetMode("ai_first".into()));
    let effects = calls(&mut model, SetupCallsEvent::SaveAnswering);
    let [Effect::SaveCallHandling { patch, .. }] = effects.as_slice() else {
        panic!("{effects:?}")
    };
    assert_eq!(
        (patch.call_handling, patch.app_ring_seconds),
        (Some(CallHandlingMode::AiFirst), None)
    );
}

#[test]
fn continue_while_who_answers_is_saving_moves_on_when_the_save_lands() {
    let (mut model, _) = on_calls(
        "ai_first",
        Ok(workspace_config()),
        Ok(fixture("district-connections.json")),
    );
    calls(&mut model, SetupCallsEvent::SetMode("app_first".into()));
    let effects = calls(&mut model, SetupCallsEvent::SaveAnswering);
    let save = ticket(&effects[0]);
    assert!(
        calls(&mut model, SetupCallsEvent::Continue).is_empty(),
        "the save on its way carries it"
    );
    let effects = model.update(Event::CallHandlingLoaded {
        ticket: save,
        result: Ok(handling("app_first", 20)),
    });
    complete(
        &mut model,
        &effects,
        state(["done", "done", "done", "done", "done", "todo"], json!({})),
    );
    assert_eq!(wizard(&model).active, Some(SetupStep::Golive));
}

#[test]
fn caller_profiles_are_written_only_when_offered_and_flipped() {
    let (mut model, _) = on_calls(
        "ai_first",
        Ok(studio_config("dgi")),
        Ok(fixture("district-connections.json")),
    );
    assert!(
        calls(&mut model, SetupCallsEvent::SetProfiles(true)).is_empty(),
        "already on"
    );
    let effects = calls(&mut model, SetupCallsEvent::SetProfiles(false));
    let [
        Effect::SavePersona {
            ticket: save,
            patch,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}")
    };
    assert_eq!(patch.dgi_enabled, Some(false));
    assert!(
        calls(&mut model, SetupCallsEvent::SetProfiles(false)).is_empty(),
        "single flight"
    );
    model.update(Event::SettingsWritten {
        ticket: *save,
        result: Err(server_error()),
    });
    assert!(wizard(&model).calls.profiles_on && wizard(&model).calls.error.is_some());
    let effects = calls(&mut model, SetupCallsEvent::SetProfiles(false));
    model.update(Event::SettingsWritten {
        ticket: ticket(&effects[0]),
        result: Ok(()),
    });
    assert!(!wizard(&model).calls.profiles_on);
    let (mut model, _) = on_calls(
        "ai_first",
        Ok(fixture("district-workspace-config-sparse.json")),
        Ok(fixture("district-connections.json")),
    );
    assert!(
        calls(&mut model, SetupCallsEvent::SetProfiles(true)).is_empty(),
        "not on this plan"
    );
}

#[test]
fn coming_back_from_the_integrations_page_reads_the_transfer_number_and_calendar_again() {
    let (mut model, _) = on_calls(
        "ai_first",
        Ok(workspace_config()),
        Ok(fixture("district-connections-viewer.json")),
    );
    let effects = send(&mut model, SetupEvent::HandOffReturned);
    assert_eq!(effects.len(), 3);
    let config_read = find(&effects, |e| {
        matches!(e, Effect::LoadWorkspaceConfig { .. })
    });
    let linked = find(&effects, |e| {
        matches!(e, Effect::LoadSetupConnections { .. })
    });
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config_read,
        result: Ok(fixture("district-workspace-config-sparse.json")),
    });
    model.update(Event::SetupConnectionsLoaded {
        ticket: linked,
        result: Ok(fixture("district-connections.json")),
    });
    let step = &wizard(&model).calls;
    assert_eq!(step.transfer_number, None);
    assert_eq!(step.calendar_name, Some("Google Calendar"));
    let effects = send(&mut model, SetupEvent::HandOffReturned);
    let config_read = find(&effects, |e| {
        matches!(e, Effect::LoadWorkspaceConfig { .. })
    });
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config_read,
        result: Err(server_error()),
    });
    assert_eq!(
        wizard(&model).calls.phase,
        StepPhase::Ready,
        "a failed re-read keeps the step"
    );
    // Before the step is read, only the wizard is read again.
    let (mut model, _) = ready(
        state(["done", "done", "done", "done", "todo", "todo"], json!({})),
        None,
    );
    assert_eq!(send(&mut model, SetupEvent::HandOffReturned).len(), 1);
}

// ── Step 6 ────────────────────────────────────────────────────────────────

fn golive(model: &mut Model, event: GoLiveEvent) -> Vec<Effect> {
    send(model, SetupEvent::GoLive(event))
}

fn forwarding(name: &str) -> ForwardingCheckResponse {
    fixture(name)
}

fn on_golive(region: &str, held: i64) -> (Model, Vec<Effect>) {
    ready(
        state(
            ["done", "done", "done", "done", "done", "todo"],
            json!({"region": region, "numbersHeld": held}),
        ),
        None,
    )
}

#[test]
fn the_summary_needs_both_reads_and_says_who_answers() {
    let (mut model, effects) = on_golive("ca", 1);
    let config_read = find(&effects, |e| {
        matches!(e, Effect::LoadWorkspaceConfig { .. })
    });
    let read = find(&effects, |e| matches!(e, Effect::LoadCallHandling { .. }));
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config_read,
        result: Ok(config_with(json!({"greeting": " Hi. "}))),
    });
    assert_eq!(wizard(&model).golive.summary, None);
    model.update(Event::CallHandlingLoaded {
        ticket: read,
        result: Ok(handling("app_first", 15)),
    });
    let summary = wizard(&model).golive.summary.clone().unwrap();
    assert_eq!(summary.greeting_line(), "Hi.");
    assert_eq!(
        summary.answers_line(),
        "After the District app rings for 15 seconds"
    );
    let lines: Vec<String> = ["ai_then_app", "ai_first"]
        .iter()
        .map(|mode| {
            district_core::GoLiveSummary {
                greeting: String::new(),
                mode: (*mode).into(),
                ring_seconds: 5,
            }
            .answers_line()
        })
        .collect();
    assert_eq!(
        lines,
        ["Every call, with the District app as backup", "Every call"]
    );
    assert_eq!(
        district_core::GoLiveSummary {
            greeting: String::new(),
            mode: String::new(),
            ring_seconds: 0
        }
        .greeting_line(),
        "Your receptionist uses its standard greeting."
    );
    let (mut model, effects) = on_golive("ca", 1);
    let read = find(&effects, |e| matches!(e, Effect::LoadCallHandling { .. }));
    model.update(Event::CallHandlingLoaded {
        ticket: read,
        result: Err(server_error()),
    });
    let config_read = find(&effects, |e| {
        matches!(e, Effect::LoadWorkspaceConfig { .. })
    });
    model.update(Event::WorkspaceConfigLoaded {
        ticket: config_read,
        result: Ok(config_with(json!(null))),
    });
    assert_eq!(
        wizard(&model).golive.summary,
        None,
        "no summary of defaults"
    );
}

#[test]
fn the_test_call_dials_the_owners_mobile_once() {
    let (mut model, _) = on_golive("ca", 0);
    golive(&mut model, GoLiveEvent::SetMobile("(416) 555-0142".into()));
    assert!(
        golive(&mut model, GoLiveEvent::CallMe).is_empty(),
        "no number to call from"
    );
    let (mut model, _) = on_golive("eu", 1);
    golive(&mut model, GoLiveEvent::SetMobile("4165550142".into()));
    assert!(golive(&mut model, GoLiveEvent::CallMe).is_empty());
    assert!(wizard(&model).golive.mobile_error);
    let (mut model, _) = on_golive("ca", 1);
    golive(&mut model, GoLiveEvent::SetMobile("(416) 555-0142".into()));
    let effects = golive(&mut model, GoLiveEvent::CallMe);
    let [
        Effect::PlaceSetupTestCall {
            ticket: call,
            phone,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}")
    };
    assert_eq!(phone, "+14165550142");
    assert!(
        golive(&mut model, GoLiveEvent::CallMe).is_empty(),
        "single flight"
    );
    model.update(Event::SetupTestCallPlaced {
        ticket: *call,
        result: Ok(fixture("district-setup-test-call.json")),
    });
    assert_eq!(
        wizard(&model).golive.call_status,
        Some(TestCallStatus::Calling)
    );
    let effects = golive(&mut model, GoLiveEvent::CallMe);
    model.update(Event::SetupTestCallPlaced {
        ticket: ticket(&effects[0]),
        result: Err(server_error()),
    });
    assert!(matches!(
        wizard(&model).golive.call_status,
        Some(TestCallStatus::Failed(_))
    ));
}

#[test]
fn the_forwarding_check_starts_polls_every_two_seconds_and_gives_up_after_two_minutes() {
    let (mut model, effects) = on_golive("ca", 1);
    let first = find(&effects, |e| {
        matches!(e, Effect::LoadSetupForwardingCheck { .. })
    });
    model.update(Event::SetupForwardingRead {
        ticket: first,
        result: Ok(forwarding("district-forwarding-check.json")),
    });
    let step = &wizard(&model).golive;
    assert!(step.check_offered());
    assert_eq!(
        step.check_sentence(),
        Some("Forwarding works. The call arrived on your District number.")
    );
    assert_eq!(
        step.attempts_sentence().as_deref(),
        Some("4 checks left today.")
    );
    assert_eq!(
        step.expect_call_line().as_deref(),
        Some("Expect a call from +14165550143.")
    );
    assert_eq!(step.forwarding_mode(), ForwardingCheckMode::All);
    golive(&mut model, GoLiveEvent::SetBusinessLine("12".into()));
    assert!(golive(&mut model, GoLiveEvent::StartCheck).is_empty());
    assert!(wizard(&model).golive.line_error);
    golive(
        &mut model,
        GoLiveEvent::SetBusinessLine("416 555 0123".into()),
    );
    golive(
        &mut model,
        GoLiveEvent::SetMode(ForwardingCheckMode::Unanswered),
    );
    let effects = golive(&mut model, GoLiveEvent::StartCheck);
    let [
        Effect::StartSetupForwardingCheck {
            ticket: start,
            business_line,
            mode,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}")
    };
    assert_eq!(
        (business_line.as_str(), *mode),
        ("+14165550123", ForwardingCheckMode::Unanswered)
    );
    assert!(
        golive(&mut model, GoLiveEvent::StartCheck).is_empty(),
        "single flight"
    );
    let effects = model.update(Event::SetupForwardingRead {
        ticket: *start,
        result: Ok(forwarding("district-forwarding-check-start.json")),
    });
    let [
        Effect::Wait {
            ticket: wait,
            delay,
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}")
    };
    assert_eq!(*delay, FORWARDING_POLL_INTERVAL);
    assert!(wizard(&model).golive.polling);
    assert_eq!(
        wizard(&model).golive.check_sentence(),
        Some("Checking. This takes up to a minute.")
    );
    assert!(
        golive(&mut model, GoLiveEvent::StartCheck).is_empty(),
        "already reading"
    );
    let mut wait = *wait;
    for tick in 1..=FORWARDING_POLL_LIMIT {
        let effects = model.update(Event::WaitOver { ticket: wait });
        let [Effect::LoadSetupForwardingCheck { ticket: read, .. }] = effects.as_slice() else {
            panic!("{effects:?}")
        };
        let answer = if tick == 1 {
            Err(server_error())
        } else {
            Ok(forwarding("district-forwarding-check-start.json"))
        };
        let effects = model.update(Event::SetupForwardingRead {
            ticket: *read,
            result: answer,
        });
        if tick < FORWARDING_POLL_LIMIT {
            wait = ticket(&effects[0]);
        } else {
            assert!(effects.is_empty());
        }
    }
    let step = &wizard(&model).golive;
    assert!(!step.polling && step.poll_gave_up);
    assert_eq!(
        step.check_sentence(),
        Some("The check is still running. Click the button to look again.")
    );
    // A pending check is read again rather than started anew, and passes.
    let effects = golive(&mut model, GoLiveEvent::StartCheck);
    let effects = model.update(Event::WaitOver {
        ticket: ticket(&effects[0]),
    });
    let effects = model.update(Event::SetupForwardingRead {
        ticket: ticket(&effects[0]),
        result: Ok(forwarding("district-forwarding-check.json")),
    });
    assert!(effects.is_empty());
    assert!(!wizard(&model).golive.poll_gave_up);
}

#[test]
fn a_check_that_fails_to_start_or_fails_says_why() {
    let (mut model, effects) = on_golive("us", 1);
    let first = find(&effects, |e| {
        matches!(e, Effect::LoadSetupForwardingCheck { .. })
    });
    model.update(Event::SetupForwardingRead {
        ticket: first,
        result: Err(server_error()),
    });
    let step = &wizard(&model).golive;
    assert!(!step.check_offered());
    assert_eq!(
        (
            step.check_sentence(),
            step.attempts_sentence(),
            step.expect_call_line()
        ),
        (None, None, None)
    );
    golive(
        &mut model,
        GoLiveEvent::SetBusinessLine("+14165550123".into()),
    );
    let effects = golive(&mut model, GoLiveEvent::StartCheck);
    model.update(Event::SetupForwardingRead {
        ticket: ticket(&effects[0]),
        result: Err(server_error()),
    });
    assert!(wizard(&model).golive.check_error.is_some());
    let failed = |failure: Option<&str>, left: i64| -> ForwardingCheckResponse {
        let mut answer = forwarding("district-forwarding-check.json");
        let check = answer.forwarding_check.as_mut().unwrap();
        check.status = "failed".into();
        check.failure = failure.map(str::to_owned);
        answer.attempts_left = left;
        answer
    };
    for (answer, sentence, attempts) in [
        (
            failed(Some("dial_failed"), 1),
            "We could not place the check call. Try again in a few minutes.",
            "1 check left today.",
        ),
        (
            failed(None, 0),
            "The call did not arrive on your District number. Check your forwarding settings and try again.",
            "0 checks left today.",
        ),
    ] {
        let effects = golive(&mut model, GoLiveEvent::StartCheck);
        model.update(Event::SetupForwardingRead {
            ticket: ticket(&effects[0]),
            result: Ok(answer),
        });
        let step = &wizard(&model).golive;
        assert_eq!(step.check_sentence(), Some(sentence));
        assert_eq!(step.attempts_sentence().as_deref(), Some(attempts));
    }
    // A pending check on the first read starts the poll at once.
    let (mut model, effects) = on_golive("ca", 1);
    let first = find(&effects, |e| {
        matches!(e, Effect::LoadSetupForwardingCheck { .. })
    });
    let effects = model.update(Event::SetupForwardingRead {
        ticket: first,
        result: Ok(forwarding("district-forwarding-check-start.json")),
    });
    let [Effect::Wait { ticket: wait, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    // Back to step 5 stops the poll.
    send(&mut model, SetupEvent::Back);
    assert_eq!(wizard(&model).active, Some(SetupStep::Calls));
    let late = model.update(Event::WaitOver { ticket: *wait });
    assert!(late.is_empty(), "{late:?}");
}

#[test]
fn a_first_read_landing_after_a_check_started_keeps_one_poll() {
    let (mut model, effects) = on_golive("ca", 1);
    let first = find(&effects, |e| {
        matches!(e, Effect::LoadSetupForwardingCheck { .. })
    });
    golive(
        &mut model,
        GoLiveEvent::SetBusinessLine("416 555 0123".into()),
    );
    let effects = golive(&mut model, GoLiveEvent::StartCheck);
    let effects = model.update(Event::SetupForwardingRead {
        ticket: ticket(&effects[0]),
        result: Ok(forwarding("district-forwarding-check-start.json")),
    });
    let [Effect::Wait { .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let late = model.update(Event::SetupForwardingRead {
        ticket: first,
        result: Ok(forwarding("district-forwarding-check-start.json")),
    });
    assert!(late.is_empty(), "{late:?}");
    assert!(wizard(&model).golive.polling);
}
