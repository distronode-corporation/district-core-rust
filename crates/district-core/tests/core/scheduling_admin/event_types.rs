//! Event types: the list and its create form, one event type's details, its
//! editor, its state actions, its hosts and its questions.

use district_core::l10n::{Locale, with_locale};
use district_core::scheduling_admin::{
    ActionsDialog, EventTypeDialog, EventTypeEditor, EventTypeEvent, EventTypeForm,
    EventTypeSection, EventTypesEvent, EventTypesSection, HostsDialog, QuestionForm,
    QuestionsDialog, SectionLoad, WriteState, location_label, minutes_label, slug_from_name,
    state_label, unique_slug,
};
use district_core::{Effect, Event, Model, Route, SchedulingAdminEvent, SchedulingSection};
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingEventType, SchedulingEventTypeFields,
    SchedulingHost, SchedulingHostAssignment, SchedulingItems, SchedulingOk, SchedulingQuestion,
    SchedulingQuestionFields, SchedulingTestEmail, SchedulingUser,
};

use super::{
    act, admin, answer, data, done, open, ready_status, refused, sent, status_answer, ticket_for,
};
use crate::support::{server_error, signed_in};

const SLUG: &str = "phone-consultation";

fn types() -> Vec<SchedulingEventType> {
    data::<SchedulingItems<_>>("district-scheduling-event-types.json").items
}

fn list(model: &Model) -> &EventTypesSection {
    admin(model).event_types.as_ref().expect("the list is open")
}

fn listed(role: &str) -> Model {
    let (mut model, effects) = open(role, SchedulingSection::EventTypes);
    status_answer(&mut model, &effects, Ok(ready_status()));
    let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::EventTypes);
    answer(
        &mut model,
        ticket,
        done(SchedulingAdminData::EventTypes(types())),
    );
    model
}

fn on_list(model: &mut Model, event: EventTypesEvent) -> Vec<Effect> {
    act(model, SchedulingAdminEvent::EventTypes(event))
}

#[test]
fn the_list_reads_each_types_length_place_state_and_link() {
    let model = listed("agency");
    let rows = list(&model).rows();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].subtitle, "30 min \u{b7} Phone");
    assert_eq!(rows[0].state, "Active");
    assert_eq!(
        rows[0].booking_page,
        "https://book.example.com/book/phone-consultation"
    );
    assert_eq!(rows[1].subtitle, "60 min \u{b7} In person");
    assert_eq!(rows[1].state, "Archived");
    assert_eq!(rows[1].booking_page, "Not public");
    assert_eq!(EventTypesSection::empty_title(), "No event types yet");
    assert!(EventTypesSection::empty_body().starts_with("An event type is a page"));
    assert_eq!(EventTypesSection::create_label(), "Create event type");
}

#[test]
fn without_a_host_no_type_has_a_link_and_a_failed_read_is_its_own_state() {
    let (mut model, effects) = open("agency", SchedulingSection::EventTypes);
    status_answer(&mut model, &effects, Err(server_error()));
    let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::EventTypes);
    assert!(list(&model).rows().is_empty());
    answer(
        &mut model,
        ticket,
        done(SchedulingAdminData::EventTypes(types())),
    );
    assert_eq!(list(&model).rows()[0].booking_page, "Not public");

    let (mut model, effects) = open("agency", SchedulingSection::EventTypes);
    let ticket = ticket_for(&effects, |r| *r == SchedulingAdminRequest::EventTypes);
    answer(&mut model, ticket, refused("unavailable"));
    assert!(list(&model).list.failure().is_some());
}

#[test]
fn the_labels_read_each_kind_of_value() {
    assert_eq!(minutes_label(None), "Not set");
    let labels: Vec<String> = [
        None,
        Some("google_meet"),
        Some("teams"),
        Some("custom_video"),
        Some("link"),
        Some("livekit"),
        Some("zoom"),
        Some(""),
    ]
    .into_iter()
    .map(location_label)
    .collect();
    assert_eq!(
        labels,
        [
            "Not set",
            "Google Meet",
            "Microsoft Teams",
            "Video link",
            "Link",
            "Built-in video",
            "zoom",
            "Not set"
        ]
    );
    let inactive = SchedulingEventType {
        is_active: Some(false),
        ..SchedulingEventType::default()
    };
    assert_eq!(state_label(&inactive).0, "Inactive");
}

#[test]
fn a_slug_is_made_from_the_name_as_the_web_makes_it() {
    assert_eq!(slug_from_name("Phone consultation"), "phone-consultation");
    assert_eq!(slug_from_name("  Caf\u{e9} & cr\u{e8}me!  "), "cafe-creme");
    assert_eq!(slug_from_name("\u{fb01}ne \u{ff21}\u{ff22}"), "fine-ab");
    assert_eq!(slug_from_name("\u{fb00}\u{fb02}"), "fffl");
    assert_eq!(slug_from_name("\u{152}uvre"), "uvre");
    assert_eq!(slug_from_name("!!!"), "event-type");
    let long = format!("{} b", "a".repeat(199));
    assert_eq!(
        slug_from_name(&long),
        "a".repeat(199),
        "no hyphen left at the cut"
    );
    let taken = vec!["intro".to_owned(), "intro-2".to_owned()];
    assert_eq!(unique_slug("Intro", &taken), "intro-3");
    assert_eq!(unique_slug("Other", &taken), "other");
}

fn editor(model: &Model) -> &EventTypeEditor {
    list(model).editor.as_ref().expect("the form is open")
}

