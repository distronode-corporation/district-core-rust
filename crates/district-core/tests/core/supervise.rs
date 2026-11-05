//! Supervising a live call from its call screen: listening in with the
//! microphone off, the voice agent's lines, a note to the receptionist sent
//! once, and a takeover that is confirmed first and handed back by leaving.

use district_api::{ApiError, ErrorDetail};
use district_core::{
    CallView, DisconnectReason, Effect, Event, HqEvent, MediaEvent, MediaOwner, Model, Route,
    SUPERVISE_LINES, SuperviseControls, SuperviseEnd, SuperviseEvent, SupervisePhase, Supervision,
    Ticket,
};
use district_model::{CallDetailResponse, CallWhisperResponse, MAX_WHISPER_CHARS};
use serde_json::json;

use crate::support::{
    AGENCY, CLIENT, VIEWER, connect, fixture, live_event, loaded, media, monitor_credential, pick,
    server_error, service, signed_in, takeover_credential, without_calls,
};

/// A call's id as the service writes one: a UUID.
const CALL: &str = "5d0c7a3e-2b1f-4c8d-9e6a-0f4b3c2d1e7a";

fn workspace_of(role: &str) -> &'static str {
    match role {
        "viewer" => VIEWER,
        "client" => CLIENT,
        _ => AGENCY,
    }
}

/// The call `id` as the service reads it, with `status`, the receptionist on
/// it.
fn call_read(id: &str, status: &str) -> CallDetailResponse {
    let mut detail: CallDetailResponse = fixture("district-call-detail.json");
    let call = detail.call.as_mut().expect("a call in the fixture");
    call.id = id.to_owned();
    call.status = status.to_owned();
    assert!(
        call.receptionist_on_call,
        "the fixture's call has its receptionist"
    );
    detail
}

/// On the call screen of `id` as `role`, with the call read as `status`.
fn on_call(role: &str, id: &str, status: &str) -> Model {
    let (mut model, _) = loaded(workspace_of(role), role);
    let effects = model.update(Event::Navigate(Route::CallDetail {
        call_id: id.to_owned(),
    }));
    let ticket = pick(&effects, |effect| matches!(effect, Effect::LoadCall { .. }));
    model.update(Event::CallLoaded {
        ticket,
        result: Ok(call_read(id, status)),
    });
    model
}

fn on_live_call(role: &str) -> Model {
    on_call(role, CALL, "in-progress")
}

fn supervise(model: &mut Model, event: SuperviseEvent) -> Vec<Effect> {
    model.update(Event::Supervise(event))
}

fn supervision(model: &Model) -> &Supervision {
    signed_in(model).supervision().expect("a call open")
}

fn controls(model: &Model) -> SuperviseControls {
    signed_in(model).supervise_controls()
}

fn phase(model: &Model) -> SupervisePhase {
    supervision(model).phase
}

fn failure(model: &Model) -> Option<&str> {
    supervision(model)
        .failure
        .as_ref()
        .map(|failure| failure.message.as_str())
}

fn refusal(code: &str, message: &str) -> ApiError {
    ApiError::Conflict(ErrorDetail {
        message: Some(message.to_owned()),
        code: Some(code.to_owned()),
        degraded_regions: Vec::new(),
    })
}

fn line(role: &str, speaker: &str, text: &str) -> MediaEvent {
    MediaEvent::AgentData {
        topic: String::new(),
        payload:
            json!({"type": "TRANSCRIPT_CHUNK", "role": role, "speaker": speaker, "text": text})
                .to_string()
                .into_bytes(),
    }
}

/// The ticket of the listen credential asked for.
fn listen(model: &mut Model) -> Ticket {
    match supervise(model, SuperviseEvent::Listen).as_slice() {
        [Effect::RequestCallMonitor { ticket, call_id }] => {
            assert_eq!(call_id.as_str(), CALL);
            *ticket
        }
        other => panic!("{other:?}"),
    }
}

/// Listening to the live call as `role`: its media session.
fn listening(role: &str) -> (Model, Ticket) {
    let mut model = on_live_call(role);
    let ticket = listen(&mut model);
    assert_eq!(phase(&model), SupervisePhase::Joining);
    assert_eq!(
        supervision(&model).status(),
        Some("Connecting you to the call.")
    );
    let effects = model.update(Event::CallMonitorIssued {
        ticket,
        result: Ok(monitor_credential()),
    });
    let (session, credential, microphone) = connect(&effects);
    assert!(!microphone, "listening joins with the microphone off");
    assert_eq!(credential.url(), "wss://media-room.example.com");
    assert_eq!(credential.passphrase(), None);
    let shown = format!("{effects:?}");
    assert!(!shown.contains("contract-livekit-monitor-jwt"), "{shown}");
    assert_eq!(
        signed_in(&model).supervise_session().unwrap().owner,
        MediaOwner::Supervise
    );
    assert_eq!(phase(&model), SupervisePhase::Joining);
    model.update(media(session, MediaEvent::Connected));
    assert_eq!(phase(&model), SupervisePhase::Listening);
    (model, session)
}

/// Confirms a takeover: the ticket of the takeover credential asked for.
fn confirm_takeover(model: &mut Model) -> Ticket {
    assert!(supervise(model, SuperviseEvent::TakeOver).is_empty());
    assert!(supervision(model).confirming_takeover);
    match supervise(model, SuperviseEvent::ConfirmTakeOver).as_slice() {
        [
            Effect::RequestCallTakeover {
                ticket,
                workspace_id,
                call_id,
            },
        ] => {
            assert_eq!(workspace_id, AGENCY);
            assert_eq!(call_id.as_str(), CALL);
            *ticket
        }
        other => panic!("{other:?}"),
    }
}

