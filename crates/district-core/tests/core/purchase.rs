//! Buying in the app: off in every app but one, and byte for byte the read-only
//! billing of 2.0.0 there; in the app that buys, a plan confirmed on a step
//! that names Stripe, a hand-off bound to the private view inside the app that
//! shows checkout, the browser when the app cannot show one, and the
//! per-device setting that hides it all.

use district_api::{ApiError, ErrorDetail, ReauthReason, TransportError, TransportKind};
use district_auth::is_valid_hand_off_state;
use district_core::{
    BillingDestination, BillingEvent, CHECKOUT_PATH, Effect, EmbeddedView, Event,
    HAND_OFF_CALLBACK_WAIT, HandOffLeg, Model, OneTimeUrl, PlanChoice, PlanTerm, PlanTier,
    PromoCode, PurchaseSetting, PurchaseState, PurchaseSurface, Route, SchedulingEvent,
    SessionState, Ticket, WorkspacesState,
};
use district_model::{BillingHandOffResponse, CODE_INVALID_NEXT};

use crate::support::{
    AGENCY, VIEWER, claims, config, desktop_fixture, fixture, last_ticket, overview, pick,
    signed_in, signed_out_error, with_purchases, workspace_list,
};

/// A nonce of the shape the service sends, which must never reach a log.
const NONCE: &str = "n0nce-n0nce_n0nce-n0nce_n0nce-n0nce_n0nce-n";

/// The hand-off fixture's link and code.
const LINK: &str = "https://www.distronode.com/dashboard/handoff?code=contract-handoff-code&next=%2Fdashboard%2Fdistrict%2Fscheduling";
const CODE: &str = "contract-handoff-code";

fn purchase(model: &Model) -> &PurchaseState {
    &signed_in(model).purchase
}

fn act(model: &mut Model, event: BillingEvent) -> Vec<Effect> {
    model.update(Event::Billing(event))
}

fn pro_annual() -> PlanChoice {
    PlanChoice {
        tier: PlanTier::VoicePro,
        term: PlanTerm::Annual,
        promo: PromoCode::parse("save-100"),
    }
}

/// Signed in, the purchases setting answered with `setting`, the workspace
/// list read, and `workspace` open as `role` (or, with `None`, the account
/// found to have no workspace).
fn signed_in_with(setting: Option<PurchaseSetting>, open: Option<(&str, &str)>) -> Model {
    let (mut model, effects) = with_purchases(|| Model::new(config()));
    let effects = model.update(Event::SessionRestored {
        ticket: last_ticket(&effects),
        result: Ok(claims()),
    });
    let read = pick(&effects, |e| {
        matches!(e, Effect::ReadPurchaseSetting { .. })
    });
    let list = pick(&effects, |e| matches!(e, Effect::LoadWorkspaces { .. }));
    assert_eq!(purchase(&model).setting, None, "not read yet");
    assert!(!signed_in(&model).offers_plans(), "nothing listed yet");
    model.update(Event::PurchaseSettingRead {
        ticket: read,
        setting,
    });
    assert!(
        !signed_in(&model).offers_plans(),
        "no plans before the workspaces are known"
    );
    let workspace = open.map_or(AGENCY, |(workspace, _)| workspace);
    let effects = model.update(Event::WorkspacesLoaded {
        ticket: list,
        remembered: Some(workspace.to_owned()),
        result: Ok(workspace_list()),
    });
    let result = match open {
        Some((workspace, role)) => Ok(overview(workspace, role)),
        None => Err(ApiError::NotFound(ErrorDetail::default())),
    };
    model.update(Event::OverviewLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadOverview { .. })),
        result,
    });
    model
}

/// An agency member of the open workspace, with purchases on.
fn agency() -> Model {
    signed_in_with(None, Some((AGENCY, "agency")))
}

/// The `state` of the start page awaited.
fn awaited_state(model: &Model) -> String {
    match &purchase(model).leg {
        HandOffLeg::Browser(state) => state.as_str().to_owned(),
        other => panic!("no start page awaited: {other:?}"),
    }
}

/// The start page's answer for `state`.
fn answer(state: &str) -> Event {
    Event::HandOffCallback(OneTimeUrl::new(format!(
        "districtai://handoff?state={state}&nonce={NONCE}"
    )))
}

