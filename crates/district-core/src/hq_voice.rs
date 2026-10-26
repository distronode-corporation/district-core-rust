//! District HQ by voice: a session in an encrypted room with the voice agent,
//! the words both sides say, and a card for every change the agent proposes.
//!
//! # Starting
//!
//! A session starts only when an agency or client member asks for it, in a
//! build that can carry audio, while nothing else holds the call engine. The
//! first time in a workspace the member is told, before anything is asked
//! for, where their voice is processed ([`HqVoicePhase::Disclosing`]); the
//! microphone starts only after they accept. The credential is asked for once
//! per start and never again by itself: each one starts a billed voice
//! session. A credential without the room's passphrase is not joined at all,
//! never joined in the clear.
//!
//! # The session
//!
//! The agent joins the room by itself. What it is doing comes from its
//! `lk.agent.state` attribute; both sides' words come as transcription
//! streams and join the HQ conversation as they are spoken (the member's
//! too, which the agent transcribes), so a typed question afterwards carries
//! them as history. The session ends when the member stops it, when the agent
//! leaves or closes the room, when the connection is lost for good, or after
//! [`HQ_VOICE_SESSION_SECONDS`], whichever is first. It also ends when the
//! member leaves the HQ screen, as a meeting room does when the member leaves
//! the lobby: the microphone is never live without the controls that stop it
//! on screen.
//!
//! While the session is live, what is on screen (the page, and the record
//! open on it) is reported to it, for the agent to read, as the web
//! dashboard reports its own: once as the session starts, and again on each
//! change of screen ([`Route::hq_voice_page`]). Only ids leave this way, never
//! a name, a number or what was typed.
//!
//! # Changes
//!
//! A change the agent proposes is a card, keyed by its id, and nothing has
//! been changed. It is applied only by the member: by saying "confirm", which
//! the agent hears and acts on (it then sends the outcome, and the card goes),
//! or by clicking Confirm here, which sends the proposal back exactly as it
//! came, with its id. Cancel is the same, either way. Never by this app on its
//! own, and never twice: each card has one request in flight at most, and a
//! card the agent resolved while a click was in flight goes at once, the
//! click's late answer with it. A card stays after the session ends, because
//! the service still accepts a click on it for a while.

use std::time::Duration;

use district_api::ApiError;
use district_model::{
    HQ_PROPOSAL_TOPIC, HQ_VOICE_SESSION_SECONDS, HqAgentState, HqConfirmResponse,
    HqProposalCancelResponse, HqResolutionStatus, HqVoicePacket, HqVoiceProposal, HqVoiceScreen,
    HqVoiceSelection, HqVoiceTokenResponse, read_packet,
};

use crate::failure::FailureText;
use crate::hq::{HqAuthor, HqMessage, HqNote, HqText};
use crate::media::{
    DisconnectReason, MediaCredential, MediaEvent, MediaOwner, MediaSession, Speaker, SpeechSegment,
};
use crate::model::{Effect, Slot, Ticket, Tickets};
#[cfg(doc)]
use crate::route::Route;
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::{l, s};

/// The refusal code of a proposal already confirmed or cancelled, by voice or
/// by another click.
const CODE_ALREADY_RESOLVED: &str = "already_resolved";
/// The refusal code of a proposal too old to confirm.
const CODE_EXPIRED: &str = "expired";
/// The refusal code of a proposal this member cannot see.
const CODE_NOT_FOUND: &str = "not_found";
/// The refusal code of a confirmation that does not match its proposal.
const CODE_INVALID_REQUEST: &str = "invalid_request";

/// District HQ's voice session, and the changes it proposed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HqVoice {
    /// Where the session stands.
    pub phase: HqVoicePhase,
    /// The changes proposed by voice and not yet resolved, oldest first.
    pub cards: Vec<HqVoiceCard>,
    /// Why the last card went without a result: the service no longer knows
    /// it, or refused it as asked. Shown until dismissed.
    pub notice: Option<FailureText>,
    /// Whether the member accepted the residency statement in this workspace.
    disclosed: bool,
    /// Whether an agent has been in this session's room.
    agent_seen: bool,
    /// The last session's room, and what was last reported to it.
    room: Option<VoiceRoom>,
}

/// A session's room, as its credential named it, and what was reported to it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct VoiceRoom {
    name: String,
    /// How many screens were reported to it.
    reports: u64,
    /// The last screen reported: its page and the record open on it.
    last: Option<(&'static str, Option<HqVoiceSelection>)>,
}

