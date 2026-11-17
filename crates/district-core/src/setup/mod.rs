//! The setup wizard, in the app: the six steps of the web's wizard, in the
//! web's order, for the workspace's owner ([`Route::Setup`](crate::Route::Setup)).
//!
//! Most of it runs here. The business, the receptionist, what callers should
//! know, how calls are answered, the test call and the forwarding check are
//! this client's. Buying a number, the plan, a calendar sign-in, the email
//! identity and the transfer number open the web's page in the browser
//! ([`SetupHandOff`]), and the wizard reads its state again when the app is
//! told the page was left ([`SetupEvent::HandOffReturned`]).
//!
//! The step states are the service's, and this reads them as the web does: the
//! wizard opens on the first step still `todo`, or on the first step when none
//! is. A state this build does not know is not `todo`, so a state added on the
//! service cannot send an owner back to a step they finished.
//!
//! Every step saves as it goes, so "Finish later" loses nothing a step saved. A
//! step writes its own data and then marks itself `done`; Skip marks it
//! `skipped`. The next open resumes from what the service holds, on any device.
//!
//! One step write at a time: Skip, a step's Continue and Finish all end in
//! [`SetupWizard::completing`], and while a step's own write is on its way
//! ([`SetupWizard::is_moving`]), Back, Skip and Finish later wait, so a late
//! completion cannot undo a move made in between.
//!
//! Owner only: every `setup/*` route answers anyone else with a 403, which is
//! the ordinary answer here and is explained ([`SetupRefusal::NotOwner`]), never
//! shown as a failure with a retry. A workspace from before the wizard answers a
//! 409 `setup_not_available`, and an answer with no progress means the same.

mod business;
mod callers;
mod calls;
pub mod facts;
mod golive;
pub mod places;
mod receptionist;
pub mod wire;

use std::collections::BTreeMap;

use district_api::ApiError;
use district_l10n::Locale;
use district_model::{
    ConnectionsResponse, ForwardingCheckResponse, SetupLookupResponse, SetupResponse, SetupStep,
    SetupStepState, SetupTestCallResponse,
};
use serde_json::{Map, Value};

pub use business::{BusinessEvent, BusinessNotice, BusinessStep};
pub use callers::{CallersEvent, CallersStep};
pub use calls::{SetupCallsEvent, SetupCallsStep};
pub use golive::{
    FORWARDING_POLL_INTERVAL, FORWARDING_POLL_LIMIT, GoLiveEvent, GoLiveStep, GoLiveSummary,
    TestCallStatus,
};
pub use receptionist::{ReceptionistEvent, ReceptionistStep};
pub use wire::SetupLanguage;

use crate::failure::FailureText;
use crate::model::{CoreConfig, Effect, Slot, Ticket, Tickets};
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::l;
use district_l10n::setup_strings as t;

/// The setup page on the web, below the service's origin, where a number is
/// bought.
pub const SETUP_PAGE_PATH: &str = "/dashboard/district";
/// The Skills page, for the email sender name and reply-to.
pub const SKILLS_PAGE_PATH: &str = "/dashboard/district/studio/skills";
/// The Integrations page, for a calendar and the transfer number.
pub const INTEGRATIONS_PAGE_PATH: &str = "/dashboard/district/studio/integrations";

/// A page of the web's that a step opens in the browser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetupHandOff {
    /// The setup page, where a number is bought.
    SetupPage,
    /// The Skills page: the email sender name and reply-to.
    Skills,
    /// The Integrations page: a calendar, and the transfer number.
    Integrations,
}

impl SetupHandOff {
    /// The page's path below the service's origin.
    pub fn path(self) -> &'static str {
        match self {
            Self::SetupPage => SETUP_PAGE_PATH,
            Self::Skills => SKILLS_PAGE_PATH,
            Self::Integrations => INTEGRATIONS_PAGE_PATH,
        }
    }
}

/// Why the wizard is not run for this person, as opposed to a failure to
/// reach it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetupRefusal {
    /// Only the workspace's owner sets it up (a 403).
    NotOwner,
    /// The workspace is from before the wizard (a 409 `setup_not_available`,
    /// or an answer with no progress).
    NotAvailable,
}

impl SetupRefusal {
    fn from_error(error: &ApiError) -> Option<Self> {
        match error {
            ApiError::Forbidden(_) => Some(Self::NotOwner),
            ApiError::Conflict(_) if error.code() == Some("setup_not_available") => {
                Some(Self::NotAvailable)
            }
            _ => None,
        }
    }

