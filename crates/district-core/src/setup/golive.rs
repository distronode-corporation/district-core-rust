//! Step 6, "Test and go live": the test call, the forwarding check, the
//! summary, and Finish.
//!
//! The test call and the forwarding check each place a real call on the
//! platform's carrier account, so each is its own single flight and neither is
//! ever repeated by this client. The service limits both per day and says so
//! in its refusal.
//!
//! The consent is recorded by the service, in its own words for the owner's
//! language, when the test call is placed; this client sends only the phone.
//! The step shows the same words beside the button.
//!
//! The forwarding check is hidden while the service says `available: false`
//! (no verifier in the region), and read again every two seconds while it is
//! pending, for at most two minutes.
//!
//! The summary is not a summary of defaults: when the reads behind it fail it
//! is left out, rather than saying "no greeting" and "every call" for a
//! workspace that has both.

use std::time::Duration;

use district_api::ApiError;
use district_l10n::setup_strings as t;
use district_model::{
    CallHandlingResponse, ForwardingCheckMode, ForwardingCheckResponse, SetupStep, SetupStepState,
    SetupTestCallResponse, WorkspaceConfigResponse,
};

use super::wire;
use crate::failure::FailureText;
use crate::model::{Effect, Slot, Tickets};
use crate::signed_in::SignedIn;
use crate::text::l;

/// How long between two reads of a pending forwarding check.
pub const FORWARDING_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// How many reads of a pending check before the step stops reading.
pub const FORWARDING_POLL_LIMIT: u32 = 60;

/// Where the test call is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestCallStatus {
    /// The receptionist is calling.
    Calling,
    /// The call could not be placed, in a sentence.
    Failed(String),
}

/// What the summary says, from the persona and call handling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoLiveSummary {
    /// The greeting, or "".
    pub greeting: String,
    /// Who answers.
    pub mode: String,
    /// The ring, in seconds.
    pub ring_seconds: i64,
}

impl GoLiveSummary {
    /// The greeting line: the greeting, or that the standard one is used.
    pub fn greeting_line(&self) -> String {
        if self.greeting.is_empty() {
            t::setup_golive_no_greeting(l()).to_owned()
        } else {
            self.greeting.clone()
        }
    }

    /// When the receptionist answers.
    pub fn answers_line(&self) -> String {
        match self.mode.as_str() {
            "app_first" => t::setup_golive_answers_app_first(l(), self.ring_seconds),
            "ai_then_app" => t::setup_golive_answers_ai_then_app(l()).to_owned(),
            _ => t::setup_calls_ai_first(l()).to_owned(),
        }
    }
}

/// Step 6.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GoLiveStep {
    /// The owner's mobile, for the test call.
    pub mobile: String,
    /// The business line the check calls.
    pub business_line: String,
    /// Which calls the line forwards.
    pub mode: Option<ForwardingCheckMode>,
    /// Where the test call is.
    pub call_status: Option<TestCallStatus>,
    /// The mobile is not a number the test call can dial.
    pub mobile_error: bool,
    /// The test call is being placed.
    pub calling: bool,
    /// The forwarding check, as last read.
    pub forwarding: Option<ForwardingCheckResponse>,
    /// The business line is not a number the check can dial.
    pub line_error: bool,
    /// The check is being started.
    pub starting_check: bool,
    /// The pending check is being read again.
    pub polling: bool,
    /// The check is still pending after the last read.
    pub poll_gave_up: bool,
    /// Why the check could not be started, in a sentence.
    pub check_error: Option<String>,
    /// The summary, once its reads landed.
    pub summary: Option<GoLiveSummary>,
    pub(crate) poll_ticks: u32,
    pub(crate) config: Option<Option<WorkspaceConfigResponse>>,
    pub(crate) handling: Option<Option<CallHandlingResponse>>,
}

/// What the person does on step 6.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GoLiveEvent {
    /// The mobile field changed.
    SetMobile(String),
    /// Place the test call.
    CallMe,
    /// The business line field changed.
    SetBusinessLine(String),
    /// Which calls the line forwards was chosen.
    SetMode(ForwardingCheckMode),
    /// Start the forwarding check, or read a pending one again.
    StartCheck,
    /// Finish setup.
    Finish,
}

