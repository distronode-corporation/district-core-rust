//! A phone number's regulatory registration, on the phone numbers screen's
//! Registrations tab, after the Android app's `RegistrationsTab` and its view
//! model: what a country's regulator asks for, starting a draft filing, the
//! workspace's filings with their status, the carrier's objections and their
//! documents, and filing a draft with the carrier.
//!
//! A draft is the workspace's own: nothing reaches the carrier until it is
//! filed. Only a draft may have a document put on it or taken off, or be
//! filed; every other status is with the carrier. Filing is confirmed first
//! ([`NumberConfirmation::SubmitRegistration`]), because it files a regulated
//! application in the business's name, and it is sent once; the other writes
//! converge (starting a draft that exists is refused, a document replaces the
//! one its requirement had), so each is offered again after a failure.
//!
//! The filings are read on opening the tab and again after every write that
//! lands, a filing that was refused included, because a refusal changes its
//! status and its reasons. A status the carrier moved is seen only when the
//! list is read: the service asks the carrier then, and nothing tells it
//! sooner.
//!
//! A viewer may not file, so the tab reads nothing for one and offers nothing;
//! the screen says why ([`numbers_scope_note`](crate::numbers_scope_note)).
//!
//! Every wire value on the tab (a filing's status, its number type and
//! end-user type, a requirement's kind) is labelled through the wire
//! vocabulary, never shown as sent.

use std::collections::{BTreeMap, BTreeSet};

use district_api::ApiError;
use district_l10n::{WireCategory, numbers_strings as ns, wire_label};
use district_model::{
    CountryRequirements, EndUserAttributes, NumberRegistration, NumberRegistrationDraft,
    NumberRegistrationsResponse, REGISTRATION_END_USER_TYPES, REGISTRATION_NUMBER_TYPES,
    RegistrationDocument, RegistrationDocumentUpload, RegulatoryRequirement,
};

use crate::failure::FailureText;
use crate::model::{Effect, Slot, Tickets};
use crate::number_admin::{NumberAnswer, NumberCall, NumberConfirmation, NumberWrite, Repeat};
use crate::signed_in::{Next, SignedIn};
use crate::text::l;
use crate::workflows::Tone;

/// The Registrations tab's state, a part of
/// [`NumberAdmin`](crate::NumberAdmin).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NumberRegistrations {
    /// The workspace's filings. Read when the tab opens.
    pub list: RegistrationList,
    /// What the form's country asks for, once asked.
    pub requirements: RequirementsState,
    /// The form that starts a filing, and asks what a country requires.
    pub form: RegistrationForm,
    /// The end-user details typed for each draft, by its id, sent when it is
    /// filed. Kept until the draft is filed or the workspace closes.
    pub details: BTreeMap<String, EndUserAttributes>,
    /// Starting a filing, a document put on or taken off, and filing one: one
    /// at a time.
    pub write: NumberWrite,
    /// What [`write`](Self::write) is or was: its kind, and the draft it is
    /// about. Its answer is read as the write it answers, never by what else
    /// is open on screen.
    pub written: Option<RegistrationWrite>,
    /// The drafts whose filing failed in a way that does not prove it was not
    /// filed (a server error, no answer). None is offered for filing again,
    /// because a second filing would be a second regulated application, until
    /// its own filing is confirmed or the list reads it back as no longer a
    /// draft, or the workspace is opened again. Every other draft, and every
    /// other write, is offered as before.
    pub unconfirmed_filing: BTreeSet<String>,
}

/// One write of the Registrations tab: what it does, and to which draft.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RegistrationWrite {
    /// Starting a draft.
    Start,
    /// Putting a document on this draft.
    Upload {
        /// The draft.
        bundle_id: String,
    },
    /// Taking a document off this draft.
    Remove {
        /// The draft.
        bundle_id: String,
    },
    /// Filing this draft with the carrier.
    Submit {
        /// The draft.
        bundle_id: String,
    },
}

impl NumberRegistrations {
    /// The filing `bundle_id`, when the list is read and holds it.
    pub fn registration(&self, bundle_id: &str) -> Option<&NumberRegistration> {
        match &self.list {
            RegistrationList::Ready(list) => list.registrations.iter().find(|r| r.id == bundle_id),
            _ => None,
        }
    }

    /// Whether `bundle_id` is a draft the list holds: the only filing a
    /// document may be put on or taken off, or that may be filed.
    pub fn is_draft(&self, bundle_id: &str) -> bool {
        self.registration(bundle_id)
            .is_some_and(NumberRegistration::is_draft)
    }

    /// Whether no write is on its way. Only one goes at a time.
    fn idle(&self) -> bool {
        !self.write.is_running()
    }

