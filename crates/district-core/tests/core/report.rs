//! Reporting AI-generated content: raised through the support form from any
//! screen, refused over a member's own draft, and put away without touching
//! one.

use district_core::{
    Effect, Event, Model, ReportStatus, ReportTarget, Route, SupportEvent, SupportForm,
    SupportScreen, report_message,
};
use district_model::{
    Platform, SupportRequestCreateResponse, SupportRequestDraft, SupportRequestFiling,
    SupportRequestKind,
};

use crate::support::{AGENCY, VIEWER, fixture, loaded, server_error, signed_in};

const SUBJECT: &str = "Report: AI-generated content";

fn support(model: &Model) -> &SupportScreen {
    &signed_in(model).support
}

fn event(model: &mut Model, event: SupportEvent) -> Vec<Effect> {
    model.update(Event::Support(event))
}

fn call() -> ReportTarget {
    ReportTarget::Call {
        call_id: "call-1".to_owned(),
    }
}

fn report(model: &mut Model, target: ReportTarget, note: &str) -> Vec<Effect> {
    event(
        model,
        SupportEvent::Report {
            platform: Platform::Linux,
            target,
            note: note.to_owned(),
        },
    )
}

/// The one support request `effects` raises, its ticket and its key.
fn raised(effects: &[Effect]) -> (district_core::Ticket, SupportRequestDraft, String) {
    match effects {
        [
            Effect::CreateSupportRequest {
                ticket,
                draft,
                idempotency_key,
                ..
            },
        ] => (*ticket, draft.clone(), idempotency_key.clone()),
        other => panic!("{other:?}"),
    }
}

fn answered(issue_key: Option<&str>, deduplicated: bool) -> SupportRequestCreateResponse {
    let mut answer: SupportRequestCreateResponse = fixture("district-support-request-create.json");
    answer.issue_key = issue_key.map(str::to_owned);
    answer.deduplicated = deduplicated;
    answer
}

/// On the call log as an agency member, with nothing in the support form.
fn on_calls() -> Model {
    let (mut model, _) = loaded(AGENCY, "agency");
    model.update(Event::Navigate(Route::Calls));
    model
}

#[test]
fn a_report_raises_a_problem_on_the_spot_without_navigating() {
    let mut model = on_calls();
    assert_eq!(support(&model).report_status(), None);
    assert_eq!(support(&model).report_refusal(), None);
    let effects = report(&mut model, call(), "  wrong name ");
    let (_, draft, _) = raised(&effects);
    assert_eq!(
        draft,
        SupportRequestDraft {
            kind: SupportRequestKind::Problem,
            subject: SUBJECT.to_owned(),
            message: report_message(Platform::Linux, &call(), "wrong name"),
        }
    );
    assert_eq!(signed_in(&model).route, Route::Calls);
    assert!(support(&model).reporting);
    assert_eq!(support(&model).report_status(), Some(ReportStatus::Sending));
    // A second press while it is on its way is nothing.
    assert!(report(&mut model, call(), "again").is_empty());
}

#[test]
fn each_outcome_has_its_sentence_and_dismissing_puts_it_away() {
    for (issue_key, deduplicated, said) in [
        (Some("SUP-7"), false, "Reported. We'll review it."),
        (
            None,
            true,
            "You have already reported this. We'll review it.",
        ),
        (None, false, "Reported. We have it and it is catching up."),
    ] {
        let mut model = on_calls();
        let (ticket, _, _) = raised(&report(&mut model, call(), ""));
        let effects = model.update(Event::SupportRequestCreated {
            ticket,
            result: Ok(answered(issue_key, deduplicated)),
        });
        assert!(matches!(
            effects.as_slice(),
            [Effect::LoadSupportRequests { .. }]
        ));
        assert_eq!(
            support(&model).report_status(),
            Some(ReportStatus::Sent {
                message: said.to_owned()
            })
        );
        assert!(event(&mut model, SupportEvent::DismissReport).is_empty());
        assert!(!support(&model).reporting);
        assert_eq!(support(&model).submitted, None);
        assert_eq!(support(&model).report_status(), None);
    }
}

