//! The saved reads (`district_api::offline`) as files in a cache directory.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use district_api::offline::SAVED_COPY_MAX_AGE_MS;
use district_api::{ReadCache, SavedRead};
use sha2::{Digest, Sha256};

use crate::files;

/// At most this many saved reads are kept; a save past it removes the oldest.
pub const READ_CACHE_MAX_FILES: usize = 64;

/// The client's saved reads, one file each, in a directory of their own.
///
/// The bodies are customer data, so:
/// - the directory is the app's cache directory (on Linux, under
///   `XDG_CACHE_HOME`), which backups leave out;
/// - on Unix the directory is its owner's alone (0700) and each file readable by
///   its owner only (0600);
/// - a file's name is the SHA-256 of the request's key, so no workspace id, query
///   or language appears in a path;
/// - every save removes copies past the seven days they may be shown for, and
///   keeps the newest [`READ_CACHE_MAX_FILES`].
///
/// A file is the time it was saved (epoch milliseconds) on one line, then the
/// body. Saving is best effort: a cache that cannot be written leaves the app
/// online-only, which is what it was without one, so a failure is not reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadCacheDir {
    dir: PathBuf,
}

impl ReadCacheDir {
    /// Saved reads in `dir`, created when the first is saved. Give the cache its
    /// own directory: [`ReadCache::clear`] removes every file in it.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The directory the saved reads live in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every saved read's file, with when it was saved. A file that does not
    /// start with a time is listed as saved at the epoch, so the next save
    /// removes it.
    fn saved_files(&self) -> Vec<(PathBuf, i64)> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .map(|path| {
                let saved_at = fs::File::open(&path)
                    .ok()
                    .and_then(|file| {
                        let mut line = String::new();
                        BufReader::new(file).read_line(&mut line).ok()?;
                        line.trim_end().parse().ok()
                    })
                    .unwrap_or(0);
                (path, saved_at)
            })
            .collect()
    }

    /// Removes the copies too old to show at `now_ms`, then the oldest past
    /// [`READ_CACHE_MAX_FILES`].
    fn prune(&self, now_ms: i64) {
        let mut kept: Vec<(PathBuf, i64)> = self
            .saved_files()
            .into_iter()
            .filter(|(path, saved_at)| {
                let fresh = now_ms - saved_at <= SAVED_COPY_MAX_AGE_MS;
                if !fresh {
                    fs::remove_file(path).ok();
                }
                fresh
            })
            .collect();
        kept.sort_by_key(|(_, saved_at)| std::cmp::Reverse(*saved_at));
        for (path, _) in kept.iter().skip(READ_CACHE_MAX_FILES) {
            fs::remove_file(path).ok();
        }
    }
}

/// The file name for `key`: its SHA-256, as 64 hexadecimal digits.
fn file_name(key: &str) -> String {
    Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl ReadCache for ReadCacheDir {
    fn load(&self, key: &str) -> Option<SavedRead> {
        let contents = fs::read(self.dir.join(file_name(key))).ok()?;
        let newline = contents.iter().position(|byte| *byte == b'\n')?;
        let saved_at_ms = std::str::from_utf8(&contents[..newline])
            .ok()?
            .parse()
            .ok()?;
        Some(SavedRead {
            body: contents[newline + 1..].to_vec(),
            saved_at_ms,
        })
    }

    fn save(&self, key: &str, read: SavedRead) {
        let mut contents = format!("{}\n", read.saved_at_ms).into_bytes();
        contents.extend_from_slice(&read.body);
        if files::replace(&self.dir, &file_name(key), &contents).is_ok() {
            self.prune(read.saved_at_ms);
        }
    }

    fn clear(&self) {
        for (path, _) in self.saved_files() {
            fs::remove_file(path).ok();
        }
    }
}
