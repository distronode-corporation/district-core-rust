//! Managing the numbers a workspace holds and its carrier account, on the phone
//! numbers screen: reapplying a number's routing, releasing it, the billed
//! lookup, which carriers are connected, SIP trunks, the one-time passcode
//! sender, and the A2P 10DLC and toll-free registrations. After the iOS app's
//! `NumberActionsSheet`, `CarrierSection` and `ComplianceSection`.
//!
//! Buying a number is not here: it stays on the web.
//!
//! Each write has a state of its own, because several are on screen at once and
//! they fail for unrelated reasons. A write that costs money or cannot be
//! undone (a release, a lookup, a trunk, the passcode sender, a registration)
//! is offered again after a failure only when the failure proves it did not
//! happen: a refusal before any work (a 4xx, or a request never sent). After a
//! server error, no answer, or an answer that could not be read, it may have
//! happened, so the control is gone until the screen is read again
//! ([`NumberWrite::Failed`]). Reapplying a number's routing converges, so it is
//! always offered again.
//!
//! A release, a lookup and the two registrations are confirmed first
//! ([`NumberConfirmation`]); reapplying routing is not, because a confirmation
//! on a harmless action teaches the member to dismiss the ones that matter.
//!
//! A viewer may read the carriers' status, the SIP trunks and the passcode
//! sender, and is offered nothing that changes them.

use district_api::ApiError;
use district_model::{
    A2P_CAMPAIGN_TYPES, A2pRegistrationDraft, A2pRegistrationResponse, NumberLookupInfo,
    NumberLookupResponse, NumberReleaseResponse, OwnedNumber, ProviderStatusEntry,
    ProviderStatusResponse, SipTrunk, SipTrunkCreateResponse, SipTrunkDraft, SipTrunksResponse,
    TOLL_FREE_USE_CASES, TollFreeVerificationDraft, TollFreeVerificationResponse,
    VerifyServiceResponse,
};

use crate::failure::FailureText;
use crate::marketplace::OwnedNumbersList;
use crate::model::{Effect, Slot, Ticket, Tickets};
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::l;
use district_l10n::{WireCategory, format, numbers_strings as ns, wire_label};

/// The slots of number management, forgotten with the workspace.
pub(crate) const NUMBER_SLOTS: [Slot; 6] = [
    Slot::NumberCarrier,
    Slot::NumberSip,
    Slot::NumberVerify,
    Slot::NumberWrite,
    Slot::NumberCarrierWrite,
    Slot::NumberLookup,
];

/// A call to the service about a held number or the carrier account. Run by
/// [`DistrictApi::number_call`](crate::DistrictApi::number_call).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NumberCall {
    /// Read which carriers are connected.
    CarrierStatus,
    /// Read the SIP trunks.
    SipTrunks,
    /// Create a SIP trunk. Billed monthly.
    CreateSipTrunk(SipTrunkDraft),
    /// Read whether the one-time passcode sender is in use.
    VerifyService,
    /// Turn the one-time passcode sender on or off.
    SetVerifyService(bool),
    /// Reapply this number's routing.
    Configure(String),
    /// Release this number. Cannot be undone.
    Release(String),
    /// Look this number up. Billed.
    Lookup(String),
    /// File an A2P 10DLC registration. Billed.
    RegisterA2p(A2pRegistrationDraft),
    /// File a toll-free verification.
    VerifyTollFree(TollFreeVerificationDraft),
}

/// The service's answer to a [`NumberCall`], one variant per call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NumberAnswer {
    /// To [`NumberCall::CarrierStatus`].
    CarrierStatus(ProviderStatusResponse),
    /// To [`NumberCall::SipTrunks`].
    SipTrunks(SipTrunksResponse),
    /// To [`NumberCall::CreateSipTrunk`].
    SipTrunkCreated(SipTrunkCreateResponse),
    /// To [`NumberCall::VerifyService`] and [`NumberCall::SetVerifyService`]:
    /// the state stored.
    VerifyService(VerifyServiceResponse),
    /// To [`NumberCall::Configure`].
    Configured,
    /// To [`NumberCall::Release`].
    Released(NumberReleaseResponse),
    /// To [`NumberCall::Lookup`].
    LookedUp(NumberLookupResponse),
    /// To [`NumberCall::RegisterA2p`].
    A2pRegistered(A2pRegistrationResponse),
    /// To [`NumberCall::VerifyTollFree`].
    TollFreeFiled(TollFreeVerificationResponse),
}

