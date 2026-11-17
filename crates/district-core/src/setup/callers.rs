//! Step 4, "What callers should know": the time zone, the address, the opening
//! hours, the services and the common questions the receptionist answers from.
//!
//! A drafted item arrives switched off and stays off until the owner switches
//! it on: the website read produces drafts, never facts, and only enabled items
//! reach the agent. An item the owner adds by hand starts on, because typing
//! one is the confirmation.
//!
//! Hours from the website are a draft until the owner uses them. "Fill from my
//! website" puts the lookup's hours on screen only; a save leaves `weeklyHours`
//! out while that draft is unconfirmed, and Continue refuses to move on past it
//! rather than drop it silently. "Use these hours", or editing any day,
//! confirms it; "Discard" puts back what was there.
//!
//! Saved on Save and on Continue, not on every keystroke. Every section present
//! replaces the stored one whole, so each save sends all five, and an empty
//! section is sent as `null` (a clear).

use district_api::ApiError;
use district_l10n::setup_strings as t;
use district_model::{SetupLookupResponse, SetupResponse, SetupStep, SetupStepState};
use serde_json::{Map, Value};

use super::facts::{
    self, AddressDraft, DayDraft, DayState, FaqDraft, Interval, ServiceDraft, Week, Weekday,
};
use super::places;
use crate::model::{Effect, Slot, Tickets};
use crate::signed_in::SignedIn;
use crate::text::l;

/// Step 4's form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallersStep {
    /// The time zone, or "".
    pub timezone: String,
    /// The address.
    pub address: AddressDraft,
    /// The week, Monday first.
    pub hours: Week,
    /// The services.
    pub services: Vec<ServiceDraft>,
    /// The questions.
    pub faqs: Vec<FaqDraft>,
    /// The website to read hours from.
    pub website_url: String,
    /// The hours on screen are the website's, not yet confirmed.
    pub hours_draft_pending: bool,
    /// What the hours read found, or why it failed.
    pub hours_notice: Option<String>,
    /// The website is being read.
    pub reading: bool,
    /// The step is being saved.
    pub saving: bool,
    /// The last save landed.
    pub saved: bool,
    /// Whether to show the fields' errors (after a first save).
    pub show_errors: bool,
    /// Why the save failed, or was refused.
    pub save_error: Option<String>,
    /// The week before the website's hours were put on screen.
    pub(crate) hours_before_draft: Week,
    /// Whether the save on its way moves on when it lands.
    pub(crate) continuing: bool,
}

/// What the person does on step 4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallersEvent {
    /// A time zone was chosen.
    SetTimezone(String),
    /// The address changed.
    SetAddress(AddressDraft),
    /// A day was set open, closed or not set. A day opened with no period
    /// starts with nine to five.
    SetDayState(Weekday, DayState),
    /// One more period on a day, after its last.
    AddPeriod(Weekday),
    /// A day's period changed, by its place in the day's list.
    SetPeriod(Weekday, usize, Interval),
    /// A day's period went; a day with none left is not set.
    RemovePeriod(Weekday, usize),
    /// One more service, switched on.
    AddService,
    /// A service changed, by its place in the list.
    SetService(usize, ServiceDraft),
    /// A service went.
    RemoveService(usize),
    /// One more question, switched on.
    AddFaq,
    /// A question changed, by its place in the list.
    SetFaq(usize, FaqDraft),
    /// A question went.
    RemoveFaq(usize),
    /// The website field changed.
    SetWebsite(String),
    /// Read the website's hours onto the screen.
    FillHoursFromWebsite,
    /// Use the website's hours.
    UseDraftHours,
    /// Put back the hours there were before the website's.
    DiscardDraftHours,
    /// Save the step and stay.
    Save,
    /// Save the step and move on.
    SaveAndContinue,
}

impl CallersStep {
    /// Whether any service or question is a draft (switched off).
    pub fn has_drafts(&self) -> bool {
        self.services.iter().any(|service| !service.enabled)
            || self.faqs.iter().any(|faq| !faq.enabled)
    }

