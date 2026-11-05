//! Supervising a live call from its call screen: listening in, sending the
//! receptionist a private note (a whisper), and taking the call over by
//! voice. The web dashboard's Live calls page is the reference: the same
//! states, refusals and confirmation, on the one call open here.
//!
//! # Who may do what
//!
//! The service's own rules, so nothing is offered that it would refuse:
//! every role may listen; an agency or client member may also whisper and
//! take over, and a viewer may not. Listening and taking over join the call's
//! room through the call engine, so they need a build with calls and an
//! engine nothing else holds; a whisper is a request the service delivers
//! itself, and needs neither. All three need the call in progress
//! (`in-progress`), as the service does for a whisper and a takeover.
//!
//! A member's own call, placed or answered on their softphone, has no AI on
//! it, and the service refuses a note or a takeover on it with its own code.
//! Nothing in the call as the service reads it says so beforehand, so the
//! first such refusal marks the call ([`Supervision::ai_absent`]), and
//! neither is offered on it again. Listening to a call that has ended is
//! refused too, and ends the session as the call's end does. Both codes'
//! names here are provisional until the service's are final.
//!
//! # Listening
//!
//! [`SuperviseEvent::Listen`] asks for a listen-only credential
//! ([`SupervisePhase::Joining`]), then joins the room it names with the
//! microphone off. The credential publishes nothing, and the microphone is
//! never turned on in this session. While the session is in the room
//! ([`SupervisePhase::Listening`]), the voice agent's lines of the call
//! arrive as its data packets ([`Supervision::lines`]), the last
//! [`SUPERVISE_LINES`] of them, cleared whenever a session joins.
//!
//! # Taking over
//!
//! [`SuperviseEvent::TakeOver`] only asks for confirmation; nothing is
//! requested until [`SuperviseEvent::ConfirmTakeOver`]. Then the takeover
//! credential is asked for first, so a refusal leaves a listening session
//! listening ([`SupervisePhase::TakingOver`]); once it lands, the listening
//! session is left and the room joined again with the microphone on
//! ([`SupervisePhase::OnCall`]). The receptionist announces a supervisor and
//! steps back. Handing back ([`SuperviseEvent::HandBack`]) leaves the room,
//! which is what makes the receptionist carry on, and the screen goes back to
//! idle, as the web's does. The takeover route sends no passphrase: a phone
//! call's room is not encrypted.
//!
//! # Whispers
//!
//! A note is sent once and never repeated by itself, because the AI would act
//! on it twice. One is in flight at most. A blank note sends nothing, and one
//! longer than [`MAX_WHISPER_CHARS`] is refused here rather than by the
//! service. The draft is cleared when the note is delivered, unless it was
//! edited meanwhile, and kept when it was not. What the receptionist then
//! did with it arrives as a live update ([`Whisper::outcome`]).
//!
//! # The end
//!
//! A session ends when the member stops listening or hands back, when the
//! room ends ([`SuperviseEnd::CallEnded`]), when the call is read again and is
//! no longer in progress, when the connection is lost for good, and when the
//! call screen closes: the microphone is never live without the controls
//! that stop it on screen. The workspace closing, signing out and the
//! desktop sleeping or quitting end it too.

use district_api::ApiError;
use district_model::{
    CallMonitorTokenResponse, CallTakeoverResponse, CallWhisperResponse, LiveCallId,
    MAX_WHISPER_CHARS, TelemetryEnvelope, TranscriptChunk, WhisperApplied,
};

use crate::calls::CallView;
use crate::failure::FailureText;
use crate::media::{DisconnectReason, MediaCredential, MediaEvent, MediaOwner, MediaSession};
use crate::model::{Effect, Slot, Ticket, Tickets};
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::{l, s};

/// How many of the call's lines a session keeps, the newest.
pub const SUPERVISE_LINES: usize = 300;

/// The status of a call that is under way, the only one supervised.
const IN_PROGRESS: &str = "in-progress";

