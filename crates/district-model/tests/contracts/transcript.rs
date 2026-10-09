//! The live transcript's frames read into their typed events: every field of
//! the service's recorded frames, the explicit nulls written back, what is not
//! read (another version, another call, malformed data, text beyond the
//! contract's bound), the open vocabularies, the text kept out of `Debug`, and
//! the ops a client sends.
use std::fmt;

use district_model::{
    TRANSCRIPT_CALL_ID_MAX, TRANSCRIPT_SUBSCRIBE_OP, TRANSCRIPT_TEXT_MAX_UTF16, TRANSCRIPT_VERSION,
    TelemetryEnvelope, TelemetryEventType, TranscriptClientOp, TranscriptEndReason,
    TranscriptErrorCode, TranscriptEvent, TranscriptRetractReason, TranscriptSpeaker,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const SEGMENT: &str =
    include_str!("../../../../contracts/fixtures/telemetry-event-transcript-segment.json");
const INTERIM: &str =
    include_str!("../../../../contracts/fixtures/telemetry-event-transcript-segment-interim.json");
const SNAPSHOT: &str =
    include_str!("../../../../contracts/fixtures/telemetry-event-transcript-snapshot.json");
const ENDED: &str =
    include_str!("../../../../contracts/fixtures/telemetry-event-transcript-ended.json");
const RETRACTED: &str =
    include_str!("../../../../contracts/fixtures/telemetry-event-transcript-retracted.json");
const ERROR: &str =
    include_str!("../../../../contracts/fixtures/telemetry-event-transcript-error.json");

fn envelope(raw: &str) -> TelemetryEnvelope {
    serde_json::from_str(raw).unwrap()
}

fn read(raw: &str) -> TranscriptEvent {
    envelope(raw)
        .transcript_event()
        .expect("a transcript event")
}

/// The fixture with `edit` applied to its data.
fn edited(raw: &str, edit: impl FnOnce(&mut Value)) -> TelemetryEnvelope {
    let mut envelope = envelope(raw);
    edit(&mut envelope.data);
    envelope
}

#[test]
fn a_segment_is_read_with_every_field() {
    let TranscriptEvent::Segment(data) = read(SEGMENT) else {
        panic!("a segment");
    };
    assert_eq!((data.version, data.call_id.as_str()), (1, "call_1"));
    let segment = data.segment;
    assert_eq!(segment.segment_id, "item_b2");
    assert_eq!((segment.index, segment.epoch), (1, 1_791_297_000_000));
    assert_eq!((segment.seq, segment.rev), (2, 0));
    assert_eq!(segment.speaker, TranscriptSpeaker::Caller);
    assert_eq!(segment.speaker_name, None);
    assert!(segment.text.contains("Thursday"));
    assert!(segment.is_final && !segment.interrupted);
    assert_eq!(segment.language.as_deref(), Some("en"));
    assert_eq!(segment.started_at, "2026-10-06T14:30:07.200Z");
    assert_eq!(
        segment.ended_at.as_deref(),
        Some("2026-10-06T14:30:10.700Z")
    );

    let TranscriptEvent::Segment(interim) = read(INTERIM) else {
        panic!("a segment");
    };
    assert!(!interim.segment.is_final);
    assert_eq!(interim.segment.ended_at, None);
}

#[test]
fn every_frame_is_read_and_its_data_encodes_back_with_its_nulls() {
    for raw in [SEGMENT, INTERIM, SNAPSHOT, ENDED, RETRACTED, ERROR] {
        let envelope = envelope(raw);
        let event = envelope.transcript_event().expect(raw);
        assert_eq!(event.version(), TRANSCRIPT_VERSION);
        let encoded = match &event {
            TranscriptEvent::Snapshot(data) => serde_json::to_value(data),
            TranscriptEvent::Segment(data) => serde_json::to_value(data),
            TranscriptEvent::Ended(data) => serde_json::to_value(data),
            TranscriptEvent::Retracted(data) => serde_json::to_value(data),
            TranscriptEvent::Error(data) => serde_json::to_value(data),
        }
        .unwrap();
        assert_eq!(encoded, envelope.data, "{raw}");
        assert_eq!(event.call_id(), Some("call_1"));
    }
}

#[test]
fn a_snapshot_an_end_a_retraction_and_an_error_are_read() {
    let TranscriptEvent::Snapshot(snapshot) = read(SNAPSHOT) else {
        panic!("a snapshot");
    };
    assert!(snapshot.live && snapshot.complete && !snapshot.more);
    assert_eq!(snapshot.ended_reason, None);
    assert_eq!(
        (snapshot.epoch, snapshot.last_seq),
        (Some(1_791_297_000_000), Some(2))
    );
    assert_eq!(snapshot.part, 0);
    let ids: Vec<&str> = snapshot
        .segments
        .iter()
        .map(|s| s.segment_id.as_str())
        .collect();
    assert_eq!(ids, ["item_a1", "item_b2"]);
    assert_eq!(snapshot.segments[0].speaker, TranscriptSpeaker::Agent);
    assert_eq!(snapshot.segments[0].speaker_name.as_deref(), Some("Ava"));

    let TranscriptEvent::Ended(ended) = read(ENDED) else {
        panic!("an end");
    };
    assert_eq!((ended.seq, ended.last_index), (3, Some(1)));
    assert_eq!(ended.reason, TranscriptEndReason::CallEnded);

    let TranscriptEvent::Retracted(retracted) = read(RETRACTED) else {
        panic!("a retraction");
    };
    assert!(retracted.all && retracted.segment_ids.is_empty());
    assert_eq!((retracted.epoch, retracted.seq), (None, None));
    assert_eq!(retracted.reason, TranscriptRetractReason::Erased);

    let TranscriptEvent::Error(error) = read(ERROR) else {
        panic!("an error");
    };
    assert_eq!(error.op.as_deref(), Some(TRANSCRIPT_SUBSCRIBE_OP));
    assert_eq!(error.code, TranscriptErrorCode::NotLive);
    assert_eq!(error.retry_after_ms, None);
}

#[test]
fn a_snapshot_from_a_service_without_its_end_reason_reads_none() {
    let envelope = edited(SNAPSHOT, |data| {
        data.as_object_mut().unwrap().remove("endedReason");
    });
    let Some(TranscriptEvent::Snapshot(snapshot)) = envelope.transcript_event() else {
        panic!("a snapshot");
    };
    assert_eq!(snapshot.ended_reason, None);
}

#[test]
fn the_call_ids_must_agree_unless_the_error_names_none() {
    for raw in [SEGMENT, SNAPSHOT, ENDED, RETRACTED, ERROR] {
        let mut envelope = envelope(raw);
        envelope.call_id = "call_2".to_owned();
        assert_eq!(envelope.transcript_event(), None, "{raw}");
    }
    let mut unnamed = edited(ERROR, |data| data["callId"] = Value::Null);
    unnamed.call_id = String::new();
    let Some(event) = unnamed.transcript_event() else {
        panic!("an error naming no call is read");
    };
    assert_eq!(event.call_id(), None);
}

#[test]
fn another_version_malformed_data_and_other_events_are_not_read() {
    for raw in [SEGMENT, SNAPSHOT, ENDED, RETRACTED, ERROR] {
        assert_eq!(
            edited(raw, |data| data["v"] = json!(2)).transcript_event(),
            None
        );
        assert_eq!(
            edited(raw, |data| *data = json!([])).transcript_event(),
            None
        );
    }
    // A missing field, a seq that is not a whole number, an epoch that is a
    // string: none is a frame this client can trust.
    let missing = edited(SEGMENT, |data| {
        data["segment"].as_object_mut().unwrap().remove("text");
    });
    assert_eq!(missing.transcript_event(), None);
    let fraction = edited(SEGMENT, |data| data["segment"]["seq"] = json!(2.5));
    assert_eq!(fraction.transcript_event(), None);
    let text_epoch = edited(ENDED, |data| data["epoch"] = json!("1791297000000"));
    assert_eq!(text_epoch.transcript_event(), None);

    let mut other = envelope(SEGMENT);
    other.event_type = TelemetryEventType::CallUpdated;
    assert_eq!(other.transcript_event(), None);
}

#[test]
fn test_builds_refuse_an_unknown_data_key() {
    let extra = edited(SEGMENT, |data| data["segment"]["extra"] = json!(1));
    assert_eq!(extra.transcript_event(), None);
}

#[test]
fn text_beyond_the_contracts_bound_is_not_read() {
    // Each of these is two UTF-16 units, so 1000 of them is the bound exactly.
    let at_bound = "\u{1F600}".repeat(TRANSCRIPT_TEXT_MAX_UTF16 / 2);
    let envelope = edited(SEGMENT, |data| data["segment"]["text"] = json!(at_bound));
    let Some(TranscriptEvent::Segment(data)) = envelope.transcript_event() else {
        panic!("text at the bound is read");
    };
    assert_eq!(data.segment.text_utf16_len(), TRANSCRIPT_TEXT_MAX_UTF16);

    let over = "a".repeat(TRANSCRIPT_TEXT_MAX_UTF16 + 1);
    let segment = edited(SEGMENT, |data| data["segment"]["text"] = json!(over));
    assert_eq!(segment.transcript_event(), None);
    let snapshot = edited(SNAPSHOT, |data| data["segments"][1]["text"] = json!(over));
    assert_eq!(snapshot.transcript_event(), None);
}

#[test]
fn every_vocabulary_keeps_a_name_it_does_not_know() {
    fn check<T>(known: &[(&str, T)], unknown: T)
    where
        T: Clone + fmt::Debug + PartialEq + Serialize + DeserializeOwned + Into<String>,
    {
        for (name, value) in known {
            let decoded: T = serde_json::from_value(json!(name)).unwrap();
            assert_eq!(&decoded, value);
            assert_eq!(serde_json::to_value(&decoded).unwrap(), json!(name));
            assert_eq!(decoded.into(), *name);
        }
        let decoded: T = serde_json::from_value(json!("later")).unwrap();
        assert_eq!(decoded, unknown);
        assert_eq!(serde_json::to_value(&decoded).unwrap(), json!("later"));
    }
    check(
        &[
            ("caller", TranscriptSpeaker::Caller),
            ("agent", TranscriptSpeaker::Agent),
        ],
        TranscriptSpeaker::Other("later".to_owned()),
    );
    check(
        &[
            ("call_ended", TranscriptEndReason::CallEnded),
            ("handed_off", TranscriptEndReason::HandedOff),
            ("agent_error", TranscriptEndReason::AgentError),
        ],
        TranscriptEndReason::Other("later".to_owned()),
    );
    check(
        &[
            ("erased", TranscriptRetractReason::Erased),
            ("policy", TranscriptRetractReason::Policy),
        ],
        TranscriptRetractReason::Other("later".to_owned()),
    );
    check(
        &[
            ("bad_request", TranscriptErrorCode::BadRequest),
            (
                "unsupported_version",
                TranscriptErrorCode::UnsupportedVersion,
            ),
            ("not_live", TranscriptErrorCode::NotLive),
            ("forbidden_role", TranscriptErrorCode::ForbiddenRole),
            (
                "too_many_subscriptions",
                TranscriptErrorCode::TooManySubscriptions,
            ),
            ("rate_limited", TranscriptErrorCode::RateLimited),
        ],
        TranscriptErrorCode::Other("later".to_owned()),
    );
    assert_eq!(TranscriptSpeaker::Other("x".to_owned()).as_str(), "x");
}

#[test]
fn the_text_is_left_out_of_every_debug_output() {
    for raw in [SEGMENT, SNAPSHOT] {
        let shown = format!("{:?}", read(raw));
        assert!(!shown.contains("Thursday"), "{shown}");
        assert!(shown.contains("<40 units>"), "{shown}");
    }
}

#[test]
fn the_ops_are_the_contracts_frames() {
    let subscribe = TranscriptClientOp::Subscribe("call_1".to_owned());
    assert_eq!(
        subscribe.text().as_deref(),
        Some(r#"{"op":"transcript.subscribe","v":1,"callId":"call_1"}"#)
    );
    let unsubscribe = TranscriptClientOp::Unsubscribe("call_1".to_owned());
    assert_eq!(
        unsubscribe.text().as_deref(),
        Some(r#"{"op":"transcript.unsubscribe","v":1,"callId":"call_1"}"#)
    );
    let mode = TranscriptClientOp::SocketMode { broadcast: false };
    assert_eq!(
        mode.text().as_deref(),
        Some(r#"{"op":"socket.mode","v":1,"broadcast":false}"#)
    );
    for op in [subscribe, unsubscribe, mode] {
        let parsed: Value = serde_json::from_str(&op.text().unwrap()).unwrap();
        assert_eq!(parsed["v"], TRANSCRIPT_VERSION);
    }
}

#[test]
fn the_longest_op_is_far_below_the_frame_limit() {
    let longest = "A".repeat(TRANSCRIPT_CALL_ID_MAX);
    let text = TranscriptClientOp::Unsubscribe(longest).text().unwrap();
    assert!(
        text.len() < TranscriptClientOp::MAX_FRAME_BYTES / 4,
        "{text}"
    );
}

#[test]
fn a_call_id_the_service_would_refuse_makes_no_frame() {
    let too_long = "a".repeat(TRANSCRIPT_CALL_ID_MAX + 1);
    for call_id in [
        "",
        "call 1",
        "call\"1",
        "caf\u{e9}",
        "a/b",
        too_long.as_str(),
    ] {
        assert!(!TranscriptClientOp::is_valid_call_id(call_id), "{call_id}");
        assert_eq!(
            TranscriptClientOp::Subscribe(call_id.to_owned()).text(),
            None
        );
    }
    assert!(TranscriptClientOp::is_valid_call_id("Az09_-"));
}
