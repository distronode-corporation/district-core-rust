//! Starting a conversation from the inbox: choose a contact or type a phone
//! number or an email address, write the first message, send it, and land in
//! the thread it started.
//!
//! The thread's key is the service's to decide, never this app's: it is
//! `contact:<id>` when the other party is a contact and `addr:<address>` when
//! not, and for a newly typed address the send itself may create the contact.
//! So the message is sent first and its thread then asked for by the message's
//! id, the way a notification's thread is.
//!
//! The send is billed and is sent once. When it went out and its thread cannot
//! be found, the form still closes: the message reached the customer, and
//! offering to send it again would send it twice. The list is read again, and
//! the new thread is at the top of it.
//!
//! The contacts offered are the most recent window only, because the contacts
//! route takes no search. Anybody else is reached by typing their number or
//! address, which lands in the same thread: the service resolves the address
//! to the contact either way. The form says so.

use district_api::ApiError;
use district_model::{
    CHANNEL_EMAIL, CHANNEL_SMS, Contact, ContactListResponse, MessageThreadResponse, ReplyTarget,
    SendMessageRequest, SendMessageResponse,
};

use crate::failure::FailureText;
use crate::model::{Effect, Slot, Tickets};
use crate::route::Route;
use crate::signed_in::{Next, SignedIn, stay};
use crate::text::{l, s};

/// How many of the most recent contacts the form offers, the size of a page
/// on the contacts screen.
pub const COMPOSE_CONTACT_WINDOW: u32 = 50;

/// The slots of the new message form, forgotten with the workspace.
pub(crate) const COMPOSE_SLOTS: [Slot; 3] = [
    Slot::ComposeContacts,
    Slot::ComposeSend,
    Slot::ComposeLookup,
];

/// The new message form, while it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposeScreen {
    /// What is typed in the recipient field: the filter over the contacts, and
    /// itself a recipient once [`ComposeEvent::UseTyped`] takes it.
    pub query: String,
    /// The contacts offered.
    pub contacts: ComposeContacts,
    /// Who the message goes to, once chosen.
    pub recipient: Option<ComposeRecipient>,
    /// Which of [`channel_choices`](Self::channel_choices) is chosen.
    pub channel: usize,
    /// The subject, for an email.
    pub subject: String,
    /// The message.
    pub body: String,
    /// Whether the message is on its way.
    pub sending: bool,
    /// Why the last send failed. What was typed stays.
    pub failure: Option<FailureText>,
}

/// The contacts the new message form offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComposeContacts {
    /// Being read.
    Loading,
    /// Read: the most recent window, newest first.
    Ready(Vec<Contact>),
    /// The read failed. The form still works with an address typed in, and
    /// says so ([`ComposeScreen::suggestions_failed`]).
    Failed(FailureText),
}

/// Who a new message goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComposeRecipient {
    /// A contact: the channels are its own phone number and email address.
    Contact(Box<Contact>),
    /// A phone number or email address as typed, trimmed. The service checks
    /// it; this app decides only its channel, by an `@`, as the service does.
    Typed(String),
}

impl ComposeScreen {
    fn new() -> Self {
        Self {
            query: String::new(),
            contacts: ComposeContacts::Loading,
            recipient: None,
            channel: 0,
            subject: String::new(),
            body: String::new(),
            sending: false,
            failure: None,
        }
    }

