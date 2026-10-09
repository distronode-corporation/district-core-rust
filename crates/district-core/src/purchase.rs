//! Buying in the app: choosing a plan and managing billing on the service's own
//! checkout and billing pages, shown inside the app window, for an app built
//! with [`CoreConfig::in_app_purchases`]. District AI for Windows is the one
//! app that buys in the app; in every other build this module does nothing,
//! and billing is read only ([`BillingScreen`](crate::BillingScreen)).
//!
//! # The flow
//!
//! It is the booking pages' hand-off ([`scheduling`](crate::SchedulingScreen))
//! with two destinations and one more place to open them:
//!
//! 1. The member chooses a plan ([`BillingEvent::ChoosePlan`]) and confirms
//!    it on a step that names Stripe ([`PurchaseState::CONFIRM_BODY`],
//!    [`BillingEvent::ConfirmPurchase`]), or asks to manage billing
//!    ([`BillingEvent::ManageInApp`]).
//! 2. The core opens the service's hand-off start page with a fresh `state`
//!    in a new private view inside the app ([`Effect::OpenEmbedded`] with
//!    [`EmbeddedView::NewPrivate`]), and waits [`HAND_OFF_CALLBACK_WAIT`] for
//!    its answer.
//! 3. The start page sets its nonce cookie in that view and answers with a
//!    `districtai://handoff` link, which the app catches in the view and hands
//!    over as [`Event::HandOffCallback`], exactly as it does for a link the
//!    system browser opens ([`Event::from_link`]). A link that does not answer
//!    this hand-off is dropped and cancels nothing.
//! 4. The core asks for the hand-off link with that nonce
//!    ([`Effect::RequestBillingHandOff`]), landing on the checkout or the
//!    billing page in exactly the shape the service admits
//!    ([`BillingDestination::next`]).
//! 5. The link is checked to be on the service's own address and opened in the
//!    same view ([`EmbeddedView::Same`]), which holds the cookie it is redeemed
//!    with. Nothing of it is kept.
//!
//! An app that cannot show a page inside its window answers
//! [`UrlOpener::open_embedded`](crate::UrlOpener::open_embedded) with false,
//! as the trait's own default does. The core then starts again in the system
//! browser, which needs both legs in one browser too, says so
//! ([`PurchaseState::IN_BROWSER`]), and keeps using the browser for the rest
//! of the session.
//!
//! # "Sign in every time"
//!
//! The one setting a member has on this computer ([`PurchaseSetting`]). Each
//! purchase, and each visit to the billing page, signs in afresh: a fresh
//! one-time code, minted after the press and good for one sign-in within a
//! minute, redeemed in a fresh private view that the app opens with nothing
//! from any earlier one and keeps nothing from once it closes. No web session
//! outlives the view, so the app never holds a signed-in web page between
//! purchases. The code is opened as soon as it arrives, well inside its minute,
//! and a second press mints a second code rather than reusing the first.

use district_api::ApiError;
use district_auth::{HAND_OFF_START_PATH, HandOffNonce, HandOffState};
use district_model::BillingHandOffResponse;

use crate::billing::BILLING_WEB_PATH;
use crate::failure::{FailureText, PURCHASE_NOT_SHOWN};
use crate::model::{CoreConfig, Effect, Slot, Ticket, Tickets};
use crate::route::Route;
use crate::scheduling::{
    HAND_OFF_CALLBACK_WAIT, HandOffLeg, OneTimeUrl, hand_off_failure, on_origin,
};
use crate::signed_in::{Next, SignedIn, stay};
use crate::workspaces::WorkspacesState;
#[cfg(doc)]
use crate::{BillingEvent, Event};

/// The service's checkout page, below its origin.
pub const CHECKOUT_PATH: &str = "/checkout";

/// The one `reason` the checkout page reads: the account has no workspace yet,
/// and checkout is what makes one.
const NO_WORKSPACE_REASON: &str = "no-workspace";

/// A plan the app offers, by the key the service publishes it under. The
/// self-serve voice plans only: the service's network plan is not sold in the
/// app. Prices are not here; checkout shows them, from the service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlanTier {
    /// Voice Solo.
    VoiceSolo,
    /// Voice Starter.
    VoiceStarter,
    /// Voice Pro.
    VoicePro,
    /// Voice Studio.
    VoiceStudio,
}

