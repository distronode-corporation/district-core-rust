//! Booking pages: whether the workspace has them and where they stand, turning
//! them on, the way into their administration (whose sections are
//! [`scheduling_admin`](crate::scheduling_admin), listed once the pages are
//! live), and the web dashboard for whatever this app does not draw.
//!
//! Whether "Enable" is offered is the service's answer (the status says whether
//! this member may), never worked out from a role here: a copy that failed
//! closed would hide the button from an owner the service would admit. Turning
//! them on reaches two other services and has a small hourly budget per
//! workspace, so it is one press at a time and never repeated by the app. Its
//! answer is re-read from the status either way, because the status is the
//! truth; a setup that ran and failed is an ordinary answer carrying its reason.
//!
//! The web dashboard is reached through a hand-off: a link the service
//! mints on request that signs the browser in, good for one use within a minute.
//! It is asked for when the member asks to go, checked to be on the service's
//! own address, opened at once, and then gone. It is never kept in the state,
//! never logged, and never printed in `Debug` ([`OneTimeUrl`]).
//!
//! The link is bound to the browser that will open it ([`HandOffLeg`]). The
//! press first opens the service's start page in the browser with a fresh
//! `state`; the browser answers through `districtai://handoff` with a nonce the
//! service also left in that browser, and the link is asked for with that
//! nonce, so only that browser can redeem it. A browser that has not answered
//! after [`HAND_OFF_CALLBACK_WAIT`] gives the hand-off up, with a notice; the
//! link is never asked for unbound, and an answer that arrives after that is
//! dropped.

use std::fmt;
use std::time::Duration;

use district_api::ApiError;
use district_auth::{HAND_OFF_START_PATH, HandOffNonce, HandOffState};
use district_model::{
    CODE_INVALID_NEXT, CODE_INVALID_NONCE, CODE_NONCE_REQUIRED, SchedulingEnableResponse,
    SchedulingHandOffResponse, SchedulingStatusResponse, SchedulingTenant,
};

use crate::Locale;
use crate::failure::FailureText;
use crate::model::{CoreConfig, Effect, Slot, Ticket, Tickets};
use crate::scheduling_admin::SchedulingSection;
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::{l, s};

/// Where the hand-off lands: the web dashboard's booking pages. The service
/// replaces anything that is not a path under `/dashboard` with its own default,
/// so a mistake here costs the deep link and nothing else.
pub const SCHEDULING_WEB_PATH: &str = "/dashboard/district/scheduling";

/// How long the browser has to answer the start page before the hand-off is
/// given up. It covers the person's time on the browser's "Open District AI?"
/// prompt, and a hand-off that runs out is pressed again rather than minted
/// unbound, so it is generous: thirty seconds, as District Core for Swift waits,
/// well inside the five minutes the service's nonce cookie lives.
pub const HAND_OFF_CALLBACK_WAIT: Duration = Duration::from_secs(30);

/// A link that carries a sign-in of its own, or a value that binds one (the
/// start page's `state`, the browser's nonce). It is opened or checked at once
/// and never kept, and its `Debug` output is redacted, so no `{:?}` of an
/// effect, an event or the model can put it in a log.
#[derive(Clone, PartialEq, Eq)]
pub struct OneTimeUrl(String);

impl OneTimeUrl {
    /// Wraps `url`.
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }

    /// The link, for the one call that opens it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for OneTimeUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OneTimeUrl(<redacted>)")
    }
}

/// The booking pages screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchedulingScreen {
    /// Where the booking pages stand.
    pub status: SchedulingStatus,
    /// Whether turning them on is on its way. One press at a time.
    pub enabling: bool,
    /// Where the hand-off to the web stands.
    pub hand_off: HandOffLeg,
    /// What the last press came to, when it needs saying: a setup that failed,
    /// or a refusal. Shown beside the card, never instead of it.
    pub notice: Option<FailureText>,
}

impl SchedulingScreen {
    /// Whether a hand-off to the web is under way, in either leg.
    pub fn opening(&self) -> bool {
        self.hand_off != HandOffLeg::Idle
    }

