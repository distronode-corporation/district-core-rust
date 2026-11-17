//! The new-customer setup wizard: its state, the step saves, the website
//! lookup, the test call, the call forwarding check and the connections read.
//!
//! Every `setup/*` route is the workspace owner's alone (the workspace's
//! `ownerId`, not a role): anyone else gets a 403, and a workspace from before
//! the wizard gets a 409 `setup_not_available`. Buying a number, the plan and a
//! calendar provider's sign-in are not here: they charge a card or need a
//! browser, and stay on the web.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// A setup step that has not been done.
pub const SETUP_STEP_TODO: &str = "todo";
/// A setup step that has been done.
pub const SETUP_STEP_DONE: &str = "done";
/// A setup step the owner chose to skip.
pub const SETUP_STEP_SKIPPED: &str = "skipped";

/// `GET /api/district/setup`: where the workspace owner is in the setup wizard.
///
/// The overview reads one fact from this, through
/// [`needs_web_setup`](Self::needs_web_setup): whether to show the card that
/// opens the setup wizard for the owner to finish. Only the workspace's owner
/// gets an answer; anyone else gets an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SetupResponse {
    /// The wizard's progress. `None` means this workspace never goes through the
    /// wizard (it existed before the wizard did), not that setup has not started.
    pub setup_progress: Option<SetupProgress>,
    /// The owner's answers about the business (hours, services and so on). Free
    /// form, edited only on the web, and not read by the app, so it is kept as
    /// plain JSON: a reshaped answer cannot fail this response.
    pub business_facts: Option<Map<String, Value>>,
    /// The region the workspace's data lives in, for example `ca`.
    pub region: Option<String>,
    /// The subscription tier as stored, for example `VoicePro`.
    pub tier: Option<String>,
    /// How many phone numbers the tier includes, or `None` when there is no limit.
    pub included_numbers: Option<i64>,
    /// How many phone numbers the workspace holds now.
    #[serde(default)]
    pub numbers_held: i64,
}

impl SetupResponse {
    /// Whether to show the overview's "Finish setting up" card: the workspace
    /// is in the wizard and has not finished it. A workspace that never goes
    /// through the wizard, and one that has finished, both answer `false`.
    pub fn needs_web_setup(&self) -> bool {
        self.setup_progress
            .as_ref()
            .is_some_and(|progress| progress.completed_at.is_none())
    }
}

/// The wizard's stored progress. The server leaves each timestamp out until the
/// thing it records has happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SetupProgress {
    /// The state of each step.
    #[serde(default)]
    pub steps: SetupSteps,
    /// When the subscription was paid for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paid_at: Option<String>,
    /// When the workspace received its first real call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_real_call_at: Option<String>,
    /// When setup was first finished. Absent while the owner is still setting up,
    /// which is what [`SetupResponse::needs_web_setup`] reads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// The owner's consent to a test call, with the wording they agreed to. Not
    /// read by the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_call_consent: Option<Map<String, Value>>,
    /// The state of the call forwarding check. Not read by the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forwarding_check: Option<Map<String, Value>>,
}

/// The six wizard steps, each [`SETUP_STEP_TODO`], [`SETUP_STEP_DONE`] or
/// [`SETUP_STEP_SKIPPED`].
///
/// Strings rather than an enum, so a state the server adds later cannot make the
/// whole response unreadable. A step the server leaves out reads as not done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(default)]
pub struct SetupSteps {
    /// Describing the business.
    pub business: String,
    /// Getting a phone number.
    pub number: String,
    /// Setting up the AI receptionist.
    pub receptionist: String,
    /// Deciding how callers are handled.
    pub callers: String,
    /// Trying calls.
    pub calls: String,
    /// Going live.
    pub golive: String,
}

impl Default for SetupSteps {
    fn default() -> Self {
        Self {
            business: SETUP_STEP_TODO.to_owned(),
            number: SETUP_STEP_TODO.to_owned(),
            receptionist: SETUP_STEP_TODO.to_owned(),
            callers: SETUP_STEP_TODO.to_owned(),
            calls: SETUP_STEP_TODO.to_owned(),
            golive: SETUP_STEP_TODO.to_owned(),
        }
    }
}