    /// Whether a draft's document controls (Upload, Remove) may be pressed
    /// now.
    pub fn draft_armed(&self, bundle_id: &str) -> bool {
        self.is_draft(bundle_id) && self.idle()
    }

    /// Whether a draft's "File it" may be pressed now: not for a draft whose
    /// filing may already have happened ([`unconfirmed_filing`](Self::unconfirmed_filing)).
    pub fn can_file(&self, bundle_id: &str) -> bool {
        self.draft_armed(bundle_id) && !self.unconfirmed_filing.contains(bundle_id)
    }

    /// Marks the filings as being read, and the call that reads them. A list
    /// already on screen stays while it is read again.
    fn reading(&mut self) -> (Slot, NumberCall) {
        if !matches!(self.list, RegistrationList::Ready(_)) {
            self.list = RegistrationList::Loading;
        }
        (Slot::NumberRegistrations, NumberCall::Registrations)
    }

    /// Whether "Start registration" may be pressed now: a country typed, and
    /// no write on its way.
    pub fn can_start(&self) -> bool {
        self.form.is_submittable() && self.idle()
    }

    /// The form changed to `form`. A change of what the requirements are
    /// asked for (the country, the number type or the end-user type) puts
    /// away the requirements shown, which were for the old question, and
    /// says whether a lookup on its way is for it.
    fn edit(&mut self, form: RegistrationForm) -> bool {
        let asked =
            |f: &RegistrationForm| (f.trimmed_country(), f.number_type(), f.end_user_type());
        let changed = asked(&self.form) != asked(&form);
        if changed {
            self.requirements = RequirementsState::Idle;
        }
        self.form = form;
        changed
    }
}

/// The workspace's filings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RegistrationList {
    /// Never read.
    #[default]
    NotLoaded,
    /// Being read for the first time.
    Loading,
    /// Read. Kept on screen while it is read again.
    Ready(NumberRegistrationsResponse),
    /// The read failed.
    Failed(FailureText),
}

/// What a country's regulator asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RequirementsState {
    /// Not asked.
    #[default]
    Idle,
    /// Being asked.
    Loading,
    /// The regulation.
    Ready {
        /// What it asks for.
        regulation: CountryRequirements,
        /// Whether numbers in the country can be bought through Distronode at
        /// all.
        purchasable: bool,
    },
    /// The country publishes no regulation for the number type: nothing to
    /// file.
    NoRegulation {
        /// The country, in capitals.
        country: String,
        /// Whether numbers in the country can be bought through Distronode at
        /// all.
        purchasable: bool,
    },
    /// The lookup failed, which is not "nothing to file".
    Failed(FailureText),
}

impl RequirementsState {
    /// The heading over a country that asks for nothing:
    /// "DE · No registration required".
    pub fn none_heading(country: &str) -> String {
        ns::numbers_requirements_none_heading(l(), country)
    }
}

/// The form that starts a filing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RegistrationForm {
    /// The country, two letters, as typed.
    pub country: String,
    /// One of [`REGISTRATION_NUMBER_TYPES`]; `local` at first, as the
    /// service's default.
    pub number_type: String,
    /// One of [`REGISTRATION_END_USER_TYPES`]; `business` at first, as the
    /// service's default.
    pub end_user_type: String,
    /// The name the filing gets in the carrier's console. Optional.
    pub friendly_name: String,
}

impl Default for RegistrationForm {
    fn default() -> Self {
        Self {
            country: String::new(),
            number_type: REGISTRATION_NUMBER_TYPES[0].to_owned(),
            end_user_type: REGISTRATION_END_USER_TYPES[0].to_owned(),
            friendly_name: String::new(),
        }
    }
}

impl RegistrationForm {
    /// The number types to choose from: the value sent, and its label.
    pub fn number_types() -> Vec<(&'static str, String)> {
        REGISTRATION_NUMBER_TYPES
            .iter()
            .map(|value| (*value, number_type_label(value)))
            .collect()
    }

    /// The end-user types to choose from: the value sent, and its label.
    pub fn end_user_types() -> Vec<(&'static str, String)> {
        REGISTRATION_END_USER_TYPES
            .iter()
            .map(|value| (*value, end_user_type_label(value)))
            .collect()
    }

    /// The country as sent: trimmed, in capitals.
    pub fn trimmed_country(&self) -> String {
        self.country.trim().to_ascii_uppercase()
    }

    /// Whether "Check requirements" and "Start registration" work: a country
    /// typed. The service checks the rest and says what is wrong.
    pub fn is_submittable(&self) -> bool {
        !self.trimmed_country().is_empty()
    }

    pub(crate) fn number_type(&self) -> Option<String> {
        Some(self.number_type.trim().to_owned()).filter(|t| !t.is_empty())
    }