    /// The heading that names it.
    pub fn title(self) -> &'static str {
        match self {
            Self::NotOwner => t::setup_not_owner_title(l()),
            Self::NotAvailable => t::setup_not_available_title(l()),
        }
    }

    /// The sentence that explains it.
    pub fn message(self) -> &'static str {
        match self {
            Self::NotOwner => t::setup_not_owner_body(l()),
            Self::NotAvailable => t::setup_not_available_body(l()),
        }
    }
}

/// Where a step's own reads are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StepPhase {
    /// Being read.
    #[default]
    Loading,
    /// Read.
    Ready,
    /// A read failed: the failure, and the step's retry.
    Failed(FailureText),
}

/// Where the wizard is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SetupPhase {
    /// Being read.
    #[default]
    Loading,
    /// The read failed: the failure, and a retry ([`SetupEvent::Retry`]).
    Failed(FailureText),
    /// Not run for this person or workspace.
    Refused(SetupRefusal),
    /// A step is on screen ([`SetupWizard::active`]).
    Ready,
    /// Setup is finished.
    Finished,
}

/// What step 1 learned that steps 2 and 3 need before a read could give it
/// back: the name, the place, and the language, which only step 3 stores (in
/// the persona).
///
/// Kept in memory per workspace while the app runs, so the language chosen in
/// step 1 survives "Finish later" until step 3 writes it, as the web keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusinessDraft {
    /// The business's name ("" when the workspace still has its provisioned
    /// name).
    pub business_name: String,
    /// The two-letter country code, or "" when not chosen yet.
    pub country: String,
    /// The province or state code, or the region as typed.
    pub subdivision: String,
    /// The city.
    pub city: String,
    /// The receptionist's language.
    pub language: SetupLanguage,
}

impl BusinessDraft {
    /// The web's `initialBusinessDraft`.
    pub(crate) fn initial(
        data: Option<&SetupResponse>,
        persona_language: Option<&str>,
        workspace_name: &str,
        locale: Locale,
    ) -> Self {
        let address = data.and_then(|data| data.business_facts.as_ref());
        let stored = facts::address(address);
        let country = if stored.country.is_empty() {
            places::default_country(data.and_then(|data| data.region.as_deref())).to_owned()
        } else {
            stored.country
        };
        Self {
            business_name: if wire::is_default_workspace_name(workspace_name) {
                String::new()
            } else {
                workspace_name.to_owned()
            },
            subdivision: places::subdivision_code(&stored.region, &country),
            country,
            city: stored.city,
            language: SetupLanguage::from_persona(persona_language)
                .unwrap_or(SetupLanguage::app_default(locale)),
        }
    }
}

/// The setup wizard's screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupWizard {
    /// Where it is.
    pub phase: SetupPhase,
    /// The step on screen, while [`SetupPhase::Ready`].
    pub active: Option<SetupStep>,
    /// The wizard's state as the service last answered it.
    pub data: Option<SetupResponse>,
    /// What step 1 learned ([`BusinessDraft`]).
    pub draft: Option<BusinessDraft>,
    /// The workspace's name as the service holds it.
    pub workspace_name: String,
    /// A step's state is being written (Skip, Continue or Finish).
    pub completing: bool,
    /// Why the last step write failed, shown above the step.
    pub step_error: Option<FailureText>,
    /// Whether this app buys in the app ([`CoreConfig::in_app_purchases`]).
    /// Buying a number stays on the web in every build today, so step 2 opens
    /// the setup page either way; the flag is here for the step to tell when a
    /// destination to buy a number in the app exists.
    pub buys_in_app: bool,
    /// Step 1.
    pub business: BusinessStep,
    /// Step 3.
    pub receptionist: ReceptionistStep,
    /// Step 4.
    pub callers: CallersStep,
    /// Step 5.
    pub calls: SetupCallsStep,
    /// Step 6.
    pub golive: GoLiveStep,
    /// Step writes on their way, other than the shell's own.
    pub(crate) step_writes: u32,
    /// The step [`completing`](Self::completing) marks, which its answer
    /// moves on from.
    pub(crate) completing_step: Option<SetupStep>,
}