/// Opens the start page for `destination`'s action, checks it opened inside
/// the app in a new private view, and returns the wait's ticket.
fn press(model: &mut Model, event: BillingEvent) -> Ticket {
    let effects = act(model, event);
    let [
        Effect::OpenEmbedded { url, view },
        Effect::Wait { ticket, delay },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    let state = awaited_state(model);
    assert!(is_valid_hand_off_state(&state));
    assert_eq!(
        url.expose(),
        format!("https://www.distronode.com/dashboard/handoff/start?state={state}")
    );
    assert_eq!(*view, EmbeddedView::NewPrivate);
    assert_eq!(*delay, HAND_OFF_CALLBACK_WAIT);
    assert_eq!(purchase(model).surface, Some(PurchaseSurface::InApp));
    assert!(purchase(model).opening() && !purchase(model).minting());
    *ticket
}

/// Chooses Voice Pro, annual, with a code, and confirms it.
fn checkout(model: &mut Model) -> Ticket {
    assert!(act(model, BillingEvent::ChoosePlan(pro_annual())).is_empty());
    assert_eq!(purchase(model).confirming, Some(pro_annual()));
    if !purchase(model).in_browser {
        assert_eq!(purchase(model).confirm_body(), PurchaseState::CONFIRM_BODY);
    }
    let wait = press(model, BillingEvent::ConfirmPurchase);
    assert_eq!(purchase(model).confirming, None);
    wait
}

/// The view answers: the link is asked for, bound. Its ticket.
fn answered(model: &mut Model) -> Ticket {
    let state = awaited_state(model);
    let effects = model.update(answer(&state));
    let [Effect::RequestBillingHandOff { ticket, nonce, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(nonce.as_ref().map(|n| n.as_str()), Some(NONCE));
    assert!(purchase(model).minting());
    *ticket
}

fn hand_off() -> BillingHandOffResponse {
    desktop_fixture("district-scheduling-handoff.json")
}

fn refused_with(code: &str) -> ApiError {
    ApiError::Envelope {
        status: 400,
        code: code.to_owned(),
        detail: ErrorDetail {
            message: Some("the service's own words".to_owned()),
            code: Some(code.to_owned()),
            degraded_regions: Vec::new(),
        },
    }
}

/// The parts of a `next` the service's `billingHandoffNext` admits, checked
/// the way it checks them (lib/auth/billing-handoff.ts): exactly
/// `/dashboard/district/billing`, or `/checkout` whose query holds only
/// `tier` (a published key), `term`, `promo` (already canonical) and `reason`
/// (`no-workspace`), each at most once, with nothing that parsing would
/// change. A `next` built here passes this, or the service would refuse it.
fn server_admits(next: &str) -> bool {
    const PUBLISHED_TIERS: [&str; 5] = [
        "Dgi",
        "VoiceSolo",
        "VoiceStarter",
        "VoicePro",
        "VoiceStudio",
    ];
    if next
        .chars()
        .any(|c| c.is_control() || c == '\\' || c == '#')
        || next.contains("://")
    {
        return false;
    }
    if next == "/dashboard/district/billing" {
        return true;
    }
    let Some(query) = next.strip_prefix("/checkout?") else {
        return next == "/checkout";
    };
    let mut seen = Vec::new();
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            return false;
        };
        // A canonical query needs no escaping: anything escaped would come
        // back from the server's rebuild in another spelling.
        if seen.contains(&key) || value.contains('%') || value.contains('+') {
            return false;
        }
        let admitted = match key {
            "tier" => PUBLISHED_TIERS.contains(&value),
            "term" => ["monthly", "annual"].contains(&value),
            "promo" => {
                (1..=40).contains(&value.len())
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b"-_".contains(&b))
            }
            "reason" => value == "no-workspace",
            _ => false,
        };
        if !admitted {
            return false;
        }
        seen.push(key);
    }
    true
}

#[test]
fn every_next_the_core_builds_is_one_the_service_admits() {
    let promos = [
        None,
        PromoCode::parse("SAVE100"),
        PromoCode::parse("a"),
        PromoCode::parse(&"Z".repeat(PromoCode::MAX_LEN)),
    ];
    let mut built = Vec::new();
    for tier in PlanTier::ALL {
        for term in PlanTerm::ALL {
            for promo in &promos {
                for has_workspace in [true, false] {
                    let choice = PlanChoice {
                        tier,
                        term,
                        promo: promo.clone(),
                    };
                    let next = BillingDestination::Checkout(choice).next(has_workspace);
                    assert!(server_admits(&next), "{next}");
                    assert!(next.starts_with(CHECKOUT_PATH), "{next}");
                    assert_eq!(next.contains("reason=no-workspace"), !has_workspace);
                    built.push(next);
                }
            }
        }
    }
    assert_eq!(built.len(), 4 * 2 * 4 * 2);
    for has_workspace in [true, false] {
        assert_eq!(
            BillingDestination::BillingPage.next(has_workspace),
            "/dashboard/district/billing"
        );
    }
    assert_eq!(
        BillingDestination::Checkout(pro_annual()).next(true),
        "/checkout?tier=VoicePro&term=annual&promo=SAVE-100"
    );
    assert_eq!(
        BillingDestination::Checkout(PlanChoice {
            tier: PlanTier::VoiceSolo,
            term: PlanTerm::default(),
            promo: None,
        })
        .next(false),
        "/checkout?tier=VoiceSolo&term=monthly&reason=no-workspace"
    );
    assert_eq!(
        BillingDestination::default(),
        BillingDestination::BillingPage
    );

    // The check above refuses what the service refuses, so passing it means
    // something.
    for refused in [
        "/checkout?tier=VoicePro&tier=VoicePro",
        "/checkout?tier=voicepro",
        "/checkout?tier=VoicePro&utm=x",
        "/checkout?promo=save",
        "/checkout?promo=SAVE%20100",
        "/checkout?reason=other",
        "/checkout?tier",
        "/checkout#promo",
        "/dashboard/district/billing?x=1",
        "/dashboard",
        "https://www.distronode.com/checkout",
        "/checkout?promo=A\tB",
        "/checkout?promo=A\\B",
    ] {
        assert!(!server_admits(refused), "{refused}");
    }
}