    /// Whether the form can be saved.
    pub fn is_valid(&self) -> bool {
        facts::address_is_valid(&self.address)
            && self.hours.iter().all(facts::day_is_valid)
            && self.services.iter().all(facts::service_is_valid)
            && self.faqs.iter().all(facts::faq_is_valid)
    }

    /// The facts the save writes: all five sections.
    pub fn patch(&self) -> Map<String, Value> {
        let mut patch = Map::new();
        patch.insert("address".into(), facts::address_json(&self.address));
        patch.insert("services".into(), facts::services_json(&self.services));
        patch.insert("faqs".into(), facts::faqs_json(&self.faqs));
        if !self.timezone.is_empty() {
            patch.insert("timezone".into(), self.timezone.clone().into());
        }
        if !self.hours_draft_pending {
            patch.insert("weeklyHours".into(), facts::hours_json(&self.hours));
        }
        patch
    }

    fn day(&mut self, day: Weekday) -> &mut DayDraft {
        // An edit to any day confirms the website's hours.
        self.hours_draft_pending = false;
        &mut self.hours[Weekday::ALL
            .iter()
            .position(|each| *each == day)
            .unwrap_or_default()]
    }
}

impl SignedIn {
    /// Opens step 4 on what the service holds.
    pub(crate) fn open_callers(&mut self) {
        let stored = self.setup.facts();
        let weekly = stored.and_then(|facts| facts.get("weeklyHours"));
        self.setup.callers = CallersStep {
            timezone: facts::timezone(stored),
            address: facts::address(stored),
            hours: facts::hours(weekly),
            services: facts::services(stored),
            faqs: facts::faqs(stored),
            ..CallersStep::default()
        };
    }

    /// The zones step 4 offers.
    pub fn callers_zones(&self) -> Vec<String> {
        let step = &self.setup.callers;
        let country = if step.address.country.is_empty() {
            self.setup_draft().country
        } else {
            step.address.country.to_ascii_uppercase()
        };
        places::zones_with(&country, &step.timezone)
    }

