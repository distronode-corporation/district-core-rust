//! District HQ by voice: the credential for a voice session, the cancellation
//! of a proposal, and what the voice agent sends the app in the room.
//!
//! A voice session is a media room the service names `hq_<workspace>_<uuid>`,
//! always end-to-end encrypted, which the voice agent joins by itself. The
//! member speaks; the agent answers aloud, sends both sides' words as
//! transcription streams, and sends every change it proposes as a packet on
//! [`HQ_PROPOSAL_TOPIC`]. The app can never send the agent anything but its
//! voice: the credential may not publish data.
//!
//! A proposal is applied in one of two ways, both outside the app's own
//! judgement: the member says "confirm" and the agent applies it (and then
//! sends a [`HqVoiceResolution`]), or the member clicks Confirm and the app
//! sends the proposal back to `POST /api/district/hq`, with its id.
//!
//! The packets are read by hand ([`read_packet`]) rather than decoded with
//! unknown fields refused: they come from the agent over the room, not from a
//! route with a recorded fixture, and like the web client's readers, a packet
//! that does not validate is ignored rather than an error.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::rooms::RoomE2ee;

/// The prefix of every HQ voice room's name. The service names the room; a
/// client never builds one.
pub const HQ_ROOM_PREFIX: &str = "hq_";

/// The data topic the voice agent sends proposals and their resolutions on.
pub const HQ_PROPOSAL_TOPIC: &str = "hq.proposal";

/// The text stream topic the agents framework sends transcriptions on, both the
/// member's words and the agent's.
pub const TRANSCRIPTION_TOPIC: &str = "lk.transcription";

/// The transcription stream attribute naming the segment a stream updates.
pub const ATTR_SEGMENT_ID: &str = "lk.segment_id";

/// The transcription stream attribute naming the track whose speech it
/// transcribes: one of the member's own when the words are the member's.
pub const ATTR_TRANSCRIBED_TRACK_ID: &str = "lk.transcribed_track_id";

/// The transcription stream attribute saying the segment is final, `"true"`.
pub const ATTR_TRANSCRIPTION_FINAL: &str = "lk.transcription_final";

/// The agent participant's attribute holding what it is doing.
pub const ATTR_AGENT_STATE: &str = "lk.agent.state";

/// How long a voice session may last: the agent ends it, and the credential
/// expires, after thirty minutes.
pub const HQ_VOICE_SESSION_SECONDS: u64 = 30 * 60;

/// How many earlier turns a voice session may start with. The service keeps
/// the last this many and drops the rest.
pub const HQ_VOICE_HISTORY_MAX_TURNS: usize = 6;

/// How many characters of each earlier turn the service keeps.
pub const HQ_VOICE_HISTORY_MAX_CHARS: usize = 500;

/// `POST /api/district/hq/voice-token`: the credential for one voice session.
///
/// Its `Debug` output leaves out the media token, which carries the session's
/// grant, and the room's passphrase.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct HqVoiceTokenResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The media server credential. Never log it.
    pub token: String,
    /// The media server to join, exactly as named: the room exists only there,
    /// whatever region the workspace is in.
    pub url: String,
    /// The room's name, `hq_<workspace>_<uuid>`, fresh for every session.
    pub room_name: String,
    /// Where the member's voice is processed, `us` today. The app says so
    /// before the microphone starts.
    pub processing_region: String,
    /// The room's end-to-end encryption passphrase. Absent when the room has
    /// none, which the app does not join.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e2ee: Option<RoomE2ee>,
}

impl HqVoiceTokenResponse {
    /// Whether this credential is one to join: an HQ room, with a passphrase.
    /// A room without one is not joined unencrypted; it is not joined.
    pub fn is_joinable(&self) -> bool {
        self.room_name.starts_with(HQ_ROOM_PREFIX)
            && self
                .e2ee
                .as_ref()
                .is_some_and(|e2ee| !e2ee.key.trim().is_empty())
    }
}