/// The refusal code of a call that is not in progress any more, or whose
/// room has gone.
const CODE_CALL_NOT_IN_PROGRESS: &str = "call_not_in_progress";

/// PROVISIONAL name, until the service's code is final (GitLab #2,
/// 2026-11-05): the refusal of listening to a call that has ended.
const CODE_LISTEN_CALL_ENDED: &str = "call_ended";

/// PROVISIONAL name, until the service's code is final (GitLab #2,
/// 2026-11-05): the refusal of a note or a takeover on a member's own call,
/// placed or answered on their softphone, which has no AI on it.
const CODE_NO_AI_ON_CALL: &str = "call_has_no_ai";

/// Whether `error` says the call is over.
fn says_call_over(error: &ApiError) -> bool {
    matches!(
        error.code(),
        Some(CODE_CALL_NOT_IN_PROGRESS | CODE_LISTEN_CALL_ENDED)
    )
}

/// Whether `error` says the call has no AI on it.
fn says_no_ai(error: &ApiError) -> bool {
    error.code() == Some(CODE_NO_AI_ON_CALL)
}

/// Supervising the open call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Supervision {
    /// Where it stands.
    pub phase: SupervisePhase,
    /// Why the last listen or takeover did not happen, shown beside the
    /// controls until the next one.
    pub failure: Option<FailureText>,
    /// Whether the takeover confirmation is showing.
    pub confirming_takeover: bool,
    /// The note to the receptionist.
    pub whisper: Whisper,
    /// Whether the service said this is a member's own call, with no AI on
    /// it: nothing to send a note to or take over. It may still be listened
    /// to. Nothing in a call as the service reads it says so beforehand, so
    /// it is learned from the first refusal.
    pub ai_absent: bool,
    /// The call's lines heard so far, oldest first.
    lines: Vec<TranscriptChunk>,
    /// Whether a takeover was asked for while listening, so a refusal goes
    /// back to listening.
    listening_before_takeover: bool,
}

impl Supervision {
    /// The call's lines heard in this session, oldest first. A line the
    /// caller said is [`TranscriptChunk::is_caller`]; label it with
    /// [`speaker_label`](Self::speaker_label).
    pub fn lines(&self) -> &[TranscriptChunk] {
        &self.lines
    }

    /// The name to show beside a line: "Caller" for the caller, and the
    /// receptionist's own name for the rest.
    pub fn speaker_label(line: &TranscriptChunk) -> &str {
        if line.is_caller() {
            s::transcript_caller(l())
        } else {
            &line.speaker
        }
    }

    /// The line saying where the session stands, or `None` while idle.
    pub fn status(&self) -> Option<&'static str> {
        match self.phase {
            SupervisePhase::Idle => None,
            SupervisePhase::Joining => Some(s::supervise_joining(l())),
            SupervisePhase::Listening => Some(s::supervise_listening(l())),
            SupervisePhase::TakingOver => Some(s::supervise_taking_over(l())),
            SupervisePhase::OnCall => Some(s::supervise_on_call_title(l())),
            SupervisePhase::Ended(end) => end.message(),
        }
    }

    /// Whether a credential is being asked for, which holds the call engine
    /// for this session as much as a room joined does.
    pub(crate) fn minting(&self) -> bool {
        matches!(
            self.phase,
            SupervisePhase::Joining | SupervisePhase::TakingOver
        )
    }

    fn is_active(&self) -> bool {
        matches!(
            self.phase,
            SupervisePhase::Joining
                | SupervisePhase::Listening
                | SupervisePhase::TakingOver
                | SupervisePhase::OnCall
        )
    }

    /// The session's room ended under it. A room that could not be joined
    /// says so; one that ended is the call's end.
    fn media_ended(&mut self, reason: DisconnectReason) {
        let taking_over = self.phase == SupervisePhase::TakingOver;
        self.confirming_takeover = false;
        self.phase = match reason {
            DisconnectReason::ConnectFailed => {
                let words = if taking_over {
                    s::supervise_take_over_join_failed(l())
                } else {
                    s::supervise_listen_failed(l())
                };
                self.failure = Some(FailureText::retryable(words));
                SupervisePhase::Idle
            }
            DisconnectReason::Left => SupervisePhase::Idle,
            DisconnectReason::RoomEnded => SupervisePhase::Ended(SuperviseEnd::CallEnded),
            other => SupervisePhase::Ended(SuperviseEnd::Media(other)),
        };
    }

    fn heard(&mut self, line: TranscriptChunk) {
        if line.text.trim().is_empty() {
            return;
        }
        if self.lines.len() == SUPERVISE_LINES {
            self.lines.remove(0);
        }
        self.lines.push(line);
    }
}

