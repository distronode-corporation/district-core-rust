//! The words spoken in a room, as the agents framework transcribes them: one
//! text stream per update on `lk.transcription`, for the member's words as
//! well as the agent's.
//!
//! Whose words a stream carries is decided as the web client decides it: the
//! member's when the stream comes from this participant, or names one of this
//! participant's tracks as the one it transcribes (the agent sends the
//! member's words on their behalf); the agent's when an agent participant
//! sends it about anything else; and nobody's otherwise, which is not read.
//!
//! A stream's text is everything it has carried so far, reported as it grows
//! and once more when it ends, with whether its segment is final (read from
//! the attributes after the stream's trailer, which can change them). A stream
//! that breaks off keeps the last text it reported.

use std::sync::Arc;

use district_core::{MediaEvent, Speaker, SpeechSegment};
use district_model::{
    ATTR_SEGMENT_ID, ATTR_TRANSCRIBED_TRACK_ID, ATTR_TRANSCRIPTION_FINAL, TRANSCRIPTION_TOPIC,
};
use futures_util::StreamExt;
use livekit::id::ParticipantIdentity;
use livekit::prelude::Room;
use livekit::{ParticipantKind, StreamReader, TextStreamInfo, TextStreamReader};

use super::{Session, Shared};

/// Whether a text stream on `topic` is a transcription.
pub(super) fn is_transcription(topic: &str) -> bool {
    topic == TRANSCRIPTION_TOPIC
}

/// Whose words the stream `reader` from `sender` carries, or `None` when it
/// is nobody this app reads.
pub(super) fn speaker(
    room: &Room,
    sender: &ParticipantIdentity,
    reader: &TextStreamReader,
) -> Option<Speaker> {
    let local = room.local_participant();
    let transcribed = reader
        .info()
        .attributes()
        .get(ATTR_TRANSCRIBED_TRACK_ID)
        .cloned();
    let own_track = transcribed.is_some_and(|track| {
        local
            .track_publications()
            .keys()
            .any(|sid| sid.as_str() == track)
    });
    if *sender == local.identity() || own_track {
        return Some(Speaker::Member);
    }
    room.remote_participants()
        .get(sender)
        .filter(|participant| participant.kind() == ParticipantKind::Agent)
        .map(|_| Speaker::Agent)
}

/// The segment a stream updates: its `lk.segment_id`, or the stream's own id
/// when it names none.
fn segment_id(info: &TextStreamInfo) -> String {
    info.attributes()
        .get(ATTR_SEGMENT_ID)
        .filter(|id| !id.is_empty())
        .cloned()
        .unwrap_or_else(|| info.id.clone())
}

/// Reads `reader` to its end, reporting its text as it grows for `session`,
/// until the session is no longer held.
pub(super) async fn read(
    shared: Arc<Shared>,
    session: Arc<Session>,
    mut reader: TextStreamReader,
    speaker: Speaker,
) {
    let info = reader.info().clone();
    let segment_id = segment_id(&info);
    let mut text = String::new();
    let said = |text: &str, is_final: bool| {
        MediaEvent::Speech(SpeechSegment {
            segment_id: segment_id.clone(),
            speaker,
            text: text.to_owned(),
            is_final,
        })
    };
    while let Some(chunk) = reader.next().await {
        let Ok(chunk) = chunk else {
            return;
        };
        text.push_str(&chunk);
        if !shared.report(&session, said(&text, false)) {
            return;
        }
    }
    let is_final = info
        .attributes()
        .get(ATTR_TRANSCRIPTION_FINAL)
        .is_some_and(|value| value == "true");
    shared.report(&session, said(&text, is_final));
}