#[test]
fn a_create_sends_a_unique_slug_the_name_the_duration_and_the_location() {
    let mut model = listed("agency");
    on_list(&mut model, EventTypesEvent::OpenCreate);
    let form = editor(&model);
    assert!(form.is_creating());
    assert_eq!(form.title(), "Create event type");
    assert_eq!(
        form.location_value_label(),
        None,
        "built-in video makes its own link"
    );
    assert_eq!(form.location_choices().len(), 4);
    assert!(on_list(&mut model, EventTypesEvent::Save).is_empty());
    assert_eq!(
        editor(&model).validation,
        Some("Give the event type a name.")
    );
    let mut fields = EventTypeForm {
        name: "Phone consultation".into(),
        duration: "0".into(),
        location: "phone".into(),
        ..EventTypeForm::default()
    };
    on_list(
        &mut model,
        EventTypesEvent::EditForm(Box::new(fields.clone())),
    );
    assert!(on_list(&mut model, EventTypesEvent::Save).is_empty());
    assert_eq!(
        editor(&model).validation,
        Some("The duration has to be a whole number of minutes, at least 1.")
    );
    assert_eq!(editor(&model).location_value_label(), Some("Phone number"));
    fields.duration = " 45 ".into();
    on_list(&mut model, EventTypesEvent::EditForm(Box::new(fields)));
    let effects = on_list(&mut model, EventTypesEvent::Save);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::CreateEventType {
            slug: "phone-consultation-2".into(),
            fields: SchedulingEventTypeFields {
                name: Some("Phone consultation".into()),
                duration_minutes: Some(45),
                location_type: Some("phone".into()),
                ..SchedulingEventTypeFields::default()
            },
        }
    );
    assert!(
        on_list(&mut model, EventTypesEvent::Save).is_empty(),
        "one at a time"
    );
    let ticket = *ticket;
    answer(&mut model, ticket, refused("conflict"));
    assert!(editor(&model).write.failure().is_some());
    let effects = on_list(&mut model, EventTypesEvent::Save);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::EventType(Box::new(types()[0].clone()))),
    );
    assert!(list(&model).editor.is_none());
    assert_eq!(
        sent(&effects)
            .into_iter()
            .map(|(_, r)| r)
            .collect::<Vec<_>>(),
        [SchedulingAdminRequest::EventTypes]
    );
}

#[test]
fn a_viewer_cannot_open_the_create_form_and_it_closes() {
    let mut model = listed("viewer");
    on_list(&mut model, EventTypesEvent::OpenCreate);
    assert!(list(&model).editor.is_none());
    on_list(&mut model, EventTypesEvent::EditForm(Box::default()));
    assert!(on_list(&mut model, EventTypesEvent::Save).is_empty());

    let (mut model, _) = open("agency", SchedulingSection::EventTypes);
    on_list(&mut model, EventTypesEvent::OpenCreate);
    assert!(list(&model).editor.is_none(), "not before the list is read");

    let mut model = listed("agency");
    on_list(&mut model, EventTypesEvent::OpenCreate);
    on_list(&mut model, EventTypesEvent::CloseEditor);
    assert!(list(&model).editor.is_none());
}

// One event type.

fn section(model: &Model) -> &EventTypeSection {
    admin(model)
        .event_type
        .as_ref()
        .expect("an event type is open")
}

fn one(model: &mut Model, event: EventTypeEvent) -> Vec<Effect> {
    act(model, SchedulingAdminEvent::EventType(event))
}

fn hosts() -> Vec<SchedulingHost> {
    data::<SchedulingItems<_>>("district-scheduling-hosts.json").items
}

fn questions() -> Vec<SchedulingQuestion> {
    data::<SchedulingItems<_>>("district-scheduling-questions.json").items
}

/// One event type open as `role`, its three reads answered.
fn opened(role: &str) -> Model {
    let (mut model, effects) = open(role, SchedulingSection::EventType { slug: SLUG.into() });
    assert_eq!(section(&model).title(), SLUG);
    let slug = || SLUG.to_owned();
    let replies = [
        (
            SchedulingAdminRequest::EventType { slug: slug() },
            SchedulingAdminData::EventType(Box::new(types()[0].clone())),
        ),
        (
            SchedulingAdminRequest::Hosts { slug: slug() },
            SchedulingAdminData::Hosts(hosts()),
        ),
        (
            SchedulingAdminRequest::Questions { slug: slug() },
            SchedulingAdminData::Questions(questions()),
        ),
    ];
    for (request, reply) in replies {
        answer(
            &mut model,
            ticket_for(&effects, |r| *r == request),
            done(reply),
        );
    }
    model
}