/// Where supervising the open call stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SupervisePhase {
    /// Not in the call's room.
    #[default]
    Idle,
    /// The listen-only credential is being asked for, or the room joined.
    Joining,
    /// In the room, listening. The caller does not hear the member.
    Listening,
    /// The takeover credential is being asked for, or the room joined again
    /// with the microphone.
    TakingOver,
    /// On the call with the microphone: the receptionist has stepped back.
    OnCall,
    /// The session ended without the member ending it.
    Ended(SuperviseEnd),
}

/// Why a session the member did not end ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SuperviseEnd {
    /// The call ended, or its room went.
    CallEnded,
    /// The media connection ended for this reason.
    Media(DisconnectReason),
}

impl SuperviseEnd {
    /// What to tell the member, or `None` when there is nothing worth saying.
    pub fn message(self) -> Option<&'static str> {
        match self {
            Self::CallEnded => Some(s::supervise_call_ended(l())),
            Self::Media(reason) => reason.message(),
        }
    }
}

/// The note to the receptionist on the open call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Whisper {
    /// What is typed.
    pub draft: String,
    /// Why the last note was not sent or not delivered. The draft is kept.
    pub failure: Option<FailureText>,
    /// What the receptionist did with the last note delivered, once the
    /// service says.
    pub outcome: Option<WhisperApplied>,
    /// The draft as it was when the note in flight was sent.
    sending: Option<String>,
}

impl Whisper {
    /// Whether a note is in flight.
    pub fn is_sending(&self) -> bool {
        self.sending.is_some()
    }

    /// The line saying what the receptionist did with the last note, or
    /// `None` until the service says. Show
    /// [`WhisperApplied::first_words`] with it, to say which note.
    pub fn outcome_line(&self) -> Option<&'static str> {
        self.outcome.as_ref().map(|outcome| {
            if outcome.applied {
                s::supervise_whisper_applied(l())
            } else {
                s::supervise_whisper_not_applied(l())
            }
        })
    }
}

/// What the supervise controls may offer now, on the open call.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SuperviseControls {
    /// Whether the call is in progress, which every control needs. When it is
    /// false nothing below is offered, and the app shows no controls.
    pub live: bool,
    /// Whether Listen works: any role, in a build with calls, with the call
    /// engine free, while not in the room.
    pub can_listen: bool,
    /// Whether Stop listening works: listening, or joining to.
    pub can_stop_listening: bool,
    /// Whether the member's role lets them whisper and take over at all
    /// (agency and client). A viewer sees neither control.
    pub can_supervise: bool,
    /// Whether Send works for the note: a member who may supervise, not on
    /// the call themselves, with no note in flight.
    pub can_whisper: bool,
    /// Whether Take over the call works: a member who may supervise, in a
    /// build with calls, with the engine free of anything else, not on the
    /// call or taking it over already.
    pub can_take_over: bool,
    /// Whether Hand back works: on the call, or joining it.
    pub can_hand_back: bool,
}

