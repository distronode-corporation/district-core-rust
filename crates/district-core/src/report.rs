//! Reporting AI-generated content: a support request to Distronode, raised
//! through the support form without leaving the screen the content is on.
//!
//! Lifted from District AI for Windows, which kept it beside its own screens,
//! so that both desktop apps raise the same report. The words and the shape of
//! the message follow the iOS app's: the message names what was reported by
//! kind and id and quotes none of it. A support request is a ticket at a
//! vendor, and the content stays in the workspace, where an agent can read it
//! under the workspace's own rules. What the support desk receives is English
//! in every language (it is read by Distronode's staff); what the member reads
//! is in theirs.
//!
//! The support form is also the one the Support area writes a request in, and
//! the core holds one draft. So a report is refused while that draft is a
//! member's with anything typed in it ([`SupportScreen::report_refusal`]):
//! filling the form would wipe what they wrote.

use district_l10n::Locale;
use district_l10n::support_strings as ss;
use district_model::{Platform, SupportRequestFiling, SupportRequestKind, ThreadRef};

use crate::support::{SupportEvent, SupportForm, SupportScreen};
use crate::text::l;

/// The longest note a report carries, in characters after trimming. The iOS
/// app's limit: well inside the service's 10,000, which the rest of the
/// message shares.
pub const REPORT_NOTE_LIMIT: usize = 2000;

/// The language of everything a report sends: the support desk's.
const STAFF: Locale = Locale::En;

/// What a report is about. Only its kind and id reach the support desk, never
/// the content itself.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReportTarget {
    /// A call's AI summary.
    Call {
        /// The call.
        call_id: String,
    },
    /// A contact's AI dossier.
    Contact {
        /// The contact.
        contact_id: String,
    },
    /// A call's AI summary in a conversation's timeline.
    ThreadEvent {
        /// The conversation. Never sent: a thread with no contact is keyed by
        /// an address. A contact's thread names the contact by its id.
        thread_key: String,
        /// The timeline event.
        event_id: String,
    },
    /// A reply the model wrote into a conversation's reply box. The service
    /// gives a written reply no id, so the report names its kind and the
    /// conversation's contact, and the member's note says the rest.
    AiDraft {
        /// The conversation. Never sent, as for [`ReportTarget::ThreadEvent`].
        thread_key: String,
    },
    /// An answer District HQ gave. The service keeps no conversation and gives
    /// an answer no id, so the report names its kind, and the member's note
    /// says the rest.
    HqAnswer,
    /// The minutes the Companion wrote for a meeting.
    MeetingMinutes {
        /// The meeting.
        meeting_id: String,
    },
    /// The action items the Companion wrote for a meeting.
    MeetingActionItems {
        /// The meeting.
        meeting_id: String,
    },
}

/// Where the report this session started stands.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ReportStatus {
    /// On its way.
    Sending,
    /// Raised.
    Sent {
        /// The confirmation, in the member's language.
        message: String,
    },
    /// Not raised. The note is kept, and sending again sends the same report.
    Failed {
        /// Why, in the member's language.
        message: String,
    },
}

/// The report dialog's own words, in the member's language.
#[derive(Clone, Copy, Debug)]
pub struct ReportDialog;

impl ReportDialog {
    /// The dialog's title.
    pub fn title() -> &'static str {
        ss::report_title(l())
    }

    /// The line under the title.
    pub fn intro() -> &'static str {
        ss::report_intro(l())
    }

    /// The note field's label.
    pub fn note_label() -> &'static str {
        ss::report_note_label(l())
    }

    /// The button that sends the report.
    pub fn send() -> &'static str {
        ss::report_send(l())
    }

    /// The button that closes the dialog without sending.
    pub fn cancel() -> &'static str {
        ss::report_cancel(l())
    }
}

/// The subject every report is raised under, which the support desk sorts on.
pub fn report_subject() -> &'static str {
    ss::report_subject(STAFF)
}