    /// Whether the hand-off link is being asked for. Pressing again then does
    /// nothing; while the browser is awaited, a press starts over.
    pub fn minting(&self) -> bool {
        self.hand_off == HandOffLeg::Minting
    }

    /// The administration's sections to list, in the web dashboard's order:
    /// all of them once the booking pages are live, and none before (every
    /// one would answer that there are no booking pages), nor while the status
    /// is unknown.
    pub fn sections(&self) -> Vec<SchedulingSection> {
        match self.status.presentation() {
            Some(SchedulingPresentation::Live(_)) => SchedulingSection::listed().to_vec(),
            _ => Vec::new(),
        }
    }

    /// The heading over the sections: "Manage".
    pub fn sections_heading() -> &'static str {
        district_l10n::scheduling_strings::sections_eyebrow(l())
    }
}

/// Where a hand-off to the web stands. One at a time: a press while the
/// browser is awaited starts a new one, and the old one's answer no longer
/// matches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum HandOffLeg {
    /// None under way.
    #[default]
    Idle,
    /// The start page was opened in the browser (or, for a purchase, in a view
    /// inside the app) with this `state`, and its answer is awaited, for
    /// [`HAND_OFF_CALLBACK_WAIT`] at most. Redacted in `Debug`.
    Browser(HandOffState),
    /// The link is being asked for.
    Minting,
}

/// Where the booking pages stand, as read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SchedulingStatus {
    /// Never read.
    #[default]
    NotLoaded,
    /// Being read, with nothing to show yet.
    Loading,
    /// Read.
    Ready {
        /// What the service sent.
        status: SchedulingStatusResponse,
        /// Whether it is being read again, with this still showing.
        refreshing: bool,
    },
    /// The read failed. Never shown as a workspace without booking pages, which
    /// is an ordinary answer of its own.
    Failed(FailureText),
}

impl SchedulingStatus {
    /// The heading for a failed read.
    pub const FAILED_TITLE: &'static str = district_l10n::english::SCHEDULING_FAILED_TITLE;

    /// What the card shows, once read.
    pub fn presentation(&self) -> Option<SchedulingPresentation> {
        match self {
            Self::Ready { status, .. } => Some(SchedulingPresentation::of(status)),
            _ => None,
        }
    }
}

/// What the booking pages card shows.
///
/// A workspace with no booking pages is two different things, one offered them
/// and one not, and they are kept apart: one gets a button, the other a sentence.
/// Where there are booking pages, their state decides, whether or not the
/// workspace is still offered them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchedulingPresentation {
    /// The workspace is not offered booking pages. No button and no way on.
    NotOffered,
    /// Offered, never set up: the state of every workspace before the first
    /// press.
    NotSetUp,
    /// Being set up. The service retries a stalled setup by itself.
    Provisioning(SchedulingTenant),
    /// Live.
    Live(SchedulingTenant),
    /// The last setup failed. Not final: the service retries, and enabling again
    /// is the way to retry by hand.
    SetupFailed(SchedulingTenant),
    /// Switched off by a person. Nothing turns them back on from here.
    SwitchedOff(SchedulingTenant),
    /// A state this build does not know. The booking pages exist; what they are
    /// doing cannot be said. Not a failure, and not a workspace without them.
    Unknown(SchedulingTenant),
}

impl SchedulingPresentation {
    fn of(status: &SchedulingStatusResponse) -> Self {
        let Some(tenant) = status.tenant.clone() else {
            return if status.eligible {
                Self::NotSetUp
            } else {
                Self::NotOffered
            };
        };
        match tenant.status.as_str() {
            "provisioning" => Self::Provisioning(tenant),
            "ready" => Self::Live(tenant),
            "error" => Self::SetupFailed(tenant),
            "disabled" => Self::SwitchedOff(tenant),
            _ => Self::Unknown(tenant),
        }
    }

