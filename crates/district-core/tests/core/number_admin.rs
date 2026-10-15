//! Managing held numbers and the carrier account: the carrier tab's reads, a
//! number's routing and its release behind a confirmation, the billed lookup,
//! SIP trunks, the one-time passcode sender and the two registrations, and a
//! failed write that may have happened not offered again.

use std::collections::BTreeMap;
use std::time::Duration;

use district_api::{ApiError, ErrorDetail, RetryReason, UnauthorizedReason};
use district_core::{
    A2pForm, CarrierStatus, Effect, Event, MarketplaceEvent, MarketplaceScreen, MarketplaceTab,
    Model, NumberAdmin, NumberAdminEvent, NumberAnswer, NumberCall, NumberConfirmation,
    NumberWrite, OwnedNumbersList, Route, SessionState, SipTrunkForm, SipTrunks, Ticket,
    TollFreeForm, VerifyService, carrier_balance, carrier_number_count, carrier_state,
    number_actions_note, sip_trunk_meta, verify_state,
};
use district_model::{
    A2pRegistrationDraft, A2pRegistrationResponse, NumberLookupInfo, NumberLookupResponse,
    NumberReleaseResponse, OwnedNumbersResponse, ProviderStatusEntry, ProviderStatusResponse,
    SipTrunk, SipTrunkCreateResponse, SipTrunkDraft, SipTrunksResponse, TollFreeVerificationDraft,
    TollFreeVerificationResponse, VerifyServiceResponse,
};

use crate::support::{
    AGENCY, CLIENT, VIEWER, fixture, last_ticket, loaded, server_error, signed_in, signed_out_error,
};

/// `f` with the core writing French, on this thread only.
fn french<T>(f: impl FnOnce() -> T) -> T {
    district_core::l10n::with_locale(district_core::l10n::Locale::FrCa, f)
}

/// A number the fixture's workspace holds on its own carrier account.
const OWN: &str = "+14165550100";
/// A number Distronode holds for it.
const MANAGED: &str = "+14165550199";

fn screen(model: &Model) -> &MarketplaceScreen {
    &signed_in(model).marketplace
}

fn admin(model: &Model) -> &NumberAdmin {
    &screen(model).admin
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

/// On the phone numbers screen as `role`, with the numbers held read.
fn on_numbers(workspace: &str, role: &str) -> Model {
    let (mut model, _) = loaded(workspace, role);
    let effects = model.update(Event::Navigate(Route::Marketplace));
    model.update(Event::OwnedNumbersLoaded {
        ticket: last_ticket(&effects),
        result: Ok(fixture("district-provider-numbers.json")),
    });
    model
}

/// The one call `effects` makes, and its ticket.
fn called(effects: &[Effect]) -> (Ticket, NumberCall) {
    match effects {
        [Effect::NumberAdmin { ticket, call, .. }] => (*ticket, call.clone()),
        other => panic!("{other:?}"),
    }
}

/// The tickets of the carrier tab's three reads, in order.
fn carrier_reads(effects: &[Effect]) -> [Ticket; 3] {
    let calls: Vec<(Ticket, NumberCall)> = effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::NumberAdmin { ticket, call, .. } => Some((*ticket, call.clone())),
            _ => None,
        })
        .collect();
    let [
        (carrier, NumberCall::CarrierStatus),
        (sip, NumberCall::SipTrunks),
        (verify, NumberCall::VerifyService),
    ] = calls.as_slice()
    else {
        panic!("{effects:?}");
    };
    [*carrier, *sip, *verify]
}

fn detail(code: &str) -> ErrorDetail {
    ErrorDetail {
        code: Some(code.to_owned()),
        message: Some("Refused.".to_owned()),
        ..ErrorDetail::default()
    }
}

fn trunk() -> SipTrunk {
    SipTrunk {
        id: "item_1".to_owned(),
        name: "Office".to_owned(),
        domain: "office.sip.twilio.com".to_owned(),
        ip_access_control_list: vec!["203.0.113.7".to_owned()],
        domain_sid: Some("SD1".to_owned()),
        ip_acl_sid: Some("AL1".to_owned()),
        status: "active".to_owned(),
    }
}