/// The message's first line, which says which app the report came from.
fn preamble(platform: Platform) -> &'static str {
    match platform {
        Platform::Linux => ss::report_preamble_linux(STAFF),
        Platform::Windows => ss::report_preamble_windows(STAFF),
    }
}

/// What the support desk receives about `target`.
fn reference(target: &ReportTarget) -> String {
    match target {
        ReportTarget::Call { call_id } => ss::report_reference_call(STAFF, call_id),
        ReportTarget::Contact { contact_id } => ss::report_reference_contact(STAFF, contact_id),
        // Never the thread key itself: a thread with no contact is keyed by the
        // other party's phone number or email address, which must not reach
        // the support desk. A contact's thread names the contact by its id.
        ReportTarget::ThreadEvent {
            thread_key,
            event_id,
        } => with_contact(
            ss::report_reference_conversation_event(STAFF, event_id),
            thread_key,
        ),
        // The same rule: a contact's thread names the contact, an address's
        // thread nothing but the kind.
        ReportTarget::AiDraft { thread_key } => {
            with_contact(ss::report_reference_ai_draft(STAFF).to_owned(), thread_key)
        }
        ReportTarget::HqAnswer => ss::report_reference_hq_answer(STAFF).to_owned(),
        ReportTarget::MeetingMinutes { meeting_id } => {
            ss::report_reference_meeting_minutes(STAFF, meeting_id)
        }
        ReportTarget::MeetingActionItems { meeting_id } => {
            ss::report_reference_meeting_action_items(STAFF, meeting_id)
        }
    }
}

/// `line`, and the contact on a line of its own when `thread_key` is a
/// contact's thread.
fn with_contact(line: String, thread_key: &str) -> String {
    match ThreadRef::from_thread_key(thread_key) {
        Some(ThreadRef::Contact(contact_id)) => {
            format!(
                "{line}\n{}",
                ss::report_reference_contact(STAFF, &contact_id)
            )
        }
        Some(ThreadRef::Address(_)) | None => line,
    }
}

/// The report's message as the support desk receives it: which app it came
/// from, what it is about, and the note, trimmed and cut to
/// [`REPORT_NOTE_LIMIT`] characters. English whatever the member's language.
pub fn report_message(platform: Platform, target: &ReportTarget, note: &str) -> String {
    let note: String = note.trim().chars().take(REPORT_NOTE_LIMIT).collect();
    let note = note.trim_end();
    let tail = if note.is_empty() {
        ss::report_no_note(STAFF)
    } else {
        note
    };
    format!("{}\n{}\n\n{tail}", preamble(platform), reference(target))
}

/// The support events that raise a report: drop any draft left over, open the
/// support form, fill it in, send it.
///
/// The cancel comes first because the draft carries the idempotency key: a
/// failed report's draft stays open, and filling it with a report on another
/// target would send that under the old key, which the service could answer as
/// already reported. With no draft the cancel does nothing, and while a report
/// is on its way the support form refuses all four, so a second press is
/// ignored.
pub(crate) fn raise(platform: Platform, target: &ReportTarget, note: &str) -> [SupportEvent; 4] {
    [
        SupportEvent::CancelRequest,
        SupportEvent::StartRequest,
        SupportEvent::EditRequest(SupportForm {
            kind: SupportRequestKind::Problem,
            subject: report_subject().to_owned(),
            message: report_message(platform, target, note),
        }),
        SupportEvent::SubmitRequest,
    ]
}

/// Whether `form` is a report's own draft, as [`raise`] fills it, rather than
/// one a member wrote in the Support area.
fn is_report(form: &SupportForm) -> bool {
    form.subject == report_subject()
        && Platform::ALL
            .iter()
            .any(|platform| form.message.starts_with(preamble(*platform)))
}

/// The confirmation of a report raised.
fn confirmation(filing: &SupportRequestFiling) -> &'static str {
    match filing {
        SupportRequestFiling::Filed(_) => ss::report_accepted(l()),
        SupportRequestFiling::Deduplicated => ss::report_already_reported(l()),
        SupportRequestFiling::Pending => ss::report_pending(l()),
    }
}

