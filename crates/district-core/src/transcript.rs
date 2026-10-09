//! The live transcript of the call on this desktop.
//!
//! Once the call has an id (a placed call's from the dial's answer, an answered
//! call's from its ring), the core asks the call's workspace socket for the
//! call's live transcript ([`Effect::WatchTranscript`]), and applies the
//! `transcript_*` frames that socket delivers for that call to
//! [`ActiveCall::transcript`](crate::ActiveCall::transcript), which the apps
//! show as the call's lines. A frame for any other call or workspace, one that
//! cannot be read, and one that arrives after the call ended (apart from a
//! retraction, which still takes lines off the screen) change nothing.
//!
//! [`LiveTranscript`] is the contract's client algorithm for one call, ported
//! rule for rule from district-core-swift 6.0.0's `TranscriptReducer`, with its
//! timers turned into the model's waits:
//!
//! 1. **Stale frames.** Per epoch, a high-water mark of `seq` and the seqs
//!    missing below it. A frame above the mark raises it, and the seqs it
//!    skipped are missing; a frame whose seq is missing fills its gap and is
//!    applied; anything else is a duplicate and is dropped. Delivery is at least
//!    once and unordered, so a late frame is normal.
//! 2. **Revisions.** Each segment keeps its latest revision by `rev`, and a
//!    final always beats an interim at any revision.
//! 3. **Order.** (epoch, index), then the segment id so ties are stable.
//! 4. **Gaps.** A gap still open after [`GAP_HEAL`] asks the socket for the call
//!    again, and the snapshot that answers replaces the state. One heal at a
//!    time: no subscribe goes out while an earlier one is unanswered.
//! 5. **Snapshots.** Their parts arrive back to back with the same header; the
//!    union replaces the state once the last arrives, sets its epoch's mark with
//!    nothing missing, and closes every older epoch. A broken one (a part out of
//!    order, a header that changed, another frame between parts) is asked for
//!    again. One with no epoch follows an `all` retraction: the next frame of
//!    each epoch sets its baseline.
//! 6. **End.** `transcript_ended` with `call_ended` or `handed_off` (or an
//!    unknown reason), or the call's own end, is final: the transcript is not
//!    live, and the full one is read with backoff, because the service writes it
//!    only after the call's session closes. `agent_error` is not final: the call
//!    may get a fresh assistant, so the transcript is
//!    [`Reconnecting`](LiveTranscriptPhase::Reconnecting) until a new epoch or
//!    the end.
//! 7. **Refusals.** Only a `transcript_error` about a subscribe changes
//!    anything. `rate_limited` asks again after the wait it names; `not_live` and
//!    the others make the live transcript unavailable, and only a call event
//!    showing the call in progress (at most once per [`NOT_LIVE_RETRY`]) asks
//!    again after `not_live`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use district_api::ApiError;
use district_live::{LiveUpdate, is_transient};
use district_model::{
    CallTranscriptResponse, TRANSCRIPT_SUBSCRIBE_OP, TelemetryEnvelope, TelemetryEventType,
    TranscriptClientOp, TranscriptEndReason, TranscriptEndedData, TranscriptErrorCode,
    TranscriptErrorData, TranscriptEvent, TranscriptRetractedData, TranscriptSegment,
    TranscriptSnapshotData, TranscriptSpeaker,
};

use crate::failure::FailureText;
use crate::model::{Effect, Slot, Ticket, Tickets};
use crate::ringing::is_over;
use crate::signed_in::SignedIn;

/// How long a gap in a transcript's frames may stay open before the call is
/// asked for again.
pub const GAP_HEAL: Duration = Duration::from_secs(2);

/// The wait before asking again after `rate_limited` named none.
pub const RATE_LIMIT_FALLBACK: Duration = Duration::from_secs(2);

/// The least time between two subscribes after `not_live`.
pub const NOT_LIVE_RETRY: Duration = Duration::from_secs(30);

/// How many times the full transcript is read after the end before giving up.
pub const FINAL_FETCH_ATTEMPTS: u32 = 8;

/// The wait before reading the full transcript the `attempt`th time (1 for the
/// first): 2 seconds, doubling up to 30. Eight attempts span two and a half
/// minutes.
pub fn final_fetch_delay(attempt: u32) -> Duration {
    let doublings = attempt.clamp(1, 9) - 1;
    Duration::from_millis((2000_u64 << doublings).min(30_000))
}