fn entry(connected: bool) -> ProviderStatusEntry {
    ProviderStatusEntry {
        connected,
        ..ProviderStatusEntry::default()
    }
}

fn carriers(entries: &[(&str, ProviderStatusEntry)]) -> ProviderStatusResponse {
    ProviderStatusResponse {
        connected: entries.iter().any(|(_, e)| e.connected),
        providers: Some(
            entries
                .iter()
                .map(|(id, e)| ((*id).to_owned(), e.clone()))
                .collect::<BTreeMap<_, _>>(),
        ),
        provider: None,
    }
}

/// On the carrier tab as `role`, its three reads on their way.
fn on_carrier_tab(role: &str) -> (Model, [Ticket; 3]) {
    let workspace = if role == "viewer" { VIEWER } else { AGENCY };
    let mut model = on_numbers(workspace, role);
    let effects = model.update(Event::Marketplace(MarketplaceEvent::SelectTab(
        MarketplaceTab::Carrier,
    )));
    let tickets = carrier_reads(&effects);
    assert_eq!(admin(&model).carrier, CarrierStatus::Loading);
    assert_eq!(admin(&model).sip, SipTrunks::Loading);
    assert_eq!(admin(&model).verify, VerifyService::Loading);
    (model, tickets)
}

#[test]
fn the_carrier_tab_reads_its_three_answers_once_and_at_a_refresh() {
    let (mut model, [carrier, sip, verify]) = on_carrier_tab("agency");
    let status = carriers(&[("twilio", entry(true)), ("telnyx", entry(false))]);
    answer(
        &mut model,
        carrier,
        Ok(NumberAnswer::CarrierStatus(status.clone())),
    );
    answer(
        &mut model,
        sip,
        Ok(NumberAnswer::SipTrunks(SipTrunksResponse {
            success: true,
            trunks: vec![trunk()],
        })),
    );
    answer(
        &mut model,
        verify,
        Ok(NumberAnswer::VerifyService(VerifyServiceResponse {
            success: true,
            enabled: true,
            verify_service_sid: Some("VA1".to_owned()),
        })),
    );
    assert_eq!(admin(&model).carrier, CarrierStatus::Ready(status));
    assert_eq!(admin(&model).sip, SipTrunks::Ready(vec![trunk()]));
    assert_eq!(admin(&model).verify, VerifyService::Ready { enabled: true });

    // Coming back to the tab reads nothing; a refresh reads all of it again.
    for tab in [MarketplaceTab::Owned, MarketplaceTab::Carrier] {
        let effects = model.update(Event::Marketplace(MarketplaceEvent::SelectTab(tab)));
        assert!(effects.is_empty(), "{effects:?}");
    }
    let effects = model.update(Event::Refresh);
    assert!(matches!(effects[0], Effect::LoadOwnedNumbers { .. }));
    carrier_reads(&effects);
}

#[test]
fn no_carrier_is_an_account_state_and_a_failed_read_is_a_failure() {
    for (status, connected) in [
        (
            ProviderStatusResponse {
                connected: false,
                providers: None,
                provider: None,
            },
            false,
        ),
        (carriers(&[]), false),
        (carriers(&[("twilio", entry(true))]), true),
    ] {
        let (mut model, [carrier, ..]) = on_carrier_tab("agency");
        answer(&mut model, carrier, Ok(NumberAnswer::CarrierStatus(status)));
        assert_eq!(
            admin(&model).carrier != CarrierStatus::NotConnected,
            connected
        );
    }
    let (mut model, [carrier, sip, verify]) = on_carrier_tab("agency");
    for ticket in [carrier, sip, verify] {
        answer(&mut model, ticket, Err(server_error()));
    }
    assert!(matches!(admin(&model).carrier, CarrierStatus::Failed(_)));
    assert!(matches!(admin(&model).sip, SipTrunks::Failed(_)));
    assert!(matches!(admin(&model).verify, VerifyService::Failed(_)));
    // An answer of the wrong kind is not taken for the right one.
    let (mut model, [carrier, sip, verify]) = on_carrier_tab("agency");
    for ticket in [carrier, sip, verify] {
        answer(&mut model, ticket, Ok(NumberAnswer::Configured));
    }
    assert_eq!(admin(&model).carrier, CarrierStatus::NotConnected);
    assert!(matches!(admin(&model).sip, SipTrunks::Failed(_)));
    assert!(matches!(admin(&model).verify, VerifyService::Failed(_)));
}