impl GoLiveStep {
    /// Which calls the line forwards: all of them until chosen.
    pub fn forwarding_mode(&self) -> ForwardingCheckMode {
        self.mode.unwrap_or(ForwardingCheckMode::All)
    }

    /// Whether the check is offered (the region has a verifier).
    pub fn check_offered(&self) -> bool {
        self.forwarding
            .as_ref()
            .is_some_and(|forwarding| forwarding.available)
    }

    /// Whether the latest check is pending.
    pub fn check_is_pending(&self) -> bool {
        self.forwarding
            .as_ref()
            .and_then(|forwarding| forwarding.forwarding_check.as_ref())
            .is_some_and(|check| check.is_pending())
    }

    /// What the latest check says.
    pub fn check_sentence(&self) -> Option<&'static str> {
        let check = self.forwarding.as_ref()?.forwarding_check.as_ref()?;
        let locale = l();
        Some(if check.is_pending() {
            if self.poll_gave_up {
                t::setup_golive_check_still_pending(locale)
            } else {
                t::setup_golive_check_pending(locale)
            }
        } else if check.has_passed() {
            t::setup_golive_check_passed(locale)
        } else if check.failure.as_deref() == Some("dial_failed") {
            t::setup_golive_check_dial_failed(locale)
        } else {
            t::setup_golive_check_failed(locale)
        })
    }

    /// How many checks are left today.
    pub fn attempts_sentence(&self) -> Option<String> {
        let left = self.forwarding.as_ref()?.attempts_left;
        Some(if left == 1 {
            t::setup_golive_attempts_left_one(l()).to_owned()
        } else {
            t::setup_golive_attempts_left(l(), left)
        })
    }

    /// The number the check calls from, once a check named one.
    pub fn expect_call_line(&self) -> Option<String> {
        let check = self.forwarding.as_ref()?.forwarding_check.as_ref()?;
        Some(t::setup_golive_expect_call(l(), &check.verifier_number))
    }
}

impl SignedIn {
    /// Opens step 6: reads the forwarding check, the persona and call
    /// handling.
    pub(crate) fn open_golive(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.setup.golive = GoLiveStep::default();
        let workspace_id = self.workspace_id();
        vec![
            Effect::LoadSetupForwardingCheck {
                ticket: tickets.issue(Slot::SetupForwarding),
                workspace_id: workspace_id.clone(),
            },
            Effect::LoadWorkspaceConfig {
                ticket: tickets.issue(Slot::SetupGoLiveConfig),
                workspace_id: workspace_id.clone(),
            },
            Effect::LoadCallHandling {
                ticket: tickets.issue(Slot::SetupGoLiveHandling),
                workspace_id,
            },
        ]
    }

    pub(crate) fn golive_event(
        &mut self,
        event: GoLiveEvent,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let step = &mut self.setup.golive;
        match event {
            GoLiveEvent::SetMobile(mobile) => step.mobile = mobile,
            GoLiveEvent::SetBusinessLine(line) => step.business_line = line,
            GoLiveEvent::SetMode(mode) => step.mode = Some(mode),
            GoLiveEvent::CallMe => return self.place_test_call(tickets),
            GoLiveEvent::StartCheck => return self.start_forwarding_check(tickets),
            GoLiveEvent::Finish => {
                return self.complete_step(SetupStep::Golive, SetupStepState::Done, tickets);
            }
        }
        Vec::new()
    }

    fn place_test_call(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if self.setup.golive.calling || self.setup.numbers_held() == 0 {
            return Vec::new();
        }
        let region = self.setup.region().map(str::to_owned);
        let workspace_id = self.workspace_id();
        let step = &mut self.setup.golive;
        let Some(phone) = wire::e164(&step.mobile, region.as_deref()) else {
            step.mobile_error = true;
            return Vec::new();
        };
        step.calling = true;
        step.mobile_error = false;
        step.call_status = None;
        vec![Effect::PlaceSetupTestCall {
            ticket: tickets.issue(Slot::SetupTestCall),
            workspace_id,
            phone,
        }]
    }

