//! Works offline, read-only: the last answer of the reads the first screens make,
//! shown again when the service cannot be reached.
//!
//! Five reads are kept ([`is_saved_read`]): the workspace list, the overview, the
//! inbox's conversations, the calls and the contacts. Nothing else is, and never a
//! write. A request always goes to the network first; the saved copy answers only
//! when the request could not reach the service ([`ApiError::Offline`], or no token
//! because the refresh could not leave the machine). An answer from the service,
//! a 4xx or a 5xx, is never covered over.
//!
//! The bodies are customer data. The app chooses where they live through
//! [`ReadCache`]; a copy older than [`SAVED_COPY_MAX_AGE_MS`] is never shown, a
//! body over [`SAVED_COPY_MAX_BYTES`] is never kept, and the language is part of
//! the key, so a French answer never fills an English screen. A sign-out clears
//! them ([`ApiClient::clear_saved_reads`](crate::ApiClient::clear_saved_reads)), and
//! so does the service ending the session.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::clock::Clock;
use crate::endpoints::Endpoint;
use crate::error::{ApiError, UnauthorizedReason};
use crate::token::RetryReason;
use url::Url;

/// Seven days: a copy older than this is not shown.
pub const SAVED_COPY_MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// Two megabytes: a body larger than this is not kept.
pub const SAVED_COPY_MAX_BYTES: usize = 2 * 1024 * 1024;

/// One read's last answer: the body as the service sent it, and when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedRead {
    /// The JSON body, byte for byte.
    pub body: Vec<u8>,
    /// When it was answered, in Unix epoch milliseconds.
    pub saved_at_ms: i64,
}

/// Where the saved reads are kept. The app implements it (a cache directory that is
/// never backed up, with names that do not reveal the request), or uses
/// [`MemoryReadCache`].
///
/// A key is the request's language and URL. It names a workspace id and the
/// query, so an implementation that writes files hashes it rather than using it
/// as a file name.
pub trait ReadCache: Send + Sync + 'static {
    /// The saved answer for `key`, if there is one.
    fn load(&self, key: &str) -> Option<SavedRead>;
    /// Keeps `read` as the answer for `key`, replacing any older one.
    fn save(&self, key: &str, read: SavedRead);
    /// Forgets the saved answer for `key`, if there is one.
    fn remove(&self, key: &str);
    /// Forgets every saved answer.
    fn clear(&self);
}

/// A [`ReadCache`] held in memory: saved reads last until the process exits.
#[derive(Debug, Default)]
pub struct MemoryReadCache {
    reads: Mutex<HashMap<String, SavedRead>>,
}

impl MemoryReadCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn reads(&self) -> MutexGuard<'_, HashMap<String, SavedRead>> {
        // Every change is one map call, so a panic cannot leave the map half-changed.
        self.reads.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl ReadCache for MemoryReadCache {
    fn load(&self, key: &str) -> Option<SavedRead> {
        self.reads().get(key).cloned()
    }

    fn save(&self, key: &str, read: SavedRead) {
        self.reads().insert(key.to_owned(), read);
    }

    fn remove(&self, key: &str) {
        self.reads().remove(key);
    }

    fn clear(&self) {
        self.reads().clear();
    }
}

/// A cache the app keeps a handle on as well, to look at or clear it itself.
impl<C: ReadCache> ReadCache for Arc<C> {
    fn load(&self, key: &str) -> Option<SavedRead> {
        C::load(self, key)
    }

    fn save(&self, key: &str, read: SavedRead) {
        C::save(self, key, read);
    }

    fn remove(&self, key: &str) {
        C::remove(self, key);
    }

    fn clear(&self) {
        C::clear(self);
    }
}

/// Whether `endpoint` is one of the five reads whose last answer is kept.
pub fn is_saved_read(endpoint: Endpoint) -> bool {
    matches!(
        endpoint,
        Endpoint::WorkspaceList
            | Endpoint::Overview
            | Endpoint::Conversations
            | Endpoint::Calls
            | Endpoint::Contacts
    )
}

/// The request never reached the service: the saved copy may answer it.
fn unreachable(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::Offline(_) | ApiError::TokenUnavailable(RetryReason::Offline)
    )
}

/// The session is over: nothing saved under it may be shown again.
fn session_over(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::Unauthorized(
            UnauthorizedReason::SignInRequired(_) | UnauthorizedReason::SessionEnded
        )
    )
}

/// A saved read's request, as it was before it was sent.
pub(crate) struct Asked {
    key: String,
    generation: u64,
    /// When the request was sent: the time its copy is stamped with.
    asked_at_ms: i64,
    /// The order requests were sent in, which the wall clock is not (it can be
    /// set back).
    sequence: u64,
}

