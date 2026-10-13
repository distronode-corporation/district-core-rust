//! The workspaces the signed-in member may open, and which one is open.
//!
//! The open workspace is chosen here and named on every request. It cannot be
//! left to the service: without a workspace the service falls back to the first
//! of its own membership list, which knows nothing of what the user picked in this
//! app, and the screen would show one workspace's name over another's numbers
//! with no error anywhere.

use district_api::ApiError;
use district_model::{WorkspaceEntry, WorkspaceListResponse};

use crate::failure::{FailureText, regions};
use crate::text::{l, s};

/// The workspace list, and what it means for the screens.
///
/// Four of these states show no workspace, and they are kept apart on purpose:
/// only [`NoWorkspaces`](Self::NoWorkspaces) means what an empty screen looks
/// like it means. Showing [`Unavailable`](Self::Unavailable) as "no workspaces"
/// is indistinguishable from losing the account to the person reading it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspacesState {
    /// Being read.
    Loading,
    /// At least one workspace, and one of them open.
    Ready(Workspaces),
    /// The account belongs to no workspace, and the answer was complete.
    NoWorkspaces,
    /// Every workspace the account has lacks an active subscription. Billing is
    /// not managed in this app: say so, and offer no way to pay.
    BillingBlocked {
        /// How many workspaces are affected.
        inactive_count: i64,
    },
    /// The list could not be read, or was incomplete with nothing in it. Always
    /// retryable, never an empty account.
    Unavailable(FailureText),
}

impl WorkspacesState {
    /// The body for [`NoWorkspaces`](Self::NoWorkspaces) in place of
    /// [`message`](Self::message) while the plans are offered
    /// ([`SignedIn::no_workspace_message`](crate::SignedIn::no_workspace_message)):
    /// choosing one is what sets up a workspace.
    pub const CHOOSE_PLAN_MESSAGE: &'static str =
        district_l10n::english::WORKSPACES_CHOOSE_PLAN_MESSAGE;

    /// The heading for [`NoWorkspaces`](Self::NoWorkspaces) while the workspace
    /// a checkout may have paid for is waited for
    /// ([`SignedIn::no_workspace_title`](crate::SignedIn::no_workspace_title)).
    pub const SETTING_UP_TITLE: &'static str = district_l10n::english::WORKSPACES_SETTING_UP_TITLE;

    /// The body meanwhile, in place of [`CHOOSE_PLAN_MESSAGE`](Self::CHOOSE_PLAN_MESSAGE):
    /// no plans are offered, so nobody pays twice for one workspace.
    pub const SETTING_UP_MESSAGE: &'static str =
        district_l10n::english::WORKSPACES_SETTING_UP_MESSAGE;

    /// The body once that wait is over with still no workspace: the plans are
    /// offered again, and a workspace paid for may still be on its way.
    pub const CHOOSE_PLAN_AFTER_CHECKOUT_MESSAGE: &'static str =
        district_l10n::english::WORKSPACES_CHOOSE_PLAN_AFTER_CHECKOUT_MESSAGE;

    /// The heading for a state that shows no workspace, or `None` for the other
    /// two.
    pub fn title(&self) -> Option<&'static str> {
        match self {
            Self::Loading | Self::Ready(_) => None,
            Self::NoWorkspaces => Some(s::workspaces_none_title(l())),
            Self::BillingBlocked { .. } => Some(s::workspaces_billing_blocked_title(l())),
            Self::Unavailable(_) => Some(s::workspaces_failed_title(l())),
        }
    }

    /// The body for a state that shows no workspace, or `None` for the other two.
    pub fn message(&self) -> Option<String> {
        match self {
            Self::Loading | Self::Ready(_) => None,
            Self::NoWorkspaces => Some(s::workspaces_none_message(l()).to_owned()),
            Self::BillingBlocked { inactive_count: 1 } => {
                Some(s::workspaces_billing_blocked_one(l()).to_owned())
            }
            Self::BillingBlocked { inactive_count } => {
                Some(s::workspaces_billing_blocked_many(l(), *inactive_count))
            }
            Self::Unavailable(failure) => Some(failure.message.clone()),
        }
    }
}