/// On the call as an agency member, taken over from listening: the takeover
/// session.
fn on_the_call() -> (Model, Ticket) {
    let (mut model, listen_session) = listening("agency");
    let ticket = confirm_takeover(&mut model);
    assert_eq!(phase(&model), SupervisePhase::TakingOver);
    assert_eq!(supervision(&model).status(), Some("Taking over the call."));
    let effects = model.update(Event::CallTakeoverIssued {
        ticket,
        result: Ok(takeover_credential()),
    });
    assert!(
        effects.contains(&Effect::DisconnectMedia {
            session: listen_session
        }),
        "the listening session is left first: {effects:?}"
    );
    let (session, credential, microphone) = connect(&effects);
    assert!(microphone, "the takeover joins with the microphone on");
    assert_eq!(credential.url(), "wss://media-room.example.com");
    assert!(!format!("{effects:?}").contains("contract-livekit-takeover-jwt"));
    model.update(media(session, MediaEvent::Connected));
    assert_eq!(phase(&model), SupervisePhase::OnCall);
    (model, session)
}

#[test]
fn every_role_may_listen_and_only_members_may_whisper_or_take_over() {
    for role in ["agency", "client"] {
        let model = on_live_call(role);
        assert_eq!(
            controls(&model),
            SuperviseControls {
                live: true,
                can_listen: true,
                can_stop_listening: false,
                can_supervise: true,
                can_whisper: true,
                can_take_over: true,
                can_hand_back: false,
            },
            "{role}"
        );
        assert_eq!(
            signed_in(&model).whisper_placeholder(),
            "For example: Offer a 10% discount to book today."
        );
    }

    let mut viewer = on_live_call("viewer");
    assert_eq!(
        controls(&viewer),
        SuperviseControls {
            live: true,
            can_listen: true,
            ..SuperviseControls::default()
        }
    );
    assert_eq!(
        signed_in(&viewer).whisper_placeholder(),
        "Viewers cannot send notes"
    );
    supervise(
        &mut viewer,
        SuperviseEvent::EditWhisper("Offer a discount".to_owned()),
    );
    assert!(supervise(&mut viewer, SuperviseEvent::SendWhisper).is_empty());
    assert!(supervise(&mut viewer, SuperviseEvent::TakeOver).is_empty());
    assert!(!supervision(&viewer).confirming_takeover);
    assert!(supervise(&mut viewer, SuperviseEvent::ConfirmTakeOver).is_empty());
    assert_eq!(phase(&viewer), SupervisePhase::Idle);
}

#[test]
fn only_a_call_in_progress_with_a_call_id_is_offered() {
    for model in [
        on_call("agency", CALL, "completed"),
        on_call("agency", CALL, "ringing"),
        // The routes would read this as another kind of room.
        on_call("agency", "meet_ws_weekly", "in-progress"),
    ] {
        assert_eq!(controls(&model), SuperviseControls::default());
    }
    let mut model = on_call("agency", CALL, "completed");
    assert!(supervise(&mut model, SuperviseEvent::Listen).is_empty());
    supervise(&mut model, SuperviseEvent::EditWhisper("Hello".to_owned()));
    assert!(supervise(&mut model, SuperviseEvent::SendWhisper).is_empty());

    // Before the call is read, nothing either.
    let (mut model, _) = loaded(AGENCY, "agency");
    model.update(Event::Navigate(Route::CallDetail {
        call_id: CALL.to_owned(),
    }));
    assert!(matches!(
        signed_in(&model).call.as_ref().unwrap().call,
        CallView::Loading
    ));
    assert_eq!(controls(&model), SuperviseControls::default());

    // With no call open, no supervision at all.
    let (model, _) = loaded(AGENCY, "agency");
    assert!(signed_in(&model).supervision().is_none());
    assert_eq!(controls(&model), SuperviseControls::default());
}

#[test]
fn a_build_without_calls_cannot_join_but_can_still_send_a_note() {
    without_calls(|| {
        let mut model = on_live_call("agency");
        let shown = controls(&model);
        assert!(!shown.can_listen);
        assert!(!shown.can_take_over);
        assert!(shown.can_whisper);
        assert!(supervise(&mut model, SuperviseEvent::Listen).is_empty());
    });
}