impl SetupWizard {
    /// A step's title.
    pub fn title(step: SetupStep) -> &'static str {
        let locale = l();
        match step {
            SetupStep::Business => t::setup_business_title(locale),
            SetupStep::Number => t::setup_number_title(locale),
            SetupStep::Receptionist => t::setup_receptionist_title(locale),
            SetupStep::Callers => t::setup_callers_title(locale),
            SetupStep::Calls => t::setup_calls_title(locale),
            SetupStep::Golive => t::setup_golive_title(locale),
        }
    }

    /// A step's one-line description.
    pub fn description(step: SetupStep) -> &'static str {
        let locale = l();
        match step {
            SetupStep::Business => t::setup_business_description(locale),
            SetupStep::Number => t::setup_number_description(locale),
            SetupStep::Receptionist => t::setup_receptionist_description(locale),
            SetupStep::Callers => t::setup_callers_description(locale),
            SetupStep::Calls => t::setup_calls_description(locale),
            SetupStep::Golive => t::setup_golive_description(locale),
        }
    }

    /// A step's place, zero-based.
    pub fn position(step: SetupStep) -> usize {
        SetupStep::ALL
            .iter()
            .position(|each| *each == step)
            .unwrap_or_default()
    }

    /// The state of `step` as the service answered it, or `None` before an
    /// answer or for a value this build does not know.
    pub fn state(&self, step: SetupStep) -> Option<SetupStepState> {
        self.data
            .as_ref()?
            .setup_progress
            .as_ref()?
            .steps
            .state(step)
    }

    /// A step write or a move on its way: Back, Skip and Finish later wait.
    pub fn is_moving(&self) -> bool {
        self.completing || self.step_writes > 0
    }

    /// Whether Back is offered.
    pub fn can_go_back(&self) -> bool {
        self.active.is_some_and(|step| Self::position(step) > 0)
    }

    /// The workspace's region, for example `ca`.
    pub fn region(&self) -> Option<&str> {
        self.data.as_ref()?.region.as_deref()
    }

    /// The stored business facts.
    pub fn facts(&self) -> Option<&Map<String, Value>> {
        self.data.as_ref()?.business_facts.as_ref()
    }

    /// How many numbers the workspace holds.
    pub fn numbers_held(&self) -> i64 {
        self.data.as_ref().map_or(0, |data| data.numbers_held)
    }

    /// Step 2's "where calls are answered", from the workspace's region.
    pub fn answered_in(&self) -> Option<&'static str> {
        let locale = l();
        match self.region()? {
            "us" => Some(t::setup_number_answered_us(locale)),
            "ca" => Some(t::setup_number_answered_ca(locale)),
            "eu" => Some(t::setup_number_answered_eu(locale)),
            "apac" => Some(t::setup_number_answered_apac(locale)),
            _ => None,
        }
    }

    /// Step 2's line on the plan's numbers, when the plan includes some.
    pub fn numbers_line(&self) -> Option<String> {
        let data = self.data.as_ref()?;
        Some(match data.included_numbers {
            Some(included) => t::setup_number_included(l(), data.numbers_held, included),
            None => t::setup_number_held(l(), data.numbers_held),
        })
    }

    /// Where step 2 sends an owner with no number. Buying a number stays on
    /// the web in every build, so this is the setup page whether or not the
    /// app buys in the app ([`buys_in_app`](Self::buys_in_app)); a destination
    /// that buys a number in the app would be chosen here.
    pub fn number_hand_off(&self) -> SetupHandOff {
        SetupHandOff::SetupPage
    }
}

/// What the person does on the wizard. Every write is single-flight: an event
/// that would start one while one is on its way is ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupEvent {
    /// Read the wizard again, after its read failed.
    Retry,
    /// Go to the previous step. Nothing is written.
    Back,
    /// Mark the step on screen skipped, and move on.
    Skip,
    /// Dismiss the step write's failure.
    DismissError,
    /// Open one of the web's pages in the browser.
    OpenHandOff(SetupHandOff),
    /// The person came back from a page [`OpenHandOff`](Self::OpenHandOff)
    /// opened (the app's window was activated again): read the wizard again,
    /// and move on if the page finished the step on screen.
    HandOffReturned,
    /// Step 2: keep the number held, and move on.
    KeepNumber,
    /// Step 1.
    Business(BusinessEvent),
    /// Step 3.
    Receptionist(ReceptionistEvent),
    /// Step 4.
    Callers(CallersEvent),
    /// Step 5.
    Calls(SetupCallsEvent),
    /// Step 6.
    GoLive(GoLiveEvent),
}