/// For a saved read, its key and generation, taken before the request is sent;
/// `None` without saved reads or for any other request.
pub(crate) fn asking(saved: Option<&SavedReads>, endpoint: Endpoint, url: &Url) -> Option<Asked> {
    saved?.asking(endpoint, url)
}

/// What the caller is told: `result`, or with saved reads, what
/// [`SavedReads::answer`] makes of it.
pub(crate) fn answer(
    saved: Option<&SavedReads>,
    asked: Option<Asked>,
    result: Result<Vec<u8>, ApiError>,
) -> Result<Vec<u8>, ApiError> {
    match saved {
        Some(saved) => saved.answer(asked, result),
        None => result,
    }
}

/// Forgets every saved read, if there are any.
pub(crate) fn clear(saved: Option<&SavedReads>) {
    if let Some(saved) = saved {
        saved.clear();
    }
}

/// Makes `user_id` the owner of the saved reads, if there are any.
pub(crate) fn own(saved: Option<&SavedReads>, user_id: &str) {
    if let Some(saved) = saved {
        saved.own(user_id);
    }
}

/// The cache entry that names whose saved reads these are. Not a read's key:
/// those always hold spaces. ⚠️ A [`ReadCache`] that prunes by age or count
/// leaves this entry alone (it is as old as the session, not a read); only
/// [`ReadCache::clear`] removes it.
pub const OWNER_KEY: &str = "owner";

/// The client's saved reads, and which of them are on screen.
pub(crate) struct SavedReads {
    cache: Box<dyn ReadCache>,
    clock: Box<dyn Clock>,
    state: Mutex<State>,
}

struct State {
    /// Moved on by every clear, so an answer to a request sent before a sign-out
    /// is not kept after it.
    generation: u64,
    /// The saved copies the last answers were, by key, with when each was saved.
    shown: HashMap<String, i64>,
    /// The sequence of the request whose answer was last kept, by key. An answer
    /// to an older request (two in flight, answering out of order) is not kept
    /// over it.
    kept: HashMap<String, u64>,
    /// The next request's sequence.
    next_sequence: u64,
    /// The account the saved reads belong to, or `None` before a session is
    /// known. ⛔ EVERY KEY NAMES IT, and with none nothing is kept or shown, so
    /// one account can never be shown another's saved reads, even if a sign-out
    /// that should have cleared them did not run.
    owner: Option<String>,
}

impl SavedReads {
    pub(crate) fn new(cache: impl ReadCache, clock: impl Clock) -> Self {
        // ⚠️ Read once, here: the owner of a cache saved before this launch, so a
        // launch offline (whose session cannot be refreshed, and so cannot say
        // whose it is) still shows that account's copies.
        let owner = cache
            .load(OWNER_KEY)
            .and_then(|read| String::from_utf8(read.body).ok());
        Self {
            cache: Box::new(cache),
            clock: Box::new(clock),
            state: Mutex::new(State {
                generation: 0,
                shown: HashMap::new(),
                kept: HashMap::new(),
                next_sequence: 0,
                owner,
            }),
        }
    }

    /// A session for `user_id` is in use: the saved reads are now that
    /// account's. Another account's are forgotten first.
    pub(crate) fn own(&self, user_id: &str) {
        let changed = {
            let mut state = self.state();
            let changed = state.owner.as_deref() != Some(user_id);
            if changed {
                state.generation += 1;
                state.shown.clear();
                state.kept.clear();
                state.owner = Some(user_id.to_owned());
            }
            changed
        };
        // Outside the lock, like every other use of the cache; see `answered`.
        if changed {
            self.cache.clear();
        }
        // Stamped now, so the marker is as fresh as the session that set it.
        self.cache.save(
            OWNER_KEY,
            SavedRead {
                body: user_id.as_bytes().to_vec(),
                saved_at_ms: self.clock.now_ms(),
            },
        );
    }

    // Every change below is made whole under the lock, so a poisoned lock still
    // holds a consistent state.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Taken before the request to `url` is sent: for a saved read, its key and
    /// the generation (see [`State::generation`]); `None` for anything else.
    pub(crate) fn asking(&self, endpoint: Endpoint, url: &Url) -> Option<Asked> {
        if !is_saved_read(endpoint) {
            return None;
        }
        let mut state = self.state();
        // No account, no saved reads: see `State::owner`.
        let key = format!(
            "{} {} {url}",
            state.owner.as_deref()?,
            district_l10n::current().tag()
        );
        state.next_sequence += 1;
        Some(Asked {
            key,
            generation: state.generation,
            asked_at_ms: self.clock.now_ms(),
            sequence: state.next_sequence,
        })
    }

