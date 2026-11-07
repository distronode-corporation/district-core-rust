//! A number's regulatory registration on the phone numbers screen's
//! Registrations tab: the filings read on opening it and after every write,
//! a viewer offered nothing, what a country requires, a draft started, a
//! document put on and taken off, filing behind a confirmation, and every
//! wire value labelled through the vocabulary in both languages.

use district_api::{ApiError, ErrorDetail};
use district_core::{
    Effect, Event, MarketplaceEvent, MarketplaceTab, Model, NumberAdminEvent, NumberAnswer,
    NumberCall, NumberConfirmation, NumberRegistrations, NumberWrite, RegistrationEvent,
    RegistrationForm, RegistrationList, RequirementsState, Route, Ticket, Tone, document_state,
    end_user_type_label, number_type_label, registration_status, registration_title,
    requirement_fields, requirement_kind,
};
use district_model::{
    CountryRequirements, NumberRegistration, NumberRegistrationCreateResponse,
    NumberRegistrationDraft, NumberRegistrationSubmitResponse, NumberRegistrationsResponse,
    NumberRequirementsResponse, RegistrationDocument, RegistrationDocumentRemovalResponse,
    RegistrationDocumentUpload, RegulatoryRequirement, SubmittedRegistration,
};

use crate::support::{AGENCY, VIEWER, fixture, last_ticket, loaded, server_error, signed_in};

fn french<T>(f: impl FnOnce() -> T) -> T {
    district_core::l10n::with_locale(district_core::l10n::Locale::FrCa, f)
}

fn registrations(model: &Model) -> &NumberRegistrations {
    &signed_in(model).marketplace.admin.registrations
}

fn event(model: &mut Model, event: RegistrationEvent) -> Vec<Effect> {
    model.update(Event::Marketplace(MarketplaceEvent::Numbers(
        NumberAdminEvent::Registration(event),
    )))
}

fn numbers(model: &mut Model, event: NumberAdminEvent) -> Vec<Effect> {
    model.update(Event::Marketplace(MarketplaceEvent::Numbers(event)))
}

fn answer(
    model: &mut Model,
    ticket: Ticket,
    result: Result<NumberAnswer, ApiError>,
) -> Vec<Effect> {
    model.update(Event::NumberAdminDone { ticket, result })
}

/// The one call `effects` makes, and its ticket.
fn called(effects: &[Effect]) -> (Ticket, NumberCall) {
    match effects {
        [Effect::NumberAdmin { ticket, call, .. }] => (*ticket, call.clone()),
        other => panic!("{other:?}"),
    }
}

fn document(id: &str, stored: bool, submitted: bool) -> RegistrationDocument {
    RegistrationDocument {
        id: id.to_owned(),
        requirement_name: "proof_of_address".to_owned(),
        mime_type: "application/pdf".to_owned(),
        size_bytes: 25,
        stored,
        submitted,
        updated_at: "2026-11-07T09:30:00.000Z".to_owned(),
    }
}

fn filing(id: &str, status: &str) -> NumberRegistration {
    NumberRegistration {
        id: id.to_owned(),
        iso_country: "DE".to_owned(),
        number_type: "toll free".to_owned(),
        end_user_type: "business".to_owned(),
        friendly_name: None,
        status: status.to_owned(),
        rejection_reasons: Vec::new(),
        submitted_at: None,
        reviewed_at: None,
        created_at: "2026-11-07T09:00:00.000Z".to_owned(),
        documents: vec![document("doc_1", true, false)],
    }
}

fn listing(filings: Vec<NumberRegistration>) -> NumberAnswer {
    NumberAnswer::Registrations(NumberRegistrationsResponse {
        success: true,
        registrations: filings,
        approved_countries: vec!["FR".to_owned()],
        platform_countries: vec!["US".to_owned()],
    })
}

/// On the phone numbers screen as `role`, the held numbers read.
fn on_numbers(workspace: &str, role: &str) -> Model {
    let (mut model, _) = loaded(workspace, role);
    let effects = model.update(Event::Navigate(Route::Marketplace));
    model.update(Event::OwnedNumbersLoaded {
        ticket: last_ticket(&effects),
        result: Ok(fixture("district-provider-numbers.json")),
    });
    model
}