#[test]
fn a_viewer_reads_the_carrier_tab_and_changes_nothing() {
    let (mut model, _) = on_carrier_tab("viewer");
    let events = [
        NumberAdminEvent::Configure(OWN.to_owned()),
        NumberAdminEvent::AskRelease(OWN.to_owned()),
        NumberAdminEvent::EditLookup(OWN.to_owned()),
        NumberAdminEvent::AskLookup,
        NumberAdminEvent::SetVerify(true),
        NumberAdminEvent::CreateSipTrunk,
    ];
    for event in events {
        assert!(numbers(&mut model, event).is_empty());
    }
    assert_eq!(admin(&model).confirmation, None);
    assert_eq!(admin(&model).lookup_number, "");
}

#[test]
fn reapplying_routing_goes_at_once_and_may_always_be_tried_again() {
    let mut model = on_numbers(AGENCY, "agency");
    // Not a number Distronode holds, nor one the workspace does not hold.
    for number in [MANAGED, "+12125550142"] {
        assert!(numbers(&mut model, NumberAdminEvent::Configure(number.to_owned())).is_empty());
    }
    let (ticket, call) = called(&numbers(
        &mut model,
        NumberAdminEvent::Configure(OWN.to_owned()),
    ));
    assert_eq!(call, NumberCall::Configure(OWN.to_owned()));
    assert!(admin(&model).number_write.is_running());
    assert_eq!(admin(&model).number_written.as_deref(), Some(OWN));
    assert!(numbers(&mut model, NumberAdminEvent::Configure(OWN.to_owned())).is_empty());
    answer(&mut model, ticket, Err(server_error()));
    // It converges, so even a server error offers it again.
    assert!(admin(&model).number_write.is_armed());
    assert_eq!(admin(&model).number_write.unrepeatable_note(), None);
    let (ticket, _) = called(&numbers(
        &mut model,
        NumberAdminEvent::Configure(OWN.to_owned()),
    ));
    answer(&mut model, ticket, Ok(NumberAnswer::Configured));
    assert_eq!(
        admin(&model).number_write,
        NumberWrite::Done("Routing reapplied.".to_owned())
    );
    assert_eq!(
        admin(&model).number_write.notice(),
        Some("Routing reapplied.")
    );
}

#[test]
fn nothing_is_done_to_a_number_before_the_list_is_read() {
    let (mut model, _) = loaded(AGENCY, "agency");
    model.update(Event::Navigate(Route::Marketplace));
    assert!(matches!(screen(&model).owned, OwnedNumbersList::Loading));
    assert!(numbers(&mut model, NumberAdminEvent::Configure(OWN.to_owned())).is_empty());
}

/// Opens the release confirmation for [`OWN`] and confirms it.
fn releasing(model: &mut Model) -> Ticket {
    assert!(numbers(model, NumberAdminEvent::AskRelease(MANAGED.to_owned())).is_empty());
    assert_eq!(admin(model).confirmation, None);
    numbers(model, NumberAdminEvent::AskRelease(OWN.to_owned()));
    let confirmation = admin(model).confirmation.clone().expect("asked");
    assert_eq!(
        confirmation,
        NumberConfirmation::Release {
            phone_number: OWN.to_owned()
        }
    );
    assert!(confirmation.is_destructive());
    assert_eq!(confirmation.title(), "Release this number?");
    assert!(confirmation.body().starts_with(OWN));
    assert_eq!(confirmation.action(), "Release it");
    assert_eq!(admin(model).confirm_label(), Some("Release it"));
    let (ticket, call) = called(&numbers(model, NumberAdminEvent::Confirm));
    assert_eq!(call, NumberCall::Release(OWN.to_owned()));
    assert_eq!(admin(model).confirm_label(), Some("Working\u{2026}"));
    // Not twice, and the confirmation cannot be put away while it is on its way.
    assert!(numbers(model, NumberAdminEvent::Confirm).is_empty());
    numbers(model, NumberAdminEvent::CancelConfirmation);
    assert!(admin(model).confirmation.is_some());
    ticket
}

