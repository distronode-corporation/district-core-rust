//! Team: the workspace's members beside the scheduler's accounts, the teams
//! and their hosts, and archiving a scheduler account.

use district_api::ApiError;
use district_core::Tone;
use district_core::scheduling_admin::{
    ArchiveDialog, MemberState, TEAM_NAME_MAX, TeamDialog, TeamEditor, TeamEvent,
    TeamMembersDialog, TeamSection, WriteState, workspace_role_label,
};
use district_core::{Effect, Event, Model, SchedulingAdminEvent, SchedulingSection, Ticket};
use district_model::{
    MemberListResponse, SchedulingAdminData, SchedulingAdminRequest, SchedulingItems, SchedulingOk,
    SchedulingTeam, SchedulingUpcomingBooking, SchedulingUser, SchedulingUserArchived,
    WorkspaceMember,
};

use super::{act, admin, answer, data, done, open, refused, sent, ticket_for};
use crate::support::server_error;

fn section(model: &Model) -> &TeamSection {
    admin(model).team.as_ref().expect("the team is open")
}

fn team(model: &mut Model, event: TeamEvent) -> Vec<Effect> {
    act(model, SchedulingAdminEvent::Team(event))
}

fn users() -> Vec<SchedulingUser> {
    data("district-scheduling-users.json")
}

fn teams() -> Vec<SchedulingTeam> {
    data::<SchedulingItems<SchedulingTeam>>("district-scheduling-teams.json").items
}

fn member(email: &str, role: &str) -> WorkspaceMember {
    WorkspaceMember {
        email: email.into(),
        role: role.into(),
        created_at: "2026-08-15T14:30:00.000Z".into(),
    }
}

fn members_ticket(effects: &[Effect]) -> Ticket {
    effects
        .iter()
        .find_map(|e| match e {
            Effect::LoadMembers { ticket, .. } => Some(*ticket),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no members read in {effects:?}"))
}

fn members_answer(
    model: &mut Model,
    effects: &[Effect],
    result: Result<MemberListResponse, ApiError>,
) {
    model.update(Event::MembersLoaded {
        ticket: members_ticket(effects),
        result,
    });
}

/// The team section open as `role`, with `members` as the workspace's
/// members (or their failure) and the recorded users and teams.
fn read_with(role: &str, members: Result<Vec<WorkspaceMember>, ApiError>) -> Model {
    let (mut model, effects) = open(role, SchedulingSection::Team);
    let requests: Vec<SchedulingAdminRequest> = sent(&effects).into_iter().map(|s| s.1).collect();
    assert_eq!(
        requests,
        [
            SchedulingAdminRequest::Users {
                include_archived: Some(true)
            },
            SchedulingAdminRequest::Teams,
        ]
    );
    assert!(section(&model).members().value().is_none());
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::Users { .. })
        }),
        done(SchedulingAdminData::Users(users())),
    );
    // The users alone are not enough: the roles are awaited.
    assert!(section(&model).members().value().is_none());
    members_answer(
        &mut model,
        &effects,
        members.map(|members| MemberListResponse {
            success: true,
            members,
        }),
    );
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::Teams),
        done(SchedulingAdminData::Teams(teams())),
    );
    model
}

fn read(role: &str) -> Model {
    read_with(
        role,
        Ok(vec![
            member("Contract@Example.com", "agency"),
            member("contract@example.com", "client"),
            member("new@example.com", "viewer"),
        ]),
    )
}

