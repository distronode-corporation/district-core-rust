//! Where this client's endpoint table departs from the Android app's, on purpose.
//!
//! [`EXCLUDED`] names every endpoint the Android app calls (or could be expected
//! to) that this client does not, each with the reason. [`DESKTOP_ONLY`] names the
//! endpoints this client calls and the Android app does not. Nothing else may
//! differ: `tests/endpoint_parity.rs` fails on any other difference between
//! [`ALL_ENDPOINTS`](crate::ALL_ENDPOINTS) and the snapshot of the Android app's
//! endpoints in `contracts/endpoints.snapshot.json`.
//!
//! The strings in [`EXCLUDED`] are also the only place in any crate's `src/`
//! where the paths it names may be written. `tests/forbidden_literals.rs` scans
//! every crate for them and allows them only as these exact values inside this
//! list, so an excluded endpoint cannot come back by being typed somewhere else.

use crate::endpoints::{Endpoint, HttpMethod};

/// Which paths an [`Exclusion`] covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PathMatch {
    /// Exactly this path. `{name}` placeholders match any placeholder.
    Exact(&'static str),
    /// This path and every path below it.
    Subtree(&'static str),
}

impl PathMatch {
    /// Whether `path` (a template, with `{name}` placeholders) is covered.
    pub fn matches(self, path: &str) -> bool {
        let path = normalize_template(path);
        match self {
            Self::Exact(exact) => path == normalize_template(exact),
            Self::Subtree(root) => {
                let root = normalize_template(root);
                path == root || path.starts_with(&format!("{root}/"))
            }
        }
    }

    /// The path or path root as written.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact(path) | Self::Subtree(path) => path,
        }
    }
}

/// An endpoint, or a family of endpoints, this client never calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Exclusion {
    /// A short name for the excluded surface.
    pub name: &'static str,
    /// The method, or `None` for every method.
    pub method: Option<HttpMethod>,
    /// The path or paths.
    pub path: PathMatch,
    /// Why it is excluded, in one line.
    pub reason: &'static str,
    /// Whether the Android app calls it. The parity test holds this to the
    /// snapshot in both directions: an exclusion that stops matching anything the
    /// Android app calls is stale, and so is one that starts to.
    pub android_calls_it: bool,
}

impl Exclusion {
    /// Whether this exclusion covers `method` on `path`.
    pub fn covers(&self, method: HttpMethod, path: &str) -> bool {
        self.method.is_none_or(|m| m == method) && self.path.matches(path)
    }
}

/// Endpoints this client never calls, and why.
pub const EXCLUDED: &[Exclusion] = &[
    Exclusion {
        name: "service administration",
        method: None,
        path: PathMatch::Subtree("/api/admin"),
        reason: "Cross-tenant administration is web-only; the service admits no app credential to it. \
                 The Android app dropped its administration screens too, so it no longer calls it either.",
        android_calls_it: false,
    },
    Exclusion {
        name: "administrator step-up sign-in",
        method: None,
        path: PathMatch::Subtree("/api/auth/native/elevate"),
        reason: "Removed from the service together with administration from the apps.",
        android_calls_it: false,
    },
    Exclusion {
        name: "sign-in code exchange",
        method: Some(HttpMethod::Post),
        path: PathMatch::Exact("/api/auth/native/token"),
        reason: "Unauthenticated sign-in call; it belongs to the district-auth crate.",
        android_calls_it: true,
    },
    Exclusion {
        name: "token refresh",
        method: Some(HttpMethod::Post),
        path: PathMatch::Exact("/api/auth/native/refresh"),
        reason: "Unauthenticated refresh-token rotation; it belongs to the district-auth crate.",
        android_calls_it: true,
    },
    Exclusion {
        name: "sign-out",
        method: Some(HttpMethod::Post),
        path: PathMatch::Exact("/api/auth/native/revoke"),
        reason: "Unauthenticated refresh-token revocation; it belongs to the district-auth crate.",
        android_calls_it: true,
    },
    Exclusion {
        name: "campaign dialling",
        method: None,
        path: PathMatch::Exact("/api/district/calls/outbound"),
        reason: "Bulk dialling puts an AI agent on the line and is web-only.",
        android_calls_it: false,
    },
];

/// An endpoint this client calls that the Android app does not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Addition {
    /// The endpoint.
    pub endpoint: Endpoint,
    /// Why the desktop needs it.
    pub reason: &'static str,
}

/// Endpoints the desktop apps call and the Android app does not, and why. Named
/// for the desktop, not for Linux, because a desktop app on another system shares
/// this table.
pub const DESKTOP_ONLY: &[Addition] = &[
    Addition {
        endpoint: Endpoint::SaveSettings,
        reason: "The account's language, written when a desktop app signs in or starts in a \
                 language the account was not last told, so that what the service says to the \
                 member away from the app (an email, a text message) is in that language. The \
                 Android app's 3.0 writes it too; the snapshot predates it.",
    },
    Addition {
        endpoint: Endpoint::TelemetryToken,
        reason: "Live calls and messages reach a desktop app over the telemetry socket while it \
                 runs. District AI for Linux has no push service; District AI for Windows can \
                 also be reached by Windows push notifications (WNS).",
    },
    Addition {
        endpoint: Endpoint::AuthMe,
        reason: "The simplest authenticated request, which the client's own auth tests drive. \
                 The Android app called it only to decide whether to show its administration \
                 screens, and stopped when those left the app.",
    },
    Addition {
        endpoint: Endpoint::ContactBlock,
        reason: "Blocking a caller from a contact's page. The Android app has no screen that \
                 blocks a caller and removed its unused client for these routes.",
    },
    Addition {
        endpoint: Endpoint::ContactsBlocked,
        reason: "The blocked callers list on the contacts page, for the same reason as blocking.",
    },
    Addition {
        endpoint: Endpoint::NumberConfigure,
        reason: "Pointing a held number back at the workspace. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::NumberRelease,
        reason: "Releasing a held number, behind a confirmation. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::NumberLookup,
        reason: "The billed number lookup, behind a confirmation. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::ProviderStatus,
        reason: "The carrier accounts' status on the phone numbers screen. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::SipTrunks,
        reason: "The SIP trunks on the phone numbers screen. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::SipTrunkCreate,
        reason: "Creating a SIP trunk. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::VerifyService,
        reason: "Whether the one-time passcode sender is in use. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::VerifyServiceSet,
        reason: "Turning the one-time passcode sender on or off. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::A2pRegistration,
        reason: "Filing an A2P 10DLC registration, behind a confirmation. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::TollFreeVerification,
        reason: "Filing a toll-free number's verification, behind a confirmation. Number management, as in the iOS and \
                 Mac apps; the Android app has no screen for it yet.",
    },
    Addition {
        endpoint: Endpoint::BillingHandOff,
        reason: "Checkout and the billing page inside District AI for Windows, the one app that \
                 buys in the app. Every other app's billing is read only.",
    },
];

/// A path template with every `{name}` placeholder written as `{}`, so that two
/// templates naming the same parameter differently compare equal.
pub fn normalize_template(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with('{') && segment.ends_with('}') {
                "{}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}
