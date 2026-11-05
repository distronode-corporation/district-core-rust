//! App parity: the reply channels the service lists for a thread, Messenger and
//! Instagram, who owns a thread, and the WhatsApp templates a closed window
//! offers.

use district_api::{ApiError, ConversationsQuery, ErrorDetail};
use district_core::{
    AssigneeList, Effect, Event, FailureText, InboxScreen, Model, PickedAttachment, Route,
    TemplateList, ThreadAssignment, ThreadControls, ThreadEvent, ThreadScreen, WhatsAppComposer,
    WhatsAppWindowState, reply_channel_label,
};
use district_model::{
    CHANNEL_EMAIL, CHANNEL_INSTAGRAM, CHANNEL_MESSENGER, CHANNEL_SMS, CHANNEL_WHATSAPP,
    ConversationAssignResponse, ConversationsResponse, MemberListResponse, ReplyOption,
    ReplyTarget, SendMessageRequest, TelemetryEnvelope, TelemetryEventType, TimelineResponse,
    WhatsAppTemplatesResponse, WhatsAppWindowResponse,
};
use serde_json::json;

use crate::inbox::{ADA, WALK_IN, unread};
use crate::support::{
    AGENCY, VIEWER, fixture, has, last_ticket, live_event, loaded, pick, server_error, signed_in,
    with_meta_channels,
};

/// The WhatsApp-only thread of the reply options fixture.
const WHATSAPP_ONLY: &str = "addr:14165550177";
/// Its Messenger thread.
const MESSENGER: &str = "addr:fb:5550101";

fn reply_options() -> ConversationsResponse {
    fixture("district-conversations-reply-options.json")
}

fn owners() -> ConversationsResponse {
    fixture("district-conversations-assignee.json")
}

fn members() -> MemberListResponse {
    fixture("district-members.json")
}

/// On the inbox of `workspace_id` as `role`, its list read as `list`.
fn inbox_with(workspace_id: &str, role: &str, list: ConversationsResponse) -> (Model, Vec<Effect>) {
    let (mut model, _) = loaded(workspace_id, role);
    model.update(Event::Navigate(Route::Inbox));
    let effects = model.update(Event::Refresh);
    model.update(Event::ConversationsLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadConversations { .. })),
        result: Ok(list),
    });
    model.update(Event::UnreadCountLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadUnreadCount { .. })),
        result: Ok(unread(workspace_id, 0)),
    });
    (model, effects)
}

fn open(model: &mut Model, thread_key: &str) -> Vec<Effect> {
    model.update(Event::Navigate(Route::Thread {
        thread_key: thread_key.to_owned(),
    }))
}

/// Opens `thread_key` and reads its newest page.
fn opened(model: &mut Model, thread_key: &str) -> Vec<Effect> {
    let effects = open(model, thread_key);
    model.update(Event::TimelineLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadTimeline { .. })),
        result: Ok(fixture::<TimelineResponse>("district-timeline.json")),
    });
    effects
}

fn screen(model: &Model) -> &ThreadScreen {
    signed_in(model).thread.as_ref().expect("a thread is open")
}

fn controls(model: &Model) -> ThreadControls {
    screen(model).controls(&signed_in(model).capabilities())
}

fn whatsapp(model: &Model) -> &WhatsAppComposer {
    screen(model)
        .whatsapp
        .as_ref()
        .expect("the reply goes by WhatsApp")
}

fn thread_event(model: &mut Model, event: ThreadEvent) -> Vec<Effect> {
    model.update(Event::Thread(event))
}

fn target(to: &str, channel: &str) -> ReplyTarget {
    ReplyTarget::from_option(&ReplyOption {
        channel: channel.to_owned(),
        to: to.to_owned(),
    })
    .expect("a known channel")
}

fn coded(status: u16, code: &str, message: &str) -> ApiError {
    ApiError::Envelope {
        status,
        code: code.to_owned(),
        detail: ErrorDetail {
            message: Some(message.to_owned()),
            code: Some(code.to_owned()),
            degraded_regions: Vec::new(),
        },
    }
}

fn the_send(effects: &[Effect]) -> SendMessageRequest {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::SendMessage { message, .. } => Some(message.clone()),
            _ => None,
        })
        .expect("a send")
}

// Reply options.

#[test]
fn the_first_reply_option_is_the_default_and_the_rest_are_offered() {
    with_meta_channels(|| {
        let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
        opened(&mut model, ADA);
        let screen = screen(&model);
        assert_eq!(
            screen.reply_choices,
            [
                target("ada@example.com", CHANNEL_EMAIL),
                target("14165550142", CHANNEL_SMS),
                target("+14165550142", CHANNEL_WHATSAPP),
            ]
        );
        assert_eq!(
            screen.reply_target,
            Some(target("ada@example.com", CHANNEL_EMAIL))
        );
        assert!(controls(&model).can_choose_channel);
        assert!(screen.whatsapp.is_none(), "no window to read on email");

        // A channel the service did not list is not taken.
        assert!(
            thread_event(
                &mut model,
                ThreadEvent::ChooseChannel("messenger".to_owned())
            )
            .is_empty()
        );
        // Choosing SMS sends by SMS, to the address the service gave for it.
        assert!(thread_event(&mut model, ThreadEvent::ChooseChannel("sms".to_owned())).is_empty());
        thread_event(&mut model, ThreadEvent::Compose("On our way".to_owned()));
        let message = the_send(&thread_event(&mut model, ThreadEvent::Send));
        assert_eq!(
            (message.channel.as_str(), message.to.as_str()),
            ("sms", "14165550142")
        );
        assert_eq!(message.whatsapp_sender_id, None);
    });
}