    /// The form's title.
    pub fn title() -> &'static str {
        s::inbox_compose_title(l())
    }

    /// The inbox's button that opens the form.
    pub fn action() -> &'static str {
        s::inbox_compose_action(l())
    }

    /// The recipient field's label.
    pub fn to_label() -> &'static str {
        s::inbox_compose_to(l())
    }

    /// The button beside the chosen recipient that goes back to choosing.
    pub fn change_label() -> &'static str {
        s::inbox_compose_change(l())
    }

    /// The note under the contacts offered: only the most recent are.
    pub fn recent_contacts_note() -> &'static str {
        s::inbox_compose_recent_contacts(l())
    }

    /// The label above the choice of channel.
    pub fn send_on_label() -> &'static str {
        s::inbox_compose_send_on(l())
    }

    /// The subject field's placeholder.
    pub fn subject_label() -> &'static str {
        s::inbox_compose_subject(l())
    }

    /// The message field's label.
    pub fn message_label() -> &'static str {
        s::inbox_compose_message(l())
    }

    /// The cancel button's label.
    pub fn cancel_label() -> &'static str {
        s::inbox_compose_cancel(l())
    }

    /// The send button's label, which says when the message is on its way.
    pub fn send_label(&self) -> &'static str {
        if self.sending {
            s::inbox_compose_sending(l())
        } else {
            s::inbox_compose_send(l())
        }
    }

    /// A channel's name: "Text message" or "Email".
    pub fn channel_label(channel: &str) -> &'static str {
        if channel == CHANNEL_EMAIL {
            s::inbox_compose_channel_email(l())
        } else {
            s::inbox_compose_channel_sms(l())
        }
    }

    /// The button that sends to what is typed, or `None` while nothing is.
    pub fn typed_action(&self) -> Option<String> {
        non_blank(&self.query).map(|typed| s::inbox_compose_message_address(l(), typed))
    }

    /// The sentence for contacts that could not be read, or `None` when they
    /// were.
    pub fn suggestions_failed(&self) -> Option<String> {
        match &self.contacts {
            ComposeContacts::Failed(failure) => {
                Some(s::inbox_compose_suggestions_failed(l(), &failure.message))
            }
            _ => None,
        }
    }

    /// The note for a chosen contact with nothing to send to, or `None`.
    pub fn no_address_note(&self) -> Option<&'static str> {
        (self.recipient.is_some() && self.channel_choices().is_empty())
            .then(|| s::inbox_compose_no_address(l()))
    }

    /// The contacts matching what is typed, by name, phone number or email
    /// address, ignoring case. Nothing typed shows them all, newest first.
    pub fn matches(&self) -> Vec<&Contact> {
        let ComposeContacts::Ready(contacts) = &self.contacts else {
            return Vec::new();
        };
        let needle = self.query.trim().to_lowercase();
        contacts
            .iter()
            .filter(|contact| {
                [
                    Some(contact.name.as_str()),
                    contact.phone_number.as_deref(),
                    contact.email.as_deref(),
                ]
                .into_iter()
                .flatten()
                .any(|field| field.to_lowercase().contains(&needle))
            })
            .collect()
    }

    /// Every way the chosen recipient can be reached, a text message first.
    /// A contact's come from its own phone number and email address; a typed
    /// address's from whether it holds an `@`. Empty for a contact with
    /// neither, or before a recipient is chosen.
    pub fn channel_choices(&self) -> Vec<ReplyTarget> {
        match &self.recipient {
            None => Vec::new(),
            Some(ComposeRecipient::Contact(contact)) => [
                (contact.phone_number.as_deref(), CHANNEL_SMS),
                (contact.email.as_deref(), CHANNEL_EMAIL),
            ]
            .into_iter()
            .filter_map(|(address, channel)| {
                address.and_then(non_blank).map(|to| ReplyTarget {
                    to: to.to_owned(),
                    channel,
                })
            })
            .collect(),
            Some(ComposeRecipient::Typed(address)) => vec![ReplyTarget {
                to: address.clone(),
                channel: if address.contains('@') {
                    CHANNEL_EMAIL
                } else {
                    CHANNEL_SMS
                },
            }],
        }
    }

    /// Where the message goes and on which channel, once that is known.
    pub fn target(&self) -> Option<ReplyTarget> {
        self.channel_choices().into_iter().nth(self.channel)
    }

    /// Whether the message goes as an email, which needs a subject: the
    /// service would send one without, under a subject of its own.
    pub fn requires_subject(&self) -> bool {
        self.target()
            .is_some_and(|target| target.channel == CHANNEL_EMAIL)
    }

    /// Whether "Send" works: a recipient with an address, a message, a subject
    /// for an email, and nothing on its way.
    pub fn can_send(&self) -> bool {
        !self.sending
            && self.target().is_some()
            && non_blank(&self.body).is_some()
            && (!self.requires_subject() || non_blank(&self.subject).is_some())
    }

    /// The request "Send" makes, when it can.
    fn request(&self) -> Option<SendMessageRequest> {
        let target = self.target().filter(|_| self.can_send())?;
        Some(SendMessageRequest {
            subject: self
                .requires_subject()
                .then(|| self.subject.trim().to_owned()),
            to: target.to,
            body: self.body.clone(),
            channel: target.channel.to_owned(),
            media_urls: Vec::new(),
        })
    }

    fn clear_failure(&mut self) {
        self.failure = None;
    }

    /// The contacts were read. A failure leaves the form working.
    pub(crate) fn contacts_loaded(&mut self, result: Result<ContactListResponse, ApiError>) {
        self.contacts = match result {
            Ok(page) => ComposeContacts::Ready(page.contacts),
            Err(error) => ComposeContacts::Failed(FailureText::from_api_error(&error)),
        };
    }
}

/// `value` trimmed, when it holds more than white space.
fn non_blank(value: &str) -> Option<&str> {
    Some(value.trim()).filter(|value| !value.is_empty())
}

/// What the member does in the new message form.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ComposeEvent {
    /// Open the form, empty, and read the most recent contacts.
    Open,
    /// The recipient field changed.
    Query(String),
    /// Send to the offered contact with this id.
    ChooseContact(String),
    /// Send to what is typed in the recipient field.
    UseTyped,
    /// Go back to choosing a recipient.
    ClearRecipient,
    /// Send on the channel at this index of
    /// [`ComposeScreen::channel_choices`].
    SelectChannel(usize),
    /// The subject changed.
    EditSubject(String),
    /// The message changed.
    EditBody(String),
    /// Send the message. Billed, and sent once.
    Send,
    /// Close the form. Refused while the message is on its way.
    Cancel,
}