/// What the member does to supervise the open call.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SuperviseEvent {
    /// Listen in.
    Listen,
    /// Stop listening.
    StopListening,
    /// The note's text changed.
    EditWhisper(String),
    /// Send the note.
    SendWhisper,
    /// Take over the call: shows the confirmation, asks for nothing.
    TakeOver,
    /// Close the confirmation: nothing is asked for.
    CancelTakeOver,
    /// Take over, from the confirmation.
    ConfirmTakeOver,
    /// Hand the call back to the receptionist, which leaves the room.
    HandBack,
    /// Dismiss what the controls are saying: a failure, or how the session
    /// ended.
    Dismiss,
}

impl SignedIn {
    /// Supervising the open call, or `None` with no call open.
    pub fn supervision(&self) -> Option<&Supervision> {
        self.call.as_ref().map(|screen| &screen.supervise)
    }

    /// The supervise session's room, for its connection, the microphone and
    /// its notices.
    pub fn supervise_session(&self) -> Option<&MediaSession> {
        self.media
            .as_ref()
            .filter(|media| media.owner == MediaOwner::Supervise)
    }

    /// The placeholder of the note's field: why it cannot be sent, or an
    /// example when it can.
    pub fn whisper_placeholder(&self) -> &'static str {
        let controls = self.supervise_controls();
        if !controls.can_supervise {
            s::supervise_whisper_viewer(l())
        } else if self
            .supervision()
            .is_some_and(|supervision| supervision.ai_absent)
        {
            s::supervise_no_ai(l())
        } else if self
            .supervision()
            .is_some_and(|supervision| supervision.phase == SupervisePhase::OnCall)
        {
            s::supervise_whisper_on_call(l())
        } else {
            s::supervise_whisper_placeholder(l())
        }
    }

    /// What the supervise controls may offer now.
    pub fn supervise_controls(&self) -> SuperviseControls {
        let Some(supervision) = self.supervision() else {
            return SuperviseControls::default();
        };
        let live = self.supervised_call_live();
        if !live {
            return SuperviseControls::default();
        }
        let capabilities = self.capabilities();
        let can_supervise = capabilities.can_change;
        let engine = self.calls_available && self.engine_free_for_supervise();
        let phase = supervision.phase;
        let out_of_room = matches!(phase, SupervisePhase::Idle | SupervisePhase::Ended(_));
        SuperviseControls {
            live,
            can_listen: capabilities.role.is_some() && engine && out_of_room,
            can_stop_listening: matches!(
                phase,
                SupervisePhase::Joining | SupervisePhase::Listening
            ),
            can_supervise,
            can_whisper: can_supervise
                && !supervision.ai_absent
                && phase != SupervisePhase::OnCall
                && !supervision.whisper.is_sending(),
            can_take_over: can_supervise
                && !supervision.ai_absent
                && engine
                && !matches!(phase, SupervisePhase::TakingOver | SupervisePhase::OnCall),
            can_hand_back: phase == SupervisePhase::OnCall
                || (phase == SupervisePhase::TakingOver && self.supervise_session().is_some()),
        }
    }

    /// Whether the open call is in progress, as last read, with an id the
    /// supervise routes take as a call's.
    fn supervised_call_live(&self) -> bool {
        self.supervised_call_id().is_some()
            && self.call.as_ref().is_some_and(|screen| {
                matches!(&screen.call, CallView::Ready(call) if call.status == IN_PROGRESS)
            })
    }

    /// Whether the call engine is free for supervising: held by nothing but
    /// the supervise session itself, and wanted by nothing else.
    fn engine_free_for_supervise(&self) -> bool {
        let held_elsewhere = self
            .media
            .as_ref()
            .is_some_and(|media| media.owner != MediaOwner::Supervise);
        !held_elsewhere && !self.engine_wanted_elsewhere()
    }

    /// Runs `change` on the open call's supervision. With no call open there
    /// is none, and nothing runs: a credential, a room and a note in flight
    /// all go with the call screen, so their answers only arrive while it is
    /// open.
    fn with_supervision<T: Default>(&mut self, change: impl FnOnce(&mut Supervision) -> T) -> T {
        self.call
            .as_mut()
            .map(|screen| change(&mut screen.supervise))
            .unwrap_or_default()
    }

    /// The call's id as the supervise routes take it, or `None` for one they
    /// would read as another kind of room.
    fn supervised_call_id(&self) -> Option<LiveCallId> {
        self.call
            .as_ref()
            .and_then(|screen| LiveCallId::new(&screen.call_id))
    }

    pub(crate) fn supervise_event(&mut self, event: SuperviseEvent, tickets: &mut Tickets) -> Next {
        Next::Stay(match event {
            SuperviseEvent::Listen => self.listen(tickets),
            SuperviseEvent::StopListening => self.stop_listening(tickets),
            SuperviseEvent::EditWhisper(text) => {
                self.with_supervision(|supervision| supervision.whisper.draft = text);
                Vec::new()
            }
            SuperviseEvent::SendWhisper => self.send_whisper(tickets),
            SuperviseEvent::TakeOver => {
                if self.supervise_controls().can_take_over {
                    self.with_supervision(|supervision| {
                        supervision.confirming_takeover = true;
                        supervision.failure = None;
                    });
                }
                Vec::new()
            }
            SuperviseEvent::CancelTakeOver => {
                self.with_supervision(|supervision| supervision.confirming_takeover = false);
                Vec::new()
            }
            SuperviseEvent::ConfirmTakeOver => self.take_over(tickets),
            SuperviseEvent::HandBack => self.hand_back(tickets),
            SuperviseEvent::Dismiss => {
                self.with_supervision(|supervision| {
                    supervision.failure = None;
                    supervision.whisper.failure = None;
                    if matches!(supervision.phase, SupervisePhase::Ended(_)) {
                        supervision.phase = SupervisePhase::Idle;
                    }
                });
                Vec::new()
            }
        })
    }

    fn listen(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let can_listen = self.supervise_controls().can_listen;
        // Offered only for a call with such an id.
        let Some(call_id) = self.supervised_call_id().filter(|_| can_listen) else {
            return Vec::new();
        };
        self.with_supervision(|supervision| {
            supervision.phase = SupervisePhase::Joining;
            supervision.failure = None;
        });
        vec![Effect::RequestCallMonitor {
            ticket: tickets.issue(Slot::SuperviseListen),
            call_id,
        }]
    }

    fn stop_listening(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if !self.supervise_controls().can_stop_listening {
            return Vec::new();
        }
        self.end_supervision(SupervisePhase::Idle, tickets)
    }

    fn hand_back(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if !self.supervise_controls().can_hand_back {
            return Vec::new();
        }
        self.end_supervision(SupervisePhase::Idle, tickets)
    }

    /// Leaves the room and forgets any credential asked for, ending in
    /// `phase`.
    fn end_supervision(&mut self, phase: SupervisePhase, tickets: &mut Tickets) -> Vec<Effect> {
        tickets.cancel_each(&[Slot::SuperviseListen, Slot::SuperviseTakeover]);
        self.with_supervision(|supervision| {
            supervision.phase = phase;
            supervision.confirming_takeover = false;
        });
        self.leave_media_of(MediaOwner::Supervise)
    }

    /// Ends the session for good, as the call screen closes, the workspace
    /// closes, or the desktop sleeps or quits. How a session that already
    /// ended ended stays on screen. A note in flight is not this session's:
    /// it lands unless the screen itself closes.
    pub(crate) fn stop_supervising(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let active = self.supervision().is_some_and(Supervision::is_active);
        if active {
            self.end_supervision(SupervisePhase::Idle, tickets)
        } else {
            tickets.cancel_each(&[Slot::SuperviseListen, Slot::SuperviseTakeover]);
            self.leave_media_of(MediaOwner::Supervise)
        }
    }

    fn take_over(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let workspace_id = self.workspace_id();
        let can_take_over = self.supervise_controls().can_take_over;
        let listening = self.supervise_session().is_some();
        let confirming = self
            .with_supervision(|supervision| std::mem::take(&mut supervision.confirming_takeover));
        // The confirmation can outlive what allowed it: the engine taken by
        // something else, or the call read again as over.
        let Some(call_id) = self
            .supervised_call_id()
            .filter(|_| confirming && can_take_over)
        else {
            return Vec::new();
        };
        self.with_supervision(|supervision| {
            supervision.listening_before_takeover = listening;
            supervision.phase = SupervisePhase::TakingOver;
            supervision.failure = None;
        });
        // A listen still being asked for is overtaken by the takeover.
        tickets.cancel(Slot::SuperviseListen);
        vec![Effect::RequestCallTakeover {
            ticket: tickets.issue(Slot::SuperviseTakeover),
            workspace_id,
            call_id,
        }]
    }

    fn send_whisper(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let workspace_id = self.workspace_id();
        let can_whisper = self.supervise_controls().can_whisper;
        let Some(call_id) = self.supervised_call_id().filter(|_| can_whisper) else {
            return Vec::new();
        };
        let draft = self.with_supervision(|supervision| supervision.whisper.draft.clone());
        let text = draft.trim();
        if text.is_empty() {
            return Vec::new();
        }
        if text.chars().count() > MAX_WHISPER_CHARS {
            let failure =
                FailureText::final_(s::supervise_whisper_too_long(l(), MAX_WHISPER_CHARS as i64));
            self.with_supervision(|supervision| supervision.whisper.failure = Some(failure));
            return Vec::new();
        }
        let text = text.to_owned();
        self.with_supervision(|supervision| {
            supervision.whisper.failure = None;
            supervision.whisper.outcome = None;
            supervision.whisper.sending = Some(draft);
        });
        vec![Effect::SendCallWhisper {
            ticket: tickets.issue(Slot::SuperviseWhisper),
            workspace_id,
            call_id,
            text,
        }]
    }

    pub(crate) fn call_monitor_issued(
        &mut self,
        ticket: Ticket,
        result: Result<CallMonitorTokenResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::SuperviseListen, ticket) {
            return stay();
        }
        match result {
            Ok(credential) => {
                let media = MediaCredential::from_monitor(&credential);
                Next::Stay(self.join_supervised(media, false, tickets))
            }
            Err(error) => {
                let effects = self.supervise_refused(&error, s::supervise_listen_failed(l()));
                Next::Stay(effects)
            }
        }
    }

    pub(crate) fn call_takeover_issued(
        &mut self,
        ticket: Ticket,
        result: Result<CallTakeoverResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::SuperviseTakeover, ticket) {
            return stay();
        }
        match result {
            Ok(credential) => {
                let media = MediaCredential::from_takeover(&credential);
                Next::Stay(self.join_supervised(media, true, tickets))
            }
            Err(error) => {
                let effects = self.supervise_refused(&error, s::supervise_take_over_failed(l()));
                Next::Stay(effects)
            }
        }
    }

    /// Joins the call's room for the session, leaving any room it was in:
    /// the lines start again, as the web's do.
    fn join_supervised(
        &mut self,
        media: MediaCredential,
        microphone: bool,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        self.with_supervision(|supervision| supervision.lines.clear());
        self.start_media(MediaOwner::Supervise, media, microphone, tickets)
    }

    /// A listen or a takeover was refused or failed. A call no longer in
    /// progress ends the session; any other refusal says why, and a takeover
    /// asked for while listening goes back to listening.
    fn supervise_refused(&mut self, error: &ApiError, fallback: &str) -> Vec<Effect> {
        if says_call_over(error) {
            self.with_supervision(|supervision| {
                supervision.phase = SupervisePhase::Ended(SuperviseEnd::CallEnded);
            });
            return self.leave_media_of(MediaOwner::Supervise);
        }
        let listening = self.supervise_session().is_some();
        let no_ai = says_no_ai(error);
        let failure = if no_ai {
            FailureText::final_(s::supervise_no_ai(l()))
        } else {
            described(error, fallback)
        };
        self.with_supervision(|supervision| {
            supervision.ai_absent |= no_ai;
            supervision.failure = Some(failure);
            supervision.phase = if supervision.phase == SupervisePhase::TakingOver
                && supervision.listening_before_takeover
                && listening
            {
                SupervisePhase::Listening
            } else {
                SupervisePhase::Idle
            };
        });
        Vec::new()
    }

    pub(crate) fn call_whispered(
        &mut self,
        ticket: Ticket,
        result: Result<CallWhisperResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::SuperviseWhisper, ticket) {
            return stay();
        }
        let no_ai = result.as_ref().is_err_and(says_no_ai);
        let failure = result.err().map(|error| {
            if no_ai {
                FailureText::final_(s::supervise_no_ai(l()))
            } else {
                described(&error, s::supervise_whisper_failed(l()))
            }
        });
        self.with_supervision(|supervision| {
            supervision.ai_absent |= no_ai;
            let whisper = &mut supervision.whisper;
            let sent = whisper.sending.take();
            if failure.is_none() && sent.as_deref() == Some(whisper.draft.as_str()) {
                whisper.draft.clear();
            }
            whisper.failure = failure;
        });
        stay()
    }

    /// The supervise session is in its room.
    /// Connected again after reconnecting changes nothing.
    pub(crate) fn supervise_media_connected(&mut self) {
        self.with_supervision(|supervision| {
            supervision.phase = match supervision.phase {
                SupervisePhase::Joining => SupervisePhase::Listening,
                SupervisePhase::TakingOver => SupervisePhase::OnCall,
                other => other,
            };
        });
    }

    /// A line of the call, from the voice agent.
    pub(crate) fn supervise_agent(&mut self, event: MediaEvent) {
        let MediaEvent::AgentData { payload, .. } = event else {
            return;
        };
        if let Some(line) = TranscriptChunk::read(&payload) {
            self.with_supervision(|supervision| supervision.heard(line));
        }
    }

    /// The supervise session's room ended under it. A room that could not be
    /// joined says so; one that ended is the call's end.
    pub(crate) fn supervise_media_ended(&mut self, reason: DisconnectReason) {
        self.with_supervision(|supervision| supervision.media_ended(reason));
    }

    /// The service said what the receptionist did with a note: shown when it
    /// is about the open call, whoever sent the note.
    pub(crate) fn whisper_applied(&mut self, envelope: &TelemetryEnvelope) {
        let open = self
            .call
            .as_ref()
            .is_some_and(|screen| screen.call_id == envelope.call_id);
        if let (true, Some(outcome)) = (open, envelope.whisper_applied()) {
            self.with_supervision(|supervision| supervision.whisper.outcome = Some(outcome));
        }
    }

    /// The open call was read again: a session on a call that is no longer
    /// in progress ends.
    pub(crate) fn supervised_call_reread(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let live = self.supervised_call_live();
        let active = self.supervision().is_some_and(Supervision::is_active);
        let loaded = self
            .call
            .as_ref()
            .is_some_and(|screen| matches!(screen.call, CallView::Ready(_)));
        if active && loaded && !live {
            self.end_supervision(SupervisePhase::Ended(SuperviseEnd::CallEnded), tickets)
        } else {
            Vec::new()
        }
    }
}

/// The words for a refusal: the service's own for one it gave a code to (in
/// French, the catalog's), and `fallback` for a failure with none, as the
/// web's supervise controls say it. A signed-out session keeps its own words.
fn described(error: &ApiError, fallback: &str) -> FailureText {
    match error {
        ApiError::Unauthorized(_) => FailureText::from_api_error(error),
        _ if error.code().is_some() => FailureText::from_api_error(error),
        _ => FailureText::retryable(fallback),
    }
}