#[test]
fn members_join_the_workspace_and_the_scheduler_by_address() {
    let model = read("agency");
    let section = section(&model);
    let rows = section.members().value().cloned().expect("read");
    let summary: Vec<(&str, &str, MemberState, Option<&str>)> = rows
        .iter()
        .map(|r| {
            (
                r.email.as_str(),
                r.name.as_str(),
                r.state,
                r.scheduler_user_id.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                "Contract@Example.com",
                "Contract Member",
                MemberState::Host,
                Some("sched-user-contract")
            ),
            ("new@example.com", "new", MemberState::Absent, None),
            (
                "gone@example.com",
                "Former Host",
                MemberState::Archived,
                Some("sched-user-archived")
            ),
        ]
    );
    let subtitles: Vec<String> = rows.iter().map(|r| section.member_subtitle(r)).collect();
    assert_eq!(
        subtitles,
        [
            "Contract@Example.com · Agency",
            "new@example.com · Viewer",
            "gone@example.com · Removed from the workspace",
        ]
    );
    let badges: Vec<(&str, Tone)> = rows.iter().map(|r| r.state.badge()).collect();
    assert_eq!(
        badges,
        [
            ("Host", Tone::Success),
            ("Not set up yet", Tone::Neutral),
            ("Archived", Tone::Neutral),
        ]
    );
    // An archived account with no role is not stranded: it holds nothing.
    assert_eq!(section.stranded_notice(), None);
    assert!(TeamSection::offers_archive(&rows[0], true));
    assert!(!TeamSection::offers_archive(&rows[0], false));
    assert!(!TeamSection::offers_archive(&rows[1], true));
    let counts: Vec<String> = section
        .teams
        .value()
        .expect("read")
        .iter()
        .map(TeamSection::member_count)
        .collect();
    assert_eq!(counts, ["2 members", "0 members"]);
    let one = SchedulingTeam {
        member_count: None,
        ..teams()[0].clone()
    };
    assert_eq!(TeamSection::member_count(&one), "2 members");
    let single = SchedulingTeam {
        member_count: Some(1),
        ..teams()[0].clone()
    };
    assert_eq!(TeamSection::member_count(&single), "1 member");
    assert!(TeamSection::labels().iter().all(|l| !l.is_empty()));
    assert_eq!(workspace_role_label(Some(" Client ")), "Client");
    assert_eq!(workspace_role_label(Some("owner")), "owner");
}

#[test]
fn hosts_who_left_the_workspace_are_named_unless_the_roles_could_not_be_read() {
    // Nobody in the workspace: the live host has left it.
    let model = read_with("agency", Ok(Vec::new()));
    assert_eq!(
        section(&model).stranded_notice().as_deref(),
        Some(
            "Contract Member is no longer in this workspace but still holds upcoming bookings, \
             so the scheduler kept their account open. Reassign or cancel those bookings, then \
             archive the account."
        )
    );

    // Two of them.
    let (mut model, effects) = open("agency", SchedulingSection::Team);
    let mut both = users();
    both[1].archived = false;
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::Users { .. })
        }),
        done(SchedulingAdminData::Users(both)),
    );
    members_answer(
        &mut model,
        &effects,
        Ok(MemberListResponse {
            success: true,
            members: Vec::new(),
        }),
    );
    let notice = section(&model).stranded_notice().expect("two stranded");
    assert!(
        notice.starts_with("Contract Member and Former Host are no longer in this workspace"),
        "{notice}"
    );

    // The roles could not be read: nobody is accused of leaving.
    let model = read_with("agency", Err(server_error()));
    let section = section(&model);
    assert!(section.members_degraded());
    assert_eq!(section.stranded_notice(), None);
    let rows = section.members().value().cloned().expect("read");
    assert_eq!(section.member_subtitle(&rows[0]), "contract@example.com");
}

#[test]
fn a_failed_read_of_the_scheduler_accounts_fails_the_members() {
    let (mut model, effects) = open("agency", SchedulingSection::Team);
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::Users { .. })
        }),
        refused("unavailable"),
    );
    assert!(section(&model).members().failure().is_some());
    assert_eq!(section(&model).stranded_notice(), None);
}