#[test]
fn a_meta_thread_is_answered_on_its_own_channel_and_never_by_sms() {
    with_meta_channels(|| {
        let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
        let row = signed_in(&model)
            .inbox
            .conversation(MESSENGER)
            .expect("listed");
        assert_eq!(InboxScreen::meta_channel_label(row), Some("Messenger"));
        assert!(!row.can_sms && !row.can_email);
        opened(&mut model, MESSENGER);
        assert_eq!(
            screen(&model).reply_choices,
            [target("fb:5550101", CHANNEL_MESSENGER)]
        );
        let controls = controls(&model);
        assert!(controls.can_reply && !controls.can_choose_channel && !controls.can_attach);
        thread_event(&mut model, ThreadEvent::Compose("Thanks!".to_owned()));
        let message = the_send(&thread_event(&mut model, ThreadEvent::Send));
        assert_eq!(
            (message.channel.as_str(), message.to.as_str()),
            ("messenger", "fb:5550101")
        );
    });
}

#[test]
fn can_sms_plays_no_part_in_the_choice() {
    with_meta_channels(|| {
        let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
        // The service says the customer can be texted, and offers WhatsApp only.
        assert!(
            signed_in(&model)
                .inbox
                .conversation(WHATSAPP_ONLY)
                .unwrap()
                .can_sms
        );
        opened(&mut model, WHATSAPP_ONLY);
        assert_eq!(
            screen(&model).reply_choices,
            [target("+14165550177", CHANNEL_WHATSAPP)]
        );
        assert!(thread_event(&mut model, ThreadEvent::ChooseChannel("sms".to_owned())).is_empty());
        assert_eq!(
            screen(&model).reply_target.as_ref().unwrap().channel,
            "whatsapp"
        );
    });
}

#[test]
fn no_reply_options_is_no_reply_box_and_an_unknown_channel_is_skipped() {
    with_meta_channels(|| {
        let mut list = reply_options();
        list.conversations[0].reply_options = Vec::new();
        list.conversations[1].reply_options = vec![
            ReplyOption {
                channel: "carrier_pigeon".to_owned(),
                to: "loft 3".to_owned(),
            },
            ReplyOption {
                channel: CHANNEL_INSTAGRAM.to_owned(),
                to: " ".to_owned(),
            },
            ReplyOption {
                channel: CHANNEL_MESSENGER.to_owned(),
                to: "fb:5550101".to_owned(),
            },
        ];
        let (mut model, _) = inbox_with(AGENCY, "agency", list);

        opened(&mut model, WHATSAPP_ONLY);
        assert!(screen(&model).reply_choices.is_empty());
        let capabilities = signed_in(&model).capabilities();
        assert!(!controls(&model).can_reply);
        assert_eq!(
            screen(&model).read_only_note(&capabilities),
            Some(ThreadScreen::NO_REPLY_TARGET)
        );
        model.update(Event::Navigate(Route::Inbox));

        opened(&mut model, MESSENGER);
        assert_eq!(
            screen(&model).reply_choices,
            [target("fb:5550101", CHANNEL_MESSENGER)]
        );
        assert!(
            thread_event(
                &mut model,
                ThreadEvent::ChooseChannel("carrier_pigeon".to_owned())
            )
            .is_empty()
        );
    });
}

#[test]
fn without_meta_channels_a_reply_goes_by_text_or_email_as_before() {
    let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
    let legacy = signed_in(&model)
        .inbox
        .conversation(ADA)
        .and_then(|row| row.reply_target())
        .expect("a text or email address");
    assert_eq!(legacy.channel, CHANNEL_SMS);
    opened(&mut model, ADA);
    let screen = screen(&model);
    assert_eq!(screen.reply_choices, std::slice::from_ref(&legacy));
    assert_eq!(screen.reply_target, Some(legacy));
    assert!(!controls(&model).can_choose_channel);
}

#[test]
fn a_channel_cannot_change_while_a_reply_is_on_its_way() {
    with_meta_channels(|| {
        let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
        opened(&mut model, ADA);
        thread_event(&mut model, ThreadEvent::Compose("Hi".to_owned()));
        thread_event(&mut model, ThreadEvent::Send);
        assert!(!controls(&model).can_choose_channel);
        assert!(thread_event(&mut model, ThreadEvent::ChooseChannel("sms".to_owned())).is_empty());
        assert_eq!(
            screen(&model).reply_target.as_ref().unwrap().channel,
            "email"
        );
    });
}

#[test]
fn attachments_go_by_sms_only() {
    with_meta_channels(|| {
        let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
        opened(&mut model, ADA);
        thread_event(&mut model, ThreadEvent::ChooseChannel("sms".to_owned()));
        assert!(controls(&model).can_attach);
        let effects = thread_event(
            &mut model,
            ThreadEvent::Attach(PickedAttachment {
                file_name: "roof.png".to_owned(),
                mime_type: "image/png".to_owned(),
                bytes: vec![7; 3],
            }),
        );
        model.update(Event::MediaUploaded {
            ticket: last_ticket(&effects),
            result: Ok(fixture("district-media-upload.json")),
        });
        thread_event(&mut model, ThreadEvent::Compose("The roof".to_owned()));
        assert!(controls(&model).can_send && !screen(&model).attachments_blocked());

        thread_event(&mut model, ThreadEvent::ChooseChannel("email".to_owned()));
        assert!(screen(&model).attachments_blocked());
        assert!(!controls(&model).can_attach && !controls(&model).can_send);
        assert!(thread_event(&mut model, ThreadEvent::Send).is_empty());
        let url = screen(&model).composer.attachments[0].clone();
        thread_event(&mut model, ThreadEvent::RemoveAttachment(url));
        assert!(controls(&model).can_send && !screen(&model).attachments_blocked());
    });
}

