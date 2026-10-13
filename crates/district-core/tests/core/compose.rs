//! Starting a conversation: a contact or a typed address, the channel, the
//! first message sent once, and the thread it started.

use district_core::{
    COMPOSE_CONTACT_WINDOW, ComposeContacts, ComposeEvent, ComposeRecipient, ComposeScreen, Effect,
    Event, InboxEvent, Model, Route, Ticket,
};
use district_model::{
    CHANNEL_EMAIL, CHANNEL_SMS, ContactListResponse, MessageThreadResponse, ReplyTarget,
    SendMessageRequest, SendMessageResponse,
};

use crate::inbox::{inbox, on_inbox};
use crate::support::{
    AGENCY, CLIENT, VIEWER, fixture, last_ticket, pick, refusal, server_error, signed_in,
};

const ADA_ID: &str = "contact_contract_1";
const SPARSE_ID: &str = "contact_contract_2";

fn compose(model: &Model) -> &ComposeScreen {
    inbox(model).compose.as_ref().expect("the form is open")
}

fn event(model: &mut Model, event: ComposeEvent) -> Vec<Effect> {
    model.update(Event::Inbox(InboxEvent::Compose(event)))
}

fn contacts() -> ContactListResponse {
    fixture("district-contacts.json")
}

/// On the inbox as an agency member, with the form open and its contacts
/// read as `answer`.
fn composing(answer: Result<ContactListResponse, district_api::ApiError>) -> Model {
    let mut model = on_inbox(AGENCY, "agency", 0);
    let effects = event(&mut model, ComposeEvent::Open);
    let [
        Effect::LoadContacts {
            limit,
            offset,
            workspace_id,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(
        (*limit, *offset, workspace_id.as_str()),
        (COMPOSE_CONTACT_WINDOW, 0, AGENCY)
    );
    assert_eq!(compose(&model).contacts, ComposeContacts::Loading);
    assert!(compose(&model).matches().is_empty());
    model.update(Event::ContactsLoaded {
        ticket: last_ticket(&effects),
        result: answer,
    });
    model
}

fn ids(model: &Model) -> Vec<&str> {
    compose(model)
        .matches()
        .into_iter()
        .map(|contact| contact.id.as_str())
        .collect()
}

/// The one message `effects` sends.
fn sent(effects: &[Effect]) -> (Ticket, SendMessageRequest) {
    match effects {
        [
            Effect::SendMessage {
                ticket, message, ..
            },
        ] => (*ticket, message.clone()),
        other => panic!("{other:?}"),
    }
}

fn target(to: &str, channel: &'static str) -> ReplyTarget {
    ReplyTarget {
        to: to.to_owned(),
        channel,
    }
}

#[test]
fn a_viewer_is_not_offered_the_form() {
    let mut model = on_inbox(VIEWER, "viewer", 0);
    assert!(event(&mut model, ComposeEvent::Open).is_empty());
    assert_eq!(inbox(&model).compose, None);
    assert!(event(&mut model, ComposeEvent::Send).is_empty());
}

#[test]
fn the_contacts_offered_match_by_name_number_or_address() {
    let mut model = composing(Ok(contacts()));
    assert_eq!(ids(&model), [ADA_ID, SPARSE_ID]);
    assert_eq!(compose(&model).suggestions_failed(), None);
    for (query, expected) in [
        ("sparse", vec![SPARSE_ID]),
        ("4165550142", vec![ADA_ID]),
        ("EXAMPLE.COM", vec![ADA_ID, SPARSE_ID]),
        ("nobody", vec![]),
    ] {
        event(&mut model, ComposeEvent::Query(query.to_owned()));
        assert_eq!(ids(&model), expected, "{query}");
    }
}

#[test]
fn contacts_that_could_not_be_read_leave_the_form_working() {
    let mut model = composing(Err(server_error()));
    let Some(said) = compose(&model).suggestions_failed() else {
        panic!("{:?}", compose(&model).contacts);
    };
    assert!(
        said.ends_with("You can still enter a number or an email address above."),
        "{said}"
    );
    assert!(compose(&model).matches().is_empty());
    // An unknown contact cannot be chosen, but an address can be typed.
    event(&mut model, ComposeEvent::ChooseContact(ADA_ID.to_owned()));
    assert_eq!(compose(&model).recipient, None);
    event(&mut model, ComposeEvent::Query(" +12125550142 ".to_owned()));
    assert_eq!(
        compose(&model).typed_action().as_deref(),
        Some("Message +12125550142")
    );
    event(&mut model, ComposeEvent::UseTyped);
    assert_eq!(
        compose(&model).recipient,
        Some(ComposeRecipient::Typed("+12125550142".to_owned()))
    );
    assert_eq!(
        compose(&model).channel_choices(),
        [target("+12125550142", CHANNEL_SMS)]
    );
}

#[test]
fn a_contact_is_reached_on_its_own_number_and_address_a_text_first() {
    let mut model = composing(Ok(contacts()));
    event(
        &mut model,
        ComposeEvent::ChooseContact("not-listed".to_owned()),
    );
    assert_eq!(compose(&model).recipient, None);
    assert_eq!(compose(&model).target(), None);
    event(&mut model, ComposeEvent::ChooseContact(ADA_ID.to_owned()));
    assert_eq!(
        compose(&model).channel_choices(),
        [
            target("14165550142", CHANNEL_SMS),
            target("ada@example.com", CHANNEL_EMAIL)
        ]
    );
    assert!(!compose(&model).requires_subject());
    assert_eq!(compose(&model).no_address_note(), None);
    event(&mut model, ComposeEvent::SelectChannel(1));
    assert!(compose(&model).requires_subject());
    // A channel the recipient does not have is not chosen.
    event(&mut model, ComposeEvent::SelectChannel(2));
    assert_eq!(compose(&model).channel, 1);
    // Choosing again starts from the first channel.
    event(
        &mut model,
        ComposeEvent::ChooseContact(SPARSE_ID.to_owned()),
    );
    assert_eq!(
        compose(&model).target(),
        Some(target("sparse@example.com", CHANNEL_EMAIL))
    );
    event(&mut model, ComposeEvent::ClearRecipient);
    assert_eq!(compose(&model).recipient, None);
}

#[test]
fn a_contact_with_nothing_to_send_to_says_so() {
    let mut answer = contacts();
    answer.contacts[0].phone_number = Some("  ".to_owned());
    answer.contacts[0].email = None;
    let mut model = composing(Ok(answer));
    event(&mut model, ComposeEvent::ChooseContact(ADA_ID.to_owned()));
    assert_eq!(
        compose(&model).no_address_note(),
        Some("This contact has no phone number or email address on file.")
    );
    event(&mut model, ComposeEvent::EditBody("Hello".to_owned()));
    assert!(!compose(&model).can_send());
    assert!(event(&mut model, ComposeEvent::Send).is_empty());
}

#[test]
fn a_typed_address_is_an_email_by_its_at_and_nothing_blank_is_taken() {
    let mut model = composing(Ok(contacts()));
    assert_eq!(compose(&model).typed_action(), None);
    event(&mut model, ComposeEvent::Query("   ".to_owned()));
    event(&mut model, ComposeEvent::UseTyped);
    assert_eq!(compose(&model).recipient, None);
    event(
        &mut model,
        ComposeEvent::Query("new@example.com".to_owned()),
    );
    event(&mut model, ComposeEvent::UseTyped);
    assert_eq!(
        compose(&model).target(),
        Some(target("new@example.com", CHANNEL_EMAIL))
    );
}

#[test]
fn an_email_needs_a_subject_and_goes_with_it_trimmed() {
    let mut model = composing(Ok(contacts()));
    event(
        &mut model,
        ComposeEvent::ChooseContact(SPARSE_ID.to_owned()),
    );
    assert!(!compose(&model).can_send());
    event(&mut model, ComposeEvent::EditBody("Hello there".to_owned()));
    assert!(!compose(&model).can_send());
    assert!(event(&mut model, ComposeEvent::Send).is_empty());
    event(
        &mut model,
        ComposeEvent::EditSubject("  Your quote ".to_owned()),
    );
    assert!(compose(&model).can_send());
    let (_, message) = sent(&event(&mut model, ComposeEvent::Send));
    assert_eq!(
        message,
        SendMessageRequest {
            to: "sparse@example.com".to_owned(),
            body: "Hello there".to_owned(),
            channel: CHANNEL_EMAIL.to_owned(),
            subject: Some("Your quote".to_owned()),
            media_urls: Vec::new(),
        }
    );
}

#[test]
fn a_text_goes_once_and_a_refusal_keeps_what_was_typed() {
    let mut model = composing(Ok(contacts()));
    event(&mut model, ComposeEvent::ChooseContact(ADA_ID.to_owned()));
    event(&mut model, ComposeEvent::EditSubject("ignored".to_owned()));
    event(&mut model, ComposeEvent::EditBody("On my way".to_owned()));
    assert_eq!(compose(&model).send_label(), "Send");
    let (ticket, message) = sent(&event(&mut model, ComposeEvent::Send));
    assert_eq!(message.subject, None);
    assert_eq!(message.channel, CHANNEL_SMS);
    assert!(compose(&model).sending);
    assert_eq!(compose(&model).send_label(), "Sending\u{2026}");
    // Nothing changes while it is on its way, and it is not sent twice.
    assert!(event(&mut model, ComposeEvent::Send).is_empty());
    event(&mut model, ComposeEvent::EditBody("changed".to_owned()));
    event(&mut model, ComposeEvent::Cancel);
    assert_eq!(compose(&model).body, "On my way");
    model.update(Event::MessageSent {
        ticket,
        result: Err(refusal("The sender is not verified.")),
    });
    let form = compose(&model);
    assert!(!form.sending);
    assert_eq!(
        form.failure.as_ref().map(|f| f.message.as_str()),
        Some("The sender is not verified.")
    );
    assert_eq!(form.body, "On my way");
    // Choosing again clears the failure.
    event(&mut model, ComposeEvent::ChooseContact(ADA_ID.to_owned()));
    assert_eq!(compose(&model).failure, None);
    event(&mut model, ComposeEvent::UseTyped);
    event(&mut model, ComposeEvent::Query("x".to_owned()));
    event(&mut model, ComposeEvent::UseTyped);
    assert_eq!(compose(&model).failure, None);
}

/// Sends a text to Ada from the inbox and returns the model and what the send
/// asked for once it went out with `answer`.
fn sent_to_ada(answer: SendMessageResponse) -> (Model, Vec<Effect>) {
    let mut model = composing(Ok(contacts()));
    event(&mut model, ComposeEvent::ChooseContact(ADA_ID.to_owned()));
    event(&mut model, ComposeEvent::EditBody("On my way".to_owned()));
    let (ticket, _) = sent(&event(&mut model, ComposeEvent::Send));
    let effects = model.update(Event::MessageSent {
        ticket,
        result: Ok(answer),
    });
    (model, effects)
}

fn thread_of(key: &str, counterpart: &str, channel: &str) -> MessageThreadResponse {
    let mut found: MessageThreadResponse = fixture("district-message-thread.json");
    found.thread.thread_key = key.to_owned();
    found.thread.counterpart = counterpart.to_owned();
    found.thread.channel = channel.to_owned();
    found
}

#[test]
fn a_sent_message_closes_the_form_and_opens_its_thread() {
    let (mut model, effects) = sent_to_ada(fixture("district-message-send.json"));
    assert_eq!(inbox(&model).compose, None);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::LoadConversations { .. }))
    );
    let lookup = pick(
        &effects,
        |e| matches!(e, Effect::FindMessageThread { message_id, .. } if message_id == "msg_contract_sent"),
    );
    model.update(Event::MessageThreadFound {
        ticket: lookup,
        result: Ok(fixture("district-message-thread.json")),
    });
    assert_eq!(
        signed_in(&model).route,
        Route::Thread {
            thread_key: "contact:contact_contract_1".to_owned()
        }
    );
}