#[test]
fn a_release_is_asked_first_and_reads_the_numbers_again() {
    for (warnings, said) in [
        (vec![], "Released."),
        (
            vec!["The charge could not be ended.".to_owned()],
            "Released, but The charge could not be ended.",
        ),
    ] {
        let mut model = on_numbers(AGENCY, "agency");
        let ticket = releasing(&mut model);
        let effects = answer(
            &mut model,
            ticket,
            Ok(NumberAnswer::Released(NumberReleaseResponse {
                success: true,
                warnings,
            })),
        );
        assert!(matches!(
            effects.as_slice(),
            [Effect::LoadOwnedNumbers { .. }]
        ));
        assert_eq!(admin(&model).confirmation, None);
        assert_eq!(
            admin(&model).number_write,
            NumberWrite::Done(said.to_owned())
        );
        assert_eq!(admin(&model).confirm_label(), None);
    }
}

#[test]
fn a_release_that_may_have_happened_is_not_offered_again() {
    let mut model = on_numbers(AGENCY, "agency");
    let ticket = releasing(&mut model);
    answer(&mut model, ticket, Err(server_error()));
    let write = &admin(&model).number_write;
    assert!(!write.is_armed());
    assert!(write.notice().is_some());
    assert!(
        write
            .unrepeatable_note()
            .unwrap()
            .starts_with("We could not confirm")
    );
    assert_eq!(write.close_label(), "Close");
    numbers(&mut model, NumberAdminEvent::AskRelease(OWN.to_owned()));
    assert_eq!(admin(&model).confirmation, None);
    assert!(numbers(&mut model, NumberAdminEvent::Configure(OWN.to_owned())).is_empty());

    // A refusal proves nothing happened: offered again.
    let mut model = on_numbers(AGENCY, "agency");
    let ticket = releasing(&mut model);
    answer(
        &mut model,
        ticket,
        Err(ApiError::Forbidden(detail("phone_number_not_found"))),
    );
    assert!(admin(&model).number_write.is_armed());
    assert_eq!(admin(&model).number_write.close_label(), "Cancel");
}

#[test]
fn only_a_refusal_before_any_work_proves_a_write_did_not_happen() {
    let refused = [
        ApiError::Unauthorized(UnauthorizedReason::RefusedNotRetried),
        ApiError::NotFound(detail("not_found")),
        ApiError::Conflict(detail("conflict")),
        ApiError::RateLimited {
            retry_after: Some(Duration::from_secs(60)),
            detail: detail("RATE_LIMITED"),
        },
        ApiError::TokenUnavailable(RetryReason::Offline),
        ApiError::Rejected {
            status: 400,
            detail: ErrorDetail::default(),
        },
        ApiError::InvalidRequest("no workspace".to_owned()),
        ApiError::Envelope {
            status: 400,
            code: "invalid_request".to_owned(),
            detail: detail("invalid_request"),
        },
    ];
    let unknown = [
        ApiError::Envelope {
            status: 502,
            code: "upstream_error".to_owned(),
            detail: detail("upstream_error"),
        },
        ApiError::Redirect {
            status: 302,
            location: None,
        },
        ApiError::Unconfirmed {
            endpoint: district_api::Endpoint::NumberRelease,
        },
        ApiError::Decode {
            endpoint: district_api::Endpoint::NumberRelease,
            line: 1,
            column: 1,
        },
    ];
    for (errors, armed) in [(refused.to_vec(), true), (unknown.to_vec(), false)] {
        for error in errors {
            let mut model = on_numbers(AGENCY, "agency");
            let ticket = releasing(&mut model);
            answer(&mut model, ticket, Err(error.clone()));
            assert_eq!(admin(&model).number_write.is_armed(), armed, "{error:?}");
        }
    }
}

