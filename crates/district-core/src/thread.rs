//! One thread: its history of messages and calls, and the reply composer.
//!
//! # The history
//!
//! The newest page is read when the thread opens, and again whenever it may
//! have changed (a reply sent, a live update, a refresh). Older pages are read
//! backwards from a cursor, one at a time. Pages may overlap by the service's
//! design, so every merge is by event id, and the events are kept in the order
//! the service itself uses: by timestamp, then id.
//!
//! # The composer
//!
//! - What is typed is saved on the service as the member's own draft two
//!   seconds after the typing stops, and deleted once the box is cleared: a
//!   blank draft is the absence of one. The drafts route's rate limit is shared
//!   by every member of the workspace, which is why it is a debounce and not a
//!   write per keystroke. One draft write is on its way at a time: effects run
//!   side by side, so a save and the delete after it could otherwise land in
//!   either order and bring back a draft of a message already sent.
//! - The saved draft is restored when the thread opens, unless something has
//!   been typed already: adopting into an empty box can only add, overwriting a
//!   typed one can only lose.
//! - Sending is single-flight: a second send while one is on its way is
//!   ignored, because every send is billed and the customer would get the
//!   message twice. The saved draft is deleted as the message goes, so a draft
//!   can never outlive its own send, and saved again if the send fails.
//! - Attachments are uploaded when they are picked, checked here first against
//!   the service's own rules so a refused file costs no upload.
//! - A reply written by a model is asked for only by an explicit
//!   [`ThreadEvent::DraftReply`]: every one is a billed model run.
//!
//! # The reply channel
//!
//! In an app that answers Messenger and Instagram
//! ([`CoreConfig::meta_channels`](crate::CoreConfig::meta_channels)), the
//! channels offered are the ones the service lists for the thread, in its
//! order, the first chosen when the thread opens; a channel this build does
//! not know is skipped, and a thread the service lists none for opens read
//! only. Otherwise the one reply target is the thread's text or email
//! address, as before.
//!
//! On WhatsApp, the customer-service window is read when the channel is
//! chosen. Closed, free text is not offered (the service would refuse it), and
//! the sender's approved templates are, one field per `{{n}}`, with the filled
//! text shown before it goes, filled exactly as the service fills it. Not yet
//! known or unreadable, the window blocks nothing: the service is the
//! authority, and a guess either way would mislead.
//!
//! # The owner
//!
//! A member who may write gives the thread to a member, or takes it back, from
//! the workspace's Agency and Client members; a Viewer can be neither, but a
//! thread already given to one keeps showing them until it is changed. One
//! change is on its way at a time.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::Duration;

use district_api::ApiError;
use district_model::{
    AiDraftResponse, CHANNEL_EMAIL, CHANNEL_INSTAGRAM, CHANNEL_MESSENGER, CHANNEL_SMS,
    CHANNEL_WHATSAPP, ConversationAssignResponse, DraftResponse, DraftSaveRequest,
    MarkReadResponse, MediaUploadResponse, MemberListResponse, ReplyTarget, SendMessageRequest,
    SendMessageResponse, ThreadRef, TimelineCursor, TimelineEvent, TimelineResponse,
    WhatsAppTemplatesResponse, WhatsAppWindowResponse, fill_whatsapp_template,
};

use crate::failure::FailureText;
use crate::inbox::InboxScreen;
use crate::model::{Effect, Slot, Ticket, Tickets};
use crate::role::{Capabilities, WorkspaceRole};
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::{l, s};

/// How long after the last change the composer's text is saved.
pub const DRAFT_SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

/// The most attachments one message may carry, the service's own ceiling.
pub const MAX_ATTACHMENTS: usize = 5;

/// The largest attachment the service accepts, in bytes.
pub const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;

/// The only image types the service accepts as attachments.
pub const ATTACHMENT_TYPES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];

/// The slots of an open thread, forgotten when it closes. The draft write is
/// not among them: a save sent as the thread closes still lands. Nor is an
/// assignment: the inbox list shows its outcome.
const THREAD_SLOTS: [Slot; 10] = [
    Slot::Timeline,
    Slot::TimelineOlder,
    Slot::DraftLoad,
    Slot::DraftTimer,
    Slot::Send,
    Slot::Upload,
    Slot::AiDraft,
    Slot::ThreadMembers,
    Slot::WhatsAppWindow,
    Slot::WhatsAppTemplates,
];

/// The error code of the service's refusal of free WhatsApp text once the
/// window has closed.
const WHATSAPP_WINDOW_CLOSED: &str = "whatsapp_window_closed";

/// What a reply channel is called, in the app's language: on the channel
/// chooser and on an inbox row. `None` for a channel this build does not know.
pub fn reply_channel_label(channel: &str) -> Option<&'static str> {
    match channel {
        CHANNEL_SMS => Some(s::inbox_compose_channel_sms(l())),
        CHANNEL_EMAIL => Some(s::inbox_compose_channel_email(l())),
        CHANNEL_WHATSAPP => Some(s::thread_channel_whatsapp(l())),
        CHANNEL_MESSENGER => Some(s::thread_channel_messenger(l())),
        CHANNEL_INSTAGRAM => Some(s::thread_channel_instagram(l())),
        _ => None,
    }
}

/// The member's saved replies being written: one write on its way at a time,
/// and the next ones waiting until it lands, the last one wanted for each
/// thread in place of any before it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DraftWrites {
    /// By workspace and thread key.
    waiting: BTreeMap<(String, String), DraftWrite>,
}

/// One write of a saved reply.
#[derive(Clone, Debug, PartialEq, Eq)]
enum DraftWrite {
    Save {
        workspace_id: String,
        draft: DraftSaveRequest,
    },
    Delete {
        workspace_id: String,
        thread_key: String,
    },
}

impl DraftWrite {
    fn key(&self) -> (String, String) {
        match self {
            Self::Save {
                workspace_id,
                draft,
            } => (workspace_id.clone(), draft.thread_key.clone()),
            Self::Delete {
                workspace_id,
                thread_key,
            } => (workspace_id.clone(), thread_key.clone()),
        }
    }

    fn effect(self, ticket: Ticket) -> Effect {
        match self {
            Self::Save {
                workspace_id,
                draft,
            } => Effect::SaveDraft {
                ticket,
                workspace_id,
                draft,
            },
            Self::Delete {
                workspace_id,
                thread_key,
            } => Effect::DeleteDraft {
                ticket,
                workspace_id,
                thread_key,
            },
        }
    }
}

impl DraftWrites {
    /// Sends `write` now, or keeps it until the write on its way lands.
    fn write(&mut self, write: DraftWrite, tickets: &mut Tickets) -> Vec<Effect> {
        if tickets.awaiting(Slot::DraftWrite) {
            self.waiting.insert(write.key(), write);
            return Vec::new();
        }
        vec![write.effect(tickets.issue(Slot::DraftWrite))]
    }

    /// The app is quitting: every write still waiting goes now, since nothing
    /// will be left to send it when the one on its way lands. Their answers
    /// no longer matter, so each simply takes the slot.
    fn drain(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        std::mem::take(&mut self.waiting)
            .into_values()
            .map(|write| write.effect(tickets.issue(Slot::DraftWrite)))
            .collect()
    }