#[test]
fn every_channel_has_its_label() {
    assert_eq!(reply_channel_label(CHANNEL_SMS), Some("Text message"));
    assert_eq!(reply_channel_label(CHANNEL_EMAIL), Some("Email"));
    assert_eq!(reply_channel_label(CHANNEL_WHATSAPP), Some("WhatsApp"));
    assert_eq!(reply_channel_label(CHANNEL_MESSENGER), Some("Messenger"));
    assert_eq!(reply_channel_label(CHANNEL_INSTAGRAM), Some("Instagram"));
    assert_eq!(reply_channel_label("carrier_pigeon"), None);
    assert_eq!(ThreadScreen::CHANNEL_LABEL, "Reply channel");
}

// The Meta flag on every read.

#[test]
fn an_app_that_answers_meta_asks_for_it_on_every_read() {
    for meta in [false, true] {
        let run = || {
            let (mut model, effects) = inbox_with(AGENCY, "agency", reply_options());
            assert!(effects.iter().any(|e| matches!(
                e,
                Effect::LoadConversations { query, .. }
                    if *query == ConversationsQuery { meta_channels: meta, assignments: true }
            )));
            assert!(effects.iter().any(|e| matches!(
                e,
                Effect::LoadUnreadCount { meta_channels, .. } if *meta_channels == meta
            )));
            let effects = open(&mut model, WALK_IN);
            assert!(effects.iter().any(|e| matches!(
                e,
                Effect::LoadTimeline { meta_channels, .. } if *meta_channels == meta
            )));
            model.update(Event::TimelineLoaded {
                ticket: pick(&effects, |e| matches!(e, Effect::LoadTimeline { .. })),
                result: Ok(fixture::<TimelineResponse>("district-timeline-page.json")),
            });
            let older = thread_event(&mut model, ThreadEvent::LoadOlder);
            assert!(older.iter().any(|e| matches!(
                e,
                Effect::LoadTimeline { older_than: Some(_), meta_channels, .. }
                    if *meta_channels == meta
            )));
        };
        if meta { with_meta_channels(run) } else { run() }
    }
}

// The owner.

#[test]
fn each_row_says_who_owns_it() {
    let (model, _) = inbox_with(AGENCY, "agency", owners());
    let inbox = &signed_in(&model).inbox;
    assert_eq!(
        InboxScreen::assignee_line(inbox.conversation(ADA).unwrap()).as_deref(),
        Some("Assigned to contract@example.com")
    );
    assert_eq!(
        InboxScreen::assignee_line(inbox.conversation(WALK_IN).unwrap()),
        None
    );
    assert_eq!(
        InboxScreen::meta_channel_label(inbox.conversation(ADA).unwrap()),
        None
    );
}

#[test]
fn the_picker_offers_agency_and_client_members_only() {
    let (mut model, _) = inbox_with(AGENCY, "agency", owners());
    let effects = opened(&mut model, WALK_IN);
    assert_eq!(screen(&model).assignment.members, AssigneeList::Loading);
    assert!(
        !controls(&model).can_assign,
        "not before the members are read"
    );
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });
    let assignment = &screen(&model).assignment;
    assert_eq!(assignment.assignee, None);
    assert_eq!(
        assignment.options(),
        ["founder@example.com", "operator@example.com"]
    );
    assert!(controls(&model).can_assign);
    assert_eq!(ThreadAssignment::UNASSIGNED, "Unassigned");
    assert_eq!(ThreadAssignment::LABEL, "Assigned to");
}

#[test]
fn a_thread_already_given_to_a_viewer_keeps_showing_them() {
    let mut list = owners();
    list.conversations[0].assignee_email = Some(Some("auditor@example.com".to_owned()));
    let (mut model, _) = inbox_with(AGENCY, "agency", list);
    let effects = opened(&mut model, ADA);
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });
    let assignment = &screen(&model).assignment;
    assert_eq!(assignment.assignee.as_deref(), Some("auditor@example.com"));
    assert_eq!(
        assignment.options(),
        [
            "founder@example.com",
            "operator@example.com",
            "auditor@example.com"
        ]
    );
    // Choosing them again changes nothing.
    assert!(
        thread_event(
            &mut model,
            ThreadEvent::Assign(Some("auditor@example.com".to_owned()))
        )
        .is_empty()
    );
}

