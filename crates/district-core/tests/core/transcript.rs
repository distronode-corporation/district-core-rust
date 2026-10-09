//! The live transcript of the call on this desktop: asked for once the call has
//! an id, read from the service's recorded frames into the call's lines in
//! order, interim lines replaced by their finals, gaps healed, refusals and
//! ends followed as district-core-swift 6.0.0 follows them, and everything
//! that is not this call's, cannot be read, or comes after the hang-up left
//! alone.

use std::time::Duration;

use district_api::{ApiError, ErrorDetail};
use district_core::{
    CallEvent, DialerEvent, Effect, Event, FINAL_FETCH_ATTEMPTS, FinalTranscript, GAP_HEAL,
    LiveTranscript, LiveTranscriptPhase, MediaEvent, Model, NOT_LIVE_RETRY, RATE_LIMIT_FALLBACK,
    RingEvent, Route, SessionState, Ticket, TranscriptWatch, final_fetch_delay,
};
use district_live::{LiveUpdate, WorkspaceUpdate};
use district_model::{
    CallAnswerResponse, CallTranscriptResponse, DialResponse, TelemetryEnvelope,
    TelemetryEventType, TranscriptEndReason, TranscriptErrorCode, TranscriptSegment,
};
use serde_json::{Value, json};

use crate::support::{
    AGENCY, CLIENT, USER, call_event, connect, fixture, live_event, loaded, media, person, pick,
    ring_here, ringing, server_error, signed_in, signed_out_error,
};

const CALL: &str = "call_live_1";
/// The epoch every recorded frame names.
const EPOCH: i64 = 1_791_297_000_000;

fn answer() -> CallAnswerResponse {
    fixture("district-call-answer.json")
}

/// What the call's socket is asked for, with `resubscribes` heals so far.
fn watching(resubscribes: u32) -> Option<TranscriptWatch> {
    Some(TranscriptWatch {
        workspace_id: AGENCY.to_owned(),
        call_id: CALL.to_owned(),
        resubscribes,
    })
}

/// The one request to the socket in `effects`, if there is one.
fn watched(effects: &[Effect]) -> Option<Option<TranscriptWatch>> {
    let found: Vec<&Option<TranscriptWatch>> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::WatchTranscript { transcript, .. } => Some(transcript),
            _ => None,
        })
        .collect();
    match found.as_slice() {
        [] => None,
        [one] => Some((*one).clone()),
        _ => panic!("more than one request in {effects:?}"),
    }
}

/// The one wait in `effects`, and how long it is.
fn wait(effects: &[Effect]) -> (Ticket, Duration) {
    let found: Vec<(Ticket, Duration)> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Wait { ticket, delay } => Some((*ticket, *delay)),
            _ => None,
        })
        .collect();
    match found.as_slice() {
        [one] => *one,
        _ => panic!("not exactly one wait in {effects:?}"),
    }
}

/// Signed in, with a call rung here and answered, its media up, and its live
/// transcript asked for.
fn on_call() -> Model {
    on_call_with(CALL)
}

fn on_call_with(call_id: &str) -> Model {
    let (mut model, _) = loaded(AGENCY, "agency");
    ring_here(&mut model);
    model.update(ringing(AGENCY, call_id, &[USER]));
    let effects = model.update(Event::Ring(RingEvent::Answer {
        call_id: call_id.to_owned(),
    }));
    let ticket = pick(&effects, |e| matches!(e, Effect::AnswerCall { .. }));
    let effects = model.update(Event::CallAnswered {
        ticket,
        result: Ok(answer()),
    });
    let (session, _, _) = connect(&effects);
    model.update(media(session, MediaEvent::Connected));
    model
}

fn transcript(model: &Model) -> &LiveTranscript {
    signed_in(model)
        .active_call
        .as_ref()
        .expect("a call")
        .transcript()
        .expect("a live transcript")
}

fn phase(model: &Model) -> &LiveTranscriptPhase {
    transcript(model).phase()
}

/// The ids of the lines shown, in order.
fn ids(model: &Model) -> Vec<String> {
    transcript(model)
        .lines()
        .into_iter()
        .map(|line| line.segment_id.clone())
        .collect()
}

fn line<'a>(model: &'a Model, id: &str) -> &'a TranscriptSegment {
    transcript(model)
        .lines()
        .into_iter()
        .find(|line| line.segment_id == id)
        .expect(id)
}

/// A recorded frame, about this call in this workspace, with `edit` made to
/// its data.
fn recorded(name: &str, edit: impl FnOnce(&mut Value)) -> TelemetryEnvelope {
    let mut envelope: TelemetryEnvelope = fixture(name);
    envelope.workspace_id = AGENCY.to_owned();
    envelope.call_id = CALL.to_owned();
    envelope.data["callId"] = json!(CALL);
    edit(&mut envelope.data);
    envelope
}

fn frame(name: &str, edit: impl FnOnce(&mut Value)) -> Event {
    live_event(recorded(name, edit))
}

/// The recorded snapshot: two final lines, `item_a1` and `item_b2`, up to seq 2.
fn snapshot() -> Event {
    frame("telemetry-event-transcript-snapshot.json", |_| {})
}

fn snapshot_with(edit: impl FnOnce(&mut Value)) -> Event {
    frame("telemetry-event-transcript-snapshot.json", edit)
}

/// A segment of `EPOCH` with this id, index, seq and revision.
fn segment(id: &str, index: i64, seq: i64, rev: i64, is_final: bool) -> Event {
    segment_of(EPOCH, id, index, seq, rev, is_final)
}