#[test]
fn a_new_threads_reply_goes_where_the_service_says_it_is_reached() {
    for (channel, expected) in [
        (CHANNEL_SMS, Some(target("+12125550142", CHANNEL_SMS))),
        (CHANNEL_EMAIL, Some(target("+12125550142", CHANNEL_EMAIL))),
        ("whatsapp", None),
    ] {
        let (mut model, effects) = sent_to_ada(fixture("district-message-send.json"));
        model.update(Event::MessageThreadFound {
            ticket: pick(&effects, |e| matches!(e, Effect::FindMessageThread { .. })),
            result: Ok(thread_of("addr:+12125550142", "+12125550142", channel)),
        });
        let thread = signed_in(&model).thread.as_ref().expect("a thread open");
        assert_eq!(thread.reply_target, expected, "{channel}");
    }
    let (mut model, effects) = sent_to_ada(fixture("district-message-send.json"));
    model.update(Event::MessageThreadFound {
        ticket: pick(&effects, |e| matches!(e, Effect::FindMessageThread { .. })),
        result: Ok(thread_of("addr:+12125550142", " ", CHANNEL_SMS)),
    });
    assert_eq!(
        signed_in(&model).thread.as_ref().unwrap().reply_target,
        None
    );
}

#[test]
fn a_thread_that_cannot_be_found_or_is_no_longer_wanted_stays_on_the_list() {
    let (mut model, effects) = sent_to_ada(fixture("district-message-send.json"));
    model.update(Event::MessageThreadFound {
        ticket: pick(&effects, |e| matches!(e, Effect::FindMessageThread { .. })),
        result: Err(server_error()),
    });
    assert_eq!(signed_in(&model).route, Route::Inbox);

    let (mut model, effects) = sent_to_ada(fixture("district-message-send.json"));
    model.update(Event::Navigate(Route::Calls));
    model.update(Event::MessageThreadFound {
        ticket: pick(&effects, |e| matches!(e, Effect::FindMessageThread { .. })),
        result: Ok(fixture("district-message-thread.json")),
    });
    assert_eq!(signed_in(&model).route, Route::Calls);
}