    /// The write on its way landed, whatever its answer (a failed save is
    /// superseded by the next one, and the text is still in the composer):
    /// the next one waiting goes.
    fn landed(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.waiting
            .pop_first()
            .map(|(_, write)| write.effect(tickets.issue(Slot::DraftWrite)))
            .into_iter()
            .collect()
    }
}

/// The open thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadScreen {
    /// The service's key for the thread, as the route carries it.
    pub thread_key: String,
    /// The title: the contact's name, else the other party's address.
    pub title: String,
    /// Where a reply goes and on which channel: the one of
    /// [`reply_choices`](Self::reply_choices) chosen, the first until another
    /// is. `None` when the thread is not in the list (opened from a search
    /// result for an older thread) or has nothing to reply on: the thread opens
    /// read-only then, rather than offer a reply that could only be refused.
    pub reply_target: Option<ReplyTarget>,
    /// Every way to answer the thread, from the inbox list, which is the only
    /// place the service publishes them. Offer a choice when there is more
    /// than one ([`ThreadEvent::ChooseChannel`]).
    pub reply_choices: Vec<ReplyTarget>,
    /// The history.
    pub history: ThreadHistory,
    /// The reply being written.
    pub composer: Composer,
    /// Who owns the thread, and the picker that changes it.
    pub assignment: ThreadAssignment,
    /// The WhatsApp window and its templates, while the reply goes by WhatsApp.
    pub whatsapp: Option<WhatsAppComposer>,
    pub(crate) thread: ThreadRef,
    pub(crate) workspace_id: String,
    pub(crate) meta_channels: bool,
}

impl ThreadScreen {
    /// Said in place of the composer to a member whose role cannot reply.
    pub const READ_ONLY_ROLE: &'static str = district_l10n::english::THREAD_READ_ONLY_ROLE;
    /// Said in place of the composer on a thread with nothing to reply on, or
    /// one opened from a search result for a thread the inbox list does not
    /// hold, whose reply address the service has not published.
    pub const NO_REPLY_TARGET: &'static str = district_l10n::english::THREAD_NO_REPLY_TARGET;
    /// The channel chooser's label.
    pub const CHANNEL_LABEL: &'static str = district_l10n::english::THREAD_CHANNEL_LABEL;
    /// Said beside the attachments when they cannot go on the chosen channel.
    pub const ATTACHMENTS_SMS_ONLY: &'static str =
        district_l10n::english::THREAD_ATTACHMENTS_SMS_ONLY;

    /// Why there is no composer, for a member with `capabilities`, or `None`
    /// when there is one.
    pub fn read_only_note(&self, capabilities: &Capabilities) -> Option<&'static str> {
        if !capabilities.can_change {
            Some(s::thread_read_only_role(l()))
        } else if self.reply_target.is_none() {
            Some(s::thread_no_reply_target(l()))
        } else {
            None
        }
    }

    /// What the composer may offer, for a member with `capabilities`.
    pub fn controls(&self, capabilities: &Capabilities) -> ThreadControls {
        let ready = matches!(self.history, ThreadHistory::Ready(_));
        let can_reply = capabilities.can_change && self.reply_target.is_some();
        let composer = &self.composer;
        let on_sms = self.on_channel(CHANNEL_SMS);
        let window_closed = self.whatsapp_window_closed();
        let busy = composer.sending || self.whatsapp.as_ref().is_some_and(|w| w.sending);
        ThreadControls {
            can_reply,
            // Text messages only. The email branch of the send route ignores
            // attachments altogether, so one on an email thread would upload and
            // silently not be delivered, and WhatsApp, Messenger and Instagram
            // refuse them.
            can_attach: can_reply && ready && !composer.attaching && on_sms,
            // Not while an upload is on its way: the message would go without
            // the image the user is waiting for. Not with attachments on a
            // channel that cannot carry them, nor as free text once the
            // WhatsApp window has closed: the service would refuse both.
            can_send: can_reply
                && ready
                && !busy
                && !composer.attaching
                && !composer.text.trim().is_empty()
                && (on_sms || composer.attachments.is_empty())
                && !window_closed,
            can_draft_reply: can_reply && ready && !composer.generating && !window_closed,
            can_choose_channel: can_reply && !busy && self.reply_choices.len() > 1,
            can_assign: capabilities.can_change
                && !self.assignment.saving
                && matches!(self.assignment.members, AssigneeList::Ready(_)),
            can_send_template: can_reply
                && window_closed
                && self
                    .whatsapp
                    .as_ref()
                    .is_some_and(WhatsAppComposer::template_complete),
        }
    }

    /// Whether the reply goes on `channel`.
    fn on_channel(&self, channel: &str) -> bool {
        self.reply_target
            .as_ref()
            .is_some_and(|target| target.channel == channel)
    }

    /// Whether the reply goes by WhatsApp and the service said the window is
    /// closed.
    pub fn whatsapp_window_closed(&self) -> bool {
        self.whatsapp
            .as_ref()
            .is_some_and(|whatsapp| whatsapp.window == WhatsAppWindowState::Closed)
    }

    /// Whether to say [`ATTACHMENTS_SMS_ONLY`](Self::ATTACHMENTS_SMS_ONLY):
    /// attachments are held and the reply goes on a channel that cannot carry
    /// them.
    pub fn attachments_blocked(&self) -> bool {
        !self.composer.attachments.is_empty()
            && self.reply_target.is_some()
            && !self.on_channel(CHANNEL_SMS)
    }
}

/// Who owns a thread, and the picker that changes it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadAssignment {
    /// The member who owns the thread, or `None` when nobody does (or the
    /// thread is not in the list, whose rows are the only place the service
    /// publishes it).
    pub assignee: Option<String>,
    /// The members the picker offers, read when the thread opens, for a member
    /// who may assign.
    pub members: AssigneeList,
    /// Whether a change is on its way. One at a time: the member given the
    /// thread is notified.
    pub saving: bool,
    /// Why the last change failed, shown beside the picker.
    pub failure: Option<FailureText>,
}

impl ThreadAssignment {
    /// The picker's label.
    pub const LABEL: &'static str = district_l10n::english::THREAD_ASSIGN_LABEL;
    /// The picker's choice for nobody.
    pub const UNASSIGNED: &'static str = district_l10n::english::THREAD_UNASSIGNED;

    /// Who the picker offers, besides [`UNASSIGNED`](Self::UNASSIGNED): the
    /// Agency and Client members, in the service's order, and the owner when
    /// they are not among them (a Viewer given the thread before Viewers could
    /// not be, or a member since removed), so the picker always shows who owns
    /// the thread. Empty until the members are read.
    pub fn options(&self) -> Vec<String> {
        let mut options = match &self.members {
            AssigneeList::Ready(members) => members.clone(),
            _ => Vec::new(),
        };
        if let Some(assignee) = &self.assignee
            && !options.contains(assignee)
        {
            options.push(assignee.clone());
        }
        options
    }
}

/// The members a thread can be given to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AssigneeList {
    /// Not read: the member may not assign.
    #[default]
    NotLoaded,
    /// Being read.
    Loading,
    /// The Agency and Client members' addresses. A Viewer is never among them:
    /// the service refuses to give one a thread.
    Ready(Vec<String>),
    /// The read failed. The owner still shows; the picker cannot change it.
    Failed(FailureText),
}

