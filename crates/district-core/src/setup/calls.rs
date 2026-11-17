//! Step 5, "Calls, people and bookings".
//!
//! Who answers is saved here, through the call handling route the settings
//! screen uses: `ai_first`, or `app_first` with a 5 to 30 second ring. An
//! `ai_then_app` chosen earlier in settings is shown and kept, never offered
//! (the web's step 5 offers only the two). "Only when I don't pick up" is the
//! carrier's no-answer forward, set up in step 6, and is not a mode.
//!
//! The transfer number and the calendar are shown here and changed on the
//! web's Integrations page, in the browser; the step reads both again when the
//! person comes back.
//!
//! Caller profiles are an opt-in that starts off: read strictly (only `true` is
//! on), offered only on the plans that include them (Voice Studio and DGI), and
//! written only when the owner flips the switch.

use district_api::ApiError;
use district_model::{
    CallHandlingMode, CallHandlingPatch, CallHandlingResponse, ConnectionsResponse, PersonaPatch,
    SetupStep, SetupStepState, WorkspaceConfig, WorkspaceConfigResponse, clamp_app_ring_seconds,
};

use super::StepPhase;
use crate::failure::FailureText;
use crate::model::{Effect, Slot, Tickets};
use crate::signed_in::SignedIn;

/// `ai_first`.
const AI_FIRST: &str = "ai_first";
/// `app_first`.
const APP_FIRST: &str = "app_first";
/// `ai_then_app`.
const AI_THEN_APP: &str = "ai_then_app";

/// The calendars the receptionist books into, by the provider the
/// connections read names, with the connection row that holds its status.
/// Product names, the same in every language.
const CALENDARS: [(&str, &str, &str); 2] = [
    ("google", "google-calendar", "Google Calendar"),
    (
        "microsoft",
        "microsoft-calendar",
        "Microsoft Outlook Calendar",
    ),
];

/// Step 5's form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupCallsStep {
    /// The reads behind it.
    pub phase: StepPhase,
    /// Who answers, as stored.
    pub stored_mode: String,
    /// The ring, as stored, in seconds.
    pub stored_ring: i64,
    /// Who answers, as chosen.
    pub mode: String,
    /// The ring, as chosen, in seconds.
    pub ring_seconds: i64,
    /// The number a caller asking for a person is transferred to.
    pub transfer_number: Option<String>,
    /// The calendar the receptionist books into.
    pub calendar_name: Option<&'static str>,
    /// Whether caller profiles are offered on this plan.
    pub profiles_offered: bool,
    /// Whether caller profiles are on.
    pub profiles_on: bool,
    /// Who answers is being saved.
    pub saving_answering: bool,
    /// Caller profiles are being saved.
    pub saving_profiles: bool,
    /// The step is moving on.
    pub continuing: bool,
    /// The last answering save landed.
    pub answering_saved: bool,
    /// Why the last write failed.
    pub error: Option<FailureText>,
    pub(crate) handling: Option<Result<CallHandlingResponse, FailureText>>,
    pub(crate) config: Option<Result<WorkspaceConfig, FailureText>>,
    pub(crate) profiles_wanted: bool,
}

/// What the person does on step 5.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupCallsEvent {
    /// Read the step again, after a read failed.
    Retry,
    /// Who answers was chosen: `ai_first`, `app_first`, or a stored
    /// `ai_then_app` kept.
    SetMode(String),
    /// The ring changed, in seconds.
    SetRing(i64),
    /// Save who answers.
    SaveAnswering,
    /// Caller profiles were switched.
    SetProfiles(bool),
    /// Save what changed and move on.
    Continue,
}

