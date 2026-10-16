//! The live telemetry socket: its credential and the envelopes it delivers.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `POST /api/district/telemetry/token`: a credential for one workspace's live
/// telemetry socket, and where that socket is.
///
/// The token is a bearer credential for that one workspace and lives fifteen
/// minutes. It is presented in the socket's `Sec-WebSocket-Protocol` header,
/// never in the URL. Its `Debug` output is redacted, so it cannot reach a log
/// through a `{:?}` of this type.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct TelemetryToken {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The credential. Never log it.
    pub token: String,
    /// When the credential expires, in Unix epoch milliseconds by the server's
    /// clock. The socket is closed by the server once this passes, so a client
    /// replaces it shortly before.
    pub expires_at: i64,
    /// The socket's address, for example `wss://telemetry.example.com/ws/telemetry`.
    /// It is chosen by the workspace's region, not by which server answered, so
    /// always dial this one. `None` when the service has none configured, which
    /// only a development setup does.
    pub ws_url: Option<String>,
}

impl fmt::Debug for TelemetryToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryToken")
            .field("success", &self.success)
            .field("token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("ws_url", &self.ws_url)
            .finish()
    }
}

/// One message from the live telemetry socket.
///
/// The service publishes one of these whenever a call or a message thread in the
/// workspace changes, and the socket relays it as it is. An event is a hint that
/// something changed, delivered at most once and only while the socket is open:
/// a client that was disconnected has missed whatever happened meanwhile and
/// must read the current state again.
///
/// Its `Debug` output leaves out [`data`](Self::data), which is customer data.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct TelemetryEnvelope {
    /// The workspace the event belongs to.
    pub workspace_id: String,
    /// The call the event is about. For the two message events, the message's
    /// id instead.
    pub call_id: String,
    /// What happened.
    pub event_type: TelemetryEventType,
    /// The event's content, which depends on [`event_type`](Self::event_type):
    ///
    /// - The three call events carry the call as the service stores it, which
    ///   is customer data (the caller's number and name, a summary, a
    ///   transcript). `call_ended` can carry only the fields that changed.
    /// - `call_ringing` carries `{callId, userIds}`, ids only: the members the
    ///   call is ringing for on a desktop. Every socket in the workspace gets the
    ///   event, so a desktop rings only when its own user id is in `userIds`.
    /// - `call_ring_cancelled` carries `{callId, reason}`, ids and a word only: a
    ///   ring the service started has ended, read with
    ///   [`ring_cancel_reason`](Self::ring_cancel_reason).
    /// - `tool_outcome` carries `{tool, result, provider?, reason?}` and nothing
    ///   about the caller.
    /// - The five `transcript_*` events carry one call's live transcript, read
    ///   with [`transcript_event`](Self::transcript_event).
    /// - The message events carry `{messageId, counterpart, type}`, where
    ///   `counterpart` is the other party's number or address. Fetch the thread
    ///   for its content.
    ///
    /// Kept as plain JSON, so a reshaped call row cannot make the event
    /// unreadable.
    pub data: Value,
    /// When the service published the event, as an ISO 8601 instant.
    pub timestamp: String,
}

impl fmt::Debug for TelemetryEnvelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryEnvelope")
            .field("workspace_id", &self.workspace_id)
            .field("call_id", &self.call_id)
            .field("event_type", &self.event_type)
            .field("data", &"<omitted>")
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