#[test]
fn assigning_and_unassigning_go_once_and_update_the_row() {
    let (mut model, _) = inbox_with(AGENCY, "agency", owners());
    let effects = opened(&mut model, WALK_IN);
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });

    // Someone not offered (a viewer, a stranger) is never sent.
    for refused in ["auditor@example.com", "stranger@example.com"] {
        assert!(thread_event(&mut model, ThreadEvent::Assign(Some(refused.to_owned()))).is_empty());
    }
    let effects = thread_event(
        &mut model,
        ThreadEvent::Assign(Some("operator@example.com".to_owned())),
    );
    let [
        Effect::AssignConversation {
            ticket,
            workspace_id,
            thread_key,
            assignee_email,
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(
        (
            workspace_id.as_str(),
            thread_key.as_str(),
            assignee_email.as_deref()
        ),
        (AGENCY, WALK_IN, Some("operator@example.com"))
    );
    assert!(screen(&model).assignment.saving && !controls(&model).can_assign);
    // One at a time.
    assert!(thread_event(&mut model, ThreadEvent::Assign(None)).is_empty());

    let mut answer: ConversationAssignResponse = fixture("district-conversation-assign.json");
    answer.assignment.as_mut().unwrap().assignee_email = "operator@example.com".to_owned();
    let effects = model.update(Event::ConversationAssigned {
        ticket: *ticket,
        result: Ok(answer),
    });
    assert!(has(&effects, |e| matches!(
        e,
        Effect::LoadConversations { .. }
    )));
    assert_eq!(
        screen(&model).assignment.assignee.as_deref(),
        Some("operator@example.com")
    );
    let row = signed_in(&model).inbox.conversation(WALK_IN).unwrap();
    assert_eq!(row.assignee(), Some("operator@example.com"));

    // Unassigning sends null and clears the row.
    let effects = thread_event(&mut model, ThreadEvent::Assign(None));
    let [
        Effect::AssignConversation {
            ticket,
            assignee_email: None,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    model.update(Event::ConversationAssigned {
        ticket: *ticket,
        result: Ok(fixture("district-conversation-unassign.json")),
    });
    assert_eq!(screen(&model).assignment.assignee, None);
    assert_eq!(
        signed_in(&model)
            .inbox
            .conversation(WALK_IN)
            .unwrap()
            .assignee(),
        None
    );
}

#[test]
fn a_refused_assignment_keeps_the_owner_and_says_why() {
    let (mut model, _) = inbox_with(AGENCY, "agency", owners());
    let effects = opened(&mut model, ADA);
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });
    let effects = thread_event(
        &mut model,
        ThreadEvent::Assign(Some("founder@example.com".to_owned())),
    );
    let refusal = coded(
        400,
        "assignee_not_member",
        "That person is not a member of this workspace.",
    );
    assert!(
        model
            .update(Event::ConversationAssigned {
                ticket: last_ticket(&effects),
                result: Err(refusal.clone()),
            })
            .is_empty()
    );
    let assignment = &screen(&model).assignment;
    assert_eq!(assignment.assignee.as_deref(), Some("contract@example.com"));
    assert!(!assignment.saving);
    assert_eq!(
        assignment.failure,
        Some(FailureText::from_api_error(&refusal))
    );
    thread_event(&mut model, ThreadEvent::DismissAssignFailure);
    assert_eq!(screen(&model).assignment.failure, None);
}

#[test]
fn members_that_cannot_be_read_leave_the_owner_showing() {
    let (mut model, _) = inbox_with(AGENCY, "agency", owners());
    let effects = opened(&mut model, ADA);
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Err(server_error()),
    });
    let assignment = &screen(&model).assignment;
    assert!(matches!(assignment.members, AssigneeList::Failed(_)));
    assert_eq!(assignment.options(), ["contract@example.com"]);
    assert!(!controls(&model).can_assign);
}

#[test]
fn a_viewer_sees_the_owner_and_cannot_assign() {
    let (mut model, _) = inbox_with(VIEWER, "viewer", owners());
    let effects = opened(&mut model, ADA);
    assert!(!has(&effects, |e| matches!(e, Effect::LoadMembers { .. })));
    assert_eq!(
        screen(&model).assignment.assignee.as_deref(),
        Some("contract@example.com")
    );
    assert_eq!(screen(&model).assignment.members, AssigneeList::NotLoaded);
    assert!(!controls(&model).can_assign);
    assert!(thread_event(&mut model, ThreadEvent::Assign(None)).is_empty());
}

#[test]
fn an_assignment_landing_after_the_thread_closed_reads_the_list_again() {
    let (mut model, _) = inbox_with(AGENCY, "agency", owners());
    let effects = opened(&mut model, WALK_IN);
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });
    let effects = thread_event(
        &mut model,
        ThreadEvent::Assign(Some("founder@example.com".to_owned())),
    );
    let back = model.update(Event::Navigate(Route::Inbox));
    let list = pick(&back, |e| matches!(e, Effect::LoadConversations { .. }));
    // The list is already being read, so the read again waits for it.
    assert!(
        model
            .update(Event::ConversationAssigned {
                ticket: last_ticket(&effects),
                result: Ok(fixture("district-conversation-assign.json")),
            })
            .is_empty()
    );
    let effects = model.update(Event::ConversationsLoaded {
        ticket: list,
        result: Ok(owners()),
    });
    assert!(has(&effects, |e| matches!(
        e,
        Effect::LoadConversations { .. }
    )));
}

#[test]
fn an_assignment_lands_on_the_thread_it_was_asked_for_not_the_one_open() {
    let (mut model, _) = inbox_with(AGENCY, "agency", owners());
    let effects = opened(&mut model, WALK_IN);
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });
    let given = thread_event(
        &mut model,
        ThreadEvent::Assign(Some("founder@example.com".to_owned())),
    );

    // The member goes on to another thread before the answer lands.
    model.update(Event::Navigate(Route::Inbox));
    let effects = opened(&mut model, ADA);
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });
    let before = screen(&model).assignment.clone();
    assert_eq!(before.assignee.as_deref(), Some("contract@example.com"));

    model.update(Event::ConversationAssigned {
        ticket: last_ticket(&given),
        result: Ok(fixture("district-conversation-assign.json")),
    });
    assert_eq!(
        screen(&model).assignment,
        before,
        "the open thread is not touched"
    );
    let inbox = &signed_in(&model).inbox;
    assert_eq!(
        inbox.conversation(ADA).unwrap().assignee(),
        Some("contract@example.com")
    );
    assert_eq!(
        inbox.conversation(WALK_IN).unwrap().assignee(),
        Some("teammate@example.com"),
        "the thread it was asked for takes the answer"
    );

    // A refusal for the thread left behind is not said on this one either.
    let effects = thread_event(
        &mut model,
        ThreadEvent::Assign(Some("operator@example.com".to_owned())),
    );
    model.update(Event::Navigate(Route::Inbox));
    let reopened = opened(&mut model, WALK_IN);
    model.update(Event::MembersLoaded {
        ticket: pick(&reopened, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });
    let before = screen(&model).assignment.clone();
    model.update(Event::ConversationAssigned {
        ticket: last_ticket(&effects),
        result: Err(coded(
            400,
            "assignee_not_member",
            "That person is not a member of this workspace.",
        )),
    });
    assert_eq!(screen(&model).assignment, before);
}