/// On the Registrations tab as an agency member, with a draft (`bu_draft`)
/// and a filing under review (`bu_review`) read.
fn on_registrations() -> Model {
    let mut model = on_numbers(AGENCY, "agency");
    let effects = model.update(Event::Marketplace(MarketplaceEvent::SelectTab(
        MarketplaceTab::Registrations,
    )));
    let (ticket, call) = called(&effects);
    assert_eq!(call, NumberCall::Registrations);
    assert_eq!(registrations(&model).list, RegistrationList::Loading);
    answer(
        &mut model,
        ticket,
        Ok(listing(vec![
            filing("bu_draft", "draft"),
            filing("bu_review", "in-review"),
        ])),
    );
    model
}

#[test]
fn the_tab_reads_the_filings_once_and_again_at_a_refresh() {
    let mut model = on_registrations();
    let RegistrationList::Ready(list) = &registrations(&model).list else {
        panic!("{:?}", registrations(&model).list);
    };
    assert_eq!(list.registrations.len(), 2);
    assert_eq!(list.approved_countries, ["FR"]);

    // Coming back to the tab reads nothing; a refresh reads the filings
    // again, keeping them on screen meanwhile.
    for tab in [MarketplaceTab::Owned, MarketplaceTab::Registrations] {
        let effects = model.update(Event::Marketplace(MarketplaceEvent::SelectTab(tab)));
        assert!(effects.is_empty(), "{effects:?}");
    }
    let effects = model.update(Event::Refresh);
    assert!(matches!(effects[0], Effect::LoadOwnedNumbers { .. }));
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::NumberAdmin {
                call: NumberCall::Registrations,
                ..
            }
        )),
        "{effects:?}"
    );
    assert!(matches!(
        registrations(&model).list,
        RegistrationList::Ready(_)
    ));

    // The tab's "Try again" reads them too, and a failure is one.
    let (ticket, call) = called(&event(&mut model, RegistrationEvent::Reload));
    assert_eq!(call, NumberCall::Registrations);
    answer(&mut model, ticket, Err(server_error()));
    assert!(matches!(
        registrations(&model).list,
        RegistrationList::Failed(_)
    ));
}

#[test]
fn a_viewer_reads_nothing_and_is_offered_nothing() {
    let mut model = on_numbers(VIEWER, "viewer");
    let effects = model.update(Event::Marketplace(MarketplaceEvent::SelectTab(
        MarketplaceTab::Registrations,
    )));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(registrations(&model).list, RegistrationList::NotLoaded);
    for ev in [
        RegistrationEvent::Reload,
        RegistrationEvent::Edit(RegistrationForm {
            country: "DE".to_owned(),
            ..RegistrationForm::default()
        }),
        RegistrationEvent::CheckRequirements,
        RegistrationEvent::Start,
    ] {
        assert!(event(&mut model, ev).is_empty());
    }
    assert_eq!(registrations(&model).form, RegistrationForm::default());
}

