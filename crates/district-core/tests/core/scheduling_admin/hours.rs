//! Working hours: the week and the overrides as read, the week's editor and
//! its save as a difference, and the overrides' editor.

use std::time::Duration;

use district_core::scheduling_admin::{
    HoursDialog, HoursEvent, HoursSection, OverrideForm, OverridesEditor, SectionLoad, WeekEditor,
    WriteState, weekday_name,
};
use district_core::{Effect, Model, SchedulingAdminEvent, SchedulingSection};
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingAvailabilityOverride,
    SchedulingAvailabilityRule, SchedulingItems, SchedulingOk, SchedulingOverrideDraft,
};

use super::{act, admin, answer, data, done, now, open, refused, sent, ticket_for};
use crate::support::server_error;

fn section(model: &Model) -> &HoursSection {
    admin(model).hours.as_ref().expect("the hours are open")
}

fn hours(model: &mut Model, event: HoursEvent) -> Vec<Effect> {
    act(model, SchedulingAdminEvent::Hours(event))
}

fn rule(id: &str, day: i64, start: &str, end: &str) -> SchedulingAvailabilityRule {
    SchedulingAvailabilityRule {
        id: id.into(),
        event_type_id: None,
        day_of_week: day,
        start_time: start.into(),
        end_time: end.into(),
    }
}

fn rules() -> Vec<SchedulingAvailabilityRule> {
    vec![
        rule("mon", 1, "09:00", "17:00"),
        rule("tue", 2, "13:00", "17:00"),
        rule("tue-am", 2, "09:00", "12:00"),
        rule("bad", 9, "09:00", "10:00"),
        SchedulingAvailabilityRule {
            event_type_id: Some("et".into()),
            ..rule("own", 3, "10:00", "11:00")
        },
    ]
}

fn day_off(
    id: &str,
    date: &str,
    group: Option<&str>,
    reason: &str,
) -> SchedulingAvailabilityOverride {
    SchedulingAvailabilityOverride {
        id: id.into(),
        date: date.into(),
        is_available: false,
        reason: reason.into(),
        start_time: None,
        end_time: None,
        group_id: group.map(Into::into),
    }
}

fn overrides() -> Vec<SchedulingAvailabilityOverride> {
    vec![
        day_off("past", "2026-09-01", None, "day_off"),
        day_off("x2", "2026-12-25", Some("xmas"), "out_of_office"),
        day_off("x1", "2026-12-24", Some("xmas"), "out_of_office"),
        SchedulingAvailabilityOverride {
            is_available: true,
            reason: "custom_hours".into(),
            start_time: Some("12:00".into()),
            end_time: Some("16:00".into()),
            ..day_off("half", "2026-09-24", None, "custom_hours")
        },
        day_off("today", "2026-09-14", Some(""), "day_off"),
    ]
}

