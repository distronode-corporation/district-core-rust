//! HQ voice mode's wire, read from DRAFT fixtures in `tests/hq-voice-drafts/`.
//!
//! The server does not record these yet, so none of them is in `contracts/`,
//! which holds only what `scripts/sync-contracts.py` vendors (and which the
//! manifest, the checksums and every app's contract gate count). They are the
//! drafts written from the server and voice agent source for every app at
//! once, copied here byte for byte; when the server's own tests record them,
//! the recorded files land in `contracts/` through the sync script and replace
//! these, and this test moves to the manifest.
//!
//! The packets are not route bodies: the agent sends them compact, over the
//! room, so they are read with `read_packet` (never compared as bytes), and
//! anything that does not validate is ignored, as the web client does.

use std::fs;
use std::path::PathBuf;

use district_model::{
    HqConfirmResponse, HqProposalCancelResponse, HqResolutionStatus, HqVoicePacket,
    HqVoiceResolution, read_packet,
};
use serde_json::{Value, json};

use crate::round_trip::differences;
use crate::support::{codec, decode_str};

fn draft(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/hq-voice-drafts")
        .join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// The draft's packet, sent as the agent sends it: compact JSON, non-ASCII
/// unescaped.
fn packet(name: &str) -> Option<HqVoicePacket> {
    let value: Value = serde_json::from_str(&draft(name)).expect("a draft is JSON");
    read_packet(value.to_string().as_bytes())
}

fn resolution(status: HqResolutionStatus) -> HqVoiceResolution {
    HqVoiceResolution {
        proposal_id: "11111111-1111-4111-8111-111111111111".to_owned(),
        status,
        error: None,
        join_call: None,
    }
}

#[test]
fn the_proposal_packet_reads_with_its_arguments_as_sent() {
    let Some(HqVoicePacket::Proposal(proposal)) = packet("hq-voice-packet-proposal.json") else {
        panic!("the proposal draft does not read");
    };
    assert_eq!(proposal.proposal_id, "11111111-1111-4111-8111-111111111111");
    assert_eq!(proposal.tool, "update_persona");
    assert_eq!(
        Value::Object(proposal.args),
        json!({"greeting": "Good afternoon, thanks for calling Analytical Engines."})
    );
    assert!(
        proposal
            .summary
            .starts_with("Update the AI receptionist persona")
    );
    // The pretty-printed file itself reads too: the reader parses, it does not
    // compare bytes.
    assert!(read_packet(draft("hq-voice-packet-proposal.json").as_bytes()).is_some());
}

#[test]
fn each_resolution_packet_reads_with_its_status() {
    for (name, status) in [
        (
            "hq-voice-packet-resolved-applied.json",
            HqResolutionStatus::Applied,
        ),
        (
            "hq-voice-packet-resolved-cancelled.json",
            HqResolutionStatus::Cancelled,
        ),
    ] {
        assert_eq!(
            packet(name),
            Some(HqVoicePacket::Resolved(resolution(status))),
            "{name}"
        );
    }
    assert_eq!(
        packet("hq-voice-packet-resolved-failed.json"),
        Some(HqVoicePacket::Resolved(HqVoiceResolution {
            error: Some("The change could not be applied.".to_owned()),
            ..resolution(HqResolutionStatus::Failed)
        }))
    );
    assert_eq!(
        packet("hq-voice-packet-resolved-join-call.json"),
        Some(HqVoicePacket::Resolved(HqVoiceResolution {
            join_call: Some("call-contract-test-1".to_owned()),
            ..resolution(HqResolutionStatus::Applied)
        }))
    );
}

#[test]
fn the_click_answers_decode_strictly_and_round_trip() {
    for (name, codec) in [
        (
            "district-hq-voice-confirm.json",
            codec::<HqConfirmResponse> as fn(&str) -> Result<Value, String>,
        ),
        (
            "district-hq-proposal-cancel.json",
            codec::<HqProposalCancelResponse>,
        ),
    ] {
        let raw = draft(name);
        let encoded = codec(&raw).unwrap_or_else(|error| panic!("{name} {error}"));
        let original: Value = serde_json::from_str(&raw).expect("JSON");
        assert_eq!(differences(&original, &encoded), Vec::new(), "{name}");
    }
    let confirmed: HqConfirmResponse = decode_str(
        "district-hq-voice-confirm.json",
        &draft("district-hq-voice-confirm.json"),
    );
    assert!(confirmed.success && confirmed.executed);
    let cancelled: HqProposalCancelResponse = decode_str(
        "district-hq-proposal-cancel.json",
        &draft("district-hq-proposal-cancel.json"),
    );
    assert!(cancelled.success);
}