    /// The card's main sentence.
    pub fn message(&self) -> &'static str {
        match self {
            Self::NotOffered => s::scheduling_not_offered(l()),
            Self::NotSetUp => s::scheduling_not_set_up(l()),
            Self::Provisioning(_) => s::scheduling_provisioning(l()),
            Self::Live(tenant) if tenant.booking_url.is_none() => {
                s::scheduling_live_without_url(l())
            }
            Self::Live(_) => s::scheduling_live(l()),
            Self::SetupFailed(_) => s::scheduling_setup_failed(l()),
            Self::SwitchedOff(_) => s::scheduling_switched_off(l()),
            Self::Unknown(_) => s::scheduling_unknown(l()),
        }
    }

    /// Whether to offer "Enable": only where turning them on is the next step,
    /// and only when the service says the workspace is offered them and this
    /// member may. Not for pages a person switched off, and not for a state this
    /// build does not know.
    pub fn offers_enable(&self, status: &SchedulingStatusResponse) -> bool {
        status.eligible
            && status.can_manage
            && matches!(self, Self::NotSetUp | Self::SetupFailed(_))
    }

    /// Whether to offer "Refresh": only where reading again could change the
    /// answer. Nothing polls.
    pub fn offers_refresh(&self) -> bool {
        matches!(self, Self::Provisioning(_) | Self::SetupFailed(_))
    }

    /// Whether to offer the web dashboard, the way to whatever this app does
    /// not draw.
    pub fn offers_web(&self) -> bool {
        matches!(self, Self::Live(_))
    }
}

/// What the member does on the booking pages screen. Reading the status again
/// is [`Event::Refresh`](crate::Event::Refresh).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SchedulingEvent {
    /// Turn booking pages on.
    Enable,
    /// Open the web dashboard's booking pages, signed in.
    ManageOnWeb,
    /// Dismiss the notice.
    DismissNotice,
}

impl SignedIn {
    /// Reads the status: on entering the screen, and at a refresh.
    pub(crate) fn enter_scheduling(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        match &mut self.scheduling.status {
            SchedulingStatus::Ready { refreshing, .. } => *refreshing = true,
            other => *other = SchedulingStatus::Loading,
        }
        vec![Effect::LoadSchedulingStatus {
            ticket: tickets.issue(Slot::SchedulingStatus),
            workspace_id: self.workspace_id(),
        }]
    }

    pub(crate) fn scheduling_event(
        &mut self,
        event: SchedulingEvent,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Next {
        let workspace_id = self.workspace_id();
        let screen = &mut self.scheduling;
        let effects = match (event, &screen.status) {
            (SchedulingEvent::Enable, SchedulingStatus::Ready { status, .. })
                if !screen.enabling && SchedulingPresentation::of(status).offers_enable(status) =>
            {
                screen.enabling = true;
                screen.notice = None;
                vec![Effect::EnableScheduling {
                    ticket: tickets.issue(Slot::SchedulingEnable),
                    workspace_id,
                }]
            }
            (SchedulingEvent::ManageOnWeb, SchedulingStatus::Ready { status, .. })
                if !screen.minting() && SchedulingPresentation::of(status).offers_web() =>
            {
                // A fresh `state` for every press, so a press while the browser
                // is awaited replaces that hand-off: its answer no longer
                // matches, and its wait's ticket is no longer awaited.
                let state = HandOffState::generate();
                let start =
                    config.web_url(&format!("{HAND_OFF_START_PATH}?state={}", state.as_str()));
                screen.hand_off = HandOffLeg::Browser(state);
                screen.notice = None;
                vec![
                    Effect::OpenOneTimeUrl {
                        url: OneTimeUrl::new(start),
                    },
                    Effect::Wait {
                        ticket: tickets.issue(Slot::SchedulingHandOffWait),
                        delay: HAND_OFF_CALLBACK_WAIT,
                    },
                ]
            }
            (SchedulingEvent::DismissNotice, _) => {
                screen.notice = None;
                Vec::new()
            }
            _ => Vec::new(),
        };
        Next::Stay(effects)
    }

    pub(crate) fn scheduling_status_loaded(
        &mut self,
        ticket: Ticket,
        result: Result<SchedulingStatusResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::SchedulingStatus, ticket) {
            return stay();
        }
        self.scheduling.status = match result {
            Ok(status) => SchedulingStatus::Ready {
                status,
                refreshing: false,
            },
            Err(error) => SchedulingStatus::Failed(FailureText::from_api_error(&error)),
        };
        Next::Stay(self.scheduling_status_arrived(tickets))
    }

