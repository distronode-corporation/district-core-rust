//! A hand-off's `state`, and every way the browser's answer can be refused.
//!
//! The rule is the sign-in's: an answer whose `state` is not the one this app
//! generated is never used, and nothing about such an answer uses the hand-off
//! up. Any program can open a `districtai://handoff` link.

use std::collections::HashSet;

use district_auth::{
    HAND_OFF_HOST, HAND_OFF_NONCE_LEN, HAND_OFF_START_PATH, HandOffError, HandOffState,
    REDIRECT_SCHEME, is_valid_hand_off_nonce, is_valid_hand_off_state,
};

/// A nonce of the shape the service sends: 32 random bytes in base64url, which
/// is also what a `state` is. Made fresh for each test, never written down.
fn sent_nonce() -> String {
    HandOffState::generate().as_str().to_owned()
}

fn answer(state: &HandOffState, sent: &str) -> String {
    format!("districtai://handoff?state={}&nonce={sent}", state.as_str())
}

#[test]
fn the_names_are_the_services() {
    assert_eq!(HAND_OFF_START_PATH, "/dashboard/handoff/start");
    assert_eq!(HAND_OFF_HOST, "handoff");
    assert_eq!(REDIRECT_SCHEME, "districtai");
    assert_eq!(HAND_OFF_NONCE_LEN, 43);
    assert_eq!(sent_nonce().len(), HAND_OFF_NONCE_LEN);
}

#[test]
fn a_state_is_fresh_and_of_the_shape_the_start_page_accepts() {
    let states: HashSet<String> = (0..100)
        .map(|_| HandOffState::generate().as_str().to_owned())
        .collect();
    assert_eq!(states.len(), 100);
    for state in &states {
        assert_eq!(state.len(), 43, "{state}");
        assert!(is_valid_hand_off_state(state), "{state}");
        assert!(
            state
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "{state}"
        );
    }
}

#[test]
fn the_shape_checks_match_the_service() {
    let sent = sent_nonce();
    assert!(!is_valid_hand_off_state(&"a".repeat(15)));
    assert!(is_valid_hand_off_state(&"a".repeat(16)));
    assert!(is_valid_hand_off_state(&"a".repeat(256)));
    assert!(!is_valid_hand_off_state(&"a".repeat(257)));
    assert!(is_valid_hand_off_state("abcDEF0123-._~xy"));
    assert!(!is_valid_hand_off_state("abcDEF0123-._~x+"));
    assert!(!is_valid_hand_off_state("abcDEF0123-._~x/"));

    assert!(is_valid_hand_off_nonce(&sent));
    assert!(!is_valid_hand_off_nonce(&sent[1..]));
    assert!(!is_valid_hand_off_nonce(&format!("{sent}a")));
    for odd in ['=', '.', '~', '+', '/', ' '] {
        let nonce = format!("{}{odd}", &sent[1..]);
        assert!(!is_valid_hand_off_nonce(&nonce), "{nonce}");
    }
}

#[test]
fn the_answer_to_this_hand_off_yields_its_nonce() {
    let sent = sent_nonce();
    let state = HandOffState::generate();
    let nonce = state.check(&answer(&state, &sent)).unwrap();
    assert_eq!(nonce.as_str(), sent);
    // As the desktop may hand it over, with a `/` for a path, and with the
    // parameters the other way round.
    let slashed = format!(
        "districtai://handoff/?nonce={sent}&state={}",
        state.as_str()
    );
    assert_eq!(state.check(&slashed).unwrap(), nonce);
    // The same `state` checks again: nothing here uses it up.
    assert_eq!(state.check(&answer(&state, &sent)).unwrap(), nonce);
}

#[test]
fn a_link_that_is_not_the_hand_off_address_is_refused() {
    let sent = sent_nonce();
    let state = HandOffState::generate();
    let s = state.as_str();
    for link in [
        format!("districtai://auth?state={s}&nonce={sent}"),
        format!("districtai://HANDOFF?state={s}&nonce={sent}"),
        format!("districtai://handoff:8443?state={s}&nonce={sent}"),
        format!("districtai://ada@handoff?state={s}&nonce={sent}"),
        format!("districtai://ada:pw@handoff?state={s}&nonce={sent}"),
        format!("districtai://handoff/x?state={s}&nonce={sent}"),
        format!("districtai:handoff?state={s}&nonce={sent}"),
        format!("https://handoff/?state={s}&nonce={sent}"),
        format!("other://handoff?state={s}&nonce={sent}"),
        "not a link".to_owned(),
        String::new(),
    ] {
        assert_eq!(state.check(&link), Err(HandOffError::NotOurLink), "{link}");
    }
}

#[test]
fn a_link_for_another_hand_off_is_refused() {
    let sent = sent_nonce();
    let state = HandOffState::generate();
    let other = HandOffState::generate();
    let s = state.as_str();
    for link in [
        answer(&other, &sent),
        format!("districtai://handoff?nonce={sent}"),
        format!("districtai://handoff?state=&nonce={sent}"),
        format!("districtai://handoff?state={s}&state={s}&nonce={sent}"),
        format!("districtai://handoff?state={s}x&nonce={sent}"),
        format!("districtai://handoff?state={}&nonce={sent}", &s[1..]),
    ] {
        assert_eq!(
            state.check(&link),
            Err(HandOffError::StateMismatch),
            "{link}"
        );
    }
}

#[test]
fn a_missing_repeated_or_malformed_nonce_is_refused() {
    let sent = sent_nonce();
    let state = HandOffState::generate();
    let s = state.as_str();
    for link in [
        format!("districtai://handoff?state={s}"),
        format!("districtai://handoff?state={s}&nonce="),
        format!("districtai://handoff?state={s}&nonce={sent}&nonce={sent}"),
        format!("districtai://handoff?state={s}&nonce={}", &sent[1..]),
        format!("districtai://handoff?state={s}&nonce={sent}a"),
        format!("districtai://handoff?state={s}&nonce={}%2B", &sent[1..]),
    ] {
        assert_eq!(
            state.check(&link),
            Err(HandOffError::MalformedNonce),
            "{link}"
        );
    }
}

#[test]
fn neither_the_state_nor_the_nonce_is_ever_printed() {
    let sent = sent_nonce();
    let state = HandOffState::generate();
    let nonce = state.check(&answer(&state, &sent)).unwrap();
    let shown = format!("{state:?} {nonce:?}");
    assert_eq!(shown, "HandOffState(<redacted>) HandOffNonce(<redacted>)");
    assert!(!shown.contains(state.as_str()) && !shown.contains(&sent));
    for error in [
        HandOffError::NotOurLink,
        HandOffError::StateMismatch,
        HandOffError::MalformedNonce,
    ] {
        assert!(!error.to_string().is_empty());
        assert!(!format!("{error:?}").contains(&sent));
    }
}