/// The list read again, by the live update for a message, as `list`.
fn list_read_again(model: &mut Model, list: ConversationsResponse) {
    let effects = model.update(live_event(TelemetryEnvelope {
        workspace_id: AGENCY.to_owned(),
        call_id: "m1".to_owned(),
        event_type: TelemetryEventType::MessageReceived,
        data: json!({}),
        timestamp: "2026-11-05T18:40:00.000Z".to_owned(),
    }));
    model.update(Event::ConversationsLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadConversations { .. })),
        result: Ok(list),
    });
}

/// `owners()` with the walk-in thread given to `assignee`.
fn walk_in_given_to(assignee: Option<&str>) -> ConversationsResponse {
    let mut list = owners();
    for thread in &mut list.conversations {
        if thread.thread_key == WALK_IN {
            thread.assignee_email = Some(assignee.map(str::to_owned));
        }
    }
    list
}

#[test]
fn a_thread_given_away_while_open_can_be_taken_back() {
    let (mut model, _) = inbox_with(AGENCY, "agency", owners());
    let effects = opened(&mut model, WALK_IN);
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });
    assert_eq!(screen(&model).assignment.assignee, None);

    // Another member gives the open thread to the founder, and the list says so.
    list_read_again(&mut model, walk_in_given_to(Some("founder@example.com")));
    assert_eq!(
        screen(&model).assignment.assignee.as_deref(),
        Some("founder@example.com")
    );
    // Taking it back to nobody, its owner before, is sent.
    let effects = thread_event(&mut model, ThreadEvent::Assign(None));
    let [
        Effect::AssignConversation {
            thread_key,
            assignee_email: None,
            ..
        },
    ] = effects.as_slice()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(thread_key, WALK_IN);

    // While that is on its way the list is not who owns it: the answer is.
    list_read_again(&mut model, walk_in_given_to(Some("operator@example.com")));
    assert_eq!(
        screen(&model).assignment.assignee.as_deref(),
        Some("founder@example.com")
    );
}

// WhatsApp.

/// On WhatsApp in the WhatsApp-only thread, with the window read as `window`.
fn on_whatsapp(window: WhatsAppWindowResponse) -> (Model, Vec<Effect>) {
    let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
    let effects = opened(&mut model, WHATSAPP_ONLY);
    let [.., Effect::LoadWhatsAppWindow { ticket, to, .. }] = effects.as_slice() else {
        panic!("{effects:?}");
    };
    assert_eq!(to, "+14165550177");
    assert_eq!(whatsapp(&model).window, WhatsAppWindowState::Loading);
    let effects = model.update(Event::WhatsAppWindowLoaded {
        ticket: *ticket,
        result: Ok(window),
    });
    (model, effects)
}

fn closed() -> WhatsAppWindowResponse {
    fixture("district-whatsapp-window-closed.json")
}

/// On WhatsApp with the window closed and the recorded templates read.
fn templates_offered() -> Model {
    let (mut model, effects) = on_whatsapp(closed());
    model.update(Event::WhatsAppTemplatesLoaded {
        ticket: last_ticket(&effects),
        result: Ok(fixture("district-whatsapp-templates.json")),
    });
    model
}

#[test]
fn an_open_window_takes_free_text_from_the_sender_it_was_read_for() {
    with_meta_channels(|| {
        let (mut model, effects) = on_whatsapp(fixture("district-whatsapp-window.json"));
        assert!(effects.is_empty(), "no templates while it is open");
        assert!(matches!(
            whatsapp(&model).window,
            WhatsAppWindowState::Open {
                expires_at: Some(_)
            }
        ));
        assert_eq!(whatsapp(&model).templates, TemplateList::NotLoaded);
        thread_event(&mut model, ThreadEvent::Compose("See you soon".to_owned()));
        assert!(controls(&model).can_send && !controls(&model).can_send_template);
        let message = the_send(&thread_event(&mut model, ThreadEvent::Send));
        assert_eq!(
            message,
            SendMessageRequest {
                to: "+14165550177".to_owned(),
                body: "See you soon".to_owned(),
                channel: "whatsapp".to_owned(),
                whatsapp_sender_id: Some("wa_sender_contract".to_owned()),
                ..SendMessageRequest::default()
            }
        );
    });
}

#[test]
fn a_closed_window_offers_approved_templates_only() {
    with_meta_channels(|| {
        let (mut model, effects) = on_whatsapp(closed());
        let [Effect::LoadWhatsAppTemplates { sender_id, .. }] = effects.as_slice() else {
            panic!("{effects:?}");
        };
        assert_eq!(sender_id, "wa_sender_contract");
        assert_eq!(whatsapp(&model).templates, TemplateList::Loading);
        assert!(screen(&model).whatsapp_window_closed());
        thread_event(&mut model, ThreadEvent::Compose("Free text".to_owned()));
        let controls_now = controls(&model);
        assert!(!controls_now.can_send && !controls_now.can_draft_reply);
        assert!(thread_event(&mut model, ThreadEvent::Send).is_empty());

        model.update(Event::WhatsAppTemplatesLoaded {
            ticket: last_ticket(&effects),
            result: Ok(fixture("district-whatsapp-templates.json")),
        });
        // The pending one with named variables is not offered.
        let TemplateList::Ready(templates) = &whatsapp(&model).templates else {
            panic!("{:?}", whatsapp(&model).templates);
        };
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].id, "wa_tpl_reminder");
        assert_eq!(templates[0].variable_count, 2);
        assert_eq!(templates[0].label(), "appointment_reminder (en_US)");
    });
}