#[test]
fn listening_shows_the_calls_lines_and_never_opens_the_microphone() {
    let (mut model, session) = listening("viewer");
    assert_eq!(supervision(&model).status(), Some("You are listening"));
    let shown = controls(&model);
    assert!(shown.can_stop_listening);
    assert!(!shown.can_listen);

    model.update(media(
        session,
        MediaEvent::ParticipantJoined(service("agent-1")),
    ));
    model.update(media(
        session,
        line("user", "Caller", "Do you have Tuesday?"),
    ));
    model.update(media(session, line("assistant", "Ava", "We do, at nine.")));
    // Neither a packet of another kind, nor an empty line, nor what is not
    // a packet at all, is a line.
    model.update(media(
        session,
        MediaEvent::AgentData {
            topic: String::new(),
            payload: br#"{"type":"WHISPER","text":"Offer a discount"}"#.to_vec(),
        },
    ));
    model.update(media(session, line("assistant", "Ava", "  ")));
    model.update(media(
        session,
        MediaEvent::AgentData {
            topic: "lk.other".to_owned(),
            payload: b"not json".to_vec(),
        },
    ));
    model.update(media(
        session,
        MediaEvent::AgentState("speaking".to_owned()),
    ));

    let lines = supervision(&model).lines();
    let shown: Vec<(&str, &str)> = lines
        .iter()
        .map(|line| (Supervision::speaker_label(line), line.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Caller", "Do you have Tuesday?"),
            ("Ava", "We do, at nine.")
        ]
    );

    // The listen-only session is never asked to publish.
    assert!(model.update(Event::Microphone(true)).is_empty());

    // Only the newest lines are kept.
    for n in 0..SUPERVISE_LINES {
        model.update(media(session, line("user", "Caller", &format!("Line {n}"))));
    }
    let lines = supervision(&model).lines();
    assert_eq!(lines.len(), SUPERVISE_LINES);
    assert_eq!(lines[0].text, "Line 0");
    assert_eq!(
        lines[SUPERVISE_LINES - 1].text,
        format!("Line {}", SUPERVISE_LINES - 1)
    );

    // Stopping leaves the room, and the session's late reports change nothing.
    assert_eq!(
        supervise(&mut model, SuperviseEvent::StopListening),
        [Effect::DisconnectMedia { session }]
    );
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert!(signed_in(&model).supervise_session().is_none());
    model.update(media(session, line("user", "Caller", "Late")));
    model.update(media(
        session,
        MediaEvent::Disconnected(DisconnectReason::Left),
    ));
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert!(controls(&model).can_listen);
    assert!(supervise(&mut model, SuperviseEvent::StopListening).is_empty());
}

#[test]
fn a_supervise_session_holds_the_engine_like_any_other() {
    let mut model = on_live_call("agency");
    listen(&mut model);
    assert!(
        signed_in(&model).media_busy(),
        "while the credential is asked for"
    );
    let (model, _) = listening("agency");
    assert!(signed_in(&model).media_busy());
    assert!(!signed_in(&model).hq_voice_controls().can_start);
}

#[test]
fn another_session_holding_the_engine_keeps_listen_and_takeover_back() {
    let mut model = on_live_call("agency");
    // An HQ voice session asking for its credential holds the engine.
    model.update(Event::Navigate(Route::Hq));
    model.update(Event::Hq(HqEvent::StartVoice));
    model.update(Event::Hq(HqEvent::AcceptVoiceDisclosure));
    assert!(signed_in(&model).hq.voice.phase != district_core::HqVoicePhase::Off);
    // Back on the call: the call screen opens again, and the engine is busy.
    let effects = model.update(Event::Navigate(Route::CallDetail {
        call_id: CALL.to_owned(),
    }));
    let ticket = pick(&effects, |effect| matches!(effect, Effect::LoadCall { .. }));
    model.update(Event::CallLoaded {
        ticket,
        result: Ok(call_read(CALL, "in-progress")),
    });
    let shown = controls(&model);
    assert!(shown.live);
    assert!(!shown.can_listen);
    assert!(!shown.can_take_over);
    assert!(shown.can_whisper, "a note needs no engine");
}

#[test]
fn a_refused_listen_says_why_and_a_call_no_longer_live_ends() {
    let mut model = on_live_call("agency");
    let ticket = listen(&mut model);
    model.update(Event::CallMonitorIssued {
        ticket,
        result: Err(server_error()),
    });
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert_eq!(
        failure(&model),
        Some("Could not connect you to the call. Try again.")
    );
    assert!(controls(&model).can_listen);

    // The next attempt clears it, and a refusal with a code says the
    // service's own words.
    let ticket = listen(&mut model);
    assert_eq!(supervision(&model).failure, None);
    model.update(Event::CallMonitorIssued {
        ticket,
        result: Err(refusal(
            "call_predates_feature",
            "This call started before.",
        )),
    });
    assert_eq!(
        supervision(&model).failure.as_ref().unwrap().message,
        "This call started before."
    );

    let ticket = listen(&mut model);
    model.update(Event::CallMonitorIssued {
        ticket,
        result: Err(refusal("call_not_in_progress", "This call has ended.")),
    });
    assert_eq!(
        phase(&model),
        SupervisePhase::Ended(SuperviseEnd::CallEnded)
    );
    assert_eq!(supervision(&model).status(), Some("The call has ended."));
    supervise(&mut model, SuperviseEvent::Dismiss);
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert_eq!(supervision(&model).status(), None);
}

#[test]
fn a_late_or_stale_credential_joins_nothing() {
    let mut model = on_live_call("agency");
    let ticket = listen(&mut model);
    // Stopping while it is asked for forgets it.
    assert!(supervise(&mut model, SuperviseEvent::StopListening).is_empty());
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert!(
        model
            .update(Event::CallMonitorIssued {
                ticket,
                result: Ok(monitor_credential()),
            })
            .is_empty()
    );
    assert!(signed_in(&model).supervise_session().is_none());
}

#[test]
fn a_room_that_cannot_be_joined_or_that_ends_says_so() {
    let mut model = on_live_call("agency");
    let ticket = listen(&mut model);
    let effects = model.update(Event::CallMonitorIssued {
        ticket,
        result: Ok(monitor_credential()),
    });
    let (session, _, _) = connect(&effects);
    model.update(media(
        session,
        MediaEvent::Disconnected(DisconnectReason::ConnectFailed),
    ));
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert_eq!(
        supervision(&model).failure.as_ref().unwrap().message,
        "Could not connect you to the call. Try again."
    );

    let (mut model, session) = listening("agency");
    model.update(media(
        session,
        MediaEvent::Disconnected(DisconnectReason::RoomEnded),
    ));
    assert_eq!(
        phase(&model),
        SupervisePhase::Ended(SuperviseEnd::CallEnded)
    );
    assert!(signed_in(&model).supervise_session().is_none());

    let (mut model, session) = listening("agency");
    model.update(media(
        session,
        MediaEvent::Disconnected(DisconnectReason::JoinedElsewhere),
    ));
    let end = SuperviseEnd::Media(DisconnectReason::JoinedElsewhere);
    assert_eq!(phase(&model), SupervisePhase::Ended(end));
    assert_eq!(
        supervision(&model).status(),
        DisconnectReason::JoinedElsewhere.message()
    );
    // Listening again works from an ended session.
    assert!(controls(&model).can_listen);
}