impl SetupCallsStep {
    /// The modes offered: the two the web offers, and a stored `ai_then_app`.
    pub fn modes(&self) -> Vec<&'static str> {
        if self.stored_mode == AI_THEN_APP {
            vec![AI_FIRST, APP_FIRST, AI_THEN_APP]
        } else {
            vec![AI_FIRST, APP_FIRST]
        }
    }

    /// Whether who answers differs from what is stored.
    pub fn answering_changed(&self) -> bool {
        self.mode != self.stored_mode
            || (self.mode == APP_FIRST && self.ring_seconds != self.stored_ring)
    }

    fn patch(&self) -> CallHandlingPatch {
        let mode = match self.mode.as_str() {
            AI_FIRST => Some(CallHandlingMode::AiFirst),
            APP_FIRST => Some(CallHandlingMode::AppFirst),
            _ => Some(CallHandlingMode::AiThenApp),
        };
        CallHandlingPatch {
            call_handling: mode.filter(|_| self.mode != self.stored_mode),
            app_ring_seconds: (self.mode == APP_FIRST && self.ring_seconds != self.stored_ring)
                .then(|| clamp_app_ring_seconds(self.ring_seconds)),
        }
    }

    fn adopt(&mut self, handling: &CallHandlingResponse) {
        self.stored_mode = handling.call_handling.clone();
        self.stored_ring = handling.app_ring_seconds;
        self.mode = handling.call_handling.clone();
        self.ring_seconds = handling.app_ring_seconds;
    }
}

/// The transfer number a configuration holds, if any.
pub fn transfer_number(config: &WorkspaceConfig) -> Option<String> {
    config
        .tool_config
        .as_ref()
        .and_then(|tools| tools.support_phone_number.clone())
        .filter(|number| !number.trim().is_empty())
}

/// Whether caller profiles come with `tier`.
pub fn profiles_offered(tier: Option<&str>) -> bool {
    matches!(
        tier.unwrap_or_default().to_lowercase().as_str(),
        "voicestudio" | "dgi"
    )
}

/// The calendar the receptionist books into, when one is connected and in use.
pub fn calendar_name(connections: &ConnectionsResponse) -> Option<&'static str> {
    let active = connections.calendar.as_ref()?.active_provider.as_deref()?;
    CALENDARS
        .iter()
        .find(|(provider, row, _)| {
            *provider == active
                && connections
                    .connections
                    .iter()
                    .any(|connection| connection.provider == *row && connection.status == "active")
        })
        .map(|(_, _, name)| *name)
}

impl SignedIn {
    /// Opens step 5: reads call handling, the configuration and the
    /// connections.
    pub(crate) fn open_calls(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.setup.calls = SetupCallsStep::default();
        let workspace_id = self.workspace_id();
        vec![
            Effect::LoadCallHandling {
                ticket: tickets.issue(Slot::SetupCallsHandling),
                workspace_id: workspace_id.clone(),
            },
            Effect::LoadWorkspaceConfig {
                ticket: tickets.issue(Slot::SetupCallsConfig),
                workspace_id: workspace_id.clone(),
            },
            Effect::LoadSetupConnections {
                ticket: tickets.issue(Slot::SetupConnections),
                workspace_id,
            },
        ]
    }

    /// After the Integrations page was left: the transfer number and the
    /// calendar are read again.
    pub(crate) fn refresh_calls_after_hand_off(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if self.setup.calls.phase != StepPhase::Ready {
            return Vec::new();
        }
        let workspace_id = self.workspace_id();
        vec![
            Effect::LoadWorkspaceConfig {
                ticket: tickets.issue(Slot::SetupCallsConfig),
                workspace_id: workspace_id.clone(),
            },
            Effect::LoadSetupConnections {
                ticket: tickets.issue(Slot::SetupConnections),
                workspace_id,
            },
        ]
    }

    pub(crate) fn setup_calls_event(
        &mut self,
        event: SetupCallsEvent,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let step = &mut self.setup.calls;
        match event {
            SetupCallsEvent::Retry => {
                if matches!(step.phase, StepPhase::Failed(_)) {
                    return self.open_calls(tickets);
                }
            }
            SetupCallsEvent::SetMode(mode) => step.mode = mode,
            SetupCallsEvent::SetRing(seconds) => step.ring_seconds = seconds,
            SetupCallsEvent::SaveAnswering => return self.save_answering(tickets),
            SetupCallsEvent::SetProfiles(on) => {
                if !step.profiles_offered || on == step.profiles_on || step.saving_profiles {
                    return Vec::new();
                }
                step.saving_profiles = true;
                step.profiles_wanted = on;
                step.error = None;
                self.begin_step_write();
                return vec![Effect::SavePersona {
                    ticket: tickets.issue(Slot::SetupProfilesSave),
                    workspace_id: self.workspace_id(),
                    patch: Box::new(PersonaPatch {
                        dgi_enabled: Some(on),
                        ..PersonaPatch::default()
                    }),
                }];
            }
            SetupCallsEvent::Continue => {
                if step.continuing || step.phase != StepPhase::Ready {
                    return Vec::new();
                }
                if step.answering_changed() {
                    step.continuing = true;
                    return self.save_answering(tickets);
                }
                return self.complete_step(SetupStep::Calls, SetupStepState::Done, tickets);
            }
        }
        Vec::new()
    }