#[test]
fn a_promotion_code_is_taken_only_in_the_shape_the_service_reads() {
    for (typed, code) in [
        ("SAVE100", "SAVE100"),
        ("  save-100_x\t", "SAVE-100_X"),
        ("a", "A"),
    ] {
        assert_eq!(PromoCode::parse(typed).unwrap().as_str(), code, "{typed:?}");
    }
    let longest = "9".repeat(PromoCode::MAX_LEN);
    assert_eq!(PromoCode::parse(&longest).unwrap().as_str(), longest);
    // Refused whole, never cleaned into a code nobody typed.
    for hostile in [
        "",
        "   ",
        &"9".repeat(PromoCode::MAX_LEN + 1),
        "save 100",
        "SAVE&tier=Dgi",
        "SAVE#1",
        "SAVE=1",
        "SAVE%41",
        "SAVE+1",
        "../x",
        "SAVE\u{0}",
        "\u{17f}ALE",
        "\u{131}NFO",
        "caf\u{e9}",
        "SAVE\u{2014}1",
    ] {
        assert_eq!(PromoCode::parse(hostile), None, "{hostile:?}");
    }
    assert!(PromoCode::INVALID.contains("40"));
}

#[test]
fn the_plans_and_terms_name_the_published_keys() {
    let keys: Vec<_> = PlanTier::ALL.iter().map(|tier| tier.key()).collect();
    assert_eq!(
        keys,
        ["VoiceSolo", "VoiceStarter", "VoicePro", "VoiceStudio"]
    );
    let labels: Vec<_> = PlanTier::ALL.iter().map(|tier| tier.label()).collect();
    assert_eq!(
        labels,
        ["Voice Solo", "Voice Starter", "Voice Pro", "Voice Studio"]
    );
    let terms: Vec<_> = PlanTerm::ALL
        .iter()
        .map(|term| (term.key(), term.label()))
        .collect();
    assert_eq!(terms, [("monthly", "Monthly"), ("annual", "Annual")]);
    assert_eq!(PlanTerm::default(), PlanTerm::Monthly);
}

#[test]
fn the_setting_has_two_values_and_sign_in_every_time_is_the_default() {
    assert_eq!(PurchaseSetting::default(), PurchaseSetting::SignInEveryTime);
    let values: Vec<_> = PurchaseSetting::ALL
        .iter()
        .map(|setting| (setting.label(), setting.key()))
        .collect();
    assert_eq!(
        values,
        [("Sign in every time", "sign_in_every_time"), ("Off", "off")]
    );
    for setting in PurchaseSetting::ALL {
        assert_eq!(PurchaseSetting::from_key(setting.key()), Some(setting));
    }
    for unknown in ["", "on", "OFF", "ask_every_time"] {
        assert_eq!(PurchaseSetting::from_key(unknown), None, "{unknown:?}");
    }
    assert_eq!(PurchaseSetting::LABEL, "Purchases on this computer");
    assert!(PurchaseSetting::BODY.contains("Sign in every time"));
}

/// The copy names Stripe before checkout opens, and names no price.
#[test]
fn the_confirmation_names_stripe_and_no_price() {
    for text in [
        PurchaseState::CONFIRM_TITLE,
        PurchaseState::CONFIRM_BODY,
        PurchaseState::CONFIRM_BODY_BROWSER,
        PurchaseState::IN_APP_NOTE,
    ] {
        assert!(text.contains("Stripe"), "{text}");
    }
    for text in [
        PurchaseState::CHOOSE_PLAN_ACTION,
        PurchaseState::MANAGE_ACTION,
        PurchaseState::PRICES_NOTE,
        PurchaseState::IN_APP_NOTE,
        PurchaseState::CONFIRM_TITLE,
        PurchaseState::CONFIRM_BODY,
        PurchaseState::CONFIRM_BODY_BROWSER,
        PurchaseState::CONFIRM_ACTION,
        PurchaseState::CANCEL_ACTION,
        PurchaseState::OPENING,
        PurchaseState::IN_BROWSER,
        PurchaseSetting::BODY,
        PromoCode::LABEL,
        PromoCode::INVALID,
    ] {
        assert!(!text.contains('$') && !text.contains('\u{20ac}'), "{text}");
        assert!(
            !text.contains('\u{2014}') && !text.contains('\u{2013}'),
            "{text}"
        );
    }
}

