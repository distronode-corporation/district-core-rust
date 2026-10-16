//! Developer: API keys, connected apps and the MCP address, webhooks and
//! their deliveries, and the key and secret shown once.

use district_core::l10n::{Locale, with_locale};
use district_core::scheduling_admin::{
    DeveloperDialog, DeveloperEvent, DeveloperSection, DeveloperTab, KeyDialog, MintedSecret,
    Removal, SectionLoad, WEBHOOK_EVENTS, WEBHOOK_FIELDS, WebhookEditor, WriteState,
    delivery_outcome, is_personal_data,
};
use district_core::{Effect, Model, SchedulingAdminEvent, SchedulingSection, Tone};
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingApiKey, SchedulingApiKeyCreated,
    SchedulingItems, SchedulingMe, SchedulingOauthConnection, SchedulingOk, SchedulingWebhook,
    SchedulingWebhookCreated, SchedulingWebhookDelivery,
};

use super::{
    act, admin, answer, data, done, open, ready_status, refused, sent, status_answer, ticket_for,
};
use crate::support::server_error;

fn section(model: &Model) -> &DeveloperSection {
    admin(model).developer.as_ref().expect("developer is open")
}

fn developer(model: &mut Model, event: DeveloperEvent) -> Vec<Effect> {
    act(model, SchedulingAdminEvent::Developer(event))
}

fn items<T: serde::de::DeserializeOwned>(name: &str) -> Vec<T> {
    data::<SchedulingItems<T>>(name).items
}

fn keys() -> Vec<SchedulingApiKey> {
    items("district-scheduling-api-keys.json")
}

fn apps() -> Vec<SchedulingOauthConnection> {
    items("district-scheduling-oauth-connections.json")
}

fn hooks() -> Vec<SchedulingWebhook> {
    items("district-scheduling-webhooks.json")
}

fn ok() -> SchedulingAdminData {
    SchedulingAdminData::Done(SchedulingOk { ok: true })
}

/// Developer open as `role`: the profile in Toronto, the status read, the
/// keys read.
fn read(role: &str) -> Model {
    let (mut model, effects) = open(role, SchedulingSection::Developer);
    let requests: Vec<SchedulingAdminRequest> = sent(&effects).into_iter().map(|s| s.1).collect();
    assert_eq!(
        requests,
        [SchedulingAdminRequest::Me, SchedulingAdminRequest::ApiKeys]
    );
    let me = SchedulingMe {
        timezone: "America/Toronto".into(),
        ..data::<SchedulingMe>("district-scheduling-me.json")
    };
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::Me),
        done(SchedulingAdminData::Me(me)),
    );
    status_answer(&mut model, &effects, Ok(ready_status()));
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::ApiKeys),
        done(SchedulingAdminData::ApiKeys(keys())),
    );
    model
}

/// The tab shown as `tab`, its read answered with `data`.
fn show(model: &mut Model, tab: DeveloperTab, data: SchedulingAdminData) {
    let effects = developer(model, DeveloperEvent::SelectTab(tab));
    answer(model, sent(&effects)[0].0, done(data));
}

#[test]
fn each_tab_is_read_once_in_the_members_zone() {
    let mut model = read("viewer");
    let s = section(&model);
    assert_eq!(s.times_in(), "Times in America/Toronto");
    let tabs: Vec<&str> = DeveloperTab::ALL.iter().map(|t| t.label()).collect();
    assert_eq!(tabs, ["API keys", "Connected apps", "Webhooks"]);
    assert!(DeveloperSection::labels().iter().all(|l| !l.is_empty()));
    let rows = s.keys.as_ref().and_then(SectionLoad::value).expect("read");
    let lines: Vec<String> = rows.iter().map(|k| s.key_line(k)).collect();
    assert!(lines[0].starts_with("Created "), "{lines:?}");
    assert!(lines[1].ends_with(" · Last used Never"), "{lines:?}");
    let odd = SchedulingApiKey {
        created_at: None,
        last_used_at: Some(Some("not a time".into())),
        ..keys()[0].clone()
    };
    assert_eq!(s.key_line(&odd), "Created Unknown · Last used Unknown");

    show(
        &mut model,
        DeveloperTab::Apps,
        SchedulingAdminData::OauthConnections(apps()),
    );
    let s = section(&model);
    assert_eq!(
        s.mcp(),
        Some((
            "https://book.example.com/mcp".to_owned(),
            "Bearer <your API key>".to_owned()
        ))
    );
    let rows = s.apps.as_ref().and_then(SectionLoad::value).expect("read");
    assert_eq!(s.app_line(&rows[1]), "Never");
    assert_ne!(s.app_line(&rows[0]), "Never");

    show(
        &mut model,
        DeveloperTab::Webhooks,
        SchedulingAdminData::Webhooks(hooks()),
    );
    let states: Vec<&str> = hooks()
        .iter()
        .map(DeveloperSection::webhook_state)
        .collect();
    assert_eq!(states, ["On", "Off"]);
    // Back to a tab read already: no read.
    assert!(developer(&mut model, DeveloperEvent::SelectTab(DeveloperTab::Keys)).is_empty());
    assert_eq!(section(&model).tab, DeveloperTab::Keys);
}

