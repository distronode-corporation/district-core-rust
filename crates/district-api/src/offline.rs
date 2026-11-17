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
}

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
}

impl SavedReads {
    pub(crate) fn new(cache: impl ReadCache, clock: impl Clock) -> Self {
        Self {
            cache: Box::new(cache),
            clock: Box::new(clock),
            state: Mutex::new(State {
                generation: 0,
                shown: HashMap::new(),
            }),
        }
    }

    // Every change below is made whole under the lock, so a poisoned lock still
    // holds a consistent state.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Taken before the request to `url` is sent: for a saved read, its key and
    /// the generation (see [`State::generation`]); `None` for anything else.
    pub(crate) fn asking(&self, endpoint: Endpoint, url: &Url) -> Option<Asked> {
        is_saved_read(endpoint).then(|| Asked {
            key: format!("{} {url}", district_l10n::current().tag()),
            generation: self.state().generation,
        })
    }

    /// What the caller is told, given what the request `asked` got back.
    pub(crate) fn answer(
        &self,
        asked: Option<Asked>,
        result: Result<Vec<u8>, ApiError>,
    ) -> Result<Vec<u8>, ApiError> {
        match (result, asked) {
            (Ok(body), Some(asked)) => {
                self.answered(&asked.key, &body, asked.generation);
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

    /// The service answered `key` with `body`: keep it, and it is no longer a
    /// saved copy on screen.
    fn answered(&self, key: &str, body: &[u8], generation: u64) {
        let mut state = self.state();
        state.shown.remove(key);
        // Under the lock, so a clear cannot land between the check and the save.
        if state.generation == generation && !body.is_empty() && body.len() <= SAVED_COPY_MAX_BYTES
        {
            self.cache.save(
                key,
                SavedRead {
                    body: body.to_vec(),
                    saved_at_ms: self.clock.now_ms(),
                },
            );
        }
    }

    /// The saved answer for `key`, if one is young enough, now marked as shown.
    fn saved(&self, key: &str) -> Option<Vec<u8>> {
        let mut state = self.state();
        let read = self.cache.load(key)?;
        let age = self.clock.now_ms() - read.saved_at_ms;
        // A copy from the future (the clock was moved back) is not trusted either.
        if !(0..=SAVED_COPY_MAX_AGE_MS).contains(&age) {
            return None;
        }
        state.shown.insert(key.to_owned(), read.saved_at_ms);
        Some(read.body)
    }

    /// When the oldest saved copy on screen was saved, or `None` when every read
    /// answered live.
    pub(crate) fn showing_since(&self) -> Option<i64> {
        self.state().shown.values().copied().min()
    }

    pub(crate) fn clear(&self) {
        let mut state = self.state();
        state.generation += 1;
        state.shown.clear();
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
        cache.clear();
        assert_eq!(cache.load("k"), None);
    }
}
