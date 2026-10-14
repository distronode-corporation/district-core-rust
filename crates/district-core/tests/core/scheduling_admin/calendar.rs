//! Calendars: the connections and their calendars, connecting through the
//! provider's consent page and through the CalDAV form, choosing calendars,
//! and disconnecting.

use district_api::{ApiError, ErrorDetail};
use district_core::scheduling_admin::{
    CaldavDialog, CaldavField, CaldavForm, CaldavPreset, CalendarDialog, CalendarEvent,
    CalendarPicker, CalendarSection, SectionLoad, WriteState,
};
use district_core::{Effect, Event, Model, SchedulingAdminEvent, SchedulingSection};
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingCaldavConnection,
    SchedulingCalendarSelections, SchedulingCalendarStatus, SchedulingOk, SchedulingSecret,
    SchedulingZoomStatus,
};

use super::{act, admin, answer, data, done, open, refused, sent, ticket_for};
use crate::support::server_error;

fn section(model: &Model) -> &CalendarSection {
    admin(model)
        .calendar
        .as_ref()
        .expect("the calendars are open")
}

fn calendar(model: &mut Model, event: CalendarEvent) -> Vec<Effect> {
    act(model, SchedulingAdminEvent::Calendar(event))
}

fn status() -> SchedulingCalendarStatus {
    data("district-scheduling-calendar-status.json")
}

fn selections() -> SchedulingAdminData {
    SchedulingAdminData::Calendars(
        data::<SchedulingCalendarSelections>("district-scheduling-calendars.json").calendars,
    )
}

/// The calendars open as `role`, read; the Google account's calendars read
/// and the CalDAV account's not.
fn read(role: &str) -> Model {
    let (mut model, effects) = open(role, SchedulingSection::Calendar);
    let zoom = SchedulingZoomStatus {
        configured: true,
        connected: false,
    };
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::ZoomStatus),
        done(SchedulingAdminData::ZoomStatus(zoom)),
    );
    let effects = answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::CalendarStatus),
        done(SchedulingAdminData::CalendarStatus(status())),
    );
    let requests = sent(&effects);
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].1,
        SchedulingAdminRequest::Calendars {
            id: "cal-conn-contract-1".into(),
            provider: "google".into(),
            account: Some("contract@example.com".into()),
        }
    );
    answer(&mut model, requests[0].0, done(selections()));
    answer(&mut model, requests[1].0, Err(server_error()));
    model
}

#[test]
fn each_account_says_what_it_checks_and_what_receives_bookings() {
    let model = read("viewer");
    let cards = section(&model).connections();
    assert_eq!(cards[0].provider, "Google Calendar");
    assert_eq!(cards[0].conflicts, "Contract Member");
    assert_eq!(cards[0].destination, "Contract Member");
    // Its calendars were not read: the account's own flags speak.
    assert_eq!(cards[1].provider, "CalDAV");
    assert_eq!(cards[1].conflicts, "Not checked");
    assert_eq!(cards[1].destination, "No calendar in this account");
    let section = section(&model);
    assert_eq!(
        section.sentence(),
        Some("Another account can be connected to check it for conflicts as well.")
    );
    assert_eq!(section.zoom_line(), Some(("Zoom", "Not connected")));
    let offered: Vec<&str> = section
        .offered_providers()
        .iter()
        .map(|(p, _)| *p)
        .collect();
    assert_eq!(
        offered,
        ["google"],
        "Microsoft is not set up on this scheduler"
    );
    assert_eq!(section.offered_providers()[0].1, "Connect Google Calendar");
    assert_eq!(
        section.connect_note(),
        "Finish in the browser, then come back and refresh this page."
    );
    assert_eq!(CalendarSection::labels()[4], "Choose calendars");
}