    /// What the caller is told, given what the request `asked` got back.
    pub(crate) fn answer(
        &self,
        asked: Option<Asked>,
        result: Result<Vec<u8>, ApiError>,
    ) -> Result<Vec<u8>, ApiError> {
        // ⚠️ The service answered (any read, even with a refusal or a 5xx): the
        // machine is online, so no saved copy is on screen any more and the banner
        // and the withheld writes go, whichever reads the copies came from.
        if !result.as_ref().is_err_and(unreachable) {
            self.state().shown.clear();
        }
        match (result, asked) {
            (Ok(body), Some(asked)) => {
                self.answered(&asked, &body);
                Ok(body)
            }
            (Err(error), Some(asked)) if unreachable(&error) => self.saved(&asked.key).ok_or(error),
            (result, _) => {
                if result.as_ref().is_err_and(session_over) {
                    self.clear();
                }
                result
            }
        }
    }

    /// The service answered `asked` with `body`: keep it.
    fn answered(&self, asked: &Asked, body: &[u8]) {
        {
            let mut state = self.state();
            if state.generation != asked.generation
                || body.is_empty()
                || body.len() > SAVED_COPY_MAX_BYTES
                || state.kept.get(&asked.key) > Some(&asked.sequence)
            {
                return;
            }
            state.kept.insert(asked.key.clone(), asked.sequence);
        }
        // ⚠️ OUTSIDE THE LOCK: a cache that writes files (and flushes them) must not
        // hold up every other request's bookkeeping while it does. Stamped with when
        // the request was sent, so an answer never claims to be fresher than it is.
        self.cache.save(
            &asked.key,
            SavedRead {
                body: body.to_vec(),
                saved_at_ms: asked.asked_at_ms,
            },
        );
        // A clear that landed while it was being written still wins: what was just
        // written belonged to the session that ended. Only this copy goes: one a
        // newer session has saved since is its own.
        if self.state().generation != asked.generation {
            self.cache.remove(&asked.key);
        }
    }

    /// The saved answer for `key`, if one is young enough, now marked as shown.
    fn saved(&self, key: &str) -> Option<Vec<u8>> {
        let generation = self.state().generation;
        let now = self.clock.now_ms();
        // Read outside the lock, as a save is written. A copy from the future (the
        // clock was moved back) is not trusted either.
        let read = self
            .cache
            .load(key)
            .filter(|read| (0..=SAVED_COPY_MAX_AGE_MS).contains(&(now - read.saved_at_ms)));
        let mut state = self.state();
        match read {
            // Not a copy a clear has since forgotten.
            Some(read) if state.generation == generation => {
                state.shown.insert(key.to_owned(), read.saved_at_ms);
                Some(read.body)
            }
            // ⚠️ Nor is it on screen any more: the read is now the offline error, so a
            // copy it showed before (since expired, or evicted) must not keep the
            // banner up and the writes withheld.
            _ => {
                state.shown.remove(key);
                None
            }
        }
    }

    /// When the oldest saved copy on screen was saved, or `None` when every read
    /// answered live.
    pub(crate) fn showing_since(&self) -> Option<i64> {
        self.state().shown.values().copied().min()
    }

