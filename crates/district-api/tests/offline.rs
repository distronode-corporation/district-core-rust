//! Works offline, read-only: the saved copy of the first screens' reads.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use district_api::offline::{SAVED_COPY_MAX_AGE_MS, SAVED_COPY_MAX_BYTES, is_saved_read};
use district_api::{
    AccessToken, ApiClient, ApiConfig, ApiError, Clock, Endpoint, MemoryReadCache, ReadCache,
    ReauthReason, RetryReason, SavedRead, TokenError, TokenSource, UnauthorizedReason,
};
use district_l10n::Locale;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;

const LIST: &str = "/api/district/workspace/list";

/// A clock the test moves by hand.
#[derive(Clone, Default)]
struct HandClock(Arc<AtomicI64>);

impl HandClock {
    fn at(ms: i64) -> Self {
        let clock = Self::default();
        clock.set(ms);
        clock
    }
    fn set(&self, ms: i64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl Clock for HandClock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// A token source that is either fine, offline (its refresh cannot leave the
/// machine), or signed out.
#[derive(Clone, Default)]
struct Tokens {
    offline: Arc<AtomicBool>,
    signed_out: Arc<AtomicBool>,
}

impl Tokens {
    fn go_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::SeqCst);
    }
}

impl TokenSource for Tokens {
    async fn access_token(&self) -> Result<AccessToken, TokenError> {
        if self.signed_out.load(Ordering::SeqCst) {
            Err(TokenError::SignInRequired(ReauthReason::NoSession))
        } else if self.offline.load(Ordering::SeqCst) {
            Err(TokenError::RetryLater(RetryReason::Offline))
        } else {
            Ok(AccessToken::new("t1"))
        }
    }

    fn invalidate(&self, _rejected: &AccessToken) -> bool {
        false
    }
}

struct Setup {
    client: ApiClient<Tokens>,
    tokens: Tokens,
    clock: HandClock,
    cache: Arc<MemoryReadCache>,
}

fn setup(server: &MockServer) -> Setup {
    let tokens = Tokens::default();
    let clock = HandClock::at(1_000);
    let cache = Arc::new(MemoryReadCache::new());
    let config = ApiConfig::new(common::app())
        .with_base_url(&server.uri())
        .expect("the mock server URI parses");
    let client = ApiClient::new(config, tokens.clone())
        .expect("a loopback base URL is accepted")
        .with_saved_reads(cache.clone(), clock.clone());
    Setup {
        client,
        tokens,
        clock,
        cache,
    }
}

fn list(name: &str) -> Value {
    json!({"success": true, "workspaces": [
        {"id": "ws-1", "name": name, "region": "us", "role": "agency"}
    ]})
}

async fn answer(server: &MockServer, route: &str, template: ResponseTemplate) {
    server.reset().await;
    Mock::given(method("GET"))
        .and(path(route))
        .respond_with(template)
        .mount(server)
        .await;
}

fn name(list: &district_model::WorkspaceListResponse) -> &str {
    &list.workspaces[0].name
}

#[tokio::test]
async fn a_read_answered_live_is_shown_again_offline_with_when_it_was_saved() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);

    assert_eq!(name(&s.client.workspace_list().await.unwrap()), "Alpha");
    assert_eq!(
        s.client.showing_saved_since(),
        None,
        "a live answer is not a saved copy"
    );

    s.clock.set(5_000);
    s.tokens.go_offline(true);
    assert_eq!(name(&s.client.workspace_list().await.unwrap()), "Alpha");
    assert_eq!(s.client.showing_saved_since(), Some(1_000));

    // The next answer from the service takes the copy off the screen.
    s.tokens.go_offline(false);
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha Inc")),
    )
    .await;
    assert_eq!(name(&s.client.workspace_list().await.unwrap()), "Alpha Inc");
    assert_eq!(s.client.showing_saved_since(), None);
}

#[tokio::test]
async fn a_connection_that_cannot_be_made_is_answered_from_the_copy() {
    // ⚠️ Not a dropped MockServer: wiremock pools its servers, so the port of one
    // that was dropped still answers.
    let base = closed_port();
    let cache = Arc::new(MemoryReadCache::new());
    cache.save(
        &format!("en {base}{LIST}"),
        SavedRead {
            body: list("Alpha").to_string().into_bytes(),
            saved_at_ms: 1_000,
        },
    );
    let config = ApiConfig::new(common::app()).with_base_url(&base).unwrap();
    let client = ApiClient::new(config, Tokens::default())
        .unwrap()
        .with_saved_reads(cache, HandClock::at(2_000));

    assert_eq!(name(&client.workspace_list().await.unwrap()), "Alpha");
    assert_eq!(client.showing_saved_since(), Some(1_000));
}

/// A loopback address nothing listens on: bound, then let go.
fn closed_port() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