#[test]
fn the_mcp_address_waits_for_a_public_host_and_failed_reads_say_why() {
    let (mut model, effects) = open("agency", SchedulingSection::Developer);
    // No profile: the times stay in UTC.
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::Me),
        refused("unavailable"),
    );
    assert_eq!(section(&model).times_in(), "Times in UTC");
    assert_eq!(section(&model).mcp(), None);
    status_answer(&mut model, &effects, Err(server_error()));
    assert_eq!(section(&model).public_host, None);
    let mut blank = section(&model).clone();
    blank.public_host = Some("  ".into());
    assert_eq!(blank.mcp(), None);
    answer(
        &mut model,
        ticket_for(&effects, |r| *r == SchedulingAdminRequest::ApiKeys),
        Err(server_error()),
    );
    assert!(
        section(&model)
            .keys
            .as_ref()
            .and_then(SectionLoad::failure)
            .is_some()
    );

    // A refresh reads the tab again, and drops the read it supersedes.
    let effects = developer(&mut model, DeveloperEvent::Refresh);
    let again = developer(&mut model, DeveloperEvent::Refresh);
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::ApiKeys(keys())),
    );
    assert!(
        section(&model)
            .keys
            .as_ref()
            .is_some_and(|k| k.value().is_none())
    );
    answer(
        &mut model,
        sent(&again)[0].0,
        done(SchedulingAdminData::ApiKeys(keys())),
    );
    assert!(
        section(&model)
            .keys
            .as_ref()
            .is_some_and(|k| k.value().is_some())
    );
    for tab in [DeveloperTab::Apps, DeveloperTab::Webhooks] {
        developer(&mut model, DeveloperEvent::SelectTab(tab));
        let effects = developer(&mut model, DeveloperEvent::Refresh);
        assert_eq!(sent(&effects).len(), 1);
    }

    // Closed: the events do nothing.
    let (mut model, _) = open("agency", SchedulingSection::Overview);
    assert!(developer(&mut model, DeveloperEvent::Refresh).is_empty());
}

#[test]
fn deliveries_open_under_their_webhook_and_say_what_came_of_each() {
    let mut model = read("viewer");
    show(
        &mut model,
        DeveloperTab::Webhooks,
        SchedulingAdminData::Webhooks(hooks()),
    );
    let effects = developer(
        &mut model,
        DeveloperEvent::ToggleDeliveries("wh-contract-1".into()),
    );
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::WebhookDeliveries {
            id: "wh-contract-1".into()
        }
    );
    let rows: Vec<SchedulingWebhookDelivery> = items("district-scheduling-webhook-deliveries.json");
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Deliveries(rows.clone())),
    );
    assert_eq!(
        section(&model).deliveries["wh-contract-1"]
            .value()
            .map(Vec::len),
        Some(2)
    );
    let never = SchedulingWebhookDelivery {
        last_attempted_at: None,
        ..rows[0].clone()
    };
    assert_eq!(section(&model).delivery_line(&never), "Not tried yet");
    assert!(section(&model).delivery_line(&rows[0]).contains("2026"));
    // Closed, then opened again: not read again.
    developer(
        &mut model,
        DeveloperEvent::ToggleDeliveries("wh-contract-1".into()),
    );
    assert_eq!(section(&model).open_webhook, None);
    assert!(
        developer(
            &mut model,
            DeveloperEvent::ToggleDeliveries("wh-contract-1".into())
        )
        .is_empty()
    );

    let outcome = |status: &str, code: Option<i64>| {
        delivery_outcome(&SchedulingWebhookDelivery {
            status: status.into(),
            response_status: code,
            ..rows[0].clone()
        })
    };
    assert_eq!(
        outcome("success", Some(200)),
        ("Delivered".into(), Tone::Success)
    );
    assert_eq!(outcome("pending", None), ("Waiting".into(), Tone::Neutral));
    assert_eq!(outcome("failed", None), ("No answer".into(), Tone::Danger));
    assert_eq!(
        outcome("failed", Some(0)),
        ("No answer".into(), Tone::Danger)
    );
    assert_eq!(
        outcome("failed", Some(410)),
        ("Rejected with 410".into(), Tone::Danger)
    );
    // A status the vocabulary does not list reads as the generic label, never
    // as itself.
    assert_eq!(outcome("delivered", None), ("Other".into(), Tone::Neutral));
    assert_eq!(
        with_locale(Locale::FrCa, || outcome("bounced", None)),
        ("Autre".into(), Tone::Neutral)
    );
    assert_eq!(
        with_locale(Locale::FrCa, || outcome("success", Some(200))).0,
        "Livr\u{e9}"
    );
}