/// The WhatsApp side of the composer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WhatsAppComposer {
    /// The customer-service window.
    pub window: WhatsAppWindowState,
    /// The sender the window is for, which a reply goes from, once the service
    /// has named one.
    pub sender_id: Option<String>,
    /// The sender's templates that may be sent, read once the window is known
    /// to be closed.
    pub templates: TemplateList,
    /// The id of the template chosen.
    pub chosen: Option<String>,
    /// What is typed for each `{{n}}`, in order.
    pub variables: Vec<String>,
    /// Whether a template is on its way.
    pub sending: bool,
}

impl WhatsAppComposer {
    /// Said when the window is closed.
    pub const WINDOW_CLOSED: &'static str = district_l10n::english::THREAD_WHATSAPP_WINDOW_CLOSED;
    /// The template picker's label.
    pub const TEMPLATE_LABEL: &'static str = district_l10n::english::THREAD_TEMPLATE_LABEL;
    /// The template picker's placeholder.
    pub const TEMPLATE_PLACEHOLDER: &'static str =
        district_l10n::english::THREAD_TEMPLATE_PLACEHOLDER;
    /// Under a variable left empty.
    pub const VARIABLE_REQUIRED: &'static str =
        district_l10n::english::THREAD_TEMPLATE_VARIABLE_REQUIRED;
    /// The filled template's label.
    pub const PREVIEW: &'static str = district_l10n::english::THREAD_TEMPLATE_PREVIEW;
    /// The template's send button.
    pub const SEND_TEMPLATE: &'static str = district_l10n::english::THREAD_TEMPLATE_SEND;
    /// While the templates are read.
    pub const TEMPLATES_LOADING: &'static str = district_l10n::english::THREAD_TEMPLATES_LOADING;
    /// When the sender has no template that may be sent.
    pub const TEMPLATES_NONE: &'static str = district_l10n::english::THREAD_TEMPLATES_NONE;
    /// When the templates could not be read.
    pub const TEMPLATES_FAILED: &'static str = district_l10n::english::THREAD_TEMPLATES_FAILED;

    /// The template chosen, if it is still offered.
    pub fn template(&self) -> Option<&SendableTemplate> {
        let TemplateList::Ready(templates) = &self.templates else {
            return None;
        };
        let chosen = self.chosen.as_deref()?;
        templates.iter().find(|template| template.id == chosen)
    }

    /// The chosen template's variables as they will be sent: exactly as many
    /// as it has, each trimmed.
    pub fn filled_variables(&self) -> Vec<String> {
        let count = self
            .template()
            .map_or(0, |template| template.variable_count);
        (0..count)
            .map(|index| {
                self.variables
                    .get(index)
                    .map(|value| value.trim().to_owned())
                    .unwrap_or_default()
            })
            .collect()
    }

    /// The chosen template's text with what is typed so far, filled exactly as
    /// the service fills it; an empty variable shows as its `{{n}}`.
    pub fn preview(&self) -> Option<String> {
        self.template()
            .map(|template| fill_whatsapp_template(&template.body, &self.filled_variables()))
    }

    /// Whether a template is chosen and every variable has something in it.
    pub fn template_complete(&self) -> bool {
        !self.sending
            && self.template().is_some()
            && self
                .filled_variables()
                .iter()
                .all(|value| !value.is_empty())
    }

    /// The label for the `index`th variable (from zero): `Variable {{1}}`.
    pub fn variable_label(index: usize) -> String {
        s::thread_template_variable(l(), &format!("{{{{{}}}}}", index + 1))
    }
}

/// The customer-service window, as the service last said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum WhatsAppWindowState {
    /// Being read. Free text is offered meanwhile: the service decides.
    #[default]
    Loading,
    /// Free text may be sent, until the instant given, when known.
    Open {
        /// When it closes, as an ISO 8601 instant.
        expires_at: Option<String>,
    },
    /// Only a template may be sent.
    Closed,
    /// The read failed. Nothing is blocked: the service is the authority.
    Unknown(FailureText),
}

/// The templates of the sender whose window is closed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TemplateList {
    /// Not asked for: the window is not known to be closed, or the service
    /// named no sender.
    #[default]
    NotLoaded,
    /// Being read.
    Loading,
    /// The ones that may be sent from this sender. May be empty.
    Ready(Vec<SendableTemplate>),
    /// The read failed.
    Failed(FailureText),
}

/// A template that may be sent from the sender: approved, and with numbered
/// variables only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendableTemplate {
    /// Its id, which the send route takes.
    pub id: String,
    /// Its name at the provider.
    pub name: String,
    /// Its language code, for example `en_US`.
    pub language: String,
    /// Its text, with its `{{n}}` variables.
    pub body: String,
    /// How many variables it has.
    pub variable_count: usize,
}

impl SendableTemplate {
    /// The template's line in the picker: its name and language code.
    pub fn label(&self) -> String {
        s::thread_template_option(l(), &self.name, &self.language)
    }
}

/// What the composer of the open thread may offer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ThreadControls {
    /// Whether to show the composer at all: the role may change things, and the
    /// thread has somewhere to reply to. When false, say the thread is read
    /// only.
    pub can_reply: bool,
    /// Whether the attach button works now.
    pub can_attach: bool,
    /// Whether the send button works now. False while a send is on its way,
    /// which is what makes a second click harmless.
    pub can_send: bool,
    /// Whether the "Draft reply" button works now. False while one is being
    /// written, and while the WhatsApp window is closed.
    pub can_draft_reply: bool,
    /// Whether to offer the channel chooser: there is more than one way to
    /// reply, and nothing is on its way.
    pub can_choose_channel: bool,
    /// Whether the owner picker works now: the role may change things, the
    /// members are read and no change is on its way. When false, show the
    /// owner without the picker.
    pub can_assign: bool,
    /// Whether the template's send button works now: the WhatsApp window is
    /// closed, a template is chosen and every variable is filled.
    pub can_send_template: bool,
}

/// The thread's history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadHistory {
    /// Being read for the first time.
    Loading,
    /// Read.
    Ready(ThreadEvents),
    /// The first read failed.
    Failed(FailureText),
}

/// The read history of a thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadEvents {
    /// Messages and calls, oldest first.
    pub events: Vec<TimelineEvent>,
    /// Whether an older page may exist. It can be followed by an empty page,
    /// which is the start of the thread.
    pub has_more: bool,
    /// Whether an older page is on its way.
    pub loading_older: bool,
    /// Why the last older page failed, shown at the top where it was asked
    /// for. The events already read stay.
    pub older_failure: Option<FailureText>,
    /// Whether the newest page is being read again, with these still showing.
    pub refreshing: bool,
    /// Why the last read of the newest page failed, shown beside the thread.
    pub refresh_failure: Option<FailureText>,
    older: Option<TimelineCursor>,
}

impl ThreadEvents {
    /// Whether to offer "Older messages" now.
    pub fn can_load_older(&self) -> bool {
        self.has_more && self.older.is_some() && !self.loading_older
    }

    fn from_page(page: TimelineResponse) -> Self {
        let mut events = page.timeline;
        sort(&mut events);
        Self {
            events,
            has_more: page.page_info.has_more,
            loading_older: false,
            older_failure: None,
            refreshing: false,
            refresh_failure: None,
            older: page.page_info.older_page(),
        }
    }
}