    /// Turning them on finished, one way or another. The status is read again
    /// whatever the answer: a success has changed it, a refusal may mean the
    /// status showing is stale, and the status is the truth.
    pub(crate) fn scheduling_enabled(
        &mut self,
        ticket: Ticket,
        result: Result<SchedulingEnableResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::SchedulingEnable, ticket) {
            return stay();
        }
        self.scheduling.enabling = false;
        self.scheduling.notice = match result {
            Ok(answer) if answer.ok => None,
            // The service's reason is its own English: shown as it is in
            // English, and in French replaced by a sentence that says only
            // that it did not finish, never that no reason was given.
            Ok(answer) => Some(FailureText::final_(match answer.error {
                Some(reason) if l() == Locale::En => reason,
                Some(_) => s::failure_scheduling_setup_failed_reason_unshown(l()).to_owned(),
                None => s::failure_scheduling_setup_failed(l()).to_owned(),
            })),
            Err(ApiError::Forbidden(_)) => {
                Some(FailureText::final_(s::failure_scheduling_not_offered(l())))
            }
            Err(ApiError::RateLimited { .. }) => {
                Some(FailureText::retryable(s::failure_scheduling_too_many(l())))
            }
            Err(error) => Some(FailureText::from_api_error(&error)),
        };
        Next::Stay(self.enter_scheduling(tickets))
    }

    /// The desktop handed over a `districtai://handoff` link. If it answers the
    /// hand-off awaiting the browser, the link is asked for, bound by its nonce.
    /// Anything else is dropped and leaves that hand-off waiting for its own
    /// answer: a link with nothing awaiting it, one for another hand-off (an old
    /// one, or one an answer arrived after the wait gave up on), or one that is
    /// not well formed. A stray or forged link cannot cancel a hand-off.
    ///
    /// The same link answers a purchase's start page, whether that page showed
    /// in the browser or in a view inside the app: each hand-off has its own
    /// `state`, so a link answers one of them at most.
    pub(crate) fn hand_off_callback(&mut self, link: &OneTimeUrl, tickets: &mut Tickets) -> Next {
        if let Some(nonce) = answer_to(&self.scheduling.hand_off, link) {
            tickets.cancel(Slot::SchedulingHandOffWait);
            return Next::Stay(self.mint_hand_off(nonce, tickets));
        }
        if let Some(nonce) = answer_to(&self.purchase.leg, link) {
            return Next::Stay(self.mint_purchase(nonce, tickets));
        }
        stay()
    }

    /// The browser did not answer in time: the hand-off is given up and says
    /// so, and pressing again starts afresh. The link is never asked for
    /// unbound: a link with no nonce signs in whichever browser opens it, and
    /// the service refuses one once `HANDOFF_REQUIRE_NONCE` is on. The wait's
    /// ticket is awaited only while the browser is: an answer cancels it, and
    /// leaving the workspace forgets it.
    pub(crate) fn hand_off_unanswered(&mut self) {
        self.scheduling.hand_off = HandOffLeg::Idle;
        self.scheduling.notice = Some(FailureText::retryable(s::failure_hand_off_unanswered(l())));
    }

    /// No browser took a page. One that was to answer a hand-off never will, so
    /// that hand-off is given up rather than asked for unbound, which would
    /// open no browser either.
    pub(crate) fn hand_off_unopened(&mut self, tickets: &mut Tickets) {
        if matches!(self.scheduling.hand_off, HandOffLeg::Browser(_)) {
            tickets.cancel(Slot::SchedulingHandOffWait);
            self.scheduling.hand_off = HandOffLeg::Idle;
        }
    }

    /// The link, asked for bound to the browser by `nonce`: always one, so
    /// [`Effect::RequestSchedulingHandOff`]'s `nonce` is never `None` from here.
    fn mint_hand_off(&mut self, nonce: HandOffNonce, tickets: &mut Tickets) -> Vec<Effect> {
        self.scheduling.hand_off = HandOffLeg::Minting;
        vec![Effect::RequestSchedulingHandOff {
            ticket: tickets.issue(Slot::SchedulingHandOff),
            workspace_id: self.workspace_id(),
            nonce: Some(nonce),
        }]
    }

    /// The hand-off link arrived: open it at once, if it is on the service's own
    /// address, and keep nothing. It is opened as it came, in the same browser
    /// as the start page, which holds the cookie a bound link is redeemed with.
    pub(crate) fn scheduling_hand_off(
        &mut self,
        ticket: Ticket,
        result: Result<SchedulingHandOffResponse, ApiError>,
        tickets: &mut Tickets,
        config: &CoreConfig,
    ) -> Next {
        if !tickets.accept(Slot::SchedulingHandOff, ticket) {
            return stay();
        }
        let screen = &mut self.scheduling;
        screen.hand_off = HandOffLeg::Idle;
        match result {
            Ok(answer) if on_origin(&answer.url, config) => {
                return Next::Stay(vec![Effect::OpenOneTimeUrl {
                    url: OneTimeUrl::new(answer.url),
                }]);
            }
            Ok(_) => screen.notice = Some(hand_off_failure(None)),
            Err(error) => screen.notice = Some(hand_off_failure(Some(&error))),
        }
        stay()
    }
}