/// Number management's part of the phone numbers screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NumberAdmin {
    /// Which carriers are connected. Read when the carrier tab opens.
    pub carrier: CarrierStatus,
    /// The SIP trunks. Read when the carrier tab opens.
    pub sip: SipTrunks,
    /// The one-time passcode sender. Read when the carrier tab opens.
    pub verify: VerifyService,
    /// The confirmation being asked, if any.
    pub confirmation: Option<NumberConfirmation>,
    /// Reapplying or releasing a number.
    pub number_write: NumberWrite,
    /// The number [`number_write`](Self::number_write) is about.
    pub number_written: Option<String>,
    /// Creating a SIP trunk, the passcode sender, and the two registrations.
    pub carrier_write: NumberWrite,
    /// The billed lookup.
    pub lookup_write: NumberWrite,
    /// The last lookup's answer, as lines to show. Kept apart from its write,
    /// so a new confirmation does not blank it.
    pub lookup_result: Option<Vec<String>>,
    /// The number to look up, as typed.
    pub lookup_number: String,
    /// The SIP trunk form.
    pub sip_form: SipTrunkForm,
    /// The A2P 10DLC form.
    pub a2p_form: A2pForm,
    /// The toll-free verification form.
    pub toll_free_form: TollFreeForm,
}

/// Which carriers are connected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CarrierStatus {
    /// Never read.
    #[default]
    NotLoaded,
    /// Being read.
    Loading,
    /// Read: at least one carrier has credentials.
    Ready(ProviderStatusResponse),
    /// No carrier has credentials: an account state, not a fault, so no "Try
    /// again".
    NotConnected,
    /// The read failed.
    Failed(FailureText),
}

/// The SIP trunks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SipTrunks {
    /// Never read.
    #[default]
    NotLoaded,
    /// Being read.
    Loading,
    /// Read. Empty is a real answer.
    Ready(Vec<SipTrunk>),
    /// The read failed.
    Failed(FailureText),
}

/// The one-time passcode sender.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum VerifyService {
    /// Never read.
    #[default]
    NotLoaded,
    /// Being read.
    Loading,
    /// Read, or set.
    Ready {
        /// Whether it is in use.
        enabled: bool,
    },
    /// The read failed.
    Failed(FailureText),
}

/// One write's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NumberWrite {
    /// Nothing done yet, or done with nothing to say.
    #[default]
    Idle,
    /// On its way.
    Running,
    /// Done, with a notice. Not always good news: a release whose monthly
    /// charge could not be ended lands here, so it is not drawn as a failure.
    Done(String),
    /// Failed.
    Failed {
        /// Why.
        failure: FailureText,
        /// Whether the control is offered again: false when the write may have
        /// happened all the same.
        repeatable: bool,
    },
}

impl NumberWrite {
    /// Whether its control may be pressed: not while it is on its way, and not
    /// after a failure that does not prove it did not happen.
    pub fn is_armed(&self) -> bool {
        match self {
            Self::Idle | Self::Done(_) => true,
            Self::Running => false,
            Self::Failed { repeatable, .. } => *repeatable,
        }
    }

    /// Whether it is on its way.
    pub fn is_running(&self) -> bool {
        *self == Self::Running
    }

    /// The notice to show under its control, if any.
    pub fn notice(&self) -> Option<&str> {
        match self {
            Self::Idle | Self::Running => None,
            Self::Done(notice) => Some(notice),
            Self::Failed { failure, .. } => Some(&failure.message),
        }
    }

    /// The second sentence under a failure that took its control away, saying
    /// why, or `None`.
    pub fn unrepeatable_note(&self) -> Option<&'static str> {
        matches!(
            self,
            Self::Failed {
                repeatable: false,
                ..
            }
        )
        .then(|| ns::numbers_write_unrepeatable(l()))
    }

    /// The label of a confirmation's button that closes it: "Close" once the
    /// write may have happened, when "Cancel" would claim otherwise.
    pub fn close_label(&self) -> &'static str {
        if self.unrepeatable_note().is_some() {
            ns::numbers_close(l())
        } else {
            ns::numbers_cancel(l())
        }
    }

    /// `done` once it worked, else what failed and whether it may be tried
    /// again.
    fn after<T>(result: Result<T, ApiError>, repeat: Repeat, done: impl FnOnce(T) -> Self) -> Self {
        match result {
            Ok(answer) => done(answer),
            Err(error) => Self::Failed {
                repeatable: repeat == Repeat::Converges || proves_not_done(&error),
                failure: FailureText::from_api_error(&error),
            },
        }
    }
}