/// The reply being written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Composer {
    /// What is typed.
    pub text: String,
    /// The uploaded attachments waiting to go with the next message, each the
    /// URL the service answered the upload with.
    pub attachments: Vec<String>,
    /// Whether a message is on its way.
    pub sending: bool,
    /// Whether an upload is on its way.
    pub attaching: bool,
    /// Whether a reply is being written by the model.
    pub generating: bool,
    /// Why the last send, upload or written reply failed, shown above the
    /// composer. The thread and what was typed stay.
    pub failure: Option<FailureText>,
    /// A save is waiting for the typing to stop.
    pub(crate) save_pending: bool,
    /// Something was typed or written since the thread opened, so a saved draft
    /// arriving late must not replace it.
    pub(crate) edited: bool,
    /// The text and attachments of the message on its way.
    sent: Option<(String, Vec<String>)>,
}

/// An image the user picked, read into memory by the app.
///
/// Bytes rather than a path, so the upload can be sent again from the same
/// value, and so the file is read once, when it was picked. Its `Debug` output
/// leaves the bytes out: they are customer data, and there may be five
/// megabytes of them.
#[derive(Clone, PartialEq, Eq)]
pub struct PickedAttachment {
    /// The file's name, for the upload's form part.
    pub file_name: String,
    /// The type the desktop reported for it.
    pub mime_type: String,
    /// The file.
    pub bytes: Vec<u8>,
}

impl PickedAttachment {
    /// Why the service would refuse this as the next of `held` attachments, in
    /// its words, or `None` when it would take it. The limits named are the
    /// constants checked, so the words cannot fall behind them.
    pub fn problem(&self, held: usize) -> Option<String> {
        if held >= MAX_ATTACHMENTS {
            Some(s::thread_too_many_attachments(l(), MAX_ATTACHMENTS as i64))
        } else if !ATTACHMENT_TYPES.contains(&self.mime_type.as_str()) {
            Some(s::failure_unsupported_attachment(l()).to_owned())
        } else if self.bytes.is_empty() || self.bytes.len() > MAX_ATTACHMENT_BYTES {
            Some(s::thread_attachment_size(
                l(),
                (MAX_ATTACHMENT_BYTES / (1024 * 1024)) as i64,
            ))
        } else {
            None
        }
    }
}

impl fmt::Debug for PickedAttachment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PickedAttachment")
            .field("mime_type", &self.mime_type)
            .field("len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// What the user does in the open thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadEvent {
    /// The composer's text changed.
    Compose(String),
    /// Send what is typed, with the attachments.
    Send,
    /// Read the page before the oldest one showing.
    LoadOlder,
    /// Attach this image.
    Attach(PickedAttachment),
    /// The picked file could not be read at all.
    AttachFailed,
    /// Take the attachment with this URL off the message.
    RemoveAttachment(String),
    /// Have the model write a reply into the composer. Billed.
    DraftReply,
    /// Dismiss the composer's failure.
    DismissFailure,
    /// Reply on this channel, one of the thread's
    /// [`reply_choices`](ThreadScreen::reply_choices).
    ChooseChannel(String),
    /// Give the thread to this member, or take it back with `None`.
    Assign(Option<String>),
    /// Dismiss the owner picker's failure.
    DismissAssignFailure,
    /// Choose the WhatsApp template with this id.
    ChooseTemplate(String),
    /// The `index`th template variable (from zero) changed.
    TemplateVariable(usize, String),
    /// Send the chosen template, filled. Billed.
    SendTemplate,
}