#[test]
fn a_takeover_is_confirmed_first_and_handed_back_by_leaving() {
    let (mut model, listen_session) = listening("agency");
    // Opening and closing the confirmation asks for nothing.
    assert!(supervise(&mut model, SuperviseEvent::TakeOver).is_empty());
    assert!(supervision(&model).confirming_takeover);
    assert!(supervise(&mut model, SuperviseEvent::CancelTakeOver).is_empty());
    assert!(!supervision(&model).confirming_takeover);
    // Confirming with no confirmation showing asks for nothing either.
    assert!(supervise(&mut model, SuperviseEvent::ConfirmTakeOver).is_empty());
    assert_eq!(phase(&model), SupervisePhase::Listening);
    assert!(signed_in(&model).supervise_session().is_some());
    assert!(
        !model
            .update(Event::Media(district_core::MediaUpdate {
                session: listen_session,
                event: MediaEvent::Reconnecting,
            }))
            .iter()
            .any(|effect| matches!(effect, Effect::DisconnectMedia { .. }))
    );

    let (mut model, session) = on_the_call();
    assert_eq!(supervision(&model).status(), Some("You are on the call"));
    let shown = controls(&model);
    assert!(shown.can_hand_back);
    assert!(!shown.can_whisper);
    assert!(!shown.can_take_over);
    assert!(!shown.can_listen);
    assert!(!shown.can_stop_listening);
    assert_eq!(
        signed_in(&model).whisper_placeholder(),
        "Hand the call back to your receptionist to send a note"
    );
    // The microphone works on the call.
    assert_eq!(
        model.update(Event::Microphone(false)),
        [Effect::SetMicrophone {
            session,
            enabled: false
        }]
    );
    assert!(supervise(&mut model, SuperviseEvent::StopListening).is_empty());

    assert_eq!(
        supervise(&mut model, SuperviseEvent::HandBack),
        [Effect::DisconnectMedia { session }]
    );
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert!(signed_in(&model).supervise_session().is_none());
    assert!(controls(&model).can_listen);
    assert!(supervise(&mut model, SuperviseEvent::HandBack).is_empty());
}

#[test]
fn a_takeover_from_idle_joins_with_the_microphone() {
    let mut model = on_live_call("client");
    assert!(supervise(&mut model, SuperviseEvent::TakeOver).is_empty());
    let ticket = match supervise(&mut model, SuperviseEvent::ConfirmTakeOver).as_slice() {
        [
            Effect::RequestCallTakeover {
                ticket,
                workspace_id,
                ..
            },
        ] => {
            assert_eq!(workspace_id, CLIENT);
            *ticket
        }
        other => panic!("{other:?}"),
    };
    assert!(signed_in(&model).media_busy());
    let effects = model.update(Event::CallTakeoverIssued {
        ticket,
        result: Ok(takeover_credential()),
    });
    let (session, _, microphone) = connect(&effects);
    assert!(microphone);
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::DisconnectMedia { .. }))
    );
    assert!(controls(&model).can_hand_back, "joining can be abandoned");
    model.update(media(session, MediaEvent::Connected));
    assert_eq!(phase(&model), SupervisePhase::OnCall);
}

#[test]
fn a_refused_takeover_leaves_a_listener_listening() {
    let (mut model, session) = listening("agency");
    let ticket = confirm_takeover(&mut model);
    model.update(Event::CallTakeoverIssued {
        ticket,
        result: Err(server_error()),
    });
    assert_eq!(phase(&model), SupervisePhase::Listening);
    assert_eq!(
        failure(&model),
        Some("The call could not be taken over. Try again.")
    );
    assert!(signed_in(&model).supervise_session().is_some());
    // The session goes on.
    model.update(media(session, line("user", "Caller", "Still here")));
    assert_eq!(supervision(&model).lines().len(), 1);

    // From idle, a refusal goes back to idle.
    let mut model = on_live_call("agency");
    let ticket = confirm_takeover(&mut model);
    model.update(Event::CallTakeoverIssued {
        ticket,
        result: Err(refusal("service_unavailable", "Try again.")),
    });
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert_eq!(
        supervision(&model).failure.as_ref().unwrap().message,
        "Try again."
    );

    // A call that has ended ends the session, and leaves the room.
    let (mut model, session) = listening("agency");
    let ticket = confirm_takeover(&mut model);
    let effects = model.update(Event::CallTakeoverIssued {
        ticket,
        result: Err(refusal("call_not_in_progress", "This call has ended.")),
    });
    assert_eq!(effects, [Effect::DisconnectMedia { session }]);
    assert_eq!(
        phase(&model),
        SupervisePhase::Ended(SuperviseEnd::CallEnded)
    );
}

#[test]
fn a_takeover_that_cannot_join_says_to_check_the_microphone() {
    let (mut model, _) = listening("agency");
    let ticket = confirm_takeover(&mut model);
    let effects = model.update(Event::CallTakeoverIssued {
        ticket,
        result: Ok(takeover_credential()),
    });
    let (session, _, _) = connect(&effects);
    model.update(media(
        session,
        MediaEvent::Disconnected(DisconnectReason::ConnectFailed),
    ));
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert!(
        supervision(&model)
            .failure
            .as_ref()
            .unwrap()
            .message
            .starts_with("Could not join the call with your microphone.")
    );
}