/// What the core asks the call's workspace socket for: the live transcript of
/// one call. See [`Effect::WatchTranscript`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TranscriptWatch {
    /// The workspace whose socket carries the call.
    pub workspace_id: String,
    /// The call.
    pub call_id: String,
    /// How many times the call has been asked for again to heal a gap. Each
    /// increase is one more `transcript.subscribe` on the socket, which brings a
    /// fresh snapshot.
    pub resubscribes: u32,
}

/// Where a call's live transcript stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveTranscriptPhase {
    /// Asked for, and the first snapshot has not arrived. This can last up to
    /// 30 seconds: the service holds a subscribe until the assistant's first
    /// line.
    Subscribing,
    /// Lines arrive as they are spoken.
    Live,
    /// The assistant stopped with an error and the call may be handed to a
    /// fresh one, whose lines arrive as a new epoch. The lines shown stay.
    Reconnecting,
    /// The assistant stopped transcribing, or the call ended. The lines shown
    /// stay, and the full transcript is read
    /// ([`LiveTranscript::final_transcript`]).
    Ended(TranscriptEndReason),
    /// No live transcript can be shown for this call: the service has none
    /// (`not_live`), or refused for another reason. The call log has the
    /// transcript after the call.
    Unavailable(TranscriptErrorCode),
}

/// The full transcript, read once the live one ended.
#[derive(Clone, PartialEq, Eq)]
pub enum FinalTranscript {
    /// Not asked for: the live transcript has not ended.
    NotRequested,
    /// Being read; `attempt` counts from 1.
    Fetching {
        /// Which attempt.
        attempt: u32,
    },
    /// The transcript the service keeps for the call. Never log it.
    Loaded(String),
    /// Still empty after every attempt: nothing was said, or it was never
    /// written.
    Empty,
    /// The read failed, for good or after every attempt.
    Failed(FailureText),
}

impl fmt::Debug for FinalTranscript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRequested => f.write_str("NotRequested"),
            Self::Fetching { attempt } => f
                .debug_struct("Fetching")
                .field("attempt", attempt)
                .finish(),
            Self::Loaded(text) => write!(f, "Loaded(<{} bytes>)", text.len()),
            Self::Empty => f.write_str("Empty"),
            Self::Failed(failure) => f.debug_tuple("Failed").field(failure).finish(),
        }
    }
}

/// What a [`LiveTranscript`] asks the core to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    /// Ask for the call again as a new subscription, after `not_live`.
    Subscribe,
    /// Ask for the call again after the wait, for a fresh snapshot.
    Resubscribe(Duration),
    /// Look at the gap again after the wait ([`LiveTranscript::gap_check`]).
    CheckGap(Duration),
    /// Read the full transcript after the wait.
    FetchFinal(Duration),
    /// Nothing more is wanted from the socket for this call.
    Unsubscribe,
    /// No subscribe after `not_live` until the wait is over.
    NotLiveCooldown(Duration),
}

/// What has been seen of one epoch's `seq` counter.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SeqTrack {
    high: i64,
    /// The seqs below `high` not yet seen, as inclusive ranges: a jump of any
    /// size costs one.
    missing: Vec<(i64, i64)>,
}

impl SeqTrack {
    fn new(high: i64) -> Self {
        Self {
            high,
            missing: Vec::new(),
        }
    }

    /// Takes `seq` out of the missing set; whether it was there.
    fn fill(&mut self, seq: i64) -> bool {
        let Some(index) = self
            .missing
            .iter()
            .position(|&(low, high)| low <= seq && seq <= high)
        else {
            return false;
        };
        let (low, high) = self.missing.remove(index);
        if seq < high {
            self.missing.insert(index, (seq + 1, high));
        }
        if low < seq {
            self.missing.insert(index, (low, seq - 1));
        }
        true
    }
}

/// What every part of one snapshot repeats.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SnapshotHeader {
    live: bool,
    ended_reason: Option<TranscriptEndReason>,
    complete: bool,
    epoch: Option<i64>,
    last_seq: Option<i64>,
}

impl SnapshotHeader {
    fn of(data: &TranscriptSnapshotData) -> Self {
        Self {
            live: data.live,
            ended_reason: data.ended_reason.clone(),
            complete: data.complete,
            epoch: data.epoch,
            last_seq: data.last_seq,
        }
    }
}

/// A snapshot being put together from its parts.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Assembly {
    header: SnapshotHeader,
    segments: Vec<TranscriptSegment>,
    next_part: i64,
    broken: bool,
}

