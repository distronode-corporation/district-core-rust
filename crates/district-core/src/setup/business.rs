//! Step 1, "Find your business": the website read, then who the business is
//! and where.
//!
//! "Read my website" saves drafts, never live facts (the web's
//! `Step1Business`): the lookup writes nothing, and this appends what it found
//! to the stored services and questions, every one switched off as the service
//! drafted it, so nothing reaches a caller until the owner switches it on in
//! step 4. Drafted hours are not saved at all: a day has no `enabled` flag, so
//! stored hours are live. The step says hours were found and step 4 reads them
//! again.
//!
//! The business name is the workspace's name, renamed through the existing
//! route and only when it changed. The language is not saved here: it goes
//! into the draft, and step 3 writes it into the persona. "What you do" is
//! stored as one question in the receptionist's language, because the facts
//! have no description field; a switch turned off in step 4 is kept.
//!
//! The read and the save never overlap: each builds its questions from the
//! stored ones, and the service replaces a section whole, so whichever
//! answered second would drop the other's.

use district_api::ApiError;
use district_l10n::{Locale, setup_strings as t};
use district_model::{
    RenameResponse, SetupDraftFaq, SetupDraftService, SetupLookupResponse, SetupResponse,
    SetupStep, SetupStepState,
};
use serde_json::{Map, Value, json};

use super::places::{self, Subdivision, Zone};
use super::wire::{self, SetupLanguage};
use super::{BusinessDraft, facts};
use crate::failure::FailureText;
use crate::model::{Effect, Slot, Tickets};
use crate::signed_in::SignedIn;
use crate::text::l;

/// What the website read found, or why it found nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusinessNotice {
    /// What was found, in one sentence.
    Found(String),
    /// The page was read and nothing on it could be used.
    Nothing,
    /// The read failed, in a sentence.
    Failed(String),
}

impl BusinessNotice {
    /// The sentence to show.
    pub fn message(&self) -> String {
        match self {
            Self::Found(text) | Self::Failed(text) => text.clone(),
            Self::Nothing => t::setup_business_read_nothing(l()).to_owned(),
        }
    }
}

/// Step 1's form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusinessStep {
    /// The website to read.
    pub website_url: String,
    /// The business's name.
    pub business_name: String,
    /// What the business does.
    pub what: String,
    /// The two-letter country code, or "".
    pub country: String,
    /// The province or state code, or the region as typed.
    pub subdivision: String,
    /// The city.
    pub city: String,
    /// The street line.
    pub street: String,
    /// The receptionist's language.
    pub language: SetupLanguage,
    /// What the website read found.
    pub notice: Option<BusinessNotice>,
    /// The website is being read.
    pub reading: bool,
    /// The step is being saved.
    pub saving: bool,
    /// Whether to show the fields' errors (after a first Continue).
    pub show_errors: bool,
    /// Why the save failed.
    pub save_error: Option<FailureText>,
}

impl Default for BusinessStep {
    fn default() -> Self {
        Self {
            website_url: String::new(),
            business_name: String::new(),
            what: String::new(),
            country: String::new(),
            subdivision: String::new(),
            city: String::new(),
            street: String::new(),
            language: SetupLanguage::En,
            notice: None,
            reading: false,
            saving: false,
            show_errors: false,
            save_error: None,
        }
    }
}

/// What the person does on step 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusinessEvent {
    /// The website field changed.
    SetWebsite(String),
    /// Read the website.
    ReadWebsite,
    /// Dismiss what the read found.
    DismissNotice,
    /// The name field changed.
    SetName(String),
    /// The "what you do" field changed.
    SetWhat(String),
    /// A country was chosen; its subdivision is cleared.
    ChooseCountry(String),
    /// A province or state was chosen, or a region typed.
    SetSubdivision(String),
    /// The city field changed.
    SetCity(String),
    /// The street field changed.
    SetStreet(String),
    /// The receptionist's language was chosen.
    SetLanguage(SetupLanguage),
    /// Save the step and move on.
    Save,
}