#[test]
fn a_note_is_sent_once_trimmed_and_kept_until_it_is_delivered() {
    let mut model = on_live_call("agency");
    // A blank note sends nothing.
    supervise(&mut model, SuperviseEvent::EditWhisper("   ".to_owned()));
    assert!(supervise(&mut model, SuperviseEvent::SendWhisper).is_empty());

    supervise(
        &mut model,
        SuperviseEvent::EditWhisper("  Offer the morning slot ".to_owned()),
    );
    let ticket = match supervise(&mut model, SuperviseEvent::SendWhisper).as_slice() {
        [
            Effect::SendCallWhisper {
                ticket,
                workspace_id,
                call_id,
                text,
            },
        ] => {
            assert_eq!(workspace_id, AGENCY);
            assert_eq!(call_id.as_str(), CALL);
            assert_eq!(text, "Offer the morning slot");
            *ticket
        }
        other => panic!("{other:?}"),
    };
    assert!(supervision(&model).whisper.is_sending());
    // One at a time.
    assert!(!controls(&model).can_whisper);
    assert!(supervise(&mut model, SuperviseEvent::SendWhisper).is_empty());

    model.update(Event::CallWhispered {
        ticket,
        result: Ok(CallWhisperResponse { success: true }),
    });
    let whisper = &supervision(&model).whisper;
    assert!(!whisper.is_sending());
    assert_eq!(whisper.draft, "", "delivered, so cleared");
    assert_eq!(whisper.failure, None);
    // The same answer again changes nothing.
    assert!(
        model
            .update(Event::CallWhispered {
                ticket,
                result: Err(server_error()),
            })
            .is_empty()
    );
    assert_eq!(supervision(&model).whisper.failure, None);
}

#[test]
fn a_note_edited_while_it_was_sent_is_kept() {
    let mut model = on_live_call("agency");
    supervise(&mut model, SuperviseEvent::EditWhisper("Offer".to_owned()));
    let effects = supervise(&mut model, SuperviseEvent::SendWhisper);
    let ticket = pick(&effects, |effect| {
        matches!(effect, Effect::SendCallWhisper { .. })
    });
    supervise(
        &mut model,
        SuperviseEvent::EditWhisper("Offer a discount".to_owned()),
    );
    model.update(Event::CallWhispered {
        ticket,
        result: Ok(CallWhisperResponse { success: true }),
    });
    assert_eq!(supervision(&model).whisper.draft, "Offer a discount");
}

#[test]
fn a_note_that_was_not_delivered_keeps_its_draft_and_says_why() {
    let mut model = on_live_call("agency");
    supervise(&mut model, SuperviseEvent::EditWhisper("Offer".to_owned()));
    let effects = supervise(&mut model, SuperviseEvent::SendWhisper);
    let ticket = pick(&effects, |effect| {
        matches!(effect, Effect::SendCallWhisper { .. })
    });
    model.update(Event::CallWhispered {
        ticket,
        result: Err(server_error()),
    });
    let whisper = &supervision(&model).whisper;
    assert_eq!(whisper.draft, "Offer");
    assert_eq!(
        whisper
            .failure
            .as_ref()
            .map(|failure| failure.message.as_str()),
        Some("The note was not delivered. Try again.")
    );
    assert!(
        controls(&model).can_whisper,
        "it may be sent again by the member"
    );

    let effects = supervise(&mut model, SuperviseEvent::SendWhisper);
    assert_eq!(supervision(&model).whisper.failure, None);
    let ticket = pick(&effects, |effect| {
        matches!(effect, Effect::SendCallWhisper { .. })
    });
    model.update(Event::CallWhispered {
        ticket,
        result: Err(refusal(
            "call_not_in_progress",
            "This call has ended. The whisper was not delivered.",
        )),
    });
    let whisper = &supervision(&model).whisper;
    assert_eq!(whisper.draft, "Offer");
    assert_eq!(
        whisper.failure.as_ref().unwrap().message,
        "This call has ended. The whisper was not delivered."
    );
    assert_eq!(
        phase(&model),
        SupervisePhase::Ended(SuperviseEnd::CallEnded)
    );
    supervise(&mut model, SuperviseEvent::Dismiss);
    assert_eq!(supervision(&model).whisper.failure, None);
}

#[test]
fn a_note_longer_than_the_service_takes_is_refused_here() {
    let mut model = on_live_call("agency");
    let longest = "a".repeat(MAX_WHISPER_CHARS);
    supervise(
        &mut model,
        SuperviseEvent::EditWhisper(format!("{longest}b")),
    );
    assert!(supervise(&mut model, SuperviseEvent::SendWhisper).is_empty());
    assert_eq!(
        supervision(&model)
            .whisper
            .failure
            .as_ref()
            .unwrap()
            .message,
        "Keep the note to 2000 characters or fewer."
    );
    // Characters are counted, not bytes.
    supervise(
        &mut model,
        SuperviseEvent::EditWhisper("é".repeat(MAX_WHISPER_CHARS)),
    );
    assert!(matches!(
        supervise(&mut model, SuperviseEvent::SendWhisper).as_slice(),
        [Effect::SendCallWhisper { .. }]
    ));
}