/// One call's live transcript.
///
/// Its `Debug` output carries no text: a segment prints its length, and so
/// does the full transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveTranscript {
    call_id: String,
    phase: LiveTranscriptPhase,
    complete: bool,
    final_transcript: FinalTranscript,
    segments: BTreeMap<String, TranscriptSegment>,
    /// Segment ids retracted one by one, so a late copy cannot bring one back.
    tombstones: BTreeSet<String>,
    seqs: BTreeMap<i64, SeqTrack>,
    /// The newest snapshot's epoch: every older epoch is closed.
    closed_below: Option<i64>,
    /// The newest epoch any frame or snapshot named.
    newest_epoch: Option<i64>,
    /// Whether some seq is missing, and a look at it is due.
    gap_open: bool,
    /// A subscribe is unanswered: no other may be sent. True from the start,
    /// since the call is asked for before the first frame.
    awaiting_snapshot: bool,
    assembly: Option<Assembly>,
    /// The epoch the last end applied to.
    ended_epoch: Option<i64>,
    /// The end is known to be final, as opposed to `agent_error` or a snapshot
    /// that says only "not live".
    terminal: bool,
    /// A subscribe after `not_live` went out less than [`NOT_LIVE_RETRY`] ago.
    not_live_cooling: bool,
    /// An `all` retraction left no baseline: the first frame of an epoch sets
    /// one, with no earlier seq counted as missing.
    unbaselined: bool,
    /// Whether the socket is asked for this call's frames.
    pub(crate) subscribed: bool,
    /// How many heals have been asked for; see [`TranscriptWatch::resubscribes`].
    pub(crate) resubscribes: u32,
}

impl LiveTranscript {
    /// The heading over the lines.
    pub const HEADING: &'static str = "Live transcript";
    /// The status while the first snapshot is awaited.
    pub const CONNECTING: &'static str = "Connecting.";
    /// The status while lines arrive.
    pub const LIVE: &'static str = "Live";
    /// The status after `agent_error`: the call may get a fresh assistant.
    pub const RECONNECTING: &'static str = "Reconnecting.";
    /// The status after the end.
    pub const ENDED: &'static str = "Call ended";
    /// What shows while live with no line yet.
    pub const WAITING: &'static str = "Nothing has been said yet.";
    /// The note when the service's memory did not reach back to the call's
    /// first line. Saying so keeps a partial transcript from reading as the
    /// whole call.
    pub const INCOMPLETE: &'static str =
        "Earlier lines will appear in the full transcript after the call.";
    /// What shows while the full transcript is read.
    pub const LOADING_FULL: &'static str = "Loading the full transcript.";
    /// What shows when the full transcript is empty.
    pub const NO_TRANSCRIPT: &'static str = "No transcript for this call.";
    /// The note under an assistant line that was cut off: its text is what was
    /// actually played.
    pub const INTERRUPTED: &'static str = "Interrupted";
    /// The label of a caller's line.
    pub const CALLER: &'static str = "Caller";
    /// The label of an assistant's line when the service names no persona.
    pub const ASSISTANT: &'static str = "Assistant";
    /// The label of a line by a speaker this client does not know.
    pub const OTHER_SPEAKER: &'static str = "Other speaker";

    /// A transcript for `call_id`, just asked for.
    pub(crate) fn new(call_id: String) -> Self {
        Self {
            call_id,
            phase: LiveTranscriptPhase::Subscribing,
            complete: true,
            final_transcript: FinalTranscript::NotRequested,
            segments: BTreeMap::new(),
            tombstones: BTreeSet::new(),
            seqs: BTreeMap::new(),
            closed_below: None,
            newest_epoch: None,
            gap_open: false,
            awaiting_snapshot: true,
            assembly: None,
            ended_epoch: None,
            terminal: false,
            not_live_cooling: false,
            unbaselined: false,
            subscribed: true,
            resubscribes: 0,
        }
    }

    /// The call.
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// Where it stands.
    pub fn phase(&self) -> &LiveTranscriptPhase {
        &self.phase
    }

    /// False when the service's memory did not reach back to the call's first
    /// line: show [`INCOMPLETE`](Self::INCOMPLETE).
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// The full transcript, once the live one ended.
    pub fn final_transcript(&self) -> &FinalTranscript {
        &self.final_transcript
    }

    /// The lines to show, in order. One that is not
    /// [`is_final`](TranscriptSegment::is_final) is an interim hypothesis that
    /// will be replaced, and is drawn as provisional.
    pub fn lines(&self) -> Vec<&TranscriptSegment> {
        let mut lines: Vec<&TranscriptSegment> = self.segments.values().collect();
        lines.sort_by(|a, b| {
            (a.epoch, a.index, &a.segment_id).cmp(&(b.epoch, b.index, &b.segment_id))
        });
        lines
    }