#[test]
fn an_event_type_shows_its_details_hosts_and_questions_in_order() {
    let model = opened("agency");
    let section = section(&model);
    assert_eq!(section.title(), "Phone consultation");
    let facts: Vec<(&str, String)> = section.facts();
    let facts: Vec<(&str, &str)> = facts.iter().map(|(l, v)| (*l, v.as_str())).collect();
    assert_eq!(
        facts,
        [
            ("Duration", "30 min"),
            ("Starts every", "15 min"),
            ("Location", "Phone"),
            ("Address", "+14165550142"),
            ("Minimum notice", "120 min"),
            ("Bookable up to", "60 days ahead"),
            ("Buffer before", "5 min"),
            ("Buffer after", "10 min"),
            ("State", "Active"),
        ]
    );
    assert_eq!(
        section.host_rows(),
        [
            ("Contract Member".to_owned(), "Required".to_owned()),
            (
                "Rotation Host".to_owned(),
                "Rotation \u{b7} archived".to_owned()
            ),
        ]
    );
    assert_eq!(
        section.question_rows(),
        [
            (
                "Which product are you asking about?".to_owned(),
                "Select \u{b7} required".to_owned()
            ),
            (
                "Anything we should read first?".to_owned(),
                "Text".to_owned()
            ),
        ]
    );
    assert_eq!(
        EventTypeSection::headings(),
        ["Details", "Hosts", "Questions"]
    );
    assert_eq!(EventTypeSection::empties().len(), 2);
    assert_eq!(
        EventTypeSection::write_labels(),
        ["Edit", "Manage", "Hosts", "Questions"]
    );
}

#[test]
fn a_sparse_event_type_shows_only_what_it_has_and_a_blank_host_by_address() {
    let (mut model, effects) = open("agency", SchedulingSection::EventType { slug: "s".into() });
    assert!(section(&model).facts().is_empty());
    assert!(section(&model).host_rows().is_empty());
    assert!(section(&model).question_rows().is_empty());
    let sparse = SchedulingEventType {
        slug: "s".into(),
        name: "S".into(),
        duration_minutes: 20,
        buffer_before_minutes: Some(0),
        ..SchedulingEventType::default()
    };
    let host = SchedulingHost {
        email: "ann@example.com".into(),
        role: "required".into(),
        ..SchedulingHost::default()
    };
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::EventType { .. })
        }),
        done(SchedulingAdminData::EventType(Box::new(sparse))),
    );
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::Hosts { .. })
        }),
        done(SchedulingAdminData::Hosts(vec![host])),
    );
    let facts: Vec<&str> = section(&model).facts().iter().map(|(l, _)| *l).collect();
    assert_eq!(facts, ["Duration", "Location", "State"]);
    assert_eq!(section(&model).host_rows()[0].0, "ann@example.com");
}

#[test]
fn host_roles_and_question_types_read_as_their_labels_in_french() {
    let model = opened("agency");
    let section = section(&model);
    let (hosts, questions) = with_locale(Locale::FrCa, || {
        (section.host_rows(), section.question_rows())
    });
    let kinds: Vec<&str> = hosts.iter().map(|(_, role)| role.as_str()).collect();
    assert_eq!(
        kinds,
        ["Obligatoire", "Rotation \u{b7} compte archiv\u{e9}"]
    );
    let kinds: Vec<&str> = questions.iter().map(|(_, kind)| kind.as_str()).collect();
    assert_eq!(kinds, ["Liste d\u{e9}roulante \u{b7} obligatoire", "Texte"]);
}

#[test]
fn a_role_or_question_type_this_build_does_not_know_never_reads_as_itself() {
    let (mut model, effects) = open("agency", SchedulingSection::EventType { slug: "s".into() });
    let host = SchedulingHost {
        email: "ann@example.com".into(),
        role: "cohost".into(),
        archived: true,
        ..SchedulingHost::default()
    };
    let question = SchedulingQuestion {
        label: "Pick a day".into(),
        kind: "date".into(),
        required: true,
        ..SchedulingQuestion::default()
    };
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::Hosts { .. })
        }),
        done(SchedulingAdminData::Hosts(vec![host])),
    );
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::Questions { .. })
        }),
        done(SchedulingAdminData::Questions(vec![question])),
    );
    let section = section(&model);
    assert_eq!(section.host_rows()[0].1, "Other \u{b7} archived");
    assert_eq!(section.question_rows()[0].1, "Other \u{b7} required");
    with_locale(Locale::FrCa, || {
        assert_eq!(section.host_rows()[0].1, "Autre \u{b7} compte archiv\u{e9}");
        assert_eq!(section.question_rows()[0].1, "Autre \u{b7} obligatoire");
    });
}

fn dialog(model: &Model) -> &EventTypeDialog {
    section(model).dialog.as_ref().expect("a dialog")
}

#[test]
fn the_dialogs_open_for_a_member_who_may_change_things_once_the_type_is_read() {
    let mut viewer = opened("viewer");
    for event in [
        EventTypeEvent::OpenEditor,
        EventTypeEvent::OpenActions,
        EventTypeEvent::OpenHosts,
        EventTypeEvent::OpenQuestions,
    ] {
        assert!(one(&mut viewer, event).is_empty());
        assert!(section(&viewer).dialog.is_none());
    }
    let (mut model, _) = open("agency", SchedulingSection::EventType { slug: SLUG.into() });
    one(&mut model, EventTypeEvent::OpenEditor);
    assert!(section(&model).dialog.is_none(), "not read yet");
    // With no dialog, a dialog's event does nothing.
    let mut model = opened("agency");
    for event in [
        EventTypeEvent::Save,
        EventTypeEvent::DismissNotice,
        EventTypeEvent::SaveHosts,
    ] {
        assert!(one(&mut model, event).is_empty());
    }
}