/// The first step still `todo`, or the first step when none is.
pub fn first_open(data: Option<&SetupResponse>) -> SetupStep {
    let progress = data.and_then(|data| data.setup_progress.as_ref());
    SetupStep::ALL
        .into_iter()
        .find(|step| {
            progress
                .is_some_and(|progress| progress.steps.state(*step) == Some(SetupStepState::Todo))
        })
        .unwrap_or(SetupStep::Business)
}

/// The step after `step`, or `None` after the last.
pub fn next_step(step: SetupStep) -> Option<SetupStep> {
    SetupStep::ALL.get(SetupWizard::position(step) + 1).copied()
}

/// The step before `step`, or `None` before the first.
pub fn previous_step(step: SetupStep) -> Option<SetupStep> {
    SetupWizard::position(step)
        .checked_sub(1)
        .map(|index| SetupStep::ALL[index])
}

impl SignedIn {
    /// Opens the wizard, read afresh. The step 1 draft kept for this workspace
    /// comes back with it.
    pub(crate) fn enter_setup(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let workspace_id = self.workspace_id();
        self.close_setup(tickets);
        self.setup_persona_language = None;
        self.setup = SetupWizard {
            buys_in_app: self.setup.buys_in_app,
            draft: self.setup_drafts.get(&workspace_id).cloned(),
            ..SetupWizard::default()
        };
        // The wizard opens only on an open workspace (`navigate` sees to it).
        self.setup.workspace_name = self
            .workspaces
            .active()
            .map(|entry| entry.name.clone())
            .unwrap_or_default();
        self.load_setup(tickets)
    }

    /// Leaves the wizard: every read and write it had on its way is dropped
    /// with it, and the forwarding check's poll stops.
    pub(crate) fn close_setup(&mut self, tickets: &mut Tickets) {
        tickets.cancel_each(&[
            Slot::SetupLoad,
            Slot::SetupComplete,
            Slot::SetupLookupSave,
            Slot::SetupBusinessSave,
            Slot::SetupCallersSave,
            Slot::SetupLookup,
            Slot::SetupHoursLookup,
            Slot::SetupRename,
            Slot::SetupConfig,
            Slot::SetupPersonaOptions,
            Slot::SetupPersonaSave,
            Slot::SetupCallsHandlingSave,
            Slot::SetupConnections,
            Slot::SetupTestCall,
            Slot::SetupForwarding,
            Slot::SetupForwardingStart,
            Slot::SetupForwardingPoll,
            Slot::SetupReceptionistConfig,
            Slot::SetupCallsConfig,
            Slot::SetupGoLiveConfig,
            Slot::SetupCallsHandling,
            Slot::SetupGoLiveHandling,
            Slot::SetupProfilesSave,
            Slot::SetupPoll,
            Slot::SetupHandOffRead,
        ]);
        self.setup.step_writes = 0;
    }

    fn load_setup(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.setup.phase = SetupPhase::Loading;
        vec![
            Effect::LoadSetupWizard {
                ticket: tickets.issue(Slot::SetupLoad),
                workspace_id: self.workspace_id(),
            },
            // The persona's stored language opens step 1's choice, as the
            // web's `initialBusinessDraft` does.
            Effect::LoadWorkspaceConfig {
                ticket: tickets.issue(Slot::SetupConfig),
                workspace_id: self.workspace_id(),
            },
        ]
    }