#[test]
fn a_template_of_another_sender_is_not_offered() {
    with_meta_channels(|| {
        let (mut model, effects) = on_whatsapp(closed());
        let mut answer: WhatsAppTemplatesResponse = fixture("district-whatsapp-templates.json");
        answer.templates[0].sender_id = Some("wa_sender_other".to_owned());
        model.update(Event::WhatsAppTemplatesLoaded {
            ticket: last_ticket(&effects),
            result: Ok(answer),
        });
        assert_eq!(whatsapp(&model).templates, TemplateList::Ready(Vec::new()));
        assert_eq!(
            WhatsAppComposer::TEMPLATES_NONE,
            "No approved template for this number yet. Refresh the templates in Messaging settings."
        );
    });
}

#[test]
fn the_filled_template_is_previewed_and_sent_as_the_service_fills_it() {
    with_meta_channels(|| {
        let mut model = templates_offered();
        assert!(!controls(&model).can_send_template);
        thread_event(
            &mut model,
            ThreadEvent::ChooseTemplate("wa_tpl_reminder".to_owned()),
        );
        assert_eq!(
            whatsapp(&model).preview().as_deref(),
            Some("Hi {{1}}, this is a reminder of your appointment on {{2}}.")
        );
        thread_event(
            &mut model,
            ThreadEvent::TemplateVariable(0, "  Ada ".to_owned()),
        );
        // A variable the template does not have is not kept.
        thread_event(
            &mut model,
            ThreadEvent::TemplateVariable(2, "extra".to_owned()),
        );
        assert_eq!(
            whatsapp(&model).preview().as_deref(),
            Some("Hi Ada, this is a reminder of your appointment on {{2}}.")
        );
        assert!(
            !controls(&model).can_send_template,
            "every variable is required"
        );
        assert!(thread_event(&mut model, ThreadEvent::SendTemplate).is_empty());
        thread_event(
            &mut model,
            ThreadEvent::TemplateVariable(1, "Thursday".to_owned()),
        );
        assert!(controls(&model).can_send_template);
        assert_eq!(WhatsAppComposer::variable_label(0), "Variable {{1}}");

        let effects = thread_event(&mut model, ThreadEvent::SendTemplate);
        assert_eq!(
            the_send(&effects),
            SendMessageRequest {
                to: "+14165550177".to_owned(),
                body: "Hi Ada, this is a reminder of your appointment on Thursday.".to_owned(),
                channel: "whatsapp".to_owned(),
                whatsapp_sender_id: Some("wa_sender_contract".to_owned()),
                template_id: Some("wa_tpl_reminder".to_owned()),
                template_variables: Some(vec!["Ada".to_owned(), "Thursday".to_owned()]),
                ..SendMessageRequest::default()
            }
        );
        // Single flight.
        assert!(whatsapp(&model).sending && !controls(&model).can_send_template);
        assert!(thread_event(&mut model, ThreadEvent::SendTemplate).is_empty());
        assert!(
            thread_event(&mut model, ThreadEvent::ChooseTemplate("other".to_owned())).is_empty()
        );
        assert_eq!(whatsapp(&model).chosen.as_deref(), Some("wa_tpl_reminder"));

        let effects = model.update(Event::MessageSent {
            ticket: last_ticket(&effects),
            result: Ok(fixture("district-message-send-whatsapp-template.json")),
        });
        assert!(has(&effects, |e| matches!(e, Effect::LoadTimeline { .. })));
        let whatsapp = whatsapp(&model);
        assert!(!whatsapp.sending && whatsapp.chosen.is_none() && whatsapp.variables.is_empty());
    });
}

#[test]
fn a_refused_template_keeps_what_was_filled() {
    with_meta_channels(|| {
        let mut model = templates_offered();
        thread_event(
            &mut model,
            ThreadEvent::ChooseTemplate("wa_tpl_reminder".to_owned()),
        );
        thread_event(
            &mut model,
            ThreadEvent::TemplateVariable(0, "Ada".to_owned()),
        );
        thread_event(
            &mut model,
            ThreadEvent::TemplateVariable(1, "Thursday".to_owned()),
        );
        let effects = thread_event(&mut model, ThreadEvent::SendTemplate);
        let refusal = coded(
            400,
            "whatsapp_template_invalid",
            "That WhatsApp template cannot be sent.",
        );
        model.update(Event::MessageSent {
            ticket: last_ticket(&effects),
            result: Err(refusal.clone()),
        });
        assert_eq!(
            screen(&model).composer.failure,
            Some(FailureText::from_api_error(&refusal))
        );
        assert_eq!(whatsapp(&model).chosen.as_deref(), Some("wa_tpl_reminder"));
        assert!(controls(&model).can_send_template);
    });
}

#[test]
fn a_template_with_no_variables_sends_an_empty_list() {
    with_meta_channels(|| {
        let (mut model, effects) = on_whatsapp(closed());
        let mut answer: WhatsAppTemplatesResponse = fixture("district-whatsapp-templates.json");
        answer.templates[0].body = "Your order shipped.".to_owned();
        answer.templates[0].variable_count = Some(0);
        model.update(Event::WhatsAppTemplatesLoaded {
            ticket: last_ticket(&effects),
            result: Ok(answer),
        });
        thread_event(
            &mut model,
            ThreadEvent::ChooseTemplate("wa_tpl_reminder".to_owned()),
        );
        assert!(controls(&model).can_send_template);
        let message = the_send(&thread_event(&mut model, ThreadEvent::SendTemplate));
        assert_eq!(message.template_variables, Some(Vec::new()));
        assert_eq!(message.body, "Your order shipped.");
    });
}