#[test]
fn a_lookup_is_asked_first_and_its_answer_kept_apart() {
    let mut model = on_numbers(AGENCY, "agency");
    numbers(&mut model, NumberAdminEvent::AskLookup);
    assert_eq!(admin(&model).confirmation, None, "nothing typed");
    numbers(
        &mut model,
        NumberAdminEvent::EditLookup(" +12125550142 ".to_owned()),
    );
    numbers(&mut model, NumberAdminEvent::AskLookup);
    let confirmation = admin(&model).confirmation.clone().unwrap();
    assert_eq!(confirmation.title(), "Run a paid lookup?");
    assert!(confirmation.body().contains("charges for each lookup"));
    assert_eq!(confirmation.action(), "Run it");
    assert!(!confirmation.is_destructive());
    let (ticket, call) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
    assert_eq!(call, NumberCall::Lookup("+12125550142".to_owned()));
    // Reapplying routing landing meanwhile leaves this confirmation open.
    let (configure, _) = called(&numbers(
        &mut model,
        NumberAdminEvent::Configure(OWN.to_owned()),
    ));
    answer(&mut model, configure, Ok(NumberAnswer::Configured));
    assert!(admin(&model).confirmation.is_some());
    answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::LookedUp(NumberLookupResponse {
            success: true,
            info: NumberLookupInfo {
                valid: true,
                phone_number: "+12125550142".to_owned(),
                country: Some("US".to_owned()),
                line_type: Some(" ".to_owned()),
                carrier: None,
            },
        })),
    );
    assert_eq!(admin(&model).confirmation, None);
    assert_eq!(
        admin(&model).lookup_result,
        Some(vec!["+12125550142".to_owned(), "US".to_owned()])
    );
    assert_eq!(admin(&model).lookup_write, NumberWrite::Idle);

    numbers(&mut model, NumberAdminEvent::AskLookup);
    let (ticket, _) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
    answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::LookedUp(NumberLookupResponse {
            success: true,
            info: NumberLookupInfo {
                valid: false,
                phone_number: "+12125550142".to_owned(),
                country: None,
                line_type: None,
                carrier: None,
            },
        })),
    );
    assert_eq!(
        admin(&model).lookup_result,
        Some(vec![
            "The carrier could not recognise that number.".to_owned()
        ])
    );

    // A failure keeps the last answer, and takes the control away.
    numbers(&mut model, NumberAdminEvent::AskLookup);
    let (ticket, _) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
    answer(&mut model, ticket, Ok(NumberAnswer::Configured));
    assert_eq!(admin(&model).lookup_write, NumberWrite::Idle);
    numbers(&mut model, NumberAdminEvent::AskLookup);
    let (ticket, _) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
    answer(&mut model, ticket, Err(server_error()));
    assert!(admin(&model).lookup_result.is_some());
    numbers(&mut model, NumberAdminEvent::CancelConfirmation);
    numbers(&mut model, NumberAdminEvent::AskLookup);
    assert_eq!(admin(&model).confirmation, None);
}

#[test]
fn a_confirmation_can_be_put_away_and_confirming_nothing_does_nothing() {
    let mut model = on_numbers(AGENCY, "agency");
    assert!(numbers(&mut model, NumberAdminEvent::Confirm).is_empty());
    numbers(&mut model, NumberAdminEvent::CancelConfirmation);
    numbers(&mut model, NumberAdminEvent::AskRelease(OWN.to_owned()));
    numbers(&mut model, NumberAdminEvent::CancelConfirmation);
    assert_eq!(admin(&model).confirmation, None);
}