#[tokio::test]
async fn the_oldest_copy_on_screen_is_the_one_named() {
    let server = MockServer::start().await;
    let s = setup(&server);
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    s.client.workspace_list().await.unwrap();
    s.clock.set(2_000);
    answer(
        &server,
        "/api/district/overview",
        ResponseTemplate::new(200).set_body_json(json!({"success": true})),
    )
    .await;
    s.client.overview("ws-1").await.unwrap();

    s.tokens.go_offline(true);
    s.client.overview("ws-1").await.unwrap();
    assert_eq!(s.client.showing_saved_since(), Some(2_000));
    s.client.workspace_list().await.unwrap();
    assert_eq!(s.client.showing_saved_since(), Some(1_000));
}

#[tokio::test]
async fn nothing_saved_offline_is_the_offline_error() {
    let server = MockServer::start().await;
    let s = setup(&server);
    s.tokens.go_offline(true);

    assert_eq!(
        s.client.workspace_list().await.unwrap_err(),
        ApiError::TokenUnavailable(RetryReason::Offline)
    );
    assert_eq!(s.client.showing_saved_since(), None);
}

#[tokio::test]
async fn an_answer_from_the_service_is_never_covered_over() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);
    s.client.workspace_list().await.unwrap();

    answer(&server, LIST, ResponseTemplate::new(503)).await;

    assert!(matches!(
        s.client.workspace_list().await.unwrap_err(),
        ApiError::Server { status: 503, .. }
    ));
    assert_eq!(s.client.showing_saved_since(), None);
}

#[tokio::test]
async fn other_reads_and_every_write_keep_nothing() {
    let server = MockServer::start().await;
    let s = setup(&server);
    answer(
        &server,
        "/api/auth/native/devices",
        ResponseTemplate::new(200).set_body_json(json!({"devices": []})),
    )
    .await;
    let devices = || s.client.request(Endpoint::NativeDevices).send::<Value>();
    devices().await.unwrap();

    s.tokens.go_offline(true);
    assert_eq!(
        devices().await.unwrap_err(),
        ApiError::TokenUnavailable(RetryReason::Offline)
    );
    assert!(
        district_api::ALL_ENDPOINTS
            .iter()
            .filter(|spec| spec.method != district_api::HttpMethod::Get)
            .all(|spec| !is_saved_read(spec.id)),
        "no write is ever a saved read"
    );
}

#[tokio::test]
async fn a_copy_older_than_seven_days_or_from_the_future_is_not_shown() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);
    s.client.workspace_list().await.unwrap();
    s.tokens.go_offline(true);

    s.clock.set(1_000 + SAVED_COPY_MAX_AGE_MS);
    assert!(
        s.client.workspace_list().await.is_ok(),
        "seven days is still shown"
    );

    s.clock.set(1_000 + SAVED_COPY_MAX_AGE_MS + 1);
    assert!(
        s.client.workspace_list().await.is_err(),
        "a moment later it is not"
    );

    s.clock.set(999);
    assert!(
        s.client.workspace_list().await.is_err(),
        "the clock went back"
    );
}

#[tokio::test]
async fn a_copy_that_can_no_longer_be_shown_comes_off_the_screen() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);
    s.client.workspace_list().await.unwrap();
    s.tokens.go_offline(true);
    s.client.workspace_list().await.unwrap();
    assert_eq!(s.client.showing_saved_since(), Some(1_000));

    // Expired: the read is the offline error, and no copy is on screen.
    s.clock.set(1_001 + SAVED_COPY_MAX_AGE_MS);
    assert!(s.client.workspace_list().await.is_err());
    assert_eq!(s.client.showing_saved_since(), None);

    // Evicted by the cache itself: the same.
    s.clock.set(2_000);
    s.tokens.go_offline(false);
    s.client.workspace_list().await.unwrap();
    s.tokens.go_offline(true);
    s.client.workspace_list().await.unwrap();
    s.cache.clear();
    assert!(s.client.workspace_list().await.is_err());
    assert_eq!(s.client.showing_saved_since(), None);
}

#[tokio::test]
async fn a_refusal_from_the_service_after_reconnecting_takes_the_copy_off_the_screen() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);
    s.client.workspace_list().await.unwrap();
    s.tokens.go_offline(true);
    s.client.workspace_list().await.unwrap();
    assert_eq!(s.client.showing_saved_since(), Some(1_000));

    // Back online, and the service answers with a 5xx: it answered, so no saved
    // copy is on screen any more, and the error is not covered over.
    s.tokens.go_offline(false);
    answer(&server, LIST, ResponseTemplate::new(503)).await;
    assert!(matches!(
        s.client.workspace_list().await.unwrap_err(),
        ApiError::Server { status: 503, .. }
    ));
    assert_eq!(s.client.showing_saved_since(), None);
}