/// Whether sending a write again does the same thing again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Repeat {
    /// The service ends in the same state however often it is sent.
    Converges,
    /// A repeat costs something or cannot be undone.
    Once,
}

/// Whether `error` proves the request did nothing: the service refused it
/// before any work, or it was never sent. A server error, no answer, or a
/// success that could not be read prove nothing, and the last proves it
/// happened.
fn proves_not_done(error: &ApiError) -> bool {
    match error {
        ApiError::Unauthorized(_)
        | ApiError::Forbidden(_)
        | ApiError::NotFound(_)
        | ApiError::Conflict(_)
        | ApiError::RateLimited { .. }
        | ApiError::TokenUnavailable(_)
        | ApiError::Rejected { .. }
        | ApiError::InvalidRequest(_) => true,
        ApiError::Envelope { status, .. } => (400..500).contains(status),
        ApiError::Server { .. }
        | ApiError::Redirect { .. }
        | ApiError::Offline(_)
        | ApiError::Decode { .. }
        | ApiError::Unconfirmed { .. } => false,
    }
}

/// A confirmation asked before a write that cannot be undone or costs money.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NumberConfirmation {
    /// Before releasing this number.
    Release {
        /// The number, in full.
        phone_number: String,
    },
    /// Before a lookup, which the carrier charges for.
    Lookup {
        /// The number to look up.
        phone_number: String,
    },
    /// Before filing the A2P 10DLC registration.
    A2p,
    /// Before filing the toll-free verification.
    TollFree,
}

impl NumberConfirmation {
    /// The question.
    pub fn title(&self) -> &'static str {
        match self {
            Self::Release { .. } => ns::numbers_release_confirm_title(l()),
            Self::Lookup { .. } => ns::numbers_lookup_confirm_title(l()),
            Self::A2p => ns::numbers_a2p_confirm_title(l()),
            Self::TollFree => ns::numbers_tfv_confirm_title(l()),
        }
    }

    /// What happens, in the units the member weighs it in.
    pub fn body(&self) -> String {
        match self {
            Self::Release { phone_number } => ns::numbers_release_confirm_body(l(), phone_number),
            Self::Lookup { .. } => ns::numbers_lookup_confirm_body(l()).to_owned(),
            Self::A2p => ns::numbers_a2p_confirm_body(l()).to_owned(),
            Self::TollFree => ns::numbers_tfv_confirm_body(l()).to_owned(),
        }
    }

    /// The button that goes ahead.
    pub fn action(&self) -> &'static str {
        match self {
            Self::Release { .. } => ns::numbers_release_confirm_action(l()),
            Self::Lookup { .. } => ns::numbers_lookup_confirm_action(l()),
            Self::A2p => ns::numbers_a2p_confirm_action(l()),
            Self::TollFree => ns::numbers_tfv_confirm_action(l()),
        }
    }

    /// Whether this confirmation is a release, drawn as destructive.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::Release { .. })
    }
}

impl NumberAdmin {
    /// Closes the confirmation open when `which` picks it, and says whether it
    /// did. A write landing closes its own confirmation and no other.
    fn close_confirmation(&mut self, which: fn(&NumberConfirmation) -> bool) -> bool {
        let close = self.confirmation.as_ref().is_some_and(which);
        if close {
            self.confirmation = None;
        }
        close
    }

    /// The write a confirmation commits.
    pub fn write_for(&self, confirmation: &NumberConfirmation) -> &NumberWrite {
        match confirmation {
            NumberConfirmation::Release { .. } => &self.number_write,
            NumberConfirmation::Lookup { .. } => &self.lookup_write,
            NumberConfirmation::A2p | NumberConfirmation::TollFree => &self.carrier_write,
        }
    }

    /// The label of the open confirmation's button that goes ahead: "Working…"
    /// while it is on its way.
    pub fn confirm_label(&self) -> Option<&'static str> {
        let confirmation = self.confirmation.as_ref()?;
        Some(if self.write_for(confirmation).is_running() {
            ns::numbers_working(l())
        } else {
            confirmation.action()
        })
    }
}

