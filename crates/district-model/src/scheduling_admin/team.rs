//! The scheduler's own users and teams.
//!
//! A scheduler user is not a workspace member: the two populations differ, and
//! the scheduler joins them itself. Every id here is the scheduler's.

use serde::{Deserialize, Serialize};

use super::envelope::nullish;

/// A team a user belongs to, as `users.list` names it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingUserTeam {
    /// The team's id.
    pub id: String,
    /// Its name.
    pub name: String,
}

/// One scheduler user. `users.list` answers a bare array of these.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingUser {
    /// Their scheduler id.
    pub id: String,
    /// Their address.
    pub email: String,
    /// Their name. Can be empty, through single sign-on.
    pub name: String,
    /// Their time zone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Whether they administer the scheduler.
    pub is_admin: bool,
    /// Whether they own it.
    pub is_owner: bool,
    /// `owner`, `admin` or `member`.
    pub role: String,
    /// Whether they sign in with an email link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email_login: Option<bool>,
    /// How they sign in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Their picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// When they were added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Whether they were archived: they cannot sign in and are skipped in
    /// routing.
    pub archived: bool,
    /// When.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    /// By whom.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_by_name: Option<String>,
    /// Their teams.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub teams: Option<Option<Vec<SchedulingUserTeam>>>,
}

impl SchedulingUser {
    /// Their teams, left out and `null` alike being none.
    pub fn teams(&self) -> &[SchedulingUserTeam] {
        self.teams
            .as_ref()
            .and_then(|value| value.as_deref())
            .unwrap_or_default()
    }
}

/// `users.archive`: the user was archived.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingUserArchived {
    /// Whether it was done.
    pub ok: bool,
    /// When.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
}

/// One booking a user still hosts, which stops them being archived.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingUpcomingBooking {
    /// The booking's id.
    pub id: String,
    /// When it starts.
    pub start_at: String,
    /// When it ends.
    pub end_at: String,
    /// The event type's name.
    pub event_type_name: String,
    /// The event type's slug.
    pub event_type_slug: String,
    /// Who booked.
    pub attendee_name: String,
    /// Their address.
    pub attendee_email: String,
}

/// One member of a team.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingTeamMember {
    /// Their scheduler id.
    pub id: String,
    /// Their name.
    pub name: String,
    /// Their address.
    pub email: String,
    /// Their picture.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    /// Their place in the team's rotation, lowest first.
    pub routing_priority: i64,
    /// Whether they were archived.
    pub archived: bool,
}

/// One team.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
pub struct SchedulingTeam {
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its slug.
    pub slug: String,
    /// When it was made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// How many members it has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_count: Option<i64>,
    /// Its members.
    #[serde(
        default,
        deserialize_with = "nullish",
        skip_serializing_if = "Option::is_none"
    )]
    pub members: Option<Option<Vec<SchedulingTeamMember>>>,
}

impl SchedulingTeam {
    /// Its members, left out and `null` alike being none.
    pub fn members(&self) -> &[SchedulingTeamMember] {
        self.members
            .as_ref()
            .and_then(|value| value.as_deref())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_out_and_null_lists_are_empty() {
        let team: SchedulingTeam = serde_json::from_str(
            r#"{"id":"t","name":"n","slug":"s","members":[{"id":"u","name":"","email":"e","routing_priority":0,"archived":false}]}"#,
        )
        .unwrap();
        assert_eq!(team.members().len(), 1);
        assert!(SchedulingTeam::default().members().is_empty());
        let user: SchedulingUser = serde_json::from_str(
            r#"{"id":"u","email":"e","name":"n","is_admin":false,"is_owner":false,"role":"member","archived":false,"teams":[{"id":"t","name":"Sales"}]}"#,
        )
        .unwrap();
        assert_eq!(user.teams().len(), 1);
        assert!(SchedulingUser::default().teams().is_empty());
    }
}
