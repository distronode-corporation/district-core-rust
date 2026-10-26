//! District HQ by voice: the residency statement before anything starts, the
//! encrypted session, the words both sides say, and the cards in front of every
//! change proposed by voice, which only the member resolves.

use std::time::Duration;

use district_api::{ApiError, ErrorDetail};
use district_core::{
    DisconnectReason, Effect, Event, HqAuthor, HqCardState, HqEvent, HqMessage, HqNote, HqText,
    HqVoice, HqVoiceEnd, HqVoicePhase, MediaEvent, MediaOwner, MediaUpdate, Model, Participant,
    RoomsEvent, Route, Speaker, SpeechSegment, Ticket,
};
use district_model::{
    HQ_PROPOSAL_TOPIC, HQ_VOICE_SESSION_SECONDS, HqAgentState, HqConfirmResponse, HqRole, HqTurn,
    HqVoiceProposal, HqVoiceRecordKind, HqVoiceScreen, HqVoiceSelection,
};
use serde_json::json;

use crate::support::{
    AGENCY, CLIENT, VIEWER, fixture, last_ticket, loaded, restored, server_error, signed_in,
    voice_credential, voice_packet, voice_proposal, without_calls,
};

const ID: &str = "11111111-1111-4111-8111-111111111111";

fn voice(model: &Model) -> &HqVoice {
    &signed_in(model).hq.voice
}

fn hq(model: &mut Model, event: HqEvent) -> Vec<Effect> {
    model.update(Event::Hq(event))
}

fn on_hq(role: &str) -> Model {
    let workspace = if role == "viewer" { VIEWER } else { AGENCY };
    let (mut model, _) = loaded(workspace, role);
    model.update(Event::Navigate(Route::Hq));
    model
}

/// The credential request in `effects`: its ticket and the history it carries.
fn requested(effects: &[Effect]) -> (Ticket, Vec<HqTurn>) {
    match effects {
        [
            Effect::RequestHqVoice {
                ticket,
                workspace_id,
                history,
                ..
            },
        ] => {
            assert_eq!(workspace_id, AGENCY);
            (*ticket, history.clone())
        }
        other => panic!("{other:?}"),
    }
}

/// On HQ as an agency member, the statement accepted, the credential asked for.
fn minting() -> (Model, Ticket) {
    let mut model = on_hq("agency");
    assert!(hq(&mut model, HqEvent::StartVoice).is_empty());
    assert_eq!(voice(&model).phase, HqVoicePhase::Disclosing);
    let (ticket, history) = requested(&hq(&mut model, HqEvent::AcceptVoiceDisclosure));
    assert!(history.is_empty());
    assert_eq!(voice(&model).phase, HqVoicePhase::Minting);
    (model, ticket)
}

/// In a live session: its media session's name, with the agent in the room.
fn live() -> (Model, Ticket) {
    let (mut model, ticket) = minting();
    let effects = model.update(Event::HqVoiceIssued {
        ticket,
        result: Ok(voice_credential()),
    });
    let session = match effects.as_slice() {
        [
            Effect::ConnectMedia {
                session,
                credential,
                microphone: true,
            },
            Effect::Wait { delay, .. },
            Effect::ReportHqVoiceScreen {
                workspace_id,
                room_name,
                screen,
            },
        ] => {
            assert_eq!(workspace_id, AGENCY);
            assert_eq!(room_name, &voice_credential().room_name);
            assert_eq!(
                screen,
                &HqVoiceScreen {
                    version: 1,
                    page: "hq".to_owned(),
                    selected: None,
                },
                "where the member is, as the session starts"
            );
            assert_eq!(credential.url(), "wss://media.example.com");
            assert_eq!(
                credential.passphrase(),
                voice_credential().e2ee.map(|e2ee| e2ee.key).as_deref(),
                "the passphrase as sent, never decoded"
            );
            assert_eq!(*delay, Duration::from_secs(HQ_VOICE_SESSION_SECONDS));
            *session
        }
        other => panic!("{other:?}"),
    };
    let shown = format!("{effects:?}");
    assert!(!shown.contains("contract-livekit-hq-voice-jwt"), "{shown}");
    assert_eq!(voice(&model).phase, HqVoicePhase::Live { agent: None });
    let held = signed_in(&model).hq_voice_session().unwrap();
    assert_eq!(held.owner, MediaOwner::HqVoice);
    report(&mut model, session, MediaEvent::Connected);
    report(
        &mut model,
        session,
        MediaEvent::ParticipantJoined(Participant::new("agent-1", None, true)),
    );
    (model, session)
}

fn report(model: &mut Model, session: Ticket, event: MediaEvent) -> Vec<Effect> {
    model.update(Event::Media(MediaUpdate { session, event }))
}