/// The workspaces that can be opened, and the open one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspaces {
    /// Every workspace listed, in the service's order.
    pub list: Vec<WorkspaceEntry>,
    active: usize,
    /// Regions that did not answer. Not empty means the list is short, and the
    /// switcher must say so rather than present it as everything.
    pub degraded_regions: Vec<String>,
}

impl Workspaces {
    /// The open workspace.
    pub fn active(&self) -> &WorkspaceEntry {
        &self.list[self.active]
    }

    /// Whether to offer a switcher at all.
    pub fn can_switch(&self) -> bool {
        self.list.len() > 1
    }

    /// The warning to show beside the switcher when the list is short, or `None`.
    pub fn partial_warning(&self) -> Option<String> {
        (!self.degraded_regions.is_empty())
            .then(|| s::workspaces_partial_warning(l(), &regions(&self.degraded_regions)))
    }

    /// Shows the open workspace under `name`, the name the service stored when
    /// it was renamed.
    pub(crate) fn rename_active(&mut self, name: &str) {
        name.clone_into(&mut self.list[self.active].name);
    }

    /// Opens the workspace `id`, if it is listed and not already open. Answers
    /// whether anything changed.
    pub(crate) fn select(&mut self, id: &str) -> bool {
        match self.list.iter().position(|entry| entry.id == id) {
            Some(index) if index != self.active => {
                self.active = index;
                true
            }
            _ => false,
        }
    }
}

/// What a workspace list comes to.
pub(crate) struct Resolved {
    pub(crate) state: WorkspacesState,
    /// The remembered workspace is gone from a complete list, so the memory
    /// should be dropped rather than looked for at every start.
    pub(crate) forget_remembered: bool,
}

/// Chooses the open workspace from `response`, preferring, in order:
///
/// 1. `remembered`, the workspace last chosen in this app;
/// 2. the account's default, as the service reports it (it echoes a stored
///    choice without checking it, so it may name a workspace that is not
///    listed);
/// 3. the first listed, which is the service's own choice.
///
/// Only a listed id is ever opened, which is what makes a stale memory harmless.
pub(crate) fn resolve(response: WorkspaceListResponse, remembered: Option<&str>) -> Resolved {
    let WorkspaceListResponse {
        workspaces,
        degraded_regions,
        inactive_count,
        default_workspace_id,
        ..
    } = response;
    let position =
        |id: Option<&str>| id.and_then(|id| workspaces.iter().position(|entry| entry.id == id));
    let remembered_index = position(remembered);
    let active = remembered_index
        .or_else(|| position(default_workspace_id.as_deref()))
        .or((!workspaces.is_empty()).then_some(0));
    // Only a complete answer can say the remembered workspace is gone. An
    // incomplete one may simply have missed its region.
    let forget_remembered =
        remembered.is_some() && remembered_index.is_none() && degraded_regions.is_empty();

    let state = match active {
        Some(active) => WorkspacesState::Ready(Workspaces {
            list: workspaces,
            active,
            degraded_regions,
        }),
        // Checked before billing and before "none": an incomplete answer is
        // neither of those, it is a question not yet answered.
        None if !degraded_regions.is_empty() => WorkspacesState::Unavailable(FailureText {
            degraded_regions,
            ..FailureText::retryable(s::workspaces_regions_degraded(l()))
        }),
        None if inactive_count > 0 => WorkspacesState::BillingBlocked { inactive_count },
        None => WorkspacesState::NoWorkspaces,
    };
    Resolved {
        state,
        forget_remembered,
    }
}

/// The state a failed read leaves. The caller has already handled an ended
/// session.
///
/// A 404 is what the service answers a workspace-scoped read from an account with
/// no workspace at all, so it is that state rather than a failure.
pub(crate) fn failed(error: &ApiError) -> WorkspacesState {
    match error {
        ApiError::NotFound(_) => WorkspacesState::NoWorkspaces,
        other => WorkspacesState::Unavailable(FailureText::from_api_error(other)),
    }
}
