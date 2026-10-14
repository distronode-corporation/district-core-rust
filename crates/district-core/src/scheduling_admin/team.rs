//! The team: the workspace's members beside the scheduler's own users (who
//! takes bookings, who has no scheduler account yet, who was archived), the
//! teams that route bookings, and archiving a scheduler account.
//!
//! The two populations differ and are joined by address. Someone the
//! scheduler still has who left the workspace while holding bookings is named
//! at the top: the scheduler kept their account open, and their bookings need
//! a new host before it can be archived. When the workspace's members cannot
//! be read, no role is shown at all, rather than calling every host removed.
//!
//! An account is archived only once its upcoming bookings were read and there
//! are none, and the archive is asked once more. A team's members each change
//! its rotation, so their editor works from the team as last answered.

use district_api::ApiError;
use district_l10n::format;
use district_l10n::scheduling_strings as ss;
use district_model::{
    MemberListResponse, SchedulingAdminRequest, SchedulingTeam, SchedulingTeamMember,
    SchedulingUpcomingBooking, SchedulingUser,
};

use super::event_types::whole;
use super::overview::load;
use super::{Answer, Out, Pending as AdminPending, SchedulingAdmin, SectionLoad, WriteState};
use crate::failure::FailureText;
use crate::model::Tickets;
use crate::text::l;
use crate::workflows::Tone;

/// The longest team name the catalog takes.
pub const TEAM_NAME_MAX: usize = 200;

/// Where a member stands with the scheduler.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemberState {
    /// Takes bookings.
    Host,
    /// No scheduler account yet.
    Absent,
    /// Archived: takes no bookings.
    Archived,
}

impl MemberState {
    /// The badge's label and tone.
    pub fn badge(self) -> (&'static str, Tone) {
        match self {
            Self::Host => (ss::team_state_host(l()), Tone::Success),
            Self::Archived => (ss::team_state_archived(l()), Tone::Neutral),
            Self::Absent => (ss::team_state_absent(l()), Tone::Neutral),
        }
    }
}

/// One person on the team section: a workspace member, a scheduler user, or
/// both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberRow {
    /// The address, as the workspace (or else the scheduler) writes it.
    pub email: String,
    /// Their name: the scheduler's, else the address before its `@`.
    pub name: String,
    /// Their workspace role as stored, or `None` for someone not (or no longer)
    /// a member.
    pub workspace_role: Option<String>,
    /// Where they stand with the scheduler.
    pub state: MemberState,
    /// Their scheduler user id, when they have one.
    pub scheduler_user_id: Option<String>,
}

fn normalize(email: &str) -> String {
    email.trim().to_lowercase()
}

/// The part of an address before its `@`, or the whole when there is none.
fn name_from_email(email: &str) -> String {
    let trimmed = email.trim();
    match trimmed.split('@').next().filter(|local| !local.is_empty()) {
        Some(local) => local.to_owned(),
        None => trimmed.to_owned(),
    }
}

fn preferred_name(scheduler_name: Option<&str>, email: &str) -> String {
    match scheduler_name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => name.to_owned(),
        None => name_from_email(email),
    }
}

fn state_of(user: Option<&SchedulingUser>) -> MemberState {
    match user {
        None => MemberState::Absent,
        Some(user) if user.archived => MemberState::Archived,
        Some(_) => MemberState::Host,
    }
}

/// The workspace's members joined with the scheduler's users by address,
/// members first in their order, then the scheduler's users who are not
/// members.
fn join(members: &[(String, String)], users: &[SchedulingUser]) -> Vec<MemberRow> {
    let mut rows: Vec<MemberRow> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for (email, role) in members {
        let key = normalize(email);
        if seen.contains(&key) {
            continue;
        }
        let user = users.iter().find(|u| normalize(&u.email) == key);
        rows.push(MemberRow {
            email: email.clone(),
            name: preferred_name(user.map(|u| u.name.as_str()), email),
            workspace_role: Some(role.clone()),
            state: state_of(user),
            scheduler_user_id: user.map(|u| u.id.clone()),
        });
        seen.push(key);
    }
    for user in users {
        let key = normalize(&user.email);
        if seen.contains(&key) {
            continue;
        }
        rows.push(MemberRow {
            email: user.email.clone(),
            name: preferred_name(Some(&user.name), &user.email),
            workspace_role: None,
            state: state_of(Some(user)),
            scheduler_user_id: Some(user.id.clone()),
        });
        seen.push(key);
    }
    rows
}