    pub(crate) fn setup_event(
        &mut self,
        event: SetupEvent,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Next {
        let effects = match event {
            SetupEvent::Retry => {
                if matches!(self.setup.phase, SetupPhase::Failed(_)) {
                    self.load_setup(tickets)
                } else {
                    Vec::new()
                }
            }
            SetupEvent::DismissError => {
                self.setup.step_error = None;
                Vec::new()
            }
            SetupEvent::Back => {
                if !self.setup.is_moving()
                    && let Some(previous) = self.setup.active.and_then(previous_step)
                {
                    self.setup.step_error = None;
                    self.show_step(previous, tickets)
                } else {
                    Vec::new()
                }
            }
            SetupEvent::Skip => match self.setup.active {
                Some(step) if !self.setup.is_moving() => {
                    self.complete_step(step, SetupStepState::Skipped, tickets)
                }
                _ => Vec::new(),
            },
            SetupEvent::OpenHandOff(page) => vec![Effect::OpenUrl {
                url: config.web_url(page.path()),
            }],
            SetupEvent::HandOffReturned => self.setup_hand_off_returned(tickets),
            SetupEvent::KeepNumber => {
                if self.setup.active == Some(SetupStep::Number)
                    && self.setup.numbers_held() > 0
                    && !self.setup.is_moving()
                {
                    self.complete_step(SetupStep::Number, SetupStepState::Done, tickets)
                } else {
                    Vec::new()
                }
            }
            SetupEvent::Business(event) => self.business_event(event, tickets),
            SetupEvent::Receptionist(event) => self.receptionist_event(event, tickets),
            SetupEvent::Callers(event) => self.callers_event(event, tickets),
            SetupEvent::Calls(event) => self.setup_calls_event(event, tickets),
            SetupEvent::GoLive(event) => self.golive_event(event, tickets),
        };
        Next::Stay(effects)
    }

    /// Marks `step` and moves to the next one. The only way a step's state
    /// changes: a step that saved its own data ends here with `done`, Skip with
    /// `skipped`.
    pub(crate) fn complete_step(
        &mut self,
        step: SetupStep,
        state: SetupStepState,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        if self.setup.completing {
            return Vec::new();
        }
        self.setup.completing = true;
        self.setup.completing_step = Some(step);
        self.setup.step_error = None;
        vec![Effect::SaveSetupWizard {
            ticket: tickets.issue(Slot::SetupComplete),
            workspace_id: self.workspace_id(),
            steps: vec![(step, state)],
            business_facts: None,
        }]
    }

    /// A step's own write starts; [`end_step_write`](Self::end_step_write)
    /// pairs it when its answer lands.
    pub(crate) fn begin_step_write(&mut self) {
        self.setup.step_writes += 1;
    }

    pub(crate) fn end_step_write(&mut self) {
        self.setup.step_writes = self.setup.step_writes.saturating_sub(1);
    }

    /// A step's facts save, through the setup route, which answers the whole
    /// state. `slot` is the step's, so its answer reaches it.
    pub(crate) fn save_setup_facts(
        &mut self,
        facts: Map<String, Value>,
        slot: Slot,
        tickets: &mut Tickets,
    ) -> Effect {
        Effect::SaveSetupWizard {
            ticket: tickets.issue(slot),
            workspace_id: self.workspace_id(),
            steps: Vec::new(),
            business_facts: Some(facts),
        }
    }

    /// Puts `step` on screen, and reads what it shows.
    fn show_step(&mut self, step: SetupStep, tickets: &mut Tickets) -> Vec<Effect> {
        self.setup.active = Some(step);
        // Step 4 opens afresh whenever it shows, so a website hours read it
        // asked for before is dropped with what it had.
        tickets.cancel(Slot::SetupHoursLookup);
        // Off step 6 (Back, say), its forwarding check stops being polled.
        if step != SetupStep::Golive {
            tickets.cancel_each(&[Slot::SetupPoll, Slot::SetupForwardingPoll]);
        }
        match step {
            SetupStep::Business => {
                self.open_business();
                Vec::new()
            }
            SetupStep::Number => Vec::new(),
            SetupStep::Receptionist => self.open_receptionist(tickets),
            SetupStep::Callers => {
                self.open_callers();
                Vec::new()
            }
            SetupStep::Calls => self.open_calls(tickets),
            SetupStep::Golive => self.open_golive(tickets),
        }
    }

    pub(crate) fn update_setup_draft(&mut self, draft: BusinessDraft) {
        self.setup_drafts.insert(self.workspace_id(), draft.clone());
        self.setup.draft = Some(draft);
    }

    /// The draft, or the one a first open makes from what the service holds.
    pub(crate) fn setup_draft(&self) -> BusinessDraft {
        self.setup.draft.clone().unwrap_or_else(|| {
            BusinessDraft::initial(
                self.setup.data.as_ref(),
                None,
                &self.setup.workspace_name,
                l(),
            )
        })
    }

    pub(crate) fn setup_wizard_loaded(
        &mut self,
        ticket: Ticket,
        result: Result<SetupResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::SetupLoad, ticket) {
            return Next::Stay(self.setup_first_read(result, tickets));
        }
        if tickets.accept(Slot::SetupHandOffRead, ticket) {
            return Next::Stay(self.setup_read_after_hand_off(result, tickets));
        }
        stay()
    }

