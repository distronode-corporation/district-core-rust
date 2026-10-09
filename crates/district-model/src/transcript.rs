//! The live call transcript on the telemetry socket (`transcript` v1).
//!
//! A socket receives a call's transcript only after it asked for it with a
//! [`TranscriptClientOp::Subscribe`], and a subscription lasts as long as that
//! one socket: after a reconnect or a renewal it is sent again. The frames are
//! ordinary [`TelemetryEnvelope`]s of the five `transcript_*` event types, and
//! [`TelemetryEnvelope::transcript_event`] reads their `data` into the types
//! here.
//!
//! A segment's text is what a caller said, which is customer data: the `Debug`
//! output of every type that can hold one prints its length instead.
//!
//! Unknown `data` keys are ignored in a shipped build, as everywhere in this
//! crate: the service adds optional fields without changing `v`. A breaking
//! change is `v: 2`, which the service sends only to a socket that asked for it,
//! so a frame whose `v` is not [`TRANSCRIPT_VERSION`] is not read.

use std::fmt;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::telemetry::{TelemetryEnvelope, TelemetryEventType};

/// The `transcript` payload version this client speaks.
pub const TRANSCRIPT_VERSION: i64 = 1;

/// The longest text a segment may carry, in UTF-16 code units, as the contract
/// bounds it: the assistant splits a longer turn into several segments.
pub const TRANSCRIPT_TEXT_MAX_UTF16: usize = 2000;

/// The longest call id the service accepts in an op.
pub const TRANSCRIPT_CALL_ID_MAX: usize = 64;

/// One utterance of a live transcript, as `transcript_segment` and
/// `transcript_snapshot` carry it.
///
/// A segment is revised in place: [`segment_id`](Self::segment_id) stays the
/// same from the first interim hypothesis to the final, [`rev`](Self::rev)
/// grows with each revision, and a final segment is frozen. The order on screen
/// is ([`epoch`](Self::epoch), [`index`](Self::index)); [`seq`](Self::seq) is
/// for spotting duplicates and gaps only.
///
/// Its `Debug` output prints the text's length, never the text.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    /// The utterance's id, unique within the call.
    pub segment_id: String,
    /// The display order within [`epoch`](Self::epoch), never reused.
    pub index: i64,
    /// The assistant session that produced the segment, as its start time in
    /// Unix epoch milliseconds. An assistant sent to the same call again starts
    /// a new epoch.
    pub epoch: i64,
    /// The message counter of (call, epoch) at this update, 1 or more.
    pub seq: i64,
    /// The revision of this segment, 0 or more.
    pub rev: i64,
    /// Who spoke.
    pub speaker: TranscriptSpeaker,
    /// The persona's name on an `agent` line; always `None` on a `caller` line.
    pub speaker_name: Option<String>,
    /// What was said. Never log it.
    pub text: String,
    /// `false`: an interim hypothesis that will be replaced. `true`: frozen.
    #[serde(rename = "final")]
    pub is_final: bool,
    /// On an `agent` final: the speech was cut off, and the text is what was
    /// actually played.
    pub interrupted: bool,
    /// A BCP 47 primary subtag (`en`, `fr`) when known.
    pub language: Option<String>,
    /// When the segment started, an ISO 8601 instant.
    pub started_at: String,
    /// When it ended; `None` while interim.
    pub ended_at: Option<String>,
}

impl TranscriptSegment {
    /// The text's length in UTF-16 code units, the unit the contract bounds it in.
    pub fn text_utf16_len(&self) -> usize {
        self.text.encode_utf16().count()
    }
}

impl fmt::Debug for TranscriptSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TranscriptSegment")
            .field("segment_id", &self.segment_id)
            .field("epoch", &self.epoch)
            .field("index", &self.index)
            .field("seq", &self.seq)
            .field("rev", &self.rev)
            .field("speaker", &self.speaker)
            .field("text", &format_args!("<{} units>", self.text_utf16_len()))
            .field("is_final", &self.is_final)
            .finish()
    }
}