impl HqVoice {
    /// Whether a session's credential is being asked for.
    pub(crate) fn minting(&self) -> bool {
        self.phase == HqVoicePhase::Minting
    }

    /// The card for `proposal_id`, while it is on screen.
    pub fn card(&self, proposal_id: &str) -> Option<&HqVoiceCard> {
        self.cards
            .iter()
            .find(|card| card.proposal.proposal_id == proposal_id)
    }

    fn card_mut(&mut self, proposal_id: &str) -> Option<&mut HqVoiceCard> {
        self.cards
            .iter_mut()
            .find(|card| card.proposal.proposal_id == proposal_id)
    }

    /// Takes `proposal_id`'s card off the screen.
    fn remove(&mut self, proposal_id: &str) -> Option<HqVoiceCard> {
        let index = self
            .cards
            .iter()
            .position(|card| card.proposal.proposal_id == proposal_id)?;
        Some(self.cards.remove(index))
    }
}

/// Where an HQ voice session stands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum HqVoicePhase {
    /// No session.
    #[default]
    Off,
    /// The residency statement is shown, and nothing has been asked for.
    Disclosing,
    /// The session's credential is being asked for.
    Minting,
    /// The room is joined or being joined: see
    /// [`SignedIn::hq_voice_session`] for the connection and the microphone.
    Live {
        /// What the agent is doing, or `None` while it has not joined or its
        /// state is one this build does not know.
        agent: Option<HqAgentState>,
    },
    /// The session ended without the member stopping it.
    Ended(HqVoiceEnd),
    /// The session could not start.
    Failed(FailureText),
}

/// Why a session the member did not stop ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HqVoiceEnd {
    /// The agent left, or closed the room.
    AgentLeft,
    /// The session reached [`HQ_VOICE_SESSION_SECONDS`].
    TimeLimit,
    /// The media connection ended for this reason.
    Media(DisconnectReason),
}

impl HqVoiceEnd {
    /// What to tell the member, or `None` when there is nothing worth saying.
    pub fn message(self) -> Option<&'static str> {
        match self {
            Self::AgentLeft => Some(s::hq_voice_ended_agent(l())),
            Self::TimeLimit => Some(s::hq_voice_ended_time_limit(l())),
            Self::Media(reason) => reason.message(),
        }
    }
}

/// A change proposed by voice, on screen until it is resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HqVoiceCard {
    /// The change, exactly as the agent sent it.
    pub proposal: HqVoiceProposal,
    /// What is happening to it.
    pub state: HqCardState,
}

impl HqVoiceCard {
    /// The sentence to show: the service's own, or this app's when it sent
    /// none.
    pub fn summary(&self) -> &str {
        if self.proposal.summary.trim().is_empty() {
            s::hq_voice_fallback_summary(l())
        } else {
            &self.proposal.summary
        }
    }

    /// Whether Confirm and Cancel work: nothing is in flight for it.
    pub fn is_actionable(&self) -> bool {
        matches!(self.state, HqCardState::Open | HqCardState::Failed { .. })
    }
}

/// What is happening to a card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HqCardState {
    /// Waiting for the member.
    Open,
    /// A click Confirm is in flight.
    Confirming,
    /// A click Cancel is in flight.
    Cancelling,
    /// The last click failed and the card stays, for the member to decide
    /// again. After a confirmation the change may have been made before the
    /// answer was lost.
    Failed {
        /// Why.
        failure: FailureText,
        /// Whether it was a confirmation, which may have taken effect.
        confirming: bool,
    },
}

/// What the voice controls may offer now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HqVoiceControls {
    /// Whether the member may start a session: an agency or client member, in
    /// a build with calls, with no session, call, room or audition under way.
    pub can_start: bool,
    /// Whether a session is being started or is under way, to stop.
    pub can_stop: bool,
    /// Whether the member may confirm a card by click. Cancelling needs no
    /// more than seeing it.
    pub can_confirm: bool,
}

impl SignedIn {
    /// What the voice controls may offer now.
    pub fn hq_voice_controls(&self) -> HqVoiceControls {
        let can_change = self.capabilities().can_change;
        let idle = matches!(
            self.hq.voice.phase,
            HqVoicePhase::Off | HqVoicePhase::Ended(_) | HqVoicePhase::Failed(_)
        );
        HqVoiceControls {
            can_start: can_change && self.calls_available && idle && !self.media_busy(),
            can_stop: matches!(
                self.hq.voice.phase,
                HqVoicePhase::Minting | HqVoicePhase::Live { .. }
            ),
            can_confirm: can_change,
        }
    }

