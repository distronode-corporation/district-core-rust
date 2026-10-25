//! What reaches the network, counted rather than reasoned about: the DSN names
//! a mock server on this machine, and each test reads what that server
//! received. Off sends nothing at all, a panic or a captured error with the
//! user's consent sends exactly one report, and that report holds no home
//! directory.
//!
//! The reporter is one per process, so the tests take turns (`serial`), and
//! each ends with reporting off.

use std::error::Error;
use std::fmt;
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use district_crash::{Consent, CrashError, OffReason, Settings, Status};
use object::Object;
use sentry::types::DebugId;
use serde_json::Value;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const CHILD: &str = "DISTRICT_CRASH_PROOF_CHILD";

fn serial() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A mock report server on this machine, its own runtime on its own thread.
struct Server {
    runtime: tokio::runtime::Runtime,
    server: MockServer,
}

impl Server {
    fn start() -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let server = runtime.block_on(async {
            // Not the pooled `MockServer::start`, so no request from another
            // test can be counted here.
            let server = MockServer::builder().start().await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200))
                .mount(&server)
                .await;
            server
        });
        Self { runtime, server }
    }

    fn dsn(&self) -> String {
        format!("http://publickey@{}/42", self.server.address())
    }

    fn settings(&self) -> Settings {
        Settings {
            dsn: Some(self.dsn()),
            release: "district-crash-proof@1.0.0+7".to_owned(),
            environment: "proof".to_owned(),
        }
    }

    /// Every request body received so far.
    fn bodies(&self) -> Vec<String> {
        self.runtime
            .block_on(self.server.received_requests())
            .expect("the server records requests")
            .into_iter()
            .map(|request| {
                assert_eq!(request.url.path(), "/api/42/envelope/");
                String::from_utf8(request.body).unwrap()
            })
            .collect()
    }

    /// Waits for `count` requests, then long enough that one more would show,
    /// and returns them all.
    fn settle(&self, count: usize) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.bodies().len() < count && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        thread::sleep(Duration::from_millis(500));
        self.bodies()
    }
}

/// The event in an envelope: its header line, then each item's header and
/// payload, every line JSON.
fn event(body: &str) -> Value {
    let lines: Vec<Value> = body
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).expect("every envelope line is JSON"))
        .collect();
    let at = lines
        .iter()
        .position(|line| line["type"] == "event")
        .expect("an event item");
    lines[at + 1].clone()
}

fn strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(map) => map.iter().for_each(|(key, item)| {
            out.push(key);
            strings(item, out);
        }),
        _ => {}
    }
}

/// Holds what every report must: no trace of the injected user `alice`, none
/// of the real home directory of whoever runs the tests (frames name the
/// source files, which are usually under it), no machine name, user or
/// request, and the release and environment the app passed.
fn assert_clean(event: &Value) {
    let mut all = Vec::new();
    strings(event, &mut all);
    assert!(
        all.iter().all(|text| !text.contains("alice")),
        "a user name was sent: {event:#}"
    );
    if let Some(home) = real_home() {
        assert!(
            all.iter().all(|text| !text.contains(&home)),
            "the home directory {home} was sent: {event:#}"
        );
    }
    for gone in ["server_name", "user", "request"] {
        assert!(event.get(gone).is_none(), "{gone} was sent: {event:#}");
    }
    assert_eq!(event["release"], "district-crash-proof@1.0.0+7");
    assert_eq!(event["environment"], "proof");
    assert!(
        event["contexts"]["os"].is_object(),
        "no os context: {event:#}"
    );
}

/// The home directory of whoever runs the tests, when it has the form the
/// scrubber knows (it does not on a machine whose home is elsewhere, such as
/// `/root`).
fn real_home() -> Option<String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let known = home.starts_with("/home/")
        || home.starts_with("/Users/")
        || home.to_ascii_lowercase().contains(":\\users\\");
    known.then_some(home)
}

/// The report's debug image for this test binary, found by the id the binary
/// itself carries: its GNU build id (ELF, which the image names as its code
/// id), or its PDB's id and age (PE, the image's debug id, `id`).
fn image_of_this_binary(event: &Value) -> &Value {
    let data = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    let file = object::File::parse(&*data).expect("the test binary is an object file");
    let (key, id) = match file.build_id().unwrap() {
        Some(build_id) => {
            let hex: String = build_id.iter().map(|byte| format!("{byte:02x}")).collect();
            ("code_id", hex)
        }
        None => {
            let pdb = file.pdb_info().unwrap().expect("a build id or a PDB id");
            let id = DebugId::from_guid_age(&pdb.guid(), pdb.age()).unwrap();
            ("id", id.to_string())
        }
    };
    event["debug_meta"]["images"]
        .as_array()
        .expect("debug images")
        .iter()
        .find(|image| image[key] == id.as_str())
        .unwrap_or_else(|| panic!("no image with {key} {id}: {event:#}"))
}

fn frames(exception: &Value) -> &Vec<Value> {
    exception["stacktrace"]["frames"]
        .as_array()
        .expect("a stack")
}