fn edit_form(model: &Model) -> &EventTypeEditor {
    match dialog(model) {
        EventTypeDialog::Edit(editor) => editor,
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_edit_sends_every_field_and_reads_the_event_type_again() {
    let mut model = opened("agency");
    one(&mut model, EventTypeEvent::OpenEditor);
    let editor = edit_form(&model);
    assert!(!editor.is_creating());
    assert_eq!(editor.title(), "Edit event type");
    assert_eq!(editor.form.interval, "15");
    assert_eq!(EventTypeEditor::labels().len(), 17);
    assert_eq!(EventTypeEditor::count_labels()[3].1, "0 means no limit.");
    let mut form = editor.form.clone();
    form.max_active_bookings = "-1".into();
    one(&mut model, EventTypeEvent::EditForm(form.clone()));
    assert!(one(&mut model, EventTypeEvent::Save).is_empty());
    assert_eq!(
        edit_form(&model).validation,
        Some("Has to be a whole number, 0 or more.")
    );
    form.interval = "".into();
    one(&mut model, EventTypeEvent::EditForm(form.clone()));
    one(&mut model, EventTypeEvent::Save);
    assert_eq!(
        edit_form(&model).validation,
        Some("Has to be a whole number of minutes, at least 1.")
    );
    form.name = " ".into();
    one(&mut model, EventTypeEvent::EditForm(form.clone()));
    one(&mut model, EventTypeEvent::Save);
    assert_eq!(
        edit_form(&model).validation,
        Some("Give the event type a name.")
    );
    one(&mut model, EventTypeEvent::DismissNotice);
    assert_eq!(edit_form(&model).validation, None);
    form.name = "Intro call".into();
    form.interval = "30".into();
    form.max_active_bookings = "2".into();
    form.location_value = String::new();
    one(&mut model, EventTypeEvent::EditForm(form));
    let effects = one(&mut model, EventTypeEvent::Save);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::PatchEventType { slug, fields })] = requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(slug, SLUG);
    assert_eq!(fields.name.as_deref(), Some("Intro call"));
    assert_eq!(
        fields.location_value.as_deref(),
        Some(""),
        "a clear is sent"
    );
    assert_eq!(fields.max_active_bookings, Some(2));
    assert_eq!(fields.is_public, Some(true));
    let ticket = *ticket;
    answer(&mut model, ticket, Err(server_error()));
    assert!(edit_form(&model).write.failure().is_some());
    let effects = one(&mut model, EventTypeEvent::Save);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::EventType(Box::new(types()[0].clone()))),
    );
    assert!(section(&model).dialog.is_none());
    assert_eq!(sent(&effects).len(), 3, "the three reads again");
    assert_eq!(section(&model).event_type, SectionLoad::Loading);
}

#[test]
fn a_stored_location_the_form_does_not_offer_stays_on_offer() {
    let row = SchedulingEventType {
        slug: SLUG.into(),
        duration_minutes: 30,
        location_type: Some("google_meet".into()),
        ..SchedulingEventType::default()
    };
    let (mut model, effects) = open("agency", SchedulingSection::EventType { slug: SLUG.into() });
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::EventType { .. })
        }),
        done(SchedulingAdminData::EventType(Box::new(row))),
    );
    one(&mut model, EventTypeEvent::OpenEditor);
    let choices = edit_form(&model).location_choices();
    assert_eq!(
        choices.last().unwrap(),
        &("google_meet".to_owned(), "google_meet".to_owned())
    );
    let mut form = edit_form(&model).form.clone();
    form.location = "link".into();
    one(&mut model, EventTypeEvent::EditForm(form.clone()));
    assert_eq!(
        edit_form(&model).location_value_label(),
        Some("Meeting link")
    );
    form.location = "in_person".into();
    one(&mut model, EventTypeEvent::EditForm(form.clone()));
    assert_eq!(edit_form(&model).location_value_label(), Some("Address"));
    form.location = "carrier_pigeon".into();
    one(&mut model, EventTypeEvent::EditForm(form));
    assert_eq!(edit_form(&model).location_value_label(), None);
    // A form for a type that set nothing reads the scheduler's defaults.
    let defaults = EventTypeForm::from_event_type(&SchedulingEventType {
        duration_minutes: 45,
        ..SchedulingEventType::default()
    });
    assert_eq!(
        (defaults.interval.as_str(), defaults.location.as_str()),
        ("45", "livekit")
    );
    assert!(defaults.is_active && defaults.is_public && !defaults.show_taken_slots);
}

fn actions(model: &Model) -> &ActionsDialog {
    match dialog(model) {
        EventTypeDialog::Actions(actions) => actions,
        other => panic!("{other:?}"),
    }
}

#[test]
fn turning_off_and_archiving_say_so_and_read_the_type_again() {
    let mut model = opened("agency");
    one(&mut model, EventTypeEvent::OpenActions);
    assert_eq!(actions(&model).toggle_label(), "Turn off");
    assert_eq!(actions(&model).archive_label(), "Archive");
    let effects = one(&mut model, EventTypeEvent::ToggleActive);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::PatchEventType { fields, .. })] = requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(fields.is_active, Some(false));
    assert!(
        one(&mut model, EventTypeEvent::ToggleArchived).is_empty(),
        "one at a time"
    );
    let mut off = types()[0].clone();
    off.is_active = Some(false);
    let effects = answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::EventType(Box::new(off))),
    );
    assert_eq!(sent(&effects).len(), 3);
    assert_eq!(
        actions(&model).write.notice(),
        Some("Event type turned off")
    );
    assert_eq!(actions(&model).toggle_label(), "Turn on");
    let effects = one(&mut model, EventTypeEvent::ToggleActive);
    let mut on = types()[0].clone();
    on.archived = Some(true);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::EventType(Box::new(on))),
    );
    assert_eq!(actions(&model).write.notice(), Some("Event type turned on"));
    assert_eq!(actions(&model).archive_label(), "Restore");
    let effects = one(&mut model, EventTypeEvent::ToggleArchived);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::PatchEventType { fields, .. })] = requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(fields.archived, Some(false));
    answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::EventType(Box::new(types()[0].clone()))),
    );
    assert_eq!(actions(&model).write.notice(), Some("Event type restored"));
    let effects = one(&mut model, EventTypeEvent::ToggleArchived);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::EventType(Box::new(types()[0].clone()))),
    );
    assert_eq!(actions(&model).write.notice(), Some("Event type archived"));
    let effects = one(&mut model, EventTypeEvent::ToggleArchived);
    let effects = answer(&mut model, sent(&effects)[0].0, refused("rejected"));
    assert!(effects.is_empty());
    assert!(actions(&model).write.failure().is_some());
    one(&mut model, EventTypeEvent::DismissNotice);
    assert_eq!(actions(&model).write, WriteState::Idle);
}