fn key_dialog(model: &Model) -> &KeyDialog {
    match section(model).dialog.as_ref() {
        Some(DeveloperDialog::Key(dialog)) => dialog,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_key_is_made_named_and_shown_once() {
    let mut model = read("viewer");
    developer(&mut model, DeveloperEvent::OpenCreateKey);
    developer(&mut model, DeveloperEvent::OpenCreateWebhook);
    assert!(section(&model).dialog.is_none());

    let mut model = read("agency");
    developer(&mut model, DeveloperEvent::OpenCreateKey);
    assert!(KeyDialog::labels().iter().all(|l| !l.is_empty()));
    assert!(!key_dialog(&model).can_submit());
    assert_eq!(key_dialog(&model).submit_label(), "Create key");
    // Nothing to copy yet.
    developer(&mut model, DeveloperEvent::KeyCopied);
    assert!(!key_dialog(&model).copied);
    assert!(developer(&mut model, DeveloperEvent::CreateKey).is_empty());
    assert_eq!(
        key_dialog(&model).name_rejected,
        Some("Give the key a name.")
    );
    developer(&mut model, DeveloperEvent::EditKeyName(" Zapier ".into()));
    assert_eq!(key_dialog(&model).name_rejected, None);
    assert!(key_dialog(&model).can_submit());
    let effects = developer(&mut model, DeveloperEvent::CreateKey);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::CreateApiKey {
            name: "Zapier".into()
        }
    );
    assert_eq!(key_dialog(&model).submit_label(), "Creating…");
    assert!(developer(&mut model, DeveloperEvent::CreateKey).is_empty());
    answer(&mut model, sent(&effects)[0].0, refused("unavailable"));
    assert!(key_dialog(&model).write.failure().is_some());
    // An answer that is not a key is not shown as one.
    let effects = developer(&mut model, DeveloperEvent::CreateKey);
    answer(&mut model, sent(&effects)[0].0, done(ok()));
    assert!(key_dialog(&model).write.failure().is_some());
    let effects = developer(&mut model, DeveloperEvent::CreateKey);
    let created: SchedulingApiKeyCreated = data("district-scheduling-api-key-created.json");
    let reread = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::ApiKeyCreated(created)),
    );
    assert_eq!(sent(&reread)[0].1, SchedulingAdminRequest::ApiKeys);
    let dialog = key_dialog(&model);
    let minted = dialog.minted.as_ref().expect("shown");
    assert_eq!(minted.key.expose(), "cno_contract_new_api_key");
    assert!(!format!("{dialog:?}").contains("cno_contract_new_api_key"));
    assert_eq!(minted.title(), "Your key for Zapier");
    assert_eq!(dialog.name, "");
    assert_eq!(dialog.copy_label(), "Copy key");
    developer(&mut model, DeveloperEvent::KeyCopied);
    assert_eq!(key_dialog(&model).copy_label(), "Copied");
    developer(&mut model, DeveloperEvent::CloseDialog);
    assert!(section(&model).dialog.is_none());
}