#[test]
fn the_other_sentences_and_a_failed_read() {
    let (mut model, effects) = open("agency", SchedulingSection::Calendar);
    assert_eq!(section(&model).sentence(), None);
    assert!(section(&model).connections().is_empty());
    assert_eq!(section(&model).zoom_line(), None);
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::ZoomStatus),
        Err(server_error()),
    );
    let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::CalendarStatus);
    answer(&mut model, ticket, refused("unavailable"));
    assert!(section(&model).status.failure().is_some());
    for (status, sentence) in [
        (
            SchedulingCalendarStatus::default(),
            "Calendar connections are not set up on this scheduler yet.",
        ),
        (
            SchedulingCalendarStatus {
                configured: true,
                ..SchedulingCalendarStatus::default()
            },
            "No calendar is connected yet, so nothing is checked for conflicts and bookings are \
             not written anywhere.",
        ),
    ] {
        let (mut model, effects) = open("agency", SchedulingSection::Calendar);
        let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::CalendarStatus);
        answer(
            &mut model,
            ticket,
            done(SchedulingAdminData::CalendarStatus(status.clone())),
        );
        assert_eq!(section(&model).sentence(), Some(sentence));
        if !status.configured {
            assert!(section(&model).offered_providers().is_empty());
            assert_eq!(
                section(&model).connect_note(),
                "Calendar connections are not set up on this scheduler yet."
            );
        } else {
            assert_eq!(section(&model).offered_providers().len(), 2);
        }
    }
}