/// `transcript_segment`'s data: one new or revised segment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegmentData {
    /// The payload version (`v` on the wire).
    #[serde(rename = "v")]
    pub version: i64,
    /// The call.
    pub call_id: String,
    /// The segment.
    pub segment: TranscriptSegment,
}

/// `transcript_snapshot`'s data: the transcript so far, in reply to a
/// subscribe, in one or more parts.
///
/// A client replaces its state for the call with the union of the parts once
/// the part with [`more`](Self::more) false has arrived, and not before. The
/// parts arrive back to back, and every field but
/// [`segments`](Self::segments) and [`part`](Self::part) repeats, identical, on
/// each one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSnapshotData {
    /// The payload version (`v` on the wire).
    #[serde(rename = "v")]
    pub version: i64,
    /// The call.
    pub call_id: String,
    /// False once the transcript has ended. It does not say why;
    /// [`ended_reason`](Self::ended_reason) does.
    pub live: bool,
    /// Why the transcript ended; `None` while live, and from a service older
    /// than the field. `agent_error` with `live` false is the wait for a fresh
    /// assistant, not an end.
    #[serde(default)]
    pub ended_reason: Option<TranscriptEndReason>,
    /// False when the service's memory does not reach back to the call's first
    /// line. The earlier lines are in the transcript after the call.
    pub complete: bool,
    /// The epoch [`last_seq`](Self::last_seq) belongs to: the newest the service
    /// holds. `None` (with `last_seq`) only after an `all` retraction emptied
    /// its memory of the call.
    pub epoch: Option<i64>,
    /// The highest seq of [`epoch`](Self::epoch) this snapshot includes.
    pub last_seq: Option<i64>,
    /// The latest revision of each segment, in (epoch, index) order.
    pub segments: Vec<TranscriptSegment>,
    /// This part's number, from 0.
    pub part: i64,
    /// True until the last part.
    pub more: bool,
}

/// `transcript_ended`'s data: the assistant stopped transcribing the call, and
/// no segment follows in this epoch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEndedData {
    /// The payload version (`v` on the wire).
    #[serde(rename = "v")]
    pub version: i64,
    /// The call.
    pub call_id: String,
    /// The epoch that ended.
    pub epoch: i64,
    /// The message counter of (call, epoch).
    pub seq: i64,
    /// The epoch's last segment index; `None` when nothing was said.
    pub last_index: Option<i64>,
    /// Why.
    pub reason: TranscriptEndReason,
}

/// `transcript_retracted`'s data: lines that must come off every screen.
///
/// [`epoch`](Self::epoch) and [`seq`](Self::seq) are set together or not at
/// all: the assistant's retraction carries both, the website's (a contact
/// erased) neither.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct TranscriptRetractedData {
    /// The payload version (`v` on the wire).
    #[serde(rename = "v")]
    pub version: i64,
    /// The call.
    pub call_id: String,
    /// The assistant session whose counter [`seq`](Self::seq) belongs to.
    pub epoch: Option<i64>,
    /// The message counter of (call, epoch).
    pub seq: Option<i64>,
    /// True: drop every line of the call.
    pub all: bool,
    /// The segments to drop when [`all`](Self::all) is false.
    pub segment_ids: Vec<String>,
    /// Why.
    pub reason: TranscriptRetractReason,
}

/// `transcript_error`'s data: an op could not be honoured. The socket stays
/// open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct TranscriptErrorData {
    /// The payload version (`v` on the wire).
    #[serde(rename = "v")]
    pub version: i64,
    /// The call the op named; `None` for an op that named none. The envelope's
    /// call id is then empty, which names no call.
    pub call_id: Option<String>,
    /// The op the client sent, echoed exactly; `None` when the frame had none.
    pub op: Option<String>,
    /// What went wrong.
    pub code: TranscriptErrorCode,
    /// How long to wait before trying again, with `rate_limited`.
    pub retry_after_ms: Option<i64>,
}

/// A `transcript_*` event, read. See [`TelemetryEnvelope::transcript_event`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TranscriptEvent {
    /// `transcript_snapshot`.
    Snapshot(TranscriptSnapshotData),
    /// `transcript_segment`.
    Segment(TranscriptSegmentData),
    /// `transcript_ended`.
    Ended(TranscriptEndedData),
    /// `transcript_retracted`.
    Retracted(TranscriptRetractedData),
    /// `transcript_error`.
    Error(TranscriptErrorData),
}