/// The note on the phone numbers screen saying what it does and does not do,
/// for a member who can change numbers or for a viewer.
pub fn numbers_scope_note(can_change: bool) -> &'static str {
    if can_change {
        ns::numbers_purchase_elsewhere(l())
    } else {
        ns::numbers_viewer_cannot_provision(l())
    }
}

/// What can be done to a held number: `None` when nothing, with the sentence
/// saying why (a number Distronode holds, or a viewer). It decides whether to
/// offer the number's actions; the sentence to show on its row is
/// [`number_row_note`].
pub fn number_actions_note(number: &OwnedNumber, can_change: bool) -> Option<&'static str> {
    if number.managed {
        Some(ns::numbers_managed_no_release(l()))
    } else if !can_change {
        Some(ns::numbers_viewer_cannot_provision(l()))
    } else {
        None
    }
}

/// The sentence to show on a held number's row, beside the screen's note
/// ([`numbers_scope_note`]): why a number Distronode holds cannot be released
/// or reconfigured. A viewer's reason is the screen's note already, so it is
/// not said again on every row. Offer the row's actions only when
/// [`number_actions_note`] is `None`.
pub fn number_row_note(number: &OwnedNumber, can_change: bool) -> Option<&'static str> {
    number_actions_note(number, can_change).filter(|note| *note != numbers_scope_note(can_change))
}

/// A carrier account's balance, or `None` when it gave no balance. With the
/// currency the carrier named, the amount is written as money by
/// `district_l10n::format` (`Balance $12.50`, `Solde : 12,50 $ US`); with
/// none, as a number with two decimals (`Balance 12.50`, `Solde : 12,50`). A
/// balance that is not a number is shown as the carrier wrote it.
pub fn carrier_balance(entry: &ProviderStatusEntry) -> Option<String> {
    let balance = entry.balance.as_deref()?;
    let currency = entry
        .currency
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());
    let amount = balance
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite());
    Some(match (amount, currency) {
        (Some(amount), Some(currency)) => {
            ns::numbers_carrier_balance_amount(l(), &format::iso_amount(l(), amount, currency))
        }
        (Some(amount), None) => {
            ns::numbers_carrier_balance_amount(l(), &format::decimal(l(), amount, 2))
        }
        (None, Some(currency)) => ns::numbers_carrier_balance_with_currency(l(), balance, currency),
        (None, None) => ns::numbers_carrier_balance_amount(l(), balance),
    })
}

/// How many numbers a carrier account holds, or `None` when it did not say.
pub fn carrier_number_count(entry: &ProviderStatusEntry) -> Option<String> {
    entry
        .number_count
        .map(|count| ns::numbers_carrier_number_count(l(), count))
}

/// A carrier account's state in words: refused, held by Distronode, or
/// connected.
pub fn carrier_state(entry: &ProviderStatusEntry) -> &'static str {
    if !entry.connected {
        ns::numbers_carrier_refused(l())
    } else if entry.managed == Some(true) {
        ns::numbers_carrier_managed(l())
    } else {
        ns::numbers_carrier_connected(l())
    }
}

/// A SIP trunk's line under its name: its state, by its label (a state this
/// build does not know reads as the generic one, never as stored), and how many
/// addresses may use it.
pub fn sip_trunk_meta(trunk: &SipTrunk) -> String {
    let count = i64::try_from(trunk.ip_access_control_list.len()).unwrap_or(i64::MAX);
    ns::numbers_sip_trunk_meta_line(
        l(),
        &wire_label(WireCategory::SipTrunkStatus, &trunk.status, l()),
        &ns::numbers_sip_address_count(l(), count),
    )
}

/// The passcode sender's state in words.
pub fn verify_state(enabled: bool) -> &'static str {
    if enabled {
        ns::numbers_verify_on(l())
    } else {
        ns::numbers_verify_off(l())
    }
}

/// A lookup's answer as lines: the number, its country, line type and
/// carrier when the carrier said, or one line saying it did not recognise
/// the number (which was still charged).
fn lookup_lines(info: NumberLookupInfo) -> Vec<String> {
    if !info.valid {
        return vec![ns::numbers_lookup_invalid(l()).to_owned()];
    }
    std::iter::once(Some(info.phone_number))
        .chain([info.country, info.line_type, info.carrier])
        .flatten()
        .map(|line| line.trim().to_owned())
        .filter(|line| !line.is_empty())
        .collect()
}

