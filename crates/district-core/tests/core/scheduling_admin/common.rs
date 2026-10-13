//! What every section shares: the way in from the booking pages screen, the
//! routes, the five sentences for a refusal, and answers that come too late.

use std::time::Duration;

use district_api::{
    ApiError, CODE_REGIONS_DEGRADED, ErrorDetail, RetryReason, TransportError, TransportKind,
    UnauthorizedReason,
};
use district_core::scheduling_admin::{SectionLoad, WriteState, admin_failure, refusal_failure};
use district_core::{Effect, Event, Model, Route, SchedulingScreen, SchedulingSection, Tab};
use district_model::{
    CODE_INVALID_PARAMS, CODE_SCHEDULING_NOT_READY, SchedulingAdminData, SchedulingAdminFailure,
    SchedulingAdminRequest,
};

use super::{admin, answer, done, open, ready_status, sent, ticket_for};
use crate::support::{AGENCY, CLIENT, fixture, loaded, server_error, signed_in, signed_out_error};

#[test]
fn the_sections_are_listed_once_the_booking_pages_are_live() {
    let (mut model, _) = loaded(AGENCY, "agency");
    let effects = model.update(Event::Navigate(Route::Scheduling));
    assert!(
        signed_in(&model).scheduling.sections().is_empty(),
        "not read yet"
    );
    model.update(Event::SchedulingStatusLoaded {
        ticket: effects[0].ticket().unwrap(),
        result: Ok(fixture("district-scheduling-status-provisioning.json")),
    });
    assert!(signed_in(&model).scheduling.sections().is_empty());
    let effects = model.update(Event::Refresh);
    model.update(Event::SchedulingStatusLoaded {
        ticket: effects[0].ticket().unwrap(),
        result: Ok(ready_status()),
    });
    let sections = signed_in(&model).scheduling.sections();
    let titles: Vec<&str> = sections.iter().map(SchedulingSection::title).collect();
    assert_eq!(
        titles,
        [
            "Overview",
            "Event types",
            "Working hours",
            "Bookings",
            "Calendar",
            "Team",
            "Settings",
            "Developer"
        ]
    );
    assert!(sections.iter().all(|s| s.subtitle().is_some()));
    assert_eq!(
        SchedulingSection::Bookings.subtitle(),
        Some("Every booking in this workspace, upcoming and past")
    );
    assert_eq!(SchedulingScreen::sections_heading(), "Manage");
}

#[test]
fn a_detail_has_its_own_title_no_row_and_goes_back_to_its_list() {
    let booking = SchedulingSection::Booking { id: "b".into() };
    let event_type = SchedulingSection::EventType { slug: "s".into() };
    assert_eq!((booking.title(), booking.subtitle()), ("Booking", None));
    assert_eq!(
        (event_type.title(), event_type.subtitle()),
        ("Event type", None)
    );
    assert_eq!(
        Route::SchedulingAdmin(booking).parent(),
        Some(Route::SchedulingAdmin(SchedulingSection::Bookings))
    );
    assert_eq!(
        Route::SchedulingAdmin(event_type).parent(),
        Some(Route::SchedulingAdmin(SchedulingSection::EventTypes))
    );
    let route = Route::SchedulingAdmin(SchedulingSection::Team);
    assert_eq!(route.parent(), Some(Route::Scheduling));
    assert_eq!(route.tab(), Tab::Overview);
    assert!(route.is_workspace_scoped());
    assert_eq!(route.after_workspace_switch(), Route::Scheduling);
}

#[test]
fn every_section_opens_for_a_viewer() {
    for section in SchedulingSection::listed() {
        let (model, _) = open("viewer", section.clone());
        assert_eq!(
            signed_in(&model).route,
            Route::SchedulingAdmin(section),
            "a viewer may read every section"
        );
    }
}

#[test]
fn leaving_for_another_workspace_closes_every_section_and_its_answers() {
    let (mut model, effects) = open("agency", SchedulingSection::Overview);
    let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::EventTypes);
    let effects = model.update(Event::SelectWorkspace(CLIENT.into()));
    assert!(
        effects
            .iter()
            .all(|e| !matches!(e, Effect::SchedulingAdmin { .. }))
    );
    assert_eq!(signed_in(&model).route, Route::Scheduling);
    assert!(admin(&model).overview.is_none());
    assert!(
        answer(
            &mut model,
            ticket,
            done(SchedulingAdminData::EventTypes(Vec::new()))
        )
        .is_empty()
    );
}

#[test]
fn an_answer_for_a_section_left_is_dropped() {
    let (mut model, effects) = open("agency", SchedulingSection::Overview);
    let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::EventTypes);
    model.update(Event::Navigate(Route::SchedulingAdmin(
        SchedulingSection::Hours,
    )));
    assert!(admin(&model).overview.is_none());
    assert!(
        answer(
            &mut model,
            ticket,
            done(SchedulingAdminData::EventTypes(Vec::new()))
        )
        .is_empty()
    );
    assert!(
        model
            .update(Event::SchedulingSignInReady {
                ticket,
                result: Err(server_error()),
            })
            .is_empty()
    );
}

#[test]
fn a_refresh_reads_the_section_afresh() {
    let (mut model, _) = open("agency", SchedulingSection::Overview);
    let effects = model.update(Event::Refresh);
    assert_eq!(sent(&effects).len(), 4);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadSchedulingStatus { .. }))
    );
}