impl PlanTier {
    /// Every plan offered, smallest first.
    pub const ALL: [Self; 4] = [
        Self::VoiceSolo,
        Self::VoiceStarter,
        Self::VoicePro,
        Self::VoiceStudio,
    ];

    /// The key the service publishes the plan under, byte for byte.
    pub fn key(self) -> &'static str {
        match self {
            Self::VoiceSolo => "VoiceSolo",
            Self::VoiceStarter => "VoiceStarter",
            Self::VoicePro => "VoicePro",
            Self::VoiceStudio => "VoiceStudio",
        }
    }

    /// The plan's name.
    pub fn label(self) -> &'static str {
        match self {
            Self::VoiceSolo => "Voice Solo",
            Self::VoiceStarter => "Voice Starter",
            Self::VoicePro => "Voice Pro",
            Self::VoiceStudio => "Voice Studio",
        }
    }
}

/// How often a plan is billed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlanTerm {
    /// Every month: the smaller commitment, and the service's default.
    #[default]
    Monthly,
    /// A year, paid up front.
    Annual,
}

impl PlanTerm {
    /// Both terms, monthly first.
    pub const ALL: [Self; 2] = [Self::Monthly, Self::Annual];

    /// The term as the checkout page reads it.
    pub fn key(self) -> &'static str {
        match self {
            Self::Monthly => "monthly",
            Self::Annual => "annual",
        }
    }

    /// The term's name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Monthly => "Monthly",
            Self::Annual => "Annual",
        }
    }
}

/// A promotion code, in the one shape the checkout page reads: trimmed,
/// upper case, 1 to [`MAX_LEN`](Self::MAX_LEN) characters of `A` to `Z`, `0`
/// to `9`, `-` and `_`. Anything else is refused whole, never cleaned into a
/// different code nobody typed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PromoCode(String);

impl PromoCode {
    /// The longest code the service reads.
    pub const MAX_LEN: usize = 40;
    /// The field's label.
    pub const LABEL: &'static str = "Promotion code (optional)";
    /// What to say about a code that is not of that shape.
    pub const INVALID: &'static str = "A promotion code has only letters, digits, hyphens and \
        underscores, up to 40 of them.";

    /// The code the member typed, or `None` when it is not of the shape the
    /// service reads. Letters are upper-cased and surrounding spaces dropped,
    /// as the service does; nothing else is changed.
    pub fn parse(typed: &str) -> Option<Self> {
        let code = typed.trim().to_ascii_uppercase();
        let shaped = (1..=Self::MAX_LEN).contains(&code.len())
            && code.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || b"-_".contains(&byte)
            });
        shaped.then_some(Self(code))
    }

    /// The code.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A plan the member chose: the plan, how often it is billed, and an optional
/// promotion code. The service decides the price, and the code's discount, at
/// checkout.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlanChoice {
    /// The plan.
    pub tier: PlanTier,
    /// How often it is billed.
    pub term: PlanTerm,
    /// The promotion code, if one was given.
    pub promo: Option<PromoCode>,
}

/// Where a billing hand-off lands. Only these two: the service refuses any
/// other, and the shapes here are the only ones it admits.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum BillingDestination {
    /// Checkout, for this plan.
    Checkout(PlanChoice),
    /// The billing page, to change or cancel a plan or a payment method.
    #[default]
    BillingPage,
}

impl BillingDestination {
    /// The path the hand-off asks to land on, in exactly the shape the service
    /// admits: `/checkout` with `tier`, `term`, then `promo` when there is one
    /// and `reason=no-workspace` when no workspace is open, each once; or
    /// `/dashboard/district/billing` with no query. Every value is from a closed
    /// set or of [`PromoCode`]'s shape, so nothing needs escaping.
    pub fn next(&self, has_workspace: bool) -> String {
        let Self::Checkout(choice) = self else {
            return BILLING_WEB_PATH.to_owned();
        };
        let mut next = format!(
            "{CHECKOUT_PATH}?tier={}&term={}",
            choice.tier.key(),
            choice.term.key()
        );
        if let Some(promo) = &choice.promo {
            next.push_str("&promo=");
            next.push_str(promo.as_str());
        }
        if !has_workspace {
            next.push_str("&reason=");
            next.push_str(NO_WORKSPACE_REASON);
        }
        next
    }
}