impl SignedIn {
    /// Opens the thread `thread_key`, or reads it again when it is the one
    /// already open, and marks it read.
    pub(crate) fn open_thread(
        &mut self,
        thread_key: String,
        thread: ThreadRef,
        tickets: &mut Tickets,
    ) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.thread.is_none() {
            let conversation = self.inbox.conversation(&thread_key);
            let reply_choices: Vec<ReplyTarget> = match conversation {
                Some(conversation) if self.meta_channels => conversation.reply_choices(),
                Some(conversation) => conversation.reply_target().into_iter().collect(),
                None => Vec::new(),
            };
            let assignee = conversation.and_then(|c| c.assignee()).map(str::to_owned);
            self.thread = Some(ThreadScreen {
                title: title(&self.inbox, &thread_key, &thread),
                reply_target: reply_choices.first().cloned(),
                reply_choices,
                history: ThreadHistory::Loading,
                composer: Composer::default(),
                assignment: ThreadAssignment {
                    assignee,
                    ..ThreadAssignment::default()
                },
                whatsapp: None,
                thread_key,
                thread,
                workspace_id: self.workspace_id(),
                meta_channels: self.meta_channels,
            });
            effects.extend(self.load_draft(tickets));
            effects.extend(self.refresh_thread(tickets));
            effects.extend(self.mark_open_thread_read(tickets));
            effects.extend(self.load_assignees(tickets));
            // The window matters only to a member who can reply.
            if self.capabilities().can_change
                && let Some(screen) = self.thread.as_mut()
            {
                effects.extend(enter_channel(screen, tickets));
            }
            return effects;
        }
        effects.extend(self.refresh_thread(tickets));
        effects.extend(self.mark_open_thread_read(tickets));
        effects
    }

    /// Reads the members the open thread can be given to, for a member who may
    /// give it.
    fn load_assignees(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if !self.capabilities().can_change {
            return Vec::new();
        }
        self.thread
            .as_mut()
            .map(|screen| {
                screen.assignment.members = AssigneeList::Loading;
                Effect::LoadMembers {
                    ticket: tickets.issue(Slot::ThreadMembers),
                    workspace_id: screen.workspace_id.clone(),
                }
            })
            .into_iter()
            .collect()
    }

    /// The members were read: the open thread's picker takes them, or else
    /// `result` is someone else's to read.
    pub(crate) fn thread_members_loaded(
        &mut self,
        ticket: Ticket,
        result: Result<MemberListResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Option<Result<MemberListResponse, ApiError>> {
        // The slot is the open thread's, forgotten when it closes.
        if tickets.accept(Slot::ThreadMembers, ticket)
            && let Some(screen) = self.thread.as_mut()
        {
            screen.assignment.members = match result {
                Ok(answer) => AssigneeList::Ready(
                    answer
                        .members
                        .into_iter()
                        .filter(|member| {
                            matches!(
                                WorkspaceRole::from_wire(&member.role),
                                Some(WorkspaceRole::Agency | WorkspaceRole::Client)
                            )
                        })
                        .map(|member| member.email)
                        .collect(),
                ),
                Err(error) => AssigneeList::Failed(FailureText::from_api_error(&error)),
            };
            return None;
        }
        Some(result)
    }

    /// The thread was given to someone, or taken back, or the service refused.
    pub(crate) fn conversation_assigned(
        &mut self,
        ticket: Ticket,
        result: Result<ConversationAssignResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        // The answer is the thread's it was asked for, which need not be the
        // one open now: the member may have gone on to another.
        let Some(thread_key) = tickets.accept_keyed(Slot::Assign, ticket) else {
            return stay();
        };
        let assignment = self
            .thread
            .as_mut()
            .filter(|screen| screen.thread_key == thread_key)
            .map(|screen| &mut screen.assignment);
        match result {
            Ok(answer) => {
                let assignee = answer.assignment.map(|given| given.assignee_email);
                if let Some(assignment) = assignment {
                    assignment.saving = false;
                    assignment.assignee.clone_from(&assignee);
                }
                self.inbox.set_assignee_locally(&thread_key, assignee);
                // Read again for "assigned to me", which the service decides.
                Next::Stay(self.reload_conversations(tickets))
            }
            Err(error) => {
                // A refusal is said beside the picker it came from, or not at
                // all once that thread is closed.
                if let Some(assignment) = assignment {
                    assignment.saving = false;
                    assignment.failure = Some(FailureText::from_api_error(&error));
                }
                stay()
            }
        }
    }

    /// The open thread's WhatsApp window was read. Closed, the sender's
    /// templates are read next.
    pub(crate) fn whatsapp_window_loaded(
        &mut self,
        ticket: Ticket,
        result: Result<WhatsAppWindowResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        // The slot is forgotten when the thread closes and whenever the channel
        // changes, so an accepted answer is always the WhatsApp composer's.
        let Some((workspace_id, whatsapp)) = self
            .thread
            .as_mut()
            .filter(|_| tickets.accept(Slot::WhatsAppWindow, ticket))
            .and_then(|screen| {
                let whatsapp = screen.whatsapp.as_mut()?;
                Some((screen.workspace_id.clone(), whatsapp))
            })
        else {
            return stay();
        };
        match result {
            Ok(answer) => {
                whatsapp.sender_id = answer.sender_id.filter(|id| !id.is_empty());
                if answer.window.open {
                    whatsapp.window = WhatsAppWindowState::Open {
                        expires_at: answer.window.expires_at,
                    };
                    // No templates while it is open, even when a refusal had
                    // said it was closed and the window was read again.
                    whatsapp.templates = TemplateList::NotLoaded;
                    stay()
                } else {
                    whatsapp.window = WhatsAppWindowState::Closed;
                    Next::Stay(load_templates(whatsapp, workspace_id, tickets))
                }
            }
            // Read again after the service refused text because the window had
            // closed: it stays closed, and it is the templates that could not
            // be read.
            Err(error) if whatsapp.window == WhatsAppWindowState::Closed => {
                whatsapp.templates = TemplateList::Failed(FailureText::from_api_error(&error));
                stay()
            }
            Err(error) => {
                whatsapp.window = WhatsAppWindowState::Unknown(FailureText::from_api_error(&error));
                stay()
            }
        }
    }

    /// A WhatsApp sender's templates were read: the ones that may be sent are
    /// kept.
    pub(crate) fn whatsapp_templates_loaded(
        &mut self,
        ticket: Ticket,
        result: Result<WhatsAppTemplatesResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        // Forgotten with the thread and the channel, like the window's.
        let Some(whatsapp) = self
            .thread
            .as_mut()
            .filter(|_| tickets.accept(Slot::WhatsAppTemplates, ticket))
            .and_then(|screen| screen.whatsapp.as_mut())
        else {
            return stay();
        };
        // The ticket is accepted only while a sender is named: it was issued
        // for one, and a change of channel cancels it.
        let sender_id = whatsapp.sender_id.clone().unwrap_or_default();
        whatsapp.templates = match result {
            Ok(answer) => TemplateList::Ready(
                answer
                    .templates
                    .into_iter()
                    .filter_map(|template| {
                        let variable_count = template.sendable_variables(&sender_id)?;
                        Some(SendableTemplate {
                            id: template.id,
                            name: template.name,
                            language: template.language,
                            body: template.body,
                            variable_count,
                        })
                    })
                    .collect(),
            ),
            Err(error) => TemplateList::Failed(FailureText::from_api_error(&error)),
        };
        stay()
    }

    /// Reads the saved reply, for a member who can have one.
    fn load_draft(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if !self.capabilities().can_change {
            return Vec::new();
        }
        self.thread
            .as_ref()
            .map(|screen| Effect::LoadDraft {
                ticket: tickets.issue(Slot::DraftLoad),
                workspace_id: screen.workspace_id.clone(),
                thread_key: screen.thread_key.clone(),
            })
            .into_iter()
            .collect()
    }

    /// Reads the open thread's newest page, at the user's asking.
    pub(crate) fn refresh_thread(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.thread
            .as_mut()
            .map(|screen| {
                if let ThreadHistory::Ready(events) = &mut screen.history {
                    events.refreshing = true;
                }
                newest(screen, tickets.issue(Slot::Timeline))
            })
            .into_iter()
            .collect()
    }

    /// Reads the open thread's newest page again because it may have changed,
    /// quietly.
    pub(crate) fn reload_thread(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let Some(screen) = &self.thread else {
            return Vec::new();
        };
        tickets
            .refresh(Slot::Timeline)
            .map(|ticket| newest(screen, ticket))
            .into_iter()
            .collect()
    }

    /// Marks the open thread read, on screen at once and on the service. Not for
    /// a viewer, whom the service refuses: the thread stays unread for the
    /// members who can answer it.
    pub(crate) fn mark_open_thread_read(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        if !self.capabilities().can_change {
            return Vec::new();
        }
        self.thread
            .as_ref()
            .map(|screen| {
                (
                    screen.thread_key.clone(),
                    screen.thread.clone(),
                    screen.workspace_id.clone(),
                )
            })
            .map(|(thread_key, thread, workspace_id)| {
                self.inbox.mark_read_locally(&thread_key, &mut self.unread);
                Effect::MarkRead {
                    ticket: tickets.issue(Slot::MarkRead),
                    workspace_id,
                    thread,
                }
            })
            .into_iter()
            .collect()
    }

    /// Closes the open thread. A save still waiting for the typing to stop is
    /// sent now rather than lost.
    pub(crate) fn close_thread(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        let effects = self.flush_draft(tickets);
        tickets.cancel_each(&THREAD_SLOTS);
        self.thread = None;
        effects
    }

    /// The app is quitting. A save still waiting for the typing to stop goes
    /// now, and so does every write queued behind the one on its way, rather
    /// than being lost with the app. The thread stays open.
    pub(crate) fn quit_drafts(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        tickets.cancel(Slot::DraftTimer);
        let mut effects = self.flush_draft(tickets);
        effects.extend(self.draft_writes.drain(tickets));
        effects
    }

    /// The open thread's save still waiting for the typing to stop, written
    /// now; nothing when none is waiting.
    fn flush_draft(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.thread
            .as_mut()
            .filter(|screen| screen.composer.save_pending)
            .map(|screen| {
                write_draft(
                    screen,
                    &mut self.inbox.draft_keys,
                    &mut self.draft_writes,
                    tickets,
                )
            })
            .unwrap_or_default()
    }

    /// The composer's save timer ran out.
    pub(crate) fn save_draft_now(&mut self, tickets: &mut Tickets) -> Vec<Effect> {
        self.thread
            .as_mut()
            .map(|screen| {
                write_draft(
                    screen,
                    &mut self.inbox.draft_keys,
                    &mut self.draft_writes,
                    tickets,
                )
            })
            .unwrap_or_default()
    }

    /// A saved reply was written or deleted. Nothing waits on the answer, but
    /// the next write waiting goes now.
    pub(crate) fn draft_written(&mut self, ticket: Ticket, tickets: &mut Tickets) -> Next {
        if !tickets.accept(Slot::DraftWrite, ticket) {
            return stay();
        }
        Next::Stay(self.draft_writes.landed(tickets))
    }

    pub(crate) fn thread_event(&mut self, event: ThreadEvent, tickets: &mut Tickets) -> Next {
        let capabilities = self.capabilities();
        let draft_keys = &mut self.inbox.draft_keys;
        let draft_writes = &mut self.draft_writes;
        let Some(screen) = self.thread.as_mut() else {
            return stay();
        };
        let can_reply = screen.controls(&capabilities).can_reply;
        let effects = match event {
            ThreadEvent::Compose(text) if can_reply => compose(screen, text, tickets),
            ThreadEvent::Send => send(screen, &capabilities, draft_keys, draft_writes, tickets),
            ThreadEvent::LoadOlder => load_older(screen, tickets),
            ThreadEvent::Attach(picked) => attach(screen, &capabilities, picked, tickets),
            ThreadEvent::AttachFailed if screen.controls(&capabilities).can_attach => {
                screen.composer.failure =
                    Some(FailureText::final_(s::failure_unreadable_attachment(l())));
                Vec::new()
            }
            ThreadEvent::RemoveAttachment(url) => {
                screen.composer.attachments.retain(|held| *held != url);
                Vec::new()
            }
            ThreadEvent::DraftReply => draft_reply(screen, &capabilities, tickets),
            ThreadEvent::DismissFailure => {
                screen.composer.failure = None;
                Vec::new()
            }
            ThreadEvent::ChooseChannel(channel) => {
                choose_channel(screen, &capabilities, &channel, tickets)
            }
            ThreadEvent::Assign(assignee) => assign(screen, &capabilities, assignee, tickets),
            ThreadEvent::DismissAssignFailure => {
                screen.assignment.failure = None;
                Vec::new()
            }
            ThreadEvent::ChooseTemplate(id) => {
                if let Some(whatsapp) = screen.whatsapp.as_mut().filter(|w| !w.sending) {
                    whatsapp.chosen = Some(id);
                    whatsapp.variables.clear();
                }
                Vec::new()
            }
            ThreadEvent::TemplateVariable(index, value) => {
                if let Some(whatsapp) = screen.whatsapp.as_mut().filter(|w| !w.sending)
                    && index < whatsapp.template().map_or(0, |t| t.variable_count)
                {
                    if whatsapp.variables.len() <= index {
                        whatsapp.variables.resize(index + 1, String::new());
                    }
                    whatsapp.variables[index] = value;
                }
                Vec::new()
            }
            ThreadEvent::SendTemplate => send_template(screen, &capabilities, tickets),
            ThreadEvent::Compose(_) | ThreadEvent::AttachFailed => Vec::new(),
        };
        Next::Stay(effects)
    }

    pub(crate) fn timeline_loaded(
        &mut self,
        ticket: Ticket,
        result: Result<TimelineResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::Timeline, ticket) {
            if let Some(screen) = self.thread.as_mut() {
                newest_loaded(screen, result, tickets);
            }
            if tickets.take_again(Slot::Timeline) {
                return Next::Stay(self.reload_thread(tickets));
            }
        } else if tickets.accept(Slot::TimelineOlder, ticket)
            && let Some(ThreadHistory::Ready(held)) =
                self.thread.as_mut().map(|screen| &mut screen.history)
        {
            older_loaded(held, result);
        }
        stay()
    }

    pub(crate) fn draft_loaded(
        &mut self,
        ticket: Ticket,
        result: Result<DraftResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        // A failure leaves the box as it is: the draft is a convenience, and an
        // empty box is what the user would have had without it.
        if tickets.accept(Slot::DraftLoad, ticket)
            && let (Some(screen), Ok(saved)) = (self.thread.as_mut(), result)
            && let Some(draft) = saved.draft
            && !screen.composer.edited
        {
            // Attachments come back with it: restoring the text alone would send
            // a message the user believed had pictures on it.
            screen.composer.text = draft.body;
            screen.composer.attachments = draft.media_urls;
        }
        stay()
    }

    pub(crate) fn message_sent(
        &mut self,
        ticket: Ticket,
        result: Result<SendMessageResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::ComposeSend, ticket) {
            return self.compose_sent(result, tickets);
        }
        if !tickets.accept(Slot::Send, ticket) {
            return stay();
        }
        let mut effects = self
            .thread
            .as_mut()
            .map(|screen| sent(screen, result, tickets))
            .unwrap_or_default();
        effects.extend(self.reload_conversations(tickets));
        Next::Stay(effects)
    }

    pub(crate) fn media_uploaded(
        &mut self,
        ticket: Ticket,
        result: Result<MediaUploadResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::Upload, ticket)
            && let Some(screen) = self.thread.as_mut()
        {
            let composer = &mut screen.composer;
            composer.attaching = false;
            match result.map(|uploaded| uploaded.media) {
                Ok(Some(media)) => composer.attachments.push(media.url),
                Ok(None) => composer.failure = Some(FailureText::unexpected()),
                Err(error) => composer.failure = Some(FailureText::from_api_error(&error)),
            }
        }
        stay()
    }

    /// The service marked the thread read. The badge is read again, because the
    /// count taken off it on screen was only what the list knew. A failure
    /// leaves the badge as it is until the next read: it is a count.
    pub(crate) fn marked_read(
        &mut self,
        ticket: Ticket,
        result: Result<MarkReadResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if tickets.accept(Slot::MarkRead, ticket) && result.is_ok() {
            return Next::Stay(self.reload_unread(tickets));
        }
        stay()
    }

    pub(crate) fn ai_draft_written(
        &mut self,
        ticket: Ticket,
        result: Result<AiDraftResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        if !tickets.accept(Slot::AiDraft, ticket) {
            return stay();
        }
        let effects = self
            .thread
            .as_mut()
            .map(|screen| ai_draft_arrived(screen, result, tickets))
            .unwrap_or_default();
        Next::Stay(effects)
    }
}