/// The six steps of the setup wizard, in the order the service lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupStep {
    /// Describing the business.
    Business,
    /// Getting a phone number.
    Number,
    /// Setting up the AI receptionist.
    Receptionist,
    /// What callers should know.
    Callers,
    /// How calls are answered.
    Calls,
    /// Testing and going live.
    Golive,
}

impl SetupStep {
    /// Every step, in the wizard's order.
    pub const ALL: [Self; 6] = [
        Self::Business,
        Self::Number,
        Self::Receptionist,
        Self::Callers,
        Self::Calls,
        Self::Golive,
    ];

    /// The step as the service spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Business => "business",
            Self::Number => "number",
            Self::Receptionist => "receptionist",
            Self::Callers => "callers",
            Self::Calls => "calls",
            Self::Golive => "golive",
        }
    }
}

/// What the owner did with a step. The service accepts exactly these three
/// values; a step left out of a save keeps the state it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupStepState {
    /// [`SETUP_STEP_TODO`].
    Todo,
    /// [`SETUP_STEP_DONE`].
    Done,
    /// [`SETUP_STEP_SKIPPED`].
    Skipped,
}

impl SetupStepState {
    /// The state as the service spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => SETUP_STEP_TODO,
            Self::Done => SETUP_STEP_DONE,
            Self::Skipped => SETUP_STEP_SKIPPED,
        }
    }

    /// The state the service sent, or `None` for a value this client does not
    /// know. A state the service adds later is therefore not [`Todo`](Self::Todo).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            SETUP_STEP_TODO => Some(Self::Todo),
            SETUP_STEP_DONE => Some(Self::Done),
            SETUP_STEP_SKIPPED => Some(Self::Skipped),
            _ => None,
        }
    }
}

impl SetupSteps {
    /// The state of `step` as the service answered it, or `None` for a value
    /// this client does not know.
    pub fn state(&self, step: SetupStep) -> Option<SetupStepState> {
        let raw = match step {
            SetupStep::Business => &self.business,
            SetupStep::Number => &self.number,
            SetupStep::Receptionist => &self.receptionist,
            SetupStep::Callers => &self.callers,
            SetupStep::Calls => &self.calls,
            SetupStep::Golive => &self.golive,
        };
        SetupStepState::parse(raw)
    }
}

/// `POST /api/district/setup/lookup`: a draft of the business's facts, read off
/// its own website.
///
/// A suggestion, never a write: the service stores nothing, the owner reviews
/// the draft and saves what they keep through `PATCH /api/district/setup`.
/// Every service and question arrives switched off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SetupLookupResponse {
    /// The drafted facts.
    pub draft: SetupLookupDraft,
    /// The page the draft was read from.
    pub source: SetupLookupSource,
}

/// The drafted facts. `address` and `weekly_hours` stay plain JSON: they go
/// back to the service unchanged inside a business facts save, and the service
/// owns their validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SetupLookupDraft {
    /// The business's address, as a business facts `address` section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Map<String, Value>>,
    /// The opening hours, as a business facts `weeklyHours` section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly_hours: Option<Map<String, Value>>,
    /// The services on the page.
    #[serde(default)]
    pub services: Vec<SetupDraftService>,
    /// The questions and answers on the page.
    #[serde(default)]
    pub faqs: Vec<SetupDraftFaq>,
    /// How many drafted items the service dropped as not supported by the
    /// page's text.
    #[serde(default)]
    pub dropped: i64,
}

/// One service on the business's website. The price is free text, read aloud
/// as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SetupDraftService {
    /// The service's name.
    pub name: String,
    /// Its price as the page writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<String>,
    /// Whether callers hear about it. Always `false` in a draft.
    pub enabled: bool,
}

/// One question and answer on the business's website.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SetupDraftFaq {
    /// The question.
    pub question: String,
    /// Its answer.
    pub answer: String,
    /// Whether the receptionist answers it. Always `false` in a draft.
    pub enabled: bool,
}

/// Where a draft came from: today always `kind: "website"`, with the page
/// actually read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SetupLookupSource {
    /// `website`.
    pub kind: String,
    /// The page read.
    pub url: String,
}