/// `value` trimmed, or `None` when blank.
fn optional(value: &str) -> Option<String> {
    Some(value.trim().to_owned()).filter(|value| !value.is_empty())
}

/// Whether every one of `values` holds more than white space.
fn all_typed(values: &[&str]) -> bool {
    values.iter().all(|value| !value.trim().is_empty())
}

/// The SIP trunk form.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SipTrunkForm {
    /// The trunk's name.
    pub name: String,
    /// The domain's label only: the carrier adds `.sip.twilio.com`.
    pub domain: String,
    /// The allowed IPv4 addresses or ranges, one per line.
    pub ip_addresses: String,
}

impl SipTrunkForm {
    /// The addresses typed, one per line, blanks dropped.
    pub fn addresses(&self) -> Vec<String> {
        self.ip_addresses
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// Whether "Create trunk" works: a name, a domain and an address. The
    /// service checks the rest and says what is wrong.
    pub fn is_submittable(&self) -> bool {
        all_typed(&[&self.name, &self.domain]) && !self.addresses().is_empty()
    }

    fn draft(&self) -> SipTrunkDraft {
        SipTrunkDraft {
            name: self.name.trim().to_owned(),
            domain: self.domain.trim().to_owned(),
            ip_access_control_list: self.addresses(),
        }
    }
}

/// The A2P 10DLC form.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct A2pForm {
    /// The business's legal name.
    pub business_name: String,
    /// Its tax or company number. Optional.
    pub ein: String,
    /// Its website. Optional.
    pub website: String,
    /// Its industry. Optional.
    pub vertical: String,
    /// One of [`A2pForm::CAMPAIGN_TYPES`]. It decides the brand and the price.
    pub campaign_type: String,
    /// What the business will send.
    pub campaign_description: String,
    /// An example message.
    pub sample_one: String,
    /// Another example message.
    pub sample_two: String,
}

impl Default for A2pForm {
    fn default() -> Self {
        Self {
            business_name: String::new(),
            ein: String::new(),
            website: String::new(),
            vertical: String::new(),
            campaign_type: A2P_CAMPAIGN_TYPES[0].to_owned(),
            campaign_description: String::new(),
            sample_one: String::new(),
            sample_two: String::new(),
        }
    }
}

impl A2pForm {
    /// The campaign types to choose from, as the service names them. A closed
    /// list, because anything else is filed under the wrong use case.
    pub const CAMPAIGN_TYPES: [&'static str; 4] = A2P_CAMPAIGN_TYPES;

    /// Whether "Submit registration" works: the fields the service requires.
    pub fn is_submittable(&self) -> bool {
        all_typed(&[
            &self.business_name,
            &self.campaign_description,
            &self.sample_one,
            &self.sample_two,
        ])
    }

    fn draft(&self) -> A2pRegistrationDraft {
        A2pRegistrationDraft {
            business_name: self.business_name.trim().to_owned(),
            ein: optional(&self.ein),
            website: optional(&self.website),
            vertical: optional(&self.vertical),
            campaign_type: self.campaign_type.clone(),
            campaign_description: self.campaign_description.clone(),
            sample_message_1: self.sample_one.clone(),
            sample_message_2: self.sample_two.clone(),
        }
    }
}

/// The toll-free verification form.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TollFreeForm {
    /// The toll-free number.
    pub phone_number: String,
    /// The business's name.
    pub business_name: String,
    /// Its website. Optional.
    pub website: String,
    /// One of [`TollFreeForm::use_cases`]' values.
    pub use_case: String,
    /// An example message. Optional: the service writes one when left blank.
    pub message_content: String,
    /// How people agree to the messages. Optional likewise.
    pub opt_in_flow: String,
    /// A public link to the business's own opt-in form or a screenshot of it.
    pub opt_in_image_url: String,
}

impl Default for TollFreeForm {
    fn default() -> Self {
        Self {
            phone_number: String::new(),
            business_name: String::new(),
            website: String::new(),
            use_case: TOLL_FREE_USE_CASES[0].to_owned(),
            message_content: String::new(),
            opt_in_flow: String::new(),
            opt_in_image_url: String::new(),
        }
    }
}