#[test]
fn a_window_that_cannot_be_read_blocks_nothing() {
    with_meta_channels(|| {
        let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
        let effects = opened(&mut model, WHATSAPP_ONLY);
        model.update(Event::WhatsAppWindowLoaded {
            ticket: last_ticket(&effects),
            result: Err(server_error()),
        });
        assert!(matches!(
            whatsapp(&model).window,
            WhatsAppWindowState::Unknown(_)
        ));
        thread_event(&mut model, ThreadEvent::Compose("Hello".to_owned()));
        assert!(controls(&model).can_send);
        let message = the_send(&thread_event(&mut model, ThreadEvent::Send));
        assert_eq!(message.whatsapp_sender_id, None, "the service chooses");
    });
}

#[test]
fn a_closed_window_with_no_sender_has_no_templates_to_read() {
    with_meta_channels(|| {
        let mut window = closed();
        window.sender_id = None;
        let (model, effects) = on_whatsapp(window);
        assert!(effects.is_empty());
        assert_eq!(whatsapp(&model).templates, TemplateList::Ready(Vec::new()));
    });
}

#[test]
fn templates_that_cannot_be_read_say_so() {
    with_meta_channels(|| {
        let (mut model, effects) = on_whatsapp(closed());
        model.update(Event::WhatsAppTemplatesLoaded {
            ticket: last_ticket(&effects),
            result: Err(server_error()),
        });
        assert!(matches!(
            whatsapp(&model).templates,
            TemplateList::Failed(_)
        ));
    });
}

#[test]
fn text_refused_because_the_window_closed_turns_to_templates() {
    with_meta_channels(|| {
        let (mut model, _) = on_whatsapp(fixture("district-whatsapp-window.json"));
        thread_event(&mut model, ThreadEvent::Compose("Late reply".to_owned()));
        let effects = thread_event(&mut model, ThreadEvent::Send);
        let refusal = coded(
            409,
            "whatsapp_window_closed",
            "The 24-hour WhatsApp window with this contact is closed.",
        );
        let effects = model.update(Event::MessageSent {
            ticket: pick(&effects, |e| matches!(e, Effect::SendMessage { .. })),
            result: Err(refusal),
        });
        assert!(has(&effects, |e| matches!(
            e,
            Effect::LoadWhatsAppTemplates { .. }
        )));
        assert!(screen(&model).whatsapp_window_closed());
        assert_eq!(screen(&model).composer.text, "Late reply", "kept");
    });
}

/// Free text sent on WhatsApp before the window was read, refused because the
/// window has closed: the effects of the refusal, and the window read again.
fn refused_before_the_window_was_read() -> (Model, Vec<Effect>) {
    let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
    let opening = opened(&mut model, WHATSAPP_ONLY);
    let first_read = pick(&opening, |e| matches!(e, Effect::LoadWhatsAppWindow { .. }));
    assert_eq!(whatsapp(&model).sender_id, None);
    thread_event(&mut model, ThreadEvent::Compose("Late reply".to_owned()));
    let effects = thread_event(&mut model, ThreadEvent::Send);
    let effects = model.update(Event::MessageSent {
        ticket: pick(&effects, |e| matches!(e, Effect::SendMessage { .. })),
        result: Err(coded(
            409,
            "whatsapp_window_closed",
            "The 24-hour WhatsApp window with this contact is closed.",
        )),
    });
    assert!(screen(&model).whatsapp_window_closed());
    assert_eq!(
        whatsapp(&model).templates,
        TemplateList::Loading,
        "not an empty list: the sender is not known yet"
    );
    // The first read, still on its way, is no longer awaited.
    assert!(
        model
            .update(Event::WhatsAppWindowLoaded {
                ticket: first_read,
                result: Ok(fixture("district-whatsapp-window.json")),
            })
            .is_empty()
    );
    assert!(screen(&model).whatsapp_window_closed());
    (model, effects)
}

#[test]
fn text_refused_before_the_window_was_read_reads_it_again_for_the_sender() {
    with_meta_channels(|| {
        let (mut model, effects) = refused_before_the_window_was_read();
        let (ticket, to) = effects
            .iter()
            .find_map(|effect| match effect {
                Effect::LoadWhatsAppWindow { ticket, to, .. } => Some((*ticket, to.clone())),
                _ => None,
            })
            .expect("the window read again");
        assert_eq!(to, "+14165550177");
        let effects = model.update(Event::WhatsAppWindowLoaded {
            ticket,
            result: Ok(closed()),
        });
        let [
            Effect::LoadWhatsAppTemplates {
                ticket, sender_id, ..
            },
        ] = effects.as_slice()
        else {
            panic!("{effects:?}");
        };
        assert_eq!(sender_id, "wa_sender_contract");
        model.update(Event::WhatsAppTemplatesLoaded {
            ticket: *ticket,
            result: Ok(fixture("district-whatsapp-templates.json")),
        });
        let TemplateList::Ready(templates) = &whatsapp(&model).templates else {
            panic!("{:?}", whatsapp(&model).templates);
        };
        assert_eq!(templates.len(), 1);
        assert_eq!(screen(&model).composer.text, "Late reply", "kept");
    });
}

#[test]
fn a_window_read_again_that_fails_keeps_it_closed_and_says_the_templates_failed() {
    with_meta_channels(|| {
        let (mut model, effects) = refused_before_the_window_was_read();
        model.update(Event::WhatsAppWindowLoaded {
            ticket: pick(&effects, |e| matches!(e, Effect::LoadWhatsAppWindow { .. })),
            result: Err(server_error()),
        });
        assert!(screen(&model).whatsapp_window_closed());
        assert!(matches!(
            whatsapp(&model).templates,
            TemplateList::Failed(_)
        ));
        assert!(!controls(&model).can_send);
    });
}