    pub(crate) fn end_user_type(&self) -> Option<String> {
        Some(self.end_user_type.trim().to_owned()).filter(|t| !t.is_empty())
    }

    pub(crate) fn draft(&self) -> NumberRegistrationDraft {
        NumberRegistrationDraft {
            iso_country: self.trimmed_country(),
            number_type: self.number_type(),
            end_user_type: self.end_user_type(),
            friendly_name: Some(self.friendly_name.trim().to_owned()).filter(|n| !n.is_empty()),
        }
    }
}

/// A filing's status as its badge reads it, through the wire vocabulary. A
/// status the carrier added since reads as the generic "Other", never as sent.
pub fn registration_status(status: &str) -> String {
    wire_label(WireCategory::NumberRegistrationStatus, status, l())
}

/// A regulatory number type's label (`toll free` reads "Toll-free").
pub fn number_type_label(number_type: &str) -> String {
    wire_label(WireCategory::NumberRegistrationNumberType, number_type, l())
}

/// An end-user type's label.
pub fn end_user_type_label(end_user_type: &str) -> String {
    wire_label(
        WireCategory::NumberRegistrationEndUserType,
        end_user_type,
        l(),
    )
}

/// A filing's title: its country and its number type's label,
/// "DE · Local".
pub fn registration_title(registration: &NumberRegistration) -> String {
    ns::numbers_registration_title(
        l(),
        &registration.iso_country,
        &number_type_label(&registration.number_type),
    )
}

/// A requirement's kind as its badge reads it: "Your details" or "Documents".
pub fn requirement_kind(requirement: &RegulatoryRequirement) -> String {
    wire_label(WireCategory::NumberRequirementKind, &requirement.kind, l())
}

/// The fields a requirement asks for, as one line, or "No fields".
pub fn requirement_fields(requirement: &RegulatoryRequirement) -> String {
    if requirement.fields.is_empty() {
        ns::numbers_requirement_no_fields(l()).to_owned()
    } else {
        requirement.fields.join(", ")
    }
}

/// Where a filing's document is: filed with the carrier, stored and not yet
/// filed, or recorded with no file yet.
pub fn document_state(document: &RegistrationDocument) -> &'static str {
    if document.submitted {
        ns::numbers_registration_document_filed(l())
    } else if document.stored {
        ns::numbers_registration_document_stored(l())
    } else {
        ns::numbers_registration_document_recorded(l())
    }
}

impl Tone {
    /// The tone of a filing's status: approved and refused are the two
    /// decisions; a draft, a filing under review and a status this build does
    /// not know are neither.
    pub fn of_registration(status: &str) -> Self {
        match status {
            "twilio-approved" => Self::Success,
            "twilio-rejected" => Self::Danger,
            _ => Self::Neutral,
        }
    }
}

/// What the member does on the Registrations tab, inside
/// [`NumberAdminEvent::Registration`](crate::NumberAdminEvent::Registration).
/// A viewer's are ignored.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RegistrationEvent {
    /// Read the filings again: the tab's "Try again", and how a status the
    /// carrier moved is seen.
    Reload,
    /// The form changed.
    Edit(RegistrationForm),
    /// Ask what the form's country requires.
    CheckRequirements,
    /// Start a draft filing for the form's country.
    Start,
    /// The end-user details typed for a draft changed.
    EditDetails {
        /// The draft.
        bundle_id: String,
        /// The details, by the field names its regulation lists.
        details: EndUserAttributes,
    },
    /// Put a document on a draft, replacing the one its requirement had.
    Upload(RegistrationDocumentUpload),
    /// Take a document off a draft.
    RemoveDocument {
        /// The draft.
        bundle_id: String,
        /// The document.
        document_id: String,
    },
    /// Ask before filing a draft with the carrier.
    AskSubmit {
        /// The draft.
        bundle_id: String,
    },
}

impl SignedIn {
    /// Reads the filings, on opening the Registrations tab and at a refresh.
    /// A viewer may not file, so nothing is read for one.
    pub(crate) fn enter_registrations_tab(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if !self.capabilities().can_change {
            return Vec::new();
        }
        vec![self.read_registrations(tickets)]
    }

    /// The read of the filings. A list already on screen stays while it is
    /// read again.
    fn read_registrations(&mut self, tickets: &mut Tickets) -> Effect {
        let (slot, call) = self.marketplace.admin.registrations.reading();
        Effect::NumberAdmin {
            ticket: tickets.issue(slot),
            workspace_id: self.workspace_id(),
            call,
        }
    }