#[test]
fn leaving_the_call_screen_leaves_the_room() {
    let (mut model, session) = on_the_call();
    let effects = model.update(Event::Navigate(Route::Calls));
    assert!(
        effects.contains(&Effect::DisconnectMedia { session }),
        "{effects:?}"
    );
    assert!(signed_in(&model).supervise_session().is_none());
    assert!(signed_in(&model).supervision().is_none());
    assert!(!signed_in(&model).media_busy());

    // A credential still being asked for is forgotten with the screen.
    let mut model = on_live_call("agency");
    let ticket = listen(&mut model);
    model.update(Event::Navigate(Route::Calls));
    assert!(!signed_in(&model).media_busy());
    assert!(
        model
            .update(Event::CallMonitorIssued {
                ticket,
                result: Ok(monitor_credential()),
            })
            .is_empty()
    );
}

#[test]
fn the_desktop_going_to_sleep_leaves_the_room_and_keeps_the_screen() {
    let (mut model, session) = listening("agency");
    let effects = model.update(Event::Suspending);
    assert!(
        effects.contains(&Effect::DisconnectMedia { session }),
        "{effects:?}"
    );
    assert_eq!(phase(&model), SupervisePhase::Idle);
}

#[test]
fn a_call_read_again_as_over_ends_the_session() {
    let (mut model, session) = listening("agency");
    let effects = model.update(Event::Refresh);
    let ticket = pick(&effects, |effect| matches!(effect, Effect::LoadCall { .. }));
    // Still in progress: nothing changes.
    let effects = model.update(Event::CallLoaded {
        ticket,
        result: Ok(call_read(CALL, "in-progress")),
    });
    assert!(!effects.contains(&Effect::DisconnectMedia { session }));
    assert_eq!(phase(&model), SupervisePhase::Listening);

    let effects = model.update(Event::Refresh);
    let ticket = pick(&effects, |effect| matches!(effect, Effect::LoadCall { .. }));
    let effects = model.update(Event::CallLoaded {
        ticket,
        result: Ok(call_read(CALL, "completed")),
    });
    assert!(
        effects.contains(&Effect::DisconnectMedia { session }),
        "{effects:?}"
    );
    assert_eq!(
        phase(&model),
        SupervisePhase::Ended(SuperviseEnd::CallEnded)
    );
    assert_eq!(controls(&model), SuperviseControls::default());
}

#[test]
fn supervising_speaks_french_when_the_app_does() {
    let (mut model, session) = listening("agency");
    district_core::l10n::with_locale(district_core::Locale::FrCa, || {
        assert_eq!(supervision(&model).status(), Some("Vous écoutez l'appel"));
        model.update(media(session, line("user", "Caller", "Bonjour")));
        assert_eq!(
            Supervision::speaker_label(&supervision(&model).lines()[0]),
            "Appelant"
        );
        supervise(
            &mut model,
            SuperviseEvent::EditWhisper("é".repeat(MAX_WHISPER_CHARS + 1)),
        );
        supervise(&mut model, SuperviseEvent::SendWhisper);
        assert_eq!(
            supervision(&model)
                .whisper
                .failure
                .as_ref()
                .unwrap()
                .message,
            format!(
                "La consigne doit compter au plus {} caractères.",
                district_core::l10n::format::integer(district_core::Locale::FrCa, 2000)
            )
        );
    });
}

#[test]
fn with_no_call_open_nothing_is_supervised() {
    let (mut model, _) = loaded(AGENCY, "agency");
    for event in [
        SuperviseEvent::Listen,
        SuperviseEvent::StopListening,
        SuperviseEvent::EditWhisper("Offer".to_owned()),
        SuperviseEvent::SendWhisper,
        SuperviseEvent::TakeOver,
        SuperviseEvent::CancelTakeOver,
        SuperviseEvent::ConfirmTakeOver,
        SuperviseEvent::HandBack,
        SuperviseEvent::Dismiss,
    ] {
        assert!(supervise(&mut model, event.clone()).is_empty(), "{event:?}");
    }
    assert!(signed_in(&model).supervision().is_none());
}

#[test]
fn a_confirmation_that_outlived_the_live_call_asks_for_nothing() {
    let mut model = on_live_call("agency");
    assert!(supervise(&mut model, SuperviseEvent::TakeOver).is_empty());
    assert!(supervision(&model).confirming_takeover);
    let effects = model.update(Event::Refresh);
    let ticket = pick(&effects, |effect| matches!(effect, Effect::LoadCall { .. }));
    model.update(Event::CallLoaded {
        ticket,
        result: Ok(call_read(CALL, "completed")),
    });
    assert!(supervise(&mut model, SuperviseEvent::ConfirmTakeOver).is_empty());
    assert!(!supervision(&model).confirming_takeover);
    assert_eq!(phase(&model), SupervisePhase::Idle);
}

#[test]
fn a_takeover_answered_after_the_screen_closed_joins_nothing() {
    let mut model = on_live_call("agency");
    let ticket = confirm_takeover(&mut model);
    model.update(Event::Navigate(Route::Calls));
    assert!(
        model
            .update(Event::CallTakeoverIssued {
                ticket,
                result: Ok(takeover_credential()),
            })
            .is_empty()
    );
    assert!(signed_in(&model).supervise_session().is_none());
}

#[test]
fn a_connection_resumed_or_left_by_the_engine_is_said_plainly() {
    let (mut model, session) = listening("agency");
    model.update(media(session, MediaEvent::Reconnecting));
    assert_eq!(
        signed_in(&model).supervise_session().unwrap().notice(),
        Some(district_core::MediaSession::RECONNECTING)
    );
    model.update(media(session, MediaEvent::Connected));
    assert_eq!(phase(&model), SupervisePhase::Listening);
    assert_eq!(
        signed_in(&model).supervise_session().unwrap().notice(),
        None
    );

    model.update(media(
        session,
        MediaEvent::Disconnected(DisconnectReason::Left),
    ));
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert_eq!(supervision(&model).failure, None);
}