impl TranscriptEvent {
    /// The payload version the event was sent as.
    pub fn version(&self) -> i64 {
        match self {
            Self::Snapshot(data) => data.version,
            Self::Segment(data) => data.version,
            Self::Ended(data) => data.version,
            Self::Retracted(data) => data.version,
            Self::Error(data) => data.version,
        }
    }

    /// The call the event is about, from its data; `None` for an error that
    /// names none.
    pub fn call_id(&self) -> Option<&str> {
        match self {
            Self::Snapshot(data) => Some(&data.call_id),
            Self::Segment(data) => Some(&data.call_id),
            Self::Ended(data) => Some(&data.call_id),
            Self::Retracted(data) => Some(&data.call_id),
            Self::Error(data) => data.call_id.as_deref(),
        }
    }

    /// Whether every segment the event carries is within the contract's bound
    /// on text, [`TRANSCRIPT_TEXT_MAX_UTF16`].
    fn within_bounds(&self) -> bool {
        let segments: &[TranscriptSegment] = match self {
            Self::Snapshot(data) => &data.segments,
            Self::Segment(data) => std::slice::from_ref(&data.segment),
            Self::Ended(_) | Self::Retracted(_) | Self::Error(_) => &[],
        };
        segments
            .iter()
            .all(|segment| segment.text_utf16_len() <= TRANSCRIPT_TEXT_MAX_UTF16)
    }
}

impl TelemetryEnvelope {
    /// This event's data as a transcript event, or `None` when it is not one
    /// this client can read.
    ///
    /// `None` for an event type other than the five `transcript_*` ones; data
    /// that does not decode; a `v` other than [`TRANSCRIPT_VERSION`]; data whose
    /// call id disagrees with the envelope's; and a segment whose text is longer
    /// than [`TRANSCRIPT_TEXT_MAX_UTF16`], which the service never sends. An
    /// error that names no call is read whatever the envelope's call id says,
    /// since there is nothing to compare.
    pub fn transcript_event(&self) -> Option<TranscriptEvent> {
        let event = match self.event_type {
            TelemetryEventType::TranscriptSnapshot => self.data_as().map(TranscriptEvent::Snapshot),
            TelemetryEventType::TranscriptSegment => self.data_as().map(TranscriptEvent::Segment),
            TelemetryEventType::TranscriptEnded => self.data_as().map(TranscriptEvent::Ended),
            TelemetryEventType::TranscriptRetracted => {
                self.data_as().map(TranscriptEvent::Retracted)
            }
            TelemetryEventType::TranscriptError => self.data_as().map(TranscriptEvent::Error),
            _ => None,
        }?;
        let agrees = event.call_id().is_none_or(|named| named == self.call_id);
        (event.version() == TRANSCRIPT_VERSION && agrees && event.within_bounds()).then_some(event)
    }

    fn data_as<T: DeserializeOwned>(&self) -> Option<T> {
        T::deserialize(&self.data).ok()
    }
}