/// Every purchase event the app could send, in order, as a member who could
/// buy would send them.
fn purchase_events(state: &str) -> Vec<Event> {
    vec![
        Event::SetPurchases(PurchaseSetting::SignInEveryTime),
        Event::Billing(BillingEvent::ChoosePlan(pro_annual())),
        Event::Billing(BillingEvent::ConfirmPurchase),
        Event::Billing(BillingEvent::CancelPurchase),
        Event::Billing(BillingEvent::ManageInApp),
        answer(state),
        Event::EmbeddedUnavailable,
        Event::EmbeddedClosed,
        Event::Billing(BillingEvent::DismissPurchaseNotice),
        Event::SetPurchases(PurchaseSetting::Off),
    ]
}

/// The Linux app's build: nothing reads the setting, every purchase event
/// changes nothing and asks for nothing, and the state after them is exactly
/// the state without them.
#[test]
fn an_app_that_does_not_buy_in_the_app_is_exactly_as_before() {
    let (mut model, effects) = Model::new(config());
    assert!(!model.config().in_app_purchases);
    let effects = model.update(Event::SessionRestored {
        ticket: last_ticket(&effects),
        result: Ok(claims()),
    });
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::ReadPurchaseSetting { .. })),
        "{effects:?}"
    );
    let effects = model.update(Event::WorkspacesLoaded {
        ticket: last_ticket(&effects),
        remembered: Some(AGENCY.to_owned()),
        result: Ok(workspace_list()),
    });
    model.update(Event::OverviewLoaded {
        ticket: last_ticket(&effects),
        result: Ok(overview(AGENCY, "agency")),
    });
    let effects = model.update(Event::Navigate(Route::Billing));
    assert_eq!(effects.len(), 2, "{effects:?}");

    let before = signed_in(&model).clone();
    for event in purchase_events("any-state-at-all-0123456789") {
        assert_eq!(model.update(event.clone()), Vec::new(), "{event:?}");
    }
    assert_eq!(signed_in(&model), &before);
    assert_eq!(before.purchase, PurchaseState::default());
    assert!(!before.offers_plans() && !before.offers_manage_in_app());

    // The read-only screen's own actions are what they were.
    let effects = act(&mut model, BillingEvent::ManageOnWeb);
    assert_eq!(
        effects,
        [Effect::OpenUrl {
            url: "https://www.distronode.com/dashboard/district/billing".to_owned()
        }]
    );
}

#[test]
fn an_app_that_buys_reads_the_setting_and_offers_plans_to_a_role_that_can_change_them() {
    let model = agency();
    assert_eq!(
        purchase(&model).setting,
        Some(PurchaseSetting::SignInEveryTime),
        "never set reads as the default"
    );
    assert!(purchase(&model).enabled());
    assert!(signed_in(&model).offers_plans());
    assert!(signed_in(&model).offers_manage_in_app());

    let mut viewer = signed_in_with(None, Some((VIEWER, "viewer")));
    assert!(!signed_in(&viewer).offers_plans());
    assert!(!signed_in(&viewer).offers_manage_in_app());
    let before = signed_in(&viewer).clone();
    assert!(act(&mut viewer, BillingEvent::ChoosePlan(pro_annual())).is_empty());
    assert!(act(&mut viewer, BillingEvent::ManageInApp).is_empty());
    assert_eq!(signed_in(&viewer), &before);

    let off = signed_in_with(Some(PurchaseSetting::Off), Some((AGENCY, "agency")));
    assert_eq!(purchase(&off).setting, Some(PurchaseSetting::Off));
    assert!(!signed_in(&off).offers_plans() && !signed_in(&off).offers_manage_in_app());
}