#[test]
fn a_session_that_ended_ends_it_from_any_section() {
    let (mut model, effects) = open("agency", SchedulingSection::Overview);
    let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::Me);
    answer(&mut model, ticket, Err(signed_out_error()));
    assert!(!matches!(
        model.session(),
        district_core::SessionState::SignedIn(_)
    ));
}

fn detail(code: Option<&str>) -> ErrorDetail {
    ErrorDetail {
        message: Some("The service's words".into()),
        code: code.map(str::to_owned),
        degraded_regions: Vec::new(),
    }
}

#[test]
fn every_refusal_reads_as_one_of_the_web_dashboards_sentences() {
    const UNAVAILABLE: &str = "The booking system did not answer. Try again in a minute.";
    const UNKNOWN: &str = "That did not save. Try again.";
    let cases: Vec<(ApiError, &str, bool)> = vec![
        (
            ApiError::Offline(TransportError {
                kind: TransportKind::Timeout,
                message: String::new(),
            }),
            "You appear to be offline. Check your connection and try again.",
            true,
        ),
        (
            ApiError::Decode {
                endpoint: district_api::Endpoint::SchedulingAdmin,
                line: 0,
                column: 0,
            },
            "This version of the app could not read that response. Please update.",
            false,
        ),
        (
            ApiError::Unconfirmed {
                endpoint: district_api::Endpoint::SchedulingSignIn,
            },
            "This version of the app could not read that response. Please update.",
            false,
        ),
        (
            ApiError::Forbidden(detail(None)),
            "You can view this but not change it.",
            false,
        ),
        (
            ApiError::Conflict(detail(Some(CODE_SCHEDULING_NOT_READY))),
            "Scheduling is not set up for this workspace yet.",
            false,
        ),
        (
            ApiError::Envelope {
                status: 400,
                code: CODE_INVALID_PARAMS.into(),
                detail: detail(Some(CODE_INVALID_PARAMS)),
            },
            UNKNOWN,
            false,
        ),
        (
            ApiError::RateLimited {
                retry_after: Some(Duration::from_secs(3600)),
                detail: detail(Some("rate_limited")),
            },
            UNAVAILABLE,
            true,
        ),
        (
            ApiError::Server {
                status: 503,
                detail: detail(None),
            },
            UNAVAILABLE,
            true,
        ),
        (
            ApiError::Rejected {
                status: 413,
                detail: detail(None),
            },
            UNAVAILABLE,
            true,
        ),
        (
            ApiError::Envelope {
                status: 500,
                code: "server_error".into(),
                detail: detail(Some("server_error")),
            },
            UNAVAILABLE,
            true,
        ),
        (
            ApiError::Envelope {
                status: 503,
                code: CODE_REGIONS_DEGRADED.into(),
                detail: detail(Some(CODE_REGIONS_DEGRADED)),
            },
            UNAVAILABLE,
            true,
        ),
        (
            ApiError::Rejected {
                status: 400,
                detail: detail(Some("unknown_op")),
            },
            UNKNOWN,
            true,
        ),
        (ApiError::NotFound(detail(None)), UNKNOWN, true),
        (ApiError::Conflict(detail(None)), UNKNOWN, true),
        (
            ApiError::Offline(TransportError {
                kind: TransportKind::NotJson {
                    status: 200,
                    content_type: "text/html".into(),
                },
                message: String::new(),
            }),
            UNKNOWN,
            true,
        ),
    ];
    for (error, message, retryable) in cases {
        let text = admin_failure(&error);
        assert_eq!(
            (text.message.as_str(), text.retryable),
            (message, retryable),
            "{error:?}"
        );
    }
    // The session's own failures read as they do everywhere.
    let refused = ApiError::Unauthorized(UnauthorizedReason::RefusedNotRetried);
    assert_eq!(
        admin_failure(&refused),
        district_core::FailureText::from_api_error(&refused)
    );
    let keyring = ApiError::TokenUnavailable(RetryReason::SecretStoreLocked);
    assert_eq!(
        admin_failure(&keyring),
        district_core::FailureText::from_api_error(&keyring)
    );
}

#[test]
fn the_schedulers_own_refusal_reads_by_its_kind() {
    let failure = |kind: &str| SchedulingAdminFailure {
        ok: false,
        failure: kind.into(),
        status: 502,
    };
    let read = |kind| {
        let text = refusal_failure(&failure(kind));
        (text.message, text.retryable)
    };
    let unavailable = (
        "The booking system did not answer. Try again in a minute.".to_owned(),
        true,
    );
    assert_eq!(read("unavailable"), unavailable);
    assert_eq!(read("instance_unavailable"), unavailable);
    assert_eq!(
        read("slot_taken"),
        ("That time was just taken. Pick another.".to_owned(), false)
    );
    assert_eq!(
        read("not_found"),
        ("That did not save. Try again.".to_owned(), true)
    );
}

#[test]
fn a_section_load_and_a_write_say_what_they_hold() {
    let failed: SectionLoad<u8> = SectionLoad::Failed(admin_failure(&server_error()));
    assert_eq!(failed.value(), None);
    assert!(failed.failure().is_some());
    assert_eq!(SectionLoad::Ready(3).value(), Some(&3));
    assert_eq!(SectionLoad::<u8>::default(), SectionLoad::Loading);
    assert!(WriteState::Working.busy());
    assert_eq!(WriteState::Idle.notice(), None);
    assert_eq!(WriteState::Idle.failure(), None);
}

#[test]
fn a_model_that_never_opened_a_section_holds_none() {
    let (model, _) = loaded(AGENCY, "agency");
    assert_eq!(*admin(&model), Default::default());
    let _: &Model = &model;
}
