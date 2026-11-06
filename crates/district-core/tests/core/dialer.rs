//! The dialler and a placed call: the number read in the country on the button
//! and sent as E.164, the home country from the workspace's numbers, the length
//! said once the member stops, one dial at a time and never for a viewer, the service's refusals in its own words, the room joined
//! while the far end rings, the answer when a person joins, the duration by the
//! clock, and every way of ending, each of which ends the carrier's leg too.

use district_api::{ApiError, ErrorDetail};
use district_core::{
    ActiveCall, CALL_TICK, CallDirection, CallEnd, CallEvent, CallPhase, DialStatus, DialerEvent,
    DialerScreen, DisconnectReason, EMPTY_ROOM_GRACE, Effect, Event, FailureText, MediaConnection,
    MediaEvent, MediaSession, MicrophoneState, Model, OwnedNumbersList, Route, SessionState,
    Ticket,
};
use district_model::DialResponse;

use crate::settings::stale;
use crate::support::{
    AGENCY, CLIENT, VIEWER, and_transcript_read, connect, fixture, loaded, media, person, service,
    signed_in, signed_out_error,
};

/// A number in the fictional range, as a member might type it.
const TYPED: &str = "+1 (212) 555-0142";
/// The same number as the service is sent it.
const E164: &str = "+12125550142";

fn dialled() -> DialResponse {
    fixture("district-dial.json")
}

fn call(model: &Model) -> &ActiveCall {
    signed_in(model).active_call.as_ref().expect("a call")
}

fn phase(model: &Model) -> &CallPhase {
    &call(model).phase
}