impl BusinessStep {
    /// Whether the read or the save is on its way.
    pub fn is_busy(&self) -> bool {
        self.reading || self.saving
    }

    /// The country's provinces or states, or `None` for a country with none.
    pub fn subdivisions(&self) -> Option<&'static [Subdivision]> {
        places::subdivisions(&self.country)
    }

    /// The label of the subdivision field.
    pub fn subdivision_label(&self) -> &'static str {
        match self.country.as_str() {
            "CA" => t::setup_business_subdivision_label_ca(l()),
            "US" => t::setup_business_subdivision_label_us(l()),
            _ => t::setup_business_subdivision_label_other(l()),
        }
    }

    /// The zone the place implies.
    pub fn zone(&self) -> Option<Zone> {
        if self.country.is_empty() {
            return None;
        }
        places::zone(&self.country, &self.subdivision)
    }

    /// The zone a save writes: the stored one while the place is the stored
    /// place (step 4 may have chosen another), the place's own otherwise.
    pub fn timezone(&self, stored: Option<&Map<String, Value>>) -> Option<String> {
        let zone = facts::timezone(stored);
        let saved = facts::address(stored);
        let same_place = saved.country.trim().eq_ignore_ascii_case(&self.country)
            && saved
                .region
                .trim()
                .eq_ignore_ascii_case(self.subdivision.trim())
            && saved.city.trim().to_lowercase() == self.city.trim().to_lowercase();
        if !zone.is_empty() && same_place {
            return Some(zone);
        }
        self.zone().map(|zone| zone.timezone.to_owned())
    }

    /// The sentence under the time zone heading.
    pub fn timezone_line(&self, stored: Option<&Map<String, Value>>) -> String {
        let Some(timezone) = self.timezone(stored) else {
            return t::setup_business_timezone_unknown(l()).to_owned();
        };
        match self.zone() {
            Some(zone) if zone.timezone == timezone && zone.multi_zone => {
                t::setup_business_timezone_multi(l(), &timezone)
            }
            _ => t::setup_business_timezone_value(l(), &timezone),
        }
    }

    /// The name field's error.
    pub fn name_error(&self) -> Option<&'static str> {
        self.business_name
            .trim()
            .is_empty()
            .then(|| t::setup_business_name_required(l()))
    }

    /// The country field's error.
    pub fn country_error(&self) -> Option<&'static str> {
        self.country
            .is_empty()
            .then(|| t::setup_business_country_required(l()))
    }

    /// The subdivision field's error.
    pub fn subdivision_error(&self) -> Option<&'static str> {
        (self.subdivisions().is_some() && self.subdivision.is_empty())
            .then(|| t::setup_business_subdivision_required(l()))
    }

    /// The city field's error.
    pub fn city_error(&self) -> Option<&'static str> {
        self.city
            .trim()
            .is_empty()
            .then(|| t::setup_business_city_required(l()))
    }

    /// Whether the form can be saved.
    pub fn is_valid(&self) -> bool {
        self.name_error().is_none()
            && self.country_error().is_none()
            && self.subdivision_error().is_none()
            && self.city_error().is_none()
    }

    /// The save's refusal when "what you do" would be a question past the cap.
    pub fn faqs_full_error(&self, stored: Option<&Map<String, Value>>) -> Option<String> {
        if self.what.trim().is_empty() {
            return None;
        }
        let others = stored_faqs(stored).1;
        (others.len() >= facts::limit::FAQS)
            .then(|| t::setup_business_what_faqs_full(l(), facts::limit::FAQS as i64))
    }

    /// The facts the save writes: the zone, the address, and "what you do" as
    /// its question.
    pub fn facts_patch(&self, stored: Option<&Map<String, Value>>) -> Map<String, Value> {
        let mut patch = Map::new();
        if let Some(timezone) = self.timezone(stored) {
            patch.insert("timezone".into(), timezone.into());
        }
        let mut address = Map::new();
        address.insert("city".into(), self.city.trim().into());
        address.insert("country".into(), self.country.clone().into());
        if !self.street.trim().is_empty() {
            address.insert("line1".into(), self.street.trim().into());
        }
        if !self.subdivision.trim().is_empty() {
            address.insert("region".into(), self.subdivision.trim().into());
        }
        let postal_code = facts::address(stored).postal_code;
        if !postal_code.trim().is_empty() {
            address.insert("postalCode".into(), postal_code.trim().into());
        }
        patch.insert("address".into(), Value::Object(address));
        let (all, others) = stored_faqs(stored);
        if !self.what.trim().is_empty() {
            // The owner's switch is kept: a "what you do" question switched off
            // in step 4 stays off. Only a new one starts on.
            let enabled = all
                .iter()
                .find(|faq| is_what(faq))
                .is_none_or(|faq| faq.get("enabled") == Some(&Value::Bool(true)));
            let answer: String = self
                .what
                .trim()
                .chars()
                .take(facts::limit::FAQ_ANSWER)
                .collect();
            let mut rows = vec![json!({
                "question": wire::what_question(self.language),
                "answer": answer,
                "enabled": enabled,
            })];
            rows.extend(others);
            patch.insert("faqs".into(), Value::Array(rows));
        } else if others.len() != all.len() {
            patch.insert("faqs".into(), Value::Array(others));
        }
        patch
    }
}