/// What a [`TelemetryEnvelope`] reports.
///
/// The service adds event types over time, so a name this client does not know
/// decodes to [`Unknown`](Self::Unknown) with the name kept, rather than failing
/// the envelope. Treat an unknown event as a hint to read the workspace's state
/// again.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(from = "String", into = "String")]
pub enum TelemetryEventType {
    /// `call_started`: a call began. The data is the new call.
    CallStarted,
    /// `call_updated`: a call in progress changed. The data is the call.
    CallUpdated,
    /// `call_ended`: a call finished.
    CallEnded,
    /// `call_ringing`: a call is ringing for members of the workspace on their
    /// desktops. The data names them by user id; ring only when this
    /// installation's user is among them. The service sends it only while the
    /// desktop keeps its presence registered.
    CallRinging,
    /// `call_ring_cancelled`: a ring the service started has ended, because
    /// someone answered (on any device, possibly this one), the window closed,
    /// the caller hung up, or the service stopped it. The data says which; read
    /// it with [`ring_cancel_reason`](TelemetryEnvelope::ring_cancel_reason).
    /// Sent only when a desktop was rung, to every socket in the workspace, and
    /// possibly more than once.
    CallRingCancelled,
    /// `tool_outcome`: an action the assistant took during a call (a transfer,
    /// for example) succeeded or failed.
    ToolOutcome,
    /// `message_received`: a message arrived in one of the workspace's threads.
    MessageReceived,
    /// `message_sent`: a message was sent from the workspace.
    MessageSent,
    /// `transcript_snapshot`: a call's live transcript so far, in reply to a
    /// subscribe. Read the data with
    /// [`transcript_event`](TelemetryEnvelope::transcript_event), as for the
    /// four below.
    TranscriptSnapshot,
    /// `transcript_segment`: a new or revised line of a live transcript.
    TranscriptSegment,
    /// `transcript_ended`: the assistant stopped transcribing a call.
    TranscriptEnded,
    /// `transcript_retracted`: lines that must come off every screen.
    TranscriptRetracted,
    /// `transcript_error`: a transcript op could not be honoured.
    TranscriptError,
    /// A name this client does not know, kept as it was sent.
    Unknown(String),
}

impl TelemetryEventType {
    /// The name on the wire.
    pub fn as_str(&self) -> &str {
        match self {
            Self::CallStarted => "call_started",
            Self::CallUpdated => "call_updated",
            Self::CallEnded => "call_ended",
            Self::CallRinging => "call_ringing",
            Self::CallRingCancelled => "call_ring_cancelled",
            Self::ToolOutcome => "tool_outcome",
            Self::MessageReceived => "message_received",
            Self::MessageSent => "message_sent",
            Self::TranscriptSnapshot => "transcript_snapshot",
            Self::TranscriptSegment => "transcript_segment",
            Self::TranscriptEnded => "transcript_ended",
            Self::TranscriptRetracted => "transcript_retracted",
            Self::TranscriptError => "transcript_error",
            Self::Unknown(name) => name,
        }
    }

    /// Whether this is one of the five live transcript events, which reach a
    /// socket only for a call it subscribed to.
    pub fn is_transcript(&self) -> bool {
        matches!(
            self,
            Self::TranscriptSnapshot
                | Self::TranscriptSegment
                | Self::TranscriptEnded
                | Self::TranscriptRetracted
                | Self::TranscriptError
        )
    }
}

impl From<String> for TelemetryEventType {
    fn from(name: String) -> Self {
        match name.as_str() {
            "call_started" => Self::CallStarted,
            "call_updated" => Self::CallUpdated,
            "call_ended" => Self::CallEnded,
            "call_ringing" => Self::CallRinging,
            "call_ring_cancelled" => Self::CallRingCancelled,
            "tool_outcome" => Self::ToolOutcome,
            "message_received" => Self::MessageReceived,
            "message_sent" => Self::MessageSent,
            "transcript_snapshot" => Self::TranscriptSnapshot,
            "transcript_segment" => Self::TranscriptSegment,
            "transcript_ended" => Self::TranscriptEnded,
            "transcript_retracted" => Self::TranscriptRetracted,
            "transcript_error" => Self::TranscriptError,
            _ => Self::Unknown(name),
        }
    }
}

