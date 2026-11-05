//! Supervising a live call: listening in, sending the receptionist a private
//! note (a whisper), and taking the call over by voice.
//!
//! Three routes, each with its own answer here:
//!
//! - `POST /api/district/calls/token` with a call's id: a listen-only
//!   credential for the call's room ([`CallMonitorTokenResponse`]). It
//!   publishes nothing: no audio and no data.
//! - `POST /api/district/calls/whisper`: one note to the AI on the call
//!   ([`CallWhisperResponse`]). The service sends it into the room itself,
//!   after finding the media server that holds the room, because a packet sent
//!   to a server without the room reaches nobody and still reports success.
//! - `POST /api/district/calls/{callId}/takeover`: a microphone-only
//!   credential that takes the call over from the AI
//!   ([`CallTakeoverResponse`]). The AI says a supervisor is taking over and
//!   steps back until the supervisor leaves the room.
//!
//! Each credential names the media server the service found for the room.
//! Join exactly that one: a phone call's room lives where the call was
//! carried, which the workspace's region does not say.
//!
//! While listening, the voice agent's own transcript of the call arrives in
//! the room as data packets ([`TranscriptChunk`]).

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::rooms::RoomE2ee;
use crate::transcript::TranscriptClientOp;

/// The longest whisper the service accepts, in characters.
pub const MAX_WHISPER_CHARS: usize = 2000;

/// The id of a call to supervise, and the only way this client asks the token
/// route for a call's room.
///
/// The token route reads its `roomName` as a call's id unless it starts with
/// one of the service's room prefixes, which name rooms of their own where the
/// joiner is the conversation and may speak. Every such prefix ends in `_`,
/// and a call's id is a UUID, which never holds one, so a [`LiveCallId`] has
/// no `_` at all. A listen-only credential then cannot turn into a speaking
/// one by a typo or a hostile id, whatever prefixes the service adds later.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LiveCallId(String);

impl LiveCallId {
    /// `call_id`, or `None` when the service would refuse it as a call's id
    /// (empty, longer than the transcript contract's limit, or with a
    /// character other than an ASCII letter, a digit, `_` or `-`) or could
    /// read it as another kind of room (any `_`).
    pub fn new(call_id: &str) -> Option<Self> {
        (TranscriptClientOp::is_valid_call_id(call_id) && !call_id.contains('_'))
            .then(|| Self(call_id.to_owned()))
    }

    /// The id, as sent.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LiveCallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// `POST /api/district/calls/token` for a live call: a listen-only credential
/// for the call's room, and the media server that holds it.
///
/// `Debug` leaves out the credential and the passphrase.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct CallMonitorTokenResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The media server credential, good for about thirty minutes. It cannot
    /// publish anything: the member listens. Never log it.
    pub token: String,
    /// The media server that holds the call's room, found by the room. Use
    /// this one exactly.
    pub url: String,
    /// The room's end-to-end encryption passphrase, when the room is
    /// encrypted. A phone call's room is not, so it is absent today; when it is
    /// sent, the room is joined with it, never in the clear.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e2ee: Option<RoomE2ee>,
}

impl fmt::Debug for CallMonitorTokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallMonitorTokenResponse")
            .field("success", &self.success)
            .field("token", &"<redacted>")
            .field("url", &self.url)
            .field("e2ee", &self.e2ee)
            .finish()
    }
}

/// `POST /api/district/calls/{callId}/takeover`: the credential that takes a
/// live call over from the AI, by voice.
///
/// It may publish the microphone and nothing else: no camera, no screen, no
/// data. Joining with it replaces the member's listen-only connection to the
/// same call, because the service gives both the same identity.
///
/// `Debug` leaves out the credential.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct CallTakeoverResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The media server that holds the call's room, found by the room. Use
    /// this one exactly.
    pub url: String,
    /// The media server credential, good for seventy minutes. Never log it.
    pub token: String,
    /// The call's room.
    pub room_name: String,
}

impl fmt::Debug for CallTakeoverResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallTakeoverResponse")
            .field("success", &self.success)
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .field("room_name", &self.room_name)
            .finish()
    }
}

/// `POST /api/district/calls/whisper`: the note reached the call's room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct CallWhisperResponse {
    /// `true` when it was delivered.
    #[serde(default)]
    pub success: bool,
}

/// The `type` of the voice agent's transcript packet.
pub const TRANSCRIPT_CHUNK_TYPE: &str = "TRANSCRIPT_CHUNK";

/// The `role` of a line the caller said.
const CALLER_ROLE: &str = "user";