    /// The voice session's room, for its connection and its microphone.
    pub fn hq_voice_session(&self) -> Option<&MediaSession> {
        self.media
            .as_ref()
            .filter(|media| media.owner == MediaOwner::HqVoice)
    }

    /// The member pressed the voice button: the residency statement the first
    /// time in this workspace, the session itself after that.
    pub(crate) fn start_hq_voice(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if !self.hq_voice_controls().can_start {
            return Vec::new();
        }
        self.hq.voice.notice = None;
        if self.hq.voice.disclosed {
            self.mint_hq_voice(tickets)
        } else {
            self.hq.voice.phase = HqVoicePhase::Disclosing;
            Vec::new()
        }
    }

    /// The member accepted the residency statement: the session starts.
    pub(crate) fn accept_hq_voice_disclosure(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if self.hq.voice.phase != HqVoicePhase::Disclosing {
            return Vec::new();
        }
        self.hq.voice.phase = HqVoicePhase::Off;
        self.hq.voice.disclosed = true;
        // Whatever took the engine meanwhile (a call answered from its
        // notification) still comes first.
        self.start_hq_voice(tickets)
    }

    /// The member closed the residency statement: nothing starts.
    pub(crate) fn decline_hq_voice_disclosure(&mut self) {
        if self.hq.voice.phase == HqVoicePhase::Disclosing {
            self.hq.voice.phase = HqVoicePhase::Off;
        }
    }

    /// Asks for a session's credential, with the conversation so far.
    fn mint_hq_voice(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.hq.voice.phase = HqVoicePhase::Minting;
        vec![Effect::RequestHqVoice {
            ticket: tickets.issue(Slot::HqVoiceToken),
            workspace_id: self.workspace_id(),
            locale: l(),
            history: self.hq.history(),
        }]
    }

    /// Ends the session, whatever stage it is at, at the member's word or
    /// because the workspace, the session or the desktop is going away.
    pub(crate) fn stop_hq_voice(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        tickets.cancel(Slot::HqVoiceToken);
        tickets.cancel(Slot::HqVoiceLimit);
        if matches!(
            self.hq.voice.phase,
            HqVoicePhase::Minting | HqVoicePhase::Live { .. } | HqVoicePhase::Disclosing
        ) {
            self.hq.voice.phase = HqVoicePhase::Off;
        }
        self.leave_media_of(MediaOwner::HqVoice)
    }

    /// The member dismissed what the voice controls were saying.
    pub(crate) fn dismiss_hq_voice_notice(&mut self) {
        self.hq.voice.notice = None;
        if matches!(
            self.hq.voice.phase,
            HqVoicePhase::Ended(_) | HqVoicePhase::Failed(_)
        ) {
            self.hq.voice.phase = HqVoicePhase::Off;
        }
    }