fn editor(model: &Model) -> &TeamEditor {
    match section(model).dialog.as_ref() {
        Some(TeamDialog::Editor(editor)) => editor,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_team_is_created_with_a_checked_name_and_the_section_read_again() {
    let mut model = read("viewer");
    team(&mut model, TeamEvent::OpenCreate);
    team(&mut model, TeamEvent::OpenEditor("team-contract-1".into()));
    team(&mut model, TeamEvent::OpenMembers("team-contract-1".into()));
    team(
        &mut model,
        TeamEvent::OpenArchive("sched-user-contract".into()),
    );
    assert!(section(&model).dialog.is_none());

    let mut model = read("agency");
    team(&mut model, TeamEvent::OpenCreate);
    let e = editor(&model);
    assert_eq!(
        (e.title(), e.submit_label(), e.close_label()),
        ("New team".to_owned(), "Create team", "Cancel")
    );
    assert!(!e.can_delete());
    assert!(TeamEditor::labels().iter().all(|l| !l.is_empty()));
    team(&mut model, TeamEvent::AskDelete);
    assert!(!editor(&model).confirming_delete);
    assert!(team(&mut model, TeamEvent::ConfirmDelete).is_empty());

    team(&mut model, TeamEvent::EditName("  ".into()));
    assert!(team(&mut model, TeamEvent::SaveName).is_empty());
    assert_eq!(editor(&model).name_rejected, Some("Give the team a name."));
    team(
        &mut model,
        TeamEvent::EditName("x".repeat(TEAM_NAME_MAX + 1)),
    );
    assert_eq!(editor(&model).name_rejected, None);
    team(&mut model, TeamEvent::SaveName);
    assert_eq!(
        editor(&model).name_rejected,
        Some("That name is longer than 200 characters.")
    );

    team(&mut model, TeamEvent::EditName(" Field ".into()));
    let effects = team(&mut model, TeamEvent::SaveName);
    let requests = sent(&effects);
    assert_eq!(
        requests[0].1,
        SchedulingAdminRequest::CreateTeam {
            name: "Field".into(),
            slug: None
        }
    );
    assert!(team(&mut model, TeamEvent::SaveName).is_empty());
    answer(&mut model, requests[0].0, refused("conflict"));
    assert!(editor(&model).write.failure().is_some());
    team(&mut model, TeamEvent::EditName("Field".into()));
    assert_eq!(editor(&model).write, WriteState::Idle);
    team(&mut model, TeamEvent::EditName("Field team".into()));
    let effects = team(&mut model, TeamEvent::SaveName);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Team(teams()[1].clone())),
    );
    assert_eq!(
        editor(&model).write,
        WriteState::Done("Team created".into())
    );
    // Read again, the dialog kept.
    assert_eq!(sent(&effects).len(), 2);
    members_ticket(&effects);
    assert!(section(&model).teams.value().is_none());
}

#[test]
fn a_team_is_renamed_or_deleted_asked_once_more() {
    let mut model = read("agency");
    team(&mut model, TeamEvent::OpenEditor("missing".into()));
    assert!(section(&model).dialog.is_none());
    team(&mut model, TeamEvent::OpenEditor("team-contract-1".into()));
    let e = editor(&model);
    assert_eq!(
        (
            e.title(),
            e.submit_label(),
            e.close_label(),
            e.delete_title()
        ),
        (
            "Sales".to_owned(),
            "Save name",
            "Done",
            "Delete Sales?".to_owned()
        )
    );
    team(&mut model, TeamEvent::EditName("Sales East".into()));
    let effects = team(&mut model, TeamEvent::SaveName);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::PatchTeam {
            id: "team-contract-1".into(),
            name: Some("Sales East".into()),
            slug: None,
        }
    );
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Team(teams()[0].clone())),
    );
    assert_eq!(
        editor(&model).write,
        WriteState::Done("Team renamed".into())
    );

    team(&mut model, TeamEvent::AskDelete);
    assert!(editor(&model).confirming_delete);
    team(&mut model, TeamEvent::CancelDelete);
    assert!(!editor(&model).confirming_delete);
    team(&mut model, TeamEvent::AskDelete);
    let effects = team(&mut model, TeamEvent::ConfirmDelete);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::DeleteTeam {
            id: "team-contract-1".into()
        }
    );
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(editor(&model).write.failure().is_some());
    team(&mut model, TeamEvent::AskDelete);
    let effects = team(&mut model, TeamEvent::ConfirmDelete);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert_eq!(
        editor(&model).write,
        WriteState::Done("Team deleted".into())
    );

    let unnamed = TeamEditor {
        team_name: String::new(),
        ..editor(&model).clone()
    };
    assert_eq!(unnamed.delete_title(), "Delete this team?");
    team(&mut model, TeamEvent::CloseDialog);
    assert!(section(&model).dialog.is_none());
}