impl fmt::Debug for HqVoiceTokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HqVoiceTokenResponse")
            .field("success", &self.success)
            .field("token", &"<redacted>")
            .field("url", &self.url)
            .field("room_name", &self.room_name)
            .field("processing_region", &self.processing_region)
            .field("e2ee", &self.e2ee)
            .finish()
    }
}

/// `POST /api/district/hq/proposals/cancel`: the proposal is cancelled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct HqProposalCancelResponse {
    /// `true` when it was cancelled.
    #[serde(default)]
    pub success: bool,
}

/// The longest record id a screen report may name. The service refuses the
/// whole report over a longer one, or one with any character outside
/// `A-Z`, `a-z`, `0-9`, `_` and `-`.
pub const HQ_VOICE_RECORD_ID_MAX: usize = 128;

/// The kind of record a screen report names, as the service's page table
/// names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HqVoiceRecordKind {
    /// A call, by its id.
    Call,
    /// A contact, by its id.
    Contact,
    /// A conversation in the inbox, by its contact's id.
    Conversation,
}

/// The record open on screen: its kind and its id, never anything that names
/// a person.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct HqVoiceSelection {
    /// What it is.
    pub kind: HqVoiceRecordKind,
    /// Its id.
    pub id: String,
}

impl HqVoiceSelection {
    /// The record `id` of `kind`, or `None` when the service would refuse the
    /// id (empty, longer than [`HQ_VOICE_RECORD_ID_MAX`], or with a character
    /// other than an ASCII letter, a digit, `_` or `-`): a report naming it
    /// would be refused whole, so it is reported without one.
    pub fn new(kind: HqVoiceRecordKind, id: &str) -> Option<Self> {
        let valid = !id.is_empty()
            && id.len() <= HQ_VOICE_RECORD_ID_MAX
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        valid.then(|| Self {
            kind,
            id: id.to_owned(),
        })
    }
}

/// What is on the member's screen during a voice session: the `snapshot` of
/// `POST /api/district/hq/voice-context`, which the voice agent reads back so
/// it knows the page without asking. The web dashboard reports the same.
///
/// Free of personal data by construction: a page id from the service's page
/// table and a record's id. The service refuses anything else, and stores
/// nothing else.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HqVoiceScreen {
    /// The session's count of reports, from 1, one more for every change
    /// reported. The agent reads the screen again when it moves.
    pub version: u64,
    /// The page: an id of the service's page table (`hq`, `calls`, ...).
    pub page: String,
    /// The record open on the page, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<HqVoiceSelection>,
}

/// `POST /api/district/hq/voice-context`: the screen was recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct HqVoiceContextResponse {
    /// `true` when it was recorded.
    #[serde(default)]
    pub success: bool,
}

/// A change the voice agent proposed, as its `HQ_PROPOSAL` packet carries it.
///
/// Nothing has been changed. Show [`summary`](Self::summary); confirming by
/// click sends [`tool`](Self::tool) and [`args`](Self::args) back exactly as
/// they came, with the [`proposal_id`](Self::proposal_id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HqVoiceProposal {
    /// The service's id for the proposal, which keys its card.
    pub proposal_id: String,
    /// The write, for example `update_persona`. Not for display.
    pub tool: String,
    /// The write's arguments, kept as plain JSON, never read, and returned
    /// untouched: the service refuses a confirmation whose arguments differ.
    pub args: Map<String, Value>,
    /// One sentence saying what the change would do, written by the service.
    /// May be empty, in which case the app shows its own sentence.
    pub summary: String,
}

/// What became of a proposal the member resolved by voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HqResolutionStatus {
    /// The member said confirm, and the change was made.
    Applied,
    /// The member said confirm, and the change could not be made.
    Failed,
    /// The member said cancel.
    Cancelled,
}

