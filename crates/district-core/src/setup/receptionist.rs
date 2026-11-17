//! Step 3, "Your receptionist": a name, a greeting and a voice, saved into the
//! persona.
//!
//! Only what this step owns is sent: the name, the greeting, the language and
//! (when offered) the voice. The persona route merges per field, so every other
//! key is left as it is.
//!
//! The voices come from the service's options for the persona's own engine and
//! language, never from a list kept here: the persona route coerces rather
//! than refuses, so a voice typed by hand would be stored and then silently
//! replaced. With no engine recorded, or no voices for it, the step offers none
//! and leaves the voice alone.
//!
//! The starter greeting is replaced by the business's own; a greeting the owner
//! wrote is kept. A greeting is the starter one when it is empty, one of the
//! two provisioned starter sentences, or this step's own template for the
//! stored name.

use district_api::ApiError;
use district_l10n::setup_strings as t;
use district_model::{
    AiPersona, PersonaOptionsResponse, PersonaPatch, PersonaVoiceGroup, SetupStep, SetupStepState,
    WorkspaceConfigResponse,
};

use super::StepPhase;
use super::wire::{self, SetupLanguage};
use crate::failure::FailureText;
use crate::model::{Effect, Slot, Tickets};
use crate::signed_in::SignedIn;
use crate::text::l;

/// Step 3's form.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReceptionistStep {
    /// The persona's read.
    pub phase: StepPhase,
    /// The receptionist's name.
    pub name: String,
    /// What it says when it answers.
    pub greeting: String,
    /// Whether the owner wrote the greeting (it then no longer follows the
    /// name).
    pub greeting_edited: bool,
    /// The voice chosen, when voices are offered.
    pub voice: Option<String>,
    /// The voices offered, by group. Empty offers none.
    pub voice_groups: Vec<PersonaVoiceGroup>,
    /// Whether to show the fields' errors (after a first Continue).
    pub show_errors: bool,
    /// The step is being saved.
    pub saving: bool,
    /// Why the save failed.
    pub save_error: Option<FailureText>,
    /// The persona's stored language.
    pub(crate) stored_language: Option<String>,
    /// The persona read, kept until the options land.
    pub(crate) persona: Option<Option<AiPersona>>,
    /// The options read, kept until the persona lands; `None` inside when the
    /// read failed.
    pub(crate) options: Option<Option<PersonaOptionsResponse>>,
}

/// What the person does on step 3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceptionistEvent {
    /// Read the persona again, after its read failed.
    Retry,
    /// The name field changed; an untouched greeting follows it.
    SetName(String),
    /// The greeting field changed.
    SetGreeting(String),
    /// A voice was chosen.
    SetVoice(String),
    /// Save the step and move on.
    Save,
}

impl ReceptionistStep {
    /// The name field's error.
    pub fn name_error(&self) -> Option<&'static str> {
        self.name
            .trim()
            .is_empty()
            .then(|| t::setup_receptionist_name_required(l()))
    }

    /// The greeting field's error.
    pub fn greeting_error(&self) -> Option<&'static str> {
        self.greeting
            .trim()
            .is_empty()
            .then(|| t::setup_receptionist_greeting_required(l()))
    }

    /// Whether the greeting is the starter one, which the business's own
    /// replaces.
    pub fn is_starter(greeting: &str, name: &str, business: &str) -> bool {
        if greeting.is_empty() || wire::STARTER_GREETINGS.contains(&greeting) {
            return true;
        }
        SetupLanguage::ALL.iter().any(|language| {
            greeting == wire::greeting(business, name, *language)
                || greeting == wire::greeting("", name, *language)
        })
    }
}

impl super::SetupWizard {
    /// Step 3's sentence on the AI disclosure: required by law in the EU,
    /// always said elsewhere.
    pub fn disclosure_line(&self) -> &'static str {
        if self.region() == Some("eu") {
            t::setup_receptionist_disclosure_required(l())
        } else {
            t::setup_receptionist_disclosure_always(l())
        }
    }
}

impl SignedIn {
    /// Opens step 3: reads the persona and its options.
    pub(crate) fn open_receptionist(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.setup.receptionist = ReceptionistStep::default();
        let workspace_id = self.workspace_id();
        vec![
            Effect::LoadWorkspaceConfig {
                ticket: tickets.issue(Slot::SetupReceptionistConfig),
                workspace_id: workspace_id.clone(),
            },
            Effect::LoadPersonaOptions {
                ticket: tickets.issue(Slot::SetupPersonaOptions),
                workspace_id,
            },
        ]
    }

    fn receptionist_language(&self) -> SetupLanguage {
        self.setup_draft().language
    }

    /// The persona `language` the save writes.
    pub(crate) fn receptionist_language_to_save(&self) -> String {
        SetupLanguage::to_save(
            self.setup.receptionist.stored_language.as_deref(),
            self.receptionist_language(),
            SetupLanguage::app_default(l()),
        )
    }

    fn receptionist_template(&self, name: &str) -> String {
        let shown = if name.trim().is_empty() {
            wire::FALLBACK_NAME
        } else {
            name.trim()
        };
        wire::greeting(
            &self.setup_draft().business_name,
            shown,
            self.receptionist_language(),
        )
    }