/// The hours open as `role`, read.
fn read(role: &str) -> Model {
    let (mut model, effects) = open(role, SchedulingSection::Hours);
    let replies = [
        (
            SchedulingAdminRequest::Me,
            SchedulingAdminData::Me(data("district-scheduling-me.json")),
        ),
        (
            SchedulingAdminRequest::Rules {
                event_type_id: None,
            },
            SchedulingAdminData::Rules(rules()),
        ),
        (
            SchedulingAdminRequest::Overrides,
            SchedulingAdminData::Overrides(overrides()),
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
fn the_week_reads_monday_first_in_the_profiles_zone() {
    let model = read("agency");
    let section = section(&model);
    assert_eq!(section.times_in(), "Times in America/Toronto");
    assert_eq!(
        section.summary(),
        "Mon, 9:00 a.m. to 5:00 p.m.; Tue, 9:00 a.m. to 12:00 p.m., 9:00 a.m. to 10:00 a.m., \
         and 1:00 p.m. to 5:00 p.m.",
        "the web's summary folds a day outside the week in; its grid leaves it out"
    );
    let week = section.week();
    assert_eq!(week.len(), 7);
    assert_eq!(week[0], ("Monday", "9:00 a.m. to 5:00 p.m.".to_owned()));
    assert_eq!(
        week[1].1, "9:00 a.m. to 12:00 p.m., 1:00 p.m. to 5:00 p.m.",
        "a day outside the week is left out of the grid"
    );
    assert_eq!(week[6], ("Sunday", "Not bookable".to_owned()));
    assert_eq!(HoursSection::labels()[0], "Date overrides");
}

#[test]
fn the_overrides_still_to_come_show_a_run_as_one_row() {
    let model = read("agency");
    let rows = section(&model).override_rows(now());
    let shown: Vec<(&str, &str, &str)> = rows
        .iter()
        .map(|r| (r.key.as_str(), r.dates.as_str(), r.hours.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("id:today", "Sep 14, 2026", "Unavailable"),
            ("id:half", "Sep 24, 2026", "12:00 p.m. to 4:00 p.m."),
            (
                "group:xmas",
                "Dec 24, 2026 to Dec 25, 2026",
                "Out of office"
            ),
        ]
    );
    assert_eq!(rows[2].days, 2);
    assert!(rows[2].is_run);
    let later = now() + Duration::from_secs(365 * 86_400);
    assert!(section(&model).override_rows(later).is_empty());
}

#[test]
fn without_the_profile_the_hours_read_in_utc_and_failures_stay_in_their_card() {
    let (mut model, effects) = open("agency", SchedulingSection::Hours);
    assert!(section(&model).week().is_empty());
    assert!(section(&model).override_rows(now()).is_empty());
    assert_eq!(section(&model).summary(), "Not bookable on any day yet.");
    for request in [
        SchedulingAdminRequest::Me,
        SchedulingAdminRequest::Rules {
            event_type_id: None,
        },
        SchedulingAdminRequest::Overrides,
    ] {
        answer(
            &mut model,
            ticket_for(&effects, |r| *r == request),
            Err(server_error()),
        );
    }
    let section = section(&model);
    assert_eq!(section.timezone, "UTC");
    assert!(section.rules.failure().is_some() && section.overrides.failure().is_some());
}

fn week(model: &Model) -> &WeekEditor {
    match section(model).dialog.as_ref().expect("an editor") {
        HoursDialog::Week(week) => week,
        other => panic!("{other:?}"),
    }
}

/// The week's editor open, its rules read.
fn week_open() -> Model {
    let mut model = read("agency");
    let effects = hours(&mut model, HoursEvent::OpenWeek);
    assert!(!week(&model).can_save(), "not before the rules are read");
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Rules(rules())),
    );
    model
}

fn key(model: &Model, day: usize, index: usize) -> u64 {
    week(model).days[day][index].key
}

#[test]
fn the_week_is_edited_window_by_window_and_checked_before_it_is_sent() {
    let mut model = week_open();
    let days = &week(&model).days;
    assert_eq!(days[1].len(), 2);
    assert_eq!(days[1][0].start, "09:00", "by opening time");
    assert!(week(&model).offers_copy(0) && !week(&model).offers_copy(2));
    assert!(!week(&model).offers_copy(5));
    assert_eq!(WeekEditor::labels()[2], "Add range");
    hours(&mut model, HoursEvent::AddRange { day: 6 });
    hours(&mut model, HoursEvent::AddRange { day: 9 });
    let sunday = key(&model, 6, 0);
    assert_eq!(week(&model).days[6][0].start, "09:00");
    for (value, error) in [
        ("9", "Enter a time as HH:MM."),
        ("18:00", "The end has to be after the start."),
    ] {
        hours(
            &mut model,
            HoursEvent::SetRangeStart {
                day: 6,
                key: sunday,
                value: value.into(),
            },
        );
        assert_eq!(week(&model).errors(6), [Some(error)]);
        assert!(!week(&model).can_save());
        assert!(hours(&mut model, HoursEvent::SaveWeek).is_empty());
    }
    hours(
        &mut model,
        HoursEvent::SetRangeStart {
            day: 6,
            key: sunday,
            value: "10:00".into(),
        },
    );
    hours(
        &mut model,
        HoursEvent::SetRangeEnd {
            day: 6,
            key: sunday,
            value: "14:00".into(),
        },
    );
    hours(&mut model, HoursEvent::AddRange { day: 6 });
    let second = key(&model, 6, 1);
    hours(
        &mut model,
        HoursEvent::SetRangeStart {
            day: 6,
            key: second,
            value: "13:00".into(),
        },
    );
    assert_eq!(
        week(&model).errors(6),
        [None, Some("These hours overlap another range on this day.")]
    );
    hours(
        &mut model,
        HoursEvent::RemoveRange {
            day: 6,
            key: second,
        },
    );
    hours(
        &mut model,
        HoursEvent::RemoveRange {
            day: 9,
            key: second,
        },
    );
    hours(
        &mut model,
        HoursEvent::SetRangeEnd {
            day: 9,
            key: sunday,
            value: "x".into(),
        },
    );
    assert!(week(&model).errors(9).is_empty());
    assert!(week(&model).can_save());
}

#[test]
fn the_save_is_deletes_then_changes_then_creates_one_at_a_time() {
    let mut model = week_open();
    // Monday's window moves; the afternoon on Tuesday goes; Sunday gets one.
    let monday = key(&model, 0, 0);
    hours(
        &mut model,
        HoursEvent::SetRangeEnd {
            day: 0,
            key: monday,
            value: "18:00".into(),
        },
    );
    let afternoon = key(&model, 1, 1);
    hours(
        &mut model,
        HoursEvent::RemoveRange {
            day: 1,
            key: afternoon,
        },
    );
    hours(&mut model, HoursEvent::AddRange { day: 6 });
    let effects = hours(&mut model, HoursEvent::SaveWeek);
    let mut steps = Vec::new();
    let mut effects = effects;
    loop {
        let requests = sent(&effects);
        let [(ticket, request)] = requests.as_slice() else {
            break;
        };
        steps.push(request.clone());
        effects = answer(
            &mut model,
            *ticket,
            done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
        );
    }
    assert_eq!(
        steps,
        [
            SchedulingAdminRequest::DeleteRule { id: "tue".into() },
            SchedulingAdminRequest::DeleteRule { id: "bad".into() },
            SchedulingAdminRequest::PatchRule {
                id: "mon".into(),
                day_of_week: Some(1),
                start_time: Some("09:00".into()),
                end_time: Some("18:00".into()),
            },
            SchedulingAdminRequest::CreateRule {
                event_type_id: None,
                day_of_week: 0,
                start_time: "09:00".into(),
                end_time: "17:00".into(),
            },
        ]
    );
    // The last step done: the week and the section are read again.
    let requests: Vec<_> = sent(&effects).into_iter().map(|(_, r)| r).collect();
    assert_eq!(requests.len(), 4);
    assert_eq!(week(&model).write.notice(), Some("Working hours saved"));
    assert_eq!(week(&model).load, SectionLoad::Loading);
}

#[test]
fn a_save_that_stops_half_way_reads_everything_again() {
    let mut model = week_open();
    let afternoon = key(&model, 1, 1);
    hours(
        &mut model,
        HoursEvent::RemoveRange {
            day: 1,
            key: afternoon,
        },
    );
    hours(&mut model, HoursEvent::AddRange { day: 6 });
    let effects = hours(&mut model, HoursEvent::SaveWeek);
    assert!(
        hours(&mut model, HoursEvent::SaveWeek).is_empty(),
        "one at a time"
    );
    let effects = answer(&mut model, sent(&effects)[0].0, refused("rejected"));
    assert_eq!(
        sent(&effects).len(),
        4,
        "the week and the section, not the create"
    );
    assert!(week(&model).write.failure().is_some());
    hours(&mut model, HoursEvent::DismissNotice);
    assert_eq!(week(&model).write, WriteState::Idle);
}

#[test]
fn a_week_unchanged_sends_nothing_and_says_so() {
    let mut model = read("agency");
    let effects = hours(&mut model, HoursEvent::OpenWeek);
    let clean: Vec<_> = rules().into_iter().filter(|r| r.id != "bad").collect();
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Rules(clean)),
    );
    assert!(hours(&mut model, HoursEvent::SaveWeek).is_empty());
    assert_eq!(week(&model).write.notice(), Some("Nothing to save."));
}