/// A workspace role as the team section names it, or the stored value for one
/// this build does not know; "Removed from the workspace" for no role.
pub fn workspace_role_label(role: Option<&str>) -> String {
    match role.map(|r| r.trim().to_lowercase()).as_deref() {
        None => ss::team_role_removed(l()).to_owned(),
        Some("agency") => ss::team_role_agency(l()).to_owned(),
        Some("client") => ss::team_role_client(l()).to_owned(),
        Some("viewer") => ss::team_role_viewer(l()).to_owned(),
        Some(_) => role.unwrap_or_default().to_owned(),
    }
}

/// The team section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TeamSection {
    /// The scheduler's users, archived ones included.
    pub users: SectionLoad<Vec<SchedulingUser>>,
    /// The workspace's members as (address, role): `None` while awaited,
    /// `Some(None)` when they could not be read.
    pub workspace_members: Option<Option<Vec<(String, String)>>>,
    /// The teams.
    pub teams: SectionLoad<Vec<SchedulingTeam>>,
    /// The dialog open over the section.
    pub dialog: Option<TeamDialog>,
}

impl TeamSection {
    /// The people, once both reads are in: the users' failure is the list's,
    /// and the members' only drops the roles.
    pub fn members(&self) -> SectionLoad<Vec<MemberRow>> {
        match (&self.users, &self.workspace_members) {
            (SectionLoad::Failed(failure), _) => SectionLoad::Failed(failure.clone()),
            (SectionLoad::Ready(users), Some(members)) => {
                SectionLoad::Ready(join(members.as_deref().unwrap_or_default(), users))
            }
            _ => SectionLoad::Loading,
        }
    }

    /// Whether the workspace's members could not be read, which the section
    /// says instead of showing roles.
    pub fn members_degraded(&self) -> bool {
        self.workspace_members == Some(None)
    }

    /// A person's second line: their address, and their workspace role unless
    /// the members could not be read.
    pub fn member_subtitle(&self, row: &MemberRow) -> String {
        if self.members_degraded() {
            row.email.clone()
        } else {
            ss::member_subtitle_with_role(
                l(),
                &row.email,
                &workspace_role_label(row.workspace_role.as_deref()),
            )
        }
    }

    /// Who left the workspace while still a host: named at the top, unless the
    /// members could not be read.
    pub fn stranded_notice(&self) -> Option<String> {
        let SectionLoad::Ready(rows) = self.members() else {
            return None;
        };
        let names: Vec<String> = rows
            .into_iter()
            .filter(|r| r.workspace_role.is_none() && r.state == MemberState::Host)
            .map(|r| r.name)
            .collect();
        match names.as_slice() {
            _ if self.members_degraded() => None,
            [] => None,
            [one] => Some(ss::team_stranded_one(l(), one)),
            many => Some(ss::team_stranded_many(l(), &format::list(l(), many))),
        }
    }

    /// Whether a person's account may be offered "Archive account": a member
    /// who may change things, and a host with a scheduler account.
    pub fn offers_archive(row: &MemberRow, can_change: bool) -> bool {
        can_change && row.state == MemberState::Host && row.scheduler_user_id.is_some()
    }

    /// A team's member count: the scheduler's own, else its members listed.
    pub fn member_count(team: &SchedulingTeam) -> String {
        let count = team.member_count.unwrap_or(team.members().len() as i64);
        ss::member_count(l(), count)
    }

    /// The labels: the stranded notice's heading, the members' heading, what
    /// the section says when the members could not be read, an empty
    /// members list, the teams' heading, what a team is, no teams, "Create
    /// team", "Edit", "Hosts", "Archive account".
    pub fn labels() -> [&'static str; 11] {
        [
            ss::stranded_eyebrow(l()),
            ss::members_eyebrow(l()),
            ss::members_degraded(l()),
            ss::members_empty(l()),
            ss::teams_eyebrow(l()),
            ss::teams_subtitle(l()),
            ss::teams_empty(l()),
            ss::team_create(l()),
            ss::common_edit(l()),
            ss::team_members_button(l()),
            ss::team_archive(l()),
        ]
    }
}