    pub(crate) fn hq_voice_issued(
        &mut self,
        ticket: Ticket,
        result: Result<HqVoiceTokenResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::HqVoiceToken, ticket) {
            return stay();
        }
        let credential = match result {
            Ok(credential) if credential.is_joinable() => credential,
            Ok(_) => {
                let failure = FailureText::final_(s::hq_voice_unencrypted(l()));
                self.hq.voice.phase = HqVoicePhase::Failed(failure);
                return stay();
            }
            Err(error) => {
                self.hq.voice.phase = HqVoicePhase::Failed(FailureText::from_api_error(&error));
                return stay();
            }
        };
        self.hq.voice.phase = HqVoicePhase::Live { agent: None };
        self.hq.voice.agent_seen = false;
        self.hq.voice.room = Some(VoiceRoom {
            name: credential.room_name.clone(),
            reports: 0,
            last: None,
        });
        let media = MediaCredential::from_hq_voice(&credential);
        let mut effects = self.start_media(MediaOwner::HqVoice, media, true, tickets);
        effects.push(Effect::Wait {
            ticket: tickets.issue(Slot::HqVoiceLimit),
            delay: Duration::from_secs(HQ_VOICE_SESSION_SECONDS),
        });
        effects.extend(self.report_hq_voice_screen());
        Next::Stay(effects)
    }

    /// What is on screen, for a live session, when it is not what was last
    /// reported to it: the first report of a session is version 1, and each
    /// one after it one more. Nothing for a session that is not live, or a
    /// screen with no page in the service's table.
    pub(crate) fn report_hq_voice_screen(&mut self) -> Vec<Effect> {
        let workspace_id = self.workspace_id();
        let voice = &mut self.hq.voice;
        let (HqVoicePhase::Live { .. }, Some(room), Some((page, selected))) = (
            &voice.phase,
            voice.room.as_mut(),
            self.route.hq_voice_page(),
        ) else {
            return Vec::new();
        };
        let now = Some((page, selected.clone()));
        if room.last == now {
            return Vec::new();
        }
        room.last = now;
        room.reports += 1;
        let screen = HqVoiceScreen {
            version: room.reports,
            page: page.to_owned(),
            selected,
        };
        vec![Effect::ReportHqVoiceScreen {
            workspace_id,
            room_name: room.name.clone(),
            screen,
        }]
    }

    /// The session's time is up. The agent closes it at the same moment; this
    /// is so that the app says why, whichever is first.
    ///
    /// Its wait is cancelled whenever the session ends otherwise, so the session
    /// is live when it fires.
    pub(crate) fn hq_voice_time_up(&mut self) -> Vec<Effect> {
        self.hq.voice.phase = HqVoicePhase::Ended(HqVoiceEnd::TimeLimit);
        self.leave_media_of(MediaOwner::HqVoice)
    }

    /// The people in the room changed: an agent that was there and is not
    /// any more has ended the session.
    pub(crate) fn hq_voice_people_changed(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let present = self
            .hq_voice_session()
            .is_some_and(MediaSession::service_present);
        if present {
            self.hq.voice.agent_seen = true;
            return Vec::new();
        }
        if !self.hq.voice.agent_seen {
            return Vec::new();
        }
        tickets.cancel(Slot::HqVoiceLimit);
        self.hq.voice.phase = HqVoicePhase::Ended(HqVoiceEnd::AgentLeft);
        self.leave_media_of(MediaOwner::HqVoice)
    }

    /// The room ended under the session.
    pub(crate) fn hq_voice_media_ended(&mut self, reason: DisconnectReason, tickets: &mut Tickets) {
        tickets.cancel(Slot::HqVoiceLimit);
        let end = match reason {
            // The agent deletes the room when it closes the session.
            DisconnectReason::RoomEnded => HqVoiceEnd::AgentLeft,
            other => HqVoiceEnd::Media(other),
        };
        self.hq.voice.phase = HqVoicePhase::Ended(end);
    }

    /// Something the agent said or sent.
    pub(crate) fn hq_voice_agent(&mut self, event: MediaEvent) {
        match event {
            MediaEvent::AgentState(raw) => {
                if let HqVoicePhase::Live { agent } = &mut self.hq.voice.phase {
                    *agent = HqAgentState::from_wire(&raw);
                }
            }
            MediaEvent::AgentData { topic, payload } if topic == HQ_PROPOSAL_TOPIC => {
                match read_packet(&payload) {
                    Some(HqVoicePacket::Proposal(proposal)) => self.hq_voice_proposed(proposal),
                    Some(HqVoicePacket::Resolved(resolution)) => {
                        let note = match resolution.status {
                            HqResolutionStatus::Applied => HqNote::Applied,
                            HqResolutionStatus::Failed => HqNote::NotApplied,
                            HqResolutionStatus::Cancelled => HqNote::Cancelled,
                        };
                        // A resolution for a card not on screen (never seen,
                        // or already settled by a click) says nothing.
                        if self.hq.voice.remove(&resolution.proposal_id).is_some() {
                            self.hq.push_note(note);
                        }
                    }
                    None => {}
                }
            }
            MediaEvent::Speech(segment) => self.hq.spoken(segment),
            _ => {}
        }
    }

    /// A change proposed: a card, once. The agent may send the same proposal
    /// again, and the service answers a repeat with the same id.
    fn hq_voice_proposed(&mut self, proposal: HqVoiceProposal) {
        if self.hq.voice.card(&proposal.proposal_id).is_none() {
            self.hq.voice.cards.push(HqVoiceCard {
                proposal,
                state: HqCardState::Open,
            });
        }
    }

    /// The member clicked Confirm on a card.
    pub(crate) fn confirm_hq_voice_card(
        &mut self,
        proposal_id: &str,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let can_confirm = self.hq_voice_controls().can_confirm;
        let workspace_id = self.workspace_id();
        let Some(card) = self
            .hq
            .voice
            .card_mut(proposal_id)
            .filter(|card| can_confirm && card.is_actionable())
        else {
            return Vec::new();
        };
        card.state = HqCardState::Confirming;
        vec![Effect::ConfirmHqVoice {
            ticket: tickets.issue_keyed(Slot::HqVoiceCard, proposal_id),
            workspace_id,
            proposal: card.proposal.clone(),
        }]
    }

    /// The member clicked Cancel on a card.
    pub(crate) fn cancel_hq_voice_card(
        &mut self,
        proposal_id: &str,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let workspace_id = self.workspace_id();
        let Some(card) = self
            .hq
            .voice
            .card_mut(proposal_id)
            .filter(|card| card.is_actionable())
        else {
            return Vec::new();
        };
        card.state = HqCardState::Cancelling;
        vec![Effect::CancelHqVoice {
            ticket: tickets.issue_keyed(Slot::HqVoiceCard, proposal_id),
            workspace_id,
            proposal_id: proposal_id.to_owned(),
        }]
    }

    pub(crate) fn hq_voice_confirmed(
        &mut self,
        ticket: Ticket,
        result: Result<HqConfirmResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        let Some(proposal_id) = tickets.accept_keyed(Slot::HqVoiceCard, ticket) else {
            return stay();
        };
        match result {
            Ok(applied) => {
                if let Some(card) = self.hq.voice.remove(&proposal_id) {
                    let note = match (
                        applied.executed,
                        applied.is_the_voice_proposal(&card.proposal),
                    ) {
                        (false, _) => HqNote::NotApplied,
                        (true, true) => HqNote::Applied,
                        (true, false) => HqNote::Mismatched,
                    };
                    self.hq.push_note(note);
                }
            }
            Err(error) => self.hq_voice_refused(&proposal_id, &error, true),
        }
        stay()
    }

    pub(crate) fn hq_voice_cancelled(
        &mut self,
        ticket: Ticket,
        result: Result<HqProposalCancelResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        let Some(proposal_id) = tickets.accept_keyed(Slot::HqVoiceCard, ticket) else {
            return stay();
        };
        match result {
            Ok(_) => {
                if self.hq.voice.remove(&proposal_id).is_some() {
                    self.hq.push_note(HqNote::Cancelled);
                }
            }
            Err(error) => self.hq_voice_refused(&proposal_id, &error, false),
        }
        stay()
    }

    /// A click on a card was refused or failed. The service's code says
    /// whether the card is settled: resolved already (by voice or another
    /// click), too old, unknown or not this one are, and the card goes; any
    /// other failure leaves it for the member to try again.
    fn hq_voice_refused(&mut self, proposal_id: &str, error: &ApiError, confirming: bool) {
        let voice = &mut self.hq.voice;
        if voice.card(proposal_id).is_none() {
            return;
        }
        let settled_note = match error.code() {
            Some(CODE_ALREADY_RESOLVED) => Some(HqNote::AlreadyHandled),
            Some(CODE_EXPIRED) => Some(HqNote::Expired),
            _ => None,
        };
        if let Some(note) = settled_note {
            voice.remove(proposal_id);
            self.hq.push_note(note);
            return;
        }
        let failure = FailureText::from_api_error(error);
        if matches!(error.code(), Some(CODE_NOT_FOUND | CODE_INVALID_REQUEST)) {
            voice.remove(proposal_id);
            voice.notice = Some(failure);
        } else if let Some(card) = voice.card_mut(proposal_id) {
            card.state = HqCardState::Failed {
                failure,
                confirming,
            };
        }
    }
}