#[test]
fn copying_a_day_to_the_weekdays_keeps_each_days_rules() {
    let mut model = week_open();
    hours(&mut model, HoursEvent::CopyToWeekdays { day: 1 });
    let days = &week(&model).days;
    for target in [0, 2, 3, 4] {
        let windows: Vec<(&str, &str)> = days[target]
            .iter()
            .map(|r| (r.start.as_str(), r.end.as_str()))
            .collect();
        assert_eq!(
            windows,
            [("09:00", "12:00"), ("13:00", "17:00")],
            "{target}"
        );
    }
    assert_eq!(
        days[0][0].rule_id.as_deref(),
        Some("mon"),
        "Monday keeps its rule"
    );
    assert_eq!(days[0][1].rule_id, None);
    assert!(days[5].is_empty() && days[6].is_empty(), "not the weekend");
    hours(&mut model, HoursEvent::CopyToWeekdays { day: 6 });
    // A window carrying the id of a rule no longer read is created anew.
    let effects = hours(&mut model, HoursEvent::SaveWeek);
    let requests = sent(&effects);
    assert!(matches!(
        requests[0].1,
        SchedulingAdminRequest::PatchRule { .. }
    ));
}

#[test]
fn the_week_cannot_be_edited_from_a_failed_read_and_closes() {
    let mut model = read("agency");
    let effects = hours(&mut model, HoursEvent::OpenWeek);
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(week(&model).load.failure().is_some());
    assert!(!week(&model).can_save());
    hours(&mut model, HoursEvent::CloseDialog);
    assert!(section(&model).dialog.is_none());
    let mut viewer = read("viewer");
    assert!(hours(&mut viewer, HoursEvent::OpenWeek).is_empty());
    assert!(hours(&mut viewer, HoursEvent::OpenOverrides { now: now() }).is_empty());
    assert!(hours(&mut viewer, HoursEvent::SaveWeek).is_empty());
}

