//! The booking scheduler's administration: each operation's answer read from
//! its recorded fixture by the request that asks for it, the parameters each
//! request sends, and the cases each fixture exists to cover.

use district_model::{
    SchedulingAdminData, SchedulingAdminErrorBody, SchedulingAdminFailure, SchedulingAdminOp,
    SchedulingAdminRequest, SchedulingBookingQuery, SchedulingBrandingPatch,
    SchedulingCalendarSelection, SchedulingEventTypeFields, SchedulingHostAssignment,
    SchedulingOverrideCreated, SchedulingOverrideDraft, SchedulingProfilePatch,
    SchedulingQuestionFields, SchedulingSecret, SchedulingUpload, SchedulingUploadTarget,
};
use serde_json::{Value, json};

use crate::support::{decode, read_fixture};

/// The `data` of a recorded success.
fn data(name: &str) -> Value {
    let body: Value = serde_json::from_str(&read_fixture(name)).expect("JSON");
    assert_eq!(body["ok"], true, "{name} is a success");
    body["data"].clone()
}

fn read(request: &SchedulingAdminRequest, name: &str) -> SchedulingAdminData {
    request
        .decode(data(name))
        .unwrap_or_else(|error| panic!("{name} as {:?}: {error}", request.op()))
}

fn slug() -> String {
    "phone-consultation".to_owned()
}

fn id() -> String {
    "x".to_owned()
}

fn upload() -> SchedulingUpload {
    SchedulingUpload {
        target: SchedulingUploadTarget::Logo,
        file_name: "logo.png".into(),
        mime_type: "image/png".into(),
        bytes: vec![1, 2, 3],
    }
}