/// An account with no workspace yet may buy: checkout is what makes one, and
/// it is told so. There is nothing to manage.
#[test]
fn an_account_with_no_workspace_buys_and_checkout_is_told_why() {
    let mut model = signed_in_with(None, None);
    assert_eq!(signed_in(&model).workspaces, WorkspacesState::NoWorkspaces);
    assert!(signed_in(&model).offers_plans());
    assert!(!signed_in(&model).offers_manage_in_app());
    assert!(act(&mut model, BillingEvent::ManageInApp).is_empty());

    checkout(&mut model);
    let state = awaited_state(&model);
    let effects = model.update(answer(&state));
    let [
        Effect::RequestBillingHandOff {
            workspace_id,
            destination,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(*workspace_id, None);
    assert_eq!(
        destination.next(workspace_id.is_some()),
        "/checkout?tier=VoicePro&term=annual&promo=SAVE-100&reason=no-workspace"
    );
}

/// An account with no workspace that is offered the plans is told to choose
/// one, not to contact support; with purchases off it reads as before, and a
/// workspace open, or any other state, keeps the state's own words.
#[test]
fn an_account_with_no_workspace_is_told_to_choose_a_plan_only_while_they_are_offered() {
    let model = signed_in_with(None, None);
    assert_eq!(
        signed_in(&model).no_workspace_message().as_deref(),
        Some(WorkspacesState::CHOOSE_PLAN_MESSAGE)
    );
    assert!(!WorkspacesState::CHOOSE_PLAN_MESSAGE.contains("support"));
    assert_eq!(
        WorkspacesState::NoWorkspaces.title(),
        Some("No workspace found")
    );

    let off = signed_in_with(Some(PurchaseSetting::Off), None);
    assert_eq!(signed_in(&off).workspaces, WorkspacesState::NoWorkspaces);
    assert_eq!(
        signed_in(&off).no_workspace_message().as_deref(),
        Some("This account is not linked to a District workspace yet. Please contact support.")
    );

    assert_eq!(signed_in(&agency()).no_workspace_message(), None);
}

/// Checkout is what makes an account's first workspace: closing the view lists
/// the workspaces again and opens the new one, without a restart. One not made
/// yet leaves the account as it was, to be listed again later.
#[test]
fn closing_checkout_with_no_workspace_lists_again_and_opens_the_new_one() {
    let mut model = signed_in_with(None, None);
    checkout(&mut model);
    let effects = model.update(Event::EmbeddedClosed);
    let [Effect::LoadWorkspaces { ticket }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(!purchase(&model).opening());
    assert_eq!(signed_in(&model).workspaces, WorkspacesState::Loading);
    assert!(
        !signed_in(&model).offers_plans(),
        "nothing to choose while listing"
    );

    let effects = model.update(Event::WorkspacesLoaded {
        ticket: *ticket,
        remembered: None,
        result: Err(ApiError::NotFound(ErrorDetail::default())),
    });
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(signed_in(&model).workspaces, WorkspacesState::NoWorkspaces);
    assert!(signed_in(&model).offers_plans());

    // Closed again once the workspace is made: it opens.
    let effects = model.update(Event::EmbeddedClosed);
    let effects = model.update(Event::WorkspacesLoaded {
        ticket: last_ticket(&effects),
        remembered: None,
        result: Ok(workspace_list()),
    });
    let WorkspacesState::Ready(workspaces) = &signed_in(&model).workspaces else {
        panic!("{:?}", signed_in(&model).workspaces);
    };
    let opened = workspaces.active().id.clone();
    assert!(effects.iter().any(
        |e| matches!(e, Effect::LoadOverview { workspace_id, .. } if *workspace_id == opened)
    ));
    assert_eq!(signed_in(&model).route, Route::Overview);
    assert_eq!(signed_in(&model).no_workspace_message(), None);

    // With a workspace open, closing the view off the billing screen lists
    // nothing again.
    model.update(Event::OverviewLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadOverview { .. })),
        result: Ok(overview(&opened, "agency")),
    });
    assert!(model.update(Event::EmbeddedClosed).is_empty());
}