/// The nonce `link` carries, if it is the answer to the start page `leg`
/// awaits. Anything else is `None`, and leaves that hand-off waiting.
fn answer_to(leg: &HandOffLeg, link: &OneTimeUrl) -> Option<HandOffNonce> {
    let HandOffLeg::Browser(state) = leg else {
        return None;
    };
    state.check(link.expose()).ok()
}

/// What to say about a hand-off that failed: refused with `error`, or, with
/// `None`, answered with a link that is not on the service's own address.
pub(crate) fn hand_off_failure(error: Option<&ApiError>) -> FailureText {
    match error {
        None => FailureText::final_(s::failure_hand_off_elsewhere(l())),
        // On these routes a 403 is the sign-in the request carried, not the
        // role and not the workspace's offer.
        Some(ApiError::Forbidden(_)) => FailureText::final_(s::failure_hand_off_refused(l())),
        Some(ApiError::RateLimited { .. }) => {
            FailureText::retryable(s::failure_hand_off_too_many(l()))
        }
        // The service wants a bound hand-off and this one was not. This app
        // always binds both, so only a service that lost the nonce says so.
        // Pressing again binds it if the browser answers.
        Some(error) if error.code() == Some(CODE_NONCE_REQUIRED) => {
            FailureText::retryable(s::failure_hand_off_update_needed(l()))
        }
        Some(error) if error.code() == Some(CODE_INVALID_NONCE) => {
            FailureText::retryable(s::failure_hand_off_nonce_refused(l()))
        }
        // Only the billing hand-off refuses a destination; the scheduling one
        // falls back to its own.
        Some(error) if error.code() == Some(CODE_INVALID_NEXT) => {
            FailureText::final_(s::failure_hand_off_page_refused(l()))
        }
        Some(error) => FailureText::from_api_error(error),
    }
}

/// Whether `url` is on the service's own address, over HTTPS. A host test, not
/// a string test: the origin is compared with the `/` after it, so
/// `https://www.distronode.com.example` is not taken for the service. HTTPS is
/// checked on its own too, so a build pointed at a plain-HTTP address still
/// never sends this credential in the clear.
pub(crate) fn on_origin(url: &str, config: &CoreConfig) -> bool {
    let origin = format!("{}/", config.web_base_url.trim_end_matches('/'));
    starts_with_ignoring_case(url, "https://") && starts_with_ignoring_case(url, &origin)
}

fn starts_with_ignoring_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}