    /// The line under the heading, or `None` when no live transcript can be
    /// shown and the app offers the call log's instead.
    pub fn status(&self) -> Option<&'static str> {
        match self.phase {
            LiveTranscriptPhase::Subscribing => Some(Self::CONNECTING),
            LiveTranscriptPhase::Live => Some(Self::LIVE),
            LiveTranscriptPhase::Reconnecting => Some(Self::RECONNECTING),
            LiveTranscriptPhase::Ended(_) => Some(Self::ENDED),
            LiveTranscriptPhase::Unavailable(_) => None,
        }
    }

    /// Who spoke `segment`: the persona's name on an assistant line when the
    /// service sends one, and a third party, never a guess, for a speaker this
    /// client does not know.
    pub fn speaker_label(segment: &TranscriptSegment) -> &str {
        match &segment.speaker {
            TranscriptSpeaker::Caller => Self::CALLER,
            TranscriptSpeaker::Agent => segment
                .speaker_name
                .as_deref()
                .filter(|name| !name.is_empty())
                .unwrap_or(Self::ASSISTANT),
            TranscriptSpeaker::Other(_) => Self::OTHER_SPEAKER,
        }
    }

    // Inputs.

    /// Applies one event. An event about another call changes nothing.
    fn apply(&mut self, event: TranscriptEvent) -> Vec<Command> {
        if event.call_id() != Some(self.call_id.as_str()) {
            return Vec::new();
        }
        match event {
            TranscriptEvent::Snapshot(data) => self.snapshot_part(data),
            TranscriptEvent::Segment(data) => {
                let mut commands = self.interrupt_assembly();
                commands.extend(self.segment(data.segment));
                commands
            }
            TranscriptEvent::Ended(data) => {
                let mut commands = self.interrupt_assembly();
                commands.extend(self.ended(&data));
                commands
            }
            TranscriptEvent::Retracted(data) => {
                let mut commands = self.interrupt_assembly();
                commands.extend(self.retracted(data));
                commands
            }
            // An error answers an op, not the transcript's flow: it leaves a
            // snapshot that is arriving alone.
            TranscriptEvent::Error(data) => self.error(&data),
        }
    }

    /// A snapshot's parts arrive back to back, so another frame for the call
    /// while one is being put together means the rest of it was lost: ask for
    /// a whole one.
    fn interrupt_assembly(&mut self) -> Vec<Command> {
        if self.assembly.take().is_none() {
            return Vec::new();
        }
        self.heal(Duration::ZERO)
    }

    /// The socket opened again, and has asked for the call again on its own: a
    /// snapshot is on its way and will replace the state, so a half-received
    /// one from the old socket is dropped. The lines shown stay until it lands.
    fn reconnected(&mut self) {
        self.assembly = None;
        self.awaiting_snapshot = true;
    }

    /// The call ended. Nothing for a call with no live transcript: the call
    /// log already has the one after the call.
    fn call_ended(&mut self) -> Vec<Command> {
        if matches!(self.phase, LiveTranscriptPhase::Unavailable(_)) {
            return Vec::new();
        }
        self.end(TranscriptEndReason::CallEnded, true)
    }

    /// A call event shows the call in progress. The only way back from
    /// `not_live`, and at most once per [`NOT_LIVE_RETRY`]; at any other time
    /// it asks for nothing.
    fn call_shown_in_progress(&mut self) -> Vec<Command> {
        if self.phase != LiveTranscriptPhase::Unavailable(TranscriptErrorCode::NotLive)
            || self.not_live_cooling
        {
            return Vec::new();
        }
        self.not_live_cooling = true;
        self.phase = LiveTranscriptPhase::Subscribing;
        self.awaiting_snapshot = true;
        vec![Command::Subscribe, Command::NotLiveCooldown(NOT_LIVE_RETRY)]
    }

    /// The gap's wait is over. A gap that closed meanwhile asks for nothing,
    /// and nor does one while a subscribe is unanswered, since its snapshot
    /// will settle the gap.
    fn gap_check(&mut self) -> Vec<Command> {
        if !self.gap_open || self.awaiting_snapshot {
            return Vec::new();
        }
        self.heal(Duration::ZERO)
    }

    /// The answer to a read of the full transcript.
    fn final_fetched(&mut self, result: Result<String, ApiError>) -> Vec<Command> {
        let FinalTranscript::Fetching { attempt } = self.final_transcript else {
            return Vec::new();
        };
        let retryable = match &result {
            Ok(text) if !text.trim().is_empty() => {
                self.final_transcript = FinalTranscript::Loaded(text.clone());
                return vec![Command::Unsubscribe];
            }
            Ok(_) => true,
            Err(error) => is_transient(error),
        };
        if !retryable || attempt >= FINAL_FETCH_ATTEMPTS {
            self.final_transcript = match result {
                Ok(_) => FinalTranscript::Empty,
                Err(error) => FinalTranscript::Failed(FailureText::from_api_error(&error)),
            };
            return vec![Command::Unsubscribe];
        }
        self.final_transcript = FinalTranscript::Fetching {
            attempt: attempt + 1,
        };
        vec![Command::FetchFinal(final_fetch_delay(attempt + 1))]
    }

    // Snapshots.

    fn snapshot_part(&mut self, data: TranscriptSnapshotData) -> Vec<Command> {
        let header = SnapshotHeader::of(&data);
        let whole = match self.assembly.take() {
            _ if data.part == 0 => Assembly {
                header,
                segments: data.segments,
                next_part: 1,
                broken: false,
            },
            Some(mut current) if current.next_part == data.part && current.header == header => {
                current.segments.extend(data.segments);
                current.next_part += 1;
                current
            }
            // A part out of order, one with no part 0, or a header that
            // changed: nothing can be put together from it. The rest is let
            // through, then a whole snapshot is asked for, so two heals are
            // never in flight at once.
            _ => Assembly {
                header,
                segments: Vec::new(),
                next_part: data.part.saturating_add(1),
                broken: true,
            },
        };
        if data.more {
            self.assembly = Some(whole);
            return Vec::new();
        }
        self.awaiting_snapshot = false;
        if whole.broken {
            return self.heal(Duration::ZERO);
        }
        self.replace(whole)
    }

    fn replace(&mut self, whole: Assembly) -> Vec<Command> {
        let header = whole.header;
        self.segments.clear();
        for segment in whole.segments {
            if !self.tombstones.contains(&segment.segment_id) {
                self.merge(segment);
            }
        }
        self.complete = header.complete;
        // The mark covers the snapshot's epoch only, the newest the service
        // holds. Older epochs' lines stay for display, and the epochs close.
        if let (Some(epoch), Some(last_seq)) = (header.epoch, header.last_seq) {
            self.seqs = BTreeMap::from([(epoch, SeqTrack::new(last_seq))]);
            self.closed_below = Some(self.closed_below.map_or(epoch, |closed| closed.max(epoch)));
            self.newest_epoch = Some(self.newest_epoch.map_or(epoch, |newest| newest.max(epoch)));
            self.unbaselined = false;
        } else {
            // Purged by an `all` retraction: no mark and nothing missing, until
            // a frame sets one.
            self.seqs.clear();
            self.unbaselined = true;
        }
        self.gap_open = false;
        // A final end stays final, whatever a later snapshot says.
        if self.terminal {
            return Vec::new();
        }
        if header.live {
            self.revive();
            return Vec::new();
        }
        self.ended_epoch = header.epoch;
        match header.ended_reason {
            Some(TranscriptEndReason::AgentError) => {
                self.phase = LiveTranscriptPhase::Reconnecting;
                self.final_transcript = FinalTranscript::NotRequested;
                Vec::new()
            }
            Some(reason) => self.end(reason, true),
            // A service older than the field sends no reason: an end a newer
            // epoch may still undo.
            None => self.end(TranscriptEndReason::CallEnded, false),
        }
    }

    // Live frames.

    fn segment(&mut self, segment: TranscriptSegment) -> Vec<Command> {
        let (fresh, commands) = self.admit(segment.epoch, segment.seq);
        if !fresh || self.tombstones.contains(&segment.segment_id) {
            return commands;
        }
        let epoch = segment.epoch;
        self.merge(segment);
        // A newer epoch after an end that was not final is a fresh assistant:
        // live again.
        if self.awaiting_new_epoch() && self.ended_epoch.is_none_or(|ended| epoch > ended) {
            self.revive();
        }
        commands
    }

    /// After `agent_error`, or a snapshot that said only "not live".
    fn awaiting_new_epoch(&self) -> bool {
        !self.terminal
            && matches!(
                self.phase,
                LiveTranscriptPhase::Reconnecting | LiveTranscriptPhase::Ended(_)
            )
    }

    fn revive(&mut self) {
        self.phase = LiveTranscriptPhase::Live;
        self.final_transcript = FinalTranscript::NotRequested;
    }

    /// The latest revision wins, and a final beats an interim at any revision.
    fn merge(&mut self, incoming: TranscriptSegment) {
        let wins = self
            .segments
            .get(&incoming.segment_id)
            .is_none_or(|stored| {
                if stored.is_final == incoming.is_final {
                    incoming.rev >= stored.rev
                } else {
                    incoming.is_final
                }
            });
        if wins {
            self.segments.insert(incoming.segment_id.clone(), incoming);
        }
    }

    /// Only the newest epoch's end changes the phase: a late end of an older
    /// one is counted and nothing more.
    fn ended(&mut self, data: &TranscriptEndedData) -> Vec<Command> {
        let (fresh, mut commands) = self.admit(data.epoch, data.seq);
        if !fresh || Some(data.epoch) != self.newest_epoch {
            return commands;
        }
        self.ended_epoch = Some(data.epoch);
        if data.reason != TranscriptEndReason::AgentError {
            commands.extend(self.end(data.reason.clone(), true));
            return commands;
        }
        if !self.terminal {
            self.phase = LiveTranscriptPhase::Reconnecting;
        }
        commands
    }

    fn end(&mut self, reason: TranscriptEndReason, terminal: bool) -> Vec<Command> {
        self.phase = LiveTranscriptPhase::Ended(reason);
        self.terminal |= terminal;
        if self.final_transcript != FinalTranscript::NotRequested {
            return Vec::new();
        }
        self.final_transcript = FinalTranscript::Fetching { attempt: 1 };
        vec![Command::FetchFinal(final_fetch_delay(1))]
    }

    /// Applied even when its seq was seen, or its epoch is closed: dropping
    /// lines twice is harmless, and leaving one on screen is not. The
    /// assistant's carries an epoch and a seq, which are counted; the
    /// website's carries neither.
    fn retracted(&mut self, data: TranscriptRetractedData) -> Vec<Command> {
        let commands = match (data.epoch, data.seq) {
            (Some(epoch), Some(seq)) => self.admit(epoch, seq).1,
            _ => Vec::new(),
        };
        let gone: Vec<String> = if data.all {
            self.segments.keys().cloned().collect()
        } else {
            data.segment_ids
        };
        for segment_id in gone {
            self.segments.remove(&segment_id);
            self.tombstones.insert(segment_id);
        }
        commands
    }

    /// Only an error about a subscribe changes anything: the service echoes the
    /// op exactly, so a refused unsubscribe is not one, and subscribing again
    /// after it would undo it.
    fn error(&mut self, data: &TranscriptErrorData) -> Vec<Command> {
        if data.op.as_deref() != Some(TRANSCRIPT_SUBSCRIBE_OP) {
            return Vec::new();
        }
        if data.code == TranscriptErrorCode::RateLimited {
            // The refused subscribe brings no snapshot; this one replaces it.
            let wait = data.retry_after_ms.map_or(RATE_LIMIT_FALLBACK, |ms| {
                Duration::from_millis(u64::try_from(ms).unwrap_or(0))
            });
            return self.heal(wait);
        }
        self.awaiting_snapshot = false;
        // An ended transcript stays ended: `not_live` also answers a subscribe
        // long after the end, which changes nothing about the lines shown.
        if data.code == TranscriptErrorCode::NotLive
            && matches!(self.phase, LiveTranscriptPhase::Ended(_))
        {
            return Vec::new();
        }
        self.phase = LiveTranscriptPhase::Unavailable(data.code.clone());
        vec![Command::Unsubscribe]
    }

    // Sequence tracking.

    /// Asks for the call again for a fresh snapshot. Every caller has settled
    /// that no other subscribe is in flight.
    fn heal(&mut self, after: Duration) -> Vec<Command> {
        self.awaiting_snapshot = true;
        vec![Command::Resubscribe(after)]
    }

    /// Whether the frame (epoch, seq) is new, and the gap check it may start.
    /// A seq below 1 is not one the service sends, and an epoch older than the
    /// newest snapshot's is closed: both are dropped.
    fn admit(&mut self, epoch: i64, seq: i64) -> (bool, Vec<Command>) {
        if seq < 1 || self.closed_below.is_some_and(|closed| epoch < closed) {
            return (false, Vec::new());
        }
        let unbaselined = self.unbaselined;
        let track = self
            .seqs
            .entry(epoch)
            .or_insert_with(|| SeqTrack::new(if unbaselined { seq - 1 } else { 0 }));
        if seq <= track.high {
            if !track.fill(seq) {
                return (false, Vec::new());
            }
        } else {
            if seq > track.high + 1 {
                track.missing.push((track.high + 1, seq - 1));
            }
            track.high = seq;
        }
        self.newest_epoch = Some(self.newest_epoch.map_or(epoch, |newest| newest.max(epoch)));
        (true, self.update_gap())
    }

    fn update_gap(&mut self) -> Vec<Command> {
        if self.seqs.values().all(|track| track.missing.is_empty()) {
            self.gap_open = false;
            return Vec::new();
        }
        if std::mem::replace(&mut self.gap_open, true) {
            return Vec::new();
        }
        vec![Command::CheckGap(GAP_HEAL)]
    }
}