/// Defines an open vocabulary: a string enum whose unknown names decode to
/// `Other` with the name kept, and encode back as sent.
macro_rules! open_vocabulary {
    (
        $(#[$meta:meta])*
        $name:ident { $($(#[$variant_meta:meta])* $variant:ident = $wire:literal,)+ }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
        #[serde(from = "String", into = "String")]
        pub enum $name {
            $($(#[$variant_meta])* $variant,)+
            /// A name this client does not know, kept as it was sent.
            Other(String),
        }

        impl $name {
            /// The name on the wire.
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $wire,)+
                    Self::Other(name) => name,
                }
            }
        }

        impl From<String> for $name {
            fn from(name: String) -> Self {
                match name.as_str() {
                    $($wire => Self::$variant,)+
                    _ => Self::Other(name),
                }
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                match value {
                    $name::Other(name) => name,
                    known => known.as_str().to_owned(),
                }
            }
        }
    };
}

open_vocabulary! {
    /// Who spoke a line. Names reserved for later (`human_agent`, `supervisor`)
    /// and any other this client does not know are [`Other`](Self::Other): a
    /// third party.
    TranscriptSpeaker {
        /// `caller`: the person who called, or was called.
        Caller = "caller",
        /// `agent`: the AI receptionist. The segment's speaker name names the
        /// persona.
        Agent = "agent",
    }
}

open_vocabulary! {
    /// Why a live transcript ended. A reason this client does not know is
    /// final.
    TranscriptEndReason {
        /// `call_ended`. Final.
        CallEnded = "call_ended",
        /// `handed_off`: the call was transferred and the assistant left it.
        /// Final.
        HandedOff = "handed_off",
        /// `agent_error`: the assistant failed. The only reason a new epoch may
        /// follow: the call can be handed to a fresh assistant.
        AgentError = "agent_error",
    }
}

open_vocabulary! {
    /// Why lines were retracted.
    TranscriptRetractReason {
        /// `erased`: the contact was erased.
        Erased = "erased",
        /// `policy`.
        Policy = "policy",
    }
}

open_vocabulary! {
    /// What a `transcript_error` reports.
    TranscriptErrorCode {
        /// `bad_request`: the client sent something malformed. Not retried.
        BadRequest = "bad_request",
        /// `unsupported_version`: only the transcript after the call.
        UnsupportedVersion = "unsupported_version",
        /// `not_live`: no live transcript for this call within 30 seconds of
        /// the subscribe (unknown, another workspace's, long over, or not
        /// answered by the assistant). Final for that subscribe.
        NotLive = "not_live",
        /// `forbidden_role`: this member's role may not watch.
        ForbiddenRole = "forbidden_role",
        /// `too_many_subscriptions`: more than five on one socket.
        TooManySubscriptions = "too_many_subscriptions",
        /// `rate_limited`: wait `retryAfterMs`.
        RateLimited = "rate_limited",
    }
}

/// The ops a client sends on the telemetry socket, as text frames.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TranscriptClientOp {
    /// Start receiving one call's transcript. The service answers with a
    /// snapshot.
    Subscribe(String),
    /// Stop receiving it.
    Unsubscribe(String),
    /// `broadcast: false` stops the workspace-wide `call_*` and `message_*`
    /// relay to this socket. The service's default is true, which a desktop
    /// keeps: it rings from that relay.
    SocketMode {
        /// Whether the socket takes the relay.
        broadcast: bool,
    },
}

impl TranscriptClientOp {
    /// The largest frame the service accepts, in bytes. No op here comes near
    /// it: a call id is at most [`TRANSCRIPT_CALL_ID_MAX`] characters of
    /// `[A-Za-z0-9_-]`.
    pub const MAX_FRAME_BYTES: usize = 1024;

    /// Whether `call_id` is one the service accepts in an op: 1 to 64 of
    /// `A-Z a-z 0-9 _ -`.
    pub fn is_valid_call_id(call_id: &str) -> bool {
        (1..=TRANSCRIPT_CALL_ID_MAX).contains(&call_id.len())
            && call_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    }

    /// The frame to send, or `None` for a call id the service would refuse.
    ///
    /// Written out rather than encoded, in the contract's key order, which is
    /// safe because a valid call id holds nothing JSON escapes.
    pub fn text(&self) -> Option<String> {
        let (op, call_id) = match self {
            Self::Subscribe(call_id) => ("transcript.subscribe", call_id),
            Self::Unsubscribe(call_id) => ("transcript.unsubscribe", call_id),
            Self::SocketMode { broadcast } => {
                return Some(format!(
                    r#"{{"op":"socket.mode","v":{TRANSCRIPT_VERSION},"broadcast":{broadcast}}}"#
                ));
            }
        };
        Self::is_valid_call_id(call_id)
            .then(|| format!(r#"{{"op":"{op}","v":{TRANSCRIPT_VERSION},"callId":"{call_id}"}}"#))
    }
}

/// The op a `transcript_error` about a subscribe echoes.
pub const TRANSCRIPT_SUBSCRIBE_OP: &str = "transcript.subscribe";