impl SupportScreen {
    /// Whether the support form holds a member's own draft, as opposed to a
    /// report's or none.
    pub(crate) fn holds_member_draft(&self) -> bool {
        self.compose
            .as_ref()
            .is_some_and(|compose| !is_report(&compose.form))
    }

    /// Whether the support form holds a member's draft with anything typed in
    /// it: a subject or a message that is more than spaces. The kind alone is
    /// not typed (it has a default).
    fn member_draft_typed(&self) -> bool {
        self.compose.as_ref().is_some_and(|compose| {
            let form = &compose.form;
            !is_report(form) && !(form.subject.trim().is_empty() && form.message.trim().is_empty())
        })
    }

    /// Why a report would be refused now, or `None` when it would be sent.
    /// Show it in the report dialog, over the note the dialog keeps.
    pub fn report_refusal(&self) -> Option<String> {
        self.member_draft_typed()
            .then(|| ss::report_draft_open(l()).to_owned())
    }

    /// Where the report this session started stands, or `None` when no report
    /// is showing: none was started, it was dismissed, or the core refused to
    /// start it. A draft the member is writing in the Support area is theirs,
    /// not the report's: it never reads as a report on its way.
    pub fn report_status(&self) -> Option<ReportStatus> {
        if !self.reporting {
            return None;
        }
        let compose = self
            .compose
            .as_ref()
            .filter(|compose| is_report(&compose.form));
        match (compose, &self.submitted) {
            (Some(compose), _) => Some(match &compose.failure {
                Some(failure) if !compose.submitting => ReportStatus::Failed {
                    message: failure.message.clone(),
                },
                _ => ReportStatus::Sending,
            }),
            (None, Some(filing)) => Some(ReportStatus::Sent {
                message: confirmation(filing).to_owned(),
            }),
            (None, None) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS: &str = "AI-generated content was reported from the District AI Windows app.";
    const LINUX: &str = "AI-generated content was reported from the District AI Linux app.";
    const NO_NOTE: &str = "No note was added.";

    #[test]
    fn the_message_names_the_app_the_target_and_carries_the_note() {
        let call = ReportTarget::Call {
            call_id: "call-1".to_owned(),
        };
        assert_eq!(
            report_message(Platform::Windows, &call, "  wrong name  "),
            format!("{WINDOWS}\nCall: call-1\n\nwrong name")
        );
        assert_eq!(
            report_message(Platform::Linux, &call, "wrong name"),
            format!("{LINUX}\nCall: call-1\n\nwrong name")
        );
        assert_eq!(
            report_message(
                Platform::Linux,
                &ReportTarget::Contact {
                    contact_id: "c-1".to_owned()
                },
                " \n "
            ),
            format!("{LINUX}\nContact: c-1\n\n{NO_NOTE}")
        );
        let event = ReportTarget::ThreadEvent {
            thread_key: "contact:c-1".to_owned(),
            event_id: "e-1".to_owned(),
        };
        let long = "x".repeat(REPORT_NOTE_LIMIT + 50);
        let sent = report_message(Platform::Linux, &event, &long);
        assert!(sent.starts_with(&format!(
            "{LINUX}\nConversation event: e-1\nContact: c-1\n\n"
        )));
        assert!(sent.ends_with(&"x".repeat(REPORT_NOTE_LIMIT)));
        assert!(!sent.ends_with(&"x".repeat(REPORT_NOTE_LIMIT + 1)));
        // Cut in the middle of a space run: no trailing space survives.
        let spaced = format!("{} tail", "y".repeat(REPORT_NOTE_LIMIT - 1));
        assert!(
            report_message(Platform::Linux, &call, &spaced)
                .ends_with(&"y".repeat(REPORT_NOTE_LIMIT - 1))
        );
    }

    /// A thread with no contact is keyed by an address, and no part of it
    /// reaches the support desk: only the event's id does.
    #[test]
    fn an_address_thread_is_reported_by_its_event_alone() {
        for (key, address) in [
            ("addr:ada@example.com", "ada@example.com"),
            ("addr:+12125550142", "2125550142"),
            ("fax:2125550142", "2125550142"),
        ] {
            let sent = report_message(
                Platform::Windows,
                &ReportTarget::ThreadEvent {
                    thread_key: key.to_owned(),
                    event_id: "e-9".to_owned(),
                },
                "",
            );
            assert_eq!(
                sent,
                format!("{WINDOWS}\nConversation event: e-9\n\n{NO_NOTE}")
            );
            assert!(!sent.contains(address), "{sent}");
            assert!(!sent.contains("addr:"), "{sent}");
        }
    }

    /// A reply the model wrote is reported by its kind, with the contact of a
    /// contact's thread, and never the address of any other.
    #[test]
    fn an_ai_draft_is_reported_by_its_kind_and_contact() {
        let draft = |key: &str| ReportTarget::AiDraft {
            thread_key: key.to_owned(),
        };
        assert_eq!(
            report_message(
                Platform::Windows,
                &draft("contact:c-1"),
                "named the wrong day"
            ),
            format!(
                "{WINDOWS}\nConversation reply drafted with AI\nContact: c-1\n\nnamed the wrong day"
            )
        );
        for key in [
            "addr:ada@example.com",
            "addr:+12125550142",
            "fax:2125550142",
        ] {
            let sent = report_message(Platform::Windows, &draft(key), "");
            assert_eq!(
                sent,
                format!("{WINDOWS}\nConversation reply drafted with AI\n\n{NO_NOTE}")
            );
            assert!(
                !sent.contains("2125550142") && !sent.contains("ada@"),
                "{sent}"
            );
        }
    }

    #[test]
    fn an_hq_answer_and_a_meeting_record_are_reported_by_kind_and_id() {
        assert_eq!(
            report_message(
                Platform::Linux,
                &ReportTarget::HqAnswer,
                " made up a number "
            ),
            format!("{LINUX}\nDistrict HQ answer\n\nmade up a number")
        );
        let id = || "meeting_1".to_owned();
        assert_eq!(
            report_message(
                Platform::Linux,
                &ReportTarget::MeetingMinutes { meeting_id: id() },
                ""
            ),
            format!("{LINUX}\nMeeting minutes: meeting_1\n\n{NO_NOTE}")
        );
        assert_eq!(
            report_message(
                Platform::Linux,
                &ReportTarget::MeetingActionItems { meeting_id: id() },
                "wrong owner"
            ),
            format!("{LINUX}\nMeeting action items: meeting_1\n\nwrong owner")
        );
    }

    /// What the support desk receives is English in French too; what the
    /// member reads is not taken from the same table.
    #[test]
    fn the_message_is_english_in_every_language() {
        let call = ReportTarget::Call {
            call_id: "call-1".to_owned(),
        };
        let english = report_message(Platform::Linux, &call, "");
        let french =
            district_l10n::with_locale(Locale::FrCa, || report_message(Platform::Linux, &call, ""));
        assert_eq!(english, french);
        assert_eq!(
            district_l10n::with_locale(Locale::FrCa, report_subject),
            "Report: AI-generated content"
        );
    }

    #[test]
    fn each_filing_has_its_confirmation() {
        assert_eq!(
            confirmation(&SupportRequestFiling::Filed("SUP-1".to_owned())),
            "Reported. We'll review it."
        );
        assert!(confirmation(&SupportRequestFiling::Deduplicated).starts_with("You have already"));
        assert!(confirmation(&SupportRequestFiling::Pending).contains("catching up"));
    }

    #[test]
    fn the_dialog_has_its_words() {
        assert_eq!(ReportDialog::title(), "Report AI-generated content");
        assert!(ReportDialog::intro().starts_with("Tell us what is wrong"));
        assert_eq!(ReportDialog::note_label(), "What is wrong (optional)");
        assert_eq!(ReportDialog::send(), "Send");
        assert_eq!(ReportDialog::cancel(), "Cancel");
    }
}