/// A dialog over the team section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeamDialog {
    /// A team's name: create, rename, delete.
    Editor(TeamEditor),
    /// A team's hosts and their routing priorities.
    Members(TeamMembersDialog),
    /// Archiving a scheduler account.
    Archive(ArchiveDialog),
}

/// Creating a team, or renaming and deleting one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamEditor {
    /// The team edited; `None` for a create.
    pub team_id: Option<String>,
    /// Its name as it was, for the delete's question.
    pub team_name: String,
    /// The name, as typed.
    pub name: String,
    /// Why the name cannot be sent, before anything was sent.
    pub name_rejected: Option<&'static str>,
    /// Whether the delete is being asked about.
    pub confirming_delete: bool,
    /// The last write's state.
    pub write: WriteState,
}

impl TeamEditor {
    /// "New team", or the team's name.
    pub fn title(&self) -> String {
        match self.team_id {
            None => ss::team_create_title(l()).to_owned(),
            Some(_) => self.team_name.clone(),
        }
    }

    /// "Create team" or "Save name".
    pub fn submit_label(&self) -> &'static str {
        match self.team_id {
            None => ss::team_create(l()),
            Some(_) => ss::team_rename(l()),
        }
    }

    /// The way out: "Cancel" on a create, "Done" after a rename, whose writes
    /// have landed.
    pub fn close_label(&self) -> &'static str {
        match self.team_id {
            None => ss::common_cancel(l()),
            Some(_) => ss::team_close(l()),
        }
    }

    /// Whether to offer the delete: for a team, with nothing on its way.
    pub fn can_delete(&self) -> bool {
        self.team_id.is_some() && !self.write.busy()
    }

    /// "Delete Sales?".
    pub fn delete_title(&self) -> String {
        if self.team_name.is_empty() {
            ss::team_delete_title(l()).to_owned()
        } else {
            ss::team_delete_title_named(l(), &self.team_name)
        }
    }

    /// The labels: what a team is, the name's label and placeholder, that its
    /// address cannot change, the delete's body, button and way out.
    pub fn labels() -> [&'static str; 7] {
        [
            ss::teams_subtitle(l()),
            ss::team_name_label(l()),
            ss::team_name_placeholder(l()),
            ss::team_slug_read_only_hint(l()),
            ss::team_delete_body(l()),
            ss::team_delete_confirm(l()),
            ss::team_delete_keep(l()),
        ]
    }
}

/// A team's hosts: adding one, changing a routing priority, removing one,
/// asked once more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamMembersDialog {
    /// The team, as the last write answered it.
    pub team: SchedulingTeam,
    /// The scheduler's users, to add from.
    pub users: SectionLoad<Vec<SchedulingUser>>,
    /// The user chosen to add.
    pub picked: Option<String>,
    /// Each host's routing priority, as typed, by user id.
    pub priorities: Vec<(String, String)>,
    /// A priority that cannot be sent, by user id, and why.
    pub priority_rejected: Option<(String, &'static str)>,
    /// The host whose removal is being asked about.
    pub removing: Option<String>,
    /// The last write's state.
    pub write: WriteState,
}

impl TeamMembersDialog {
    fn new(team: SchedulingTeam) -> Self {
        let priorities = drafts(&team);
        Self {
            team,
            users: SectionLoad::Loading,
            picked: None,
            priorities,
            priority_rejected: None,
            removing: None,
            write: WriteState::Idle,
        }
    }

    /// The hosts.
    pub fn members(&self) -> &[SchedulingTeamMember] {
        self.team.members()
    }