impl SignedIn {
    pub(crate) fn compose_event(&mut self, event: ComposeEvent, tickets: &mut Tickets) -> Next {
        // Sending refuses a viewer, so the form is not offered to one.
        if !self.capabilities().can_change {
            return stay();
        }
        if event == ComposeEvent::Open {
            self.inbox.compose = Some(ComposeScreen::new());
            return Next::Stay(vec![Effect::LoadContacts {
                ticket: tickets.issue(Slot::ComposeContacts),
                workspace_id: self.workspace_id(),
                limit: COMPOSE_CONTACT_WINDOW,
                offset: 0,
            }]);
        }
        let workspace_id = self.workspace_id();
        let Some(compose) = self.inbox.compose.as_mut().filter(|c| !c.sending) else {
            return stay();
        };
        match event {
            ComposeEvent::Query(query) => compose.query = query,
            ComposeEvent::ChooseContact(id) => {
                let ComposeContacts::Ready(contacts) = &compose.contacts else {
                    return stay();
                };
                let Some(contact) = contacts.iter().find(|contact| contact.id == id) else {
                    return stay();
                };
                compose.recipient = Some(ComposeRecipient::Contact(Box::new(contact.clone())));
                compose.channel = 0;
                compose.clear_failure();
            }
            ComposeEvent::UseTyped => {
                let Some(typed) = non_blank(&compose.query).map(str::to_owned) else {
                    return stay();
                };
                compose.recipient = Some(ComposeRecipient::Typed(typed));
                compose.channel = 0;
                compose.clear_failure();
            }
            ComposeEvent::ClearRecipient => {
                compose.recipient = None;
                compose.channel = 0;
                compose.clear_failure();
            }
            ComposeEvent::SelectChannel(index) if index < compose.channel_choices().len() => {
                compose.channel = index;
            }
            ComposeEvent::EditSubject(subject) => compose.subject = subject,
            ComposeEvent::EditBody(body) => compose.body = body,
            ComposeEvent::Send => {
                let Some(message) = compose.request() else {
                    return stay();
                };
                compose.sending = true;
                compose.failure = None;
                return Next::Stay(vec![Effect::SendMessage {
                    ticket: tickets.issue(Slot::ComposeSend),
                    workspace_id,
                    message,
                }]);
            }
            ComposeEvent::Cancel => {
                tickets.cancel(Slot::ComposeContacts);
                self.inbox.compose = None;
            }
            ComposeEvent::Open | ComposeEvent::SelectChannel(_) => {}
        }
        stay()
    }

    /// The first message went out, or was refused. Out, the form closes and the
    /// message's thread is asked for; refused, what was typed stays with the
    /// service's reason.
    pub(crate) fn compose_sent(
        &mut self,
        result: Result<SendMessageResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        match result {
            Ok(answer) => {
                self.inbox.compose = None;
                let mut effects = self.reload_conversations(tickets);
                if let Some(message) = answer.message.filter(|message| !message.id.is_empty()) {
                    effects.push(Effect::FindMessageThread {
                        ticket: tickets.issue(Slot::ComposeLookup),
                        workspace_id: self.workspace_id(),
                        message_id: message.id,
                    });
                }
                Next::Stay(effects)
            }
            Err(error) => {
                if let Some(compose) = self.inbox.compose.as_mut() {
                    compose.sending = false;
                    compose.failure = Some(FailureText::from_api_error(&error));
                }
                stay()
            }
        }
    }

    /// The new message's thread is known: open it, unless the member has left
    /// the inbox meanwhile. Its reply goes where the service says the thread is
    /// reached, until the list holds the thread. A failure stays on the inbox,
    /// where the new thread is the newest.
    pub(crate) fn compose_thread_found(
        &mut self,
        result: Result<MessageThreadResponse, ApiError>,
        tickets: &mut Tickets,
    ) -> Next {
        let Ok(found) = result.map(|found| found.thread) else {
            return stay();
        };
        if self.route != Route::Inbox {
            return stay();
        }
        let next = self.navigate(
            Route::Thread {
                thread_key: found.thread_key,
            },
            tickets,
        );
        let channel = [CHANNEL_SMS, CHANNEL_EMAIL]
            .into_iter()
            .find(|channel| *channel == found.channel);
        if let Some(screen) = self.thread.as_mut().filter(|t| t.reply_target.is_none())
            && let (Some(to), Some(channel)) = (non_blank(&found.counterpart), channel)
        {
            screen.reply_target = Some(ReplyTarget {
                to: to.to_owned(),
                channel,
            });
        }
        next
    }
}