#[test]
fn a_sip_trunk_is_created_from_a_complete_form_and_read_back() {
    let (mut model, _) = on_carrier_tab("agency");
    assert!(numbers(&mut model, NumberAdminEvent::CreateSipTrunk).is_empty());
    let form = SipTrunkForm {
        name: " Office ".to_owned(),
        domain: " office ".to_owned(),
        ip_addresses: "203.0.113.7\n\n 203.0.113.0/24 \n".to_owned(),
    };
    numbers(&mut model, NumberAdminEvent::EditSipTrunk(form));
    let (ticket, call) = called(&numbers(&mut model, NumberAdminEvent::CreateSipTrunk));
    assert_eq!(
        call,
        NumberCall::CreateSipTrunk(SipTrunkDraft {
            name: "Office".to_owned(),
            domain: "office".to_owned(),
            ip_access_control_list: vec!["203.0.113.7".to_owned(), "203.0.113.0/24".to_owned()],
        })
    );
    assert!(numbers(&mut model, NumberAdminEvent::CreateSipTrunk).is_empty());
    let effects = answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::SipTrunkCreated(SipTrunkCreateResponse {
            success: true,
            trunk: trunk(),
        })),
    );
    assert_eq!(called(&effects).1, NumberCall::SipTrunks);
    assert_eq!(admin(&model).sip_form, SipTrunkForm::default());
    assert_eq!(admin(&model).sip, SipTrunks::Loading);
    assert_eq!(admin(&model).carrier_write, NumberWrite::Idle);

    // A refused create is offered again; the form stays.
    let typed = SipTrunkForm {
        name: "Lab".to_owned(),
        domain: "lab".to_owned(),
        ip_addresses: "203.0.113.9".to_owned(),
    };
    numbers(&mut model, NumberAdminEvent::EditSipTrunk(typed.clone()));
    let (ticket, _) = called(&numbers(&mut model, NumberAdminEvent::CreateSipTrunk));
    answer(
        &mut model,
        ticket,
        Err(ApiError::Rejected {
            status: 400,
            detail: ErrorDetail::default(),
        }),
    );
    assert!(admin(&model).carrier_write.is_armed());
    assert_eq!(admin(&model).sip_form, typed);
}

#[test]
fn the_passcode_sender_is_set_once_and_its_answer_adopted() {
    let (mut model, _) = on_carrier_tab("agency");
    let (ticket, call) = called(&numbers(&mut model, NumberAdminEvent::SetVerify(true)));
    assert_eq!(call, NumberCall::SetVerifyService(true));
    assert!(numbers(&mut model, NumberAdminEvent::SetVerify(false)).is_empty());
    answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::VerifyService(VerifyServiceResponse {
            success: true,
            enabled: true,
            verify_service_sid: Some("VA1".to_owned()),
        })),
    );
    assert_eq!(admin(&model).verify, VerifyService::Ready { enabled: true });
    let (ticket, _) = called(&numbers(&mut model, NumberAdminEvent::SetVerify(false)));
    answer(&mut model, ticket, Err(server_error()));
    assert!(!admin(&model).carrier_write.is_armed());
}

fn a2p_form() -> A2pForm {
    A2pForm {
        business_name: " Acme Roofing ".to_owned(),
        website: " https://example.com ".to_owned(),
        campaign_description: "Appointment reminders".to_owned(),
        sample_one: "Your appointment is tomorrow.".to_owned(),
        sample_two: "Reply STOP to opt out.".to_owned(),
        ..A2pForm::default()
    }
}

#[test]
fn an_a2p_registration_is_asked_first_and_reports_what_the_carrier_said() {
    let (mut model, _) = on_carrier_tab("agency");
    numbers(&mut model, NumberAdminEvent::AskA2p);
    assert_eq!(admin(&model).confirmation, None, "the form is empty");
    numbers(&mut model, NumberAdminEvent::EditA2p(a2p_form()));
    numbers(&mut model, NumberAdminEvent::AskA2p);
    let confirmation = admin(&model).confirmation.clone().unwrap();
    assert_eq!(confirmation, NumberConfirmation::A2p);
    assert_eq!(confirmation.title(), "Submit this registration?");
    assert!(confirmation.body().contains("brand fee"));
    assert_eq!(confirmation.action(), "Submit it");
    let (ticket, call) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
    assert_eq!(
        call,
        NumberCall::RegisterA2p(A2pRegistrationDraft {
            business_name: "Acme Roofing".to_owned(),
            ein: None,
            website: Some("https://example.com".to_owned()),
            vertical: None,
            campaign_type: "CUSTOMER_CARE".to_owned(),
            campaign_description: "Appointment reminders".to_owned(),
            sample_message_1: "Your appointment is tomorrow.".to_owned(),
            sample_message_2: "Reply STOP to opt out.".to_owned(),
        })
    );
    answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::A2pRegistered(A2pRegistrationResponse {
            success: true,
            status: "IN_PROGRESS".to_owned(),
            brand_sid: "BN1".to_owned(),
            messaging_service_sid: "MG1".to_owned(),
            campaign_sid: "CM1".to_owned(),
        })),
    );
    assert_eq!(admin(&model).confirmation, None);
    assert_eq!(
        admin(&model).carrier_write.notice(),
        Some("Submitted. The carrier reports IN_PROGRESS.")
    );
    // The usual first answer: an account step to do first. Offered again.
    numbers(&mut model, NumberAdminEvent::AskA2p);
    let (ticket, _) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
    answer(
        &mut model,
        ticket,
        Err(ApiError::Envelope {
            status: 400,
            code: "TRUST_BUNDLES_NOT_APPROVED".to_owned(),
            detail: detail("TRUST_BUNDLES_NOT_APPROVED"),
        }),
    );
    assert!(admin(&model).carrier_write.is_armed());
}