impl TollFreeForm {
    /// The use cases to choose from: the value sent, and its label.
    pub fn use_cases() -> [(&'static str, &'static str); 4] {
        [
            (
                TOLL_FREE_USE_CASES[0],
                ns::numbers_tfv_use_case_customer_support(l()),
            ),
            (
                TOLL_FREE_USE_CASES[1],
                ns::numbers_tfv_use_case_marketing(l()),
            ),
            (
                TOLL_FREE_USE_CASES[2],
                ns::numbers_tfv_use_case_notifications(l()),
            ),
            (TOLL_FREE_USE_CASES[3], ns::numbers_tfv_use_case_otp(l())),
        ]
    }

    /// Whether "Submit verification" works: a number, a business and the
    /// opt-in link, which the service requires.
    pub fn is_submittable(&self) -> bool {
        all_typed(&[
            &self.phone_number,
            &self.business_name,
            &self.opt_in_image_url,
        ])
    }

    fn draft(&self) -> TollFreeVerificationDraft {
        TollFreeVerificationDraft {
            phone_number: self.phone_number.trim().to_owned(),
            business_name: self.business_name.trim().to_owned(),
            website: optional(&self.website),
            use_case: self.use_case.clone(),
            message_content: optional(&self.message_content),
            opt_in_flow: optional(&self.opt_in_flow),
            opt_in_image_urls: optional(&self.opt_in_image_url).into_iter().collect(),
        }
    }
}

/// What the member does to manage numbers on the phone numbers screen.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NumberAdminEvent {
    /// Reapply this held number's routing. Not confirmed: it converges.
    Configure(String),
    /// Ask before releasing this held number.
    AskRelease(String),
    /// The number to look up changed.
    EditLookup(String),
    /// Ask before looking up the typed number.
    AskLookup,
    /// The SIP trunk form changed.
    EditSipTrunk(SipTrunkForm),
    /// Create the SIP trunk. Billed monthly; the form says so before.
    CreateSipTrunk,
    /// Turn the one-time passcode sender on or off.
    SetVerify(bool),
    /// The A2P 10DLC form changed.
    EditA2p(A2pForm),
    /// Ask before filing the A2P 10DLC registration.
    AskA2p,
    /// The toll-free form changed.
    EditTollFree(TollFreeForm),
    /// Ask before filing the toll-free verification.
    AskTollFree,
    /// Go ahead with the confirmation asked.
    Confirm,
    /// Close the confirmation. Refused while its write is on its way.
    CancelConfirmation,
}

impl SignedIn {
    /// Reads what the carrier tab shows, on opening it and at a refresh.
    pub(crate) fn enter_carrier_tab(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let workspace_id = self.workspace_id();
        let admin = &mut self.marketplace.admin;
        admin.carrier = CarrierStatus::Loading;
        admin.sip = SipTrunks::Loading;
        admin.verify = VerifyService::Loading;
        [
            (Slot::NumberCarrier, NumberCall::CarrierStatus),
            (Slot::NumberSip, NumberCall::SipTrunks),
            (Slot::NumberVerify, NumberCall::VerifyService),
        ]
        .into_iter()
        .map(|(slot, call)| Effect::NumberAdmin {
            ticket: tickets.issue(slot),
            workspace_id: workspace_id.clone(),
            call,
        })
        .collect()
    }

    /// Whether `phone_number` is a number the workspace holds on its own
    /// carrier account, so it can be reconfigured or released.
    fn administrable(&self, phone_number: &str) -> bool {
        matches!(&self.marketplace.owned, OwnedNumbersList::Ready(owned)
            if owned.numbers.iter().any(|n| n.phone_number == phone_number && !n.managed))
    }