fn editor(model: &Model) -> &OverridesEditor {
    match section(model).dialog.as_ref().expect("an editor") {
        HoursDialog::Overrides(editor) => editor,
        other => panic!("{other:?}"),
    }
}

fn overrides_open() -> Model {
    let mut model = read("agency");
    let effects = hours(&mut model, HoursEvent::OpenOverrides { now: now() });
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Overrides(overrides())),
    );
    model
}

#[test]
fn an_override_is_checked_and_added_as_a_day_a_run_or_custom_hours() {
    let mut model = overrides_open();
    assert_eq!(editor(&model).rows().len(), 3);
    assert_eq!(OverridesEditor::labels()[0], "Date overrides");
    hours(
        &mut model,
        HoursEvent::EditOverride(OverrideForm::default()),
    );
    assert!(editor(&model).adding.is_none(), "the form is not open");
    hours(&mut model, HoursEvent::AddOverride);
    assert_eq!(OverrideForm::kinds()[1].0, "custom_hours");
    let cases: Vec<(OverrideForm, &str)> = vec![
        (OverrideForm::default(), "Pick a date."),
        (
            OverrideForm {
                date: "2026-10-02".into(),
                end_date: "soon".into(),
                ..OverrideForm::default()
            },
            "Pick a date.",
        ),
        (
            OverrideForm {
                date: "2026-10-02".into(),
                end_date: "2026-10-01".into(),
                ..OverrideForm::default()
            },
            "The last day has to be on or after the first.",
        ),
        (
            OverrideForm {
                reason: "custom_hours".into(),
                date: "2026-10-02".into(),
                start: "9".into(),
                ..OverrideForm::default()
            },
            "Enter a time as HH:MM.",
        ),
        (
            OverrideForm {
                reason: "custom_hours".into(),
                date: "2026-10-02".into(),
                start: "18:00".into(),
                ..OverrideForm::default()
            },
            "The end has to be after the start.",
        ),
    ];
    for (form, error) in cases {
        hours(&mut model, HoursEvent::EditOverride(form));
        assert!(hours(&mut model, HoursEvent::SaveOverride).is_empty());
        assert_eq!(editor(&model).validation, Some(error));
    }
    let custom = OverrideForm {
        reason: "custom_hours".into(),
        date: "2026-10-02".into(),
        end_date: "2026-10-09".into(),
        ..OverrideForm::default()
    };
    assert_eq!(custom.date_label(), "Date");
    assert_eq!(OverrideForm::default().date_label(), "First day");
    hours(&mut model, HoursEvent::EditOverride(custom));
    let effects = hours(&mut model, HoursEvent::SaveOverride);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::CreateOverride(SchedulingOverrideDraft {
            date: "2026-10-02".into(),
            end_date: None,
            reason: "custom_hours".into(),
            start_time: Some("09:00".into()),
            end_time: Some("17:00".into()),
        }),
        "custom hours are for one day"
    );
    assert!(
        hours(&mut model, HoursEvent::SaveOverride).is_empty(),
        "one at a time"
    );
    answer(&mut model, *ticket, refused("conflict"));
    assert!(editor(&model).write.failure().is_some());
    assert!(editor(&model).adding.is_some());
    let run = OverrideForm {
        date: "2026-10-02".into(),
        end_date: "2026-10-04".into(),
        ..OverrideForm::default()
    };
    hours(&mut model, HoursEvent::EditOverride(run));
    let effects = hours(&mut model, HoursEvent::SaveOverride);
    let requests = sent(&effects);
    let [(ticket, SchedulingAdminRequest::CreateOverride(draft))] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(draft.end_date.as_deref(), Some("2026-10-04"));
    let effects = answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert_eq!(editor(&model).write.notice(), Some("Override added"));
    assert!(editor(&model).adding.is_none());
    let mut more = overrides();
    more.push(day_off("new", "2026-10-02", None, "day_off"));
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Overrides(more)),
    );
    assert_eq!(editor(&model).rows().len(), 4);
    assert_eq!(
        section(&model).override_rows(now()).len(),
        4,
        "the section follows"
    );
    hours(&mut model, HoursEvent::AddOverride);
    hours(&mut model, HoursEvent::CancelOverride);
    assert!(editor(&model).adding.is_none());
    hours(&mut model, HoursEvent::DismissNotice);
    assert_eq!(editor(&model).write, WriteState::Idle);
}