fn packet(model: &mut Model, session: Ticket, name: &str) -> Vec<Effect> {
    report(
        model,
        session,
        MediaEvent::AgentData {
            topic: HQ_PROPOSAL_TOPIC.to_owned(),
            payload: voice_packet(name),
        },
    )
}

/// Live, with the draft proposal on a card.
fn proposed() -> (Model, Ticket) {
    let (mut model, session) = live();
    assert!(packet(&mut model, session, "hq-voice-packet-proposal.json").is_empty());
    assert_eq!(voice(&model).cards.len(), 1);
    assert_eq!(voice(&model).card(ID).unwrap().state, HqCardState::Open);
    (model, session)
}

fn last_note(model: &Model) -> Option<HqNote> {
    match signed_in(model).hq.transcript.last() {
        Some(HqMessage {
            text: HqText::Note(note),
            ..
        }) => Some(*note),
        _ => None,
    }
}

/// The confirmation in `effects`: its ticket, and the proposal sent back.
fn confirmation(effects: &[Effect]) -> (Ticket, HqVoiceProposal) {
    match effects {
        [
            Effect::ConfirmHqVoice {
                ticket, proposal, ..
            },
        ] => (*ticket, proposal.clone()),
        other => panic!("{other:?}"),
    }
}

fn cancellation(effects: &[Effect]) -> Ticket {
    match effects {
        [
            Effect::CancelHqVoice {
                ticket,
                proposal_id,
                ..
            },
        ] => {
            assert_eq!(proposal_id, ID);
            *ticket
        }
        other => panic!("{other:?}"),
    }
}

fn refusal(code: &str) -> ErrorDetail {
    ErrorDetail {
        message: Some("Refused.".to_owned()),
        code: Some(code.to_owned()),
        degraded_regions: Vec::new(),
    }
}

fn applied(executed: bool, greeting: &str) -> HqConfirmResponse {
    HqConfirmResponse {
        success: true,
        executed,
        tool: "update_persona".to_owned(),
        args: json!({"greeting": greeting}).as_object().unwrap().clone(),
        result: None,
    }
}

const GREETING: &str = "Good afternoon, thanks for calling Analytical Engines.";

#[test]
fn the_residency_statement_comes_first_and_only_once_per_workspace() {
    let mut model = on_hq("agency");
    let controls = signed_in(&model).hq_voice_controls();
    assert!(controls.can_start && controls.can_confirm && !controls.can_stop);
    assert!(
        hq(&mut model, HqEvent::AcceptVoiceDisclosure).is_empty(),
        "not shown"
    );
    assert!(hq(&mut model, HqEvent::StartVoice).is_empty());
    assert_eq!(voice(&model).phase, HqVoicePhase::Disclosing);
    hq(&mut model, HqEvent::DeclineVoiceDisclosure);
    assert_eq!(voice(&model).phase, HqVoicePhase::Off, "nothing asked for");
    hq(&mut model, HqEvent::DeclineVoiceDisclosure);
    assert_eq!(voice(&model).phase, HqVoicePhase::Off);

    hq(&mut model, HqEvent::StartVoice);
    let (ticket, _) = requested(&hq(&mut model, HqEvent::AcceptVoiceDisclosure));
    assert!(signed_in(&model).hq_voice_controls().can_stop);
    assert!(
        hq(&mut model, HqEvent::StartVoice).is_empty(),
        "one at a time"
    );
    assert!(
        signed_in(&model).media_busy(),
        "nothing else may join meanwhile"
    );
    hq(&mut model, HqEvent::StopVoice);
    assert_eq!(voice(&model).phase, HqVoicePhase::Off);
    // A late answer to a stopped start joins nothing.
    assert!(
        model
            .update(Event::HqVoiceIssued {
                ticket,
                result: Ok(voice_credential()),
            })
            .is_empty()
    );

    // Accepted once, the next start asks for a credential at once.
    requested(&hq(&mut model, HqEvent::StartVoice));
}

#[test]
fn a_workspace_outside_the_united_states_is_told_its_voice_is_not() {
    let (model, _) = restored();
    assert_eq!(
        signed_in(&model).hq_voice_region_notice(),
        None,
        "no workspace open yet"
    );
    let (model, _) = loaded(AGENCY, "agency");
    assert_eq!(
        signed_in(&model).hq_voice_region_notice(),
        None,
        "a United States workspace"
    );
    let (model, _) = loaded(CLIENT, "client");
    assert_eq!(
        signed_in(&model).hq_voice_region_notice().as_deref(),
        Some(
            "This workspace's data lives in Canada, but District HQ voice conversations are \
             processed in the United States."
        )
    );
}