/// On the dialler of `workspace` as `role`, the workspace's numbers read (two
/// in Toronto and a toll-free one), with nothing typed.
fn at_empty_dialler(workspace: &str, role: &str) -> Model {
    let (mut model, _) = loaded(workspace, role);
    let effects = model.update(Event::Navigate(Route::Dialer));
    let [
        Effect::LoadOwnedNumbers {
            ticket,
            workspace_id,
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(workspace_id, workspace);
    assert_eq!(signed_in(&model).route, Route::Dialer);
    assert_eq!(
        signed_in(&model).dialer.country,
        None,
        "no country before the read"
    );
    model.update(Event::OwnedNumbersLoaded {
        ticket: *ticket,
        result: Ok(fixture("district-provider-numbers.json")),
    });
    model
}

/// On the dialler of `workspace` as `role`, with the number typed.
fn at_dialler(workspace: &str, role: &str) -> Model {
    let mut model = at_empty_dialler(workspace, role);
    model.update(Event::Dialer(DialerEvent::Edit(TYPED.to_owned())));
    model
}

fn edit(model: &mut Model, text: &str) {
    assert!(
        model
            .update(Event::Dialer(DialerEvent::Edit(text.to_owned())))
            .is_empty()
    );
}

/// Presses Call, and returns the dial's ticket.
fn place(model: &mut Model) -> Ticket {
    let effects = model.update(Event::Dialer(DialerEvent::Dial));
    let [
        Effect::Dial {
            ticket,
            workspace_id,
            to,
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(workspace_id, AGENCY);
    assert_eq!(to, E164, "sent as the line before Call showed it");
    *ticket
}

/// A dial answered: the room joined, and the session's name.
fn placed(model: &mut Model) -> Ticket {
    let ticket = place(model);
    let effects = model.update(Event::Dialled {
        ticket,
        result: Ok(dialled()),
    });
    let (session, credential, microphone) = connect(&effects);
    assert_eq!(effects.len(), 2, "{effects:?}");
    // The call has its id now, and its live transcript is asked for.
    assert!(
        matches!(
            &effects[1],
            Effect::WatchTranscript { transcript: Some(watch), .. }
                if watch.workspace_id == AGENCY && watch.call_id == dialled().call_id
        ),
        "{effects:?}"
    );
    assert_eq!(credential.url(), dialled().url);
    assert_eq!(credential.token(), dialled().token);
    assert_eq!(credential.passphrase(), None, "a phone call is unencrypted");
    assert!(microphone, "a call is placed to be heard");
    assert_eq!(*phase(model), CallPhase::Connecting);
    session
}

/// A placed call the far end picked up, and the ticket of its first second.
fn answered(model: &mut Model) -> (Ticket, Ticket) {
    let session = placed(model);
    assert!(
        model
            .update(media(session, MediaEvent::Connecting))
            .is_empty()
    );
    assert!(
        model
            .update(media(session, MediaEvent::Connected))
            .is_empty()
    );
    assert_eq!(*phase(model), CallPhase::Ringing);
    assert_eq!(call(model).status(), ActiveCall::RINGING);
    let effects = model.update(media(
        session,
        MediaEvent::ParticipantJoined(person("sip_callee")),
    ));
    let [Effect::Wait { ticket, delay }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(*delay, CALL_TICK);
    assert_eq!(*phase(model), CallPhase::InCall);
    (session, *ticket)
}

fn hang_up(model: &mut Model) -> Vec<Effect> {
    model.update(Event::Call(CallEvent::HangUp))
}

fn carrier_hang_up() -> Effect {
    Effect::HangUpCall {
        workspace_id: AGENCY.to_owned(),
        call_id: dialled().call_id,
    }
}

fn refused(status: u16, message: &str, code: Option<&str>) -> ApiError {
    let detail = ErrorDetail {
        message: Some(message.to_owned()),
        code: code.map(str::to_owned),
        ..ErrorDetail::default()
    };
    match (status, code) {
        (403, _) => ApiError::Forbidden(detail),
        (409, _) => ApiError::Conflict(detail),
        (_, Some(code)) => ApiError::Envelope {
            status,
            code: code.to_owned(),
            detail,
        },
        _ => ApiError::Rejected { status, detail },
    }
}

#[test]
fn a_viewer_is_not_offered_the_dialler_and_cannot_dial() {
    let (mut model, _) = loaded(VIEWER, "viewer");
    assert!(model.update(Event::Navigate(Route::Dialer)).is_empty());
    assert_ne!(signed_in(&model).route, Route::Dialer);
    assert!(!signed_in(&model).capabilities().can_dial);
    model.update(Event::Dialer(DialerEvent::Edit(TYPED.to_owned())));
    assert!(!signed_in(&model).can_place_call());
    assert!(model.update(Event::Dialer(DialerEvent::Dial)).is_empty());
    assert_eq!(signed_in(&model).active_call, None);

    // A client member dials like an agency member.
    let (model, _) = loaded(CLIENT, "client");
    assert!(signed_in(&model).capabilities().can_dial);
}

#[test]
fn the_number_moves_the_flag_reads_locally_and_is_sent_as_e164() {
    let mut model = at_dialler(AGENCY, "agency");
    let dialer = &signed_in(&model).dialer;
    assert_eq!(
        dialer.home.as_deref(),
        Some("CA"),
        "two Toronto numbers outvote nothing"
    );
    assert_eq!(
        dialer.country.as_deref(),
        Some("US"),
        "the +1 212 area code moved the flag"
    );
    assert_eq!(
        dialer.entry, "(212) 555-0142",
        "the field keeps the local form"
    );
    assert_eq!(
        dialer.destination().as_deref(),
        Some("Rings United States \u{b7} +1 212 555 0142")
    );
    assert_eq!(dialer.reading().e164.as_deref(), Some(E164));
    assert!(signed_in(&model).can_place_call());

    edit(&mut model, "(212) 555-01");
    assert!(!signed_in(&model).can_place_call(), "not a whole number");
    assert!(model.update(Event::Dialer(DialerEvent::Dial)).is_empty());
}

#[test]
fn the_dialler_opens_on_the_workspaces_own_country() {
    let mut model = at_empty_dialler(AGENCY, "agency");
    let dialer = &signed_in(&model).dialer;
    assert_eq!(dialer.country.as_deref(), Some("CA"));
    assert_eq!(dialer.placeholder(), "(416) 555-0199");
    assert_eq!(dialer.suggested(), ["CA", "US"]);
    edit(&mut model, "4165550199");
    assert_eq!(signed_in(&model).dialer.entry, "(416) 555-0199");
    assert!(signed_in(&model).can_place_call());

    // Back and in again: the numbers already read are not read again.
    model.update(Event::Navigate(Route::Calls));
    assert!(model.update(Event::Navigate(Route::Dialer)).is_empty());
    assert_eq!(signed_in(&model).dialer.home.as_deref(), Some("CA"));
}

#[test]
fn a_failed_read_leaves_the_member_to_pick_a_country() {
    let (mut model, _) = loaded(AGENCY, "agency");
    let effects = model.update(Event::Navigate(Route::Dialer));
    let ticket = crate::support::ticket(&effects[0]);
    model.update(Event::OwnedNumbersLoaded {
        ticket,
        result: Err(crate::support::server_error()),
    });
    assert!(matches!(
        signed_in(&model).marketplace.owned,
        OwnedNumbersList::Failed(_)
    ));
    let dialer = &signed_in(&model).dialer;
    assert_eq!(
        (dialer.home.as_deref(), dialer.country.as_deref()),
        (None, None)
    );
    edit(&mut model, "4165550199");
    assert_eq!(
        signed_in(&model).dialer.problem().as_deref(),
        Some("Choose a country, or start with + and the country code.")
    );
    assert!(
        model
            .update(Event::Dialer(DialerEvent::PickCountry("CA".to_owned())))
            .is_empty()
    );
    assert_eq!(signed_in(&model).dialer.entry, "(416) 555-0199");
    assert!(signed_in(&model).can_place_call());

    // In again after the failure: read again.
    model.update(Event::Navigate(Route::Calls));
    let effects = model.update(Event::Navigate(Route::Dialer));
    assert!(
        matches!(effects.as_slice(), [Effect::LoadOwnedNumbers { .. }]),
        "{effects:?}"
    );
}

/// The billed call: a Canadian mobile typed with +41 in front of its area code
/// is Switzerland's code. The flag moves there, nothing is sent, and the
/// reason is said once the member stops, never while typing.
#[test]
fn the_switzerland_misdial_is_never_placed_and_says_why_once_stopped() {
    let mut model = at_empty_dialler(AGENCY, "agency");
    let swiss = format!("+{}{}", "41", "65550123");
    edit(&mut model, &swiss);
    let dialer = &signed_in(&model).dialer;
    assert_eq!(dialer.country.as_deref(), Some("CH"));
    assert_eq!(dialer.reading().status, DialStatus::Typing);
    assert_eq!(
        (dialer.destination(), dialer.problem()),
        (None, None),
        "nothing while typing"
    );
    assert!(!signed_in(&model).can_place_call());

    let wrong = "That is the wrong number of digits for a number in the country it rings. \
                 Check it before you call.";
    // Leaving the field, or Enter, says it.
    assert!(
        model
            .update(Event::Dialer(DialerEvent::CheckLength))
            .is_empty()
    );
    assert_eq!(signed_in(&model).dialer.problem().as_deref(), Some(wrong));
    // The next keystroke clears it.
    edit(&mut model, "65 550 12 3");
    assert_eq!(signed_in(&model).dialer.problem(), None);
    // A refused Call says it too, and sends nothing.
    assert!(model.update(Event::Dialer(DialerEvent::Dial)).is_empty());
    assert_eq!(signed_in(&model).dialer.problem().as_deref(), Some(wrong));
    assert_eq!(signed_in(&model).active_call, None);
}

#[test]
fn an_emergency_number_is_refused_with_the_hand_off() {
    let mut model = at_empty_dialler(AGENCY, "agency");
    edit(&mut model, "911");
    let dialer = &signed_in(&model).dialer;
    assert_eq!(dialer.entry, "911");
    assert_eq!(dialer.reading().status, DialStatus::Emergency);
    assert!(dialer.problem().unwrap().contains("own dialer"));
    assert!(!signed_in(&model).can_place_call());
    assert!(model.update(Event::Dialer(DialerEvent::Dial)).is_empty());
}

#[test]
fn one_dial_at_a_time_and_the_call_shows_at_once() {
    let mut model = at_dialler(AGENCY, "agency");
    place(&mut model);
    let call = call(&model);
    assert_eq!(
        call.direction,
        CallDirection::Outbound {
            number: E164.to_owned()
        }
    );
    assert_eq!(call.workspace_id, AGENCY);
    assert_eq!(call.phase, CallPhase::Dialing);
    assert_eq!(call.title(), "+1 212 555 0142");
    assert_eq!(call.status(), ActiveCall::DIALING);
    assert!(!call.is_over() && !call.was_answered());
    assert!(signed_in(&model).media_busy());
    assert!(!signed_in(&model).can_place_call());
    assert!(
        model.update(Event::Dialer(DialerEvent::Dial)).is_empty(),
        "a second press sends nothing"
    );
}

#[test]
fn a_refusal_is_no_call_and_says_why_in_the_services_words() {
    let refusals = [
        refused(
            403,
            "This number has opted out of calls from this workspace (DNC).",
            Some("do_not_call"),
        ),
        refused(
            402,
            "This workspace's subscription is not active. Please update billing to resume calls \
             and messaging.",
            Some("subscription_inactive"),
        ),
        refused(
            400,
            "District AI cannot place emergency calls. Use your phone's own dialer to call for \
             help.",
            Some("emergency_number"),
        ),
        refused(409, "Included minutes used.", Some("overage_cap_reached")),
    ];
    for error in refusals {
        let mut model = at_dialler(AGENCY, "agency");
        let ticket = place(&mut model);
        let expected = FailureText::from_api_error(&error);
        let effects = model.update(Event::Dialled {
            ticket,
            result: Err(error),
        });
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(
            *phase(&model),
            CallPhase::Ended(CallEnd::NotPlaced(expected.clone()))
        );
        assert_eq!(call(&model).status(), expected.message);
        assert_eq!(signed_in(&model).media, None);

        // Put away, the dialler is ready again, the number still typed.
        assert!(model.update(Event::Call(CallEvent::Dismiss)).is_empty());
        assert_eq!(signed_in(&model).active_call, None);
        assert!(signed_in(&model).can_place_call());
    }
}

#[test]
fn a_call_is_answered_when_a_person_joins_and_timed_from_then() {
    let mut model = at_dialler(AGENCY, "agency");
    let (session, first) = answered(&mut model);
    assert!(call(&model).was_answered());
    assert_eq!(call(&model).status(), "00:00");
    let effects = model.update(Event::WaitOver { ticket: first });
    let [Effect::Wait { ticket: second, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    model.update(Event::WaitOver { ticket: *second });
    assert_eq!(call(&model).elapsed_secs, 2);
    assert_eq!(call(&model).status(), "00:02");

    // The receptionist is not a person: a service joining changes nothing,
    // and neither does another person leaving while one stays.
    assert!(
        model
            .update(media(
                session,
                MediaEvent::ParticipantJoined(service("agent-1"))
            ))
            .is_empty()
    );
    let people = signed_in(&model).media.as_ref().unwrap();
    assert_eq!(people.people().len(), 1);
    assert!(people.service_present());
}

#[test]
fn hanging_up_ends_the_carrier_leg_and_leaves_the_room_once() {
    let mut model = at_dialler(AGENCY, "agency");
    let (session, tick) = answered(&mut model);
    model.update(Event::WaitOver { ticket: tick });
    let effects = and_transcript_read(hang_up(&mut model));
    assert_eq!(
        effects,
        [carrier_hang_up(), Effect::DisconnectMedia { session }]
    );
    assert_eq!(*phase(&model), CallPhase::Ended(CallEnd::HungUp));
    assert_eq!(call(&model).status(), "Call ended. It lasted 00:01.");
    assert!(call(&model).is_over() && call(&model).was_answered());
    assert_eq!(signed_in(&model).media, None);

    assert!(hang_up(&mut model).is_empty(), "once");
    // The duration stopped, and a report from the room just left is dropped.
    assert!(model.update(Event::WaitOver { ticket: tick }).is_empty());
    assert!(
        model
            .update(media(
                session,
                MediaEvent::Disconnected(DisconnectReason::Left)
            ))
            .is_empty()
    );
    assert_eq!(call(&model).elapsed_secs, 1);
}

/// The far end leaving opens the grace window, and the room still empty
/// when it is over is them hanging up.
fn far_end_left(model: &mut Model, session: Ticket) -> Ticket {
    let effects = model.update(media(
        session,
        MediaEvent::ParticipantLeft {
            identity: "sip_callee".to_owned(),
        },
    ));
    let [Effect::Wait { ticket, delay }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(*delay, EMPTY_ROOM_GRACE);
    assert_eq!(*phase(model), CallPhase::InCall, "not yet");
    *ticket
}

#[test]
fn the_far_end_hanging_up_ends_the_call_here_and_at_the_carrier() {
    let mut model = at_dialler(AGENCY, "agency");
    let (session, _) = answered(&mut model);
    let window = far_end_left(&mut model, session);
    // A service coming and going meanwhile is no person: the one window stays.
    assert!(
        model
            .update(media(
                session,
                MediaEvent::ParticipantJoined(service("agent-1"))
            ))
            .is_empty()
    );
    let effects = model.update(Event::WaitOver { ticket: window });
    assert_eq!(
        and_transcript_read(effects),
        [carrier_hang_up(), Effect::DisconnectMedia { session }]
    );
    assert_eq!(*phase(&model), CallPhase::Ended(CallEnd::Remote));
}

/// A rebuilt connection can report the far end leaving before it says it is
/// reconnecting. Within the window that report comes, and the leave ends
/// nothing: the call looks again once it is back.
#[test]
fn a_leave_before_the_connection_says_it_is_reconnecting_ends_nothing() {
    let mut model = at_dialler(AGENCY, "agency");
    let (session, _) = answered(&mut model);
    let window = far_end_left(&mut model, session);
    assert!(
        model
            .update(media(session, MediaEvent::Reconnecting))
            .is_empty()
    );
    // The window would be over while the connection is still being rebuilt.
    assert!(
        model.update(Event::WaitOver { ticket: window }).is_empty(),
        "the window closed with the connection"
    );
    assert_eq!(*phase(&model), CallPhase::InCall);
    for event in [
        MediaEvent::ParticipantJoined(person("sip_callee")),
        MediaEvent::Connected,
    ] {
        let effects = model.update(media(session, event.clone()));
        assert!(effects.is_empty(), "{event:?}: {effects:?}");
    }
    assert_eq!(*phase(&model), CallPhase::InCall);
}

#[test]
fn the_far_end_back_within_the_window_ends_nothing() {
    let mut model = at_dialler(AGENCY, "agency");
    let (session, _) = answered(&mut model);
    let window = far_end_left(&mut model, session);
    assert!(
        model
            .update(media(
                session,
                MediaEvent::ParticipantJoined(person("sip_callee"))
            ))
            .is_empty()
    );
    assert!(model.update(Event::WaitOver { ticket: window }).is_empty());
    assert_eq!(*phase(&model), CallPhase::InCall);
}

#[test]
fn a_call_hung_up_here_within_the_window_is_not_ended_again() {
    let mut model = at_dialler(AGENCY, "agency");
    let (session, _) = answered(&mut model);
    let window = far_end_left(&mut model, session);
    hang_up(&mut model);
    assert_eq!(*phase(&model), CallPhase::Ended(CallEnd::HungUp));
    assert!(model.update(Event::WaitOver { ticket: window }).is_empty());
    assert_eq!(*phase(&model), CallPhase::Ended(CallEnd::HungUp));
}

/// A resume that fails becomes a full reconnection, and the media library
/// reports everyone as having left before it reports them back. That is the
/// connection, not the far end: the call goes on.
#[test]
fn a_rebuilt_connection_reporting_the_far_end_gone_and_back_ends_nothing() {
    let mut model = at_dialler(AGENCY, "agency");
    let (session, _) = answered(&mut model);
    for event in [
        MediaEvent::Reconnecting,
        MediaEvent::ParticipantLeft {
            identity: "sip_callee".to_owned(),
        },
        MediaEvent::ParticipantJoined(person("sip_callee")),
        MediaEvent::Connected,
    ] {
        let effects = model.update(media(session, event.clone()));
        assert!(effects.is_empty(), "{event:?}: {effects:?}");
    }
    assert_eq!(*phase(&model), CallPhase::InCall);
    let held = signed_in(&model).media.as_ref().expect("still in the room");
    assert_eq!(held.connection, MediaConnection::Connected);
    assert_eq!(held.people().len(), 1);
}

/// The far end hanging up while the connection was down is found once it is
/// back: nobody returned, so the call ends here and at the carrier.
#[test]
fn a_far_end_gone_when_the_connection_comes_back_ends_the_call() {
    let mut model = at_dialler(AGENCY, "agency");
    let (session, _) = answered(&mut model);
    assert!(
        model
            .update(media(session, MediaEvent::Reconnecting))
            .is_empty()
    );
    assert!(
        model
            .update(media(
                session,
                MediaEvent::ParticipantLeft {
                    identity: "sip_callee".to_owned(),
                },
            ))
            .is_empty(),
        "not while reconnecting"
    );
    assert_eq!(*phase(&model), CallPhase::InCall);
    // Back, and nobody returned: the window opens, and ends the call.
    let effects = model.update(media(session, MediaEvent::Connected));
    let [Effect::Wait { ticket, delay }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(*delay, EMPTY_ROOM_GRACE);
    let effects = model.update(Event::WaitOver { ticket: *ticket });
    assert_eq!(
        and_transcript_read(effects),
        [carrier_hang_up(), Effect::DisconnectMedia { session }]
    );
    assert_eq!(*phase(&model), CallPhase::Ended(CallEnd::Remote));
}

#[test]
fn a_room_that_ends_before_an_answer_ends_the_call_unanswered() {
    let mut model = at_dialler(AGENCY, "agency");
    let session = placed(&mut model);
    model.update(media(session, MediaEvent::Connected));
    let effects = model.update(media(
        session,
        MediaEvent::Disconnected(DisconnectReason::RoomEnded),
    ));
    assert_eq!(
        and_transcript_read(effects),
        [carrier_hang_up()],
        "the room is already gone"
    );
    assert_eq!(*phase(&model), CallPhase::Ended(CallEnd::Remote));
    assert!(!call(&model).was_answered());
    assert_eq!(call(&model).status(), ActiveCall::ENDED);
}

#[test]
fn a_room_that_cannot_be_joined_is_a_failed_call() {
    let mut model = at_dialler(AGENCY, "agency");
    let session = placed(&mut model);
    let effects = model.update(media(
        session,
        MediaEvent::Disconnected(DisconnectReason::ConnectFailed),
    ));
    assert_eq!(and_transcript_read(effects), [carrier_hang_up()]);
    let CallPhase::Ended(CallEnd::Failed(failure)) = phase(&model) else {
        panic!("{:?}", phase(&model));
    };
    assert_eq!(failure.message, ActiveCall::CONNECT_FAILED);
    assert!(!failure.retryable);
    assert_eq!(call(&model).status(), ActiveCall::CONNECT_FAILED);
}

#[test]
fn a_call_through_an_engine_that_can_join_nothing_fails_saying_so() {
    let mut model = at_dialler(AGENCY, "agency");
    let session = placed(&mut model);
    let effects = model.update(media(
        session,
        MediaEvent::Disconnected(DisconnectReason::Unavailable),
    ));
    assert_eq!(
        and_transcript_read(effects),
        [carrier_hang_up()],
        "the telephone leg is ended"
    );
    let CallPhase::Ended(CallEnd::Failed(failure)) = phase(&model) else {
        panic!("{:?}", phase(&model));
    };
    assert_eq!(failure.message, DisconnectReason::UNAVAILABLE);
    assert!(!failure.retryable, "trying again would fail the same way");
}

#[test]
fn a_callee_who_picked_up_before_the_room_was_joined_is_answered_on_joining() {
    let mut model = at_dialler(AGENCY, "agency");
    let session = placed(&mut model);
    model.update(media(
        session,
        MediaEvent::ParticipantJoined(person("sip_callee")),
    ));
    assert_eq!(*phase(&model), CallPhase::Connecting);
    let effects = model.update(media(session, MediaEvent::Connected));
    assert!(matches!(effects.as_slice(), [Effect::Wait { .. }]));
    assert_eq!(*phase(&model), CallPhase::InCall);
}

#[test]
fn hanging_up_while_the_dial_is_on_its_way_ends_the_call_the_answer_names() {
    let mut model = at_dialler(AGENCY, "agency");
    let ticket = place(&mut model);
    assert!(hang_up(&mut model).is_empty(), "no id to end yet");
    assert_eq!(*phase(&model), CallPhase::Ended(CallEnd::HungUp));
    assert!(
        !signed_in(&model).can_place_call(),
        "not while the dial is out"
    );
    let effects = model.update(Event::Dialled {
        ticket,
        result: Ok(dialled()),
    });
    assert_eq!(effects, [carrier_hang_up()], "no room is joined");
    assert_eq!(*phase(&model), CallPhase::Ended(CallEnd::HungUp));
    assert_eq!(signed_in(&model).media, None);

    // Dismissed before the answer: still ended at the carrier.
    let mut model = at_dialler(AGENCY, "agency");
    let ticket = place(&mut model);
    hang_up(&mut model);
    model.update(Event::Call(CallEvent::Dismiss));
    let effects = model.update(Event::Dialled {
        ticket,
        result: Ok(dialled()),
    });
    assert_eq!(effects, [carrier_hang_up()]);
    assert_eq!(signed_in(&model).active_call, None);

    // Refused after the member hung up: nothing to end, nothing to say.
    let mut model = at_dialler(AGENCY, "agency");
    let ticket = place(&mut model);
    hang_up(&mut model);
    let effects = model.update(Event::Dialled {
        ticket,
        result: Err(refused(403, "Opted out.", None)),
    });
    assert!(effects.is_empty());
    assert_eq!(*phase(&model), CallPhase::Ended(CallEnd::HungUp));
    assert!(signed_in(&model).can_place_call());
}

#[test]
fn a_dial_answered_with_a_room_this_client_would_not_join_is_ended_at_once() {
    for answer in [
        DialResponse {
            room_name: "call_ws-contract-active-CA01".to_owned(),
            ..dialled()
        },
        DialResponse {
            token: String::new(),
            ..dialled()
        },
    ] {
        let mut model = at_dialler(AGENCY, "agency");
        let ticket = place(&mut model);
        let effects = model.update(Event::Dialled {
            ticket,
            result: Ok(answer),
        });
        assert_eq!(effects, [carrier_hang_up()]);
        assert_eq!(
            *phase(&model),
            CallPhase::Ended(CallEnd::NotPlaced(FailureText::from_api_error(
                &ApiError::Decode {
                    endpoint: district_api::Endpoint::CallDial,
                    line: 1,
                    column: 1,
                }
            )))
        );
        assert_eq!(signed_in(&model).media, None);
    }
}

#[test]
fn a_call_goes_on_through_a_change_of_workspace() {
    let mut model = at_dialler(AGENCY, "agency");
    let ticket = place(&mut model);
    let effects = model.update(Event::SelectWorkspace(CLIENT.to_owned()));
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::DisconnectMedia { .. })),
        "{effects:?}"
    );
    assert_eq!(
        signed_in(&model).route,
        Route::Overview,
        "the dialler waits"
    );
    assert_eq!(signed_in(&model).dialer, DialerScreen::default());
    let effects = model.update(Event::Dialled {
        ticket,
        result: Ok(dialled()),
    });
    let (session, _, _) = connect(&effects);
    assert_eq!(
        call(&model).workspace_id,
        AGENCY,
        "the call's own workspace"
    );
    model.update(media(session, MediaEvent::Connected));
    model.update(media(
        session,
        MediaEvent::ParticipantJoined(person("sip_callee")),
    ));
    let effects = hang_up(&mut model);
    assert_eq!(effects[0], carrier_hang_up());
}

#[test]
fn the_microphone_follows_what_the_engine_reports_and_the_line_says_when_it_cannot() {
    let mut model = at_dialler(AGENCY, "agency");
    let (session, _) = answered(&mut model);
    assert_eq!(
        model.update(Event::Microphone(false)),
        [Effect::SetMicrophone {
            session,
            enabled: false
        }]
    );
    let held = |model: &Model| signed_in(model).media.clone().unwrap();
    assert_eq!(held(&model).microphone, MicrophoneState::Off);
    model.update(media(session, MediaEvent::Microphone(MicrophoneState::On)));
    assert_eq!(held(&model).microphone, MicrophoneState::On);
    assert_eq!(held(&model).notice(), None);
    model.update(media(
        session,
        MediaEvent::Microphone(MicrophoneState::Unavailable),
    ));
    assert_eq!(
        held(&model).notice(),
        Some(MediaSession::MICROPHONE_UNAVAILABLE)
    );

    // A connection lost for a moment is a banner, not an end.
    model.update(media(session, MediaEvent::Reconnecting));
    assert_eq!(held(&model).connection, MediaConnection::Reconnecting);
    assert_eq!(held(&model).notice(), Some(MediaSession::RECONNECTING));
    assert!(
        model
            .update(media(session, MediaEvent::Connected))
            .is_empty()
    );
    assert_eq!(*phase(&model), CallPhase::InCall);
    assert_eq!(held(&model).connection, MediaConnection::Connected);

    // With nothing held, the microphone has nothing to change.
    hang_up(&mut model);
    assert!(model.update(Event::Microphone(true)).is_empty());
}

#[test]
fn a_dial_answer_nobody_awaits_is_dropped() {
    let mut model = at_dialler(AGENCY, "agency");
    place(&mut model);
    assert!(
        model
            .update(Event::Dialled {
                ticket: stale(),
                result: Ok(dialled()),
            })
            .is_empty()
    );
    assert_eq!(*phase(&model), CallPhase::Dialing);

    // Nor does anything land once the session is signing out.
    let mut model = at_dialler(AGENCY, "agency");
    let ticket = place(&mut model);
    model.update(Event::SignOut);
    assert!(
        model
            .update(Event::Dialled {
                ticket,
                result: Ok(dialled()),
            })
            .is_empty()
    );
}

#[test]
fn a_dial_refused_because_the_session_ended_signs_out() {
    let mut model = at_dialler(AGENCY, "agency");
    let ticket = place(&mut model);
    model.update(Event::Dialled {
        ticket,
        result: Err(signed_out_error()),
    });
    assert!(matches!(model.session(), SessionState::SignedOut(_)));
}

#[test]
fn a_duration_reads_as_a_call_timer() {
    let mut model = at_dialler(AGENCY, "agency");
    let (_, mut tick) = answered(&mut model);
    for _ in 0..75 {
        let effects = model.update(Event::WaitOver { ticket: tick });
        tick = crate::support::ticket(&effects[0]);
    }
    assert_eq!(call(&model).status(), "01:15");
}