#[derive(Debug)]
struct Outer(Inner);

#[derive(Debug)]
struct Inner;

impl fmt::Display for Outer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(r"the settings at C:\Users\alice\AppData\Roaming\x.toml are unreadable")
    }
}

impl fmt::Display for Inner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("/home/alice/.config/x: permission denied")
    }
}

impl Error for Outer {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.0)
    }
}

impl Error for Inner {}

fn panic_on_a_thread(f: impl FnOnce() + Send + 'static) {
    assert!(thread::spawn(f).join().is_err(), "the thread panicked");
}

#[test]
fn consent_denied_sends_nothing() {
    let _turn = serial();
    let server = Server::start();

    let status = district_crash::apply(&server.settings(), Consent::Denied);

    assert_eq!(status, Ok(Status::Off(OffReason::NoConsent)));
    assert!(!district_crash::is_enabled());
    assert_eq!(district_crash::capture_error(&Outer(Inner)), None);
    panic_on_a_thread(|| panic!("cannot read /home/alice/secret.txt"));
    assert!(district_crash::flush(Duration::from_secs(1)));
    assert_eq!(server.settle(0), Vec::<String>::new(), "nothing is sent");
}

/// A `SENTRY_DSN` in the environment is not a DSN the app passed: the test
/// binary is run again with one naming the mock server, and with consent and
/// no DSN from the app, nothing reaches it.
#[test]
fn no_dsn_sends_nothing_even_with_one_in_the_environment() {
    let _turn = serial();
    let server = Server::start();

    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_granted_without_a_dsn", "--ignored"])
        .env(CHILD, "1")
        .env("SENTRY_DSN", server.dsn())
        .env("SENTRY_RELEASE", "from-the-environment")
        .output()
        .unwrap();

    assert!(
        child.status.success(),
        "the child failed: {}",
        String::from_utf8_lossy(&child.stdout)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
    assert_eq!(server.settle(0), Vec::<String>::new(), "nothing is sent");
}

#[test]
#[ignore = "run by no_dsn_sends_nothing_even_with_one_in_the_environment, in a process of its own"]
fn child_granted_without_a_dsn() {
    assert_eq!(
        std::env::var(CHILD).as_deref(),
        Ok("1"),
        "run by its parent"
    );
    let mut settings = Settings {
        dsn: None,
        release: "district-crash-proof@1.0.0+7".to_owned(),
        environment: "proof".to_owned(),
    };
    for dsn in [None, Some(String::new()), Some("   ".to_owned())] {
        settings.dsn = dsn;
        let status = district_crash::apply(&settings, Consent::Granted);
        assert_eq!(status, Ok(Status::Off(OffReason::NoDsn)));
        assert!(!district_crash::is_enabled());
    }
    assert_eq!(district_crash::capture_error(&Outer(Inner)), None);
    panic_on_a_thread(|| panic!("cannot read /home/alice/secret.txt"));
    assert!(district_crash::flush(Duration::from_secs(1)));
}

#[test]
fn consent_granted_sends_one_scrubbed_report_of_a_panic() {
    let _turn = serial();
    let server = Server::start();

    let status = district_crash::apply(&server.settings(), Consent::Granted);
    assert_eq!(status, Ok(Status::On));
    assert!(district_crash::is_enabled());
    let file = "/home/alice/secret.txt";
    panic_on_a_thread(move || panic!("cannot read {file}"));

    let bodies = server.settle(1);
    assert_eq!(bodies.len(), 1, "exactly one report");
    let event = event(&bodies[0]);
    assert_clean(&event);
    assert_eq!(event["level"], "fatal");
    let exception = &event["exception"]["values"][0];
    assert_eq!(exception["type"], "panic");
    assert_eq!(exception["value"], "cannot read /home/<user>/secret.txt");
    assert_eq!(exception["mechanism"]["handled"], false);
    assert!(
        frames(exception).iter().any(|frame| frame["function"]
            .as_str()
            .is_some_and(|f| f.contains("proof"))),
        "the stack reaches this test: {event:#}"
    );
    // This file's own frame names it by its path, under the home directory
    // when the checkout is, and so with the name replaced.
    if real_home().is_some_and(|home| env!("CARGO_MANIFEST_DIR").starts_with(&home)) {
        assert!(
            frames(exception).iter().any(|frame| frame["abs_path"]
                .as_str()
                .is_some_and(|path| path.contains("<user>") && path.ends_with("proof.rs"))),
            "this file's frame, scrubbed: {event:#}"
        );
    }
    // The debug image Sentry needs to symbolicate a stripped build: this test
    // binary's, by the id the binary itself carries, and no other library
    // than the main executable and the one holding district-crash's code
    // (both this binary here).
    let images = event["debug_meta"]["images"]
        .as_array()
        .expect("debug images");
    assert!(
        (1..=2).contains(&images.len()),
        "only the app's own images: {event:#}"
    );
    let image = image_of_this_binary(&event);
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    };
    assert_eq!(image["type"], "symbolic");
    assert_eq!(image["arch"], arch);
    // `id` is the debug id, which the service also reads as `debug_id`.
    assert!(image["id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(
        image["image_addr"]
            .as_str()
            .is_some_and(|a| a.starts_with("0x"))
    );
    assert!(image["image_size"].as_u64().is_some_and(|size| size > 0));
    let exe = std::env::current_exe().unwrap();
    let exe_name = exe.file_name().unwrap().to_string_lossy().into_owned();
    // `name` is the code file: what sentry-types writes, which the service
    // reads as `code_file`.
    let code_file = image["name"].as_str().expect("a code file");
    assert!(code_file.ends_with(&exe_name), "{code_file}");
    if real_home().is_some_and(|home| exe.starts_with(home)) {
        assert!(
            code_file.contains("<user>"),
            "the code file, scrubbed: {code_file}"
        );
    }

    district_crash::apply(&server.settings(), Consent::Denied).unwrap();
}

#[test]
fn consent_granted_sends_one_scrubbed_report_of_an_error() {
    let _turn = serial();
    let server = Server::start();

    district_crash::apply(&server.settings(), Consent::Granted).unwrap();
    let id = district_crash::capture_error(&Outer(Inner)).expect("reporting is on");
    assert!(district_crash::flush(Duration::from_secs(5)));

    let bodies = server.settle(1);
    assert_eq!(bodies.len(), 1, "exactly one report");
    let event = event(&bodies[0]);
    assert_clean(&event);
    assert_eq!(id.len(), 32);
    assert_eq!(
        event["event_id"].as_str().map(|e| e.replace('-', "")),
        Some(id)
    );
    // `error` is the protocol's default level, so it may be left out.
    assert!(matches!(event["level"].as_str(), None | Some("error")));
    let values = event["exception"]["values"].as_array().unwrap();
    assert_eq!(values.len(), 2, "the error and its source");
    assert_eq!(
        values[0]["value"],
        "/home/<user>/.config/x: permission denied"
    );
    assert_eq!(
        values[1]["value"],
        r"the settings at C:\Users\<user>\AppData\Roaming\x.toml are unreadable"
    );
    assert!(
        !frames(&values[1]).is_empty(),
        "the outermost error has the stack"
    );

    district_crash::apply(&server.settings(), Consent::Denied).unwrap();
}

/// On, off and on again, with a thread that was already running before the
/// user said yes: its panic is reported too, and nothing is while off.
#[test]
fn consent_turns_off_and_on_at_runtime() {
    let _turn = serial();
    let server = Server::start();
    let settings = server.settings();

    let (go, wait) = mpsc::channel::<()>();
    let early = thread::spawn(move || {
        wait.recv().unwrap();
        panic!("an early thread, at /Users/alice/x");
    });

    assert_eq!(
        district_crash::apply(&settings, Consent::Granted),
        Ok(Status::On)
    );
    go.send(()).unwrap();
    assert!(early.join().is_err());
    assert_eq!(server.settle(1).len(), 1, "the early thread's panic");

    assert_eq!(
        district_crash::apply(&settings, Consent::Denied),
        Ok(Status::Off(OffReason::NoConsent))
    );
    assert!(!district_crash::is_enabled());
    assert_eq!(district_crash::capture_error(&Inner), None);
    panic_on_a_thread(|| panic!("while off"));
    assert_eq!(server.settle(1).len(), 1, "nothing more while off");

    assert_eq!(
        district_crash::apply(&settings, Consent::Granted),
        Ok(Status::On)
    );
    panic_on_a_thread(|| std::panic::panic_any(7_u8));
    let bodies = server.settle(2);
    assert_eq!(bodies.len(), 2, "on again");
    let first = event(&bodies[0]);
    let second = event(&bodies[1]);
    assert_clean(&first);
    assert_clean(&second);
    assert_eq!(
        first["exception"]["values"][0]["value"],
        "an early thread, at /Users/<user>/x"
    );
    assert_eq!(second["exception"]["values"][0]["value"], "Box<dyn Any>");

    district_crash::apply(&settings, Consent::Denied).unwrap();
}

#[test]
fn an_unusable_dsn_is_refused_and_leaves_reporting_off() {
    let _turn = serial();
    let server = Server::start();
    let mut settings = server.settings();
    district_crash::apply(&settings, Consent::Granted).unwrap();

    for (dsn, error) in [
        ("not a dsn", CrashError::InvalidDsn),
        ("http://publickey@example.com/42", CrashError::InsecureDsn),
    ] {
        settings.dsn = Some(dsn.to_owned());
        assert_eq!(
            district_crash::apply(&settings, Consent::Granted),
            Err(error)
        );
        assert!(!district_crash::is_enabled());
        assert!(!error.to_string().is_empty());
    }
    // Without consent the DSN is not even read.
    assert_eq!(
        district_crash::apply(&settings, Consent::Denied),
        Ok(Status::Off(OffReason::NoConsent))
    );
    assert_eq!(server.settle(0), Vec::<String>::new(), "nothing is sent");
}
