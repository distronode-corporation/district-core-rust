// SPDX-FileCopyrightText: Distronode Corporation
//
// SPDX-License-Identifier: Apache-2.0

//! The saved reads as files, in temporary directories.

use std::fs;
use std::path::Path;

use district_api::offline::SAVED_COPY_MAX_AGE_MS;
use district_api::{OWNER_KEY, ReadCache, SavedRead};
use district_host::{READ_CACHE_MAX_FILES, ReadCacheDir};

const KEY: &str = "en https://www.distronode.com/api/district/overview?workspaceId=ws-secret";

fn read(body: &str, saved_at_ms: i64) -> SavedRead {
    SavedRead {
        body: body.as_bytes().to_vec(),
        saved_at_ms,
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn a_saved_read_comes_back_as_it_was_saved() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path().join("reads"));
    assert_eq!(cache.dir(), tmp.path().join("reads"));
    assert_eq!(cache.load(KEY), None, "nothing saved, and no directory yet");

    cache.save(KEY, read("{\"a\":\n1}", 42));
    assert_eq!(cache.load(KEY), Some(read("{\"a\":\n1}", 42)));

    cache.save(KEY, read("{}", 43));
    assert_eq!(
        cache.load(KEY),
        Some(read("{}", 43)),
        "a later save replaces it"
    );
    assert_eq!(names(cache.dir()).len(), 1);

    cache.save("other", read("{}", 44));
    cache.remove(KEY);
    assert_eq!(cache.load(KEY), None);
    assert!(cache.load("other").is_some(), "only that one");
    cache.remove("never saved");
}

#[test]
fn a_file_name_reveals_nothing_of_the_request() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path());
    cache.save(KEY, read("{}", 1));

    let name = names(tmp.path()).remove(0);
    assert_eq!(name.len(), 64);
    assert!(name.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
    assert!(!name.contains("ws-secret"));
}

#[cfg(unix)]
#[test]
fn only_the_owner_can_read_them() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("reads");
    let cache = ReadCacheDir::new(&dir);
    cache.save(KEY, read("{}", 1));

    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    let file = dir.join(names(&dir).remove(0));
    assert_eq!(mode(&file), 0o600);
}

#[test]
fn a_damaged_file_reads_as_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path());
    cache.save(KEY, read("{}", 1));
    let file = tmp.path().join(names(tmp.path()).remove(0));

    fs::write(&file, b"no newline").unwrap();
    assert_eq!(cache.load(KEY), None);
    fs::write(&file, b"yesterday\n{}").unwrap();
    assert_eq!(cache.load(KEY), None);
    fs::write(&file, b"\xff\n{}").unwrap();
    assert_eq!(cache.load(KEY), None);
}

#[test]
fn a_save_removes_copies_too_old_to_show_and_damaged_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path());
    cache.save("damaged", read("{}", 1));
    let damaged = names(tmp.path()).remove(0);
    fs::write(tmp.path().join(&damaged), b"not a saved read").unwrap();
    cache.save("old", read("{}", 0));

    cache.save(KEY, read("{}", SAVED_COPY_MAX_AGE_MS + 1));

    assert_eq!(cache.load("old"), None);
    assert!(!tmp.path().join(&damaged).exists());
    assert_eq!(names(tmp.path()).len(), 1, "only the fresh copy is left");
    assert!(cache.load(KEY).is_some());
}

#[test]
fn a_save_removes_a_copy_dated_far_in_the_future_and_keeps_a_newer_one() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path());
    let now = 10 * SAVED_COPY_MAX_AGE_MS;
    cache.save("far ahead", read("{}", now + SAVED_COPY_MAX_AGE_MS + 1));
    cache.save("a moment later", read("{}", now + 5));

    cache.save(KEY, read("{}", now));

    assert_eq!(cache.load("far ahead"), None, "the clock was set back");
    assert!(
        cache.load("a moment later").is_some(),
        "an overlapping save"
    );
    assert!(cache.load(KEY).is_some());
}

#[test]
fn only_its_own_files_are_touched() {
    // A stray file, a name in capitals and a directory: neither a prune nor a
    // clear touches them. A temporary file of a write in progress survives a
    // prune; a clear (a sign-out) removes it with the rest.
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path());
    let hex = "a".repeat(64);
    let in_progress = format!(".{hex}.0123.tmp");
    // ⚠️ "C", not "A": Windows' file names ignore case, so a capital "A" name
    // would be the `hex` directory below.
    let others = vec!["C".repeat(64), "stray".to_owned(), format!(".{hex}")];
    for other in others.iter().chain([&in_progress]) {
        fs::write(tmp.path().join(other), b"").unwrap();
    }
    fs::create_dir(tmp.path().join(&hex)).unwrap();

    cache.save(KEY, read("{}", SAVED_COPY_MAX_AGE_MS + 10));
    assert!(
        tmp.path().join(&in_progress).exists(),
        "a write in progress"
    );

    cache.clear();
    let mut expected = others.clone();
    expected.push(hex);
    expected.sort();
    assert_eq!(names(tmp.path()), expected);
}

#[test]
fn a_temporary_file_a_crash_left_behind_is_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path());
    let left = tmp.path().join(format!(".{}.0123.tmp", "b".repeat(64)));
    fs::write(&left, b"half a body").unwrap();
    let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    fs::File::options()
        .write(true)
        .open(&left)
        .unwrap()
        .set_modified(an_hour_ago)
        .unwrap();

    cache.save(KEY, read("{}", 1));

    assert!(!left.exists());
}

#[test]
fn the_owner_marker_outlives_pruning_and_only_a_clear_removes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path());
    cache.save(OWNER_KEY, read("user-a", 0));

    // Older than any copy may be, and pushed past the count by newer reads.
    let count = i64::try_from(READ_CACHE_MAX_FILES).unwrap();
    for at in 0..=count {
        cache.save(
            &format!("key {at}"),
            read("{}", SAVED_COPY_MAX_AGE_MS + 1_000 + at),
        );
    }

    assert_eq!(
        cache.load(OWNER_KEY).map(|r| r.body),
        Some(b"user-a".to_vec())
    );
    cache.clear();
    assert_eq!(cache.load(OWNER_KEY), None);
}

#[test]
fn a_save_keeps_the_newest_sixty_four() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path());
    let count = i64::try_from(READ_CACHE_MAX_FILES).unwrap();
    for at in 0..=count {
        cache.save(&format!("key {at}"), read("{}", 1_000 + at));
    }

    assert_eq!(names(tmp.path()).len(), READ_CACHE_MAX_FILES);
    assert_eq!(cache.load("key 0"), None, "the oldest went");
    assert!(cache.load(&format!("key {count}")).is_some());
}

#[test]
fn a_cache_that_cannot_be_written_keeps_nothing_and_says_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join("a file");
    fs::write(&blocker, b"").unwrap();
    let cache = ReadCacheDir::new(blocker.join("reads"));

    cache.save(KEY, read("{}", 1));

    assert_eq!(cache.load(KEY), None);
}

#[test]
fn clear_removes_every_saved_read() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ReadCacheDir::new(tmp.path().join("reads"));
    cache.clear();

    cache.save(KEY, read("{}", 1));
    cache.save("other", read("{}", 2));
    cache.clear();

    assert_eq!(names(cache.dir()), Vec::<String>::new());
}