#[test]
fn a_refused_access_token_says_so_rather_than_try_again() {
    let mut model = on_live_call("agency");
    let ticket = listen(&mut model);
    let error = ApiError::Unauthorized(district_api::UnauthorizedReason::RefusedNotRetried);
    let expected = district_core::FailureText::from_api_error(&error).message;
    model.update(Event::CallMonitorIssued {
        ticket,
        result: Err(error),
    });
    assert_eq!(phase(&model), SupervisePhase::Idle);
    assert_eq!(failure(&model), Some(expected.as_str()));
}

/// The service's codes for a call with no receptionist on it, and for
/// listening to one that has ended.
const NO_AI: &str = "call_has_no_receptionist";
const LISTEN_ENDED: &str = "call_ended";

#[test]
fn a_call_with_no_ai_on_it_offers_no_note_or_takeover_after_the_first_refusal() {
    let mut model = on_live_call("agency");
    supervise(&mut model, SuperviseEvent::EditWhisper("Offer".to_owned()));
    let effects = supervise(&mut model, SuperviseEvent::SendWhisper);
    let ticket = pick(&effects, |effect| {
        matches!(effect, Effect::SendCallWhisper { .. })
    });
    model.update(Event::CallWhispered {
        ticket,
        result: Err(refusal(NO_AI, "This call has no AI on it.")),
    });
    let no_ai = "That is a team member's own call. Your receptionist is not on it, so you cannot \
                 send a note or take the call over.";
    assert!(supervision(&model).no_receptionist);
    assert_eq!(
        supervision(&model)
            .whisper
            .failure
            .as_ref()
            .map(|failure| failure.message.as_str()),
        Some(no_ai)
    );
    assert_eq!(supervision(&model).whisper.draft, "Offer");
    let shown = controls(&model);
    assert!(shown.can_supervise);
    assert!(!shown.can_whisper);
    assert!(!shown.can_take_over);
    assert!(shown.can_listen, "it may still be listened to");
    assert_eq!(signed_in(&model).whisper_placeholder(), no_ai);
    assert!(supervise(&mut model, SuperviseEvent::SendWhisper).is_empty());
    assert!(supervise(&mut model, SuperviseEvent::TakeOver).is_empty());

    // A takeover refused the same way marks it too, and a listener listens on.
    let (mut model, _) = listening("agency");
    let ticket = confirm_takeover(&mut model);
    model.update(Event::CallTakeoverIssued {
        ticket,
        result: Err(refusal(NO_AI, "This call has no AI on it.")),
    });
    assert!(supervision(&model).no_receptionist);
    assert_eq!(phase(&model), SupervisePhase::Listening);
    assert_eq!(failure(&model), Some(no_ai));
    assert!(!controls(&model).can_take_over);
}

#[test]
fn listening_to_a_call_that_has_ended_is_the_calls_end() {
    let mut model = on_live_call("viewer");
    let ticket = listen(&mut model);
    model.update(Event::CallMonitorIssued {
        ticket,
        result: Err(refusal(LISTEN_ENDED, "This call has ended.")),
    });
    assert_eq!(
        phase(&model),
        SupervisePhase::Ended(SuperviseEnd::CallEnded)
    );
    assert_eq!(failure(&model), None);
}

fn whisper_applied(call_id: &str, data: serde_json::Value) -> Event {
    live_event(district_model::TelemetryEnvelope {
        workspace_id: AGENCY.to_owned(),
        call_id: call_id.to_owned(),
        event_type: district_model::TelemetryEventType::WhisperApplied,
        data,
        timestamp: "2026-11-05T18:00:00.000Z".to_owned(),
    })
}

#[test]
fn what_the_receptionist_did_with_a_note_is_shown_until_the_next_one() {
    let mut model = on_live_call("agency");
    assert_eq!(supervision(&model).whisper.outcome_line(), None);
    assert!(
        model
            .update(whisper_applied(
                CALL,
                json!({"applied": true, "firstWords": "Offer the morning"}),
            ))
            .is_empty()
    );
    let whisper = &supervision(&model).whisper;
    assert_eq!(
        whisper.outcome_line(),
        Some("Your receptionist used your note.")
    );
    assert_eq!(
        whisper.outcome.as_ref().unwrap().first_words,
        "Offer the morning"
    );

    model.update(whisper_applied(
        CALL,
        json!({"applied": false, "firstWords": "Offer", "reason": "transfer"}),
    ));
    assert_eq!(
        supervision(&model).whisper.outcome_line(),
        Some("Your receptionist could not use your note.")
    );

    // Another call's, and one that cannot be read, change nothing.
    model.update(whisper_applied(
        "6e1d8b4f-3c2a-4d9e-8f7b-1a5c4d3e2f80",
        json!({"applied": true}),
    ));
    model.update(whisper_applied(CALL, json!({"firstWords": "Offer"})));
    assert!(
        !supervision(&model)
            .whisper
            .outcome
            .as_ref()
            .unwrap()
            .applied
    );

    // A new note clears it.
    supervise(
        &mut model,
        SuperviseEvent::EditWhisper("Book Tuesday".to_owned()),
    );
    supervise(&mut model, SuperviseEvent::SendWhisper);
    assert_eq!(supervision(&model).whisper.outcome, None);

    // With no call open, it is dropped.
    model.update(Event::Navigate(Route::Calls));
    assert!(
        model
            .update(whisper_applied(CALL, json!({"applied": true})))
            .is_empty()
    );
}