#[test]
fn every_request_reads_its_operations_answer() {
    use SchedulingAdminData as D;
    use SchedulingAdminRequest as R;
    let fields = SchedulingEventTypeFields::default;
    let cases: Vec<(R, &str)> = vec![
        (R::Me, "district-scheduling-me.json"),
        (
            R::PatchMe(SchedulingProfilePatch::default()),
            "district-scheduling-me.json",
        ),
        (R::DeleteAvatar, "district-scheduling-no-content.json"),
        (R::EventTypes, "district-scheduling-event-types.json"),
        (
            R::EventType { slug: slug() },
            "district-scheduling-event-type.json",
        ),
        (
            R::CreateEventType {
                slug: slug(),
                fields: fields(),
            },
            "district-scheduling-event-type.json",
        ),
        (
            R::PatchEventType {
                slug: slug(),
                fields: fields(),
            },
            "district-scheduling-event-type.json",
        ),
        (
            R::DeleteEventType { slug: slug() },
            "district-scheduling-no-content.json",
        ),
        (R::Hosts { slug: slug() }, "district-scheduling-hosts.json"),
        (
            R::SetHosts {
                slug: slug(),
                hosts: Vec::new(),
            },
            "district-scheduling-hosts.json",
        ),
        (
            R::SendTestEmail {
                slug: slug(),
                kind: "reminder".into(),
            },
            "district-scheduling-test-email.json",
        ),
        (
            R::Questions { slug: slug() },
            "district-scheduling-questions.json",
        ),
        (
            R::CreateQuestion {
                slug: slug(),
                fields: SchedulingQuestionFields::default(),
            },
            "district-scheduling-question.json",
        ),
        (
            R::PatchQuestion {
                slug: slug(),
                id: id(),
                fields: SchedulingQuestionFields::default(),
            },
            "district-scheduling-question.json",
        ),
        (
            R::DeleteQuestion {
                slug: slug(),
                id: id(),
            },
            "district-scheduling-no-content.json",
        ),
        (
            R::Slots {
                slug: slug(),
                from: None,
                to: None,
                tz: None,
            },
            "district-scheduling-slots.json",
        ),
        (
            R::Rules {
                event_type_id: None,
            },
            "district-scheduling-rules.json",
        ),
        (
            R::CreateRule {
                event_type_id: None,
                day_of_week: 3,
                start_time: "10:30".into(),
                end_time: "15:00".into(),
            },
            "district-scheduling-rule.json",
        ),
        (
            R::PatchRule {
                id: id(),
                day_of_week: None,
                start_time: None,
                end_time: None,
            },
            "district-scheduling-rule.json",
        ),
        (
            R::DeleteRule { id: id() },
            "district-scheduling-no-content.json",
        ),
        (R::Overrides, "district-scheduling-overrides.json"),
        (
            R::CreateOverride(SchedulingOverrideDraft::default()),
            "district-scheduling-override-created.json",
        ),
        (
            R::CreateOverride(SchedulingOverrideDraft::default()),
            "district-scheduling-override-range.json",
        ),
        (
            R::PatchOverride {
                id: id(),
                reason: None,
                start_time: None,
                end_time: None,
            },
            "district-scheduling-override-created.json",
        ),
        (
            R::DeleteOverride { id: id() },
            "district-scheduling-no-content.json",
        ),
        (
            R::DeleteOverrideGroup { group_id: id() },
            "district-scheduling-no-content.json",
        ),
        (
            R::Bookings(SchedulingBookingQuery::default()),
            "district-scheduling-bookings.json",
        ),
        (
            R::BookingAnswers { id: id() },
            "district-scheduling-booking-answers.json",
        ),
        (
            R::CancelBooking {
                id: id(),
                reason: None,
            },
            "district-scheduling-booking.json",
        ),
        (
            R::RescheduleBooking {
                id: id(),
                start_at: "2026-09-14T13:00:00Z".into(),
            },
            "district-scheduling-booking.json",
        ),
        (
            R::ReassignBooking {
                id: id(),
                host_id: id(),
            },
            "district-scheduling-booking.json",
        ),
        (
            R::CalendarStatus,
            "district-scheduling-calendar-status.json",
        ),
        (
            R::ConnectCaldav {
                preset: None,
                server_url: None,
                username: id(),
                app_password: SchedulingSecret::new("p"),
            },
            "district-scheduling-caldav-connect.json",
        ),
        (
            R::Calendars {
                id: id(),
                provider: "google".into(),
                account: None,
            },
            "district-scheduling-calendars.json",
        ),
        (
            R::SetCalendars {
                id: id(),
                provider: "google".into(),
                account_email: None,
                calendars: Vec::new(),
            },
            "district-scheduling-ok.json",
        ),
        (
            R::SetDestination {
                id: id(),
                provider: "google".into(),
                account_email: None,
            },
            "district-scheduling-ok.json",
        ),
        (
            R::DeleteConnection {
                id: id(),
                provider: "google".into(),
                account: None,
            },
            "district-scheduling-ok.json",
        ),
        (R::ZoomStatus, "district-scheduling-zoom-status.json"),
        (
            R::Users {
                include_archived: None,
            },
            "district-scheduling-users.json",
        ),
        (
            R::ArchiveUser { id: id() },
            "district-scheduling-user-archive.json",
        ),
        (
            R::UpcomingBookings { id: id() },
            "district-scheduling-user-upcoming.json",
        ),
        (R::Teams, "district-scheduling-teams.json"),
        (R::Team { id: id() }, "district-scheduling-team.json"),
        (
            R::CreateTeam {
                name: id(),
                slug: None,
            },
            "district-scheduling-team.json",
        ),
        (
            R::PatchTeam {
                id: id(),
                name: None,
                slug: None,
            },
            "district-scheduling-team.json",
        ),
        (R::DeleteTeam { id: id() }, "district-scheduling-ok.json"),
        (
            R::AddTeamMember {
                id: id(),
                user_id: id(),
                routing_priority: None,
            },
            "district-scheduling-team.json",
        ),
        (
            R::PatchTeamMember {
                id: id(),
                user_id: id(),
                routing_priority: 1,
            },
            "district-scheduling-team.json",
        ),
        (
            R::RemoveTeamMember {
                id: id(),
                user_id: id(),
            },
            "district-scheduling-ok.json",
        ),
        (R::Branding, "district-scheduling-branding.json"),
        (
            R::PatchBranding(SchedulingBrandingPatch::default()),
            "district-scheduling-branding.json",
        ),
        (R::DeleteLogo, "district-scheduling-no-content.json"),
        (R::DeleteBanner, "district-scheduling-no-content.json"),
        (R::Llm, "district-scheduling-llm.json"),
        (
            R::PatchLlm {
                enabled: None,
                extra_instructions: None,
            },
            "district-scheduling-llm.json",
        ),
        (R::ApiKeys, "district-scheduling-api-keys.json"),
        (
            R::CreateApiKey { name: id() },
            "district-scheduling-api-key-created.json",
        ),
        (
            R::DeleteApiKey { id: id() },
            "district-scheduling-no-content.json",
        ),
        (
            R::OauthConnections,
            "district-scheduling-oauth-connections.json",
        ),
        (
            R::DeleteOauthConnection { id: id() },
            "district-scheduling-no-content.json",
        ),
        (R::Webhooks, "district-scheduling-webhooks.json"),
        (
            R::CreateWebhook {
                url: id(),
                events: Vec::new(),
                fields: None,
            },
            "district-scheduling-webhook-created.json",
        ),
        (
            R::PatchWebhook {
                id: id(),
                events: None,
                fields: None,
            },
            "district-scheduling-no-content.json",
        ),
        (
            R::DeleteWebhook { id: id() },
            "district-scheduling-no-content.json",
        ),
        (
            R::WebhookDeliveries { id: id() },
            "district-scheduling-webhook-deliveries.json",
        ),
        (R::Upload(upload()), "district-scheduling-upload.json"),
    ];
    let mut ops = std::collections::BTreeSet::new();
    for (request, name) in &cases {
        let answer = read(request, name);
        if let Some(op) = request.op() {
            ops.insert(op);
        }
        let matches = match (request, &answer) {
            (R::Me | R::PatchMe(_), D::Me(me)) => me.is_admin,
            (R::EventTypes, D::EventTypes(rows)) => rows.len() == 2,
            (_, D::EventType(row)) => row.slug == "phone-consultation",
            (_, D::Hosts(rows)) => rows.len() == 2,
            (_, D::TestEmail(sent)) => sent.sent,
            (_, D::Questions(rows)) => rows.len() == 2,
            (_, D::Question(row)) => !row.required,
            (_, D::Slots(slots)) => slots.slots.len() == 2,
            (_, D::Rules(rows)) => rows.len() == 2,
            (_, D::Rule(rule)) => rule.event_type_id.is_none(),
            (_, D::Overrides(rows)) => rows.len() == 2,
            (_, D::Override(row)) => !row.is_available,
            (_, D::OverrideCreated(SchedulingOverrideCreated::Day(day))) => {
                day.date == "2026-09-25"
            }
            (_, D::OverrideCreated(SchedulingOverrideCreated::Range(range))) => range.days == 8,
            (_, D::BookingPage(page)) => page.total == Some(2),
            (_, D::Answers(rows)) => rows.len() == 2,
            (_, D::Booking(row)) => row.status == "cancelled",
            (_, D::CalendarStatus(status)) => status.connections.len() == 2,
            (_, D::CaldavConnected(account)) => account.connected,
            (_, D::Calendars(rows)) => rows.len() == 2,
            (_, D::ZoomStatus(zoom)) => zoom.configured && !zoom.connected,
            (_, D::Users(rows)) => rows.len() == 2,
            (_, D::UserArchived(done)) => done.ok,
            (_, D::UpcomingBookings(rows)) => rows.len() == 1,
            (_, D::Teams(rows)) => rows.len() == 2,
            (_, D::Team(team)) => !team.id.is_empty(),
            (_, D::Done(done)) => done.ok,
            (_, D::Branding(branding)) => branding.logo_height == 48,
            (_, D::Llm(llm)) => llm.enabled,
            (_, D::ApiKeys(rows)) => rows.len() == 2,
            (_, D::ApiKeyCreated(key)) => key.key.starts_with("cno_"),
            (_, D::OauthConnections(rows)) => rows.len() == 2,
            (_, D::Webhooks(rows)) => rows.len() == 2,
            (_, D::WebhookCreated(hook)) => hook.secret.is_some(),
            (_, D::Deliveries(rows)) => rows.len() == 2,
            (_, D::Uploaded(result)) => result.published_url().is_some(),
            _ => false,
        };
        assert!(matches, "{name} read as {answer:?} for {request:?}");
    }
    assert_eq!(ops.len(), 64, "every operation is read from a fixture");
}