    pub(crate) fn callers_event(
        &mut self,
        event: CallersEvent,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let step = &mut self.setup.callers;
        match event {
            CallersEvent::SetTimezone(zone) => step.timezone = zone,
            CallersEvent::SetAddress(address) => step.address = address,
            CallersEvent::SetDayState(day, state) => {
                let draft = step.day(day);
                draft.state = state;
                if state == DayState::Open && draft.intervals.is_empty() {
                    draft.intervals = vec![Interval::STANDARD];
                }
            }
            CallersEvent::AddPeriod(day) => {
                let draft = step.day(day);
                if draft.state == DayState::Open
                    && draft.intervals.len() < facts::limit::INTERVALS_PER_DAY
                {
                    let last = draft
                        .intervals
                        .iter()
                        .map(|period| period.close)
                        .max()
                        .unwrap_or(9 * 60);
                    let open = (last + 60).min(23 * 60);
                    draft.intervals.push(Interval {
                        open,
                        close: (open + 60).min(24 * 60),
                    });
                }
            }
            CallersEvent::SetPeriod(day, index, period) => {
                if let Some(slot) = step.day(day).intervals.get_mut(index) {
                    *slot = period;
                }
            }
            CallersEvent::RemovePeriod(day, index) => {
                let draft = step.day(day);
                if index < draft.intervals.len() {
                    draft.intervals.remove(index);
                    if draft.intervals.is_empty() {
                        draft.state = DayState::NotSet;
                    }
                }
            }
            CallersEvent::AddService => {
                if step.services.len() < facts::limit::SERVICES {
                    step.services.push(ServiceDraft {
                        enabled: true,
                        ..ServiceDraft::default()
                    });
                }
            }
            CallersEvent::SetService(index, service) => {
                if let Some(slot) = step.services.get_mut(index) {
                    *slot = service;
                }
            }
            CallersEvent::RemoveService(index) => {
                if index < step.services.len() {
                    step.services.remove(index);
                }
            }
            CallersEvent::AddFaq => {
                if step.faqs.len() < facts::limit::FAQS {
                    step.faqs.push(FaqDraft {
                        enabled: true,
                        ..FaqDraft::default()
                    });
                }
            }
            CallersEvent::SetFaq(index, faq) => {
                if let Some(slot) = step.faqs.get_mut(index) {
                    *slot = faq;
                }
            }
            CallersEvent::RemoveFaq(index) => {
                if index < step.faqs.len() {
                    step.faqs.remove(index);
                }
            }
            CallersEvent::SetWebsite(url) => step.website_url = url,
            CallersEvent::UseDraftHours => step.hours_draft_pending = false,
            CallersEvent::DiscardDraftHours => {
                if step.hours_draft_pending {
                    step.hours = step.hours_before_draft.clone();
                    step.hours_draft_pending = false;
                }
            }
            CallersEvent::FillHoursFromWebsite => {
                let url = step.website_url.trim().to_owned();
                if step.reading || url.is_empty() {
                    return Vec::new();
                }
                step.reading = true;
                step.hours_notice = None;
                return vec![Effect::ReadSetupWebsite {
                    ticket: tickets.issue(Slot::SetupHoursLookup),
                    workspace_id: self.workspace_id(),
                    website_url: url,
                    locale: l(),
                }];
            }
            CallersEvent::Save => return self.save_callers(false, tickets),
            CallersEvent::SaveAndContinue => {
                if step.saving {
                    return Vec::new();
                }
                if step.hours_draft_pending {
                    step.show_errors = true;
                    step.save_error = Some(t::setup_callers_confirm_hours_first(l()).to_owned());
                    return Vec::new();
                }
                return self.save_callers(true, tickets);
            }
        }
        Vec::new()
    }

    /// The hours read answered: its hours go on screen, as a draft.
    pub(crate) fn callers_hours_read(&mut self, result: Result<SetupLookupResponse, ApiError>) {
        let step = &mut self.setup.callers;
        step.reading = false;
        let found = match result {
            Ok(found) => found.draft.weekly_hours,
            Err(error) => {
                step.hours_notice = Some(crate::FailureText::from_api_error(&error).message);
                return;
            }
        };
        let Some(found) = found.filter(|week| !week.is_empty()) else {
            step.hours_notice = Some(t::setup_callers_no_hours_found(l()).to_owned());
            return;
        };
        if !step.hours_draft_pending {
            step.hours_before_draft = step.hours.clone();
        }
        step.hours = facts::hours(Some(&Value::Object(found)));
        step.hours_draft_pending = true;
    }

    fn save_callers(&mut self, continuing: bool, tickets: &mut Tickets) -> Vec<Effect> {
        let step = &mut self.setup.callers;
        if step.saving {
            return Vec::new();
        }
        step.show_errors = true;
        step.saved = false;
        if !step.is_valid() {
            step.save_error = Some(t::setup_callers_fix_errors(l()).to_owned());
            return Vec::new();
        }
        step.saving = true;
        step.continuing = continuing;
        step.save_error = None;
        let patch = step.patch();
        self.begin_step_write();
        vec![self.save_setup_facts(patch, Slot::SetupCallersSave, tickets)]
    }

    /// The step's save landed, or not.
    pub(crate) fn callers_saved(
        &mut self,
        result: Result<SetupResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        self.setup.callers.saving = false;
        self.end_step_write();
        match result {
            Ok(response) => {
                self.setup.data = Some(response);
                self.setup.callers.saved = true;
            }
            Err(error) => {
                self.setup.callers.save_error =
                    Some(crate::FailureText::from_api_error(&error).message);
                return Vec::new();
            }
        }
        if self.setup.callers.continuing {
            return self.complete_step(SetupStep::Callers, SetupStepState::Done, tickets);
        }
        Vec::new()
    }
}