#[test]
fn a_toll_free_verification_needs_its_opt_in_and_is_asked_first() {
    let (mut model, _) = on_carrier_tab("agency");
    let mut form = TollFreeForm {
        phone_number: " +18005550188 ".to_owned(),
        business_name: "Acme Roofing".to_owned(),
        message_content: "Hello".to_owned(),
        ..TollFreeForm::default()
    };
    numbers(&mut model, NumberAdminEvent::EditTollFree(form.clone()));
    numbers(&mut model, NumberAdminEvent::AskTollFree);
    assert_eq!(admin(&model).confirmation, None, "no opt-in link");
    form.opt_in_image_url = " https://example.com/opt-in.png ".to_owned();
    numbers(&mut model, NumberAdminEvent::EditTollFree(form));
    numbers(&mut model, NumberAdminEvent::AskTollFree);
    let confirmation = admin(&model).confirmation.clone().unwrap();
    assert_eq!(confirmation.title(), "Submit this verification?");
    assert!(confirmation.body().contains("manual review"));
    assert_eq!(confirmation.action(), "Submit it");
    let (ticket, call) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
    assert_eq!(
        call,
        NumberCall::VerifyTollFree(TollFreeVerificationDraft {
            phone_number: "+18005550188".to_owned(),
            business_name: "Acme Roofing".to_owned(),
            website: None,
            use_case: "Customer Support".to_owned(),
            message_content: Some("Hello".to_owned()),
            opt_in_flow: None,
            opt_in_image_urls: vec!["https://example.com/opt-in.png".to_owned()],
        })
    );
    answer(
        &mut model,
        ticket,
        Ok(NumberAnswer::TollFreeFiled(TollFreeVerificationResponse {
            success: true,
            status: "PENDING_REVIEW".to_owned(),
            tfv_sid: "HH1".to_owned(),
        })),
    );
    assert_eq!(
        admin(&model).carrier_write.notice(),
        Some("Submitted. The carrier reports PENDING_REVIEW.")
    );
    // An answer of the wrong kind proves nothing either way.
    numbers(&mut model, NumberAdminEvent::AskTollFree);
    let (ticket, _) = called(&numbers(&mut model, NumberAdminEvent::Confirm));
    answer(&mut model, ticket, Ok(NumberAnswer::Configured));
    assert!(!admin(&model).carrier_write.is_armed());
}

#[test]
fn another_workspace_drops_number_management_and_its_answers() {
    let (mut model, [carrier, ..]) = on_carrier_tab("agency");
    model.update(Event::SelectWorkspace(CLIENT.to_owned()));
    answer(
        &mut model,
        carrier,
        Ok(NumberAnswer::CarrierStatus(carriers(&[(
            "twilio",
            entry(true),
        )]))),
    );
    assert_eq!(admin(&model).carrier, CarrierStatus::NotLoaded);
}

#[test]
fn an_answer_that_says_the_session_ended_signs_out() {
    let (mut model, [carrier, ..]) = on_carrier_tab("agency");
    answer(&mut model, carrier, Err(signed_out_error()));
    assert!(!matches!(model.session(), SessionState::SignedIn(_)));
}