/// The waits a call's transcript holds, dropped with it.
const TRANSCRIPT_SLOTS: [Slot; 5] = [
    Slot::TranscriptGap,
    Slot::TranscriptResubscribe,
    Slot::TranscriptFetchWait,
    Slot::TranscriptFetch,
    Slot::TranscriptCooldown,
];

impl SignedIn {
    /// The live transcript of the call on this desktop, while it has one.
    fn transcript_mut(&mut self) -> Option<&mut LiveTranscript> {
        self.active_call.as_mut()?.transcript.as_mut()
    }

    /// The call on this desktop has its id now: ask its workspace's socket for
    /// its live transcript. An id the service would refuse gets none, and the
    /// call log has the transcript after the call.
    pub(crate) fn start_transcript(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        tickets.cancel_each(&TRANSCRIPT_SLOTS);
        // Every caller has just set the call's id, so there is a call.
        for call in self.active_call.iter_mut() {
            call.transcript = call
                .call_id()
                .filter(|call_id| TranscriptClientOp::is_valid_call_id(call_id))
                .map(|call_id| LiveTranscript::new(call_id.to_owned()));
        }
        self.sync_transcript_watch(tickets)
    }

    /// The call on this desktop was put away or replaced: whatever transcript
    /// it had is no longer asked for, and its waits are dropped.
    pub(crate) fn transcript_dropped(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        tickets.cancel_each(&TRANSCRIPT_SLOTS);
        self.sync_transcript_watch(tickets)
    }