fn hosts(model: &Model) -> &TeamMembersDialog {
    match section(model).dialog.as_ref() {
        Some(TeamDialog::Members(dialog)) => dialog,
        other => panic!("{other:?}"),
    }
}

#[test]
fn hosts_are_added_prioritised_and_removed() {
    let mut model = read("agency");
    team(&mut model, TeamEvent::OpenMembers("missing".into()));
    assert!(section(&model).dialog.is_none());
    let effects = team(&mut model, TeamEvent::OpenMembers("team-contract-1".into()));
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::Users {
            include_archived: None
        }
    );
    assert!(hosts(&model).addable().is_empty());
    assert_eq!(hosts(&model).members().len(), 2);
    assert_eq!(hosts(&model).priority("sched-user-rotation"), "5");
    assert_eq!(hosts(&model).priority("nobody"), "");
    assert!(TeamMembersDialog::labels().iter().all(|l| !l.is_empty()));
    let mut candidates = users();
    candidates.push(SchedulingUser {
        id: "sched-user-new".into(),
        email: "new@example.com".into(),
        ..users()[1].clone()
    });
    candidates[2].archived = false;
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Users(candidates)),
    );
    let addable: Vec<&str> = hosts(&model)
        .addable()
        .iter()
        .map(|u| u.id.as_str())
        .collect();
    assert_eq!(addable, ["sched-user-new"]);

    // Nothing chosen.
    team(&mut model, TeamEvent::AddMember);
    assert_eq!(
        hosts(&model).write.failure().map(|f| f.message.as_str()),
        Some("Choose a host to add.")
    );
    team(&mut model, TeamEvent::Pick("sched-user-new".into()));
    assert_eq!(hosts(&model).write, WriteState::Idle);
    let effects = team(&mut model, TeamEvent::AddMember);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::AddTeamMember {
            id: "team-contract-1".into(),
            user_id: "sched-user-new".into(),
            routing_priority: None,
        }
    );
    // One write at a time.
    assert!(team(&mut model, TeamEvent::Pick("x".into())).is_empty());
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(hosts(&model).write.failure().is_some());
    team(&mut model, TeamEvent::Pick("sched-user-new".into()));
    let effects = team(&mut model, TeamEvent::AddMember);
    let mut grown = teams()[0].clone();
    grown.member_count = Some(3);
    let reread = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Team(grown)),
    );
    assert_eq!(
        hosts(&model).write,
        WriteState::Done("Host added to the team".into())
    );
    assert_eq!(hosts(&model).team.member_count, Some(3));
    assert_eq!(sent(&reread).len(), 2);

    // An answer that is not a team leaves the team as it was.
    team(&mut model, TeamEvent::Pick("sched-user-new".into()));
    let effects = team(&mut model, TeamEvent::AddMember);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert_eq!(hosts(&model).team.member_count, Some(3));

    // Priorities: a whole number, zero or more.
    for bad in ["", "-1", "1.5"] {
        team(
            &mut model,
            TeamEvent::EditPriority {
                user_id: "sched-user-rotation".into(),
                value: bad.into(),
            },
        );
        assert!(
            team(
                &mut model,
                TeamEvent::SavePriority("sched-user-rotation".into())
            )
            .is_empty()
        );
        assert_eq!(
            hosts(&model).priority_rejected,
            Some((
                "sched-user-rotation".to_owned(),
                "A routing priority is a whole number, zero or more."
            ))
        );
    }
    team(
        &mut model,
        TeamEvent::EditPriority {
            user_id: "sched-user-other".into(),
            value: "2".into(),
        },
    );
    assert_eq!(hosts(&model).priority("sched-user-other"), "2");
    assert_eq!(hosts(&model).priority_rejected, None);
    team(
        &mut model,
        TeamEvent::EditPriority {
            user_id: "sched-user-rotation".into(),
            value: "7".into(),
        },
    );
    let effects = team(
        &mut model,
        TeamEvent::SavePriority("sched-user-rotation".into()),
    );
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::PatchTeamMember {
            id: "team-contract-1".into(),
            user_id: "sched-user-rotation".into(),
            routing_priority: 7,
        }
    );
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Team(teams()[0].clone())),
    );
    assert_eq!(
        hosts(&model).write,
        WriteState::Done("Routing priority saved".into())
    );
    // The drafts follow the team as answered.
    assert_eq!(hosts(&model).priority("sched-user-rotation"), "5");
    team(
        &mut model,
        TeamEvent::EditPriority {
            user_id: "sched-user-rotation".into(),
            value: "6".into(),
        },
    );
    assert_eq!(
        hosts(&model).write,
        WriteState::Done("Routing priority saved".into())
    );

    // Removing a host, asked once more.
    assert!(team(&mut model, TeamEvent::ConfirmRemove).is_empty());
    team(
        &mut model,
        TeamEvent::AskRemove("sched-user-rotation".into()),
    );
    team(&mut model, TeamEvent::CancelRemove);
    assert_eq!(hosts(&model).removing, None);
    team(
        &mut model,
        TeamEvent::AskRemove("sched-user-rotation".into()),
    );
    let effects = team(&mut model, TeamEvent::ConfirmRemove);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::RemoveTeamMember {
            id: "team-contract-1".into(),
            user_id: "sched-user-rotation".into(),
        }
    );
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(hosts(&model).write.failure().is_some());
    team(
        &mut model,
        TeamEvent::EditPriority {
            user_id: "sched-user-rotation".into(),
            value: "5".into(),
        },
    );
    assert_eq!(hosts(&model).write, WriteState::Idle);
    team(
        &mut model,
        TeamEvent::AskRemove("sched-user-rotation".into()),
    );
    let effects = team(&mut model, TeamEvent::ConfirmRemove);
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    // The team is read again, which answers the removal.
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::Team {
            id: "team-contract-1".into()
        }
    );
    let mut shrunk = teams()[0].clone();
    shrunk.member_count = Some(1);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Team(shrunk)),
    );
    assert_eq!(
        hosts(&model).write,
        WriteState::Done("Host removed from the team".into())
    );
    // Other events do nothing here.
    assert!(team(&mut model, TeamEvent::SaveName).is_empty());
}