    pub(crate) fn clear(&self) {
        {
            let mut state = self.state();
            state.generation += 1;
            state.shown.clear();
            state.kept.clear();
            state.owner = None;
        }
        // Outside the lock, like every other use of the cache; see `answered`.
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_five_first_reads_are_kept() {
        let kept: Vec<Endpoint> = crate::ALL_ENDPOINTS
            .iter()
            .map(|spec| spec.id)
            .filter(|id| is_saved_read(*id))
            .collect();
        assert_eq!(
            kept,
            vec![
                Endpoint::WorkspaceList,
                Endpoint::Overview,
                Endpoint::Calls,
                Endpoint::Contacts,
                Endpoint::Conversations,
            ]
        );
        assert!(
            crate::ALL_ENDPOINTS
                .iter()
                .filter(|spec| is_saved_read(spec.id))
                .all(|spec| spec.method == crate::HttpMethod::Get),
            "a saved read is a GET"
        );
    }

    /// A cache whose save waits, mid-write, for the test to say go.
    struct HeldCache {
        inner: MemoryReadCache,
        writing: std::sync::Barrier,
        go_on: std::sync::Barrier,
    }

    /// The test keeps one handle, the reads the other. A wrapper rather than
    /// `Arc<HeldCache>`, so the test adds no copy of the generic `Arc` impl.
    struct Held(Arc<HeldCache>);

    impl ReadCache for Held {
        fn load(&self, key: &str) -> Option<SavedRead> {
            self.0.inner.load(key)
        }
        fn save(&self, key: &str, read: SavedRead) {
            // Only a read's write is held; the owner marker goes straight in.
            if key != OWNER_KEY {
                self.0.writing.wait();
                self.0.go_on.wait();
            }
            self.0.inner.save(key, read);
        }
        fn remove(&self, key: &str) {
            self.0.inner.remove(key);
        }
        fn clear(&self) {
            self.0.inner.clear();
        }
    }

    #[test]
    fn a_clear_that_lands_while_a_copy_is_written_still_wins() {
        let cache = Arc::new(HeldCache {
            inner: MemoryReadCache::new(),
            writing: std::sync::Barrier::new(2),
            go_on: std::sync::Barrier::new(2),
        });
        let saved = Arc::new(SavedReads::new(Held(cache.clone()), crate::SystemClock));
        saved.own("user-a");
        let asked = saved
            .asking(
                Endpoint::Overview,
                &Url::parse("https://example.test/k").unwrap(),
            )
            .unwrap();
        let key = asked.key.clone();
        let writer = {
            let saved = saved.clone();
            std::thread::spawn(move || saved.answered(&asked, b"{}"))
        };

        cache.writing.wait();
        // The lock is free while the copy is written: a sign-out is not held up.
        saved.clear();
        // And the next session saves a copy of its own meanwhile.
        cache.inner.save(
            "the next session's",
            SavedRead {
                body: b"{}".to_vec(),
                saved_at_ms: 1,
            },
        );
        cache.go_on.wait();
        writer.join().unwrap();

        assert!(
            cache.inner.load("the next session's").is_some(),
            "only the copy the ended session wrote goes"
        );

        assert_eq!(
            Held(cache).load(&key),
            None,
            "the session that ended keeps nothing"
        );
    }

    /// A clock the test sets.
    struct At(Arc<std::sync::atomic::AtomicI64>);

    impl crate::Clock for At {
        fn now_ms(&self) -> i64 {
            self.0.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[test]
    fn an_older_request_answering_last_does_not_replace_the_newer_copy() {
        let cache = Arc::new(MemoryReadCache::new());
        let now = Arc::new(std::sync::atomic::AtomicI64::new(1_000));
        let saved = SavedReads::new(cache.clone(), At(now.clone()));
        saved.own("user-a");
        let url = Url::parse("https://example.test/overview").unwrap();
        let older = saved.asking(Endpoint::Overview, &url).unwrap();
        now.store(2_000, std::sync::atomic::Ordering::SeqCst);
        let newer = saved.asking(Endpoint::Overview, &url).unwrap();
        let key = newer.key.clone();
        now.store(3_000, std::sync::atomic::Ordering::SeqCst);

        saved.answered(&newer, b"{\"n\":2}");
        saved.answered(&older, b"{\"n\":1}");

        assert_eq!(
            cache.load(&key),
            Some(SavedRead {
                body: b"{\"n\":2}".to_vec(),
                saved_at_ms: 2_000,
            }),
            "the newer answer, stamped with when it was asked"
        );
    }

    #[test]
    fn a_clock_set_back_does_not_make_a_newer_answer_look_older() {
        let cache = Arc::new(MemoryReadCache::new());
        let now = Arc::new(std::sync::atomic::AtomicI64::new(5_000));
        let saved = SavedReads::new(cache.clone(), At(now.clone()));
        saved.own("user-a");
        let url = Url::parse("https://example.test/overview").unwrap();
        let first = saved.asking(Endpoint::Overview, &url).unwrap();
        let key = first.key.clone();
        saved.answered(&first, b"{\"n\":1}");

        // The clock goes back an hour, and the next read is answered.
        now.store(1_000, std::sync::atomic::Ordering::SeqCst);
        let later = saved.asking(Endpoint::Overview, &url).unwrap();
        saved.answered(&later, b"{\"n\":2}");

        assert_eq!(
            cache.load(&key).map(|read| read.body),
            Some(b"{\"n\":2}".to_vec())
        );
    }

    #[test]
    fn the_memory_cache_keeps_replaces_and_forgets() {
        let cache = MemoryReadCache::new();
        let read = |at| SavedRead {
            body: b"{}".to_vec(),
            saved_at_ms: at,
        };
        assert_eq!(cache.load("k"), None);
        cache.save("k", read(1));
        cache.save("k", read(2));
        assert_eq!(cache.load("k"), Some(read(2)));
        cache.save("j", read(3));
        cache.remove("k");
        assert_eq!(cache.load("k"), None);
        assert_eq!(cache.load("j"), Some(read(3)), "only that one");
        cache.clear();
        assert_eq!(cache.load("j"), None);
    }
}