#[test]
fn a_shape_the_operation_does_not_answer_is_an_error() {
    let request = SchedulingAdminRequest::CreateOverride(SchedulingOverrideDraft::default());
    assert!(request.decode(json!({"neither": true})).is_err());
    assert!(SchedulingAdminRequest::Me.decode(json!([])).is_err());
}

fn params(request: SchedulingAdminRequest) -> Value {
    request.params()
}

#[test]
fn each_request_sends_its_parameters_with_its_path_keys_and_no_nulls() {
    use SchedulingAdminRequest as R;
    let empty = json!({});
    for request in [
        R::Me,
        R::DeleteAvatar,
        R::EventTypes,
        R::Overrides,
        R::CalendarStatus,
        R::ZoomStatus,
        R::Teams,
        R::Branding,
        R::DeleteLogo,
        R::DeleteBanner,
        R::Llm,
        R::ApiKeys,
        R::OauthConnections,
        R::Webhooks,
        R::Upload(upload()),
    ] {
        assert_eq!(request.params(), empty, "{request:?}");
    }
    assert_eq!(
        params(R::PatchMe(SchedulingProfilePatch {
            timezone: Some("America/Toronto".into()),
            week_start: Some(1),
            notify_host_cancel: Some(false),
            ..SchedulingProfilePatch::default()
        })),
        json!({"timezone": "America/Toronto", "week_start": 1, "notify_host_cancel": false})
    );
    for request in [
        R::EventType { slug: slug() },
        R::DeleteEventType { slug: slug() },
        R::Hosts { slug: slug() },
        R::Questions { slug: slug() },
    ] {
        assert_eq!(request.params(), json!({"slug": "phone-consultation"}));
    }
    assert_eq!(
        params(R::CreateEventType {
            slug: "intro".into(),
            fields: SchedulingEventTypeFields {
                name: Some("Intro".into()),
                duration_minutes: Some(30),
                location_type: Some("phone".into()),
                location_value: Some("+14165550142".into()),
                ..SchedulingEventTypeFields::default()
            },
        }),
        json!({"slug": "intro", "name": "Intro", "duration_minutes": 30,
               "location_type": "phone", "location_value": "+14165550142"})
    );
    assert_eq!(
        params(R::PatchEventType {
            slug: "intro".into(),
            fields: SchedulingEventTypeFields {
                is_active: Some(false),
                reminders: Some(vec![1440, 60]),
                ..SchedulingEventTypeFields::default()
            },
        }),
        json!({"slug": "intro", "is_active": false, "reminders": [1440, 60]})
    );
    assert_eq!(
        params(R::SetHosts {
            slug: "intro".into(),
            hosts: vec![SchedulingHostAssignment {
                user_id: "u1".into(),
                role: "rotation".into(),
                priority: 2,
            }],
        }),
        json!({"slug": "intro", "hosts": [{"user_id": "u1", "role": "rotation", "priority": 2}]})
    );
    assert_eq!(
        params(R::SendTestEmail {
            slug: "intro".into(),
            kind: "confirmation".into(),
        }),
        json!({"slug": "intro", "type": "confirmation"})
    );
    assert_eq!(
        params(R::CreateQuestion {
            slug: "intro".into(),
            fields: SchedulingQuestionFields {
                label: Some("Which?".into()),
                kind: Some("select".into()),
                options: Some(vec!["A".into()]),
                required: Some(true),
                position: None,
            },
        }),
        json!({"slug": "intro", "label": "Which?", "type": "select", "options": ["A"],
               "required": true})
    );
    assert_eq!(
        params(R::PatchQuestion {
            slug: "intro".into(),
            id: "q1".into(),
            fields: SchedulingQuestionFields {
                position: Some(0),
                ..SchedulingQuestionFields::default()
            },
        }),
        json!({"slug": "intro", "id": "q1", "position": 0})
    );
    assert_eq!(
        params(R::DeleteQuestion {
            slug: "intro".into(),
            id: "q1".into(),
        }),
        json!({"slug": "intro", "id": "q1"})
    );
    assert_eq!(
        params(R::Slots {
            slug: "intro".into(),
            from: Some("2026-09-14".into()),
            to: Some("2026-09-14".into()),
            tz: Some("America/Toronto".into()),
        }),
        json!({"slug": "intro", "from": "2026-09-14", "to": "2026-09-14",
               "tz": "America/Toronto"})
    );
    assert_eq!(
        params(R::Rules {
            event_type_id: None
        }),
        json!({})
    );
    assert_eq!(
        params(R::Rules {
            event_type_id: Some("et".into())
        }),
        json!({"event_type_id": "et"})
    );
    assert_eq!(
        params(R::CreateRule {
            event_type_id: None,
            day_of_week: 1,
            start_time: "09:00".into(),
            end_time: "17:00".into(),
        }),
        json!({"day_of_week": 1, "start_time": "09:00", "end_time": "17:00"})
    );
    assert_eq!(
        params(R::PatchRule {
            id: "r".into(),
            day_of_week: Some(2),
            start_time: None,
            end_time: Some("18:00".into()),
        }),
        json!({"id": "r", "day_of_week": 2, "end_time": "18:00"})
    );
    for request in [
        R::DeleteRule { id: id() },
        R::DeleteOverride { id: id() },
        R::BookingAnswers { id: id() },
        R::ArchiveUser { id: id() },
        R::UpcomingBookings { id: id() },
        R::Team { id: id() },
        R::DeleteTeam { id: id() },
        R::DeleteApiKey { id: id() },
        R::DeleteOauthConnection { id: id() },
        R::DeleteWebhook { id: id() },
        R::WebhookDeliveries { id: id() },
    ] {
        assert_eq!(request.params(), json!({"id": "x"}), "{request:?}");
    }
    assert_eq!(
        params(R::CreateOverride(SchedulingOverrideDraft {
            date: "2026-12-24".into(),
            end_date: Some("2026-12-31".into()),
            reason: "out_of_office".into(),
            start_time: None,
            end_time: None,
        })),
        json!({"date": "2026-12-24", "end_date": "2026-12-31", "reason": "out_of_office"})
    );
    assert_eq!(
        params(R::PatchOverride {
            id: "o".into(),
            reason: Some("custom_hours".into()),
            start_time: Some("12:00".into()),
            end_time: Some("16:00".into()),
        }),
        json!({"id": "o", "reason": "custom_hours", "start_time": "12:00", "end_time": "16:00"})
    );
    assert_eq!(
        params(R::DeleteOverrideGroup {
            group_id: "g".into()
        }),
        json!({"groupId": "g"})
    );
    assert_eq!(
        params(R::Bookings(SchedulingBookingQuery {
            status: Some("confirmed".into()),
            when: Some("upcoming".into()),
            event_type_slug: Some("intro".into()),
            limit: Some(25),
            offset: Some(0),
            all_hosts: true,
            order: Some("asc".into()),
            ..SchedulingBookingQuery::default()
        })),
        json!({"status": "confirmed", "when": "upcoming", "event_type": "intro", "limit": 25,
               "offset": 0, "scope": "all", "order": "asc"})
    );
    assert_eq!(
        params(R::Bookings(SchedulingBookingQuery::default())),
        json!({}),
        "no scope unless the whole tenancy's are asked for"
    );
    assert_eq!(
        params(R::CancelBooking {
            id: "b".into(),
            reason: None
        }),
        json!({"id": "b"})
    );
    assert_eq!(
        params(R::CancelBooking {
            id: "b".into(),
            reason: Some("Sick".into())
        }),
        json!({"id": "b", "reason": "Sick"})
    );
    assert_eq!(
        params(R::RescheduleBooking {
            id: "b".into(),
            start_at: "2026-09-14T13:00:00Z".into(),
        }),
        json!({"id": "b", "start_at": "2026-09-14T13:00:00Z"})
    );
    assert_eq!(
        params(R::ReassignBooking {
            id: "b".into(),
            host_id: "u".into(),
        }),
        json!({"id": "b", "host_id": "u"})
    );
    assert_eq!(
        params(R::ConnectCaldav {
            preset: Some("fastmail".into()),
            server_url: None,
            username: "me@example.com".into(),
            app_password: SchedulingSecret::new("app-pass"),
        }),
        json!({"preset": "fastmail", "username": "me@example.com", "app_password": "app-pass"})
    );
    for request in [
        R::Calendars {
            id: "c".into(),
            provider: "google".into(),
            account: Some("me@example.com".into()),
        },
        R::DeleteConnection {
            id: "c".into(),
            provider: "google".into(),
            account: Some("me@example.com".into()),
        },
    ] {
        assert_eq!(
            request.params(),
            json!({"id": "c", "provider": "google", "account": "me@example.com"})
        );
    }
    assert_eq!(
        params(R::SetCalendars {
            id: "c".into(),
            provider: "google".into(),
            account_email: Some("me@example.com".into()),
            calendars: vec![SchedulingCalendarSelection {
                id: "cal".into(),
                name: "Work".into(),
                check_conflicts: Some(true),
                ..SchedulingCalendarSelection::default()
            }],
        }),
        json!({"id": "c", "provider": "google", "account_email": "me@example.com",
               "calendars": [{"id": "cal", "name": "Work", "check_conflicts": true}]})
    );
    assert_eq!(
        params(R::SetDestination {
            id: "c".into(),
            provider: "caldav".into(),
            account_email: None,
        }),
        json!({"id": "c", "provider": "caldav"})
    );
    assert_eq!(
        params(R::Users {
            include_archived: Some(true)
        }),
        json!({"include_archived": true})
    );
    assert_eq!(
        params(R::CreateTeam {
            name: "Sales".into(),
            slug: None
        }),
        json!({"name": "Sales"})
    );
    assert_eq!(
        params(R::PatchTeam {
            id: "t".into(),
            name: Some("Sales".into()),
            slug: Some("sales".into()),
        }),
        json!({"id": "t", "name": "Sales", "slug": "sales"})
    );
    assert_eq!(
        params(R::AddTeamMember {
            id: "t".into(),
            user_id: "u".into(),
            routing_priority: Some(3),
        }),
        json!({"id": "t", "user_id": "u", "routing_priority": 3})
    );
    assert_eq!(
        params(R::PatchTeamMember {
            id: "t".into(),
            user_id: "u".into(),
            routing_priority: 0,
        }),
        json!({"id": "t", "userId": "u", "routing_priority": 0})
    );
    assert_eq!(
        params(R::RemoveTeamMember {
            id: "t".into(),
            user_id: "u".into(),
        }),
        json!({"id": "t", "userId": "u"})
    );
    assert_eq!(
        params(R::PatchBranding(SchedulingBrandingPatch {
            business_name: "Acme".into(),
            logo_height: 48,
            logo_opacity: 100,
            banner_opacity: 80,
            privacy_url: String::new(),
            terms_url: "https://example.com/terms".into(),
            fallback_locale: "en".into(),
        })),
        json!({"business_name": "Acme", "logo_height": 48, "logo_opacity": 100,
               "banner_opacity": 80, "privacy_url": "", "terms_url": "https://example.com/terms",
               "fallback_locale": "en"}),
        "every field, the empty ones too: the scheduler replaces them all"
    );
    assert_eq!(
        params(R::PatchLlm {
            enabled: Some(true),
            extra_instructions: None,
        }),
        json!({"enabled": true})
    );
    assert_eq!(
        params(R::CreateApiKey {
            name: "Zapier".into()
        }),
        json!({"name": "Zapier"})
    );
    assert_eq!(
        params(R::CreateWebhook {
            url: "https://example.com/hook".into(),
            events: vec!["booking.created".into()],
            fields: Some(vec!["attendee_name".into()]),
        }),
        json!({"url": "https://example.com/hook", "events": ["booking.created"],
               "fields": ["attendee_name"]})
    );
    assert_eq!(
        params(R::PatchWebhook {
            id: "w".into(),
            events: Some(vec!["booking.created".into()]),
            fields: None,
        }),
        json!({"id": "w", "events": ["booking.created"]})
    );
}