#[test]
fn what_a_country_requires_is_asked_as_typed_and_none_is_an_answer() {
    let mut model = on_registrations();
    // No country, no question.
    assert!(event(&mut model, RegistrationEvent::CheckRequirements).is_empty());
    event(
        &mut model,
        RegistrationEvent::Edit(RegistrationForm {
            country: " de ".to_owned(),
            number_type: "mobile".to_owned(),
            end_user_type: "individual".to_owned(),
            friendly_name: String::new(),
        }),
    );
    let (ticket, call) = called(&event(&mut model, RegistrationEvent::CheckRequirements));
    assert_eq!(
        call,
        NumberCall::Requirements {
            country: "DE".to_owned(),
            number_type: Some("mobile".to_owned()),
            end_user_type: Some("individual".to_owned()),
        }
    );
    assert_eq!(
        registrations(&model).requirements,
        RequirementsState::Loading
    );
    let regulation = CountryRequirements {
        iso_country: "DE".to_owned(),
        number_type: "mobile".to_owned(),
        end_user_type: "individual".to_owned(),
        regulation_sid: "RN1".to_owned(),
        friendly_name: "Germany mobile".to_owned(),
        requirements: Vec::new(),
    };
    answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::Requirements(NumberRequirementsResponse {
            success: true,
            country: "DE".to_owned(),
            number_type: "mobile".to_owned(),
            purchasable: false,
            requirements: Some(regulation.clone()),
        })),
    );
    assert_eq!(
        registrations(&model).requirements,
        RequirementsState::Ready {
            regulation,
            purchasable: false
        }
    );

    // A country with no regulation is "nothing to file", not a failure.
    let (ticket, _) = called(&event(&mut model, RegistrationEvent::CheckRequirements));
    answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::Requirements(NumberRequirementsResponse {
            success: true,
            country: "DE".to_owned(),
            number_type: "mobile".to_owned(),
            purchasable: true,
            requirements: None,
        })),
    );
    assert_eq!(
        registrations(&model).requirements,
        RequirementsState::NoRegulation {
            country: "DE".to_owned(),
            purchasable: true
        }
    );
    assert_eq!(
        RequirementsState::none_heading("DE"),
        "DE · No registration required"
    );

    // And a lookup that failed is a failure, not "nothing to file".
    let (ticket, _) = called(&event(&mut model, RegistrationEvent::CheckRequirements));
    answer(&mut model, ticket, Err(server_error()));
    assert!(matches!(
        registrations(&model).requirements,
        RequirementsState::Failed(_)
    ));
}

#[test]
fn a_draft_is_started_once_and_the_filings_are_read_again() {
    let mut model = on_registrations();
    assert!(!registrations(&model).can_start(), "no country yet");
    assert!(event(&mut model, RegistrationEvent::Start).is_empty());
    event(
        &mut model,
        RegistrationEvent::Edit(RegistrationForm {
            country: "de".to_owned(),
            friendly_name: "  Berlin office ".to_owned(),
            ..RegistrationForm::default()
        }),
    );
    let (ticket, call) = called(&event(&mut model, RegistrationEvent::Start));
    assert_eq!(
        call,
        NumberCall::CreateRegistration(NumberRegistrationDraft {
            iso_country: "DE".to_owned(),
            number_type: Some("local".to_owned()),
            end_user_type: Some("business".to_owned()),
            friendly_name: Some("Berlin office".to_owned()),
        })
    );
    assert!(registrations(&model).write.is_running());
    // A second press while it is on its way sends nothing.
    assert!(event(&mut model, RegistrationEvent::Start).is_empty());

    let (reread, call) = called(&answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::RegistrationCreated(
            NumberRegistrationCreateResponse {
                success: true,
                registration: filing("bu_new", "draft"),
            },
        )),
    ));
    assert_eq!(call, NumberCall::Registrations);
    assert_eq!(registrations(&model).form, RegistrationForm::default());
    assert_eq!(registrations(&model).write, NumberWrite::Idle);
    answer(
        &mut model,
        reread,
        Ok(listing(vec![filing("bu_new", "draft")])),
    );
    assert!(registrations(&model).is_draft("bu_new"));
}

#[test]
fn a_refused_start_keeps_the_form_and_may_be_tried_again() {
    let mut model = on_registrations();
    event(
        &mut model,
        RegistrationEvent::Edit(RegistrationForm {
            country: "DE".to_owned(),
            ..RegistrationForm::default()
        }),
    );
    let (ticket, _) = called(&event(&mut model, RegistrationEvent::Start));
    let effects = answer(
        &mut model,
        ticket,
        Err(ApiError::Conflict(ErrorDetail {
            message: Some("This workspace already has a registration.".to_owned()),
            ..ErrorDetail::default()
        })),
    );
    assert!(effects.is_empty(), "nothing changed, nothing to read");
    assert_eq!(registrations(&model).form.country, "DE");
    assert!(registrations(&model).write.is_armed());
    assert_eq!(
        registrations(&model).write.notice(),
        Some("This workspace already has a registration.")
    );
}