    fn setup_first_read(
        &mut self,
        result: Result<SetupResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.setup.phase = match SetupRefusal::from_error(&error) {
                    Some(refusal) => SetupPhase::Refused(refusal),
                    None => SetupPhase::Failed(FailureText::from_api_error(&error)),
                };
                return Vec::new();
            }
        };
        if response.setup_progress.is_none() {
            self.setup.phase = SetupPhase::Refused(SetupRefusal::NotAvailable);
            return Vec::new();
        }
        let finished = response
            .setup_progress
            .as_ref()
            .is_some_and(|progress| progress.completed_at.is_some());
        let first = first_open(Some(&response));
        self.setup.data = Some(response);
        if finished {
            self.setup.phase = SetupPhase::Finished;
            return Vec::new();
        }
        self.settle_setup_draft();
        self.setup.phase = SetupPhase::Ready;
        self.show_step(first, tickets)
    }

    /// The draft a first open makes, once both the wizard and the persona's
    /// language were read (or the persona's read failed). A draft kept from an
    /// earlier visit wins.
    fn settle_setup_draft(&mut self) {
        if self.setup.draft.is_some() || self.setup.data.is_none() {
            return;
        }
        let Some(language) = self.setup_persona_language.take() else {
            return;
        };
        let draft = BusinessDraft::initial(
            self.setup.data.as_ref(),
            language.as_deref(),
            &self.setup.workspace_name,
            l(),
        );
        // Step 1 may already be on screen with what the owner typed: only the
        // language the persona names comes in, and only over the app's own
        // default (one the owner chose stays).
        let shown = &mut self.setup.business;
        if self.setup.active == Some(SetupStep::Business)
            && shown.language == SetupLanguage::app_default(l())
        {
            shown.language = draft.language;
        }
        self.update_setup_draft(draft);
    }

    /// After a page opened in the browser was left: read again, and move on if
    /// the web finished the step on screen (a number bought there marks
    /// `number` done). A failed read keeps the step on screen.
    fn setup_hand_off_returned(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if self.setup.phase != SetupPhase::Ready {
            return Vec::new();
        }
        let mut effects = vec![Effect::LoadSetupWizard {
            ticket: tickets.issue(Slot::SetupHandOffRead),
            workspace_id: self.workspace_id(),
        }];
        if self.setup.active == Some(SetupStep::Calls) {
            effects.extend(self.refresh_calls_after_hand_off(tickets));
        }
        effects
    }

    fn setup_read_after_hand_off(
        &mut self,
        result: Result<SetupResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        // A failed read, or one with no progress (which says nothing about the
        // steps), keeps the step on screen.
        let Ok(response) = result else {
            return Vec::new();
        };
        if response.setup_progress.is_none() {
            return Vec::new();
        }
        // Asked for only while Ready, which always has a step on screen, and a
        // step leaves the screen only with the whole wizard, which cancels this
        // read: no `None` arm.
        let step = self
            .setup
            .active
            .expect("a ready wizard has a step on screen");
        let before = self.setup.state(step);
        let finished = response
            .setup_progress
            .as_ref()
            .is_some_and(|progress| progress.completed_at.is_some());
        let first = first_open(Some(&response));
        self.setup.data = Some(response);
        if finished {
            self.setup.phase = SetupPhase::Finished;
            return Vec::new();
        }
        // A step write on its way moves on itself when it lands.
        if self.setup.completing {
            return Vec::new();
        }
        // Only a step the page finished moves on: one that was already done or
        // skipped (reached by Back) stays with whatever was typed into it.
        if before == Some(SetupStepState::Todo) && self.setup.state(step) != before {
            return self.show_step(first, tickets);
        }
        Vec::new()
    }

    pub(crate) fn setup_saved(
        &mut self,
        ticket: Ticket,
        result: Result<SetupResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::SetupComplete, ticket) {
            drop_older_read(&result, tickets);
            return Next::Stay(self.setup_completed(result, tickets));
        }
        if tickets.accept(Slot::SetupLookupSave, ticket) {
            drop_older_read(&result, tickets);
            self.business_drafts_saved(result);
            return stay();
        }
        if tickets.accept(Slot::SetupBusinessSave, ticket) {
            drop_older_read(&result, tickets);
            return Next::Stay(self.business_saved(result, tickets));
        }
        if tickets.accept(Slot::SetupCallersSave, ticket) {
            drop_older_read(&result, tickets);
            return Next::Stay(self.callers_saved(result, tickets));
        }
        stay()
    }

    fn setup_completed(
        &mut self,
        result: Result<SetupResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        self.setup.completing = false;
        let completed = self.setup.completing_step.take();
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.setup.step_error = Some(FailureText::from_api_error(&error));
                return Vec::new();
            }
        };
        let finished = response
            .setup_progress
            .as_ref()
            .is_some_and(|progress| progress.completed_at.is_some());
        let first = first_open(Some(&response));
        self.setup.data = Some(response);
        if finished {
            self.setup_drafts.remove(&self.workspace_id());
            self.setup.phase = SetupPhase::Finished;
            return Vec::new();
        }
        // From the step this marked, not the one on screen: a re-read that
        // landed first cannot make it skip one. Past the last step with one
        // still open (reached by Back), the open one.
        let next = completed.and_then(next_step).unwrap_or(first);
        self.show_step(next, tickets)
    }

    /// The window's refresh: a failed read is tried again, and a wizard on
    /// screen reads its state again as after a hand-off.
    pub(crate) fn refresh_setup(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if matches!(self.setup.phase, SetupPhase::Failed(_)) {
            return self.load_setup(tickets);
        }
        self.setup_hand_off_returned(tickets)
    }

    pub(crate) fn setup_website_read(
        &mut self,
        ticket: Ticket,
        result: Result<SetupLookupResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::SetupLookup, ticket) {
            return Next::Stay(self.business_website_read(result, tickets));
        }
        if tickets.accept(Slot::SetupHoursLookup, ticket) {
            self.callers_hours_read(result);
        }
        stay()
    }

    pub(crate) fn setup_test_call_answered(
        &mut self,
        ticket: Ticket,
        result: Result<SetupTestCallResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::SetupTestCall, ticket) {
            self.setup_test_call_placed(result);
        }
        stay()
    }

    pub(crate) fn setup_forwarding_answered(
        &mut self,
        ticket: Ticket,
        result: Result<ForwardingCheckResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        for slot in [
            Slot::SetupForwarding,
            Slot::SetupForwardingStart,
            Slot::SetupForwardingPoll,
        ] {
            if tickets.accept(slot, ticket) {
                return Next::Stay(self.setup_forwarding_read(slot, result, tickets));
            }
        }
        stay()
    }

    pub(crate) fn setup_connections_answered(
        &mut self,
        ticket: Ticket,
        result: Result<ConnectionsResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::SetupConnections, ticket) {
            self.calls_connections_loaded(result);
        }
        stay()
    }

    pub(crate) fn setup_persona_language_read(&mut self, language: Option<String>) {
        self.setup_persona_language = Some(language);
        self.settle_setup_draft();
    }
}