/// Whether this computer offers purchases, the member's choice per device,
/// kept in the app's settings ([`Settings::purchases`](crate::Settings::purchases)).
/// Only read in an app built with [`CoreConfig::in_app_purchases`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PurchaseSetting {
    /// No plans and no billing actions on this computer. The billing screen
    /// is read only, as in an app that does not buy in the app.
    Off,
    /// Plans and billing actions, each signing in afresh in a private view that
    /// keeps nothing (see the module documentation). The default.
    #[default]
    SignInEveryTime,
}

impl PurchaseSetting {
    /// Both values, in the order the setting lists them.
    pub const ALL: [Self; 2] = [Self::SignInEveryTime, Self::Off];
    /// The setting's label.
    pub const LABEL: &'static str = "Purchases on this computer";
    /// What the setting does.
    pub const BODY: &'static str = "With Sign in every time, choosing a plan or managing billing \
        signs you in afresh, in a private window inside District AI that keeps nothing once it \
        closes. Off hides plans and billing actions on this computer.";

    /// The value's name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::SignInEveryTime => "Sign in every time",
        }
    }

    /// The value as the settings file keeps it.
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::SignInEveryTime => "sign_in_every_time",
        }
    }

    /// The value the settings file keeps as `key`, or `None` for one this build
    /// does not know, which then reads as the default.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|setting| setting.key() == key)
    }
}

/// Which view a page opens in, inside the app ([`Effect::OpenEmbedded`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EmbeddedView {
    /// A new private view in the app window, replacing any earlier one, that
    /// starts with nothing (no cookies, no storage, no cache from any earlier
    /// view or from the system browser) and keeps nothing once it closes.
    NewPrivate,
    /// The view the last [`NewPrivate`](Self::NewPrivate) opened, which holds
    /// the cookie the start page set. If it was closed, the page is not opened
    /// anywhere else and the answer is false.
    Same,
}

/// Where a purchase's pages open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PurchaseSurface {
    /// Inside the app window.
    InApp,
    /// In the system browser, because the app could not show it.
    Browser,
}

/// Purchases, while someone is signed in. Kept for the account, not the
/// workspace: an account the app just created buys before it has one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PurchaseState {
    /// The setting on this computer, once read; `None` until then, and always
    /// in an app that does not buy in the app.
    pub setting: Option<PurchaseSetting>,
    /// The plan awaiting confirmation, while the step naming Stripe shows.
    pub confirming: Option<PlanChoice>,
    /// Where the hand-off stands. One at a time: a press while the start page
    /// is awaited starts a new one, whose `state` the old answer no longer
    /// matches.
    pub leg: HandOffLeg,
    /// Where the last purchase page opened, or `None` before the first.
    pub surface: Option<PurchaseSurface>,
    /// What the last press came to, when it needs saying.
    pub notice: Option<FailureText>,
    /// Whether the app could not show a page inside its window this session,
    /// so purchase pages open in the system browser.
    pub in_browser: bool,
    /// Where the hand-off under way, or the last one, lands.
    pub(crate) destination: BillingDestination,
}

impl PurchaseState {
    /// The action that shows the plans.
    pub const CHOOSE_PLAN_ACTION: &'static str = "Choose a plan";
    /// The action that opens the billing page.
    pub const MANAGE_ACTION: &'static str = "Manage billing";
    /// The line beside the plans: the core names no price.
    pub const PRICES_NOTE: &'static str = "Prices, and any promotion, are shown at checkout \
        before you pay.";
    /// The billing screen's note, in place of the read-only one, while
    /// purchases are on.
    pub const IN_APP_NOTE: &'static str = "Choose or change your plan here. Checkout and \
        payment are handled by Stripe, and open inside District AI.";
    /// The confirmation step's heading.
    pub const CONFIRM_TITLE: &'static str = "Continue to checkout with Stripe";
    /// The confirmation step's body, naming Stripe before checkout opens.
    pub const CONFIRM_BODY: &'static str = "Payment is handled by Stripe, District AI's payment \
        processor. Checkout opens in a private window inside District AI, signed in for this \
        purchase only. Your card details go to Stripe, not to District AI, and the price is \
        shown before you pay.";
    /// The confirmation step's action.
    pub const CONFIRM_ACTION: &'static str = "Continue to checkout";
    /// The confirmation step's way back.
    pub const CANCEL_ACTION: &'static str = "Cancel";
    /// While the page is being opened.
    pub const OPENING: &'static str = "Opening a secure page";
    /// Said once purchase pages open in the system browser.
    pub const IN_BROWSER: &'static str = "This computer cannot show checkout inside District AI, \
        so it opens in your browser.";