#[test]
fn a_test_email_names_where_it_went() {
    let mut model = opened("agency");
    one(&mut model, EventTypeEvent::OpenActions);
    assert_eq!(
        ActionsDialog::test_emails()[0],
        ("confirmation", "Confirmation")
    );
    assert_eq!(ActionsDialog::test_email_heading(), "Send test email");
    let effects = one(&mut model, EventTypeEvent::SendTestEmail("reminder".into()));
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::SendTestEmail {
            slug: SLUG.into(),
            kind: "reminder".into()
        }
    );
    let sent_to = SchedulingTestEmail {
        sent: true,
        to: "ann@example.com".into(),
    };
    assert!(
        answer(
            &mut model,
            *ticket,
            done(SchedulingAdminData::TestEmail(sent_to))
        )
        .is_empty()
    );
    assert_eq!(
        actions(&model).write.notice(),
        Some("Test email sent to ann@example.com")
    );
    let effects = one(&mut model, EventTypeEvent::SendTestEmail("reminder".into()));
    let not_sent = SchedulingTestEmail {
        sent: false,
        to: String::new(),
    };
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::TestEmail(not_sent)),
    );
    assert_eq!(
        actions(&model).write.notice(),
        Some("The booking system did not send the test email.")
    );
}

#[test]
fn a_delete_is_asked_once_more_and_goes_back_to_the_list() {
    let mut model = opened("agency");
    one(&mut model, EventTypeEvent::OpenActions);
    assert!(
        one(&mut model, EventTypeEvent::ConfirmDelete).is_empty(),
        "not asked yet"
    );
    one(&mut model, EventTypeEvent::AskDelete);
    assert!(actions(&model).confirming_delete);
    one(&mut model, EventTypeEvent::CancelDelete);
    assert!(!actions(&model).confirming_delete);
    one(&mut model, EventTypeEvent::AskDelete);
    assert_eq!(ActionsDialog::delete_title(), "Delete event type?");
    assert_eq!(ActionsDialog::delete_label(), "Delete event type");
    assert!(ActionsDialog::delete_body().contains("Existing bookings keep their times"));
    let effects = one(&mut model, EventTypeEvent::ConfirmDelete);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::DeleteEventType { slug: SLUG.into() }
    );
    let ticket = *ticket;
    answer(&mut model, ticket, Err(server_error()));
    assert!(actions(&model).write.failure().is_some());
    assert!(
        one(&mut model, EventTypeEvent::ConfirmDelete).is_empty(),
        "asked again first"
    );
    one(&mut model, EventTypeEvent::AskDelete);
    let effects = one(&mut model, EventTypeEvent::ConfirmDelete);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert_eq!(
        signed_in(&model).route,
        Route::SchedulingAdmin(SchedulingSection::EventTypes)
    );
    assert!(admin(&model).event_type.is_none());
    assert!(
        sent(&effects)
            .iter()
            .any(|(_, r)| *r == SchedulingAdminRequest::EventTypes)
    );
}

fn hosts_dialog(model: &Model) -> &HostsDialog {
    match dialog(model) {
        EventTypeDialog::Hosts(hosts) => hosts,
        other => panic!("{other:?}"),
    }
}

fn users() -> Vec<SchedulingUser> {
    let user = |id: &str, name: &str, archived: bool| SchedulingUser {
        id: id.into(),
        name: name.into(),
        email: format!("{id}@example.com"),
        archived,
        ..SchedulingUser::default()
    };
    vec![
        user("sched-user-contract", "Contract Member", false),
        user("gone", "Gone", true),
        user("sam", "", false),
    ]
}

/// The hosts dialog open, its hosts and the users read.
fn hosts_open() -> Model {
    let mut model = opened("agency");
    let effects = one(&mut model, EventTypeEvent::OpenHosts);
    assert!(
        !hosts_dialog(&model).can_save(),
        "not before the hosts are read"
    );
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Hosts(hosts())),
    );
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::Users { .. })] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::Users(users())),
    );
    model
}

