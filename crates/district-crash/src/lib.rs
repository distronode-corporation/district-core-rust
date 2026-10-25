//! Crash reports for the District AI desktop apps, off until the user turns
//! them on.
//!
//! An app asks the user once, at its first start, whether to send crash
//! reports (the answer it offers first is not to), and keeps a switch in its
//! settings; both are the app's, and so are the words. This crate is the
//! mechanism the answer drives:
//!
//! - [`apply`] sets reporting up only when the app passes both a DSN and the
//!   user's [`Consent::Granted`]. Without either nothing is set up, no panic
//!   handler is installed and nothing is ever sent. Called again with
//!   [`Consent::Denied`] it closes the reporter, after sending what was already
//!   queued; with consent again it opens a new one.
//! - While it is on, a panic on any thread is reported before the panic goes
//!   on as it would have (the previous panic handler still runs), and
//!   [`capture_error`] reports an error the app chooses to.
//! - What is sent: the panic message or the error, the stack, the operating
//!   system and the Rust version, the release and environment the app names,
//!   and two debug images, the main executable and the image holding this
//!   crate's code (the app's own library on Windows), with their ids, load
//!   address, size and architecture, so the stack of a stripped build can be
//!   symbolicated against the debug files uploaded for the release. No other
//!   loaded library is listed. Every home directory path in it has the user's
//!   name replaced, and the machine's name, the user and any address are
//!   removed. Nothing is
//!   kept on disk, nothing is read from the environment (a `SENTRY_DSN` there
//!   is ignored), and nothing is sent at start up or on a clean quit.
//!
//! Transport is HTTPS with rustls and the operating system's trust store, the
//! same as the API client's; plain `http` is refused unless the DSN names this
//! machine.
//!
//! ```no_run
//! use district_crash::{Consent, Settings};
//!
//! let settings = Settings {
//!     dsn: Some("https://key@example.com/0".to_owned()),
//!     release: "com.distronode.DistrictAI@3.1.0+42".to_owned(),
//!     environment: "production".to_owned(),
//! };
//! // At start, with the answer the app stored (Denied until the user said yes):
//! district_crash::apply(&settings, Consent::Denied)?;
//! // When the user turns the switch on, and off again:
//! district_crash::apply(&settings, Consent::Granted)?;
//! district_crash::apply(&settings, Consent::Denied)?;
//! # Ok::<(), district_crash::CrashError>(())
//! ```

#![forbid(unsafe_code)]

mod images;
mod scrub;

use std::net::IpAddr;
use std::panic::{self, PanicHookInfo};
use std::sync::{Arc, Mutex, Once, PoisonError, RwLock};
use std::time::Duration;

use sentry::integrations::backtrace::{ProcessStacktraceIntegration, current_stacktrace};
use sentry::integrations::contexts::ContextIntegration;
use sentry::protocol::{Event, Exception, Level, Mechanism};
use sentry::transports::DefaultTransportFactory;
use sentry::types::{Dsn, Scheme};
use sentry::{Client, ClientOptions};

/// What the app knows about where reports go and what it is. The DSN is the
/// app's build configuration; without one nothing is ever set up.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// Where reports go. `None` or blank: reporting stays off whatever the
    /// consent.
    pub dsn: Option<String>,
    /// The app and its version as reports name it; the apps use
    /// `com.distronode.DistrictAI@<version>+<build>`.
    pub release: String,
    /// The environment reports are filed under, such as `production`.
    pub environment: String,
}

/// The user's answer. [`Consent::Denied`] is the default, and is what an app
/// passes until the user has said yes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Consent {
    /// The user has turned crash reports on.
    Granted,
    /// The user has not turned them on, or has turned them off.
    #[default]
    Denied,
}

/// Whether reports are being sent after [`apply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// On: a panic or a captured error is sent.
    On,
    /// Off, and why: nothing is set up and nothing is sent.
    Off(OffReason),
}

/// Why reporting is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffReason {
    /// The user has not consented.
    NoConsent,
    /// The app has no DSN.
    NoDsn,
}

/// A DSN [`apply`] will not use. Reporting is off after either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CrashError {
    /// The DSN is not one.
    #[error("the crash report DSN is not a valid DSN")]
    InvalidDsn,
    /// The DSN is plain `http` to another machine.
    #[error("the crash report DSN is plain http to another machine")]
    InsecureDsn,
}

/// The longest a panic waits for its report to be sent before it goes on.
const PANIC_FLUSH: Duration = Duration::from_secs(2);
/// The longest turning reporting off waits for what was queued.
const CLOSE_FLUSH: Duration = Duration::from_secs(2);

/// The reporter while one is on. Read by the panic handler on any thread, so
/// it is never held while anything is built or sent, and nothing that can
/// panic runs while it is.
static CLIENT: RwLock<Option<Arc<Client>>> = RwLock::new(None);
/// One [`apply`] at a time, so a close and the open after it are not
/// interleaved with another's.
static APPLYING: Mutex<()> = Mutex::new(());
static PANIC_HANDLER: Once = Once::new();