/// One finished line of a live call, as the voice agent sends it into the
/// room for whoever is listening: the JSON data packet
/// `{"type":"TRANSCRIPT_CHUNK","role":"user"|"agent","speaker":"<name>","text":"<words>"}`.
/// The meeting Companion sends the same packet in a meeting room.
///
/// Read only: nothing here sends one. `Debug` leaves the words out.
#[derive(Clone, PartialEq, Eq)]
pub struct TranscriptChunk {
    /// `user` for the caller; anything else is the AI's side.
    pub role: String,
    /// The name the agent gives the speaker.
    pub speaker: String,
    /// The words.
    pub text: String,
}

impl TranscriptChunk {
    /// The line a data packet carries, or `None` when it is not a well-formed
    /// transcript packet: not JSON, another type of packet, or a field that is
    /// missing or not text.
    pub fn read(payload: &[u8]) -> Option<Self> {
        let packet: Value = serde_json::from_slice(payload).ok()?;
        if packet.get("type")?.as_str()? != TRANSCRIPT_CHUNK_TYPE {
            return None;
        }
        let text = |key: &str| packet.get(key)?.as_str().map(str::to_owned);
        Some(Self {
            role: text("role")?,
            speaker: text("speaker")?,
            text: text("text")?,
        })
    }

    /// Whether the caller said it.
    pub fn is_caller(&self) -> bool {
        self.role == CALLER_ROLE
    }
}

impl fmt::Debug for TranscriptChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TranscriptChunk")
            .field("role", &self.role)
            .field("text", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_call_id_is_a_call_id_and_never_a_room() {
        let uuid = "3f2b8c1e-9a4d-4e7b-8f10-2c6d5a9e0b71";
        assert_eq!(
            LiveCallId::new(uuid).map(|id| id.to_string()),
            Some(uuid.to_owned())
        );
        assert_eq!(LiveCallId::new(uuid).unwrap().as_str(), uuid);
        for refused in [
            "",
            "meet_ws_weekly",
            // Any room prefix, including one the service adds later.
            "avatar_ws_demo",
            "_",
            "call with spaces",
            "../calls",
            &"c".repeat(65),
        ] {
            assert_eq!(LiveCallId::new(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn a_transcript_packet_is_read_only_when_it_is_one() {
        let line = TranscriptChunk::read(
            br#"{"type":"TRANSCRIPT_CHUNK","role":"user","speaker":"Caller","text":"Hello"}"#,
        )
        .unwrap();
        assert!(line.is_caller());
        assert_eq!(
            (line.speaker.as_str(), line.text.as_str()),
            ("Caller", "Hello")
        );
        let agent = TranscriptChunk::read(
            br#"{"type":"TRANSCRIPT_CHUNK","role":"assistant","speaker":"Ava","text":"Hi"}"#,
        )
        .unwrap();
        assert!(!agent.is_caller());
        let printed = format!("{agent:?}");
        assert!(!printed.contains("Hi\""), "{printed}");
        assert!(printed.contains("<redacted>"), "{printed}");

        for foreign in [
            &b"not json"[..],
            br#"[1, 2]"#,
            br#"{"role":"user","speaker":"Caller","text":"Hello"}"#,
            br#"{"type":7,"role":"user","speaker":"Caller","text":"Hello"}"#,
            br#"{"type":"WHISPER","text":"Offer a discount"}"#,
            br#"{"type":"TRANSCRIPT_CHUNK","speaker":"Caller","text":"Hello"}"#,
            br#"{"type":"TRANSCRIPT_CHUNK","role":"user","speaker":null,"text":"Hello"}"#,
            br#"{"type":"TRANSCRIPT_CHUNK","role":"user","speaker":"Caller","text":3}"#,
        ] {
            assert_eq!(TranscriptChunk::read(foreign), None);
        }
    }

    #[test]
    fn no_credential_reaches_debug_output() {
        let listen = CallMonitorTokenResponse {
            success: true,
            token: "secret-listen".to_owned(),
            url: "wss://media.example".to_owned(),
            e2ee: Some(RoomE2ee {
                key: "secret-key".to_owned(),
            }),
        };
        let printed = format!("{listen:?}");
        assert!(!printed.contains("secret"), "{printed}");
        assert!(printed.contains("wss://media.example"), "{printed}");

        let takeover = CallTakeoverResponse {
            success: true,
            url: "wss://media.example".to_owned(),
            token: "secret-takeover".to_owned(),
            room_name: "call_room".to_owned(),
        };
        let printed = format!("{takeover:?}");
        assert!(!printed.contains("secret"), "{printed}");
        assert!(printed.contains("call_room"), "{printed}");
    }
}
