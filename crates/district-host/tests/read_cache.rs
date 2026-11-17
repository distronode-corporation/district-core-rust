//! The saved reads as files, in temporary directories.

use std::fs;
use std::path::Path;

use district_api::offline::SAVED_COPY_MAX_AGE_MS;
use district_api::{ReadCache, SavedRead};
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
    cache.save("old", read("{}", 0));
    fs::write(tmp.path().join("stray"), b"not a saved read").unwrap();
    fs::create_dir(tmp.path().join("subdirectory")).unwrap();

    cache.save(KEY, read("{}", SAVED_COPY_MAX_AGE_MS + 1));

    assert_eq!(cache.load("old"), None);
    assert!(!tmp.path().join("stray").exists());
    assert!(
        tmp.path().join("subdirectory").exists(),
        "only files are its own"
    );
    assert!(cache.load(KEY).is_some());
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