/// The message on its way was sent, or refused.
fn sent(
    screen: &mut ThreadScreen,
    result: Result<SendMessageResponse, ApiError>,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    if let Some(whatsapp) = screen.whatsapp.as_mut().filter(|w| w.sending) {
        whatsapp.sending = false;
        return match result {
            Ok(_) => {
                whatsapp.chosen = None;
                whatsapp.variables.clear();
                vec![newest(screen, tickets.issue(Slot::Timeline))]
            }
            Err(error) => {
                screen.composer.failure = Some(FailureText::from_api_error(&error));
                Vec::new()
            }
        };
    }
    let mut effects = Vec::new();
    // The window closed while the reply was written: the service refused the
    // text, so the composer now offers the templates instead.
    if let (Err(error), Some(whatsapp)) = (&result, screen.whatsapp.as_mut())
        && error.code() == Some(WHATSAPP_WINDOW_CLOSED)
        && whatsapp.window != WhatsAppWindowState::Closed
    {
        whatsapp.window = WhatsAppWindowState::Closed;
        let workspace_id = screen.workspace_id.clone();
        match (&whatsapp.sender_id, &screen.reply_target) {
            // The window was not read, or named no sender: the templates are
            // a sender's, so the window is read again for the one the service
            // now names. That read stands in for any still on its way.
            (None, Some(target)) => {
                whatsapp.templates = TemplateList::Loading;
                effects.push(Effect::LoadWhatsAppWindow {
                    ticket: tickets.issue(Slot::WhatsAppWindow),
                    workspace_id,
                    to: target.to.clone(),
                });
            }
            _ => effects.extend(load_templates(whatsapp, workspace_id, tickets)),
        }
    }
    let composer = &mut screen.composer;
    composer.sending = false;
    let (body, media) = composer.sent.take().unwrap_or_default();
    match result {
        Ok(_) => {
            // What was typed while it was on its way is kept, and saved,
            // because the draft went with the message.
            composer.attachments.retain(|url| !media.contains(url));
            if composer.text == body {
                composer.text.clear();
            } else {
                effects.extend(schedule_save(screen, tickets));
            }
            // Read back rather than drawn here: the sent message gets its id,
            // status and time from the service, and a status this app made up
            // is the one thing a user checks after sending.
            effects.push(newest(screen, tickets.issue(Slot::Timeline)));
        }
        Err(error) => {
            composer.failure = Some(FailureText::from_api_error(&error));
            effects.extend(schedule_save(screen, tickets));
        }
    }
    effects
}

