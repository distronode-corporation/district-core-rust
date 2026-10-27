//! District HQ by voice: what a voice agent sends in an encrypted room reaches
//! the engine decrypted. Its state attribute, a data packet on the proposal
//! topic, and transcription streams, the member's words (sent by the agent on
//! the member's behalf, naming the member's microphone) and its own.
//!
//! The agent here is the media library itself, joined as an agent with the
//! same passphrase and the web client's key settings, as the voice agent joins
//! (its `encryption` option also encrypts data, so this is the path the real
//! one takes).

use std::collections::HashMap;
use std::time::Duration;

use district_core::{CallEngine, MediaEvent, MicrophoneState, Speaker, SpeechSegment};
use livekit::StreamTextOptions;
use livekit::e2ee::key_provider::{KeyProvider, KeyProviderOptions};
use livekit::e2ee::{E2eeOptions, EncryptionType};
use livekit::prelude::{DataPacket, Room, RoomOptions};

use crate::encryption::PASSPHRASE;
use crate::support::{Kind, Member, Peer, Server, runtime, token};

const JOIN: Duration = Duration::from_secs(20);
const SHORT: Duration = Duration::from_secs(10);

const PROPOSAL: &str = r#"{"type": "HQ_PROPOSAL", "proposalId": "11111111-1111-4111-8111-111111111111", "tool": "update_persona", "args": {"greeting": "Bonjour, ici l'accueil."}, "summary": "Mettre à jour l'accueil."}"#;

/// The options the voice agent joins with: the passphrase as the shared key,
/// verbatim, no ratchet window and no limit on failures.
fn agent_options() -> RoomOptions {
    let mut options = RoomOptions::default();
    options.encryption = Some(E2eeOptions {
        encryption_type: EncryptionType::Gcm,
        key_provider: KeyProvider::with_shared_key(
            KeyProviderOptions {
                ratchet_window_size: 0,
                failure_tolerance: -1,
                ..KeyProviderOptions::default()
            },
            PASSPHRASE.as_bytes().to_vec(),
        ),
    });
    options
}

#[test]
fn what_the_voice_agent_sends_in_an_encrypted_room_is_reported() {
    runtime().block_on(async {
        let server = Server::start();
        let url = server.url();
        let room = "hq_ws-test_00000000-0000-4000-8000-000000000000";

        let mut member = Peer::new(true);
        let (session, credential) = Member::new().join(
            &url,
            &token(room, "hq-admin-1", Kind::Standard),
            Some(PASSPHRASE),
        );
        member.engine.connect(session, credential, true).await;
        member
            .reports
            .until_event(JOIN, session, &MediaEvent::Microphone(MicrophoneState::On))
            .await;

        let (agent, _events) =
            Room::connect(&url, &token(room, "agent-hq", Kind::Agent), agent_options())
                .await
                .expect("the agent joins");
        let seen = member
            .reports
            .until(
                JOIN,
                |update| matches!(&update.event, MediaEvent::ParticipantJoined(p) if p.is_agent),
            )
            .await;
        assert!(seen.iter().all(|update| update.session == session));

        agent
            .local_participant()
            .set_attributes(HashMap::from([(
                "lk.agent.state".to_owned(),
                "listening".to_owned(),
            )]))
            .await
            .expect("the agent sets its state");
        member
            .reports
            .until_event(
                SHORT,
                session,
                &MediaEvent::AgentState("listening".to_owned()),
            )
            .await;

        agent
            .local_participant()
            .publish_data(DataPacket {
                payload: PROPOSAL.as_bytes().to_vec(),
                topic: Some("hq.proposal".to_owned()),
                reliable: true,
                destination_identities: Vec::new(),
            })
            .await
            .expect("the agent sends a proposal");
        member
            .reports
            .until_event(
                SHORT,
                session,
                &MediaEvent::AgentData {
                    topic: "hq.proposal".to_owned(),
                    payload: PROPOSAL.as_bytes().to_vec(),
                },
            )
            .await;

        // The member's words: sent by the agent, naming the member's
        // microphone as the track they transcribe.
        let microphone = agent
            .remote_participants()
            .values()
            .find(|participant| participant.identity().as_str() == "hq-admin-1")
            .and_then(|participant| participant.track_publications().keys().next().cloned())
            .expect("the member's microphone is published");
        agent
            .local_participant()
            .send_text(
                "How many calls today?",
                StreamTextOptions::new_with_topic("lk.transcription")
                    .with_attribute("lk.segment_id", "member-1")
                    .with_attribute("lk.transcribed_track_id", microphone.as_str())
                    .with_attribute("lk.transcription_final", "true"),
            )
            .await
            .expect("the agent sends the member's words");
        member
            .reports
            .until_event(
                SHORT,
                session,
                &MediaEvent::Speech(SpeechSegment {
                    segment_id: "member-1".to_owned(),
                    speaker: Speaker::Member,
                    text: "How many calls today?".to_owned(),
                    is_final: true,
                }),
            )
            .await;

        // Its own words, about nobody's track.
        agent
            .local_participant()
            .send_text(
                "Seven so far.",
                StreamTextOptions::new_with_topic("lk.transcription")
                    .with_attribute("lk.segment_id", "agent-1"),
            )
            .await
            .expect("the agent sends its words");
        member
            .reports
            .until_event(
                SHORT,
                session,
                &MediaEvent::Speech(SpeechSegment {
                    segment_id: "agent-1".to_owned(),
                    speaker: Speaker::Agent,
                    text: "Seven so far.".to_owned(),
                    is_final: false,
                }),
            )
            .await;

        // A text stream on another topic is not a transcription.
        agent
            .local_participant()
            .send_text("hello", StreamTextOptions::new_with_topic("lk.chat"))
            .await
            .expect("the agent chats");
        // What follows is at most the end of the agent's stream, reported once
        // more as it closes: nothing from the chat.
        let later = member.reports.during(Duration::from_secs(2)).await;
        assert!(
            later.iter().all(|update| match &update.event {
                MediaEvent::Speech(segment) => segment.segment_id == "agent-1",
                _ => true,
            }),
            "{later:?}"
        );

        agent.close().await.ok();
        member.engine.disconnect(session).await;
    });
}