fn segment_of(epoch: i64, id: &str, index: i64, seq: i64, rev: i64, is_final: bool) -> Event {
    frame("telemetry-event-transcript-segment.json", |data| {
        let segment = &mut data["segment"];
        segment["segmentId"] = json!(id);
        segment["index"] = json!(index);
        segment["epoch"] = json!(epoch);
        segment["seq"] = json!(seq);
        segment["rev"] = json!(rev);
        segment["final"] = json!(is_final);
        segment["text"] = json!(format!("{id} rev {rev}"));
    })
}

fn ended(epoch: i64, seq: i64, reason: &str) -> Event {
    frame("telemetry-event-transcript-ended.json", |data| {
        data["epoch"] = json!(epoch);
        data["seq"] = json!(seq);
        data["reason"] = json!(reason);
    })
}

fn error(op: &str, code: &str, retry_after_ms: Option<i64>) -> Event {
    frame("telemetry-event-transcript-error.json", |data| {
        data["op"] = json!(op);
        data["code"] = json!(code);
        data["retryAfterMs"] = json!(retry_after_ms);
    })
}

fn subscribe_refused(code: &str) -> Event {
    error("transcript.subscribe", code, None)
}

/// The socket of `workspace_id` opened again.
fn connected(workspace_id: &str) -> Event {
    Event::Live(WorkspaceUpdate {
        workspace_id: workspace_id.to_owned(),
        update: LiveUpdate::Connected,
    })
}

/// A call event for this call with the call row's `status`.
fn call_status(event_type: TelemetryEventType, status: &str) -> Event {
    call_event(
        AGENCY,
        CALL,
        event_type,
        json!({"id": CALL, "status": status}),
    )
}

/// The call ended, the full transcript's first wait run out, and its read sent.
fn reading_full(model: &mut Model) -> Ticket {
    let effects = model.update(Event::Call(CallEvent::HangUp));
    let (wait_ticket, delay) = wait(&effects);
    assert_eq!(delay, final_fetch_delay(1));
    read_due(model, wait_ticket)
}