/// The 422 `website_unreadable` body of a lookup. The client's error keeps the
/// code and the message; `reason` (`host_unavailable`, `blocked_scheme`,
/// `invalid_url` and the other page refusals) is here for a caller that reads
/// the body itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SetupLookupRefusal {
    /// The service's English sentence.
    pub error: String,
    /// `website_unreadable`.
    pub code: String,
    /// Why the page could not be read.
    pub reason: String,
}

/// `POST /api/district/setup/test-call`: the receptionist is calling the
/// owner's phone. The service records the owner's consent in its own words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct SetupTestCallResponse {
    /// `true` on a successful answer.
    #[serde(default)]
    pub success: bool,
    /// The call's id.
    pub call_id: String,
}

/// Which calls the owner says their business line forwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ForwardingCheckMode {
    /// Every call.
    All,
    /// Only the calls nobody picks up.
    Unanswered,
}

impl ForwardingCheckMode {
    /// The mode as the service spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Unanswered => "unanswered",
        }
    }
}

/// [`ForwardingCheck::status`] while the check call has not arrived.
pub const FORWARDING_CHECK_PENDING: &str = "pending";
/// [`ForwardingCheck::status`] once the check call arrived on the District
/// number.
pub const FORWARDING_CHECK_PASSED: &str = "passed";

/// `GET` and `POST /api/district/setup/forwarding-check`: whether the business
/// line forwards to the District number. The `POST` answers 202 with the same
/// body; poll the `GET` while [`ForwardingCheck::is_pending`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct ForwardingCheckResponse {
    /// `false` when the region has no verifier, so no check can be offered.
    pub available: bool,
    /// The latest check, or `None` before the first one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forwarding_check: Option<ForwardingCheck>,
    /// Checks left today.
    pub attempts_left: i64,
}

/// One forwarding check. The status, carrier and failure stay strings: the
/// service owns those vocabularies, and a new value must not fail the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct ForwardingCheck {
    /// The check's id.
    pub id: String,
    /// `pending`, `passed` or `failed`.
    pub status: String,
    /// `all` or `unanswered`.
    pub mode: String,
    /// The business line called.
    pub business_line: String,
    /// When the check started.
    pub started_at: String,
    /// When its status last changed.
    pub at: String,
    /// The number the check calls from.
    pub verifier_number: String,
    /// The carrier placing the check call.
    pub carrier: String,
    /// Why it failed, once it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    /// The District number the check call arrived on, once it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrived_on: Option<String>,
    /// Set when a newer check replaced this one before it finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_at: Option<String>,
}

impl ForwardingCheck {
    /// Whether the check call has not arrived yet.
    pub fn is_pending(&self) -> bool {
        self.status == FORWARDING_CHECK_PENDING
    }

    /// Whether the check call arrived on the District number.
    pub fn has_passed(&self) -> bool {
        self.status == FORWARDING_CHECK_PASSED
    }
}

/// `GET /api/district/connections`: the workspace's connected providers.
///
/// A viewer gets the rows without [`calendar`](Self::calendar), so its absence
/// is not "no calendar".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsResponse {
    /// One row per connected provider, as stored. Required, so a stray `{}`
    /// is not read as a workspace with nothing connected.
    pub connections: Vec<WorkspaceConnection>,
    /// Which calendar the receptionist books into, for anyone but a viewer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calendar: Option<CalendarResolution>,
}

impl ConnectionsResponse {
    /// Whether a calendar the receptionist can book into is connected.
    pub fn has_active_calendar(&self) -> bool {
        self.calendar
            .as_ref()
            .is_some_and(|calendar| calendar.active_provider.is_some())
    }
}

/// One connection row, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConnection {
    /// For example `google-calendar`.
    pub provider: String,
    /// For example `active` or `error`.
    pub status: String,
}

/// Which calendar the receptionist books into, and why that one. Both are
/// `None` when none is connected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "strict-contracts", serde(deny_unknown_fields))]
#[serde(rename_all = "camelCase")]
pub struct CalendarResolution {
    /// The provider in use.
    #[serde(default)]
    pub active_provider: Option<String>,
    /// Why that one.
    #[serde(default)]
    pub source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_answer_is_a_workspace_outside_the_wizard() {
        let response: SetupResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(response.setup_progress, None);
        assert_eq!(response.numbers_held, 0);
        assert!(!response.needs_web_setup());
    }