fn is_what(faq: &Value) -> bool {
    wire::is_what_question(
        faq.get("question")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    )
}

/// The stored questions, and those other than "what you do".
fn stored_faqs(stored: Option<&Map<String, Value>>) -> (Vec<Value>, Vec<Value>) {
    let all: Vec<Value> = stored
        .and_then(|facts| facts.get("faqs"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let others = all.iter().filter(|faq| !is_what(faq)).cloned().collect();
    (all, others)
}

fn service_row(service: &SetupDraftService) -> Value {
    let mut row = Map::new();
    row.insert("name".into(), service.name.clone().into());
    if let Some(price) = &service.price {
        row.insert("price".into(), price.clone().into());
    }
    row.insert("enabled".into(), service.enabled.into());
    Value::Object(row)
}

fn faq_row(faq: &SetupDraftFaq) -> Value {
    json!({"question": faq.question, "answer": faq.answer, "enabled": faq.enabled})
}

/// What the read found, in one sentence (French takes the singular for 0 and
/// 1).
pub fn found_sentence(services: usize, faqs: usize, hours: bool, address: bool) -> String {
    let locale = l();
    let singular = |count: usize| match locale {
        Locale::En => count == 1,
        Locale::FrCa => count <= 1,
    };
    let mut parts = Vec::new();
    if services > 0 || faqs > 0 {
        let services_text = if singular(services) {
            t::setup_business_found_services_one(locale, services as i64)
        } else {
            t::setup_business_found_services_other(locale, services as i64)
        };
        let faqs_text = if singular(faqs) {
            t::setup_business_found_faqs_one(locale, faqs as i64)
        } else {
            t::setup_business_found_faqs_other(locale, faqs as i64)
        };
        parts.push(t::setup_business_read_result(
            locale,
            &services_text,
            &faqs_text,
        ));
    }
    if hours {
        parts.push(t::setup_business_hours_found(locale).to_owned());
    }
    if parts.is_empty() && address {
        parts.push(t::setup_business_address_found(locale).to_owned());
    }
    parts.join(" ")
}

impl SignedIn {
    /// Opens step 1 on the draft and what the service holds. The saved
    /// address beats the draft's place: step 4 or the web may have changed it
    /// since, and this step writes the address back whole.
    pub(crate) fn open_business(&mut self) {
        let draft = self.setup_draft();
        let stored = self.setup.facts();
        let place = if stored
            .and_then(|facts| facts.get("address"))
            .and_then(Value::as_object)
            .is_some()
        {
            BusinessDraft::initial(
                self.setup.data.as_ref(),
                None,
                &self.setup.workspace_name,
                l(),
            )
        } else {
            draft.clone()
        };
        self.setup.business = BusinessStep {
            business_name: draft.business_name,
            what: facts::what(stored),
            country: place.country,
            subdivision: place.subdivision,
            city: place.city,
            street: facts::address(stored).line1,
            language: draft.language,
            ..BusinessStep::default()
        };
    }

    pub(crate) fn business_event(
        &mut self,
        event: BusinessEvent,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let step = &mut self.setup.business;
        match event {
            BusinessEvent::SetWebsite(url) => step.website_url = url,
            BusinessEvent::DismissNotice => step.notice = None,
            BusinessEvent::SetName(name) => step.business_name = name,
            BusinessEvent::SetWhat(what) => step.what = what,
            BusinessEvent::ChooseCountry(country) => {
                if country != step.country {
                    step.country = country;
                    step.subdivision.clear();
                }
            }
            BusinessEvent::SetSubdivision(subdivision) => step.subdivision = subdivision,
            BusinessEvent::SetCity(city) => step.city = city,
            BusinessEvent::SetStreet(street) => step.street = street,
            BusinessEvent::SetLanguage(language) => step.language = language,
            BusinessEvent::ReadWebsite => return self.read_business_website(tickets),
            BusinessEvent::Save => return self.save_business(tickets),
        }
        Vec::new()
    }

    fn read_business_website(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let url = self.setup.business.website_url.trim().to_owned();
        if self.setup.business.is_busy() || url.is_empty() {
            return Vec::new();
        }
        self.setup.business.reading = true;
        self.setup.business.notice = None;
        self.begin_step_write();
        vec![Effect::ReadSetupWebsite {
            ticket: tickets.issue(Slot::SetupLookup),
            workspace_id: self.workspace_id(),
            website_url: url,
            locale: l(),
        }]
    }

    /// The website read answered: what it found is shown, and its services and
    /// questions are saved as drafts.
    pub(crate) fn business_website_read(
        &mut self,
        result: Result<SetupLookupResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let found = match result {
            Ok(found) => found.draft,
            Err(error) => {
                self.end_business_read();
                self.setup.business.notice = Some(BusinessNotice::Failed(
                    FailureText::from_api_error(&error).message,
                ));
                return Vec::new();
            }
        };
        let hours = found
            .weekly_hours
            .as_ref()
            .is_some_and(|week| !week.is_empty());
        if found.services.is_empty() && found.faqs.is_empty() && !hours && found.address.is_none() {
            self.end_business_read();
            self.setup.business.notice = Some(BusinessNotice::Nothing);
            return Vec::new();
        }
        self.fill_business_address(found.address.as_ref());
        let stored = self.setup.facts();
        let current = |key: &str| {
            stored
                .and_then(|facts| facts.get(key))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        };
        let mut patch = Map::new();
        if !found.services.is_empty() {
            let rows = found.services.iter().map(service_row).collect();
            patch.insert(
                "services".into(),
                Value::Array(facts::appending(
                    rows,
                    &current("services"),
                    "name",
                    facts::limit::SERVICES,
                )),
            );
        }
        if !found.faqs.is_empty() {
            let rows = found.faqs.iter().map(faq_row).collect();
            patch.insert(
                "faqs".into(),
                Value::Array(facts::appending(
                    rows,
                    &current("faqs"),
                    "question",
                    facts::limit::FAQS,
                )),
            );
        }
        let sentence = found_sentence(
            found.services.len(),
            found.faqs.len(),
            hours,
            found.address.is_some(),
        );
        if patch.is_empty() {
            self.end_business_read();
            self.setup.business.notice = Some(BusinessNotice::Found(sentence));
            return Vec::new();
        }
        self.setup_found_sentence = Some(sentence);
        vec![self.save_setup_facts(patch, Slot::SetupLookupSave, tickets)]
    }

    fn end_business_read(&mut self) {
        self.setup.business.reading = false;
        self.end_step_write();
    }

    /// The drafts the read found were saved, or not.
    pub(crate) fn business_drafts_saved(&mut self, result: Result<SetupResponse, ApiError>) {
        self.end_business_read();
        let sentence = self.setup_found_sentence.take().unwrap_or_default();
        match result {
            Ok(response) => {
                self.setup.data = Some(response);
                self.setup.business.notice = Some(BusinessNotice::Found(sentence));
            }
            Err(error) => {
                self.setup.business.notice = Some(BusinessNotice::Failed(
                    FailureText::from_api_error(&error).message,
                ));
            }
        }
    }

    fn fill_business_address(&mut self, address: Option<&Map<String, Value>>) {
        let Some(address) = address else {
            return;
        };
        let text = |key: &str| address.get(key).and_then(Value::as_str);
        let step = &mut self.setup.business;
        if step.street.trim().is_empty()
            && let Some(line1) = text("line1")
        {
            step.street = line1.to_owned();
        }
        if step.city.trim().is_empty()
            && let Some(city) = text("city")
        {
            step.city = city.to_owned();
        }
        let country = text("country").unwrap_or_default();
        let region = text("region").unwrap_or_default();
        if step.country.is_empty() && places::countries().contains(&country) {
            step.country = country.to_owned();
            step.subdivision = places::subdivision_code(region, country);
        } else if step.subdivision.is_empty() && country == step.country {
            step.subdivision = places::subdivision_code(region, country);
        }
    }

    fn save_business(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.setup.business.show_errors = true;
        if !self.setup.business.is_valid() || self.setup.business.is_busy() {
            return Vec::new();
        }
        let stored = self.setup.facts();
        if let Some(full) = self.setup.business.faqs_full_error(stored) {
            self.setup.business.save_error = Some(FailureText::final_(full));
            return Vec::new();
        }
        let patch = self.setup.business.facts_patch(stored);
        self.setup.business.saving = true;
        self.setup.business.save_error = None;
        self.begin_step_write();
        vec![self.save_setup_facts(patch, Slot::SetupBusinessSave, tickets)]
    }

    /// The step's facts landed, or not: then the rename when the name changed,
    /// then the step is done.
    pub(crate) fn business_saved(
        &mut self,
        result: Result<SetupResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        match result {
            Ok(response) => self.setup.data = Some(response),
            Err(error) => return self.business_save_failed(&error),
        }
        let name = self.setup.business.business_name.trim().to_owned();
        if name != self.setup.workspace_name.trim() {
            return vec![Effect::RenameWorkspace {
                ticket: tickets.issue(Slot::SetupRename),
                workspace_id: self.workspace_id(),
                name,
            }];
        }
        self.business_done(tickets)
    }

    /// The rename landed, or not. The name kept is the service's echo: it
    /// trims before it stores.
    pub(crate) fn business_renamed(
        &mut self,
        result: Result<RenameResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        match result {
            Ok(answer) => {
                self.workspaces.rename_active(&answer.name);
                self.setup.workspace_name = answer.name;
                self.business_done(tickets)
            }
            Err(error) => self.business_save_failed(&error),
        }
    }

    fn business_save_failed(&mut self, error: &ApiError) -> Vec<Effect> {
        self.setup.business.saving = false;
        self.end_step_write();
        self.setup.business.save_error = Some(FailureText::from_api_error(error));
        Vec::new()
    }

    fn business_done(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.setup.business.saving = false;
        self.end_step_write();
        let step = &self.setup.business;
        let draft = BusinessDraft {
            business_name: step.business_name.trim().to_owned(),
            country: step.country.clone(),
            subdivision: step.subdivision.trim().to_owned(),
            city: step.city.trim().to_owned(),
            language: step.language,
        };
        self.update_setup_draft(draft);
        self.complete_step(SetupStep::Business, SetupStepState::Done, tickets)
    }
}