#[test]
fn the_hosts_are_edited_and_saved_whole_with_the_routing_after() {
    let mut model = hosts_open();
    let dialog = hosts_dialog(&model);
    assert_eq!(dialog.rows.len(), 2);
    assert_eq!(
        (dialog.routing_mode.as_str(), dialog.rotation.as_str()),
        ("fixed", "even")
    );
    assert!(!dialog.shows_rotation());
    let candidates: Vec<&str> = dialog.candidates.iter().map(|u| u.id.as_str()).collect();
    assert_eq!(candidates, ["sam"]);
    assert_eq!(HostsDialog::routing_modes()[1].1, "Round robin");
    assert_eq!(HostsDialog::rotations()[2].1, "Priority");
    assert_eq!(HostsDialog::roles()[0].1, "Required");
    assert_eq!(HostsDialog::labels()[0], "Hosts");
    one(
        &mut model,
        EventTypeEvent::SetRoutingMode("round_robin".into()),
    );
    one(&mut model, EventTypeEvent::SetRotation("priority".into()));
    assert!(hosts_dialog(&model).shows_rotation());
    one(&mut model, EventTypeEvent::AddHost("nobody".into()));
    one(&mut model, EventTypeEvent::AddHost("sam".into()));
    let added = &hosts_dialog(&model).rows[2];
    assert_eq!(
        (added.name.as_str(), added.role.as_str()),
        ("sam@example.com", "required")
    );
    one(
        &mut model,
        EventTypeEvent::RemoveHost("sched-user-rotation".into()),
    );
    one(
        &mut model,
        EventTypeEvent::SetHostRole {
            user_id: "sam".into(),
            role: "rotation".into(),
        },
    );
    one(
        &mut model,
        EventTypeEvent::SetHostPriority {
            user_id: "sam".into(),
            priority: "x".into(),
        },
    );
    one(
        &mut model,
        EventTypeEvent::SetHostPriority {
            user_id: "nobody".into(),
            priority: "1".into(),
        },
    );
    assert!(one(&mut model, EventTypeEvent::SaveHosts).is_empty());
    assert_eq!(
        hosts_dialog(&model).validation,
        Some("A host's priority has to be a whole number, 0 or more.")
    );
    one(
        &mut model,
        EventTypeEvent::SetHostPriority {
            user_id: "sam".into(),
            priority: "3".into(),
        },
    );
    let effects = one(&mut model, EventTypeEvent::SaveHosts);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::SetHosts { hosts, .. })] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *hosts,
        [
            SchedulingHostAssignment {
                user_id: "sched-user-contract".into(),
                role: "required".into(),
                priority: 0
            },
            SchedulingHostAssignment {
                user_id: "sam".into(),
                role: "rotation".into(),
                priority: 3
            },
        ]
    );
    assert!(
        one(&mut model, EventTypeEvent::SaveHosts).is_empty(),
        "one at a time"
    );
    let effects = answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::Hosts(Vec::new())),
    );
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::PatchEventType { fields, .. })] = requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(fields.routing_mode.as_deref(), Some("round_robin"));
    assert_eq!(fields.rr_strategy.as_deref(), Some("priority"));
    let effects = answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::EventType(Box::new(types()[0].clone()))),
    );
    assert!(section(&model).dialog.is_none());
    assert_eq!(sent(&effects).len(), 3);
}

#[test]
fn hosts_are_not_saved_empty_and_each_half_of_a_save_can_fail() {
    let mut model = hosts_open();
    for id in ["sched-user-contract", "sched-user-rotation"] {
        one(&mut model, EventTypeEvent::RemoveHost(id.into()));
    }
    assert!(one(&mut model, EventTypeEvent::SaveHosts).is_empty());
    assert_eq!(
        hosts_dialog(&model).validation,
        Some("An event type needs at least one host.")
    );
    one(&mut model, EventTypeEvent::AddHost("sam".into()));
    let effects = one(&mut model, EventTypeEvent::SaveHosts);
    answer(&mut model, sent(&effects)[0].0, refused("rejected"));
    assert!(hosts_dialog(&model).write.failure().is_some());
    one(&mut model, EventTypeEvent::DismissNotice);
    let effects = one(&mut model, EventTypeEvent::SaveHosts);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Hosts(Vec::new())),
    );
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(hosts_dialog(&model).write.failure().is_some());
}

#[test]
fn hosts_that_cannot_be_read_offer_no_save_and_users_that_cannot_add_nobody() {
    let mut model = opened("agency");
    let effects = one(&mut model, EventTypeEvent::OpenHosts);
    answer(&mut model, sent(&effects)[0].0, refused("unavailable"));
    assert!(hosts_dialog(&model).load.failure().is_some());
    assert!(one(&mut model, EventTypeEvent::SaveHosts).is_empty());

    let mut model = opened("agency");
    let effects = one(&mut model, EventTypeEvent::OpenHosts);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Hosts(hosts())),
    );
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(hosts_dialog(&model).candidates.is_empty());
    assert!(hosts_dialog(&model).can_save());
    // Answers for a dialog that closed land nowhere.
    let effects = one(&mut model, EventTypeEvent::SaveHosts);
    one(&mut model, EventTypeEvent::CloseDialog);
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(section(&model).dialog.is_none());
}

fn questions_dialog(model: &Model) -> &QuestionsDialog {
    match dialog(model) {
        EventTypeDialog::Questions(questions) => questions,
        other => panic!("{other:?}"),
    }
}

fn questions_open() -> Model {
    let mut model = opened("agency");
    let effects = one(&mut model, EventTypeEvent::OpenQuestions);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Questions(
            questions().into_iter().rev().collect(),
        )),
    );
    model
}