#[test]
fn an_upload_is_not_an_operation_and_its_bytes_are_not_printed() {
    let request = SchedulingAdminRequest::Upload(upload());
    assert_eq!(request.op(), None);
    let shown = format!("{request:?}");
    assert!(shown.contains("bytes: 3"), "{shown}");
    assert_eq!(SchedulingUploadTarget::Logo.as_str(), "logo");
    assert_eq!(SchedulingUploadTarget::Banner.as_str(), "banner");
    assert_eq!(SchedulingUploadTarget::Avatar.as_str(), "avatar");
    let caldav = SchedulingAdminRequest::ConnectCaldav {
        preset: None,
        server_url: None,
        username: "u".into(),
        app_password: SchedulingSecret::new("hunter2"),
    };
    assert!(!format!("{caldav:?}").contains("hunter2"));
    assert_eq!(caldav.op(), Some(SchedulingAdminOp::CalendarCaldavConnect));
}

// What each fixture is there to cover.

#[test]
fn the_refusals_are_a_kind_and_a_number_or_field_names() {
    let failure: SchedulingAdminFailure = decode("district-scheduling-admin-failure.json");
    assert!(!failure.ok);
    assert_eq!(
        (failure.failure.as_str(), failure.status),
        ("rejected", 403)
    );
    let invalid: SchedulingAdminErrorBody = decode("district-scheduling-admin-invalid-params.json");
    assert_eq!(invalid.error, "invalid_params");
    assert_eq!(
        invalid.fields.as_deref(),
        Some(["time_format".to_owned(), "week_start".to_owned()].as_slice())
    );
    let not_ready: SchedulingAdminErrorBody = decode("district-scheduling-admin-not-ready.json");
    assert_eq!(not_ready.error, "scheduling_not_ready");
    assert_eq!(not_ready.fields, None);
}

