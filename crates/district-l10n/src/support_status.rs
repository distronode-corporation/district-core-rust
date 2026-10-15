//! What a screen shows for a support request's status, in the app's language.
//!
//! The help desk's own status name (`statusName`) is never shown as it is. It
//! is the desk's English ("Waiting for support"), so a French screen showed
//! English, and the live workflow is localised besides, so a raw name can be in
//! any language. A name this build lists gets its catalog label; an unlisted
//! name reads as its category's label, and an unknown category as "Status
//! unknown". The same rule, and the same words, as District AI for iOS's
//! `SupportStatusLabel`.
//!
//! Display only: every decision about a request (resolved, closeable) stays on
//! the category and the service's flags, never on this label.

use crate::Locale;
use crate::support_strings as s;

/// The label of a support request's status: its `status_name` when this
/// build lists it (matched trimmed and without regard to case), else the label
/// of its `category` (`NEW`, `INDETERMINATE`, `DONE`, or the service's own
/// `PENDING` for a request not filed yet, in either case), else "Status
/// unknown".
pub fn support_status_label(status_name: &str, category: &str, locale: Locale) -> &'static str {
    known(&status_name.trim().to_lowercase(), locale).unwrap_or_else(|| generic(category, locale))
}

/// The label of a status name this build lists, or `None`. The names are the
/// service management workflow's and the service's own: `Received` is what the
/// service writes for a request the desk does not hold yet, `unknown` what it
/// writes when the desk sent no status, and `Done` its default for a close.
fn known(name: &str, l: Locale) -> Option<&'static str> {
    Some(match name {
        "received" => s::request_status_received(l),
        "open" | "reopened" | "to do" => s::request_status_open(l),
        "waiting for support" => s::request_status_waiting_for_support(l),
        "waiting for customer" => s::request_status_waiting_for_customer(l),
        "in progress" | "work in progress" => s::request_status_in_progress(l),
        "pending" => s::request_status_pending(l),
        "escalated" => s::request_status_escalated(l),
        "resolved" => s::request_status_resolved(l),
        "done" | "completed" => s::request_status_done(l),
        "closed" => s::request_status_closed(l),
        "canceled" | "cancelled" => s::request_status_canceled(l),
        "declined" => s::request_status_declined(l),
        "unknown" => s::request_status_unknown(l),
        _ => return None,
    })
}

/// The label of a status category, compared without case: the desk's issue
/// API reports it in lower case and its service desk API in upper case.
fn generic(category: &str, l: Locale) -> &'static str {
    match category.trim().to_ascii_uppercase().as_str() {
        "DONE" => s::request_status_closed(l),
        "NEW" => s::request_status_open(l),
        "INDETERMINATE" => s::request_status_in_progress(l),
        "PENDING" => s::request_status_received(l),
        _ => s::request_status_unknown(l),
    }
}