#[test]
fn connecting_opens_the_providers_page_through_a_single_use_sign_in() {
    let mut model = read("viewer");
    assert!(calendar(&mut model, CalendarEvent::Connect("microsoft".into())).is_empty());
    let effects = calendar(&mut model, CalendarEvent::Connect("google".into()));
    let [Effect::SchedulingSignIn { ticket, next, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        next,
        concat!(
            "/v1/calendar/connect?provider=google&return_to=https%3A%2F%2F",
            "www.distronode.com",
            "%2Fdashboard%2Fdistrict%2Fscheduling%2Fcalendar"
        )
    );
    assert!(section(&model).connect.busy);
    assert!(calendar(&mut model, CalendarEvent::Connect("google".into())).is_empty());
    let link = "https://book.example.com/v1/auth/sso?token=single-use";
    let effects = model.update(Event::SchedulingSignInReady {
        ticket: *ticket,
        result: Ok(SchedulingSecret::new(link)),
    });
    let [Effect::OpenOneTimeUrl { url }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(url.expose(), link);
    assert!(!format!("{:?}", section(&model)).contains("single-use"));
    assert_eq!(section(&model).connect.opened.as_deref(), Some("google"));
    // A refresh reads what is connected now.
    let effects = model.update(Event::Refresh);
    assert_eq!(sent(&effects).len(), 2);
}

fn sign_in_failure(result: Result<SchedulingSecret, ApiError>) -> String {
    let mut model = read("agency");
    let effects = calendar(&mut model, CalendarEvent::Connect("google".into()));
    let ticket = effects[0].ticket().unwrap();
    assert!(
        model
            .update(Event::SchedulingSignInReady { ticket, result })
            .is_empty()
    );
    let connect = &section(&model).connect;
    assert!(!connect.busy && connect.opened.is_none());
    let message = connect.failure.as_ref().unwrap().message.clone();
    calendar(&mut model, CalendarEvent::DismissConnectFailure);
    assert!(section(&model).connect.failure.is_none());
    message
}

#[test]
fn a_sign_in_that_fails_says_why_and_a_link_that_is_not_https_is_not_opened() {
    assert_eq!(
        sign_in_failure(Ok(SchedulingSecret::new("http://book.example.com/sso"))),
        "District AI sent a response this version of the app does not understand. Updating the app should fix it."
    );
    assert_eq!(
        sign_in_failure(Err(ApiError::Conflict(ErrorDetail::default()))),
        "Scheduling is still being set up for this workspace. Try again shortly."
    );
    assert!(!sign_in_failure(Err(server_error())).is_empty());
}

#[test]
fn a_disconnect_is_asked_once_more_and_says_when_nothing_receives_bookings() {
    let mut model = read("viewer");
    calendar(&mut model, CalendarEvent::AskDisconnect("nobody".into()));
    assert_eq!(section(&model).disconnect.asking, None);
    assert!(calendar(&mut model, CalendarEvent::ConfirmDisconnect).is_empty());
    calendar(
        &mut model,
        CalendarEvent::AskDisconnect("cal-conn-contract-1".into()),
    );
    assert_eq!(
        section(&model).disconnect_prompt().as_deref(),
        Some("Disconnect contract@example.com?")
    );
    calendar(&mut model, CalendarEvent::CancelDisconnect);
    assert_eq!(section(&model).disconnect_prompt(), None);
    calendar(
        &mut model,
        CalendarEvent::AskDisconnect("cal-conn-contract-1".into()),
    );
    let effects = calendar(&mut model, CalendarEvent::ConfirmDisconnect);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::DeleteConnection {
            id: "cal-conn-contract-1".into(),
            provider: "google".into(),
            account: Some("contract@example.com".into()),
        }
    );
    calendar(
        &mut model,
        CalendarEvent::AskDisconnect("cal-conn-contract-2".into()),
    );
    assert_eq!(section(&model).disconnect.asking, None, "one at a time");
    let ticket = *ticket;
    answer(&mut model, ticket, Err(server_error()));
    assert!(section(&model).disconnect.failure.is_some());
    calendar(&mut model, CalendarEvent::DismissDisconnect);
    calendar(
        &mut model,
        CalendarEvent::AskDisconnect("cal-conn-contract-1".into()),
    );
    let effects = calendar(&mut model, CalendarEvent::ConfirmDisconnect);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert!(section(&model).disconnect.removed_destination);
    assert_eq!(sent(&effects).len(), 2, "read again");
    assert_eq!(section(&model).status, SectionLoad::Loading);
}

fn picker(model: &Model) -> &CalendarPicker {
    match section(model).dialog.as_ref().expect("a dialog") {
        CalendarDialog::Calendars(picker) => picker,
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_accounts_calendars_are_chosen_and_saved_with_the_destination_after() {
    let mut model = read("viewer");
    assert!(calendar(&mut model, CalendarEvent::OpenCalendars("nobody".into())).is_empty());
    let effects = calendar(
        &mut model,
        CalendarEvent::OpenCalendars("cal-conn-contract-1".into()),
    );
    assert_eq!(picker(&model).title(), "Calendars in contract@example.com");
    assert!(!picker(&model).can_save());
    assert!(picker(&model).destination_choices().is_empty());
    assert!(calendar(&mut model, CalendarEvent::SaveCalendars).is_empty());
    answer(&mut model, sent(&effects)[0].0, done(selections()));
    assert_eq!(picker(&model).destination(), Some("contract@example.com"));
    assert_eq!(picker(&model).destination_choices().len(), 2);
    assert_eq!(CalendarPicker::labels()[0], "Checked for conflicts");
    calendar(
        &mut model,
        CalendarEvent::SetCheckConflicts {
            calendar_id: "holidays@example.com".into(),
            on: true,
        },
    );
    calendar(
        &mut model,
        CalendarEvent::ChooseDestination("holidays@example.com".into()),
    );
    let effects = calendar(&mut model, CalendarEvent::SaveCalendars);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::SetCalendars { calendars, .. })] = requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(calendars[1].check_conflicts, Some(true));
    assert_eq!(calendars[1].is_destination, Some(true));
    assert_eq!(calendars[0].is_destination, Some(false));
    assert!(
        calendar(&mut model, CalendarEvent::SaveCalendars).is_empty(),
        "one at a time"
    );
    let effects = answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::SetDestination { id, .. })] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(id, "cal-conn-contract-1");
    let ticket = *ticket;
    answer(&mut model, ticket, Err(server_error()));
    assert!(picker(&model).write.failure().is_some());
    let effects = calendar(&mut model, CalendarEvent::SaveCalendars);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert_eq!(picker(&model).write.notice(), Some("Calendars saved"));
    assert_eq!(sent(&effects).len(), 2, "the section is read again");
}

#[test]
fn calendars_with_no_destination_save_once_and_a_refusal_keeps_the_choice() {
    let mut model = read("agency");
    let effects = calendar(
        &mut model,
        CalendarEvent::OpenCalendars("cal-conn-contract-1".into()),
    );
    let mut rows =
        data::<SchedulingCalendarSelections>("district-scheduling-calendars.json").calendars;
    rows[0].is_destination = None;
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Calendars(rows)),
    );
    let effects = calendar(&mut model, CalendarEvent::SaveCalendars);
    answer(&mut model, sent(&effects)[0].0, refused("rejected"));
    assert!(picker(&model).write.failure().is_some());
    let effects = calendar(&mut model, CalendarEvent::SaveCalendars);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert_eq!(picker(&model).write.notice(), Some("Calendars saved"));
    assert_eq!(sent(&effects).len(), 2);
    // A picker whose read failed.
    let mut model = read("agency");
    let effects = calendar(
        &mut model,
        CalendarEvent::OpenCalendars("cal-conn-contract-2".into()),
    );
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(picker(&model).rows.failure().is_some());
    calendar(&mut model, CalendarEvent::CloseDialog);
    assert!(section(&model).dialog.is_none());
}