#[test]
fn checkout_opens_inside_the_app_bound_to_the_view_that_shows_it() {
    let mut model = agency();
    checkout(&mut model);
    let state = awaited_state(&model);
    let effects = model.update(answer(&state));
    let [
        request @ Effect::RequestBillingHandOff {
            ticket,
            workspace_id,
            destination,
            nonce: Some(_),
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(workspace_id.as_deref(), Some(AGENCY));
    assert_eq!(*destination, BillingDestination::Checkout(pro_annual()));
    assert!(!format!("{request:?}").contains(NONCE));
    // The same answer again matches nothing: the hand-off has moved on.
    assert!(model.update(answer(&state)).is_empty());

    let effects = model.update(Event::BillingHandOffReady {
        ticket: *ticket,
        result: Ok(hand_off()),
    });
    let [open @ Effect::OpenEmbedded { url, view }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(url.expose(), LINK);
    assert_eq!(*view, EmbeddedView::Same);
    assert!(!format!("{open:?}").contains(CODE));
    assert!(!purchase(&model).opening());
    assert_eq!(purchase(&model).notice, None);
    assert!(!format!("{:?}", signed_in(&model)).contains(CODE));
    // A late copy of the answer is not opened twice.
    assert!(
        model
            .update(Event::BillingHandOffReady {
                ticket: *ticket,
                result: Ok(hand_off()),
            })
            .is_empty()
    );
}

#[test]
fn manage_billing_opens_the_billing_page_inside_the_app_without_a_confirmation() {
    let mut model = agency();
    press(&mut model, BillingEvent::ManageInApp);
    assert_eq!(purchase(&model).confirming, None);
    let state = awaited_state(&model);
    let effects = model.update(answer(&state));
    let [
        Effect::RequestBillingHandOff {
            ticket,
            workspace_id,
            destination,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(*destination, BillingDestination::BillingPage);
    assert_eq!(
        destination.next(workspace_id.is_some()),
        "/dashboard/district/billing"
    );
    let effects = model.update(Event::BillingHandOffReady {
        ticket: *ticket,
        result: Ok(hand_off()),
    });
    let [Effect::OpenEmbedded { view, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(*view, EmbeddedView::Same);
}

/// A stray, forged or old answer is dropped and leaves the purchase waiting;
/// the booking pages' answer is not the purchase's, and the reverse.
#[test]
fn only_the_answer_to_this_purchase_mints_its_link() {
    let mut model = agency();
    // Nothing awaited.
    assert!(
        model
            .update(answer("a-state-nobody-asked-for-0123"))
            .is_empty()
    );

    checkout(&mut model);
    let first = awaited_state(&model);
    for stray in [
        format!("districtai://handoff?state=other-state-0123456789&nonce={NONCE}"),
        format!("districtai://handoff?state={first}"),
        format!("districtai://handoff?state={first}&nonce=short"),
        format!("districtai://handoff?state={first}&state={first}&nonce={NONCE}"),
        format!("districtai://auth?state={first}&nonce={NONCE}"),
        format!("https://www.distronode.com/?state={first}&nonce={NONCE}"),
    ] {
        assert!(
            model
                .update(Event::HandOffCallback(OneTimeUrl::new(stray.clone())))
                .is_empty(),
            "{stray}"
        );
        assert_eq!(awaited_state(&model), first, "{stray}");
    }

    // A second press while the view is awaited starts over: the first
    // answer no longer matches.
    act(&mut model, BillingEvent::ChoosePlan(pro_annual()));
    press(&mut model, BillingEvent::ConfirmPurchase);
    let second = awaited_state(&model);
    assert_ne!(first, second);
    assert!(model.update(answer(&first)).is_empty());
    answered(&mut model);

    // While the link is asked for, pressing again does nothing.
    act(&mut model, BillingEvent::ChoosePlan(pro_annual()));
    assert!(act(&mut model, BillingEvent::ConfirmPurchase).is_empty());
    assert!(act(&mut model, BillingEvent::ManageInApp).is_empty());
    assert_eq!(purchase(&model).confirming, Some(pro_annual()));
    act(&mut model, BillingEvent::CancelPurchase);
    assert_eq!(purchase(&model).confirming, None);
    // Confirming nothing does nothing.
    let mut idle = agency();
    assert!(act(&mut idle, BillingEvent::ConfirmPurchase).is_empty());

    // The booking pages' hand-off and the purchase's do not answer each other.
    let mut model = agency();
    let effects = model.update(Event::Navigate(Route::Scheduling));
    model.update(Event::SchedulingStatusLoaded {
        ticket: last_ticket(&effects),
        result: Ok(fixture("district-scheduling-status-ready.json")),
    });
    model.update(Event::Scheduling(SchedulingEvent::ManageOnWeb));
    let HandOffLeg::Browser(booking) = signed_in(&model).scheduling.hand_off.clone() else {
        panic!();
    };
    checkout(&mut model);
    let effects = model.update(answer(booking.as_str()));
    let [Effect::RequestSchedulingHandOff { .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(matches!(purchase(&model).leg, HandOffLeg::Browser(_)));
    answered(&mut model);
}

/// The view never answers (a service without the start page): after the
/// wait, the link is asked for unbound, and opened in the same view.
#[test]
fn an_unanswered_start_page_mints_an_unbound_link_after_the_wait() {
    let mut model = agency();
    let wait = checkout(&mut model);
    let effects = model.update(Event::WaitOver { ticket: wait });
    let [Effect::RequestBillingHandOff { nonce: None, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    // The answer arriving after the wait gave up is dropped.
    assert!(
        model
            .update(answer("anything-at-all-0123456789"))
            .is_empty()
    );

    // An answer in time cancels the wait.
    let mut model = agency();
    let wait = checkout(&mut model);
    answered(&mut model);
    assert!(model.update(Event::WaitOver { ticket: wait }).is_empty());
}

/// An app that cannot show the page inside its window: the purchase starts
/// again in the system browser, says so, and stays there for the session.
#[test]
fn an_app_that_cannot_embed_falls_back_to_the_browser_and_says_so() {
    let mut model = agency();
    let first_wait = checkout(&mut model);
    let first = awaited_state(&model);
    let effects = model.update(Event::EmbeddedUnavailable);
    let [
        Effect::OpenOneTimeUrl { url },
        Effect::Wait { ticket: wait, .. },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    let second = awaited_state(&model);
    assert_ne!(first, second, "a fresh state for the browser");
    assert_eq!(
        url.expose(),
        format!("https://www.distronode.com/dashboard/handoff/start?state={second}")
    );
    assert!(purchase(&model).in_browser);
    assert_eq!(purchase(&model).surface, Some(PurchaseSurface::Browser));
    assert_eq!(
        purchase(&model).confirm_body(),
        PurchaseState::CONFIRM_BODY_BROWSER
    );
    assert!(
        model
            .update(Event::WaitOver { ticket: first_wait })
            .is_empty()
    );
    assert!(model.update(answer(&first)).is_empty());

    // Another report changes nothing: nothing of this purchase is in the app.
    assert!(model.update(Event::EmbeddedUnavailable).is_empty());

    // The link opens in the browser too, which holds the cookie.
    let effects = model.update(Event::WaitOver { ticket: *wait });
    let [Effect::RequestBillingHandOff { ticket, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    let effects = model.update(Event::BillingHandOffReady {
        ticket: *ticket,
        result: Ok(hand_off()),
    });
    let [Effect::OpenOneTimeUrl { url }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(url.expose(), LINK);

    // The next purchase goes straight to the browser.
    let effects = act(&mut model, BillingEvent::ManageInApp);
    let [Effect::OpenOneTimeUrl { .. }, Effect::Wait { .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert!(PurchaseState::IN_BROWSER.contains("browser"));

    // No browser either: the purchase is given up, and its wait with it.
    let wait = last_ticket(&effects);
    model.update(Event::UrlOpenFailed);
    assert!(!purchase(&model).opening());
    assert!(model.update(Event::WaitOver { ticket: wait }).is_empty());
}

/// The link could not be shown in the view (it was closed, say): it is gone,
/// and pressing again opens the next purchase in the browser.
#[test]
fn a_link_the_view_could_not_show_is_gone_and_the_next_press_uses_the_browser() {
    let mut model = agency();
    checkout(&mut model);
    let ticket = answered(&mut model);
    model.update(Event::BillingHandOffReady {
        ticket,
        result: Ok(hand_off()),
    });
    assert!(model.update(Event::EmbeddedUnavailable).is_empty());
    let notice = purchase(&model).notice.clone().unwrap();
    assert!(notice.retryable);
    assert!(
        notice.message.contains("opens in your browser"),
        "{notice:?}"
    );
    assert!(purchase(&model).in_browser);
    act(&mut model, BillingEvent::DismissPurchaseNotice);
    assert_eq!(purchase(&model).notice, None);
    let effects = act(&mut model, BillingEvent::ManageInApp);
    let [Effect::OpenOneTimeUrl { .. }, Effect::Wait { .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    // In the browser, a page that would not open in the app is not ours.
    model.update(Event::UrlOpenFailed);
    assert!(model.update(Event::EmbeddedUnavailable).is_empty());
}

/// Closing the view gives up a start page it held, and the billing screen is
/// read again, since checkout may have changed the plan.
#[test]
fn closing_the_view_gives_up_its_purchase_and_reads_billing_again() {
    let mut model = agency();
    model.update(Event::Navigate(Route::Billing));
    let wait = checkout(&mut model);
    let effects = model.update(Event::EmbeddedClosed);
    let [
        Effect::LoadWorkspaceBilling { workspace_id, .. },
        Effect::LoadAccountBilling { .. },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(workspace_id, AGENCY);
    assert!(!purchase(&model).opening());
    assert!(model.update(Event::WaitOver { ticket: wait }).is_empty());

    // Elsewhere, closing it reads nothing; and the system browser's start page
    // is not the view's to give up.
    let mut model = agency();
    checkout(&mut model);
    model.update(Event::EmbeddedUnavailable);
    assert!(model.update(Event::EmbeddedClosed).is_empty());
    assert!(purchase(&model).opening());
    // Nor does a closed view give up a link already being asked for.
    let mut model = agency();
    checkout(&mut model);
    answered(&mut model);
    model.update(Event::EmbeddedClosed);
    assert!(purchase(&model).minting());
    // A browser that would not open a page does not give up a purchase in the
    // app either.
    let mut model = agency();
    checkout(&mut model);
    model.update(Event::UrlOpenFailed);
    assert!(purchase(&model).opening());
}

/// "Off" hides everything at once, and drops a purchase under way: its link,
/// if it still arrives, is not opened.
#[test]
fn turning_purchases_off_hides_them_and_drops_one_under_way() {
    let mut model = agency();
    let wait = checkout(&mut model);
    let effects = model.update(Event::SetPurchases(PurchaseSetting::Off));
    assert_eq!(
        effects,
        [Effect::SavePurchaseSetting {
            setting: PurchaseSetting::Off
        }]
    );
    assert!(!signed_in(&model).offers_plans());
    assert!(!signed_in(&model).offers_manage_in_app());
    assert!(!purchase(&model).opening());
    assert!(model.update(Event::WaitOver { ticket: wait }).is_empty());
    assert!(act(&mut model, BillingEvent::ChoosePlan(pro_annual())).is_empty());
    assert_eq!(purchase(&model).confirming, None);
    assert!(act(&mut model, BillingEvent::ConfirmPurchase).is_empty());
    assert!(act(&mut model, BillingEvent::ManageInApp).is_empty());

    // A link asked for before it was turned off is not opened.
    let mut model = agency();
    checkout(&mut model);
    let ticket = answered(&mut model);
    model.update(Event::SetPurchases(PurchaseSetting::Off));
    assert!(
        model
            .update(Event::BillingHandOffReady {
                ticket,
                result: Ok(hand_off()),
            })
            .is_empty()
    );

    // Back on, and kept; a read still on its way does not undo it.
    let (mut model, effects) = with_purchases(|| Model::new(config()));
    let effects = model.update(Event::SessionRestored {
        ticket: last_ticket(&effects),
        result: Ok(claims()),
    });
    let read = pick(&effects, |e| {
        matches!(e, Effect::ReadPurchaseSetting { .. })
    });
    let effects = model.update(Event::SetPurchases(PurchaseSetting::SignInEveryTime));
    assert_eq!(
        effects,
        [Effect::SavePurchaseSetting {
            setting: PurchaseSetting::SignInEveryTime
        }]
    );
    model.update(Event::PurchaseSettingRead {
        ticket: read,
        setting: Some(PurchaseSetting::Off),
    });
    assert_eq!(
        purchase(&model).setting,
        Some(PurchaseSetting::SignInEveryTime)
    );
}

#[test]
fn each_refusal_says_why_in_this_apps_words() {
    let cases = [
        (refused_with(CODE_INVALID_NEXT), "Updating the app", false),
        (
            refused_with("invalid_nonce"),
            "could not confirm the browser",
            true,
        ),
        (refused_with("nonce_required"), "needs your browser", true),
        (
            ApiError::RateLimited {
                retry_after: None,
                detail: ErrorDetail::default(),
            },
            "several times just now",
            true,
        ),
        (
            ApiError::Forbidden(ErrorDetail::default()),
            "confirm your sign-in",
            false,
        ),
        (
            ApiError::Offline(TransportError {
                kind: TransportKind::Connect,
                message: "connection refused".to_owned(),
            }),
            "Could not reach District AI",
            true,
        ),
    ];
    for (error, words, retryable) in cases {
        let mut model = agency();
        checkout(&mut model);
        let ticket = answered(&mut model);
        let effects = model.update(Event::BillingHandOffReady {
            ticket,
            result: Err(error),
        });
        assert!(effects.is_empty(), "{effects:?}");
        let notice = purchase(&model).notice.clone().unwrap();
        assert!(notice.message.contains(words), "{notice:?}");
        assert_eq!(notice.retryable, retryable, "{notice:?}");
        assert!(!purchase(&model).opening());
        // A new press clears it.
        act(&mut model, BillingEvent::ChoosePlan(pro_annual()));
        assert_eq!(purchase(&model).notice, None);
    }

    // A link for another address, or not over HTTPS, is never opened.
    for url in [
        "https://www.distronode.com.example/dashboard/handoff?code=c",
        "http://www.distronode.com/dashboard/handoff?code=c",
    ] {
        let mut model = agency();
        checkout(&mut model);
        let ticket = answered(&mut model);
        let effects = model.update(Event::BillingHandOffReady {
            ticket,
            result: Ok(BillingHandOffResponse {
                url: url.to_owned(),
                expires_in: 60,
            }),
        });
        assert!(effects.is_empty(), "{url}");
        let notice = purchase(&model).notice.clone().unwrap();
        assert!(notice.message.contains("another address"), "{notice:?}");
    }
}

/// A refusal because the session ended signs out, as any request's does.
#[test]
fn a_hand_off_refused_for_an_ended_session_signs_out() {
    let mut model = agency();
    checkout(&mut model);
    let ticket = answered(&mut model);
    model.update(Event::BillingHandOffReady {
        ticket,
        result: Err(signed_out_error()),
    });
    let SessionState::SignedOut(signed_out) = model.session() else {
        panic!("{:?}", model.session());
    };
    assert!(format!("{signed_out:?}").contains(&format!("{:?}", ReauthReason::RefreshRejected)));
}

/// Purchases belong to the account: switching workspace keeps a purchase
/// going, and the link records the workspace open when it is asked for.
#[test]
fn a_purchase_outlives_a_workspace_switch() {
    let mut model = agency();
    checkout(&mut model);
    model.update(Event::SelectWorkspace(VIEWER.to_owned()));
    let state = awaited_state(&model);
    let effects = model.update(answer(&state));
    let [Effect::RequestBillingHandOff { workspace_id, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(workspace_id.as_deref(), Some(VIEWER));
}