#[test]
fn documents_go_on_and_off_a_draft_only() {
    let mut model = on_registrations();
    let upload = |bundle: &str| RegistrationDocumentUpload {
        bundle_id: bundle.to_owned(),
        requirement_name: "proof_of_address".to_owned(),
        file_name: "address.pdf".to_owned(),
        mime_type: "application/pdf".to_owned(),
        bytes: b"%PDF".to_vec(),
    };
    // A filing under review, or one this list does not hold, takes nothing.
    for bundle in ["bu_review", "bu_unknown"] {
        assert!(event(&mut model, RegistrationEvent::Upload(upload(bundle))).is_empty());
        assert!(
            event(
                &mut model,
                RegistrationEvent::RemoveDocument {
                    bundle_id: bundle.to_owned(),
                    document_id: "doc_1".to_owned(),
                }
            )
            .is_empty()
        );
    }
    // Nor does a document the draft does not hold.
    assert!(
        event(
            &mut model,
            RegistrationEvent::RemoveDocument {
                bundle_id: "bu_draft".to_owned(),
                document_id: "doc_9".to_owned(),
            }
        )
        .is_empty()
    );

    let (ticket, call) = called(&event(
        &mut model,
        RegistrationEvent::Upload(upload("bu_draft")),
    ));
    assert_eq!(
        call,
        NumberCall::UploadRegistrationDocument(upload("bu_draft"))
    );
    assert_eq!(registrations(&model).written.as_deref(), Some("bu_draft"));
    assert!(
        !registrations(&model).draft_armed("bu_draft"),
        "one at a time"
    );
    // A failed upload is offered again: it replaces, so it converges.
    assert!(answer(&mut model, ticket, Err(server_error())).is_empty());
    assert!(registrations(&model).draft_armed("bu_draft"));

    let (ticket, call) = called(&event(
        &mut model,
        RegistrationEvent::RemoveDocument {
            bundle_id: "bu_draft".to_owned(),
            document_id: "doc_1".to_owned(),
        },
    ));
    assert_eq!(
        call,
        NumberCall::RemoveRegistrationDocument {
            bundle_id: "bu_draft".to_owned(),
            document_id: "doc_1".to_owned(),
        }
    );
    let (_, reread) = called(&answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::RegistrationDocumentRemoved(
            RegistrationDocumentRemovalResponse {
                success: true,
                document_id: "doc_1".to_owned(),
            },
        )),
    ));
    assert_eq!(reread, NumberCall::Registrations);
}

#[test]
fn a_draft_is_filed_behind_a_confirmation_with_its_details() {
    let mut model = on_registrations();
    // Only a draft is asked about.
    event(
        &mut model,
        RegistrationEvent::AskSubmit {
            bundle_id: "bu_review".to_owned(),
        },
    );
    assert_eq!(signed_in(&model).marketplace.admin.confirmation, None);

    let details: district_model::EndUserAttributes =
        [("business_name".to_owned(), "Acme".to_owned())].into();
    event(
        &mut model,
        RegistrationEvent::EditDetails {
            bundle_id: "bu_draft".to_owned(),
            details: details.clone(),
        },
    );
    event(
        &mut model,
        RegistrationEvent::AskSubmit {
            bundle_id: "bu_draft".to_owned(),
        },
    );
    let confirmation = signed_in(&model)
        .marketplace
        .admin
        .confirmation
        .clone()
        .expect("asked");
    assert_eq!(
        confirmation,
        NumberConfirmation::SubmitRegistration {
            bundle_id: "bu_draft".to_owned(),
            country: "DE".to_owned(),
        }
    );
    assert_eq!(confirmation.title(), "File this registration?");
    assert!(
        confirmation
            .body()
            .contains("for DE in your business's name")
    );
    assert_eq!(confirmation.action(), "File it");
    assert!(!confirmation.is_destructive());

    let (ticket, call) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
    assert_eq!(
        call,
        NumberCall::SubmitRegistration {
            bundle_id: "bu_draft".to_owned(),
            end_user_attributes: details,
        }
    );
    assert_eq!(
        signed_in(&model).marketplace.admin.confirm_label(),
        Some("Working…")
    );
    // A second press of the confirmation's button files nothing more.
    assert!(numbers(&mut model, NumberAdminEvent::Confirm).is_empty());

    let (_, reread) = called(&answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::RegistrationSubmitted(
            NumberRegistrationSubmitResponse {
                success: true,
                registration: SubmittedRegistration {
                    id: "bu_draft".to_owned(),
                    status: "pending-review".to_owned(),
                    submitted_at: "2026-11-07T10:00:00.000Z".to_owned(),
                },
            },
        )),
    ));
    assert_eq!(reread, NumberCall::Registrations);
    assert_eq!(signed_in(&model).marketplace.admin.confirmation, None);
    assert_eq!(
        registrations(&model).write.notice(),
        Some("Filed. The carrier is reviewing it.")
    );
    assert!(registrations(&model).details.is_empty(), "sent, so dropped");
}