fn caldav(model: &Model) -> &CaldavDialog {
    match section(model).dialog.as_ref().expect("a dialog") {
        CalendarDialog::Caldav(caldav) => caldav,
        other => panic!("{other:?}"),
    }
}

fn form(preset: CaldavPreset, server: &str, user: &str, password: &str) -> CaldavForm {
    CaldavForm {
        preset,
        server_url: server.into(),
        username: user.into(),
        app_password: SchedulingSecret::new(password),
    }
}

#[test]
fn a_caldav_account_is_checked_field_by_field_then_connected() {
    let mut model = read("viewer");
    calendar(&mut model, CalendarEvent::OpenCaldav);
    assert_eq!(CaldavDialog::labels()[0], "Connect a CalDAV calendar");
    let presets: Vec<(&str, bool)> = CaldavPreset::ALL
        .iter()
        .map(|p| (p.label(), p.needs_server_url()))
        .collect();
    assert_eq!(
        presets,
        [
            ("iCloud", false),
            ("Fastmail", false),
            ("Nextcloud", true),
            ("Other server URL", true)
        ]
    );
    assert!(CaldavPreset::ALL.iter().all(|p| !p.note().is_empty()));
    let cases = [
        (
            form(CaldavPreset::Nextcloud, " ", "u", "p"),
            CaldavField::ServerUrl,
            "Enter the server URL.",
        ),
        (
            form(CaldavPreset::Custom, "http://dav.example.com", "u", "p"),
            CaldavField::ServerUrl,
            "The server address must start with https://. An app password sent over http:// travels in the clear.",
        ),
        (
            form(CaldavPreset::Icloud, "", " ", "p"),
            CaldavField::Username,
            "Enter the username.",
        ),
        (
            form(CaldavPreset::Icloud, "", "u", ""),
            CaldavField::AppPassword,
            "Enter the app password.",
        ),
    ];
    for (fields, field, message) in cases {
        calendar(&mut model, CalendarEvent::EditCaldav(fields));
        assert!(calendar(&mut model, CalendarEvent::ConnectCaldav).is_empty());
        assert_eq!(caldav(&model).refused, Some((field, message)));
    }
    calendar(
        &mut model,
        CalendarEvent::EditCaldav(form(
            CaldavPreset::Nextcloud,
            " https://dav.example.com ",
            " me ",
            "secret",
        )),
    );
    assert_eq!(caldav(&model).refused, None, "an edit clears it");
    let effects = calendar(&mut model, CalendarEvent::ConnectCaldav);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::ConnectCaldav {
            preset: None,
            server_url: Some("https://dav.example.com".into()),
            username: "me".into(),
            app_password: SchedulingSecret::new("secret"),
        }
    );
    assert!(!format!("{:?}", section(&model)).contains("secret"));
    assert!(
        calendar(&mut model, CalendarEvent::ConnectCaldav).is_empty(),
        "one at a time"
    );
    let ticket = *ticket;
    answer(&mut model, ticket, refused("rejected"));
    assert_eq!(
        caldav(&model).write.failure().unwrap().message,
        "Could not connect. Check the server, username and app password."
    );
    let effects = calendar(&mut model, CalendarEvent::ConnectCaldav);
    answer(&mut model, sent(&effects)[0].0, refused("unavailable"));
    assert_eq!(
        caldav(&model).write.failure().unwrap().message,
        "The booking system did not answer. Try again in a minute."
    );
    let effects = calendar(&mut model, CalendarEvent::ConnectCaldav);
    let connected = SchedulingCaldavConnection {
        connected: true,
        account_email: "me@example.com".into(),
    };
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::CaldavConnected(connected)),
    );
    assert_eq!(sent(&effects).len(), 2, "the section is read again");
    let dialog = caldav(&model);
    assert_eq!(
        dialog.connected_line().as_deref(),
        Some("Calendar connected: me@example.com")
    );
    assert_eq!(
        dialog.form.app_password.expose(),
        "",
        "the password is not kept"
    );
    assert_eq!(dialog.form.preset, CaldavPreset::Nextcloud);
    assert_eq!(dialog.write.notice(), Some("Calendar connected"));
    // An iCloud account sends its preset and no server.
    calendar(
        &mut model,
        CalendarEvent::EditCaldav(form(CaldavPreset::Icloud, "", "u", "p")),
    );
    let effects = calendar(&mut model, CalendarEvent::ConnectCaldav);
    let requests = sent(&effects);
    let [
        (
            _,
            SchedulingAdminRequest::ConnectCaldav {
                preset, server_url, ..
            },
        ),
    ] = requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(
        (preset.as_deref(), server_url.as_deref()),
        (Some("icloud"), None)
    );
    assert_eq!(caldav(&model).connected_line(), None);
}