/// The written reply goes into the composer as if it had been typed, and is
/// saved like anything typed. An empty one is dropped: blanking a composer the
/// user had typed in is the worst thing this button could do.
fn ai_draft_arrived(
    screen: &mut ThreadScreen,
    result: Result<AiDraftResponse, ApiError>,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    screen.composer.generating = false;
    match result {
        Ok(written) if !written.draft.trim().is_empty() => compose(screen, written.draft, tickets),
        Ok(_) => Vec::new(),
        Err(error) => {
            screen.composer.failure = Some(FailureText::from_api_error(&error));
            Vec::new()
        }
    }
}

/// The title for the thread `thread_key`: the listed thread's name, else a
/// search result's, else the address the key names.
fn title(inbox: &InboxScreen, thread_key: &str, thread: &ThreadRef) -> String {
    if let Some(conversation) = inbox.conversation(thread_key) {
        return conversation.display_name().to_owned();
    }
    if let Some(hit) = inbox
        .search
        .hits
        .iter()
        .find(|hit| hit.thread_key == thread_key)
    {
        return hit.display_name().to_owned();
    }
    match thread {
        ThreadRef::Address(address) => address.clone(),
        ThreadRef::Contact(_) => s::thread_untitled(l()).to_owned(),
    }
}

/// The read of `screen`'s newest page, under `ticket`.
fn newest(screen: &ThreadScreen, ticket: Ticket) -> Effect {
    Effect::LoadTimeline {
        ticket,
        workspace_id: screen.workspace_id.clone(),
        thread: screen.thread.clone(),
        older_than: None,
        meta_channels: screen.meta_channels,
    }
}

/// The reply now goes on the channel chosen: on WhatsApp, its window is read;
/// on any other, whatever WhatsApp held is forgotten.
fn enter_channel(screen: &mut ThreadScreen, tickets: &mut Tickets) -> Vec<Effect> {
    tickets.cancel(Slot::WhatsAppWindow);
    tickets.cancel(Slot::WhatsAppTemplates);
    let Some(target) = screen
        .reply_target
        .as_ref()
        .filter(|target| target.channel == CHANNEL_WHATSAPP)
    else {
        screen.whatsapp = None;
        return Vec::new();
    };
    let effect = Effect::LoadWhatsAppWindow {
        ticket: tickets.issue(Slot::WhatsAppWindow),
        workspace_id: screen.workspace_id.clone(),
        to: target.to.clone(),
    };
    screen.whatsapp = Some(WhatsAppComposer::default());
    vec![effect]
}

/// Reads the templates of the sender whose window is closed, when the service
/// named one. With none named there is nothing to read them by, and the
/// composer says there is no template.
fn load_templates(
    whatsapp: &mut WhatsAppComposer,
    workspace_id: String,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    let Some(sender_id) = whatsapp.sender_id.clone() else {
        whatsapp.templates = TemplateList::Ready(Vec::new());
        return Vec::new();
    };
    whatsapp.templates = TemplateList::Loading;
    vec![Effect::LoadWhatsAppTemplates {
        ticket: tickets.issue(Slot::WhatsAppTemplates),
        workspace_id,
        sender_id,
    }]
}

fn choose_channel(
    screen: &mut ThreadScreen,
    capabilities: &Capabilities,
    channel: &str,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    if !screen.controls(capabilities).can_choose_channel || screen.on_channel(channel) {
        return Vec::new();
    }
    let Some(choice) = screen
        .reply_choices
        .iter()
        .find(|choice| choice.channel == channel)
        .cloned()
    else {
        return Vec::new();
    };
    screen.reply_target = Some(choice);
    screen.composer.failure = None;
    enter_channel(screen, tickets)
}

fn assign(
    screen: &mut ThreadScreen,
    capabilities: &Capabilities,
    assignee: Option<String>,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    let assignment = &screen.assignment;
    let offered = assignee
        .as_ref()
        .is_none_or(|email| assignment.options().contains(email));
    if !screen.controls(capabilities).can_assign || !offered || assignee == assignment.assignee {
        return Vec::new();
    }
    let assignment = &mut screen.assignment;
    assignment.saving = true;
    assignment.failure = None;
    vec![Effect::AssignConversation {
        // Keyed by the thread, so its answer lands on it however far the
        // member has moved on.
        ticket: tickets.issue_keyed(Slot::Assign, &screen.thread_key),
        workspace_id: screen.workspace_id.clone(),
        thread_key: screen.thread_key.clone(),
        assignee_email: assignee,
    }]
}

fn send_template(
    screen: &mut ThreadScreen,
    capabilities: &Capabilities,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    // A template can be sent only when one is chosen on WhatsApp.
    let can_send = screen.controls(capabilities).can_send_template;
    let chosen = screen.whatsapp.as_mut().and_then(|whatsapp| {
        let template = whatsapp.template()?.clone();
        Some((whatsapp, template))
    });
    let (true, Some(target), Some((whatsapp, template))) =
        (can_send, screen.reply_target.clone(), chosen)
    else {
        return Vec::new();
    };
    let variables = whatsapp.filled_variables();
    let message = SendMessageRequest {
        to: target.to,
        body: fill_whatsapp_template(&template.body, &variables),
        channel: CHANNEL_WHATSAPP.to_owned(),
        whatsapp_sender_id: whatsapp.sender_id.clone(),
        template_id: Some(template.id),
        template_variables: Some(variables),
        ..SendMessageRequest::default()
    };
    whatsapp.sending = true;
    screen.composer.failure = None;
    vec![Effect::SendMessage {
        ticket: tickets.issue(Slot::Send),
        workspace_id: screen.workspace_id.clone(),
        message,
    }]
}

/// The newest page arrived.
fn newest_loaded(
    screen: &mut ThreadScreen,
    result: Result<TimelineResponse, ApiError>,
    tickets: &mut Tickets,
) {
    match (result, &mut screen.history) {
        (Ok(page), ThreadHistory::Ready(held)) => merge_newest(held, page, tickets),
        (Ok(page), history) => *history = ThreadHistory::Ready(ThreadEvents::from_page(page)),
        (Err(error), ThreadHistory::Ready(held)) => {
            held.refreshing = false;
            held.refresh_failure = Some(FailureText::from_api_error(&error));
        }
        (Err(error), history) => {
            *history = ThreadHistory::Failed(FailureText::from_api_error(&error));
        }
    }
}