impl HqResolutionStatus {
    fn from_wire(raw: &str) -> Option<Self> {
        match raw {
            "applied" => Some(Self::Applied),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// A proposal the member resolved by voice, as its `HQ_PROPOSAL_RESOLVED`
/// packet carries it. A proposal resolved by a click sends none: the click's
/// own answer says what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HqVoiceResolution {
    /// The proposal's id.
    pub proposal_id: String,
    /// What became of it.
    pub status: HqResolutionStatus,
    /// Why a change could not be made, on [`HqResolutionStatus::Failed`]. The
    /// tool's own words, in English: for diagnostics, not for a French screen.
    pub error: Option<String>,
    /// A live call the member can join, on [`HqResolutionStatus::Applied`],
    /// when the change was taking one over.
    pub join_call: Option<String>,
}

/// One packet the voice agent sends on [`HQ_PROPOSAL_TOPIC`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HqVoicePacket {
    /// A change proposed.
    Proposal(HqVoiceProposal),
    /// A proposal resolved by voice.
    Resolved(HqVoiceResolution),
}

/// The packet in `payload`, or `None` for anything that is not one: not JSON,
/// not an object, an unknown type, or a required field missing, empty or of the
/// wrong kind. Never panics. Fields it does not know are ignored.
///
/// The rules are the web client's: a proposal needs a non-empty id and tool,
/// an object of arguments and a string summary (which may be empty); a
/// resolution needs a non-empty id, one of the three statuses, and `via` equal
/// to `"voice"`. Only a non-empty `error` is kept, and a `joinCall` only on an
/// applied change and only with a non-empty `callId`.
pub fn read_packet(payload: &[u8]) -> Option<HqVoicePacket> {
    let Value::Object(packet) = serde_json::from_slice::<Value>(payload).ok()? else {
        return None;
    };
    let text = |key: &str| packet.get(key).and_then(Value::as_str);
    let filled = |key: &str| text(key).filter(|value| !value.is_empty());
    match text("type")? {
        "HQ_PROPOSAL" => Some(HqVoicePacket::Proposal(HqVoiceProposal {
            proposal_id: filled("proposalId")?.to_owned(),
            tool: filled("tool")?.to_owned(),
            args: packet.get("args")?.as_object()?.clone(),
            summary: text("summary")?.to_owned(),
        })),
        "HQ_PROPOSAL_RESOLVED" if text("via") == Some("voice") => {
            let status = HqResolutionStatus::from_wire(text("status")?)?;
            let join_call = packet
                .get("joinCall")
                .and_then(|join| join.get("callId"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && status == HqResolutionStatus::Applied)
                .map(str::to_owned);
            Some(HqVoicePacket::Resolved(HqVoiceResolution {
                proposal_id: filled("proposalId")?.to_owned(),
                status,
                error: filled("error").map(str::to_owned),
                join_call,
            }))
        }
        _ => None,
    }
}

/// What the voice agent is doing, from its [`ATTR_AGENT_STATE`] attribute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HqAgentState {
    /// Starting up.
    Initializing,
    /// Waiting.
    Idle,
    /// Hearing the member.
    Listening,
    /// Working out an answer.
    Thinking,
    /// Answering aloud.
    Speaking,
}

impl HqAgentState {
    /// The state the attribute names, or `None` for a value this build does
    /// not know, which the app treats as still connecting.
    pub fn from_wire(raw: &str) -> Option<Self> {
        match raw {
            "initializing" => Some(Self::Initializing),
            "idle" => Some(Self::Idle),
            "listening" => Some(Self::Listening),
            "thinking" => Some(Self::Thinking),
            "speaking" => Some(Self::Speaking),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn packet(value: Value) -> Option<HqVoicePacket> {
        read_packet(value.to_string().as_bytes())
    }

    fn proposal() -> Value {
        json!({
            "type": "HQ_PROPOSAL",
            "proposalId": "p-1",
            "tool": "update_persona",
            "args": {"greeting": "Hello."},
            "summary": "Update the greeting.",
        })
    }

    fn resolved(status: &str) -> Value {
        json!({"type": "HQ_PROPOSAL_RESOLVED", "proposalId": "p-1", "status": status, "via": "voice"})
    }

    fn with(mut value: Value, key: &str, field: Value) -> Value {
        value[key] = field;
        value
    }

    fn without(mut value: Value, key: &str) -> Value {
        value.as_object_mut().unwrap().remove(key);
        value
    }

    #[test]
    fn a_proposal_is_read_with_its_arguments_untouched() {
        let read = HqVoiceProposal {
            proposal_id: "p-1".to_owned(),
            tool: "update_persona".to_owned(),
            args: json!({"greeting": "Hello."}).as_object().unwrap().clone(),
            summary: "Update the greeting.".to_owned(),
        };
        assert_eq!(
            packet(with(proposal(), "extra", json!(1))),
            Some(HqVoicePacket::Proposal(read))
        );
        // An empty summary is a proposal still: the app writes its own.
        assert!(packet(with(proposal(), "summary", json!(""))).is_some());
    }

    #[test]
    fn a_proposal_missing_anything_it_needs_is_ignored() {
        for broken in [
            without(proposal(), "proposalId"),
            with(proposal(), "proposalId", json!("")),
            with(proposal(), "proposalId", json!(7)),
            with(proposal(), "tool", json!("")),
            without(proposal(), "tool"),
            with(proposal(), "args", json!([1])),
            with(proposal(), "args", json!(null)),
            without(proposal(), "args"),
            with(proposal(), "summary", json!(null)),
            without(proposal(), "summary"),
            with(proposal(), "type", json!("HQ_PROPOSALS")),
            without(proposal(), "type"),
        ] {
            assert_eq!(packet(broken.clone()), None, "{broken}");
        }
    }

    #[test]
    fn a_voice_resolution_is_read_with_its_status() {
        for (wire, status) in [
            ("applied", HqResolutionStatus::Applied),
            ("failed", HqResolutionStatus::Failed),
            ("cancelled", HqResolutionStatus::Cancelled),
        ] {
            assert_eq!(
                packet(resolved(wire)),
                Some(HqVoicePacket::Resolved(HqVoiceResolution {
                    proposal_id: "p-1".to_owned(),
                    status,
                    error: None,
                    join_call: None,
                }))
            );
        }
    }

    fn resolution(
        status: HqResolutionStatus,
        error: Option<&str>,
        join_call: Option<&str>,
    ) -> Option<HqVoicePacket> {
        Some(HqVoicePacket::Resolved(HqVoiceResolution {
            proposal_id: "p-1".to_owned(),
            status,
            error: error.map(str::to_owned),
            join_call: join_call.map(str::to_owned),
        }))
    }

    #[test]
    fn an_error_and_a_call_to_join_are_kept_only_where_they_belong() {
        use HqResolutionStatus::{Applied, Cancelled, Failed};
        let failed = with(resolved("failed"), "error", json!("Tool refused."));
        assert_eq!(
            packet(failed),
            resolution(Failed, Some("Tool refused."), None)
        );
        let empty = with(resolved("failed"), "error", json!(""));
        assert_eq!(packet(empty), resolution(Failed, None, None));

        let call = json!({"callId": "call-1"});
        assert_eq!(
            packet(with(resolved("applied"), "joinCall", call.clone())),
            resolution(Applied, None, Some("call-1"))
        );
        for (status, wire, join) in [
            (Cancelled, "cancelled", call),
            (Applied, "applied", json!({"callId": ""})),
            (Applied, "applied", json!({"callId": 3})),
            (Applied, "applied", json!("call-1")),
        ] {
            assert_eq!(
                packet(with(resolved(wire), "joinCall", join.clone())),
                resolution(status, None, None),
                "{wire} {join}"
            );
        }
    }

    #[test]
    fn a_resolution_not_by_voice_or_of_another_status_is_ignored() {
        for broken in [
            with(resolved("applied"), "via", json!("click")),
            without(resolved("applied"), "via"),
            resolved("applying"),
            with(resolved("applied"), "status", json!(1)),
            with(resolved("applied"), "proposalId", json!("")),
            without(resolved("applied"), "proposalId"),
        ] {
            assert_eq!(packet(broken.clone()), None, "{broken}");
        }
    }

    #[test]
    fn what_is_not_a_packet_is_ignored_without_a_panic() {
        for payload in [
            &b""[..],
            b"not json",
            b"[]",
            b"\"HQ_PROPOSAL\"",
            b"null",
            b"{\"type\": 1}",
            &[0xff, 0xfe, 0x00],
            b"{\"type\":\"HQ_PROPOSAL\"",
        ] {
            assert_eq!(read_packet(payload), None, "{payload:?}");
        }
    }

    #[test]
    fn the_agents_states_are_read_and_an_unknown_one_is_not() {
        for (wire, state) in [
            ("initializing", HqAgentState::Initializing),
            ("idle", HqAgentState::Idle),
            ("listening", HqAgentState::Listening),
            ("thinking", HqAgentState::Thinking),
            ("speaking", HqAgentState::Speaking),
        ] {
            assert_eq!(HqAgentState::from_wire(wire), Some(state));
        }
        assert_eq!(HqAgentState::from_wire("Speaking"), None);
        assert_eq!(HqAgentState::from_wire(""), None);
    }

    fn token(body: Value) -> HqVoiceTokenResponse {
        serde_json::from_value(body).unwrap()
    }

    fn credential() -> Value {
        json!({
            "success": true,
            "token": "secret-jwt",
            "url": "wss://media.example.com",
            "roomName": "hq_ws-1_00000000-0000-4000-8000-000000000000",
            "processingRegion": "us",
            "e2ee": {"key": "secret-key"},
        })
    }

    #[test]
    fn only_an_encrypted_hq_room_is_joinable() {
        assert!(token(credential()).is_joinable());
        assert!(!token(without(credential(), "e2ee")).is_joinable());
        assert!(!token(with(credential(), "e2ee", json!({"key": "  "}))).is_joinable());
        assert!(!token(with(credential(), "roomName", json!("meet_ws-1_x"))).is_joinable());
    }

    #[test]
    fn no_secret_of_a_voice_credential_is_printed() {
        let shown = format!("{:?}", token(credential()));
        assert!(
            !shown.contains("secret-jwt") && !shown.contains("secret-key"),
            "{shown}"
        );
        assert!(shown.contains("wss://media.example.com") && shown.contains("\"us\""));
        // Absent, never null, and left out again when written.
        let bare = token(without(credential(), "e2ee"));
        assert!(serde_json::to_value(&bare).unwrap().get("e2ee").is_none());
    }

    #[test]
    fn a_screen_is_reported_as_the_service_reads_it() {
        let call = HqVoiceSelection::new(HqVoiceRecordKind::Call, "call_A-1").unwrap();
        let screen = HqVoiceScreen {
            version: 3,
            page: "calls".to_owned(),
            selected: Some(call),
        };
        assert_eq!(
            serde_json::to_value(&screen).unwrap(),
            json!({"version": 3, "page": "calls", "selected": {"kind": "call", "id": "call_A-1"}})
        );
        let bare = HqVoiceScreen {
            selected: None,
            ..screen
        };
        assert_eq!(
            serde_json::to_value(&bare).unwrap(),
            json!({"version": 3, "page": "calls"}),
            "no record is left out, never null"
        );
        for (kind, wire) in [
            (HqVoiceRecordKind::Contact, "contact"),
            (HqVoiceRecordKind::Conversation, "conversation"),
        ] {
            assert_eq!(serde_json::to_value(kind).unwrap(), json!(wire));
        }
        let answer: HqVoiceContextResponse =
            serde_json::from_value(json!({"success": true})).unwrap();
        assert!(answer.success);
    }

    #[test]
    fn a_record_id_the_service_would_refuse_is_not_reported() {
        let longest = "a".repeat(HQ_VOICE_RECORD_ID_MAX);
        assert!(HqVoiceSelection::new(HqVoiceRecordKind::Contact, &longest).is_some());
        for refused in [
            String::new(),
            "a".repeat(HQ_VOICE_RECORD_ID_MAX + 1),
            "+15145550100".to_owned(),
            "ada@example.com".to_owned(),
            "two words".to_owned(),
            "caf\u{e9}".to_owned(),
        ] {
            assert_eq!(
                HqVoiceSelection::new(HqVoiceRecordKind::Contact, &refused),
                None,
                "{refused}"
            );
        }
    }
}