#[test]
fn a_question_is_added_last_with_its_choices_and_the_list_read_again() {
    let mut model = questions_open();
    let dialog = questions_dialog(&model);
    let labels: Vec<&str> = dialog
        .questions
        .value()
        .unwrap()
        .iter()
        .map(|q| q.label.as_str())
        .collect();
    assert_eq!(
        labels[0], "Which product are you asking about?",
        "by position"
    );
    assert_eq!(QuestionsDialog::kinds()[1], ("select", "Select"));
    assert_eq!(QuestionsDialog::labels()[0], "Questions");
    one(&mut model, EventTypeEvent::AddQuestion);
    assert_eq!(questions_dialog(&model).editor_title(), "Add question");
    let form = questions_dialog(&model).editing.clone().unwrap();
    assert_eq!((form.kind.as_str(), form.position.as_str()), ("text", "2"));
    one(&mut model, EventTypeEvent::SaveQuestion);
    assert_eq!(
        questions_dialog(&model).validation,
        Some("Give the question a label.")
    );
    let select = QuestionForm {
        label: " Which room? ".into(),
        kind: "select".into(),
        ..form
    };
    one(&mut model, EventTypeEvent::EditQuestionForm(select.clone()));
    one(&mut model, EventTypeEvent::SaveQuestion);
    assert_eq!(
        questions_dialog(&model).validation,
        Some("A Select question needs at least one option.")
    );
    for option in [" A ", "A", " ", "B"] {
        one(&mut model, EventTypeEvent::AddOption(option.into()));
    }
    one(&mut model, EventTypeEvent::RemoveOption("B".into()));
    assert_eq!(
        questions_dialog(&model).editing.as_ref().unwrap().options,
        ["A"]
    );
    let mut current = questions_dialog(&model).editing.clone().unwrap();
    current.position = "first".into();
    one(
        &mut model,
        EventTypeEvent::EditQuestionForm(current.clone()),
    );
    one(&mut model, EventTypeEvent::SaveQuestion);
    assert_eq!(
        questions_dialog(&model).validation,
        Some("Position has to be a whole number, 0 or more.")
    );
    current.position = "2".into();
    one(&mut model, EventTypeEvent::EditQuestionForm(current));
    let effects = one(&mut model, EventTypeEvent::SaveQuestion);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::CreateQuestion {
            slug: SLUG.into(),
            fields: SchedulingQuestionFields {
                label: Some("Which room?".into()),
                kind: Some("select".into()),
                options: Some(vec!["A".into()]),
                required: Some(false),
                position: Some(2),
            },
        }
    );
    assert!(
        one(&mut model, EventTypeEvent::SaveQuestion).is_empty(),
        "one at a time"
    );
    let effects = answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::Question(questions()[0].clone())),
    );
    assert_eq!(
        questions_dialog(&model).write.notice(),
        Some("Question added")
    );
    assert!(questions_dialog(&model).editing.is_none());
    let effects_answer = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Questions(questions())),
    );
    assert!(effects_answer.is_empty());
    assert_eq!(section(&model).questions.value().unwrap().len(), 2);
}

#[test]
fn a_question_is_edited_and_a_text_one_sends_no_choices() {
    let mut model = questions_open();
    one(&mut model, EventTypeEvent::EditQuestion("nobody".into()));
    assert!(questions_dialog(&model).editing.is_none());
    one(
        &mut model,
        EventTypeEvent::EditQuestionForm(QuestionForm::default()),
    );
    assert!(
        questions_dialog(&model).editing.is_none(),
        "nothing to edit"
    );
    one(&mut model, EventTypeEvent::AddOption("A".into()));
    one(&mut model, EventTypeEvent::RemoveOption("A".into()));
    assert!(one(&mut model, EventTypeEvent::SaveQuestion).is_empty());
    one(
        &mut model,
        EventTypeEvent::EditQuestion("q-contract-1".into()),
    );
    assert_eq!(questions_dialog(&model).editor_title(), "Edit question");
    let mut form = questions_dialog(&model).editing.clone().unwrap();
    assert_eq!(form.options.len(), 3);
    form.kind = "text".into();
    form.required = false;
    one(&mut model, EventTypeEvent::EditQuestionForm(form));
    let effects = one(&mut model, EventTypeEvent::SaveQuestion);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::PatchQuestion { id, fields, .. })] = requests.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(id, "q-contract-1");
    assert_eq!(fields.options, None);
    answer(&mut model, *ticket, refused("rejected"));
    assert!(questions_dialog(&model).write.failure().is_some());
    assert!(
        questions_dialog(&model).editing.is_some(),
        "the edits are kept"
    );
    let effects = one(&mut model, EventTypeEvent::SaveQuestion);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Question(questions()[0].clone())),
    );
    assert_eq!(
        questions_dialog(&model).write.notice(),
        Some("Question saved")
    );
    one(&mut model, EventTypeEvent::AddQuestion);
    one(&mut model, EventTypeEvent::CancelQuestion);
    assert!(questions_dialog(&model).editing.is_none());
}