fn archive(model: &Model) -> &ArchiveDialog {
    match section(model).dialog.as_ref() {
        Some(TeamDialog::Archive(dialog)) => dialog,
        other => panic!("{other:?}"),
    }
}

fn upcoming() -> Vec<SchedulingUpcomingBooking> {
    data::<SchedulingItems<SchedulingUpcomingBooking>>("district-scheduling-user-upcoming.json")
        .items
}

#[test]
fn an_account_is_archived_only_with_no_upcoming_booking() {
    let mut model = read("agency");
    // Only a live host's account.
    team(
        &mut model,
        TeamEvent::OpenArchive("sched-user-archived".into()),
    );
    assert!(section(&model).dialog.is_none());
    let effects = team(
        &mut model,
        TeamEvent::OpenArchive("sched-user-contract".into()),
    );
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::UpcomingBookings {
            id: "sched-user-contract".into()
        }
    );
    assert_eq!(
        archive(&model).title(),
        "Archive Contract Member's account?"
    );
    assert_eq!(
        archive(&model).blocked_reason(),
        Some("Checking their upcoming bookings…")
    );
    assert!(ArchiveDialog::labels().iter().all(|l| !l.is_empty()));
    team(&mut model, TeamEvent::AskArchive);
    assert!(!archive(&model).confirming);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::UpcomingBookings(upcoming())),
    );
    assert_eq!(
        archive(&model).blocked_reason(),
        Some("Reassign or cancel their upcoming bookings first.")
    );
    team(&mut model, TeamEvent::AskArchive);
    assert!(team(&mut model, TeamEvent::ConfirmArchive).is_empty());

    team(&mut model, TeamEvent::CloseDialog);
    let effects = team(
        &mut model,
        TeamEvent::OpenArchive("sched-user-contract".into()),
    );
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::UpcomingBookings(Vec::new())),
    );
    assert_eq!(archive(&model).blocked_reason(), None);
    team(&mut model, TeamEvent::AskArchive);
    team(&mut model, TeamEvent::CancelArchive);
    assert!(!archive(&model).confirming);
    team(&mut model, TeamEvent::AskArchive);
    let effects = team(&mut model, TeamEvent::ConfirmArchive);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::ArchiveUser {
            id: "sched-user-contract".into()
        }
    );
    // The scheduler does not say which of its reasons it refused for.
    answer(
        &mut model,
        sent(&effects)[0].0,
        refused("not_a_known_refusal"),
    );
    assert!(
        archive(&model)
            .write
            .failure()
            .is_some_and(|f| f.message.starts_with("That account could not be archived."))
    );
    team(&mut model, TeamEvent::AskArchive);
    let effects = team(&mut model, TeamEvent::ConfirmArchive);
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    let failure = archive(&model).write.failure().cloned().expect("failed");
    assert!(
        !failure
            .message
            .starts_with("That account could not be archived.")
    );
    team(&mut model, TeamEvent::AskArchive);
    let effects = team(&mut model, TeamEvent::ConfirmArchive);
    let archived: SchedulingUserArchived = data("district-scheduling-user-archive.json");
    let effects = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::UserArchived(archived)),
    );
    assert_eq!(
        archive(&model).write,
        WriteState::Done("Contract Member's scheduler account is closed".into())
    );
    assert_eq!(sent(&effects).len(), 2);
}