    pub(crate) fn number_admin_event(
        &mut self,
        event: NumberAdminEvent,
        tickets: &mut Tickets,
    ) -> Next {
        if !self.capabilities().can_change {
            return stay();
        }
        let workspace_id = self.workspace_id();
        let call = match event {
            NumberAdminEvent::Configure(number)
                if self.administrable(&number)
                    && self.marketplace.admin.number_write.is_armed() =>
            {
                let admin = &mut self.marketplace.admin;
                admin.number_write = NumberWrite::Running;
                admin.number_written = Some(number.clone());
                Some((Slot::NumberWrite, NumberCall::Configure(number)))
            }
            NumberAdminEvent::AskRelease(number) if self.administrable(&number) => {
                self.ask(NumberConfirmation::Release {
                    phone_number: number,
                });
                None
            }
            NumberAdminEvent::EditLookup(number) => {
                self.marketplace.admin.lookup_number = number;
                None
            }
            NumberAdminEvent::AskLookup => {
                if let Some(phone_number) = optional(&self.marketplace.admin.lookup_number) {
                    self.ask(NumberConfirmation::Lookup { phone_number });
                }
                None
            }
            NumberAdminEvent::EditSipTrunk(form) => {
                self.marketplace.admin.sip_form = form;
                None
            }
            NumberAdminEvent::CreateSipTrunk => {
                let admin = &mut self.marketplace.admin;
                (admin.carrier_write.is_armed() && admin.sip_form.is_submittable()).then(|| {
                    admin.carrier_write = NumberWrite::Running;
                    (
                        Slot::NumberCarrierWrite,
                        NumberCall::CreateSipTrunk(admin.sip_form.draft()),
                    )
                })
            }
            NumberAdminEvent::SetVerify(enabled) => {
                let admin = &mut self.marketplace.admin;
                admin.carrier_write.is_armed().then(|| {
                    admin.carrier_write = NumberWrite::Running;
                    (
                        Slot::NumberCarrierWrite,
                        NumberCall::SetVerifyService(enabled),
                    )
                })
            }
            NumberAdminEvent::EditA2p(form) => {
                self.marketplace.admin.a2p_form = form;
                None
            }
            NumberAdminEvent::AskA2p => {
                if self.marketplace.admin.a2p_form.is_submittable() {
                    self.ask(NumberConfirmation::A2p);
                }
                None
            }
            NumberAdminEvent::EditTollFree(form) => {
                self.marketplace.admin.toll_free_form = form;
                None
            }
            NumberAdminEvent::AskTollFree => {
                if self.marketplace.admin.toll_free_form.is_submittable() {
                    self.ask(NumberConfirmation::TollFree);
                }
                None
            }
            NumberAdminEvent::Confirm => self.confirmed(),
            NumberAdminEvent::CancelConfirmation => {
                let admin = &mut self.marketplace.admin;
                if let Some(confirmation) = &admin.confirmation
                    && !admin.write_for(confirmation).is_running()
                {
                    admin.confirmation = None;
                }
                None
            }
            NumberAdminEvent::Configure(_) | NumberAdminEvent::AskRelease(_) => None,
        };
        Next::Stay(
            call.map(|(slot, call)| Effect::NumberAdmin {
                ticket: tickets.issue(slot),
                workspace_id,
                call,
            })
            .into_iter()
            .collect(),
        )
    }

    /// Opens `confirmation`, unless its write may not be pressed now.
    fn ask(&mut self, confirmation: NumberConfirmation) {
        let admin = &mut self.marketplace.admin;
        if admin.write_for(&confirmation).is_armed() {
            admin.confirmation = Some(confirmation);
        }
    }

    /// The member went ahead with the confirmation asked: the call it commits,
    /// asked again at the answer, because the write may have started meanwhile.
    /// The confirmation stays open while the write is on its way, to report it.
    fn confirmed(&mut self) -> Option<(Slot, NumberCall)> {
        let admin = &mut self.marketplace.admin;
        let confirmation = admin.confirmation.clone()?;
        if !admin.write_for(&confirmation).is_armed() {
            return None;
        }
        Some(match confirmation {
            NumberConfirmation::Release { phone_number } => {
                admin.number_write = NumberWrite::Running;
                admin.number_written = Some(phone_number.clone());
                (Slot::NumberWrite, NumberCall::Release(phone_number))
            }
            NumberConfirmation::Lookup { phone_number } => {
                admin.lookup_write = NumberWrite::Running;
                (Slot::NumberLookup, NumberCall::Lookup(phone_number))
            }
            NumberConfirmation::A2p => {
                admin.carrier_write = NumberWrite::Running;
                (
                    Slot::NumberCarrierWrite,
                    NumberCall::RegisterA2p(admin.a2p_form.draft()),
                )
            }
            NumberConfirmation::TollFree => {
                admin.carrier_write = NumberWrite::Running;
                (
                    Slot::NumberCarrierWrite,
                    NumberCall::VerifyTollFree(admin.toll_free_form.draft()),
                )
            }
        })
    }