#[test]
fn a_refused_filing_reads_the_list_again_and_only_a_refusal_is_offered_again() {
    for (error, repeatable) in [
        (
            ApiError::Envelope {
                status: 422,
                code: "carrier_rejected".to_owned(),
                detail: ErrorDetail {
                    code: Some("carrier_rejected".to_owned()),
                    message: Some("The carrier refused this registration.".to_owned()),
                    ..ErrorDetail::default()
                },
            },
            true,
        ),
        (server_error(), false),
    ] {
        let mut model = on_registrations();
        event(
            &mut model,
            RegistrationEvent::AskSubmit {
                bundle_id: "bu_draft".to_owned(),
            },
        );
        let (ticket, _) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
        // Read again either way: a refusal changes the filing's status and
        // reasons, and a failure may have filed it.
        let (_, reread) = called(&answer(&mut model, ticket, Err(error.clone())));
        assert_eq!(reread, NumberCall::Registrations, "{error:?}");
        assert_eq!(signed_in(&model).marketplace.admin.confirmation, None);
        assert_eq!(
            registrations(&model).write.is_armed(),
            repeatable,
            "{error:?}"
        );
    }
}

#[test]
fn every_wire_value_reads_through_the_vocabulary() {
    let mut rejected = filing("bu_1", "twilio-rejected");
    assert_eq!(registration_title(&rejected), "DE · Toll-free");
    assert_eq!(registration_status("draft"), "Draft");
    assert_eq!(registration_status("pending-review"), "Pending review");
    assert_eq!(registration_status("twilio-rejected"), "Rejected");
    // A status the carrier added since is never shown as sent.
    assert_eq!(registration_status("twilio-withdrawn"), "Other");
    assert_eq!(Tone::of_registration("twilio-approved"), Tone::Success);
    assert_eq!(Tone::of_registration("twilio-rejected"), Tone::Danger);
    assert_eq!(Tone::of_registration("in-review"), Tone::Neutral);
    assert_eq!(end_user_type_label("individual"), "Individual");
    assert_eq!(number_type_label("national"), "National");

    let detail = RegulatoryRequirement {
        kind: "end_user".to_owned(),
        name: "Business".to_owned(),
        requirement_name: "business_info".to_owned(),
        requirement_type: "business".to_owned(),
        fields: vec!["business_name".to_owned(), "website".to_owned()],
        accepted_documents: None,
    };
    let paper = RegulatoryRequirement {
        kind: "supporting_document".to_owned(),
        fields: Vec::new(),
        ..detail.clone()
    };
    assert_eq!(requirement_kind(&detail), "Your details");
    assert_eq!(requirement_kind(&paper), "Documents");
    assert_eq!(requirement_fields(&detail), "business_name, website");
    assert_eq!(requirement_fields(&paper), "No fields");
    assert_eq!(
        document_state(&document("d", true, true)),
        "Filed with the carrier"
    );
    assert_eq!(
        document_state(&document("d", true, false)),
        "Stored, not yet filed"
    );
    assert_eq!(
        document_state(&document("d", false, false)),
        "Recorded, no file yet"
    );
    assert_eq!(
        RegistrationForm::number_types(),
        [
            ("local", "Local".to_owned()),
            ("mobile", "Mobile".to_owned()),
            ("national", "National".to_owned()),
            ("toll free", "Toll-free".to_owned()),
        ]
    );
    assert_eq!(MarketplaceTab::registrations_label(), "Registrations");

    french(|| {
        rejected.number_type = "local".to_owned();
        assert_eq!(registration_title(&rejected), "DE · Local");
        assert_eq!(registration_status("draft"), "Brouillon");
        assert_eq!(registration_status("in-review"), "En cours d'examen");
        assert_eq!(registration_status("twilio-approved"), "Approuvée");
        assert_eq!(registration_status("twilio-withdrawn"), "Autre");
        assert_eq!(number_type_label("toll free"), "Sans frais");
        assert_eq!(end_user_type_label("business"), "Entreprise");
        assert_eq!(requirement_kind(&detail), "Vos renseignements");
        assert_eq!(requirement_fields(&paper), "Aucun champ");
        assert_eq!(
            document_state(&document("d", false, false)),
            "Consigné, aucun fichier pour l'instant"
        );
        assert_eq!(
            RequirementsState::none_heading("DE"),
            "DE · aucune inscription requise"
        );
        assert_eq!(MarketplaceTab::registrations_label(), "Inscriptions");
        assert_eq!(
            RegistrationForm::end_user_types(),
            [
                ("business", "Entreprise".to_owned()),
                ("individual", "Particulier".to_owned()),
            ]
        );
    });
}