    /// The session is ending: nothing more is asked for.
    pub(crate) fn stop_transcript(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if let Some(call) = self.active_call.as_mut() {
            call.transcript = None;
        }
        self.transcript_dropped(tickets)
    }

    /// What the socket is asked for, if that changed since it was last told.
    fn sync_transcript_watch(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let wanted = self.active_call.as_ref().and_then(|call| {
            let transcript = call.transcript.as_ref().filter(|t| t.subscribed)?;
            Some(TranscriptWatch {
                workspace_id: call.workspace_id.clone(),
                call_id: transcript.call_id.clone(),
                resubscribes: transcript.resubscribes,
            })
        });
        if wanted == self.transcript_watch {
            return Vec::new();
        }
        self.transcript_watch.clone_from(&wanted);
        vec![Effect::WatchTranscript {
            revision: tickets.revision(),
            transcript: wanted,
        }]
    }

    /// Carries out what the transcript asked for. Every caller has the
    /// commands from the call's transcript, so it is there.
    fn perform(&mut self, commands: Vec<Command>, tickets: &mut Tickets) -> Vec<Effect> {
        let mut effects = Vec::new();
        let mut wait = |slot: Slot, delay: Duration| {
            effects.push(Effect::Wait {
                ticket: tickets.issue(slot),
                delay,
            });
        };
        if let Some(transcript) = self.transcript_mut() {
            for command in commands {
                match command {
                    Command::Subscribe => transcript.subscribed = true,
                    Command::Resubscribe(delay) if delay.is_zero() => transcript.resubscribes += 1,
                    Command::Resubscribe(delay) => wait(Slot::TranscriptResubscribe, delay),
                    Command::CheckGap(delay) => wait(Slot::TranscriptGap, delay),
                    Command::FetchFinal(delay) => wait(Slot::TranscriptFetchWait, delay),
                    Command::Unsubscribe => transcript.subscribed = false,
                    Command::NotLiveCooldown(delay) => wait(Slot::TranscriptCooldown, delay),
                }
            }
        }
        effects.extend(self.sync_transcript_watch(tickets));
        effects
    }

