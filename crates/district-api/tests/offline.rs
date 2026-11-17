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

/// The account the tests' saved reads belong to.
const USER_A: &str = "user-a";

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
    client.own_saved_reads(USER_A);
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
    // Saved by an earlier launch, owner and all: this launch, offline, names no
    // owner of its own and still shows them.
    let cache = Arc::new(MemoryReadCache::new());
    cache.save(
        "owner",
        SavedRead {
            body: USER_A.as_bytes().to_vec(),
            saved_at_ms: 1_000,
        },
    );
    cache.save(
        &format!("{USER_A} en {base}{LIST}"),
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
async fn any_answer_from_the_service_takes_down_the_banner() {
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

    // Online again, and the next read is another one entirely: the machine is
    // online, so no copy is on screen as an offline one.
    s.tokens.go_offline(false);
    answer(
        &server,
        "/api/auth/native/devices",
        ResponseTemplate::new(200).set_body_json(json!({"devices": []})),
    )
    .await;
    s.client
        .request(Endpoint::NativeDevices)
        .send::<Value>()
        .await
        .unwrap();
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
    assert_eq!(
        s.cache.load(&format!("{USER_A} en {}{LIST}", server.uri())),
        None
    );

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
    client.own_saved_reads(USER_A);
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

/// A cache that signs out (clears the client's saved reads) in the middle of
/// writing a copy, as a sign-out on another task would.
struct SignsOutWhileWriting {
    inner: MemoryReadCache,
    client: Arc<std::sync::OnceLock<Arc<ApiClient<Tokens>>>>,
}

impl ReadCache for SignsOutWhileWriting {
    fn load(&self, key: &str) -> Option<SavedRead> {
        self.inner.load(key)
    }
    fn save(&self, key: &str, read: SavedRead) {
        // The client's lock is free while a copy is written, so this does not
        // deadlock: a sign-out is never held up by a slow cache. (The owner
        // marker is written before the client is in the slot.)
        if let Some(client) = self.client.get() {
            client.clear_saved_reads();
        }
        self.inner.save(key, read);
    }
    fn remove(&self, key: &str) {
        self.inner.remove(key);
    }
    fn clear(&self) {
        self.inner.clear();
    }
}

#[tokio::test]
async fn a_sign_out_that_lands_while_a_copy_is_written_still_wins() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let slot = Arc::new(std::sync::OnceLock::new());
    let config = ApiConfig::new(common::app())
        .with_base_url(&server.uri())
        .unwrap();
    let tokens = Tokens::default();
    let client = Arc::new(
        ApiClient::new(config, tokens.clone())
            .unwrap()
            .with_saved_reads(
                SignsOutWhileWriting {
                    inner: MemoryReadCache::new(),
                    client: slot.clone(),
                },
                HandClock::at(1_000),
            ),
    );
    client.own_saved_reads(USER_A);
    assert!(slot.set(client.clone()).is_ok());

    assert_eq!(name(&client.workspace_list().await.unwrap()), "Alpha");

    tokens.go_offline(true);
    assert!(
        client.workspace_list().await.is_err(),
        "the copy written under the session that ended is gone"
    );
}

#[tokio::test]
async fn saved_reads_belong_to_one_account() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);
    s.client.workspace_list().await.unwrap();

    // The same account again (every restore names it) keeps its copies.
    s.client.own_saved_reads(USER_A);
    s.tokens.go_offline(true);
    assert!(s.client.workspace_list().await.is_ok());

    // ⛔ Another account forgets them, and is shown none of them.
    s.client.own_saved_reads("user-b");
    assert!(s.client.workspace_list().await.is_err());
    assert_eq!(s.client.showing_saved_since(), None);
    assert_eq!(
        s.cache.load("owner").map(|read| read.body),
        Some(b"user-b".to_vec()),
        "the owner is remembered for the next launch"
    );
}

#[tokio::test]
async fn with_no_account_named_nothing_is_kept_or_shown() {
    let server = MockServer::start().await;
    answer(
        &server,
        LIST,
        ResponseTemplate::new(200).set_body_json(list("Alpha")),
    )
    .await;
    let s = setup(&server);
    // A sign-out forgets the owner with the copies: until a session names one
    // again, a read keeps nothing.
    s.client.clear_saved_reads();
    s.client.workspace_list().await.unwrap();
    s.tokens.go_offline(true);
    assert!(s.client.workspace_list().await.is_err());

    // A fresh cache, never owned: the same.
    let config = ApiConfig::new(common::app())
        .with_base_url(&server.uri())
        .unwrap();
    let tokens = Tokens::default();
    let cache = Arc::new(MemoryReadCache::new());
    let unowned = ApiClient::new(config, tokens.clone())
        .unwrap()
        .with_saved_reads(cache.clone(), HandClock::at(1_000));
    unowned.workspace_list().await.unwrap();
    assert_eq!(cache.load("owner"), None);
    tokens.go_offline(true);
    assert!(unowned.workspace_list().await.is_err());
}

#[test]
fn an_owner_that_is_not_text_is_no_owner() {
    let cache = Arc::new(MemoryReadCache::new());
    cache.save(
        "owner",
        SavedRead {
            body: vec![0xff, 0xfe],
            saved_at_ms: 1,
        },
    );
    let config = ApiConfig::new(common::app())
        .with_base_url("http://127.0.0.1:9")
        .unwrap();
    let client = ApiClient::new(config, Tokens::default())
        .unwrap()
        .with_saved_reads(cache, HandClock::at(1));
    assert_eq!(client.showing_saved_since(), None);
}

#[test]
fn a_shared_cache_is_one_cache() {
    let cache = Arc::new(MemoryReadCache::new());
    let read = SavedRead {
        body: b"{}".to_vec(),
        saved_at_ms: 7,
    };
    ReadCache::save(&cache, "k", read.clone());
    assert_eq!(cache.load("k"), Some(read.clone()));
    ReadCache::remove(&cache, "k");
    assert_eq!(ReadCache::load(&cache, "k"), None);
    ReadCache::save(&cache, "k", read.clone());
    ReadCache::clear(&cache);
    assert_eq!(ReadCache::load(&cache, "k"), None);
}