    pub(crate) fn receptionist_event(
        &mut self,
        event: ReceptionistEvent,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        match event {
            ReceptionistEvent::Retry => {
                if matches!(self.setup.receptionist.phase, StepPhase::Failed(_)) {
                    return self.open_receptionist(tickets);
                }
            }
            ReceptionistEvent::SetName(name) => {
                if !self.setup.receptionist.greeting_edited {
                    self.setup.receptionist.greeting = self.receptionist_template(&name);
                }
                self.setup.receptionist.name = name;
            }
            ReceptionistEvent::SetGreeting(greeting) => {
                self.setup.receptionist.greeting = greeting;
                self.setup.receptionist.greeting_edited = true;
            }
            ReceptionistEvent::SetVoice(voice) => self.setup.receptionist.voice = Some(voice),
            ReceptionistEvent::Save => return self.save_receptionist(tickets),
        }
        Vec::new()
    }

    /// The persona read for step 3 landed.
    pub(crate) fn receptionist_config_loaded(
        &mut self,
        result: Result<WorkspaceConfigResponse, ApiError>,
    ) {
        match result {
            Ok(response) => {
                self.setup.receptionist.persona = Some(response.config.ai_persona);
                self.adopt_receptionist();
            }
            Err(error) => {
                self.setup.receptionist.phase =
                    StepPhase::Failed(FailureText::from_api_error(&error));
            }
        }
    }

    /// The options read for step 3 landed. A failed read offers no voices.
    pub(crate) fn receptionist_options_loaded(
        &mut self,
        result: Result<PersonaOptionsResponse, ApiError>,
    ) {
        self.setup.receptionist.options = Some(result.ok());
        self.adopt_receptionist();
    }

    /// Both reads are in: the form opens on what the persona holds.
    fn adopt_receptionist(&mut self) {
        let step = &self.setup.receptionist;
        // Each open resets the step and reads each once, so both are in only
        // once per open, still Loading (a failed persona read leaves it None).
        let (Some(persona), Some(options)) = (step.persona.clone(), step.options.clone()) else {
            return;
        };
        let persona = persona.as_ref();
        let text = |field: fn(&AiPersona) -> Option<&String>| persona.and_then(field).cloned();
        self.setup.receptionist.stored_language = text(|p| p.language.as_ref());
        let stored_name = text(|p| p.name.as_ref())
            .unwrap_or_default()
            .trim()
            .to_owned();
        let name = if stored_name.is_empty() {
            wire::FALLBACK_NAME.to_owned()
        } else {
            stored_name
        };
        let stored = text(|p| p.greeting.as_ref())
            .unwrap_or_default()
            .trim()
            .to_owned();
        let business = self.setup_draft().business_name;
        let (greeting, edited) = if ReceptionistStep::is_starter(&stored, &name, &business) {
            (self.receptionist_template(&name), false)
        } else {
            (stored, true)
        };
        let language = self.receptionist_language_to_save();
        let engine = text(|p| p.model_id.as_ref());
        let groups: Vec<PersonaVoiceGroup> = match (&options, &engine) {
            (Some(options), Some(engine)) => options.voice_groups(engine, &language).to_vec(),
            _ => Vec::new(),
        };
        let exists = |voice: &str| {
            groups
                .iter()
                .any(|group| group.options.iter().any(|option| option.value == voice))
        };
        let stored_voice = text(|p| p.voice.as_ref());
        let voice = match (&stored_voice, &options, &engine) {
            (Some(voice), _, _) if exists(voice) => Some(voice.clone()),
            (_, Some(options), Some(engine)) => options
                .default_voice(engine, &language)
                .map(str::to_owned)
                .or_else(|| first_voice(&groups)),
            _ => None,
        };
        let step = &mut self.setup.receptionist;
        step.name = name;
        step.greeting = greeting;
        step.greeting_edited = edited;
        step.voice_groups = groups;
        step.voice = voice;
        step.phase = StepPhase::Ready;
    }

    fn save_receptionist(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let step = &mut self.setup.receptionist;
        step.show_errors = true;
        if step.phase != StepPhase::Ready
            || step.name_error().is_some()
            || step.greeting_error().is_some()
            || step.saving
        {
            return Vec::new();
        }
        step.saving = true;
        step.save_error = None;
        let patch = PersonaPatch {
            name: Some(step.name.trim().to_owned()),
            greeting: Some(step.greeting.trim().to_owned()),
            voice: if step.voice_groups.is_empty() {
                None
            } else {
                step.voice.clone()
            },
            language: Some(self.receptionist_language_to_save()),
            ..PersonaPatch::default()
        };
        self.begin_step_write();
        vec![Effect::SavePersona {
            ticket: tickets.issue(Slot::SetupPersonaSave),
            workspace_id: self.workspace_id(),
            patch: Box::new(patch),
        }]
    }

    /// The persona save landed, or not.
    pub(crate) fn receptionist_saved(
        &mut self,
        result: Result<(), ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        self.setup.receptionist.saving = false;
        self.end_step_write();
        if let Err(error) = result {
            self.setup.receptionist.save_error = Some(FailureText::from_api_error(&error));
            return Vec::new();
        }
        self.setup.receptionist.stored_language = Some(self.receptionist_language_to_save());
        self.complete_step(SetupStep::Receptionist, SetupStepState::Done, tickets)
    }
}

fn first_voice(groups: &[PersonaVoiceGroup]) -> Option<String> {
    groups
        .first()
        .and_then(|group| group.options.first())
        .map(|option| option.value.clone())
}