    /// The call's transcript, and whether the call is over, when `call_id` in
    /// `workspace_id` is the call on this desktop.
    fn transcript_of(
        &mut self,
        workspace_id: &str,
        call_id: &str,
    ) -> Option<(bool, &mut LiveTranscript)> {
        let call = self
            .active_call
            .as_mut()
            .filter(|call| call.workspace_id == workspace_id)?;
        let over = call.is_over();
        let transcript = call.transcript.as_mut().filter(|t| t.call_id == call_id)?;
        Some((over, transcript))
    }

    /// A `transcript_*` frame from `workspace_id`'s socket. Applied only when
    /// it is about the call on this desktop and can be read, and after the call
    /// ended only when it is a retraction: the lines shown then stay as they
    /// were until the full transcript is read, but an erased line still comes
    /// off.
    pub(crate) fn transcript_frame(
        &mut self,
        workspace_id: &str,
        envelope: &TelemetryEnvelope,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let Some((over, transcript)) = self.transcript_of(workspace_id, &envelope.call_id) else {
            return Vec::new();
        };
        let commands = match envelope.transcript_event() {
            Some(event) if !over || matches!(event, TranscriptEvent::Retracted(_)) => {
                transcript.apply(event)
            }
            _ => return Vec::new(),
        };
        self.perform(commands, tickets)
    }