#[test]
fn answers_for_a_dialog_that_closed_land_nowhere() {
    let mut model = read("agency");
    let effects = calendar(
        &mut model,
        CalendarEvent::OpenCalendars("cal-conn-contract-1".into()),
    );
    calendar(&mut model, CalendarEvent::CloseDialog);
    answer(&mut model, sent(&effects)[0].0, done(selections()));
    calendar(&mut model, CalendarEvent::SaveCalendars);
    calendar(&mut model, CalendarEvent::EditCaldav(CaldavForm::default()));
    assert!(section(&model).dialog.is_none());
    let (mut model, _) = open("agency", SchedulingSection::Overview);
    assert!(calendar(&mut model, CalendarEvent::OpenCaldav).is_empty());
    let _ = WriteState::Idle;
}

#[test]
fn the_remaining_summaries_and_choices_made_before_the_calendars_arrive() {
    use district_core::scheduling_admin::provider_label;
    use district_model::{SchedulingCalendarConnection, SchedulingCalendarSelection};
    assert_eq!(provider_label("apple"), "Apple Calendar");
    assert_eq!(provider_label("exchange"), "exchange");
    let wires: Vec<&str> = CaldavPreset::ALL.iter().map(|p| p.wire()).collect();
    assert_eq!(wires, ["icloud", "fastmail", "nextcloud", "custom"]);

    let connection = |id: &str, checks: bool| SchedulingCalendarConnection {
        id: id.into(),
        provider: "caldav".into(),
        account_email: format!("{id}@example.com"),
        is_destination: false,
        check_conflicts: checks,
    };
    let mut section = CalendarSection {
        status: SectionLoad::Ready(SchedulingCalendarStatus {
            configured: true,
            connections: vec![
                connection("a", true),
                connection("b", false),
                SchedulingCalendarConnection {
                    is_destination: true,
                    ..connection("c", false)
                },
            ],
            ..SchedulingCalendarStatus::default()
        }),
        zoom: Some(SchedulingZoomStatus {
            configured: true,
            connected: true,
        }),
        ..CalendarSection::default()
    };
    section.calendars.insert(
        "b".into(),
        vec![SchedulingCalendarSelection {
            id: "x".into(),
            name: "Home".into(),
            check_conflicts: Some(false),
            ..SchedulingCalendarSelection::default()
        }],
    );
    let cards = section.connections();
    assert_eq!(
        (cards[0].conflicts.as_str(), cards[0].destination.as_str()),
        ("Checked", "No calendar in this account")
    );
    assert_eq!(cards[1].conflicts, "None");
    assert_eq!(cards[2].destination, "This account");
    assert_eq!(section.zoom_line(), Some(("Zoom", "Connected")));

    // Choices made while the account's calendars are still on their way are
    // dropped.
    let mut model = read("agency");
    calendar(
        &mut model,
        CalendarEvent::OpenCalendars("cal-conn-contract-1".into()),
    );
    calendar(
        &mut model,
        CalendarEvent::SetCheckConflicts {
            calendar_id: "x".into(),
            on: true,
        },
    );
    calendar(&mut model, CalendarEvent::ChooseDestination("x".into()));
    assert!(picker(&model).rows.value().is_none());
}