#[test]
fn a_viewer_and_a_build_without_calls_cannot_start_a_session() {
    let mut model = on_hq("viewer");
    let controls = signed_in(&model).hq_voice_controls();
    assert!(!controls.can_start && !controls.can_confirm);
    assert!(hq(&mut model, HqEvent::StartVoice).is_empty());
    assert_eq!(voice(&model).phase, HqVoicePhase::Off);

    without_calls(|| {
        let mut model = on_hq("agency");
        assert!(!signed_in(&model).hq_voice_controls().can_start);
        assert!(
            hq(&mut model, HqEvent::StartVoice).is_empty(),
            "a billed session nothing could carry"
        );
    });
}

#[test]
fn leaving_the_screen_closes_the_statement_and_nothing_starts_while_a_room_joins() {
    let mut model = on_hq("agency");
    hq(&mut model, HqEvent::StartVoice);
    model.update(Event::Navigate(Route::Rooms));
    assert_eq!(
        voice(&model).phase,
        HqVoicePhase::Off,
        "the statement went with HQ"
    );
    assert!(hq(&mut model, HqEvent::AcceptVoiceDisclosure).is_empty());
    model.update(Event::Rooms(RoomsEvent::EditRoomName("standup".to_owned())));
    let effects = model.update(Event::Rooms(RoomsEvent::Start));
    assert!(matches!(
        effects.as_slice(),
        [Effect::RequestRoomToken { .. }]
    ));
    assert!(
        hq(&mut model, HqEvent::StartVoice).is_empty(),
        "the engine is spoken for"
    );
    assert_eq!(voice(&model).phase, HqVoicePhase::Off);
}

/// The screen reported in `effects`, if any.
fn reported(effects: &[Effect]) -> Option<HqVoiceScreen> {
    effects.iter().find_map(|effect| match effect {
        Effect::ReportHqVoiceScreen { screen, .. } => Some(screen.clone()),
        _ => None,
    })
}

fn screen(version: u64, page: &str, selected: Option<HqVoiceSelection>) -> Option<HqVoiceScreen> {
    Some(HqVoiceScreen {
        version,
        page: page.to_owned(),
        selected,
    })
}

#[test]
fn the_session_goes_on_from_screen_to_screen_and_each_one_is_reported() {
    let (mut model, session) = live();
    let effects = model.update(Event::Navigate(Route::Calls));
    assert!(
        !effects.contains(&Effect::DisconnectMedia { session }),
        "{effects:?}"
    );
    assert!(matches!(voice(&model).phase, HqVoicePhase::Live { .. }));
    assert_eq!(reported(&effects), screen(2, "calls", None));
    let effects = model.update(Event::Navigate(Route::CallDetail {
        call_id: "call_1".to_owned(),
    }));
    assert_eq!(
        reported(&effects),
        screen(
            3,
            "calls",
            HqVoiceSelection::new(HqVoiceRecordKind::Call, "call_1")
        )
    );
    // Back, to the log: the same page, with nothing open on it.
    assert_eq!(
        reported(&model.update(Event::Back)),
        screen(4, "calls", None)
    );
    let effects = model.update(Event::Navigate(Route::Thread {
        thread_key: "contact:c_1".to_owned(),
    }));
    assert_eq!(
        reported(&effects),
        screen(
            5,
            "inbox",
            HqVoiceSelection::new(HqVoiceRecordKind::Conversation, "c_1")
        )
    );
    // A screen the service's table has no page for is not reported.
    let effects = model.update(Event::Navigate(Route::Workspace(
        district_core::WorkspaceSection::Tools,
    )));
    assert_eq!(reported(&effects), None, "{effects:?}");
    assert_eq!(
        reported(&model.update(Event::Navigate(Route::Hq))),
        screen(6, "hq", None)
    );
}

#[test]
fn a_card_is_settled_and_the_session_stopped_from_another_screen() {
    let (mut model, session) = proposed();
    model.update(Event::Navigate(Route::Contacts));
    let (_, sent) = confirmation(&hq(&mut model, HqEvent::ConfirmVoiceCard(ID.to_owned())));
    assert_eq!(sent.proposal_id, ID);
    let effects = hq(&mut model, HqEvent::StopVoice);
    assert!(
        effects.contains(&Effect::DisconnectMedia { session }),
        "{effects:?}"
    );
    assert_eq!(voice(&model).phase, HqVoicePhase::Off);
    assert!(
        reported(&model.update(Event::Navigate(Route::Calls))).is_none(),
        "nothing is reported once the session is over"
    );
}