    /// Who could be added: not archived, not in the team.
    pub fn addable(&self) -> Vec<&SchedulingUser> {
        self.users
            .value()
            .map(|users| {
                users
                    .iter()
                    .filter(|u| !u.archived && !self.members().iter().any(|m| m.id == u.id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A host's priority as typed.
    pub fn priority(&self, user_id: &str) -> &str {
        self.priorities
            .iter()
            .find(|(id, _)| id == user_id)
            .map_or("", |(_, value)| value.as_str())
    }

    /// The labels: a lower priority is offered first, no hosts yet, "Add
    /// member", "Done", the host's label and placeholder, nobody left to add,
    /// the priority's, "Save priority", "Remove", the removal's title, body and
    /// way out.
    pub fn labels() -> [&'static str; 13] {
        [
            ss::team_members_hint(l()),
            ss::team_members_empty(l()),
            ss::team_add(l()),
            ss::team_close(l()),
            ss::team_add_label(l()),
            ss::team_add_placeholder(l()),
            ss::team_add_none(l()),
            ss::team_priority_label(l()),
            ss::team_save_priority(l()),
            ss::team_remove(l()),
            ss::team_remove_title(l()),
            ss::team_remove_body(l()),
            ss::team_remove_keep(l()),
        ]
    }

    fn adopt(&mut self, team: SchedulingTeam) {
        self.priorities = drafts(&team);
        self.priority_rejected = None;
        self.team = team;
    }
}

fn drafts(team: &SchedulingTeam) -> Vec<(String, String)> {
    team.members()
        .iter()
        .map(|m| (m.id.clone(), m.routing_priority.to_string()))
        .collect()
}

/// Archiving a scheduler account, once its upcoming bookings are read and
/// there are none, asked once more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveDialog {
    /// The scheduler user's id.
    pub user_id: String,
    /// Their name.
    pub name: String,
    /// Their upcoming bookings.
    pub upcoming: SectionLoad<Vec<SchedulingUpcomingBooking>>,
    /// Whether the archive is being asked about.
    pub confirming: bool,
    /// The archive's state.
    pub write: WriteState,
}

impl ArchiveDialog {
    /// Whether the archive can be pressed: no upcoming booking, read.
    pub fn can_archive(&self) -> bool {
        self.upcoming.value().is_some_and(Vec::is_empty) && !self.write.busy()
    }

    /// Why it cannot yet: still counting (or the count failed), or bookings
    /// to move first.
    pub fn blocked_reason(&self) -> Option<&'static str> {
        match &self.upcoming {
            SectionLoad::Ready(rows) if rows.is_empty() => None,
            SectionLoad::Ready(_) => Some(ss::team_archive_blocked(l())),
            _ => Some(ss::team_archive_counting(l())),
        }
    }

    /// "Archive Ann's account?".
    pub fn title(&self) -> String {
        if self.name.is_empty() {
            ss::team_archive_title(l()).to_owned()
        } else {
            ss::team_archive_title_named(l(), &self.name)
        }
    }

    /// The labels: what archiving does, the confirm, the way out.
    pub fn labels() -> [&'static str; 3] {
        [
            ss::team_archive_body(l()),
            ss::team_archive_confirm(l()),
            ss::team_archive_keep(l()),
        ]
    }
}

/// What the member does on the team section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeamEvent {
    /// Open the create form.
    OpenCreate,
    /// Open a team's editor, by id.
    OpenEditor(String),
    /// The name changed.
    EditName(String),
    /// Create, or save the name.
    SaveName,
    /// Ask about deleting the team.
    AskDelete,
    /// Do not delete it.
    CancelDelete,
    /// Delete it.
    ConfirmDelete,
    /// Open a team's hosts, by id.
    OpenMembers(String),
    /// Choose a user to add.
    Pick(String),
    /// Add the user chosen.
    AddMember,
    /// A host's priority changed.
    EditPriority {
        /// The host's id.
        user_id: String,
        /// The priority, as typed.
        value: String,
    },
    /// Save a host's priority.
    SavePriority(String),
    /// Ask about removing a host.
    AskRemove(String),
    /// Do not remove them.
    CancelRemove,
    /// Remove them.
    ConfirmRemove,
    /// Open the archive of a person's scheduler account, by scheduler user id.
    OpenArchive(String),
    /// Ask about the archive.
    AskArchive,
    /// Do not archive.
    CancelArchive,
    /// Archive.
    ConfirmArchive,
    /// Close the dialog.
    CloseDialog,
}

/// What an answer the team awaits is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    Users,
    WorkspaceMembers,
    Teams,
    Name,
    Delete,
    DialogUsers,
    TeamWrite(&'static str),
    Removed,
    Upcoming,
    Archive,
}

fn send(pending: Pending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::Team(pending), Box::new(request))
}

fn reads() -> Vec<Out> {
    vec![
        send(
            Pending::Users,
            SchedulingAdminRequest::Users {
                include_archived: Some(true),
            },
        ),
        Out::Members(AdminPending::Team(Pending::WorkspaceMembers)),
        send(Pending::Teams, SchedulingAdminRequest::Teams),
    ]
}

pub(crate) fn enter(admin: &mut SchedulingAdmin, tickets: &mut Tickets) -> Vec<Out> {
    admin.forget(tickets, |p| matches!(p, AdminPending::Team(_)));
    admin.team = Some(TeamSection::default());
    reads()
}

/// The section read again, its dialog kept.
fn reread(section: &mut TeamSection) -> Vec<Out> {
    section.users = SectionLoad::Loading;
    section.workspace_members = None;
    section.teams = SectionLoad::Loading;
    reads()
}

fn all_users() -> SchedulingAdminRequest {
    SchedulingAdminRequest::Users {
        include_archived: None,
    }
}

pub(crate) fn event(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: TeamEvent,
    can_change: bool,
) -> Vec<Out> {
    let Some(section) = admin.team.as_mut() else {
        return Vec::new();
    };
    let team = |id: &str| {
        section
            .teams
            .value()
            .and_then(|teams| teams.iter().find(|t| t.id == id).cloned())
    };
    match event {
        TeamEvent::OpenCreate if can_change => {
            section.dialog = Some(TeamDialog::Editor(TeamEditor {
                team_id: None,
                team_name: String::new(),
                name: String::new(),
                name_rejected: None,
                confirming_delete: false,
                write: WriteState::Idle,
            }));
        }
        TeamEvent::OpenEditor(id) if can_change => {
            if let Some(team) = team(&id) {
                section.dialog = Some(TeamDialog::Editor(TeamEditor {
                    team_id: Some(team.id),
                    team_name: team.name.clone(),
                    name: team.name,
                    name_rejected: None,
                    confirming_delete: false,
                    write: WriteState::Idle,
                }));
            }
        }
        TeamEvent::OpenMembers(id) if can_change => {
            if let Some(team) = team(&id) {
                section.dialog = Some(TeamDialog::Members(TeamMembersDialog::new(team)));
                return vec![send(Pending::DialogUsers, all_users())];
            }
        }
        TeamEvent::OpenArchive(user_id) if can_change => {
            let row = section.members().value().and_then(|rows| {
                rows.iter()
                    .find(|r| {
                        r.scheduler_user_id.as_deref() == Some(user_id.as_str())
                            && r.state == MemberState::Host
                    })
                    .cloned()
            });
            if let Some(row) = row {
                section.dialog = Some(TeamDialog::Archive(ArchiveDialog {
                    user_id: user_id.clone(),
                    name: row.name,
                    upcoming: SectionLoad::Loading,
                    confirming: false,
                    write: WriteState::Idle,
                }));
                return vec![send(
                    Pending::Upcoming,
                    SchedulingAdminRequest::UpcomingBookings { id: user_id },
                )];
            }
        }
        TeamEvent::CloseDialog => section.dialog = None,
        event => {
            return match &mut section.dialog {
                Some(TeamDialog::Editor(editor)) => editor_event(editor, event),
                Some(TeamDialog::Members(members)) => members_event(members, event),
                Some(TeamDialog::Archive(archive)) => archive_event(archive, event),
                None => Vec::new(),
            };
        }
    }
    Vec::new()
}

fn editor_event(editor: &mut TeamEditor, event: TeamEvent) -> Vec<Out> {
    match event {
        TeamEvent::EditName(name) => {
            editor.name = name;
            editor.name_rejected = None;
            if editor.write.failure().is_some() {
                editor.write = WriteState::Idle;
            }
        }
        TeamEvent::SaveName if !editor.write.busy() => {
            let name = editor.name.trim().to_owned();
            editor.name_rejected = if name.is_empty() {
                Some(ss::team_name_required(l()))
            } else if name.chars().count() > TEAM_NAME_MAX {
                Some(ss::team_name_too_long(l()))
            } else {
                None
            };
            if editor.name_rejected.is_none() {
                editor.write = WriteState::Working;
                let request = match &editor.team_id {
                    None => SchedulingAdminRequest::CreateTeam { name, slug: None },
                    Some(id) => SchedulingAdminRequest::PatchTeam {
                        id: id.clone(),
                        name: Some(name),
                        slug: None,
                    },
                };
                return vec![send(Pending::Name, request)];
            }
        }
        TeamEvent::AskDelete | TeamEvent::CancelDelete => {
            editor.confirming_delete = event == TeamEvent::AskDelete && editor.can_delete();
        }
        TeamEvent::ConfirmDelete if editor.confirming_delete && editor.can_delete() => {
            editor.confirming_delete = false;
            editor.write = WriteState::Working;
            let id = editor.team_id.clone().unwrap_or_default();
            return vec![send(
                Pending::Delete,
                SchedulingAdminRequest::DeleteTeam { id },
            )];
        }
        _ => {}
    }
    Vec::new()
}

fn members_event(dialog: &mut TeamMembersDialog, event: TeamEvent) -> Vec<Out> {
    if dialog.write.busy() {
        return Vec::new();
    }
    let id = dialog.team.id.clone();
    match event {
        TeamEvent::Pick(user_id) => {
            dialog.picked = Some(user_id);
            if dialog.write.failure().is_some() {
                dialog.write = WriteState::Idle;
            }
        }
        TeamEvent::AddMember => match dialog.picked.take() {
            None => {
                dialog.write = WriteState::Failed(FailureText::final_(ss::team_add_required(l())));
            }
            Some(user_id) => {
                dialog.write = WriteState::Working;
                return vec![send(
                    Pending::TeamWrite(ss::team_add_done(l())),
                    SchedulingAdminRequest::AddTeamMember {
                        id,
                        user_id,
                        routing_priority: None,
                    },
                )];
            }
        },
        TeamEvent::EditPriority { user_id, value } => {
            match dialog.priorities.iter_mut().find(|(id, _)| *id == user_id) {
                Some(entry) => entry.1 = value,
                None => dialog.priorities.push((user_id, value)),
            }
            dialog.priority_rejected = None;
            if dialog.write.failure().is_some() {
                dialog.write = WriteState::Idle;
            }
        }
        TeamEvent::SavePriority(user_id) => match whole(dialog.priority(&user_id), 0) {
            None => {
                dialog.priority_rejected = Some((user_id, ss::team_priority_invalid(l())));
            }
            Some(routing_priority) => {
                dialog.write = WriteState::Working;
                return vec![send(
                    Pending::TeamWrite(ss::team_priority_done(l())),
                    SchedulingAdminRequest::PatchTeamMember {
                        id,
                        user_id,
                        routing_priority,
                    },
                )];
            }
        },
        TeamEvent::AskRemove(user_id) => dialog.removing = Some(user_id),
        TeamEvent::CancelRemove => dialog.removing = None,
        TeamEvent::ConfirmRemove => {
            if let Some(user_id) = dialog.removing.take() {
                dialog.write = WriteState::Working;
                return vec![send(
                    Pending::Removed,
                    SchedulingAdminRequest::RemoveTeamMember { id, user_id },
                )];
            }
        }
        _ => {}
    }
    Vec::new()
}

fn archive_event(dialog: &mut ArchiveDialog, event: TeamEvent) -> Vec<Out> {
    match event {
        TeamEvent::AskArchive => dialog.confirming = dialog.can_archive(),
        TeamEvent::CancelArchive => dialog.confirming = false,
        TeamEvent::ConfirmArchive if dialog.confirming && dialog.can_archive() => {
            dialog.confirming = false;
            dialog.write = WriteState::Working;
            return vec![send(
                Pending::Archive,
                SchedulingAdminRequest::ArchiveUser {
                    id: dialog.user_id.clone(),
                },
            )];
        }
        _ => {}
    }
    Vec::new()
}

pub(crate) fn answered(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    pending: Pending,
    answer: Answer,
) -> Vec<Out> {
    admin
        .team
        .as_mut()
        .map(|section| team_answer(section, pending, answer))
        .unwrap_or_default()
}

fn team_answer(section: &mut TeamSection, pending: Pending, answer: Answer) -> Vec<Out> {
    match (pending, &mut section.dialog) {
        (Pending::Users, _) => section.users = load(answer, data!(Users)),
        (Pending::Teams, _) => section.teams = load(answer, data!(Teams)),
        (Pending::Name, Some(TeamDialog::Editor(editor))) => match answer {
            Ok(_) => {
                editor.write = WriteState::Done(
                    match editor.team_id {
                        None => ss::team_create_done(l()),
                        Some(_) => ss::team_rename_done(l()),
                    }
                    .to_owned(),
                );
                return reread(section);
            }
            Err(failure) => editor.write = WriteState::Failed(failure),
        },
        (Pending::Delete, Some(TeamDialog::Editor(editor))) => match answer {
            Ok(_) => {
                editor.write = WriteState::Done(ss::team_delete_done(l()).to_owned());
                return reread(section);
            }
            Err(failure) => editor.write = WriteState::Failed(failure),
        },
        (Pending::DialogUsers, Some(TeamDialog::Members(dialog))) => {
            dialog.users = load(answer, data!(Users));
        }
        // A host added or a priority saved: the team as answered, and the
        // section read again.
        (Pending::TeamWrite(done), Some(TeamDialog::Members(dialog))) => match answer {
            Ok(data) => {
                if let Some(team) = data!(Team)(data) {
                    dialog.adopt(team);
                }
                dialog.write = WriteState::Done(done.to_owned());
                return reread(section);
            }
            Err(failure) => dialog.write = WriteState::Failed(failure),
        },
        // A host removed: the team is read again, which answers it.
        (Pending::Removed, Some(TeamDialog::Members(dialog))) => match answer {
            Ok(_) => {
                return vec![send(
                    Pending::TeamWrite(ss::team_remove_done(l())),
                    SchedulingAdminRequest::Team {
                        id: dialog.team.id.clone(),
                    },
                )];
            }
            Err(failure) => dialog.write = WriteState::Failed(failure),
        },
        (Pending::Upcoming, Some(TeamDialog::Archive(dialog))) => {
            dialog.upcoming = load(answer, data!(UpcomingBookings));
        }
        // The scheduler refuses an archive for several reasons it does not
        // name: an unknown refusal names them all.
        (Pending::Archive, Some(TeamDialog::Archive(dialog))) => match answer {
            Ok(_) => {
                dialog.write = WriteState::Done(if dialog.name.is_empty() {
                    ss::team_archive_done(l()).to_owned()
                } else {
                    ss::team_archive_done_named(l(), &dialog.name)
                });
                return reread(section);
            }
            Err(failure) => {
                dialog.write = WriteState::Failed(if failure.message == ss::failure_unknown(l()) {
                    FailureText::final_(ss::team_archive_refused(l()))
                } else {
                    failure
                });
            }
        },
        // An answer for a dialog that has closed.
        _ => {}
    }
    Vec::new()
}

/// The workspace's members arrived (or failed to): only the roles depend on
/// them.
pub(crate) fn members_answered(
    admin: &mut SchedulingAdmin,
    result: Result<MemberListResponse, ApiError>,
) {
    let members = result.ok().map(|answer| {
        answer
            .members
            .into_iter()
            .map(|m| (m.email, m.role))
            .collect()
    });
    // The section is open: leaving it stops the read being awaited.
    let mut read = Some(members);
    admin
        .team
        .iter_mut()
        .for_each(|section| section.workspace_members = read.take());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_falls_back_to_the_address_before_its_at() {
        assert_eq!(name_from_email(" ann@example.com "), "ann");
        assert_eq!(name_from_email("@example.com"), "@example.com");
        assert_eq!(preferred_name(Some("  "), "bo@example.com"), "bo");
        assert_eq!(preferred_name(Some(" Ann "), "x"), "Ann");
    }
}