#[test]
fn keys_apps_and_webhooks_are_removed_asked_once_more() {
    let mut model = read("viewer");
    developer(
        &mut model,
        DeveloperEvent::AskRevokeKey("key-contract-1".into()),
    );
    assert_eq!(section(&model).removal.asking, None);

    let mut model = read("agency");
    developer(&mut model, DeveloperEvent::AskRevokeKey("missing".into()));
    assert!(developer(&mut model, DeveloperEvent::ConfirmRemoval).is_empty());
    developer(
        &mut model,
        DeveloperEvent::AskRevokeKey("key-contract-1".into()),
    );
    let asking = section(&model).removal.asking.clone().expect("asked");
    assert_eq!(
        asking.prompt(),
        (
            "Revoke Zapier?".to_owned(),
            "Anything using it stops working now.",
            "Revoke key"
        )
    );
    developer(&mut model, DeveloperEvent::CancelRemoval);
    assert_eq!(section(&model).removal.asking, None);
    developer(
        &mut model,
        DeveloperEvent::AskRevokeKey("key-contract-1".into()),
    );
    let effects = developer(&mut model, DeveloperEvent::ConfirmRemoval);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::DeleteApiKey {
            id: "key-contract-1".into()
        }
    );
    // One at a time.
    developer(
        &mut model,
        DeveloperEvent::AskRevokeKey("key-contract-2".into()),
    );
    assert_eq!(section(&model).removal.asking, None);
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(section(&model).removal.failure.is_some());
    developer(
        &mut model,
        DeveloperEvent::AskRevokeKey("key-contract-1".into()),
    );
    assert!(section(&model).removal.failure.is_none());
    let effects = developer(&mut model, DeveloperEvent::ConfirmRemoval);
    let reread = answer(&mut model, sent(&effects)[0].0, done(ok()));
    assert_eq!(sent(&reread)[0].1, SchedulingAdminRequest::ApiKeys);

    show(
        &mut model,
        DeveloperTab::Apps,
        SchedulingAdminData::OauthConnections(apps()),
    );
    developer(&mut model, DeveloperEvent::AskRevokeApp("missing".into()));
    assert_eq!(section(&model).removal.asking, None);
    developer(
        &mut model,
        DeveloperEvent::AskRevokeApp("oauth-contract-1".into()),
    );
    let asking = section(&model).removal.asking.clone().expect("asked");
    assert_eq!(asking.prompt().0, "Revoke Raycast?");
    let effects = developer(&mut model, DeveloperEvent::ConfirmRemoval);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::DeleteOauthConnection {
            id: "oauth-contract-1".into()
        }
    );

    let mut model = read("agency");
    show(
        &mut model,
        DeveloperTab::Webhooks,
        SchedulingAdminData::Webhooks(hooks()),
    );
    developer(
        &mut model,
        DeveloperEvent::ToggleDeliveries("wh-contract-1".into()),
    );
    developer(
        &mut model,
        DeveloperEvent::AskDeleteWebhook("missing".into()),
    );
    assert_eq!(section(&model).removal.asking, None);
    developer(
        &mut model,
        DeveloperEvent::AskDeleteWebhook("wh-contract-2".into()),
    );
    let asking = section(&model).removal.asking.clone().expect("asked");
    assert_eq!(
        asking,
        Removal::Webhook {
            id: "wh-contract-2".into()
        }
    );
    assert_eq!(asking.prompt().0, "Delete this webhook?");
    let effects = developer(&mut model, DeveloperEvent::ConfirmRemoval);
    let reread = answer(&mut model, sent(&effects)[0].0, done(ok()));
    // The webhooks are read again, and the open deliveries afresh beside them.
    let requests: Vec<SchedulingAdminRequest> = sent(&reread).into_iter().map(|s| s.1).collect();
    assert_eq!(
        requests,
        [
            SchedulingAdminRequest::Webhooks,
            SchedulingAdminRequest::WebhookDeliveries {
                id: "wh-contract-1".into()
            },
        ]
    );
    assert_eq!(section(&model).deliveries.len(), 1);
    assert_eq!(
        section(&model).deliveries["wh-contract-1"],
        SectionLoad::Loading
    );
}