/// A write answers with the whole state as it left it, so a re-read asked
/// before that answer may hold older state: it is dropped, and a later save
/// cannot write sections from it.
fn drop_older_read(result: &Result<SetupResponse, ApiError>, tickets: &mut Tickets) {
    if result.is_ok() {
        tickets.cancel(Slot::SetupHandOffRead);
    }
}

/// Every kept step 1 draft, by workspace.
pub(crate) type SetupDrafts = BTreeMap<String, BusinessDraft>;

#[cfg(test)]
mod tests {
    use super::*;

    fn with_progress(progress: Value) -> SetupWizard {
        SetupWizard {
            data: Some(
                serde_json::from_value(serde_json::json!({
                    "setupProgress": progress, "businessFacts": null, "region": "ca",
                    "tier": "VoicePro", "includedNumbers": 3, "numbersHeld": 1,
                }))
                .unwrap(),
            ),
            ..SetupWizard::default()
        }
    }

    /// Every path runs here as well as from the integration tests: a unit
    /// test binary holds its own copy of each function, measured on its own.
    #[test]
    fn a_step_has_its_state_and_none_before_an_answer_or_in_one_with_no_progress() {
        assert_eq!(SetupWizard::default().state(SetupStep::Business), None);
        assert_eq!(with_progress(Value::Null).state(SetupStep::Business), None);
        let steps = serde_json::json!({"steps": {"business": "done"}});
        assert_eq!(
            with_progress(steps).state(SetupStep::Business),
            Some(SetupStepState::Done)
        );
    }
}