    /// A number management call answered.
    pub(crate) fn number_admin_done(
        &mut self,
        ticket: Ticket,
        result: Result<NumberAnswer, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        let admin = &mut self.marketplace.admin;
        if tickets.accept(Slot::NumberCarrier, ticket) {
            admin.carrier = match result {
                Ok(NumberAnswer::CarrierStatus(status))
                    if status.providers.as_ref().is_some_and(|p| !p.is_empty()) =>
                {
                    CarrierStatus::Ready(status)
                }
                Ok(_) => CarrierStatus::NotConnected,
                Err(error) => CarrierStatus::Failed(FailureText::from_api_error(&error)),
            };
        } else if tickets.accept(Slot::NumberSip, ticket) {
            admin.sip = match result {
                Ok(NumberAnswer::SipTrunks(answer)) => SipTrunks::Ready(answer.trunks),
                Ok(_) => SipTrunks::Failed(FailureText::unexpected()),
                Err(error) => SipTrunks::Failed(FailureText::from_api_error(&error)),
            };
        } else if tickets.accept(Slot::NumberVerify, ticket) {
            admin.verify = match result {
                Ok(NumberAnswer::VerifyService(answer)) => VerifyService::Ready {
                    enabled: answer.enabled,
                },
                Ok(_) => VerifyService::Failed(FailureText::unexpected()),
                Err(error) => VerifyService::Failed(FailureText::from_api_error(&error)),
            };
        } else if tickets.accept(Slot::NumberWrite, ticket) {
            return self.number_written(result, tickets);
        } else if tickets.accept(Slot::NumberLookup, ticket) {
            admin.close_confirmation(|c| matches!(c, NumberConfirmation::Lookup { .. }));
            admin.lookup_write = NumberWrite::after(result, Repeat::Once, |answer| {
                if let NumberAnswer::LookedUp(found) = answer {
                    admin.lookup_result = Some(lookup_lines(found.info));
                }
                NumberWrite::Idle
            });
        } else if tickets.accept(Slot::NumberCarrierWrite, ticket) {
            return self.carrier_written(result, tickets);
        }
        stay()
    }

    /// A number's routing was reapplied, or it was released. A release reads
    /// the held numbers again: the one released is gone from them.
    fn number_written(
        &mut self,
        result: Result<NumberAnswer, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        let admin = &mut self.marketplace.admin;
        // A release runs from its confirmation, which stays open until it lands;
        // reapplying routing has none.
        let released =
            admin.close_confirmation(|c| matches!(c, NumberConfirmation::Release { .. }));
        let repeat = if released {
            Repeat::Once
        } else {
            Repeat::Converges
        };
        let mut reread = false;
        admin.number_write = NumberWrite::after(result, repeat, |answer| match answer {
            NumberAnswer::Released(answer) => {
                reread = true;
                NumberWrite::Done(if answer.warnings.is_empty() {
                    ns::numbers_release_done(l()).to_owned()
                } else {
                    ns::numbers_release_warning(l(), &answer.warnings.join(" "))
                })
            }
            _ => NumberWrite::Done(ns::numbers_configure_done(l()).to_owned()),
        });
        if reread {
            return Next::Stay(self.enter_marketplace(tickets));
        }
        stay()
    }

    /// A trunk was created, the passcode sender set, or a registration filed.
    fn carrier_written(
        &mut self,
        result: Result<NumberAnswer, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        let workspace_id = self.workspace_id();
        let admin = &mut self.marketplace.admin;
        admin.close_confirmation(|c| {
            matches!(c, NumberConfirmation::A2p | NumberConfirmation::TollFree)
        });
        let mut effects = Vec::new();
        admin.carrier_write = NumberWrite::after(result, Repeat::Once, |answer| match answer {
            NumberAnswer::SipTrunkCreated(_) => {
                admin.sip_form = SipTrunkForm::default();
                admin.sip = SipTrunks::Loading;
                effects.push(Effect::NumberAdmin {
                    ticket: tickets.issue(Slot::NumberSip),
                    workspace_id,
                    call: NumberCall::SipTrunks,
                });
                NumberWrite::Idle
            }
            NumberAnswer::VerifyService(answer) => {
                admin.verify = VerifyService::Ready {
                    enabled: answer.enabled,
                };
                NumberWrite::Idle
            }
            NumberAnswer::A2pRegistered(answer) => {
                NumberWrite::Done(ns::numbers_a2p_submitted(l(), &answer.status))
            }
            NumberAnswer::TollFreeFiled(answer) => {
                NumberWrite::Done(ns::numbers_tfv_submitted(l(), &answer.status))
            }
            _ => NumberWrite::Failed {
                failure: FailureText::unexpected(),
                repeatable: false,
            },
        });
        Next::Stay(effects)
    }
}