#[test]
fn a_webhook_save_reads_the_open_deliveries_again_and_a_gone_webhook_closes_them() {
    let mut model = read("agency");
    show(
        &mut model,
        DeveloperTab::Webhooks,
        SchedulingAdminData::Webhooks(hooks()),
    );
    let effects = developer(
        &mut model,
        DeveloperEvent::ToggleDeliveries("wh-contract-1".into()),
    );
    let rows: Vec<SchedulingWebhookDelivery> = items("district-scheduling-webhook-deliveries.json");
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Deliveries(rows.clone())),
    );
    developer(
        &mut model,
        DeveloperEvent::OpenEditWebhook("wh-contract-1".into()),
    );
    let effects = developer(&mut model, DeveloperEvent::SaveWebhook);
    let reread = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Webhooks(Vec::new())),
    );
    let sends = sent(&reread);
    assert_eq!(sends.len(), 2, "{reread:?}");
    assert_eq!(sends[0].1, SchedulingAdminRequest::Webhooks);
    assert_eq!(
        sends[1].1,
        SchedulingAdminRequest::WebhookDeliveries {
            id: "wh-contract-1".into()
        }
    );
    // Still open, and read again: never a list waiting on nothing.
    assert_eq!(
        section(&model).open_webhook.as_deref(),
        Some("wh-contract-1")
    );
    assert_eq!(
        section(&model).deliveries["wh-contract-1"],
        SectionLoad::Loading
    );
    answer(
        &mut model,
        sends[1].0,
        done(SchedulingAdminData::Deliveries(rows)),
    );
    answer(
        &mut model,
        sends[0].0,
        done(SchedulingAdminData::Webhooks(hooks())),
    );
    assert_eq!(
        section(&model).deliveries["wh-contract-1"]
            .value()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        section(&model).open_webhook.as_deref(),
        Some("wh-contract-1")
    );

    // Read again with the open webhook gone: its deliveries close.
    let reread = developer(&mut model, DeveloperEvent::Refresh);
    let sends = sent(&reread);
    assert_eq!(sends.len(), 2, "{reread:?}");
    let others: Vec<SchedulingWebhook> = hooks()
        .into_iter()
        .filter(|w| w.id != "wh-contract-1")
        .collect();
    answer(
        &mut model,
        sends[0].0,
        done(SchedulingAdminData::Webhooks(others)),
    );
    assert_eq!(section(&model).open_webhook, None);
    assert!(!section(&model).deliveries.contains_key("wh-contract-1"));
}