#[tokio::test]
async fn an_empty_or_oversized_body_is_not_kept() {
    let server = MockServer::start().await;
    let s = setup(&server);
    let raw = || s.client.request(Endpoint::WorkspaceList).send::<Value>();

    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_raw(Vec::new(), "application/json"),
    )
    .await;
    assert!(raw().await.is_err(), "an empty body is not JSON");

    let padding = "x".repeat(SAVED_COPY_MAX_BYTES);
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(json!({"success": true, "pad": padding})),
    )
    .await;
    raw().await.unwrap();

    s.tokens.go_offline(true);
    assert!(raw().await.is_err(), "nothing was kept");
}

#[tokio::test]
async fn clearing_forgets_every_copy_and_a_read_in_flight_keeps_nothing() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);
    s.client.workspace_list().await.unwrap();
    s.tokens.go_offline(true);
    s.client.workspace_list().await.unwrap();

    s.client.clear_saved_reads();
    assert_eq!(s.client.showing_saved_since(), None);
    assert!(s.client.workspace_list().await.is_err());

    // A read that answers after a sign-out started is not kept.
    s.tokens.go_offline(false);
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200)
            .set_body_json(list("Alpha"))
            .set_delay(Duration::from_millis(300)),
    )
    .await;
    let (read, ()) = tokio::join!(s.client.workspace_list(), async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        s.client.clear_saved_reads();
    });
    read.unwrap();
    s.tokens.go_offline(true);
    assert!(s.client.workspace_list().await.is_err());
}

#[tokio::test]
async fn the_service_ending_the_session_forgets_every_copy() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);
    s.client.workspace_list().await.unwrap();

    // On a saved read.
    s.tokens.signed_out.store(true, Ordering::SeqCst);
    assert_eq!(
        s.client.workspace_list().await.unwrap_err(),
        ApiError::Unauthorized(UnauthorizedReason::SignInRequired(ReauthReason::NoSession))
    );
    assert_eq!(s.cache.load(&format!("en {}{LIST}", server.uri())), None);

    // And on any other request.
    s.tokens.signed_out.store(false, Ordering::SeqCst);
    s.client.workspace_list().await.unwrap();
    s.tokens.signed_out.store(true, Ordering::SeqCst);
    assert!(
        s.client
            .request(Endpoint::NativeDevices)
            .send::<Value>()
            .await
            .is_err()
    );
    s.tokens.signed_out.store(false, Ordering::SeqCst);
    s.tokens.go_offline(true);
    assert!(s.client.workspace_list().await.is_err());
}

#[tokio::test]
async fn another_failure_on_another_request_keeps_the_copies() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);
    s.client.workspace_list().await.unwrap();

    answer(
        &server,
        "/api/auth/native/devices",
        ResponseTemplate::new(500),
    )
    .await;
    assert!(
        s.client
            .request(Endpoint::NativeDevices)
            .send::<Value>()
            .await
            .is_err()
    );

    s.tokens.go_offline(true);
    assert!(s.client.workspace_list().await.is_ok());
}

#[test]
fn a_copy_in_one_language_never_fills_the_other() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let server = runtime.block_on(MockServer::start());
    runtime.block_on(answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    ));
    let s = setup(&server);
    district_l10n::with_locale(Locale::En, || {
        runtime.block_on(s.client.workspace_list()).unwrap();
    });

    s.tokens.go_offline(true);
    district_l10n::with_locale(Locale::FrCa, || {
        assert!(runtime.block_on(s.client.workspace_list()).is_err());
    });
    district_l10n::with_locale(Locale::En, || {
        assert!(runtime.block_on(s.client.workspace_list()).is_ok());
    });
}

#[tokio::test]
async fn a_client_without_saved_reads_keeps_nothing() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let client = common::client(&server);
    client.workspace_list().await.unwrap();
    client.clear_saved_reads();
    assert_eq!(client.showing_saved_since(), None);

    // The same token source as every other test here, with no saved reads.
    let config = ApiConfig::new(common::app())
        .with_base_url(&server.uri())
        .unwrap();
    let plain = ApiClient::new(config, Tokens::default()).unwrap();
    assert_eq!(name(&plain.workspace_list().await.unwrap()), "Alpha");
    assert_eq!(plain.showing_saved_since(), None);

    let unreachable = common::client_for(&closed_port());
    assert!(matches!(
        unreachable.workspace_list().await.unwrap_err(),
        ApiError::Offline(_)
    ));
    assert_eq!(unreachable.showing_saved_since(), None);
}

#[test]
fn a_shared_cache_is_one_cache() {
    let cache = Arc::new(MemoryReadCache::new());
    let read = SavedRead {
        body: b"{}".to_vec(),
        saved_at_ms: 7,
    };
    ReadCache::save(&cache, "k", read.clone());
    assert_eq!(cache.load("k"), Some(read));
    ReadCache::clear(&cache);
    assert_eq!(ReadCache::load(&cache, "k"), None);
}
