//! What leaves the machine in a report: the user's home directory never does,
//! and neither does the machine's name, a user or an address.

use std::borrow::Cow;

use sentry::protocol::Event;
use serde_json::Value;

/// What stands in for the user's name in a home directory path.
pub(crate) const USER: &str = "<user>";

/// The report as it is sent: the machine's name, the user and the request
/// (where an address would be) removed, and every home directory path in
/// every string of it, keys included, with the user's name replaced by
/// [`USER`]. The walk is over the whole event as JSON rather than a list of
/// fields, so a field added to the protocol, or one an integration fills, is
/// scrubbed too. An event that does not survive the round trip is dropped
/// rather than sent unscrubbed.
pub(crate) fn scrub_event(mut event: Event<'static>) -> Option<Event<'static>> {
    event.server_name = None;
    event.user = None;
    event.request = None;
    let mut value = serde_json::to_value(&event).ok()?;
    scrub_value(&mut value);
    serde_json::from_value(value).ok()
}

fn scrub_value(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Cow::Owned(clean) = scrub_home_paths(text) {
                *text = clean;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(scrub_value),
        Value::Object(map) => {
            for (key, mut item) in std::mem::take(map) {
                scrub_value(&mut item);
                map.insert(scrub_home_paths(&key).into_owned(), item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

/// `text` with the user's name in every home directory path replaced by
/// [`USER`]: `/home/<name>` (Linux), `/Users/<name>` (macOS) and
/// `<drive>:\Users\<name>` (Windows, either slash, any case). The name runs to
/// the next path separator or to a character no user name may hold; on the
/// Windows form a space does not end it (Windows names may hold one), on the
/// others it does. Borrowed when there was nothing to replace.
pub(crate) fn scrub_home_paths(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let mut clean = String::new();
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        at = match home_at(bytes, at) {
            None => at + 1,
            Some((start, windows)) => {
                let end = bytes[start..]
                    .iter()
                    .position(|&byte| ends_name(byte, windows))
                    .map_or(bytes.len(), |n| start + n);
                if end > start {
                    clean.push_str(&text[copied..start]);
                    clean.push_str(USER);
                    copied = end;
                }
                end
            }
        };
    }
    if copied == 0 {
        Cow::Borrowed(text)
    } else {
        clean.push_str(&text[copied..]);
        Cow::Owned(clean)
    }
}

/// Where the user's name starts if a home directory begins at `at`, and
/// whether it is the Windows form. The markers are ASCII, so a match never
/// splits a character.
fn home_at(bytes: &[u8], at: usize) -> Option<(usize, bool)> {
    let rest = &bytes[at..];
    if rest.starts_with(b"/home/") {
        return Some((at + 6, false));
    }
    let [open, u, s, e, r, s2, close, ..] = *rest else {
        return None;
    };
    let word = [u, s, e, r, s2];
    if !(is_separator(open) && is_separator(close) && word.eq_ignore_ascii_case(b"users")) {
        return None;
    }
    let drive = at >= 2 && bytes[at - 1] == b':' && bytes[at - 2].is_ascii_alphabetic();
    let windows = drive || open == b'\\' || close == b'\\';
    // Without a drive or a backslash it is macOS, whose folder is `Users`
    // exactly; a lower case `/users/` is more likely part of an address.
    (windows || &word == b"Users").then_some((at + 7, windows))
}

fn is_separator(byte: u8) -> bool {
    byte == b'/' || byte == b'\\'
}

/// Whether `byte` cannot be part of a user's name, so ends it. Windows refuses
/// `" / \ [ ] : ; | = , + * ? < >` in a name; Linux and macOS names hold no
/// space, quote or bracket either.
fn ends_name(byte: u8, windows: bool) -> bool {
    b"/\\\"[]:;|=,+*?<>".contains(&byte)
        || byte.is_ascii_control()
        || (!windows && b" '`()".contains(&byte))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use sentry::protocol::{Exception, Frame, IpAddress, Request, Stacktrace, User};

    use super::*;

    fn scrubbed(text: &str) -> String {
        scrub_home_paths(text).into_owned()
    }

    #[test]
    fn linux_macos_and_windows_homes_lose_the_name() {
        assert_eq!(
            scrubbed("/home/alice/src/main.rs"),
            "/home/<user>/src/main.rs"
        );
        assert_eq!(
            scrubbed("/Users/alice/Library/x"),
            "/Users/<user>/Library/x"
        );
        assert_eq!(
            scrubbed(r"C:\Users\alice\AppData\x.dll"),
            r"C:\Users\<user>\AppData\x.dll"
        );
        assert_eq!(scrubbed("d:/users/alice/x"), "d:/users/<user>/x");
        assert_eq!(
            scrubbed(r"\\?\C:\USERS\Alice Smith\x"),
            r"\\?\C:\USERS\<user>\x"
        );
        assert_eq!(scrubbed(r"/Users\alice\x"), r"/Users\<user>\x");
    }

    #[test]
    fn every_home_in_a_sentence_and_one_at_the_very_end() {
        assert_eq!(
            scrubbed("cannot open /home/bob/a: no such file, nor /Users/carol"),
            "cannot open /home/<user>/a: no such file, nor /Users/<user>"
        );
        assert_eq!(
            scrubbed("'/home/bob' (from /home/eve)"),
            "'/home/<user>' (from /home/<user>)"
        );
        assert_eq!(
            scrubbed(r#"at "C:\Users\O'Brien (work)\x""#),
            r#"at "C:\Users\<user>\x""#
        );
        assert_eq!(scrubbed("a\t/home/bob\tb"), "a\t/home/<user>\tb");
    }

    #[test]
    fn a_name_keeps_characters_other_than_ascii() {
        assert_eq!(scrubbed("/home/zoë/é"), "/home/<user>/é");
    }

    #[test]
    fn text_without_a_home_is_borrowed_unchanged() {
        for text in [
            "",
            "/",
            "/home/",
            "/home//x",
            "/usr/lib/x.so",
            "https://example.com/api/users/42",
            "/users/x",
            "C:\\Users",
            "C:\\Program Files\\x",
            "xUsersx",
            "Users/home",
        ] {
            assert!(
                matches!(scrub_home_paths(text), Cow::Borrowed(t) if t == text),
                "{text:?}"
            );
        }
    }

    fn event() -> Event<'static> {
        Event {
            message: Some("open /home/alice/notes.txt".into()),
            exception: vec![Exception {
                ty: "panic".into(),
                value: Some(r"read C:\Users\alice\x".into()),
                stacktrace: Some(Stacktrace {
                    frames: vec![Frame {
                        filename: Some("/Users/alice/src/lib.rs".into()),
                        abs_path: Some("/home/alice/src/lib.rs".into()),
                        function: Some("district::run".into()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            }]
            .into(),
            timestamp: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            ..Default::default()
        }
    }

    #[test]
    fn an_event_loses_its_homes_machine_user_and_request() {
        let mut raw = event();
        raw.server_name = Some("alices-laptop".into());
        raw.user = Some(User {
            ip_address: Some(IpAddress::Auto),
            ..Default::default()
        });
        raw.request = Some(Request::default());
        raw.extra
            .insert("/home/alice/key".into(), "/home/alice/value".into());
        raw.tags.insert("list".into(), "/home/alice".into());
        let clean = scrub_event(raw).expect("the event survives");

        assert_eq!(clean.server_name, None);
        assert_eq!(clean.user, None);
        assert_eq!(clean.request, None);
        assert_eq!(
            clean.message.as_deref(),
            Some("open /home/<user>/notes.txt")
        );
        let exception = &clean.exception.values[0];
        assert_eq!(exception.value.as_deref(), Some(r"read C:\Users\<user>\x"));
        let frame = &exception.stacktrace.as_ref().unwrap().frames[0];
        assert_eq!(frame.filename.as_deref(), Some("/Users/<user>/src/lib.rs"));
        assert_eq!(frame.abs_path.as_deref(), Some("/home/<user>/src/lib.rs"));
        assert_eq!(frame.function.as_deref(), Some("district::run"));
        assert_eq!(
            clean.extra.get("/home/<user>/key"),
            Some(&Value::from("/home/<user>/value"))
        );
        assert_eq!(
            clean.tags.get("list").map(String::as_str),
            Some("/home/<user>")
        );
    }

    #[test]
    fn an_event_with_nothing_to_scrub_comes_back_as_it_was() {
        let mut raw = event();
        raw.message = Some("nothing here".into());
        raw.exception = Default::default();
        raw.extra.insert("n".into(), Value::from(1));
        raw.extra.insert("flag".into(), Value::from(true));
        raw.extra.insert("none".into(), Value::Null);
        raw.extra.insert("list".into(), Value::from(vec!["a", "b"]));
        assert_eq!(scrub_event(raw.clone()), Some(raw));
    }
}