#[test]
fn the_rows_cover_a_full_and_a_sparse_shape() {
    let SchedulingAdminData::BookingPage(page) = read(
        &SchedulingAdminRequest::Bookings(SchedulingBookingQuery::default()),
        "district-scheduling-bookings.json",
    ) else {
        unreachable!()
    };
    let sparse = &page.items[1];
    assert!(sparse.event_type_slug.is_none() && sparse.host_name.is_none());
    assert!(page.items[0].attendees.is_some());
    let SchedulingAdminData::EventTypes(types) = read(
        &SchedulingAdminRequest::EventTypes,
        "district-scheduling-event-types.json",
    ) else {
        unreachable!()
    };
    assert_eq!(types[0].reminders(), [1440, 60]);
    assert_eq!(types[1].reminders, Some(None), "null, not left out");
    assert_eq!(types[1].slot_interval_minutes, None, "left out");
    assert_eq!(types[1].archived, Some(true));
    let SchedulingAdminData::Users(users) = read(
        &SchedulingAdminRequest::Users {
            include_archived: None,
        },
        "district-scheduling-users.json",
    ) else {
        unreachable!()
    };
    assert_eq!(users[0].teams().len(), 1);
    assert_eq!(users[1].teams, Some(None));
    assert!(users[1].archived);
    let SchedulingAdminData::CalendarStatus(status) = read(
        &SchedulingAdminRequest::CalendarStatus,
        "district-scheduling-calendar-status.json",
    ) else {
        unreachable!()
    };
    assert_eq!(status.providers().len(), 3);
    assert_eq!(status.unconfigured_providers(), ["microsoft"]);
}