fn editor(model: &Model) -> &WebhookEditor {
    match section(model).dialog.as_ref() {
        Some(DeveloperDialog::Webhook(editor)) => editor,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_webhook_is_added_to_a_public_https_address_and_its_secret_shown_once() {
    let mut model = read("agency");
    show(
        &mut model,
        DeveloperTab::Webhooks,
        SchedulingAdminData::Webhooks(hooks()),
    );
    developer(&mut model, DeveloperEvent::OpenCreateWebhook);
    let e = editor(&model);
    assert_eq!(
        (e.title(), e.submit_label()),
        ("Add webhook", "Add webhook")
    );
    assert!(WebhookEditor::labels().iter().all(|l| !l.is_empty()));
    assert!(!e.can_submit());
    assert_eq!(e.dropped_line(), None);
    // The attendee's data is left out until chosen.
    assert_eq!(e.fields.len(), WEBHOOK_FIELDS.len() - 4);
    assert!(!e.fields.iter().any(|f| f == "answers"));
    assert!(is_personal_data("host_email") && is_personal_data("answers"));
    assert!(!is_personal_data("status"));

    for (url, issue) in [
        ("  ", "Give the webhook a URL."),
        (
            "not a url",
            "That is not a URL. It has to start with https://.",
        ),
        (
            "http://hooks.example.com",
            "The URL has to start with https://.",
        ),
        (
            "https://192.168.1.4/hook",
            "That address is not reachable from the internet. Use a public https URL.",
        ),
    ] {
        developer(&mut model, DeveloperEvent::EditWebhookUrl(url.into()));
        assert!(developer(&mut model, DeveloperEvent::SaveWebhook).is_empty());
        assert_eq!(editor(&model).url_rejected, Some(issue), "{url}");
        assert_eq!(
            editor(&model).events_rejected,
            Some("Choose at least one event.")
        );
    }
    developer(
        &mut model,
        DeveloperEvent::EditWebhookUrl(" https://hooks.example.com/b ".into()),
    );
    assert_eq!(editor(&model).url_rejected, None);
    // Only the four booking events are offered: one about recordings, which
    // are never made, cannot be ticked.
    assert_eq!(
        WEBHOOK_EVENTS,
        [
            "booking.created",
            "booking.cancelled",
            "booking.rescheduled",
            "booking.reminder"
        ]
    );
    for event in ["notes.ready", "booking.created", "booking.reminder"] {
        developer(&mut model, DeveloperEvent::ToggleEvent(event.into()));
    }
    developer(
        &mut model,
        DeveloperEvent::ToggleEvent("booking.reminder".into()),
    );
    assert_eq!(editor(&model).events, ["booking.created"]);
    assert_eq!(editor(&model).events_rejected, None);
    developer(&mut model, DeveloperEvent::ToggleField("answers".into()));
    developer(&mut model, DeveloperEvent::ToggleField("id".into()));
    assert!(editor(&model).can_submit());
    let effects = developer(&mut model, DeveloperEvent::SaveWebhook);
    let SchedulingAdminRequest::CreateWebhook {
        url,
        events,
        fields,
    } = sent(&effects)[0].1.clone()
    else {
        panic!("{effects:?}");
    };
    assert_eq!(url, "https://hooks.example.com/b");
    assert_eq!(events, ["booking.created"]);
    let fields = fields.expect("sent");
    assert_eq!(fields.first().map(String::as_str), Some("status"));
    assert_eq!(fields.last().map(String::as_str), Some("hours_before"));
    assert!(fields.iter().any(|f| f == "answers"));
    assert_eq!(editor(&model).submit_label(), "Saving…");
    // One save at a time.
    assert!(developer(&mut model, DeveloperEvent::ToggleEvent("x".into())).is_empty());
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(editor(&model).write.failure().is_some());

    let effects = developer(&mut model, DeveloperEvent::SaveWebhook);
    let created: SchedulingWebhookCreated = data("district-scheduling-webhook-created.json");
    let reread = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::WebhookCreated(created.clone())),
    );
    assert_eq!(sent(&reread)[0].1, SchedulingAdminRequest::Webhooks);
    let minted = editor(&model).minted.clone().expect("shown");
    assert_eq!(minted.secret.expose(), "whsec_contract_shown_once");
    assert_eq!(
        minted.note(),
        "Every delivery to https://hooks.contract.test/bookings is signed with it, in the \
         X-Calnode-Signature header."
    );
    assert!(MintedSecret::labels().iter().all(|l| !l.is_empty()));
    assert_eq!(editor(&model).write, WriteState::Idle);

    // Without a secret, the add says it was done.
    developer(&mut model, DeveloperEvent::OpenCreateWebhook);
    developer(
        &mut model,
        DeveloperEvent::EditWebhookUrl("https://hooks.example.com".into()),
    );
    developer(
        &mut model,
        DeveloperEvent::ToggleEvent("booking.created".into()),
    );
    let effects = developer(&mut model, DeveloperEvent::SaveWebhook);
    let no_secret = SchedulingWebhookCreated {
        secret: None,
        ..created
    };
    answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::WebhookCreated(no_secret)),
    );
    assert_eq!(
        editor(&model).write,
        WriteState::Done("Webhook added".into())
    );
    assert_eq!(editor(&model).minted, None);
}