    #[test]
    fn empty_progress_has_every_step_to_do() {
        let progress: SetupProgress = serde_json::from_str("{}").unwrap();
        assert_eq!(progress.steps, SetupSteps::default());
        for step in [
            &progress.steps.business,
            &progress.steps.number,
            &progress.steps.receptionist,
            &progress.steps.callers,
            &progress.steps.calls,
            &progress.steps.golive,
        ] {
            assert_eq!(step, SETUP_STEP_TODO);
        }
    }

    #[test]
    fn a_step_left_out_reads_as_not_done() {
        let steps: SetupSteps = serde_json::from_str(r#"{"business":"skipped"}"#).unwrap();
        assert_eq!(steps.business, SETUP_STEP_SKIPPED);
        assert_eq!(steps.golive, SETUP_STEP_TODO);
        assert_ne!(SETUP_STEP_DONE, SETUP_STEP_TODO);
    }

    #[test]
    fn step_states_read_only_the_three_the_service_sends() {
        let steps: SetupSteps =
            serde_json::from_str(r#"{"business":"done","number":"skipped","calls":"later"}"#)
                .unwrap();
        assert_eq!(steps.state(SetupStep::Business), Some(SetupStepState::Done));
        assert_eq!(
            steps.state(SetupStep::Number),
            Some(SetupStepState::Skipped)
        );
        assert_eq!(
            steps.state(SetupStep::Receptionist),
            Some(SetupStepState::Todo)
        );
        assert_eq!(steps.state(SetupStep::Callers), Some(SetupStepState::Todo));
        assert_eq!(steps.state(SetupStep::Calls), None);
        assert_eq!(steps.state(SetupStep::Golive), Some(SetupStepState::Todo));
        for state in [
            SetupStepState::Todo,
            SetupStepState::Done,
            SetupStepState::Skipped,
        ] {
            assert_eq!(SetupStepState::parse(state.as_str()), Some(state));
            assert_eq!(
                serde_json::to_value(state).unwrap(),
                serde_json::json!(state.as_str())
            );
        }
    }

    #[test]
    fn steps_are_spelled_as_the_service_spells_them() {
        let names: Vec<&str> = SetupStep::ALL.iter().map(|step| step.as_str()).collect();
        assert_eq!(
            names,
            [
                "business",
                "number",
                "receptionist",
                "callers",
                "calls",
                "golive"
            ]
        );
        for step in SetupStep::ALL {
            assert_eq!(
                serde_json::to_value(step).unwrap(),
                serde_json::json!(step.as_str())
            );
        }
        for mode in [ForwardingCheckMode::All, ForwardingCheckMode::Unanswered] {
            assert_eq!(
                serde_json::to_value(mode).unwrap(),
                serde_json::json!(mode.as_str())
            );
        }
    }

    #[test]
    fn a_forwarding_check_says_whether_it_is_pending_or_passed() {
        let check = |status: &str| ForwardingCheck {
            id: "fwd_1".to_owned(),
            status: status.to_owned(),
            mode: "all".to_owned(),
            business_line: "+14165550123".to_owned(),
            started_at: "2026-09-23T15:10:00.000Z".to_owned(),
            at: "2026-09-23T15:10:00.000Z".to_owned(),
            verifier_number: "+14165550143".to_owned(),
            carrier: "twilio".to_owned(),
            failure: None,
            arrived_on: None,
            superseded_at: None,
        };
        assert!(check("pending").is_pending());
        assert!(!check("pending").has_passed());
        assert!(check("passed").has_passed());
        assert!(!check("failed").is_pending());
        assert!(!check("failed").has_passed());
    }

    #[test]
    fn only_a_resolved_provider_is_an_active_calendar() {
        let read = |calendar: Option<CalendarResolution>| ConnectionsResponse {
            connections: Vec::new(),
            calendar,
        };
        assert!(!read(None).has_active_calendar());
        assert!(
            !read(Some(CalendarResolution {
                active_provider: None,
                source: None
            }))
            .has_active_calendar()
        );
        assert!(
            read(Some(CalendarResolution {
                active_provider: Some("google".to_owned()),
                source: None,
            }))
            .has_active_calendar()
        );
    }
}