#[test]
fn a_question_is_deleted_after_asking() {
    let mut model = questions_open();
    assert!(one(&mut model, EventTypeEvent::ConfirmDeleteQuestion).is_empty());
    one(
        &mut model,
        EventTypeEvent::AskDeleteQuestion("q-contract-2".into()),
    );
    one(&mut model, EventTypeEvent::CancelDeleteQuestion);
    assert_eq!(questions_dialog(&model).confirming_delete, None);
    one(
        &mut model,
        EventTypeEvent::AskDeleteQuestion("q-contract-2".into()),
    );
    let effects = one(&mut model, EventTypeEvent::ConfirmDeleteQuestion);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::DeleteQuestion {
            slug: SLUG.into(),
            id: "q-contract-2".into()
        }
    );
    one(
        &mut model,
        EventTypeEvent::AskDeleteQuestion("q-contract-1".into()),
    );
    assert!(
        one(&mut model, EventTypeEvent::ConfirmDeleteQuestion).is_empty(),
        "one at a time"
    );
    let effects = answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert_eq!(
        questions_dialog(&model).write.notice(),
        Some("Question deleted")
    );
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(questions_dialog(&model).questions.failure().is_some());
    assert_eq!(section(&model).questions.value().unwrap().len(), 2, "kept");
    one(&mut model, EventTypeEvent::DismissNotice);
    assert_eq!(questions_dialog(&model).write, WriteState::Idle);
    // A question written after its dialog closed lands nowhere.
    one(&mut model, EventTypeEvent::AddQuestion);
    let mut form = questions_dialog(&model).editing.clone().unwrap();
    form.label = "Why?".into();
    one(&mut model, EventTypeEvent::EditQuestionForm(form));
    let effects = one(&mut model, EventTypeEvent::SaveQuestion);
    one(&mut model, EventTypeEvent::CloseDialog);
    assert!(
        answer(
            &mut model,
            sent(&effects)[0].0,
            done(SchedulingAdminData::Done(SchedulingOk { ok: true }))
        )
        .is_empty()
    );
}

#[test]
fn an_answer_for_a_dialog_closed_or_replaced_lands_nowhere() {
    let mut model = opened("agency");
    let effects = one(&mut model, EventTypeEvent::OpenQuestions);
    one(&mut model, EventTypeEvent::OpenActions);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Questions(questions())),
    );
    assert!(matches!(dialog(&model), EventTypeDialog::Actions(_)));
    let effects = one(&mut model, EventTypeEvent::OpenHosts);
    one(&mut model, EventTypeEvent::CloseDialog);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Hosts(hosts())),
    );
    // The editor's refusal after it closed.
    one(&mut model, EventTypeEvent::OpenEditor);
    let effects = one(&mut model, EventTypeEvent::Save);
    one(&mut model, EventTypeEvent::CloseDialog);
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(section(&model).dialog.is_none());
    // An action whose dialog closed still reads the type again.
    one(&mut model, EventTypeEvent::OpenActions);
    let effects = one(&mut model, EventTypeEvent::ToggleActive);
    one(&mut model, EventTypeEvent::CloseDialog);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::EventType(Box::new(types()[0].clone()))),
    );
    assert_eq!(sent(&effects).len(), 3);
    // The list's create, likewise.
    let mut model = listed("agency");
    on_list(&mut model, EventTypesEvent::OpenCreate);
    let mut form = list(&model).editor.as_ref().unwrap().form.clone();
    form.name = "X".into();
    on_list(&mut model, EventTypesEvent::EditForm(Box::new(form)));
    let effects = on_list(&mut model, EventTypesEvent::Save);
    on_list(&mut model, EventTypesEvent::CloseEditor);
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(list(&model).editor.is_none());
}

#[test]
fn an_event_for_a_closed_section_does_nothing() {
    let (mut model, _) = open("agency", SchedulingSection::Overview);
    assert!(on_list(&mut model, EventTypesEvent::Save).is_empty());
    assert!(one(&mut model, EventTypeEvent::Save).is_empty());
    let _ = Event::Back;
}

#[test]
fn hosts_named_or_not_and_answers_for_dialogs_that_moved_on() {
    let mut model = opened("agency");
    let effects = one(&mut model, EventTypeEvent::OpenHosts);
    let mut blank = hosts();
    blank[0].name = String::new();
    let effects2 = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Hosts(blank.clone())),
    );
    assert_eq!(hosts_dialog(&model).rows[0].name, blank[0].email);
    // A named user added keeps their name.
    let mut candidates = users();
    candidates.push(SchedulingUser {
        id: "ann".into(),
        name: "Ann".into(),
        email: "ann@example.com".into(),
        ..SchedulingUser::default()
    });
    answer(
        &mut model,
        sent(&effects2)[0].0,
        done(SchedulingAdminData::Users(candidates)),
    );
    one(&mut model, EventTypeEvent::AddHost("ann".into()));
    assert!(hosts_dialog(&model).rows.iter().any(|r| r.name == "Ann"));

    // The users arrive after the dialog closed.
    let mut model = opened("agency");
    let effects = one(&mut model, EventTypeEvent::OpenHosts);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Hosts(hosts())),
    );
    one(&mut model, EventTypeEvent::CloseDialog);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Users(users())),
    );
    assert!(section(&model).dialog.is_none());

    // The editor ignores what is not its own.
    one(&mut model, EventTypeEvent::OpenEditor);
    assert!(one(&mut model, EventTypeEvent::SetRotation("priority".into())).is_empty());
    assert!(matches!(dialog(&model), EventTypeDialog::Edit(_)));
}

#[test]
fn hosts_of_a_type_with_no_routing_stored_route_fixed() {
    let (mut model, effects) = open("agency", SchedulingSection::EventType { slug: SLUG.into() });
    let mut bare = types()[0].clone();
    bare.routing_mode = None;
    bare.rr_strategy = None;
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::EventType { .. })
        }),
        done(SchedulingAdminData::EventType(Box::new(bare))),
    );
    one(&mut model, EventTypeEvent::OpenHosts);
    assert_eq!(hosts_dialog(&model).routing_mode, "fixed");
    assert_eq!(hosts_dialog(&model).rotation, "even");
}