    /// Whether purchases are on for this computer.
    pub fn enabled(&self) -> bool {
        self.setting == Some(PurchaseSetting::SignInEveryTime)
    }

    /// Whether a hand-off is under way, in either leg.
    pub fn opening(&self) -> bool {
        self.leg != HandOffLeg::Idle
    }

    /// Whether the hand-off link is being asked for. A press then does nothing.
    pub fn minting(&self) -> bool {
        self.leg == HandOffLeg::Minting
    }
}

impl SignedIn {
    /// Whether to offer the plans: purchases are on, and either no workspace
    /// exists yet (an account checkout will make one for) or the member's role
    /// could change the plan of the one open.
    pub fn offers_plans(&self) -> bool {
        self.purchase.enabled()
            && match self.workspaces {
                WorkspacesState::NoWorkspaces => true,
                WorkspacesState::Ready(_) => self.capabilities().can_change,
                _ => false,
            }
    }

    /// Whether to offer "Manage billing" inside the app: purchases are on, a
    /// workspace is open, and the member's role could change its plan.
    pub fn offers_manage_in_app(&self) -> bool {
        self.purchase.enabled()
            && matches!(self.workspaces, WorkspacesState::Ready(_))
            && self.capabilities().can_change
    }

    /// The setting was read at sign-in.
    pub(crate) fn purchase_setting_read(
        &mut self,
        ticket: Ticket,
        setting: Option<PurchaseSetting>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::PurchaseSetting, ticket) {
            self.purchase.setting = Some(setting.unwrap_or_default());
        }
        stay()
    }

    /// The member changed the setting. Nothing in an app that does not buy in
    /// the app. Turning it off drops a purchase under way: its link, if one
    /// arrives, is not opened.
    pub(crate) fn set_purchases(
        &mut self,
        setting: PurchaseSetting,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Next {
        if !config.in_app_purchases {
            return stay();
        }
        // A read still on its way would undo this choice.
        tickets.cancel(Slot::PurchaseSetting);
        self.purchase.setting = Some(setting);
        if setting == PurchaseSetting::Off {
            tickets.cancel(Slot::BillingHandOff);
            tickets.cancel(Slot::BillingHandOffWait);
            self.purchase.confirming = None;
            self.purchase.leg = HandOffLeg::Idle;
            self.purchase.notice = None;
        }
        Next::Stay(vec![Effect::SavePurchaseSetting { setting }])
    }

    /// A plan was chosen: the step naming Stripe shows.
    pub(crate) fn choose_plan(&mut self, choice: PlanChoice) {
        if self.offers_plans() {
            self.purchase.confirming = Some(choice);
            self.purchase.notice = None;
        }
    }

    /// The plan was confirmed: checkout opens.
    pub(crate) fn confirm_purchase(&mut self, tickets: &mut Tickets, config: &CoreConfig) -> Next {
        if !self.offers_plans() || self.purchase.minting() {
            return stay();
        }
        let Some(choice) = self.purchase.confirming.take() else {
            return stay();
        };
        Next::Stay(self.start_purchase(BillingDestination::Checkout(choice), tickets, config))
    }

    /// "Manage billing", inside the app.
    pub(crate) fn manage_in_app(&mut self, tickets: &mut Tickets, config: &CoreConfig) -> Next {
        if !self.offers_manage_in_app() || self.purchase.minting() {
            return stay();
        }
        Next::Stay(self.start_purchase(BillingDestination::BillingPage, tickets, config))
    }

    /// Opens the start page with a fresh `state`, inside the app unless this
    /// session found it cannot show one, and waits for its answer.
    fn start_purchase(
        &mut self,
        destination: BillingDestination,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Vec<Effect> {
        let state = HandOffState::generate();
        let start = OneTimeUrl::new(
            config.web_url(&format!("{HAND_OFF_START_PATH}?state={}", state.as_str())),
        );
        let surface = if self.purchase.in_browser {
            PurchaseSurface::Browser
        } else {
            PurchaseSurface::InApp
        };
        self.purchase.leg = HandOffLeg::Browser(state);
        self.purchase.destination = destination;
        self.purchase.surface = Some(surface);
        self.purchase.notice = None;
        vec![
            open_page(surface, start, EmbeddedView::NewPrivate),
            Effect::Wait {
                ticket: tickets.issue(Slot::BillingHandOffWait),
                delay: HAND_OFF_CALLBACK_WAIT,
            },
        ]
    }

    /// The link is asked for, bound by `nonce`, or unbound when the start page
    /// did not answer in time. The workspace open now is recorded with it.
    pub(crate) fn mint_purchase(
        &mut self,
        nonce: Option<HandOffNonce>,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        self.purchase.leg = HandOffLeg::Minting;
        tickets.cancel(Slot::BillingHandOffWait);
        vec![Effect::RequestBillingHandOff {
            ticket: tickets.issue(Slot::BillingHandOff),
            workspace_id: self.active_id(),
            destination: self.purchase.destination.clone(),
            nonce,
        }]
    }

    /// The link arrived: open it at once in the view that holds the start
    /// page's cookie, if it is on the service's own address, and keep nothing.
    pub(crate) fn billing_hand_off(
        &mut self,
        ticket: Ticket,
        result: Result<BillingHandOffResponse, ApiError>,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Next {
        if !tickets.accept(Slot::BillingHandOff, ticket) {
            return stay();
        }
        self.purchase.leg = HandOffLeg::Idle;
        let surface = self.purchase.surface.unwrap_or(PurchaseSurface::Browser);
        let notice = match result {
            Ok(answer) if on_origin(&answer.url, config) => {
                return Next::Stay(vec![open_page(
                    surface,
                    OneTimeUrl::new(answer.url),
                    EmbeddedView::Same,
                )]);
            }
            Ok(_) => hand_off_failure(None),
            Err(error) => hand_off_failure(Some(&error)),
        };
        self.purchase.notice = Some(notice);
        stay()
    }

    /// The app could not show a page inside its window. A purchase awaiting
    /// its start page there starts again in the system browser, and every
    /// purchase after it this session opens there. A link that could not be
    /// shown is gone: pressing again starts afresh, in the browser.
    pub(crate) fn embedded_unavailable(
        &mut self,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Next {
        if self.purchase.surface != Some(PurchaseSurface::InApp) {
            return stay();
        }
        self.purchase.in_browser = true;
        if matches!(self.purchase.leg, HandOffLeg::Browser(_)) {
            let destination = self.purchase.destination.clone();
            return Next::Stay(self.start_purchase(destination, tickets, config));
        }
        self.purchase.notice = Some(FailureText::retryable(PURCHASE_NOT_SHOWN));
        stay()
    }

    /// The member closed the view inside the app. A start page it held will
    /// never answer, so its purchase is given up; and the billing screen, if it
    /// shows, is read again, since a purchase may have changed the plan.
    pub(crate) fn embedded_closed(&mut self, tickets: &mut Tickets, config: &CoreConfig) -> Next {
        if !config.in_app_purchases {
            return stay();
        }
        if self.purchase.surface == Some(PurchaseSurface::InApp)
            && matches!(self.purchase.leg, HandOffLeg::Browser(_))
        {
            tickets.cancel(Slot::BillingHandOffWait);
            self.purchase.leg = HandOffLeg::Idle;
        }
        if self.route == Route::Billing {
            return Next::Stay(self.enter_billing(tickets));
        }
        stay()
    }

    /// No browser took a page. A purchase awaiting its start page there never
    /// gets an answer, so it is given up.
    pub(crate) fn purchase_unopened(&mut self, tickets: &mut Tickets) {
        if self.purchase.surface == Some(PurchaseSurface::Browser)
            && matches!(self.purchase.leg, HandOffLeg::Browser(_))
        {
            tickets.cancel(Slot::BillingHandOffWait);
            self.purchase.leg = HandOffLeg::Idle;
        }
    }

    /// The member dismissed the notice.
    pub(crate) fn dismiss_purchase_notice(&mut self) {
        self.purchase.notice = None;
    }

    /// The member backed out of the confirmation step.
    pub(crate) fn cancel_purchase(&mut self) {
        self.purchase.confirming = None;
    }
}

/// The effect that opens `url` where `surface` says.
fn open_page(surface: PurchaseSurface, url: OneTimeUrl, view: EmbeddedView) -> Effect {
    match surface {
        PurchaseSurface::InApp => Effect::OpenEmbedded { url, view },
        PurchaseSurface::Browser => Effect::OpenOneTimeUrl { url },
    }
}
