//! Supervising a live call: the three route answers, written from the
//! server's route source until its contract tests record them.
//!
//! None of them is in `contracts/` yet, which holds only what
//! `scripts/sync-contracts.py` vendors. Each body below is the exact shape its
//! route returns (`calls/token` for a call's id, `calls/{callId}/takeover`,
//! `calls/whisper`), held here strictly: an extra key or a missing one fails.
//! When the server records them, they move to the manifest.

use district_model::{CallMonitorTokenResponse, CallTakeoverResponse, CallWhisperResponse};
use serde_json::{Value, json};

use crate::round_trip::differences;
use crate::support::{Codec, codec, decode_str};

fn bodies() -> Vec<(&'static str, Value, Codec)> {
    vec![
        (
            "calls/token, a call's id",
            json!({
                "success": true,
                "token": "eyJ.listen.only",
                "url": "wss://livekit.example.com",
            }),
            codec::<CallMonitorTokenResponse>,
        ),
        (
            "calls/{callId}/takeover",
            json!({
                "success": true,
                "url": "wss://livekit.example.com",
                "token": "eyJ.microphone.only",
                "roomName": "call_contract_room",
            }),
            codec::<CallTakeoverResponse>,
        ),
        (
            "calls/whisper",
            json!({"success": true}),
            codec::<CallWhisperResponse>,
        ),
    ]
}

#[test]
fn each_answer_decodes_strictly_and_round_trips() {
    for (name, body, codec) in bodies() {
        let encoded = codec(&body.to_string()).unwrap_or_else(|error| panic!("{name} {error}"));
        assert_eq!(differences(&body, &encoded), Vec::new(), "{name}");
    }
}

#[test]
fn an_unknown_key_is_a_finding() {
    for (name, mut body, codec) in bodies() {
        body["unexpected"] = json!(1);
        assert!(codec(&body.to_string()).is_err(), "{name}");
    }
}

#[test]
fn an_encrypted_room_carries_its_passphrase_to_the_listener() {
    let listen: CallMonitorTokenResponse = decode_str(
        "calls/token, an encrypted room",
        &json!({
            "success": true,
            "token": "eyJ.listen.only",
            "url": "wss://livekit.example.com",
            "e2ee": {"key": "c2VjcmV0"},
        })
        .to_string(),
    );
    assert_eq!(
        listen.e2ee.map(|e2ee| e2ee.key),
        Some("c2VjcmV0".to_owned())
    );
}

#[test]
fn a_refusal_keeps_success_false() {
    let refused: CallWhisperResponse = decode_str("calls/whisper, refused", "{}");
    assert!(!refused.success);
}