#[test]
fn a_webhooks_events_and_fields_are_changed_its_address_fixed() {
    let mut model = read("agency");
    let mut rows = hooks();
    rows[1].events.push("booking.no_show".into());
    show(
        &mut model,
        DeveloperTab::Webhooks,
        SchedulingAdminData::Webhooks(rows.clone()),
    );
    developer(
        &mut model,
        DeveloperEvent::OpenEditWebhook("missing".into()),
    );
    assert!(section(&model).dialog.is_none());
    developer(
        &mut model,
        DeveloperEvent::OpenEditWebhook("wh-contract-1".into()),
    );
    let e = editor(&model);
    assert_eq!(
        (e.title(), e.submit_label()),
        ("Edit webhook", "Save changes")
    );
    assert_eq!(e.fields, ["attendee_name", "attendee_email"]);
    // The address cannot be changed.
    developer(
        &mut model,
        DeveloperEvent::EditWebhookUrl("https://elsewhere.example".into()),
    );
    assert_eq!(editor(&model).url, "https://hooks.contract.test/bookings");
    developer(
        &mut model,
        DeveloperEvent::ToggleEvent("booking.rescheduled".into()),
    );
    let effects = developer(&mut model, DeveloperEvent::SaveWebhook);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::PatchWebhook {
            id: "wh-contract-1".into(),
            events: Some(vec![
                "booking.created".into(),
                "booking.cancelled".into(),
                "booking.rescheduled".into()
            ]),
            fields: Some(vec!["attendee_name".into(), "attendee_email".into()]),
        }
    );
    let reread = answer(
        &mut model,
        sent(&effects)[0].0,
        done(SchedulingAdminData::Webhooks(Vec::new())),
    );
    answer(
        &mut model,
        sent(&reread)[0].0,
        done(SchedulingAdminData::Webhooks(rows)),
    );
    assert_eq!(
        editor(&model).write,
        WriteState::Done("Webhook updated".into())
    );

    // An event this build does not know is said to stop on save; no fields
    // stored reads as the defaults.
    developer(
        &mut model,
        DeveloperEvent::OpenEditWebhook("wh-contract-2".into()),
    );
    let e = editor(&model);
    assert_eq!(e.dropped_events(), ["booking.no_show"]);
    // An event the scheduler knows and the form does not offer is kept, and
    // cannot be unticked here.
    assert_eq!(e.kept_events(), ["recording.completed"]);
    assert_eq!(
        e.dropped_line().as_deref(),
        Some(
            "Saving will stop sending booking.no_show. This app does not know those events, so \
             it cannot send them back."
        )
    );
    assert_eq!(e.fields.len(), WEBHOOK_FIELDS.len() - 4);

    assert!(
        developer(
            &mut model,
            DeveloperEvent::ToggleEvent("recording.completed".into())
        )
        .is_empty()
    );
    developer(
        &mut model,
        DeveloperEvent::ToggleEvent("booking.reminder".into()),
    );
    assert_eq!(
        editor(&model).events,
        ["booking.reminder", "recording.completed"]
    );

    // Answers for a dialog that closed land nowhere.
    let effects = developer(&mut model, DeveloperEvent::SaveWebhook);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::PatchWebhook {
            id: "wh-contract-2".into(),
            events: Some(vec![
                "booking.reminder".into(),
                "recording.completed".into()
            ]),
            fields: Some(editor(&model).fields.clone()),
        }
    );
    developer(&mut model, DeveloperEvent::CloseDialog);
    answer(&mut model, sent(&effects)[0].0, done(ok()));
    assert!(section(&model).dialog.is_none());
    // No dialog: its events do nothing.
    assert!(developer(&mut model, DeveloperEvent::SaveWebhook).is_empty());
    developer(&mut model, DeveloperEvent::OpenCreateKey);
    assert!(developer(&mut model, DeveloperEvent::SaveWebhook).is_empty());
}

/// Deliveries that could not be read are read again on request, for the
/// webhook open and only after a failure.
#[test]
fn deliveries_that_failed_are_read_again_on_request() {
    let mut model = read("viewer");
    show(
        &mut model,
        DeveloperTab::Webhooks,
        SchedulingAdminData::Webhooks(hooks()),
    );
    assert!(developer(&mut model, DeveloperEvent::RetryDeliveries).is_empty());
    let effects = developer(
        &mut model,
        DeveloperEvent::ToggleDeliveries("wh-contract-1".into()),
    );
    assert!(
        developer(&mut model, DeveloperEvent::RetryDeliveries).is_empty(),
        "still reading"
    );
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    assert!(
        section(&model).deliveries["wh-contract-1"]
            .failure()
            .is_some()
    );
    let effects = developer(&mut model, DeveloperEvent::RetryDeliveries);
    assert_eq!(
        sent(&effects)[0].1,
        SchedulingAdminRequest::WebhookDeliveries {
            id: "wh-contract-1".into()
        }
    );
    assert_eq!(
        section(&model).deliveries["wh-contract-1"],
        SectionLoad::Loading
    );
    answer(&mut model, sent(&effects)[0].0, Err(server_error()));
    // Closed, a failure is not read again.
    developer(
        &mut model,
        DeveloperEvent::ToggleDeliveries("wh-contract-1".into()),
    );
    assert!(developer(&mut model, DeveloperEvent::RetryDeliveries).is_empty());
}