/// The wait before a read of the full transcript ran out: the read.
fn read_due(model: &mut Model, wait_ticket: Ticket) -> Ticket {
    let effects = model.update(Event::WaitOver {
        ticket: wait_ticket,
    });
    let [
        Effect::LoadTranscript {
            ticket,
            workspace_id,
            call_id,
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!((workspace_id.as_str(), call_id.as_str()), (AGENCY, CALL));
    *ticket
}

fn full(text: &str) -> CallTranscriptResponse {
    CallTranscriptResponse {
        success: true,
        transcript: text.to_owned(),
    }
}

// Asking for it.

#[test]
fn answering_a_call_asks_its_workspaces_socket_for_its_transcript() {
    let (mut model, _) = loaded(AGENCY, "agency");
    ring_here(&mut model);
    model.update(ringing(AGENCY, CALL, &[USER]));
    let effects = model.update(Event::Ring(RingEvent::Answer {
        call_id: CALL.to_owned(),
    }));
    let ticket = pick(&effects, |e| matches!(e, Effect::AnswerCall { .. }));
    let effects = model.update(Event::CallAnswered {
        ticket,
        result: Ok(answer()),
    });
    assert_eq!(watched(&effects), Some(watching(0)));
    let live = transcript(&model);
    assert_eq!(live.call_id(), CALL);
    assert_eq!(*live.phase(), LiveTranscriptPhase::Subscribing);
    assert_eq!(live.status(), Some(LiveTranscript::CONNECTING));
    assert!(live.is_complete() && live.lines().is_empty());
    assert_eq!(*live.final_transcript(), FinalTranscript::NotRequested);
}

#[test]
fn a_call_id_the_service_would_refuse_gets_no_live_transcript() {
    let (mut model, _) = loaded(AGENCY, "agency");
    ring_here(&mut model);
    let odd = "call id with spaces";
    model.update(ringing(AGENCY, odd, &[USER]));
    let effects = model.update(Event::Ring(RingEvent::Answer {
        call_id: odd.to_owned(),
    }));
    let ticket = pick(&effects, |e| matches!(e, Effect::AnswerCall { .. }));
    let effects = model.update(Event::CallAnswered {
        ticket,
        result: Ok(answer()),
    });
    assert_eq!(watched(&effects), None, "{effects:?}");
    let call = signed_in(&model).active_call.as_ref().unwrap();
    assert_eq!(call.transcript(), None);
}

// Reading the frames.

#[test]
fn the_recorded_frames_become_the_calls_lines() {
    let mut model = on_call();
    assert!(model.update(snapshot()).is_empty());
    assert_eq!(*phase(&model), LiveTranscriptPhase::Live);
    assert_eq!(transcript(&model).status(), Some(LiveTranscript::LIVE));
    assert_eq!(ids(&model), ["item_a1", "item_b2"]);
    let agent = line(&model, "item_a1");
    assert_eq!(LiveTranscript::speaker_label(agent), "Ava");
    assert!(agent.text.starts_with("Good afternoon"));
    let caller = line(&model, "item_b2");
    assert_eq!(
        LiveTranscript::speaker_label(caller),
        LiveTranscript::CALLER
    );
    assert!(caller.is_final);

    // The recorded interim and final of the same line: a revision in place.
    let interim = frame("telemetry-event-transcript-segment-interim.json", |data| {
        data["segment"]["segmentId"] = json!("item_c3");
        data["segment"]["index"] = json!(2);
        data["segment"]["seq"] = json!(3);
    });
    assert!(model.update(interim).is_empty());
    assert!(!line(&model, "item_c3").is_final);
    let finished = frame("telemetry-event-transcript-segment.json", |data| {
        data["segment"]["segmentId"] = json!("item_c3");
        data["segment"]["index"] = json!(2);
        data["segment"]["seq"] = json!(4);
    });
    assert!(model.update(finished).is_empty());
    assert!(line(&model, "item_c3").is_final);
    assert_eq!(ids(&model), ["item_a1", "item_b2", "item_c3"]);

    // The recorded end: the call is over for the transcript, and the full one
    // is read after the first wait.
    let effects = model.update(frame("telemetry-event-transcript-ended.json", |data| {
        data["seq"] = json!(5);
    }));
    assert_eq!(wait(&effects).1, final_fetch_delay(1));
    assert_eq!(
        *phase(&model),
        LiveTranscriptPhase::Ended(TranscriptEndReason::CallEnded)
    );
    assert_eq!(transcript(&model).status(), Some(LiveTranscript::ENDED));
    assert_eq!(
        *transcript(&model).final_transcript(),
        FinalTranscript::Fetching { attempt: 1 }
    );
    // The lines stay until the full transcript replaces them.
    assert_eq!(ids(&model).len(), 3);

    // The recorded retraction (the contact erased) takes every line off.
    assert!(
        model
            .update(frame("telemetry-event-transcript-retracted.json", |_| {}))
            .is_empty()
    );
    assert!(ids(&model).is_empty());
    // A late copy of an erased line does not bring it back.
    model.update(segment("item_b2", 1, 6, 1, true));
    assert!(ids(&model).is_empty());
    // And `not_live`, the recorded error, changes nothing about an ended one.
    assert!(
        model
            .update(frame("telemetry-event-transcript-error.json", |_| {}))
            .is_empty()
    );
    assert!(matches!(phase(&model), LiveTranscriptPhase::Ended(_)));
}

#[test]
fn lines_are_shown_by_epoch_then_index_whatever_order_they_arrive_in() {
    let mut model = on_call();
    model.update(snapshot());
    model.update(segment("item_e5", 4, 3, 0, true));
    model.update(segment("item_d4", 3, 4, 0, true));
    // A fresh assistant's first line belongs after every line of the first.
    model.update(segment_of(EPOCH + 1, "item_f1", 0, 1, 0, false));
    // Two lines at one index keep a stable order, by id.
    model.update(segment("item_z9", 3, 5, 0, true));
    assert_eq!(
        ids(&model),
        [
            "item_a1", "item_b2", "item_d4", "item_z9", "item_e5", "item_f1"
        ]
    );
}

#[test]
fn an_interim_line_is_replaced_by_newer_revisions_and_by_its_final_for_good() {
    let mut model = on_call();
    model.update(snapshot());
    model.update(segment("item_c3", 2, 3, 2, false));
    // An older revision that arrives later is not shown over a newer one.
    model.update(segment("item_c3", 2, 4, 1, false));
    assert_eq!(line(&model, "item_c3").rev, 2);
    model.update(segment("item_c3", 2, 5, 3, false));
    assert_eq!(line(&model, "item_c3").text, "item_c3 rev 3");
    // The final wins at any revision, and no interim replaces it after.
    model.update(segment("item_c3", 2, 6, 0, true));
    assert!(line(&model, "item_c3").is_final);
    model.update(segment("item_c3", 2, 7, 9, false));
    assert!(line(&model, "item_c3").is_final);
    assert_eq!(line(&model, "item_c3").rev, 0);
    // Between finals the higher revision wins.
    model.update(segment("item_c3", 2, 8, 2, true));
    model.update(segment("item_c3", 2, 9, 1, true));
    assert_eq!(line(&model, "item_c3").rev, 2);
}

// Duplicates and gaps.

#[test]
fn a_duplicate_is_dropped_and_a_late_frame_fills_its_gap() {
    let mut model = on_call();
    model.update(snapshot());
    // At or below the snapshot's mark: already seen.
    assert!(model.update(segment("item_x", 9, 2, 0, true)).is_empty());
    assert!(!ids(&model).contains(&"item_x".to_owned()));
    // No seq is below 1.
    assert!(model.update(segment("item_x", 9, 0, 0, true)).is_empty());
    assert!(!ids(&model).contains(&"item_x".to_owned()));

    // 3 to 6 missing: a gap, looked at again after its wait.
    let effects = model.update(segment("item_g", 7, 7, 0, true));
    let (gap, delay) = wait(&effects);
    assert_eq!(delay, GAP_HEAL);
    assert!(
        model.update(segment("item_g", 7, 7, 0, true)).is_empty(),
        "a duplicate"
    );
    // Late frames fill it from the middle out; each is shown.
    for (seq, id) in [(4, "item_d"), (3, "item_c"), (6, "item_f"), (5, "item_e")] {
        assert!(model.update(segment(id, seq, seq, 0, true)).is_empty());
        assert!(ids(&model).contains(&id.to_owned()), "{id}");
    }
    // Filled before its wait ran out: nothing to heal.
    assert!(model.update(Event::WaitOver { ticket: gap }).is_empty());
    assert_eq!(ids(&model).len(), 7);
}

#[test]
fn a_gap_still_open_after_its_wait_asks_for_the_call_again_once() {
    let mut model = on_call();
    model.update(snapshot());
    let (gap, _) = wait(&model.update(segment("item_e", 4, 5, 0, true)));
    // Wider while it waits: still one wait.
    assert!(model.update(segment("item_g", 6, 8, 0, true)).is_empty());
    let effects = model.update(Event::WaitOver { ticket: gap });
    assert_eq!(watched(&effects), Some(watching(1)), "{effects:?}");

    // The snapshot that answers replaces the lines, gap and all.
    let effects = model.update(snapshot_with(|data| {
        data["lastSeq"] = json!(8);
    }));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(ids(&model), ["item_a1", "item_b2"]);
    // Its mark is the new one: 8 was seen, 9 is new.
    assert!(model.update(segment("item_g", 6, 8, 0, true)).is_empty());
    assert_eq!(ids(&model).len(), 2);
    assert!(model.update(segment("item_h", 7, 9, 0, true)).is_empty());
    assert_eq!(ids(&model).len(), 3);
}

#[test]
fn no_second_heal_goes_out_while_one_is_unanswered() {
    let mut model = on_call();
    model.update(snapshot());
    let (gap, _) = wait(&model.update(segment("item_e", 4, 5, 0, true)));
    // A part with no part 0 is a broken snapshot: the call is asked for again
    // at once.
    let effects = model.update(snapshot_with(|data| {
        data["part"] = json!(1);
    }));
    assert_eq!(watched(&effects), Some(watching(1)));
    // So the gap's wait, run out meanwhile, asks for nothing more.
    assert!(model.update(Event::WaitOver { ticket: gap }).is_empty());
}

#[test]
fn a_frame_of_an_epoch_the_snapshot_closed_is_dropped() {
    let mut model = on_call();
    model.update(snapshot());
    assert!(
        model
            .update(segment_of(EPOCH - 1, "item_old", 0, 40, 0, true))
            .is_empty()
    );
    assert!(!ids(&model).contains(&"item_old".to_owned()));
    // An older epoch's end, of an epoch still open, is counted and changes
    // nothing: only the newest epoch's end does.
    model.update(segment_of(EPOCH + 1, "item_new", 0, 1, 0, true));
    assert!(model.update(ended(EPOCH, 3, "call_ended")).is_empty());
    assert_eq!(*phase(&model), LiveTranscriptPhase::Live);
}

// Snapshots in parts.

#[test]
fn a_snapshot_in_parts_replaces_the_lines_once_its_last_part_is_in() {
    let mut model = on_call();
    model.update(snapshot());
    model.update(segment("item_c3", 2, 3, 0, true));
    let first = snapshot_with(|data| {
        data["segments"].as_array_mut().unwrap().truncate(1);
        data["more"] = json!(true);
        data["lastSeq"] = json!(3);
    });
    assert!(model.update(first).is_empty());
    assert_eq!(ids(&model).len(), 3, "nothing changes before the last part");
    // An error about another op answers that op: the parts are still whole.
    assert!(
        model
            .update(error("transcript.unsubscribe", "bad_request", None))
            .is_empty()
    );
    let second = snapshot_with(|data| {
        data["segments"].as_array_mut().unwrap().remove(0);
        data["part"] = json!(1);
        data["lastSeq"] = json!(3);
    });
    assert!(model.update(second).is_empty());
    assert_eq!(ids(&model), ["item_a1", "item_b2"]);
}

#[test]
fn a_snapshot_broken_by_another_frame_or_a_changed_header_is_asked_for_again() {
    // Another frame between the parts.
    for interloper in [
        segment("item_c3", 2, 3, 0, true),
        ended(EPOCH, 3, "agent_error"),
        frame("telemetry-event-transcript-retracted.json", |data| {
            data["all"] = json!(false);
            data["segmentIds"] = json!(["item_zz"]);
        }),
    ] {
        let mut model = on_call();
        model.update(snapshot());
        model.update(snapshot_with(|data| data["more"] = json!(true)));
        let effects = model.update(interloper);
        assert_eq!(watched(&effects), Some(watching(1)), "{effects:?}");
    }

    // A header that changed between parts: the rest is let through, then the
    // whole is asked for.
    let mut model = on_call();
    model.update(snapshot_with(|data| data["more"] = json!(true)));
    assert!(
        model
            .update(snapshot_with(|data| {
                data["part"] = json!(1);
                data["more"] = json!(true);
                data["complete"] = json!(false);
            }))
            .is_empty()
    );
    let effects = model.update(snapshot_with(|data| data["part"] = json!(2)));
    assert_eq!(watched(&effects), Some(watching(1)));
    assert!(ids(&model).is_empty(), "nothing was put together");
}

#[test]
fn an_incomplete_snapshot_says_so() {
    let mut model = on_call();
    model.update(snapshot_with(|data| data["complete"] = json!(false)));
    assert!(!transcript(&model).is_complete());
    assert_eq!(
        LiveTranscript::INCOMPLETE,
        "Earlier lines will appear in the full transcript after the call."
    );
}

// A reconnect.

#[test]
fn a_reconnect_mid_call_drops_a_half_snapshot_and_takes_the_new_one() {
    let mut model = on_call();
    model.update(snapshot());
    let (gap, _) = wait(&model.update(segment("item_e", 4, 5, 0, true)));
    model.update(snapshot_with(|data| data["more"] = json!(true)));

    // The socket was lost and opened again; it has asked for the call again on
    // its own, so nothing more is sent, and the gap's wait is moot.
    let effects = model.update(connected(AGENCY));
    assert_eq!(watched(&effects), None, "{effects:?}");
    assert!(model.update(Event::WaitOver { ticket: gap }).is_empty());
    assert_eq!(
        ids(&model),
        ["item_a1", "item_b2", "item_e"],
        "the lines stay"
    );

    // The new socket's snapshot replaces them: the half from the old socket is
    // not part of it.
    let effects = model.update(snapshot_with(|data| {
        data["segments"].as_array_mut().unwrap().truncate(1);
        data["lastSeq"] = json!(6);
    }));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(ids(&model), ["item_a1"]);
    assert_eq!(*phase(&model), LiveTranscriptPhase::Live);

    // Another workspace's socket opening says nothing about this call.
    model.update(snapshot_with(|data| {
        data["more"] = json!(true);
        data["lastSeq"] = json!(6);
    }));
    model.update(connected(CLIENT));
    let effects = model.update(snapshot_with(|data| {
        data["part"] = json!(1);
        data["lastSeq"] = json!(6);
    }));
    assert!(effects.is_empty(), "the parts are still whole: {effects:?}");
    assert_eq!(ids(&model), ["item_a1", "item_b2"]);
}

// What is not this call's, or cannot be read.

#[test]
fn frames_about_another_call_or_workspace_change_nothing() {
    let mut model = on_call();
    model.update(snapshot());
    let line = || {
        let mut envelope = recorded("telemetry-event-transcript-segment.json", |data| {
            data["segment"]["segmentId"] = json!("item_x");
            data["segment"]["seq"] = json!(3);
        });
        envelope.call_id = "call_someone_else".to_owned();
        envelope.data["callId"] = json!("call_someone_else");
        envelope
    };
    // Another call in this workspace.
    assert!(model.update(live_event(line())).is_empty());
    // This call's id in another workspace's socket, watched or not.
    for workspace in [CLIENT, "ws-not-watched"] {
        let mut envelope = recorded("telemetry-event-transcript-segment.json", |data| {
            data["segment"]["segmentId"] = json!("item_x");
            data["segment"]["seq"] = json!(3);
        });
        envelope.workspace_id = workspace.to_owned();
        assert!(model.update(live_event(envelope)).is_empty(), "{workspace}");
    }
    // The envelope names this call, and the data another.
    let mut mismatched = line();
    mismatched.call_id = CALL.to_owned();
    assert!(model.update(live_event(mismatched)).is_empty());
    // An error that names no call is about no call.
    let unnamed = frame("telemetry-event-transcript-error.json", |data| {
        data["callId"] = Value::Null;
        data["code"] = json!("bad_request");
    });
    assert!(model.update(unnamed).is_empty());
    assert_eq!(ids(&model), ["item_a1", "item_b2"]);
    assert_eq!(*phase(&model), LiveTranscriptPhase::Live);
}

#[test]
fn hostile_frames_are_left_unread() {
    let mut model = on_call();
    model.update(snapshot());
    let hostile = [
        // Longer than the contract allows a line to be.
        frame("telemetry-event-transcript-segment.json", |data| {
            data["segment"]["seq"] = json!(3);
            data["segment"]["text"] = json!("x".repeat(2001));
        }),
        // A required field missing.
        frame("telemetry-event-transcript-segment.json", |data| {
            data["segment"]["seq"] = json!(3);
            data["segment"].as_object_mut().unwrap().remove("speaker");
        }),
        // A seq that is not a whole number, and data that is not an object.
        frame("telemetry-event-transcript-segment.json", |data| {
            data["segment"]["seq"] = json!("3");
        }),
        frame("telemetry-event-transcript-segment.json", |data| {
            *data = json!("hello");
        }),
        // A version this client does not speak.
        frame("telemetry-event-transcript-segment.json", |data| {
            data["segment"]["seq"] = json!(3);
            data["v"] = json!(2);
        }),
    ];
    for event in hostile {
        assert!(model.update(event).is_empty());
    }
    assert_eq!(ids(&model), ["item_a1", "item_b2"]);
    assert_eq!(*phase(&model), LiveTranscriptPhase::Live);

    // A line at the bound is a line, and its text never reaches `Debug`.
    model.update(frame("telemetry-event-transcript-segment.json", |data| {
        data["segment"]["segmentId"] = json!("item_long");
        data["segment"]["seq"] = json!(3);
        data["segment"]["text"] = json!("secret ".repeat(285));
    }));
    assert!(ids(&model).contains(&"item_long".to_owned()));
    let shown = format!("{:?}", signed_in(&model).active_call);
    assert!(
        !shown.contains("secret") && !shown.contains("Thursday"),
        "{shown}"
    );
}

#[test]
fn an_unknown_field_is_read_as_a_shipped_build_reads_it() {
    // A test build of district-model refuses unknown fields; whichever way this
    // build reads one, the frame is applied whole or not at all.
    let mut model = on_call();
    model.update(snapshot());
    model.update(frame("telemetry-event-transcript-segment.json", |data| {
        data["segment"]["segmentId"] = json!("item_new");
        data["segment"]["seq"] = json!(3);
        data["segment"]["sentiment"] = json!("calm");
        data["confidence"] = json!(0.9);
    }));
    let shown = ids(&model);
    assert!(
        shown.len() == 2 || shown.contains(&"item_new".to_owned()),
        "{shown:?}"
    );
}

// The end of the call.

#[test]
fn a_frame_after_the_hang_up_is_left_alone_but_a_retraction_still_applies() {
    let mut model = on_call();
    model.update(snapshot());
    let effects = model.update(Event::Call(CallEvent::HangUp));
    assert_eq!(wait(&effects).1, final_fetch_delay(1));
    assert_eq!(
        *phase(&model),
        LiveTranscriptPhase::Ended(TranscriptEndReason::CallEnded)
    );

    for late in [
        segment("item_c3", 2, 3, 0, true),
        snapshot_with(|data| data["segments"] = json!([])),
        ended(EPOCH, 3, "agent_error"),
        subscribe_refused("forbidden_role"),
    ] {
        assert!(model.update(late).is_empty());
    }
    assert_eq!(ids(&model), ["item_a1", "item_b2"]);
    assert!(matches!(phase(&model), LiveTranscriptPhase::Ended(_)));

    // An erased line comes off even now.
    let retraction = frame("telemetry-event-transcript-retracted.json", |data| {
        data["epoch"] = json!(EPOCH);
        data["seq"] = json!(3);
        data["all"] = json!(false);
        data["segmentIds"] = json!(["item_b2"]);
    });
    assert!(model.update(retraction).is_empty());
    assert_eq!(ids(&model), ["item_a1"]);
    // A call event in progress after the end asks for nothing.
    assert!(
        model
            .update(call_status(TelemetryEventType::CallUpdated, "in-progress"))
            .iter()
            .all(|effect| !matches!(effect, Effect::WatchTranscript { .. }))
    );
}

#[test]
fn the_full_transcript_is_read_with_backoff_until_it_is_written() {
    let mut model = on_call();
    model.update(snapshot());
    let mut read = reading_full(&mut model);
    // Empty until the service writes it, and a server error, are tried again,
    // each wait longer.
    for (attempt, result) in [
        (2, Ok(full(" \n"))),
        (3, Err(server_error())),
        (4, Ok(full(""))),
    ] {
        let effects = model.update(Event::TranscriptLoaded {
            ticket: read,
            result,
        });
        let (next, delay) = wait(&effects);
        assert_eq!(delay, final_fetch_delay(attempt));
        assert_eq!(
            *transcript(&model).final_transcript(),
            FinalTranscript::Fetching { attempt }
        );
        read = read_due(&mut model, next);
    }
    let effects = model.update(Event::TranscriptLoaded {
        ticket: read,
        result: Ok(full("Agent: Hello.\nCaller: Hi.")),
    });
    // Nothing more is wanted from the socket.
    assert_eq!(watched(&effects), Some(None));
    let FinalTranscript::Loaded(text) = transcript(&model).final_transcript() else {
        panic!("{:?}", transcript(&model).final_transcript());
    };
    assert!(text.starts_with("Agent: Hello."));
    let shown = format!("{:?}", transcript(&model).final_transcript());
    assert_eq!(shown, "Loaded(<25 bytes>)");
    // An answer nobody waits for any more changes nothing.
    assert!(
        model
            .update(Event::TranscriptLoaded {
                ticket: read,
                result: Ok(full("other")),
            })
            .is_empty()
    );
}

#[test]
fn the_full_transcript_gives_up_after_its_attempts_or_a_refusal() {
    // Empty every time: nothing was said.
    let mut model = on_call();
    let mut read = reading_full(&mut model);
    for attempt in 2..=FINAL_FETCH_ATTEMPTS {
        let effects = model.update(Event::TranscriptLoaded {
            ticket: read,
            result: Ok(full("")),
        });
        let (next, delay) = wait(&effects);
        assert_eq!(delay, final_fetch_delay(attempt));
        read = read_due(&mut model, next);
    }
    let effects = model.update(Event::TranscriptLoaded {
        ticket: read,
        result: Ok(full("")),
    });
    assert_eq!(watched(&effects), Some(None));
    assert_eq!(
        *transcript(&model).final_transcript(),
        FinalTranscript::Empty
    );
    assert_eq!(format!("{:?}", FinalTranscript::Empty), "Empty");

    // A refusal that trying again cannot fix.
    let mut model = on_call();
    let read = reading_full(&mut model);
    let effects = model.update(Event::TranscriptLoaded {
        ticket: read,
        result: Err(ApiError::NotFound(ErrorDetail::default())),
    });
    assert_eq!(watched(&effects), Some(None));
    let FinalTranscript::Failed(failure) = transcript(&model).final_transcript() else {
        panic!();
    };
    assert!(format!("{:?}", transcript(&model).final_transcript()).starts_with("Failed("));
    assert!(!failure.message.is_empty());
    assert_eq!(
        format!("{:?}", FinalTranscript::Fetching { attempt: 3 }),
        "Fetching { attempt: 3 }"
    );
    assert_eq!(
        format!("{:?}", FinalTranscript::NotRequested),
        "NotRequested"
    );
}

#[test]
fn a_full_transcript_read_refused_because_the_session_ended_signs_out() {
    let mut model = on_call();
    let read = reading_full(&mut model);
    model.update(Event::TranscriptLoaded {
        ticket: read,
        result: Err(signed_out_error()),
    });
    assert!(matches!(model.session(), SessionState::SignedOut(_)));
}

#[test]
fn the_waits_follow_the_contracts_numbers() {
    let seconds: Vec<u64> = (0..=10).map(|n| final_fetch_delay(n).as_secs()).collect();
    assert_eq!(seconds, [2, 2, 4, 8, 16, 30, 30, 30, 30, 30, 30]);
    assert_eq!(FINAL_FETCH_ATTEMPTS, 8);
    assert_eq!(GAP_HEAL, Duration::from_secs(2));
    assert_eq!(RATE_LIMIT_FALLBACK, Duration::from_secs(2));
    assert_eq!(NOT_LIVE_RETRY, Duration::from_secs(30));
}

// Ends that are not the call's.

#[test]
fn an_assistant_error_is_reconnecting_until_a_fresh_assistant_speaks() {
    let mut model = on_call();
    model.update(snapshot());
    assert!(model.update(ended(EPOCH, 3, "agent_error")).is_empty());
    assert_eq!(*phase(&model), LiveTranscriptPhase::Reconnecting);
    assert_eq!(
        transcript(&model).status(),
        Some(LiveTranscript::RECONNECTING)
    );
    // A line of the same epoch is no fresh assistant.
    model.update(segment("item_c3", 2, 4, 0, true));
    assert_eq!(*phase(&model), LiveTranscriptPhase::Reconnecting);
    model.update(segment_of(EPOCH + 1, "item_n1", 0, 1, 0, true));
    assert_eq!(*phase(&model), LiveTranscriptPhase::Live);

    // A snapshot that says the same thing: reconnecting, with nothing to read.
    let mut model = on_call();
    let effects = model.update(snapshot_with(|data| {
        data["live"] = json!(false);
        data["endedReason"] = json!("agent_error");
    }));
    assert!(effects.is_empty());
    assert_eq!(*phase(&model), LiveTranscriptPhase::Reconnecting);
}

#[test]
fn a_snapshot_that_says_only_not_live_ends_until_a_newer_epoch_revives_it() {
    let mut model = on_call();
    let effects = model.update(snapshot_with(|data| {
        data["live"] = json!(false);
        data.as_object_mut().unwrap().remove("endedReason");
    }));
    let (fetch, _) = wait(&effects);
    assert_eq!(
        *phase(&model),
        LiveTranscriptPhase::Ended(TranscriptEndReason::CallEnded)
    );
    model.update(segment_of(EPOCH + 1, "item_n1", 0, 1, 0, true));
    assert_eq!(*phase(&model), LiveTranscriptPhase::Live);
    assert_eq!(
        *transcript(&model).final_transcript(),
        FinalTranscript::NotRequested
    );
    // The read the end asked for is no longer wanted.
    assert!(model.update(Event::WaitOver { ticket: fetch }).is_empty());

    // Revived while the read was on its way: its answer is not wanted either.
    let mut model = on_call();
    let effects = model.update(snapshot_with(|data| {
        data["live"] = json!(false);
        data["endedReason"] = Value::Null;
    }));
    let read = read_due(&mut model, wait(&effects).0);
    model.update(segment_of(EPOCH + 1, "item_n1", 0, 1, 0, true));
    let effects = model.update(Event::TranscriptLoaded {
        ticket: read,
        result: Ok(full("Agent: Hello.")),
    });
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(
        *transcript(&model).final_transcript(),
        FinalTranscript::NotRequested
    );
}

#[test]
fn a_final_end_stays_final_whatever_comes_after() {
    let mut model = on_call();
    let effects = model.update(snapshot_with(|data| {
        data["live"] = json!(false);
        data["endedReason"] = json!("handed_off");
    }));
    assert_eq!(wait(&effects).1, final_fetch_delay(1));
    let ended_by = LiveTranscriptPhase::Ended(TranscriptEndReason::HandedOff);
    assert_eq!(*phase(&model), ended_by);
    // A live snapshot after it replaces the lines, not the end.
    model.update(snapshot_with(|data| data["lastSeq"] = json!(3)));
    assert_eq!(*phase(&model), ended_by);
    // Nor does an assistant error, or a fresh epoch.
    model.update(ended(EPOCH, 4, "agent_error"));
    model.update(segment_of(EPOCH + 1, "item_n1", 0, 1, 0, true));
    assert_eq!(*phase(&model), ended_by);
    // And an end for a reason this client does not know is final too.
    let mut model = on_call();
    model.update(snapshot());
    model.update(ended(EPOCH, 3, "carrier_dropped"));
    model.update(segment_of(EPOCH + 1, "item_n1", 0, 1, 0, true));
    assert_eq!(
        *phase(&model),
        LiveTranscriptPhase::Ended(TranscriptEndReason::Other("carrier_dropped".to_owned()))
    );
}

#[test]
fn after_an_all_retraction_the_next_frame_of_each_epoch_sets_its_baseline() {
    let mut model = on_call();
    model.update(snapshot());
    model.update(frame("telemetry-event-transcript-retracted.json", |_| {}));
    assert!(ids(&model).is_empty());
    // The service's memory of the call is empty: a snapshot with no mark.
    model.update(snapshot_with(|data| {
        data["epoch"] = Value::Null;
        data["lastSeq"] = Value::Null;
        data["live"] = json!(false);
        data["endedReason"] = Value::Null;
        data["segments"] = json!([]);
    }));
    assert!(matches!(phase(&model), LiveTranscriptPhase::Ended(_)));
    // Seq 7 of a fresh epoch is its first: no gap below it, and it revives the
    // transcript, which no end had a mark for.
    let effects = model.update(segment_of(EPOCH + 5, "item_p", 0, 7, 0, true));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(ids(&model), ["item_p"]);
    assert_eq!(*phase(&model), LiveTranscriptPhase::Live);
    // A gap after it is one.
    let effects = model.update(segment_of(EPOCH + 5, "item_q", 1, 9, 0, true));
    assert_eq!(wait(&effects).1, GAP_HEAL);
}

// Refusals.

#[test]
fn a_rate_limited_subscribe_is_sent_again_after_the_wait() {
    let mut model = on_call();
    let effects = model.update(error("transcript.subscribe", "rate_limited", Some(5000)));
    let (later, delay) = wait(&effects);
    assert_eq!(delay, Duration::from_secs(5));
    assert_eq!(watched(&effects), None);
    let effects = model.update(Event::WaitOver { ticket: later });
    assert_eq!(watched(&effects), Some(watching(1)));

    // With no wait named, the fallback.
    let effects = model.update(error("transcript.subscribe", "rate_limited", None));
    let (later, delay) = wait(&effects);
    assert_eq!(delay, RATE_LIMIT_FALLBACK);
    // Refused for good meanwhile: the wait asks for nothing.
    let effects = model.update(subscribe_refused("forbidden_role"));
    assert_eq!(watched(&effects), Some(None));
    assert!(model.update(Event::WaitOver { ticket: later }).is_empty());
    assert_eq!(
        *phase(&model),
        LiveTranscriptPhase::Unavailable(TranscriptErrorCode::ForbiddenRole)
    );
    assert_eq!(transcript(&model).status(), None);
}

#[test]
fn not_live_is_final_until_a_call_event_shows_the_call_in_progress() {
    let mut model = on_call();
    let effects = model.update(subscribe_refused("not_live"));
    assert_eq!(watched(&effects), Some(None));
    assert_eq!(
        *phase(&model),
        LiveTranscriptPhase::Unavailable(TranscriptErrorCode::NotLive)
    );

    // The call row shows the call in progress: asked for again, once per 30
    // seconds.
    let effects = model.update(call_status(TelemetryEventType::CallUpdated, "in-progress"));
    assert_eq!(watched(&effects), Some(watching(0)));
    let (cooled, delay) = wait(&effects);
    assert_eq!(delay, NOT_LIVE_RETRY);
    assert_eq!(*phase(&model), LiveTranscriptPhase::Subscribing);
    model.update(subscribe_refused("not_live"));
    let effects = model.update(call_status(TelemetryEventType::CallStarted, "in-progress"));
    assert_eq!(watched(&effects), None, "too soon: {effects:?}");
    assert!(model.update(Event::WaitOver { ticket: cooled }).is_empty());
    let effects = model.update(call_status(TelemetryEventType::CallUpdated, "in-progress"));
    assert_eq!(watched(&effects), Some(watching(0)));

    // While live, a call event asks for nothing.
    model.update(snapshot());
    let effects = model.update(call_status(TelemetryEventType::CallUpdated, "in-progress"));
    assert_eq!(watched(&effects), None);
}

#[test]
fn a_call_with_no_live_transcript_ends_without_reading_one() {
    let mut model = on_call();
    model.update(subscribe_refused("too_many_subscriptions"));
    let effects = model.update(Event::Call(CallEvent::HangUp));
    assert!(
        effects
            .iter()
            .all(|effect| !matches!(effect, Effect::Wait { .. })),
        "{effects:?}"
    );
    assert_eq!(
        *transcript(&model).final_transcript(),
        FinalTranscript::NotRequested
    );
}

// A placed call, putting it away, and signing out.

#[test]
fn a_placed_call_is_transcribed_and_its_end_on_the_socket_ends_the_transcript() {
    let (mut model, _) = loaded(AGENCY, "agency");
    model.update(Event::Navigate(Route::Dialer));
    model.update(Event::Dialer(DialerEvent::Edit(
        "+1 212 555 0142".to_owned(),
    )));
    let effects = model.update(Event::Dialer(DialerEvent::Dial));
    let dial = pick(&effects, |e| matches!(e, Effect::Dial { .. }));
    let answer: DialResponse = fixture("district-dial.json");
    let call_id = answer.call_id.clone();
    let effects = model.update(Event::Dialled {
        ticket: dial,
        result: Ok(answer),
    });
    let Some(Some(watch)) = watched(&effects) else {
        panic!("{effects:?}");
    };
    assert_eq!(
        (watch.workspace_id.as_str(), watch.call_id.as_str()),
        (AGENCY, call_id.as_str())
    );
    let (session, _, _) = connect(&effects);
    model.update(media(session, MediaEvent::Connected));
    model.update(media(
        session,
        MediaEvent::ParticipantJoined(person("sip_callee")),
    ));

    // The service says the call ended: the transcript ends with it, though the
    // room is still being left.
    let effects = model.update(call_event(
        AGENCY,
        &call_id,
        TelemetryEventType::CallEnded,
        json!({"id": call_id}),
    ));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Wait { delay, .. } if *delay == final_fetch_delay(1))),
        "{effects:?}"
    );
    let call = signed_in(&model).active_call.as_ref().unwrap();
    assert!(matches!(
        call.transcript().unwrap().phase(),
        LiveTranscriptPhase::Ended(_)
    ));
}