#[test]
fn the_confirmation_reads_in_french() {
    let mut model = on_registrations();
    event(
        &mut model,
        RegistrationEvent::AskSubmit {
            bundle_id: "bu_draft".to_owned(),
        },
    );
    let confirmation = signed_in(&model)
        .marketplace
        .admin
        .confirmation
        .clone()
        .expect("asked");
    french(|| {
        assert_eq!(confirmation.title(), "Déposer cette inscription?");
        assert!(
            confirmation.body().contains("(pays\u{a0}: DE)"),
            "{}",
            confirmation.body()
        );
        assert_eq!(confirmation.action(), "Déposer");
    });
}

#[test]
fn nothing_is_done_to_a_filing_before_the_list_is_read() {
    let mut model = on_numbers(AGENCY, "agency");
    model.update(Event::Marketplace(MarketplaceEvent::SelectTab(
        MarketplaceTab::Registrations,
    )));
    assert_eq!(registrations(&model).registration("bu_draft"), None);
    event(
        &mut model,
        RegistrationEvent::AskSubmit {
            bundle_id: "bu_draft".to_owned(),
        },
    );
    assert_eq!(signed_in(&model).marketplace.admin.confirmation, None);
}

#[test]
fn a_filing_that_left_draft_while_asked_is_not_filed() {
    let mut model = on_registrations();
    event(
        &mut model,
        RegistrationEvent::AskSubmit {
            bundle_id: "bu_draft".to_owned(),
        },
    );
    // Filed elsewhere meanwhile: the list read again shows it under review.
    let (ticket, _) = called(&event(&mut model, RegistrationEvent::Reload));
    answer(
        &mut model,
        ticket,
        Ok(listing(vec![filing("bu_draft", "pending-review")])),
    );
    assert!(numbers(&mut model, NumberAdminEvent::Confirm).is_empty());
    assert_eq!(registrations(&model).write, NumberWrite::Idle);
}

#[test]
fn an_answer_of_another_kind_is_an_unreadable_one() {
    let mut model = on_registrations();
    let (ticket, _) = called(&event(&mut model, RegistrationEvent::Reload));
    answer(&mut model, ticket, Ok(NumberAnswer::Configured));
    assert!(matches!(
        registrations(&model).list,
        RegistrationList::Failed(ref failure) if !failure.retryable
    ));

    event(
        &mut model,
        RegistrationEvent::Edit(RegistrationForm {
            country: "DE".to_owned(),
            ..RegistrationForm::default()
        }),
    );
    let (ticket, _) = called(&event(&mut model, RegistrationEvent::CheckRequirements));
    answer(&mut model, ticket, Ok(NumberAnswer::Configured));
    assert!(matches!(
        registrations(&model).requirements,
        RequirementsState::Failed(ref failure) if !failure.retryable
    ));
}