#[test]
fn a_failed_report_keeps_its_place_and_a_new_one_gets_a_new_key() {
    let mut model = on_calls();
    let (ticket, _, first_key) = raised(&report(&mut model, call(), "note"));
    model.update(Event::SupportRequestCreated {
        ticket,
        result: Err(server_error()),
    });
    let Some(ReportStatus::Failed { message }) = support(&model).report_status() else {
        panic!("{:?}", support(&model).report_status());
    };
    assert!(!message.is_empty());
    // A report's own draft is not the member's, so nothing is refused.
    assert_eq!(support(&model).report_refusal(), None);
    let other = ReportTarget::HqAnswer;
    let (_, draft, second_key) = raised(&report(&mut model, other.clone(), "note"));
    assert_ne!(first_key, second_key);
    assert_eq!(
        draft.message,
        report_message(Platform::Linux, &other, "note")
    );
}

#[test]
fn dismissing_a_failed_report_closes_its_draft() {
    let mut model = on_calls();
    let (ticket, _, _) = raised(&report(&mut model, call(), ""));
    model.update(Event::SupportRequestCreated {
        ticket,
        result: Err(server_error()),
    });
    event(&mut model, SupportEvent::DismissReport);
    assert_eq!(support(&model).compose, None);
    assert_eq!(support(&model).report_status(), None);
}

#[test]
fn a_report_is_refused_over_a_draft_the_member_typed() {
    let mut model = on_calls();
    event(&mut model, SupportEvent::StartRequest);
    let typed = SupportForm {
        kind: SupportRequestKind::Question,
        subject: "Billing".to_owned(),
        message: String::new(),
    };
    event(&mut model, SupportEvent::EditRequest(typed.clone()));
    assert_eq!(
        support(&model).report_refusal().as_deref(),
        Some("Send or discard the support request you are writing in Support, then report this.")
    );
    assert!(report(&mut model, call(), "note").is_empty());
    assert!(!support(&model).reporting);
    assert_eq!(support(&model).report_status(), None);
    assert_eq!(
        support(&model).compose.as_ref().map(|c| &c.form),
        Some(&typed)
    );
}

#[test]
fn an_untyped_draft_gives_way_to_a_report() {
    let mut model = on_calls();
    event(&mut model, SupportEvent::StartRequest);
    // A kind alone is not typed: it has a default.
    event(
        &mut model,
        SupportEvent::EditRequest(SupportForm {
            kind: SupportRequestKind::Suggestion,
            subject: "  ".to_owned(),
            message: "\n".to_owned(),
        }),
    );
    assert_eq!(support(&model).report_refusal(), None);
    raised(&report(&mut model, call(), ""));
}

#[test]
fn putting_a_report_away_leaves_a_members_draft_alone() {
    let mut model = on_calls();
    let (ticket, _, _) = raised(&report(&mut model, call(), ""));
    model.update(Event::SupportRequestCreated {
        ticket,
        result: Ok(answered(Some("SUP-1"), false)),
    });
    // The member starts writing in Support while the confirmation shows: it
    // is still the report's confirmation, and the draft is theirs.
    event(&mut model, SupportEvent::StartRequest);
    let typed = SupportForm {
        kind: SupportRequestKind::Problem,
        subject: "Mine".to_owned(),
        message: "Mine too".to_owned(),
    };
    event(&mut model, SupportEvent::EditRequest(typed.clone()));
    assert_eq!(
        support(&model).report_status(),
        Some(ReportStatus::Sent {
            message: "Reported. We'll review it.".to_owned()
        })
    );
    event(&mut model, SupportEvent::DismissReport);
    assert_eq!(
        support(&model).compose.as_ref().map(|c| &c.form),
        Some(&typed)
    );
    assert_eq!(support(&model).submitted, None::<SupportRequestFiling>);
}

#[test]
fn a_confirmation_put_away_in_support_ends_the_report() {
    let mut model = on_calls();
    let (ticket, _, _) = raised(&report(&mut model, call(), ""));
    model.update(Event::SupportRequestCreated {
        ticket,
        result: Ok(answered(Some("SUP-1"), false)),
    });
    event(&mut model, SupportEvent::DismissSubmitted);
    assert!(support(&model).reporting);
    assert_eq!(support(&model).report_status(), None);
}

#[test]
fn a_viewer_cannot_report() {
    let (mut model, _) = loaded(VIEWER, "viewer");
    assert!(report(&mut model, call(), "note").is_empty());
    assert!(event(&mut model, SupportEvent::DismissReport).is_empty());
    assert!(!support(&model).reporting);
}