#[test]
fn an_account_with_no_name_or_address_is_archived_unnamed() {
    let (mut model, effects) = open("agency", SchedulingSection::Team);
    let blank = SchedulingUser {
        id: "sched-user-blank".into(),
        email: String::new(),
        name: String::new(),
        ..users()[0].clone()
    };
    answer(
        &mut model,
        ticket_for(&effects, |r| {
            matches!(r, SchedulingAdminRequest::Users { .. })
        }),
        done(SchedulingAdminData::Users(vec![blank])),
    );
    members_answer(
        &mut model,
        &effects,
        Ok(MemberListResponse {
            success: true,
            members: Vec::new(),
        }),
    );
    let effects = team(
        &mut model,
        TeamEvent::OpenArchive("sched-user-blank".into()),
    );
    assert_eq!(archive(&model).title(), "Archive this account?");
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::UpcomingBookings(Vec::new())),
    );
    team(&mut model, TeamEvent::AskArchive);
    let effects = team(&mut model, TeamEvent::ConfirmArchive);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Done(SchedulingOk { ok: true })),
    );
    assert_eq!(
        archive(&model).write,
        WriteState::Done("Scheduler account closed".into())
    );
}

#[test]
fn answers_for_a_dialog_that_closed_land_nowhere() {
    let mut model = read("agency");
    let effects = team(&mut model, TeamEvent::OpenMembers("team-contract-1".into()));
    team(&mut model, TeamEvent::CloseDialog);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Users(users())),
    );
    assert!(section(&model).dialog.is_none());
    // No dialog: the dialog's events do nothing.
    assert!(team(&mut model, TeamEvent::SaveName).is_empty());
    // A section closed: its events do nothing, and the members read goes to
    // the settings, which are not open either.
    let (mut model, effects) = open("agency", SchedulingSection::Team);
    model.update(Event::Navigate(district_core::Route::SchedulingAdmin(
        SchedulingSection::Overview,
    )));
    members_answer(&mut model, &effects, Err(server_error()));
    assert!(team(&mut model, TeamEvent::OpenCreate).is_empty());
    assert!(admin(&model).team.is_none());
}