#[test]
fn the_screen_is_reported_once_as_the_session_starts_and_not_again_unchanged() {
    let (mut model, _) = live();
    let effects = model.update(Event::Navigate(Route::Hq));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::ReportHqVoiceScreen { .. })),
        "the same screen is not reported twice: {effects:?}"
    );
    // Nothing is reported without a live session, wherever the member goes.
    let mut model = on_hq("agency");
    for route in [Route::Calls, Route::Hq] {
        let effects = model.update(Event::Navigate(route));
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::ReportHqVoiceScreen { .. })),
            "{effects:?}"
        );
    }
}

#[test]
fn every_screen_is_named_as_the_web_dashboards_page_with_its_record_by_id() {
    use district_core::WorkspaceSection as W;
    use district_core::scheduling_admin::SchedulingSection as S;
    let selected = |kind, id| HqVoiceSelection::new(kind, id);
    type Page = Option<(&'static str, Option<HqVoiceSelection>)>;
    let cases: Vec<(Route, Page)> = vec![
        (Route::Overview, Some(("home", None))),
        (Route::Inbox, Some(("inbox", None))),
        (
            Route::Thread {
                thread_key: "contact:c_1".to_owned(),
            },
            Some(("inbox", selected(HqVoiceRecordKind::Conversation, "c_1"))),
        ),
        (
            Route::Thread {
                thread_key: "addr:+15145550100".to_owned(),
            },
            Some(("inbox", None)),
        ),
        (Route::Calls, Some(("calls", None))),
        (
            Route::CallDetail {
                call_id: "call_1".to_owned(),
            },
            Some(("calls", selected(HqVoiceRecordKind::Call, "call_1"))),
        ),
        (Route::Dialer, Some(("phone", None))),
        (Route::Contacts, Some(("contacts", None))),
        (Route::BlockedContacts, Some(("contacts", None))),
        (
            Route::ContactDetail {
                contact_id: "c_1".to_owned(),
            },
            Some(("contacts", selected(HqVoiceRecordKind::Contact, "c_1"))),
        ),
        (
            Route::ContactDetail {
                contact_id: "not an id".to_owned(),
            },
            Some(("contacts", None)),
        ),
        (Route::Hq, Some(("hq", None))),
        (Route::Analytics, Some(("analytics", None))),
        (Route::Marketplace, Some(("marketplace", None))),
        (Route::Workspace(W::Numbers), Some(("marketplace", None))),
        (Route::Billing, Some(("billing", None))),
        (Route::Workflows, Some(("workflows", None))),
        (Route::Scheduling, Some(("scheduling", None))),
        (
            Route::SchedulingAdmin(S::Overview),
            Some(("scheduling", None)),
        ),
        (
            Route::SchedulingAdmin(S::EventTypes),
            Some(("scheduling-event-types", None)),
        ),
        (
            Route::SchedulingAdmin(S::EventType {
                slug: "intro".to_owned(),
            }),
            Some(("scheduling-event-type", None)),
        ),
        (
            Route::SchedulingAdmin(S::Hours),
            Some(("scheduling-hours", None)),
        ),
        (
            Route::SchedulingAdmin(S::Bookings),
            Some(("scheduling-bookings", None)),
        ),
        (
            Route::SchedulingAdmin(S::Booking {
                id: "b_1".to_owned(),
            }),
            Some(("scheduling-bookings", None)),
        ),
        (
            Route::SchedulingAdmin(S::Calendar),
            Some(("scheduling-calendar", None)),
        ),
        (
            Route::SchedulingAdmin(S::Team),
            Some(("scheduling-team", None)),
        ),
        (
            Route::SchedulingAdmin(S::Settings),
            Some(("scheduling-settings", None)),
        ),
        (
            Route::SchedulingAdmin(S::Developer),
            Some(("scheduling-developer", None)),
        ),
        (Route::Desk, Some(("desk", None))),
        (
            Route::DeskTicket {
                ticket_id: "t_1".to_owned(),
            },
            Some(("desk", None)),
        ),
        (Route::DeskSettings, Some(("desk", None))),
        (Route::Support, Some(("support", None))),
        (
            Route::SupportRequest {
                key: "DA-42".to_owned(),
            },
            Some(("support", None)),
        ),
        (Route::Rooms, Some(("rooms", None))),
        (Route::Account, Some(("account", None))),
        (Route::Devices, Some(("account", None))),
        (Route::Workspace(W::Hub), Some(("studio", None))),
        (Route::Workspace(W::Persona), Some(("studio-persona", None))),
        (
            Route::Workspace(W::VoiceStudio),
            Some(("studio-voice", None)),
        ),
        (
            Route::Workspace(W::CallHandling),
            Some(("studio-call-handling", None)),
        ),
        (
            Route::Workspace(W::Routing),
            Some(("studio-call-handling", None)),
        ),
        (
            Route::Workspace(W::Knowledge),
            Some(("studio-knowledge", None)),
        ),
        (Route::Workspace(W::Tools), None),
        (Route::Workspace(W::Directory), None),
        (Route::Workspace(W::Messaging), None),
        (Route::Workspace(W::Members), None),
    ];
    for (route, page) in cases {
        assert_eq!(route.hq_voice_page(), page, "{route:?}");
    }
}

#[test]
fn a_credential_without_a_passphrase_or_a_failure_joins_nothing() {
    let (mut model, ticket) = minting();
    let mut clear = voice_credential();
    clear.e2ee = None;
    assert!(
        model
            .update(Event::HqVoiceIssued {
                ticket,
                result: Ok(clear),
            })
            .is_empty()
    );
    let HqVoicePhase::Failed(failure) = &voice(&model).phase else {
        panic!("{:?}", voice(&model).phase);
    };
    assert!(!failure.retryable);
    assert!(signed_in(&model).hq_voice_session().is_none());
    assert!(
        signed_in(&model).hq_voice_controls().can_start,
        "may try again"
    );

    let (ticket, _) = requested(&hq(&mut model, HqEvent::StartVoice));
    model.update(Event::HqVoiceIssued {
        ticket,
        result: Err(server_error()),
    });
    assert!(matches!(voice(&model).phase, HqVoicePhase::Failed(_)));
    hq(&mut model, HqEvent::DismissVoiceNotice);
    assert_eq!(voice(&model).phase, HqVoicePhase::Off);
}

#[test]
fn the_agents_state_is_followed_and_an_unknown_one_reads_as_not_ready() {
    let (mut model, session) = live();
    for (raw, state) in [
        ("listening", Some(HqAgentState::Listening)),
        ("speaking", Some(HqAgentState::Speaking)),
        ("dreaming", None),
    ] {
        report(&mut model, session, MediaEvent::AgentState(raw.to_owned()));
        assert_eq!(voice(&model).phase, HqVoicePhase::Live { agent: state });
    }
    // Data on another topic, and a packet that does not read, change nothing.
    for (topic, payload) in [
        ("lk.chat", b"{}".to_vec()),
        (HQ_PROPOSAL_TOPIC, b"not json".to_vec()),
    ] {
        report(
            &mut model,
            session,
            MediaEvent::AgentData {
                topic: topic.to_owned(),
                payload,
            },
        );
    }
    assert!(voice(&model).cards.is_empty());
}

#[test]
fn spoken_words_join_the_conversation_and_only_final_ones_are_history() {
    let (mut model, session) = live();
    let said = |speaker, id: &str, text: &str, is_final| {
        MediaEvent::Speech(SpeechSegment {
            segment_id: id.to_owned(),
            speaker,
            text: text.to_owned(),
            is_final,
        })
    };
    report(
        &mut model,
        session,
        said(Speaker::Member, "s1", "  ", false),
    );
    assert!(
        signed_in(&model).hq.transcript.is_empty(),
        "nothing said yet"
    );
    report(
        &mut model,
        session,
        said(Speaker::Member, "s1", "How many", false),
    );
    report(
        &mut model,
        session,
        said(Speaker::Agent, "s1", "Three", false),
    );
    report(
        &mut model,
        session,
        said(Speaker::Member, "s1", "How many calls?", true),
    );
    let transcript = &signed_in(&model).hq.transcript;
    assert_eq!(transcript.len(), 2, "one line per speaker's segment");
    assert_eq!(transcript[0].author, HqAuthor::Member);
    assert_eq!(
        transcript[0].text,
        HqText::Spoken {
            segment: "s1".to_owned(),
            text: "How many calls?".to_owned(),
            is_final: true,
        }
    );
    assert_eq!(transcript[1].author, HqAuthor::Assistant);

    let effects = hq(&mut model, HqEvent::Ask("And today?".to_owned()));
    let [Effect::AskHq { history, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        history,
        &[HqTurn {
            role: HqRole::User,
            text: "How many calls?".to_owned(),
        }],
        "the agent's words are still being transcribed"
    );

    // A failed prompt is retried even with words spoken after it.
    let ticket = last_ticket(&effects);
    model.update(Event::HqAnswered {
        ticket,
        result: Err(server_error()),
    });
    report(
        &mut model,
        session,
        said(Speaker::Agent, "s2", "Hello", true),
    );
    let effects = hq(&mut model, HqEvent::Retry);
    let [
        Effect::AskHq {
            prompt, history, ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(prompt, "And today?");
    assert_eq!(history.len(), 1, "the turns before the question");

    // A new voice session starts with the conversation so far.
    hq(&mut model, HqEvent::StopVoice);
    let (_, history) = requested(&hq(&mut model, HqEvent::StartVoice));
    assert_eq!(history.len(), 3);
}

#[test]
fn a_proposal_is_a_card_once_and_a_voice_resolution_settles_it() {
    let (mut model, session) = proposed();
    packet(&mut model, session, "hq-voice-packet-proposal.json");
    assert_eq!(voice(&model).cards.len(), 1, "a repeat is the same card");
    let card = voice(&model).card(ID).unwrap();
    assert!(
        card.summary()
            .starts_with("Update the AI receptionist persona")
    );
    assert!(card.is_actionable());

    packet(&mut model, session, "hq-voice-packet-resolved-applied.json");
    assert!(voice(&model).cards.is_empty());
    assert_eq!(last_note(&model), Some(HqNote::Applied));
    let notes = signed_in(&model).hq.transcript.len();
    packet(&mut model, session, "hq-voice-packet-resolved-applied.json");
    assert_eq!(
        signed_in(&model).hq.transcript.len(),
        notes,
        "a card already settled says nothing"
    );

    for (name, note) in [
        ("hq-voice-packet-resolved-failed.json", HqNote::NotApplied),
        ("hq-voice-packet-resolved-cancelled.json", HqNote::Cancelled),
    ] {
        packet(&mut model, session, "hq-voice-packet-proposal.json");
        packet(&mut model, session, name);
        assert!(voice(&model).cards.is_empty(), "{name}");
        assert_eq!(last_note(&model), Some(note), "{name}");
    }
}

#[test]
fn a_card_without_a_summary_says_so_in_the_apps_words() {
    let (mut model, session) = live();
    let packet = json!({
        "type": "HQ_PROPOSAL", "proposalId": "p-2", "tool": "add_note",
        "args": {}, "summary": " ",
    });
    report(
        &mut model,
        session,
        MediaEvent::AgentData {
            topic: HQ_PROPOSAL_TOPIC.to_owned(),
            payload: packet.to_string().into_bytes(),
        },
    );
    assert_eq!(
        voice(&model).card("p-2").unwrap().summary(),
        "District HQ proposed a change."
    );
}

#[test]
fn a_click_confirm_sends_the_proposal_back_once_and_notes_the_outcome() {
    for (answer, note) in [
        (applied(true, GREETING), HqNote::Applied),
        (applied(false, GREETING), HqNote::NotApplied),
        (applied(true, "Something else."), HqNote::Mismatched),
    ] {
        let (mut model, _) = proposed();
        let (ticket, proposal) =
            confirmation(&hq(&mut model, HqEvent::ConfirmVoiceCard(ID.to_owned())));
        assert_eq!(proposal, voice_proposal(), "exactly as the agent sent it");
        assert_eq!(
            voice(&model).card(ID).unwrap().state,
            HqCardState::Confirming
        );
        assert!(
            hq(&mut model, HqEvent::ConfirmVoiceCard(ID.to_owned())).is_empty(),
            "one click in flight"
        );
        assert!(hq(&mut model, HqEvent::CancelVoiceCard(ID.to_owned())).is_empty());
        model.update(Event::HqVoiceConfirmed {
            ticket,
            result: Ok(answer),
        });
        assert!(voice(&model).cards.is_empty());
        assert_eq!(last_note(&model), Some(note));
    }
    let (mut model, _) = proposed();
    assert!(
        hq(
            &mut model,
            HqEvent::ConfirmVoiceCard("p-unknown".to_owned())
        )
        .is_empty(),
        "no such card"
    );
}

#[test]
fn a_card_the_agent_settles_while_a_click_is_in_flight_goes_at_once() {
    // The click lost the race: its answer, whichever it is, says nothing more.
    let refused = Err(ApiError::Conflict(refusal("already_resolved")));
    for result in [refused, Ok(applied(true, GREETING))] {
        let (mut model, session) = proposed();
        let (ticket, _) = confirmation(&hq(&mut model, HqEvent::ConfirmVoiceCard(ID.to_owned())));
        packet(&mut model, session, "hq-voice-packet-resolved-applied.json");
        assert!(voice(&model).cards.is_empty());
        let notes = signed_in(&model).hq.transcript.len();
        model.update(Event::HqVoiceConfirmed { ticket, result });
        assert_eq!(signed_in(&model).hq.transcript.len(), notes);
    }

    // The same for a cancel whose card the agent settled.
    let (mut model, session) = proposed();
    let ticket = cancellation(&hq(&mut model, HqEvent::CancelVoiceCard(ID.to_owned())));
    packet(
        &mut model,
        session,
        "hq-voice-packet-resolved-cancelled.json",
    );
    let notes = signed_in(&model).hq.transcript.len();
    model.update(Event::HqVoiceCancelled {
        ticket,
        result: Ok(district_model::HqProposalCancelResponse { success: true }),
    });
    assert_eq!(signed_in(&model).hq.transcript.len(), notes);
}

#[test]
fn a_refused_click_settles_the_card_by_its_code_or_keeps_it_to_try_again() {
    let settled = [
        (
            ApiError::Conflict(refusal("already_resolved")),
            Some(HqNote::AlreadyHandled),
        ),
        (
            ApiError::Envelope {
                status: 410,
                code: "expired".to_owned(),
                detail: refusal("expired"),
            },
            Some(HqNote::Expired),
        ),
        (ApiError::NotFound(refusal("not_found")), None),
        (
            ApiError::Envelope {
                status: 400,
                code: "invalid_request".to_owned(),
                detail: refusal("invalid_request"),
            },
            None,
        ),
    ];
    for (error, note) in settled {
        let (mut model, _) = proposed();
        let (ticket, _) = confirmation(&hq(&mut model, HqEvent::ConfirmVoiceCard(ID.to_owned())));
        model.update(Event::HqVoiceConfirmed {
            ticket,
            result: Err(error.clone()),
        });
        assert!(voice(&model).cards.is_empty(), "{error:?}");
        match note {
            Some(note) => assert_eq!(last_note(&model), Some(note)),
            None => {
                assert!(voice(&model).notice.is_some(), "{error:?}");
                hq(&mut model, HqEvent::DismissVoiceNotice);
                assert!(voice(&model).notice.is_none());
                assert!(
                    matches!(voice(&model).phase, HqVoicePhase::Live { .. }),
                    "the session goes on"
                );
            }
        }
    }

    let (mut model, _) = proposed();
    let (ticket, _) = confirmation(&hq(&mut model, HqEvent::ConfirmVoiceCard(ID.to_owned())));
    model.update(Event::HqVoiceConfirmed {
        ticket,
        result: Err(server_error()),
    });
    let card = voice(&model).card(ID).unwrap();
    assert!(
        matches!(
            &card.state,
            HqCardState::Failed {
                confirming: true,
                ..
            }
        ),
        "it may have been made: the member decides"
    );
    assert!(card.is_actionable());
    let ticket = cancellation(&hq(&mut model, HqEvent::CancelVoiceCard(ID.to_owned())));
    model.update(Event::HqVoiceCancelled {
        ticket,
        result: Err(server_error()),
    });
    assert!(matches!(
        voice(&model).card(ID).unwrap().state,
        HqCardState::Failed {
            confirming: false,
            ..
        }
    ));
    let ticket = cancellation(&hq(&mut model, HqEvent::CancelVoiceCard(ID.to_owned())));
    model.update(Event::HqVoiceCancelled {
        ticket,
        result: Ok(district_model::HqProposalCancelResponse { success: true }),
    });
    assert!(voice(&model).cards.is_empty());
    assert_eq!(last_note(&model), Some(HqNote::Cancelled));
    // An answer nobody is waiting for changes nothing.
    model.update(Event::HqVoiceCancelled {
        ticket,
        result: Err(server_error()),
    });
    model.update(Event::HqVoiceConfirmed {
        ticket,
        result: Err(server_error()),
    });
    assert_eq!(last_note(&model), Some(HqNote::Cancelled));
}

#[test]
fn the_session_ends_when_the_agent_leaves_and_the_cards_stay() {
    let (mut model, session) = proposed();
    let effects = report(
        &mut model,
        session,
        MediaEvent::ParticipantLeft {
            identity: "agent-1".to_owned(),
        },
    );
    assert_eq!(effects, [Effect::DisconnectMedia { session }]);
    assert_eq!(
        voice(&model).phase,
        HqVoicePhase::Ended(HqVoiceEnd::AgentLeft)
    );
    assert_eq!(voice(&model).cards.len(), 1, "still confirmable by click");
    confirmation(&hq(&mut model, HqEvent::ConfirmVoiceCard(ID.to_owned())));
}

#[test]
fn people_before_the_agent_joins_end_nothing() {
    let (mut model, ticket) = minting();
    let effects = model.update(Event::HqVoiceIssued {
        ticket,
        result: Ok(voice_credential()),
    });
    let [Effect::ConnectMedia { session, .. }, _, _] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let person = Participant::new("someone", None, false);
    assert!(report(&mut model, *session, MediaEvent::ParticipantJoined(person)).is_empty());
    assert!(
        report(
            &mut model,
            *session,
            MediaEvent::ParticipantLeft {
                identity: "someone".to_owned()
            }
        )
        .is_empty()
    );
    assert_eq!(voice(&model).phase, HqVoicePhase::Live { agent: None });
}

#[test]
fn the_room_ending_says_why() {
    for (reason, end) in [
        (DisconnectReason::RoomEnded, HqVoiceEnd::AgentLeft),
        (
            DisconnectReason::ConnectionLost,
            HqVoiceEnd::Media(DisconnectReason::ConnectionLost),
        ),
    ] {
        let (mut model, session) = live();
        report(&mut model, session, MediaEvent::Disconnected(reason));
        assert_eq!(voice(&model).phase, HqVoicePhase::Ended(end));
        assert!(signed_in(&model).hq_voice_session().is_none());
        assert!(signed_in(&model).hq_voice_controls().can_start);
    }
    assert_eq!(
        HqVoiceEnd::AgentLeft.message(),
        Some("Voice session ended because District HQ left the session.")
    );
    assert_eq!(
        HqVoiceEnd::TimeLimit.message(),
        Some("Voice session ended after reaching its 30-minute limit.")
    );
    assert!(
        HqVoiceEnd::Media(DisconnectReason::ConnectionLost)
            .message()
            .is_some()
    );
    assert_eq!(HqVoiceEnd::Media(DisconnectReason::Left).message(), None);
}

#[test]
fn the_session_is_left_at_its_time_limit() {
    let (mut model, ticket) = minting();
    let effects = model.update(Event::HqVoiceIssued {
        ticket,
        result: Ok(voice_credential()),
    });
    let [
        Effect::ConnectMedia { session, .. },
        Effect::Wait { ticket: wait, .. },
        Effect::ReportHqVoiceScreen { .. },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    let effects = model.update(Event::WaitOver { ticket: *wait });
    assert_eq!(effects, [Effect::DisconnectMedia { session: *session }]);
    assert_eq!(
        voice(&model).phase,
        HqVoicePhase::Ended(HqVoiceEnd::TimeLimit)
    );
    assert!(model.update(Event::WaitOver { ticket: *wait }).is_empty());
}

#[test]
fn stopping_switching_workspace_or_sleeping_leaves_the_room() {
    let (mut model, session) = live();
    assert_eq!(
        hq(&mut model, HqEvent::StopVoice),
        [Effect::DisconnectMedia { session }]
    );
    assert_eq!(voice(&model).phase, HqVoicePhase::Off);

    let (mut model, session) = live();
    let effects = model.update(Event::Suspending);
    assert!(
        effects.contains(&Effect::DisconnectMedia { session }),
        "{effects:?}"
    );

    let (mut model, session) = live();
    let effects = model.update(Event::SelectWorkspace(VIEWER.to_owned()));
    assert!(
        effects.contains(&Effect::DisconnectMedia { session }),
        "{effects:?}"
    );
    assert_eq!(*voice(&model), HqVoice::default());
}

#[test]
fn what_an_agent_sends_in_another_kind_of_room_is_not_read() {
    let (mut model, _) = loaded(AGENCY, "agency");
    model.update(Event::Navigate(Route::Rooms));
    model.update(Event::Rooms(RoomsEvent::EditRoomName("standup".to_owned())));
    let effects = model.update(Event::Rooms(RoomsEvent::Start));
    let effects = model.update(Event::RoomTokenIssued {
        ticket: last_ticket(&effects),
        result: Ok(fixture("district-room-token.json")),
    });
    let [Effect::ConnectMedia { session, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let effects = packet(&mut model, *session, "hq-voice-packet-proposal.json");
    assert!(effects.is_empty());
    assert!(voice(&model).cards.is_empty());
}

#[test]
fn nothing_an_agent_says_or_sends_is_printed() {
    let events = [
        MediaEvent::AgentData {
            topic: HQ_PROPOSAL_TOPIC.to_owned(),
            payload: b"secret-payload".to_vec(),
        },
        MediaEvent::Speech(SpeechSegment {
            segment_id: "s1".to_owned(),
            speaker: Speaker::Member,
            text: "secret words".to_owned(),
            is_final: true,
        }),
        MediaEvent::AgentState("listening".to_owned()),
    ];
    let shown = format!("{events:?}");
    assert!(!shown.contains("secret"), "{shown}");
    assert!(shown.contains("14 bytes") && shown.contains("listening") && shown.contains("Member"));
}

#[test]
fn the_new_notes_have_their_sentences() {
    assert_eq!(HqNote::Cancelled.text(), "Cancelled. Nothing was changed.");
    assert_eq!(
        HqNote::AlreadyHandled.text(),
        "This change was already handled."
    );
    assert!(HqNote::Expired.text().starts_with("This change expired"));
}