    /// The test call was placed, or not.
    pub(crate) fn setup_test_call_placed(
        &mut self,
        result: Result<SetupTestCallResponse, ApiError>,
    ) {
        let step = &mut self.setup.golive;
        step.calling = false;
        step.call_status = Some(match result {
            Ok(_) => TestCallStatus::Calling,
            Err(error) => TestCallStatus::Failed(FailureText::from_api_error(&error).message),
        });
    }

    fn start_forwarding_check(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let region = self.setup.region().map(str::to_owned);
        let workspace_id = self.workspace_id();
        let step = &mut self.setup.golive;
        if step.starting_check || step.polling {
            return Vec::new();
        }
        if step.check_is_pending() {
            return self.poll_forwarding(tickets);
        }
        let Some(line) = wire::e164(&step.business_line, region.as_deref()) else {
            step.line_error = true;
            return Vec::new();
        };
        step.starting_check = true;
        step.line_error = false;
        step.check_error = None;
        vec![Effect::StartSetupForwardingCheck {
            ticket: tickets.issue(Slot::SetupForwardingStart),
            workspace_id,
            business_line: line,
            mode: step.forwarding_mode(),
        }]
    }

    /// Starts reading a pending check again, every two seconds.
    fn poll_forwarding(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let step = &mut self.setup.golive;
        if step.polling || !step.check_is_pending() {
            return Vec::new();
        }
        step.polling = true;
        step.poll_gave_up = false;
        step.poll_ticks = 0;
        vec![Effect::Wait {
            ticket: tickets.issue(Slot::SetupPoll),
            delay: FORWARDING_POLL_INTERVAL,
        }]
    }

    /// The wait between two reads is over.
    pub(crate) fn setup_poll_due(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.setup.golive.poll_ticks += 1;
        vec![Effect::LoadSetupForwardingCheck {
            ticket: tickets.issue(Slot::SetupForwardingPoll),
            workspace_id: self.workspace_id(),
        }]
    }

    /// A forwarding check arrived: the step's first read, a start's answer, or
    /// a poll's read.
    pub(crate) fn setup_forwarding_read(
        &mut self,
        slot: Slot,
        result: Result<ForwardingCheckResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let step = &mut self.setup.golive;
        match slot {
            Slot::SetupForwardingStart => {
                step.starting_check = false;
                match result {
                    Ok(response) => step.forwarding = Some(response),
                    Err(error) => {
                        step.check_error = Some(FailureText::from_api_error(&error).message);
                        return Vec::new();
                    }
                }
                self.poll_forwarding(tickets)
            }
            Slot::SetupForwardingPoll => {
                // A failed read keeps the last answer; the next read may land.
                if let Ok(response) = result {
                    step.forwarding = Some(response);
                }
                if step.check_is_pending() && step.poll_ticks < FORWARDING_POLL_LIMIT {
                    return vec![Effect::Wait {
                        ticket: tickets.issue(Slot::SetupPoll),
                        delay: FORWARDING_POLL_INTERVAL,
                    }];
                }
                step.polling = false;
                step.poll_gave_up = step.check_is_pending();
                Vec::new()
            }
            _ => {
                step.forwarding = result.ok();
                self.poll_forwarding(tickets)
            }
        }
    }

    /// The persona read for the summary landed.
    pub(crate) fn golive_config_loaded(
        &mut self,
        result: Result<WorkspaceConfigResponse, ApiError>,
    ) {
        self.setup.golive.config = Some(result.ok());
        self.settle_golive_summary();
    }

    /// Call handling for the summary landed.
    pub(crate) fn golive_handling_loaded(
        &mut self,
        result: Result<CallHandlingResponse, ApiError>,
    ) {
        self.setup.golive.handling = Some(result.ok());
        self.settle_golive_summary();
    }

    fn settle_golive_summary(&mut self) {
        let step = &mut self.setup.golive;
        if let (Some(Some(config)), Some(Some(handling))) = (&step.config, &step.handling) {
            step.summary = Some(GoLiveSummary {
                greeting: config
                    .config
                    .ai_persona
                    .as_ref()
                    .and_then(|persona| persona.greeting.as_deref())
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                mode: handling.call_handling.clone(),
                ring_seconds: handling.app_ring_seconds,
            });
        }
    }
}