/// Why a ring ended, as a `call_ring_cancelled` event says.
///
/// The service may add reasons; one this client does not know reads as
/// [`Withdrawn`](Self::Withdrawn), never as a failure to read the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RingCancelReason {
    /// `answered`: somebody took the call, on some device. That can be this
    /// desktop: a device that answered the call, or is answering it, ignores
    /// the event.
    Answered,
    /// `timeout`: the ring's window closed with nobody answering, and the
    /// receptionist kept the call.
    Timeout,
    /// `call_ended`: the caller hung up while it rang.
    CallEnded,
    /// `withdrawn`: the service stopped ringing for a reason of its own, and
    /// any reason this client does not know.
    Withdrawn,
}

impl RingCancelReason {
    /// The reason named `name` on the wire.
    pub fn from_wire(name: &str) -> Self {
        match name {
            "answered" => Self::Answered,
            "timeout" => Self::Timeout,
            "call_ended" => Self::CallEnded,
            _ => Self::Withdrawn,
        }
    }

    /// The name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Timeout => "timeout",
            Self::CallEnded => "call_ended",
            Self::Withdrawn => "withdrawn",
        }
    }
}

impl TelemetryEnvelope {
    /// Why the ring ended, for a `call_ring_cancelled` event, or `None` for any
    /// other event. A reason missing or not a string reads as
    /// [`RingCancelReason::Withdrawn`]. The call is
    /// [`call_id`](Self::call_id).
    pub fn ring_cancel_reason(&self) -> Option<RingCancelReason> {
        (self.event_type == TelemetryEventType::CallRingCancelled).then(|| {
            RingCancelReason::from_wire(
                self.data
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
        })
    }
}

impl From<TelemetryEventType> for String {
    fn from(event_type: TelemetryEventType) -> Self {
        match event_type {
            TelemetryEventType::Unknown(name) => name,
            known => known.as_str().to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: [(&str, TelemetryEventType); 13] = [
        ("call_started", TelemetryEventType::CallStarted),
        ("call_updated", TelemetryEventType::CallUpdated),
        ("call_ended", TelemetryEventType::CallEnded),
        ("call_ringing", TelemetryEventType::CallRinging),
        ("call_ring_cancelled", TelemetryEventType::CallRingCancelled),
        ("tool_outcome", TelemetryEventType::ToolOutcome),
        ("message_received", TelemetryEventType::MessageReceived),
        ("message_sent", TelemetryEventType::MessageSent),
        (
            "transcript_snapshot",
            TelemetryEventType::TranscriptSnapshot,
        ),
        ("transcript_segment", TelemetryEventType::TranscriptSegment),
        ("transcript_ended", TelemetryEventType::TranscriptEnded),
        (
            "transcript_retracted",
            TelemetryEventType::TranscriptRetracted,
        ),
        ("transcript_error", TelemetryEventType::TranscriptError),
    ];

    #[test]
    fn every_known_event_type_decodes_and_encodes_back() {
        for (name, expected) in KNOWN {
            let decoded: TelemetryEventType = serde_json::from_value(name.into()).unwrap();
            assert_eq!(decoded, expected);
            assert_eq!(decoded.as_str(), name);
            assert_eq!(serde_json::to_value(&decoded).unwrap(), name);
            assert_eq!(decoded.is_transcript(), name.starts_with("transcript_"));
        }
    }

    #[test]
    fn an_unknown_event_type_keeps_its_name() {
        let decoded: TelemetryEventType = serde_json::from_str(r#""call_parked""#).unwrap();
        assert_eq!(
            decoded,
            TelemetryEventType::Unknown("call_parked".to_owned())
        );
        assert_eq!(decoded.as_str(), "call_parked");
        assert_eq!(serde_json::to_string(&decoded).unwrap(), r#""call_parked""#);
    }

    #[test]
    fn an_envelope_round_trips_with_its_data_untouched() {
        let raw = r#"{"workspaceId":"ws_1","callId":"call_1","eventType":"call_started",
            "data":{"id":"call_1","from":"+1 212 555 0142","nested":{"a":[1,2]}},
            "timestamp":"2026-09-26T12:00:00.000Z"}"#;
        let envelope: TelemetryEnvelope = serde_json::from_str(raw).unwrap();
        assert_eq!(envelope.workspace_id, "ws_1");
        assert_eq!(envelope.call_id, "call_1");
        assert_eq!(envelope.event_type, TelemetryEventType::CallStarted);
        assert_eq!(envelope.data["nested"]["a"][1], 2);
        assert_eq!(
            serde_json::to_value(&envelope).unwrap(),
            serde_json::from_str::<Value>(raw).unwrap()
        );
    }

    #[test]
    fn a_ring_cancel_reads_its_reason_and_an_unknown_one_as_withdrawn() {
        let cancel = |data: Value| TelemetryEnvelope {
            workspace_id: "ws_1".to_owned(),
            call_id: "call_1".to_owned(),
            event_type: TelemetryEventType::CallRingCancelled,
            data,
            timestamp: "t".to_owned(),
        };
        for reason in [
            RingCancelReason::Answered,
            RingCancelReason::Timeout,
            RingCancelReason::CallEnded,
            RingCancelReason::Withdrawn,
        ] {
            let envelope =
                cancel(serde_json::json!({"callId": "call_1", "reason": reason.as_str()}));
            assert_eq!(envelope.ring_cancel_reason(), Some(reason));
            assert_eq!(RingCancelReason::from_wire(reason.as_str()), reason);
        }
        for data in [
            serde_json::json!({"callId": "call_1", "reason": "parked"}),
            serde_json::json!({"callId": "call_1", "reason": 3}),
            serde_json::json!({"callId": "call_1"}),
            serde_json::json!(null),
        ] {
            assert_eq!(
                cancel(data).ring_cancel_reason(),
                Some(RingCancelReason::Withdrawn)
            );
        }
        let ringing = TelemetryEnvelope {
            event_type: TelemetryEventType::CallRinging,
            ..cancel(serde_json::json!({"reason": "answered"}))
        };
        assert_eq!(ringing.ring_cancel_reason(), None);
    }

    #[test]
    fn debug_output_leaves_out_the_data_and_the_token() {
        let envelope = TelemetryEnvelope {
            workspace_id: "ws_1".to_owned(),
            call_id: "call_1".to_owned(),
            event_type: TelemetryEventType::Unknown("later".to_owned()),
            data: serde_json::json!({"from": "+1 212 555 0142"}),
            timestamp: "t".to_owned(),
        };
        let shown = format!("{envelope:?}");
        assert!(shown.contains("ws_1") && shown.contains("later"), "{shown}");
        assert!(!shown.contains("555"), "{shown}");

        let token: TelemetryToken = serde_json::from_str(
            r#"{"success":true,"token":"secret.jwt.value","expiresAt":1,"wsUrl":null}"#,
        )
        .unwrap();
        let shown = format!("{token:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("expires_at: 1"), "{shown}");
    }

    #[test]
    fn a_token_answer_decodes_and_round_trips() {
        let raw = r#"{"success":true,"token":"a.b.c","expiresAt":1790000000000,
            "wsUrl":"wss://telemetry.example.com/ws/telemetry"}"#;
        let token: TelemetryToken = serde_json::from_str(raw).unwrap();
        assert!(token.success);
        assert_eq!(token.token, "a.b.c");
        assert_eq!(token.expires_at, 1_790_000_000_000);
        assert_eq!(
            token.ws_url.as_deref(),
            Some("wss://telemetry.example.com/ws/telemetry")
        );
        assert_eq!(
            serde_json::to_value(&token).unwrap(),
            serde_json::from_str::<Value>(raw).unwrap()
        );

        let local: TelemetryToken =
            serde_json::from_str(r#"{"token":"t","expiresAt":0,"wsUrl":null}"#).unwrap();
        assert!(!local.success);
        assert_eq!(local.ws_url, None);
    }

    #[test]
    fn test_builds_refuse_unknown_fields() {
        let error = serde_json::from_str::<TelemetryToken>(
            r#"{"token":"t","expiresAt":0,"wsUrl":null,"extra":1}"#,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("unknown field `extra`"),
            "{error}"
        );
    }
}