/// Turns reporting on or off for this process, and says which it is.
///
/// Whatever was on before is closed first, after the reports it had queued
/// are sent (waiting up to two seconds). Then, only with [`Consent::Granted`]
/// and a DSN, a new reporter is opened and, the first time, the panic handler
/// is installed. Call it at start with the stored answer, and again whenever
/// the user changes it.
///
/// # Errors
///
/// [`CrashError`] when consent is granted but the DSN is unusable; reporting
/// is then off.
pub fn apply(settings: &Settings, consent: Consent) -> Result<Status, CrashError> {
    let _one_at_a_time = APPLYING.lock().unwrap_or_else(PoisonError::into_inner);
    let previous = replace(None);
    if let Some(client) = previous {
        client.close(Some(CLOSE_FLUSH));
    }
    if consent == Consent::Denied {
        return Ok(Status::Off(OffReason::NoConsent));
    }
    let Some(dsn) = settings
        .dsn
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty())
    else {
        return Ok(Status::Off(OffReason::NoDsn));
    };
    let dsn: Dsn = dsn.parse().map_err(|_| CrashError::InvalidDsn)?;
    if dsn.scheme() == Scheme::Http && !is_loopback(dsn.host()) {
        return Err(CrashError::InsecureDsn);
    }
    let client = Arc::new(Client::from(options(dsn, settings)));
    PANIC_HANDLER.call_once(install_panic_handler);
    replace(Some(client));
    Ok(Status::On)
}

/// Whether reporting is on.
pub fn is_enabled() -> bool {
    current().is_some()
}

/// Reports `error`, with its sources and the current stack. Returns the
/// report's id, or `None` when reporting is off, in which case nothing is
/// sent.
pub fn capture_error<E: std::error::Error + ?Sized>(error: &E) -> Option<String> {
    let client = current()?;
    let mut event = sentry::event_from_error(error);
    if let Some(outermost) = event.exception.values.last_mut() {
        outermost.stacktrace = current_stacktrace();
    }
    let id = client.capture_event(event, None);
    Some(id.simple().to_string())
}

/// Waits up to `timeout` for queued reports to be sent; `true` when nothing is
/// left (and when reporting is off).
pub fn flush(timeout: Duration) -> bool {
    current().is_none_or(|client| client.flush(Some(timeout)))
}

fn current() -> Option<Arc<Client>> {
    CLIENT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

fn replace(client: Option<Arc<Client>>) -> Option<Arc<Client>> {
    let mut slot = CLIENT.write().unwrap_or_else(PoisonError::into_inner);
    std::mem::replace(&mut *slot, client)
}

fn is_loopback(host: &str) -> bool {
    host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// The reporter's options, built by hand rather than by `sentry::init`, which
/// would read a DSN, a release and a certificate check switch from the
/// environment, and whose default integrations list every library loaded
/// (here only the app's own images are, see [`images`]). No breadcrumbs (they come from logging, and nothing here logs), no
/// personal data, and [`scrub::scrub_event`] the last thing before sending.
fn options(dsn: Dsn, settings: &Settings) -> ClientOptions {
    let mut options = ClientOptions::default();
    options.dsn = Some(dsn);
    options.release = Some(settings.release.clone().into());
    options.environment = Some(settings.environment.clone().into());
    options.send_default_pii = false;
    options.max_breadcrumbs = 0;
    options.default_integrations = false;
    options.integrations = vec![
        Arc::new(ContextIntegration::new()),
        Arc::new(ProcessStacktraceIntegration),
        Arc::new(images::OwnImages::new()),
    ];
    options.before_send = Some(Arc::new(scrub::scrub_event));
    options.transport = Some(Arc::new(DefaultTransportFactory));
    options.shutdown_timeout = CLOSE_FLUSH;
    options
}

/// Reports a panic, then hands it to whatever handler was installed before,
/// which still prints it (or does whatever the app set it to do). Installed
/// once, the first time reporting is turned on, and left in place when it is
/// turned off again, when it only hands on.
fn install_panic_handler() {
    let next = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        report_panic(info);
        next(info);
    }));
}

fn report_panic(info: &PanicHookInfo<'_>) {
    if let Some(client) = current() {
        client.capture_event(panic_event(info), None);
        client.flush(Some(PANIC_FLUSH));
    }
}

fn panic_event(info: &PanicHookInfo<'_>) -> Event<'static> {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("Box<dyn Any>");
    Event {
        exception: vec![Exception {
            ty: "panic".into(),
            value: Some(message.to_owned()),
            mechanism: Some(Mechanism {
                ty: "panic".into(),
                handled: Some(false),
                ..Default::default()
            }),
            stacktrace: current_stacktrace(),
            ..Default::default()
        }]
        .into(),
        level: Level::Fatal,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_hosts() {
        for host in ["localhost", "127.0.0.1", "127.8.9.10", "[::1]", "::1"] {
            assert!(is_loopback(host), "{host}");
        }
        for host in ["example.com", "10.0.0.1", "[::2]", "localhost.example.com"] {
            assert!(!is_loopback(host), "{host}");
        }
    }

    #[test]
    fn the_defaults_are_off() {
        assert_eq!(Consent::default(), Consent::Denied);
        assert_eq!(Settings::default().dsn, None);
    }
}