#[test]
fn a_window_read_again_that_is_open_takes_free_text_again() {
    with_meta_channels(|| {
        let (mut model, effects) = refused_before_the_window_was_read();
        model.update(Event::WhatsAppWindowLoaded {
            ticket: pick(&effects, |e| matches!(e, Effect::LoadWhatsAppWindow { .. })),
            result: Ok(fixture("district-whatsapp-window.json")),
        });
        assert!(!screen(&model).whatsapp_window_closed());
        assert_eq!(whatsapp(&model).templates, TemplateList::NotLoaded);
        assert!(controls(&model).can_send);
    });
}

#[test]
fn a_template_waits_for_free_text_on_its_way() {
    with_meta_channels(|| {
        // Free text goes while the window is still being read.
        let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
        let opening = opened(&mut model, WHATSAPP_ONLY);
        thread_event(&mut model, ThreadEvent::Compose("On my way".to_owned()));
        let text = pick(&thread_event(&mut model, ThreadEvent::Send), |e| {
            matches!(e, Effect::SendMessage { .. })
        });
        // The window is read closed meanwhile, and a template is filled in.
        let effects = model.update(Event::WhatsAppWindowLoaded {
            ticket: pick(&opening, |e| matches!(e, Effect::LoadWhatsAppWindow { .. })),
            result: Ok(closed()),
        });
        model.update(Event::WhatsAppTemplatesLoaded {
            ticket: last_ticket(&effects),
            result: Ok(fixture("district-whatsapp-templates.json")),
        });
        thread_event(
            &mut model,
            ThreadEvent::ChooseTemplate("wa_tpl_reminder".to_owned()),
        );
        for (index, value) in ["Ada", "Thursday"].into_iter().enumerate() {
            thread_event(
                &mut model,
                ThreadEvent::TemplateVariable(index, value.to_owned()),
            );
        }
        assert!(whatsapp(&model).template_complete());
        assert!(
            !controls(&model).can_send_template,
            "the text is still on its way"
        );
        assert!(thread_event(&mut model, ThreadEvent::SendTemplate).is_empty());

        // Once the text settles, the template can go.
        model.update(Event::MessageSent {
            ticket: text,
            result: Err(server_error()),
        });
        assert!(!screen(&model).composer.sending);
        assert!(controls(&model).can_send_template);
        let message = the_send(&thread_event(&mut model, ThreadEvent::SendTemplate));
        assert_eq!(message.template_id.as_deref(), Some("wa_tpl_reminder"));
    });
}

#[test]
fn leaving_whatsapp_forgets_its_window_and_a_late_answer_lands_nowhere() {
    with_meta_channels(|| {
        let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
        opened(&mut model, ADA);
        let effects = thread_event(
            &mut model,
            ThreadEvent::ChooseChannel("whatsapp".to_owned()),
        );
        let [Effect::LoadWhatsAppWindow { ticket, to, .. }] = effects.as_slice() else {
            panic!("{effects:?}");
        };
        assert_eq!(to, "+14165550142");
        let ticket = *ticket;
        assert!(thread_event(&mut model, ThreadEvent::ChooseChannel("sms".to_owned())).is_empty());
        assert!(screen(&model).whatsapp.is_none());
        assert!(
            model
                .update(Event::WhatsAppWindowLoaded {
                    ticket,
                    result: Ok(closed()),
                })
                .is_empty()
        );
        assert!(screen(&model).whatsapp.is_none());
    });
}

#[test]
fn a_stale_answer_changes_nothing() {
    with_meta_channels(|| {
        let (mut model, _) = inbox_with(AGENCY, "agency", reply_options());
        let effects = opened(&mut model, WHATSAPP_ONLY);
        // The window's ticket, once its answer has landed, is no one's.
        let stale = last_ticket(&effects);
        let templates = model.update(Event::WhatsAppWindowLoaded {
            ticket: stale,
            result: Ok(closed()),
        });
        assert!(has(&templates, |e| matches!(
            e,
            Effect::LoadWhatsAppTemplates { .. }
        )));
        let before = screen(&model).clone();
        for event in [
            Event::ConversationAssigned {
                ticket: stale,
                result: Ok(fixture("district-conversation-assign.json")),
            },
            Event::WhatsAppWindowLoaded {
                ticket: stale,
                result: Ok(fixture("district-whatsapp-window.json")),
            },
            Event::WhatsAppTemplatesLoaded {
                ticket: stale,
                result: Err(server_error()),
            },
        ] {
            assert!(model.update(event).is_empty());
        }
        assert_eq!(screen(&model), &before);
    });
}

#[test]
fn a_thread_opened_before_the_inbox_was_read_can_still_be_assigned() {
    let (mut model, _) = loaded(AGENCY, "agency");
    let effects = opened(&mut model, WALK_IN);
    model.update(Event::MembersLoaded {
        ticket: pick(&effects, |e| matches!(e, Effect::LoadMembers { .. })),
        result: Ok(members()),
    });
    let effects = thread_event(
        &mut model,
        ThreadEvent::Assign(Some("founder@example.com".to_owned())),
    );
    model.update(Event::ConversationAssigned {
        ticket: last_ticket(&effects),
        result: Ok(fixture("district-conversation-assign.json")),
    });
    assert_eq!(
        screen(&model).assignment.assignee.as_deref(),
        Some("teammate@example.com"),
        "the service's answer, whoever was asked for"
    );
}

#[test]
fn a_viewer_reads_no_whatsapp_window() {
    with_meta_channels(|| {
        let (mut model, _) = inbox_with(VIEWER, "viewer", reply_options());
        let effects = opened(&mut model, WHATSAPP_ONLY);
        assert!(!has(&effects, |e| matches!(
            e,
            Effect::LoadWhatsAppWindow { .. }
        )));
        assert!(screen(&model).whatsapp.is_none());
        assert!(!controls(&model).can_reply);
    });
}