    /// One of the Registrations tab's events, for a member who may file: the
    /// call it makes, if any.
    pub(crate) fn registration_event(
        &mut self,
        event: RegistrationEvent,
        tickets: &mut Tickets,
    ) -> Option<(Slot, NumberCall)> {
        let registrations = &mut self.marketplace.admin.registrations;
        match event {
            RegistrationEvent::Reload => Some(registrations.reading()),
            RegistrationEvent::Edit(form) => {
                if registrations.edit(form) {
                    // Its answer would be for a question no longer asked.
                    tickets.cancel(Slot::NumberRequirements);
                }
                None
            }
            RegistrationEvent::CheckRequirements => {
                registrations.form.is_submittable().then(|| {
                    // A lookup on its way is for the country asked before.
                    registrations.requirements = RequirementsState::Loading;
                    (
                        Slot::NumberRequirements,
                        NumberCall::Requirements {
                            country: registrations.form.trimmed_country(),
                            number_type: registrations.form.number_type(),
                            end_user_type: registrations.form.end_user_type(),
                        },
                    )
                })
            }
            RegistrationEvent::Start => registrations.can_start().then(|| {
                registrations.write = NumberWrite::Running;
                registrations.written = Some(RegistrationWrite::Start);
                (
                    Slot::NumberRegistrationWrite,
                    NumberCall::CreateRegistration(registrations.form.draft()),
                )
            }),
            RegistrationEvent::EditDetails { bundle_id, details } => {
                if registrations.is_draft(&bundle_id) {
                    registrations.details.insert(bundle_id, details);
                }
                None
            }
            RegistrationEvent::Upload(upload) => {
                registrations.draft_armed(&upload.bundle_id).then(|| {
                    registrations.write = NumberWrite::Running;
                    registrations.written = Some(RegistrationWrite::Upload {
                        bundle_id: upload.bundle_id.clone(),
                    });
                    (
                        Slot::NumberRegistrationWrite,
                        NumberCall::UploadRegistrationDocument(upload),
                    )
                })
            }
            RegistrationEvent::RemoveDocument {
                bundle_id,
                document_id,
            } => {
                let held = registrations
                    .registration(&bundle_id)
                    .is_some_and(|r| r.documents.iter().any(|d| d.id == document_id));
                (held && registrations.draft_armed(&bundle_id)).then(|| {
                    registrations.write = NumberWrite::Running;
                    registrations.written = Some(RegistrationWrite::Remove {
                        bundle_id: bundle_id.clone(),
                    });
                    (
                        Slot::NumberRegistrationWrite,
                        NumberCall::RemoveRegistrationDocument {
                            bundle_id,
                            document_id,
                        },
                    )
                })
            }
            RegistrationEvent::AskSubmit { bundle_id } => {
                if registrations.can_file(&bundle_id)
                    && let Some(registration) = registrations.registration(&bundle_id)
                {
                    let country = registration.iso_country.clone();
                    self.marketplace.admin.confirmation =
                        Some(NumberConfirmation::SubmitRegistration { bundle_id, country });
                }
                None
            }
        }
    }

    /// A filing was started, a document put on or taken off, or a draft
    /// filed. Each reads the filings again whatever the outcome: a refusal
    /// changes a filing's status and its reasons, and a write whose answer
    /// was lost may still have landed (a draft started, so a second start
    /// would only be refused as a duplicate).
    pub(crate) fn registration_written(
        &mut self,
        result: Result<NumberAnswer, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        let admin = &mut self.marketplace.admin;
        // The answer is to the write on its way: its kind decides, never what
        // else is open on screen.
        let filed = match &admin.registrations.written {
            Some(RegistrationWrite::Submit { bundle_id }) => Some(bundle_id.clone()),
            _ => None,
        };
        let repeat = if filed.is_some() {
            // Its confirmation stayed open to report it, and closes now.
            admin
                .close_confirmation(|c| matches!(c, NumberConfirmation::SubmitRegistration { .. }));
            Repeat::Once
        } else {
            Repeat::Converges
        };
        let registrations = &mut admin.registrations;
        registrations.write = NumberWrite::after(result, repeat, |answer| match answer {
            NumberAnswer::RegistrationCreated(_) => {
                registrations.form = RegistrationForm::default();
                NumberWrite::Idle
            }
            NumberAnswer::RegistrationSubmitted(done) => {
                registrations.details.remove(&done.registration.id);
                registrations
                    .unconfirmed_filing
                    .remove(&done.registration.id);
                NumberWrite::Done(ns::numbers_submit_filed(l()).to_owned())
            }
            _ => NumberWrite::Idle,
        });
        if let Some(bundle_id) = filed
            && registrations.write.unrepeatable_note().is_some()
        {
            registrations.unconfirmed_filing.insert(bundle_id);
        }
        Next::Stay(vec![self.read_registrations(tickets)])
    }
}