#[test]
fn a_send_with_no_message_id_still_closes_and_reads_the_list() {
    let (model, effects) = sent_to_ada(SendMessageResponse {
        success: true,
        message: None,
        error: None,
    });
    assert_eq!(inbox(&model).compose, None);
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::FindMessageThread { .. }))
    );
}

#[test]
fn cancelling_drops_the_form_and_a_late_answer() {
    let mut model = on_inbox(AGENCY, "agency", 0);
    let effects = event(&mut model, ComposeEvent::Open);
    event(&mut model, ComposeEvent::Cancel);
    assert_eq!(inbox(&model).compose, None);
    model.update(Event::ContactsLoaded {
        ticket: last_ticket(&effects),
        result: Ok(contacts()),
    });
    assert_eq!(inbox(&model).compose, None);
    // Nothing to do without a form.
    assert!(event(&mut model, ComposeEvent::Send).is_empty());
}

#[test]
fn another_workspace_closes_the_form() {
    let mut model = on_inbox(AGENCY, "agency", 0);
    let effects = event(&mut model, ComposeEvent::Open);
    model.update(Event::SelectWorkspace(CLIENT.to_owned()));
    assert_eq!(inbox(&model).compose, None);
    model.update(Event::ContactsLoaded {
        ticket: last_ticket(&effects),
        result: Ok(contacts()),
    });
    assert_eq!(inbox(&model).compose, None);
}

#[test]
fn the_form_has_its_words() {
    assert_eq!(ComposeScreen::title(), "New message");
    assert_eq!(ComposeScreen::action(), "New message");
    assert_eq!(ComposeScreen::to_label(), "To");
    assert_eq!(ComposeScreen::change_label(), "Change");
    assert!(ComposeScreen::recent_contacts_note().starts_with("Showing your most recent"));
    assert_eq!(ComposeScreen::send_on_label(), "Send on");
    assert_eq!(ComposeScreen::subject_label(), "Subject");
    assert_eq!(ComposeScreen::message_label(), "Message");
    assert_eq!(ComposeScreen::cancel_label(), "Cancel");
    assert_eq!(ComposeScreen::channel_label(CHANNEL_SMS), "Text message");
    assert_eq!(ComposeScreen::channel_label(CHANNEL_EMAIL), "Email");
}