#[test]
fn the_carrier_tab_says_each_thing_in_words() {
    let mut twilio = entry(true);
    assert_eq!(carrier_balance(&twilio), None);
    assert_eq!(carrier_number_count(&twilio), None);
    assert_eq!(carrier_state(&twilio), "Connected");
    twilio.balance = Some("12.50".to_owned());
    assert_eq!(carrier_balance(&twilio).as_deref(), Some("Balance 12.50"));
    twilio.currency = Some(" ".to_owned());
    assert_eq!(carrier_balance(&twilio).as_deref(), Some("Balance 12.50"));
    assert_eq!(
        french(|| carrier_balance(&twilio)).as_deref(),
        Some("Solde\u{a0}: 12,50")
    );
    // With its currency, money as the service writes it (formatIsoAmount).
    twilio.currency = Some("USD".to_owned());
    assert_eq!(carrier_balance(&twilio).as_deref(), Some("Balance $12.50"));
    assert_eq!(
        french(|| carrier_balance(&twilio)).as_deref(),
        Some("Solde\u{a0}: 12,50\u{a0}$\u{a0}US")
    );
    twilio.balance = Some("1500".to_owned());
    twilio.currency = Some("CAD".to_owned());
    assert_eq!(
        carrier_balance(&twilio).as_deref(),
        Some("Balance CA$1,500.00")
    );
    assert_eq!(
        french(|| carrier_balance(&twilio)).as_deref(),
        Some("Solde\u{a0}: 1\u{a0}500,00\u{a0}$\u{a0}CA")
    );
    // A balance that is not a number is shown as the carrier wrote it.
    twilio.balance = Some("n/a".to_owned());
    assert_eq!(carrier_balance(&twilio).as_deref(), Some("Balance n/a CAD"));
    twilio.currency = None;
    assert_eq!(carrier_balance(&twilio).as_deref(), Some("Balance n/a"));
    for (count, said) in [
        (1, "1 number on this account"),
        (3, "3 numbers on this account"),
        (1500, "1500 numbers on this account"),
    ] {
        twilio.number_count = Some(count);
        assert_eq!(carrier_number_count(&twilio).as_deref(), Some(said));
    }
    // A count in a French sentence is grouped, with a no-break space.
    twilio.number_count = Some(1500);
    assert_eq!(
        french(|| carrier_number_count(&twilio)).as_deref(),
        Some("1\u{a0}500 num\u{e9}ros dans ce compte")
    );
    assert!(carrier_state(&entry(false)).starts_with("These credentials were refused."));
    let managed = ProviderStatusEntry {
        managed: Some(true),
        ..entry(true)
    };
    assert!(carrier_state(&managed).starts_with("Held by Distronode."));
    // The status by its label, never as stored.
    assert_eq!(sip_trunk_meta(&trunk()), "Active \u{b7} 1 address");
    assert_eq!(
        french(|| sip_trunk_meta(&trunk())),
        "Actif \u{b7} 1 adresse"
    );
    let suspended = SipTrunk {
        status: "suspended".to_owned(),
        ..trunk()
    };
    assert_eq!(sip_trunk_meta(&suspended), "Other \u{b7} 1 address");
    assert_eq!(
        french(|| sip_trunk_meta(&suspended)),
        "Autre \u{b7} 1 adresse"
    );
    assert_eq!(verify_state(true), "Enabled");
    assert_eq!(verify_state(false), "Not enabled");
    assert_eq!(MarketplaceTab::carrier_label(), "Carrier");

    let held: OwnedNumbersResponse = fixture("district-provider-numbers.json");
    assert_eq!(number_actions_note(&held.numbers[0], true), None);
    assert!(
        number_actions_note(&held.numbers[1], true)
            .unwrap()
            .contains("Distronode")
    );
    assert!(
        number_actions_note(&held.numbers[0], false)
            .unwrap()
            .starts_with("Carrier setup")
    );

    let use_cases = TollFreeForm::use_cases();
    assert_eq!(use_cases[0], ("Customer Support", "Customer Support"));
    assert_eq!(use_cases[3], ("2FA / OTP", "2FA / OTP"));
    assert_eq!(A2pForm::CAMPAIGN_TYPES[1], "LOW_VOLUME");
    assert_eq!(NumberWrite::Running.notice(), None);
    assert!(NumberWrite::Idle.notice().is_none());
}