    /// A call event from `workspace_id`'s socket, which may say the call on
    /// this desktop ended, or is in progress (the only way back after
    /// `not_live`).
    pub(crate) fn transcript_call_signal(
        &mut self,
        workspace_id: &str,
        envelope: &TelemetryEnvelope,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let in_progress = matches!(
            envelope.event_type,
            TelemetryEventType::CallStarted | TelemetryEventType::CallUpdated
        );
        let commands = match self.transcript_of(workspace_id, &envelope.call_id) {
            Some((_, transcript)) if is_over(envelope) => transcript.call_ended(),
            Some((false, transcript)) if in_progress => transcript.call_shown_in_progress(),
            _ => return Vec::new(),
        };
        self.perform(commands, tickets)
    }

    /// `workspace_id`'s socket opened. When it carries the call's transcript it
    /// has asked for the call again on its own, so a snapshot is coming, and a
    /// gap's wait is moot.
    pub(crate) fn transcript_reconnected(
        &mut self,
        workspace_id: &str,
        update: &LiveUpdate,
        tickets: &mut Tickets,
    ) {
        let reopened = *update == LiveUpdate::Connected;
        let carried = self
            .active_call
            .as_mut()
            .filter(|call| reopened && call.workspace_id == workspace_id)
            .and_then(|call| call.transcript.as_mut())
            .filter(|transcript| transcript.subscribed);
        if let Some(transcript) = carried {
            transcript.reconnected();
            tickets.cancel(Slot::TranscriptGap);
        }
    }

    /// The call on this desktop ended.
    pub(crate) fn transcript_call_ended(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let commands = self
            .transcript_mut()
            .map(LiveTranscript::call_ended)
            .unwrap_or_default();
        self.perform(commands, tickets)
    }

    /// One of the transcript's waits is over, if `ticket` is one of them.
    pub(crate) fn transcript_wait_over(
        &mut self,
        ticket: Ticket,
        tickets: &mut Tickets,
    ) -> Option<Vec<Effect>> {
        if tickets.accept(Slot::TranscriptGap, ticket) {
            let commands = self.transcript_mut().map(LiveTranscript::gap_check);
            return Some(self.perform(commands.unwrap_or_default(), tickets));
        }
        if tickets.accept(Slot::TranscriptResubscribe, ticket) {
            if let Some(transcript) = self.transcript_mut().filter(|t| t.subscribed) {
                transcript.resubscribes += 1;
            }
            return Some(self.sync_transcript_watch(tickets));
        }
        if tickets.accept(Slot::TranscriptCooldown, ticket) {
            if let Some(transcript) = self.transcript_mut() {
                transcript.not_live_cooling = false;
            }
            return Some(Vec::new());
        }
        if !tickets.accept(Slot::TranscriptFetchWait, ticket) {
            return None;
        }
        let read = self.active_call.as_ref().and_then(|call| {
            let transcript = call.transcript.as_ref()?;
            let fetching = matches!(
                transcript.final_transcript,
                FinalTranscript::Fetching { .. }
            );
            fetching.then(|| (call.workspace_id.clone(), transcript.call_id.clone()))
        });
        Some(
            read.map(|(workspace_id, call_id)| Effect::LoadTranscript {
                ticket: tickets.issue(Slot::TranscriptFetch),
                workspace_id,
                call_id,
            })
            .into_iter()
            .collect(),
        )
    }

    /// The full transcript was read, if `ticket` is the read's.
    pub(crate) fn final_transcript_read(
        &mut self,
        ticket: Ticket,
        result: &Result<CallTranscriptResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Option<Vec<Effect>> {
        if !tickets.accept(Slot::TranscriptFetch, ticket) {
            return None;
        }
        let text = result.clone().map(|answer| answer.transcript);
        let commands = self
            .transcript_mut()
            .map(|transcript| transcript.final_fetched(text))
            .unwrap_or_default();
        Some(self.perform(commands, tickets))
    }
}