#[test]
fn putting_the_call_away_or_placing_another_stops_asking_for_its_transcript() {
    let mut model = on_call();
    model.update(Event::Call(CallEvent::HangUp));
    let effects = model.update(Event::Call(CallEvent::Dismiss));
    assert_eq!(watched(&effects), Some(None));
    assert_eq!(signed_in(&model).active_call, None);

    // A new call placed over the summary of one before it.
    let mut model = on_call();
    model.update(Event::Call(CallEvent::HangUp));
    model.update(Event::Navigate(Route::Dialer));
    model.update(Event::Dialer(DialerEvent::Edit(
        "+1 212 555 0142".to_owned(),
    )));
    let effects = model.update(Event::Dialer(DialerEvent::Dial));
    assert_eq!(watched(&effects), Some(None), "{effects:?}");
    assert!(matches!(effects.last(), Some(Effect::Dial { .. })));
}

#[test]
fn signing_out_stops_asking_for_the_transcript() {
    let mut model = on_call();
    let effects = model.update(Event::SignOut);
    assert_eq!(watched(&effects), Some(None), "{effects:?}");
    // Signing out with no call asks for nothing about a transcript.
    let (mut model, _) = loaded(AGENCY, "agency");
    assert_eq!(watched(&model.update(Event::SignOut)), None);
}