#[test]
fn a_single_day_and_a_run_are_deleted_by_their_own_ids_after_asking() {
    let mut model = overrides_open();
    assert!(
        hours(&mut model, HoursEvent::ConfirmDeleteOverride).is_empty(),
        "not asked"
    );
    hours(
        &mut model,
        HoursEvent::AskDeleteOverride("group:xmas".into()),
    );
    assert_eq!(
        editor(&model).delete_body(),
        "All 2 days go back to your weekly hours."
    );
    hours(&mut model, HoursEvent::CancelDeleteOverride);
    assert_eq!(
        editor(&model).delete_body(),
        "That day goes back to your weekly hours."
    );
    hours(
        &mut model,
        HoursEvent::AskDeleteOverride("group:xmas".into()),
    );
    let effects = hours(&mut model, HoursEvent::ConfirmDeleteOverride);
    let requests = sent(&effects);
    let [(ticket, request)] = requests.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(
        *request,
        SchedulingAdminRequest::DeleteOverrideGroup {
            group_id: "xmas".into()
        }
    );
    hours(&mut model, HoursEvent::AskDeleteOverride("id:half".into()));
    assert!(
        hours(&mut model, HoursEvent::ConfirmDeleteOverride).is_empty(),
        "one at a time"
    );
    let effects = answer(
        &mut model,
        *ticket,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert_eq!(editor(&model).write.notice(), Some("Override deleted"));
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(editor(&model).load.failure().is_some());
    hours(&mut model, HoursEvent::AskDeleteOverride("id:half".into()));
    let effects = hours(&mut model, HoursEvent::ConfirmDeleteOverride);
    let requests = sent(&effects);
    assert_eq!(
        requests[0].1,
        SchedulingAdminRequest::DeleteOverride { id: "half".into() }
    );
    hours(
        &mut model,
        HoursEvent::AskDeleteOverride("id:nothing".into()),
    );
    hours(&mut model, HoursEvent::DismissNotice);
    assert!(hours(&mut model, HoursEvent::ConfirmDeleteOverride).is_empty());
}

#[test]
fn answers_for_an_editor_that_closed_land_nowhere() {
    let mut model = read("agency");
    let effects = hours(&mut model, HoursEvent::OpenWeek);
    hours(&mut model, HoursEvent::CloseDialog);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Rules(rules())),
    );
    assert!(section(&model).dialog.is_none());
    assert_eq!(weekday_name(2), "Wednesday");
    let rules =
        data::<SchedulingItems<SchedulingAvailabilityRule>>("district-scheduling-rules.json");
    assert_eq!(rules.items.len(), 2);
    let (mut model, _) = open("agency", SchedulingSection::Overview);
    assert!(hours(&mut model, HoursEvent::OpenWeek).is_empty());
}