    fn save_answering(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let step = &mut self.setup.calls;
        if !step.answering_changed() || step.saving_answering {
            step.continuing = false;
            return Vec::new();
        }
        step.saving_answering = true;
        step.error = None;
        step.answering_saved = false;
        let patch = step.patch();
        self.begin_step_write();
        vec![Effect::SaveCallHandling {
            ticket: tickets.issue(Slot::SetupCallsHandlingSave),
            workspace_id: self.workspace_id(),
            patch,
        }]
    }

    /// Call handling for step 5 arrived: from its read, or as the save stored
    /// it (the echo is the new baseline). A Continue waiting on the save moves
    /// on when it lands.
    pub(crate) fn calls_handling_loaded(
        &mut self,
        saved: bool,
        result: Result<CallHandlingResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        if !saved {
            self.setup.calls.handling =
                Some(result.map_err(|error| FailureText::from_api_error(&error)));
            self.settle_calls();
            return Vec::new();
        }
        self.end_step_write();
        let step = &mut self.setup.calls;
        step.saving_answering = false;
        let continuing = std::mem::take(&mut step.continuing);
        match result {
            Ok(stored) => {
                step.adopt(&stored);
                step.answering_saved = true;
            }
            Err(error) => {
                step.error = Some(FailureText::from_api_error(&error));
                return Vec::new();
            }
        }
        if continuing {
            return self.complete_step(SetupStep::Calls, SetupStepState::Done, tickets);
        }
        Vec::new()
    }

    /// The configuration read for step 5 arrived, first or after a hand-off.
    pub(crate) fn calls_config_loaded(
        &mut self,
        result: Result<WorkspaceConfigResponse, ApiError>,
    ) {
        let step = &mut self.setup.calls;
        if step.phase == StepPhase::Ready {
            if let Ok(response) = result {
                step.transfer_number = transfer_number(&response.config);
            }
            return;
        }
        step.config = Some(
            result
                .map(|response| response.config)
                .map_err(|error| FailureText::from_api_error(&error)),
        );
        self.settle_calls();
    }

    /// The connections read for step 5 arrived. A failed read shows no
    /// calendar.
    pub(crate) fn calls_connections_loaded(
        &mut self,
        result: Result<ConnectionsResponse, ApiError>,
    ) {
        if let Ok(connections) = result {
            self.setup.calls.calendar_name = calendar_name(&connections);
        }
    }

    /// Both reads are in: the step is ready, or failed with the first failure.
    fn settle_calls(&mut self) {
        let step = &mut self.setup.calls;
        match (step.handling.take(), step.config.take()) {
            (Some(Ok(handling)), Some(Ok(config))) => {
                step.adopt(&handling);
                step.transfer_number = transfer_number(&config);
                step.profiles_offered = profiles_offered(config.subscription_tier.as_deref());
                step.profiles_on = config
                    .ai_persona
                    .as_ref()
                    .and_then(|persona| persona.dgi_enabled)
                    == Some(true);
                step.phase = StepPhase::Ready;
            }
            (Some(Err(failure)), Some(_)) | (Some(_), Some(Err(failure))) => {
                step.phase = StepPhase::Failed(failure);
            }
            // One read is still on its way: the one that landed waits for it.
            (handling, config) => {
                step.handling = handling;
                step.config = config;
            }
        }
    }

    /// Caller profiles were saved, or not.
    pub(crate) fn calls_profiles_saved(&mut self, result: Result<(), ApiError>) {
        self.end_step_write();
        let step = &mut self.setup.calls;
        step.saving_profiles = false;
        match result {
            Ok(()) => step.profiles_on = step.profiles_wanted,
            Err(error) => step.error = Some(FailureText::from_api_error(&error)),
        }
    }
}