// What the apps show.

#[test]
fn every_speaker_has_a_label_and_a_stranger_is_never_guessed_at() {
    let mut model = on_call();
    model.update(snapshot());
    let speak = |id: &str, seq: i64, speaker: &str, name: Value| {
        frame("telemetry-event-transcript-segment.json", move |data| {
            data["segment"]["segmentId"] = json!(id);
            data["segment"]["seq"] = json!(seq);
            data["segment"]["speaker"] = json!(speaker);
            data["segment"]["speakerName"] = name;
        })
    };
    model.update(speak("item_s", 3, "supervisor", json!("Dana")));
    model.update(speak("item_t", 4, "agent", Value::Null));
    model.update(speak("item_u", 5, "agent", json!("")));
    assert_eq!(
        LiveTranscript::speaker_label(line(&model, "item_s")),
        LiveTranscript::OTHER_SPEAKER
    );
    for id in ["item_t", "item_u"] {
        assert_eq!(
            LiveTranscript::speaker_label(line(&model, id)),
            LiveTranscript::ASSISTANT
        );
    }
    // The words the apps share.
    assert_eq!(LiveTranscript::HEADING, "Live transcript");
    for words in [
        LiveTranscript::WAITING,
        LiveTranscript::LOADING_FULL,
        LiveTranscript::NO_TRANSCRIPT,
        LiveTranscript::INTERRUPTED,
    ] {
        assert!(!words.is_empty());
    }
}