/// Seer, PR #70: a takeover answered after the listening room ended must not
/// join with the microphone under a screen that offers no controls.
#[test]
fn a_takeover_answered_after_the_room_ended_joins_nothing() {
    for reason in [
        DisconnectReason::RoomEnded,
        DisconnectReason::ConnectionLost,
        DisconnectReason::ConnectFailed,
    ] {
        let (mut model, session) = listening("agency");
        let ticket = confirm_takeover(&mut model);
        model.update(media(session, MediaEvent::Disconnected(reason)));
        assert!(
            model
                .update(Event::CallTakeoverIssued {
                    ticket,
                    result: Ok(takeover_credential()),
                })
                .is_empty(),
            "{reason:?}"
        );
        assert!(
            signed_in(&model).supervise_session().is_none(),
            "{reason:?}"
        );
        assert!(!signed_in(&model).media_busy(), "{reason:?}");
        if reason == DisconnectReason::ConnectFailed {
            // The listening room failed, not the microphone's.
            assert_eq!(
                failure(&model),
                Some("Could not connect you to the call. Try again.")
            );
        }
    }
}

/// Seer, PR #70: a listening session that finishes joining while a takeover
/// is asked for does not put the member on the call.
#[test]
fn a_listening_session_joining_mid_takeover_leaves_the_takeover_asked_for() {
    let mut model = on_live_call("agency");
    let ticket = listen(&mut model);
    let effects = model.update(Event::CallMonitorIssued {
        ticket,
        result: Ok(monitor_credential()),
    });
    let (listen_session, _, _) = connect(&effects);
    let takeover = confirm_takeover(&mut model);
    model.update(media(listen_session, MediaEvent::Connected));
    assert_eq!(phase(&model), SupervisePhase::TakingOver);
    assert!(model.update(Event::Microphone(true)).is_empty());

    // Refused, it goes back to listening on the session that joined.
    model.update(Event::CallTakeoverIssued {
        ticket: takeover,
        result: Err(server_error()),
    });
    assert_eq!(phase(&model), SupervisePhase::Listening);

    // Granted, only the microphone's session puts the member on the call.
    let takeover = confirm_takeover(&mut model);
    let effects = model.update(Event::CallTakeoverIssued {
        ticket: takeover,
        result: Ok(takeover_credential()),
    });
    let (session, _, microphone) = connect(&effects);
    assert!(microphone);
    assert_eq!(phase(&model), SupervisePhase::TakingOver);
    model.update(media(session, MediaEvent::Connected));
    assert_eq!(phase(&model), SupervisePhase::OnCall);
}

/// A member's own softphone call, or one with no room: the call row says no
/// receptionist is on it, so neither a note nor a takeover is offered, before
/// anything is asked for. It may still be listened to.
#[test]
fn a_call_row_without_its_receptionist_offers_no_note_or_takeover_up_front() {
    let (mut model, _) = loaded(AGENCY, "agency");
    let effects = model.update(Event::Navigate(Route::CallDetail {
        call_id: CALL.to_owned(),
    }));
    let ticket = pick(&effects, |effect| matches!(effect, Effect::LoadCall { .. }));
    let mut detail = call_read(CALL, "in-progress");
    detail.call.as_mut().unwrap().receptionist_on_call = false;
    model.update(Event::CallLoaded {
        ticket,
        result: Ok(detail),
    });
    assert!(!signed_in(&model).supervised_call_has_receptionist());
    let shown = controls(&model);
    assert!(shown.live && shown.can_supervise && shown.can_listen);
    assert!(!shown.can_whisper);
    assert!(!shown.can_take_over);
    assert!(!supervision(&model).no_receptionist, "nothing was refused");
    assert_eq!(
        signed_in(&model).whisper_placeholder(),
        "That is a team member's own call. Your receptionist is not on it, so you cannot \
         send a note or take the call over."
    );
    supervise(&mut model, SuperviseEvent::EditWhisper("Offer".to_owned()));
    assert!(supervise(&mut model, SuperviseEvent::SendWhisper).is_empty());
    assert!(supervise(&mut model, SuperviseEvent::TakeOver).is_empty());
    assert!(!supervision(&model).confirming_takeover);
    assert!(matches!(
        supervise(&mut model, SuperviseEvent::Listen).as_slice(),
        [Effect::RequestCallMonitor { .. }]
    ));

    // A call with its receptionist offers both, and a call not read yet
    // has none to offer.
    let model = on_live_call("agency");
    assert!(signed_in(&model).supervised_call_has_receptionist());
    let (model, _) = loaded(AGENCY, "agency");
    assert!(!signed_in(&model).supervised_call_has_receptionist());
}

/// Seer, PR #70: a note refused because the call is over ends a listening
/// session and leaves its room, as a refused listen or takeover does.
#[test]
fn a_note_refused_because_the_call_ended_ends_the_session() {
    let (mut model, session) = listening("agency");
    supervise(&mut model, SuperviseEvent::EditWhisper("Offer".to_owned()));
    let effects = supervise(&mut model, SuperviseEvent::SendWhisper);
    let ticket = pick(&effects, |effect| {
        matches!(effect, Effect::SendCallWhisper { .. })
    });
    let effects = model.update(Event::CallWhispered {
        ticket,
        result: Err(refusal("call_not_in_progress", "Call is not in progress")),
    });
    assert_eq!(effects, [Effect::DisconnectMedia { session }]);
    assert_eq!(
        phase(&model),
        SupervisePhase::Ended(SuperviseEnd::CallEnded)
    );
    assert!(signed_in(&model).supervise_session().is_none());
    assert_eq!(supervision(&model).whisper.draft, "Offer");
    assert!(supervision(&model).whisper.failure.is_some());
}