/// Merges the newest page into what is held. The service's fresh copy of an
/// event wins, because a message's delivery status changes. A page that shares
/// nothing with what is held means more happened than one page holds, so the
/// history starts again from it rather than leave a gap in the middle.
fn merge_newest(held: &mut ThreadEvents, page: TimelineResponse, tickets: &mut Tickets) {
    held.refreshing = false;
    held.refresh_failure = None;
    let fresh: BTreeSet<&str> = page
        .timeline
        .iter()
        .map(|event| event.id.as_str())
        .collect();
    let overlaps = held
        .events
        .iter()
        .any(|event| fresh.contains(event.id.as_str()));
    if !overlaps {
        tickets.cancel(Slot::TimelineOlder);
        *held = ThreadEvents::from_page(page);
        return;
    }
    let fresh: BTreeSet<String> = fresh.into_iter().map(str::to_owned).collect();
    held.events.retain(|event| !fresh.contains(&event.id));
    held.events.extend(page.timeline);
    sort(&mut held.events);
}

/// An older page arrived. The copy already on screen wins a collision: both are
/// the same row, and replacing it would redraw what the user is reading.
fn older_loaded(held: &mut ThreadEvents, result: Result<TimelineResponse, ApiError>) {
    held.loading_older = false;
    match result {
        Ok(page) => {
            let known: BTreeSet<String> =
                held.events.iter().map(|event| event.id.clone()).collect();
            held.events.extend(
                page.timeline
                    .into_iter()
                    .filter(|event| !known.contains(&event.id)),
            );
            sort(&mut held.events);
            // Taken from the new page, not combined with the old: an empty page
            // says there is nothing older, whatever the last one hoped.
            held.has_more = page.page_info.has_more;
            held.older = page.page_info.older_page();
        }
        Err(error) => held.older_failure = Some(FailureText::from_api_error(&error)),
    }
}

/// The service's own order: by timestamp, then id.
fn sort(events: &mut [TimelineEvent]) {
    events.sort_by(|a, b| (&a.timestamp, &a.id).cmp(&(&b.timestamp, &b.id)));
}

fn load_older(screen: &mut ThreadScreen, tickets: &mut Tickets) -> Vec<Effect> {
    let ThreadHistory::Ready(held) = &mut screen.history else {
        return Vec::new();
    };
    if !held.can_load_older() {
        return Vec::new();
    }
    held.loading_older = true;
    held.older_failure = None;
    vec![Effect::LoadTimeline {
        ticket: tickets.issue(Slot::TimelineOlder),
        workspace_id: screen.workspace_id.clone(),
        thread: screen.thread.clone(),
        older_than: held.older.clone(),
        meta_channels: screen.meta_channels,
    }]
}

/// The text changed: keep it, and save it once the typing stops. The timer
/// starts again at each change, so a steady typist saves when they pause.
fn compose(screen: &mut ThreadScreen, text: String, tickets: &mut Tickets) -> Vec<Effect> {
    screen.composer.text = text;
    screen.composer.edited = true;
    schedule_save(screen, tickets)
}

fn schedule_save(screen: &mut ThreadScreen, tickets: &mut Tickets) -> Vec<Effect> {
    screen.composer.save_pending = true;
    vec![Effect::Wait {
        ticket: tickets.issue(Slot::DraftTimer),
        delay: DRAFT_SAVE_DEBOUNCE,
    }]
}

/// Saves the composer's text as the member's draft, or deletes the draft when
/// the text is blank: the service refuses a blank one.
fn write_draft(
    screen: &mut ThreadScreen,
    draft_keys: &mut BTreeSet<String>,
    draft_writes: &mut DraftWrites,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    screen.composer.save_pending = false;
    let workspace_id = screen.workspace_id.clone();
    let thread_key = screen.thread_key.clone();
    let write = if screen.composer.text.trim().is_empty() {
        draft_keys.remove(&thread_key);
        DraftWrite::Delete {
            workspace_id,
            thread_key,
        }
    } else {
        draft_keys.insert(thread_key.clone());
        DraftWrite::Save {
            workspace_id,
            draft: DraftSaveRequest {
                thread_key,
                body: screen.composer.text.clone(),
                subject: None,
                media_urls: screen.composer.attachments.clone(),
            },
        }
    };
    draft_writes.write(write, tickets)
}

fn send(
    screen: &mut ThreadScreen,
    capabilities: &Capabilities,
    draft_keys: &mut BTreeSet<String>,
    draft_writes: &mut DraftWrites,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    if !screen.controls(capabilities).can_send {
        return Vec::new();
    }
    screen
        .reply_target
        .clone()
        .map(|target| start_send(screen, target, draft_keys, draft_writes, tickets))
        .unwrap_or_default()
}

fn start_send(
    screen: &mut ThreadScreen,
    target: ReplyTarget,
    draft_keys: &mut BTreeSet<String>,
    draft_writes: &mut DraftWrites,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    let composer = &mut screen.composer;
    composer.sending = true;
    composer.failure = None;
    composer.save_pending = false;
    tickets.cancel(Slot::DraftTimer);
    // The sender whose window was shown, so the reply goes from it.
    let whatsapp_sender_id = screen
        .whatsapp
        .as_ref()
        .and_then(|whatsapp| whatsapp.sender_id.clone());
    let message = SendMessageRequest {
        to: target.to,
        body: composer.text.clone(),
        channel: target.channel.to_owned(),
        media_urls: composer.attachments.clone(),
        whatsapp_sender_id,
        ..SendMessageRequest::default()
    };
    composer.sent = Some((message.body.clone(), message.media_urls.clone()));
    draft_keys.remove(&screen.thread_key);
    let mut effects = vec![Effect::SendMessage {
        ticket: tickets.issue(Slot::Send),
        workspace_id: screen.workspace_id.clone(),
        message,
    }];
    effects.extend(draft_writes.write(
        DraftWrite::Delete {
            workspace_id: screen.workspace_id.clone(),
            thread_key: screen.thread_key.clone(),
        },
        tickets,
    ));
    effects
}

fn attach(
    screen: &mut ThreadScreen,
    capabilities: &Capabilities,
    picked: PickedAttachment,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    if !screen.controls(capabilities).can_attach {
        return Vec::new();
    }
    let composer = &mut screen.composer;
    // Checked before the upload, so a file the service would refuse costs no
    // upload, and the refusal can name the rule. The service checks again.
    if let Some(problem) = picked.problem(composer.attachments.len()) {
        composer.failure = Some(FailureText::final_(problem));
        return Vec::new();
    }
    composer.attaching = true;
    composer.failure = None;
    vec![Effect::UploadMedia {
        ticket: tickets.issue(Slot::Upload),
        workspace_id: screen.workspace_id.clone(),
        attachment: picked,
    }]
}

fn draft_reply(
    screen: &mut ThreadScreen,
    capabilities: &Capabilities,
    tickets: &mut Tickets,
) -> Vec<Effect> {
    if !screen.controls(capabilities).can_draft_reply {
        return Vec::new();
    }
    screen.composer.generating = true;
    screen.composer.failure = None;
    vec![Effect::GenerateAiDraft {
        ticket: tickets.issue(Slot::AiDraft),
        workspace_id: screen.workspace_id.clone(),
        thread: screen.thread.clone(),
    }]
}