impl crate::hq::HqScreen {
    /// Adds an app note to the conversation.
    pub(crate) fn push_note(&mut self, note: HqNote) {
        self.transcript.push(HqMessage {
            author: HqAuthor::Assistant,
            text: HqText::Note(note),
        });
    }

    /// A transcribed segment: it updates its line of the conversation, which
    /// stays where it first appeared, or starts one. An empty segment starts
    /// nothing.
    pub(crate) fn spoken(&mut self, segment: SpeechSegment) {
        let author = match segment.speaker {
            Speaker::Member => HqAuthor::Member,
            Speaker::Agent => HqAuthor::Assistant,
        };
        let existing = self.transcript.iter_mut().find(|message| {
            message.author == author
                && matches!(&message.text, HqText::Spoken { segment: id, .. } if *id == segment.segment_id)
        });
        match existing {
            Some(message) => {
                message.text = HqText::Spoken {
                    segment: segment.segment_id,
                    text: segment.text,
                    is_final: segment.is_final,
                };
            }
            None if !segment.text.trim().is_empty() => self.transcript.push(HqMessage {
                author,
                text: HqText::Spoken {
                    segment: segment.segment_id,
                    text: segment.text,
                    is_final: segment.is_final,
                },
            }),
            None => {}
        }
    }
}
