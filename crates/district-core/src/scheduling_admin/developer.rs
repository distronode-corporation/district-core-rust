//! Developer: the API keys that let the workspace's own tools reach its
//! bookings, the apps connected with OAuth, an AI assistant's MCP address,
//! and webhooks with their deliveries.
//!
//! A key and a webhook's signing secret are shown once, when made, and never
//! again. Revoking a key or an app and deleting a webhook are asked once more.
//! Reading is open to every member; every write needs a member who may change
//! the workspace.

use std::collections::BTreeMap;

use district_l10n::{WireCategory, scheduling_strings as ss, wire_label};
use district_model::{
    SCHEDULING_WEBHOOK_EVENTS, SchedulingAdminData, SchedulingAdminRequest, SchedulingApiKey,
    SchedulingOauthConnection, SchedulingSecret, SchedulingStatusResponse, SchedulingWebhook,
    SchedulingWebhookDelivery,
};
use url::Url;

use super::overview::load;
use super::{
    Answer, Out, Pending as AdminPending, SchedulingAdmin, SectionLoad, WriteState, clock,
};
use crate::failure::FailureText;
use crate::model::Tickets;
use crate::text::l;
use crate::workflows::Tone;

/// The events the form offers, as the web portal offers them: the four about
/// bookings. The scheduler also knows `recording.completed`,
/// `transcript.ready` and `notes.ready` ([`SCHEDULING_WEBHOOK_EVENTS`]), which
/// never fire, since no meeting is recorded. A webhook already subscribed to
/// one of those keeps it when saved here; it is not offered to tick.
pub const WEBHOOK_EVENTS: [&str; 4] = [
    "booking.created",
    "booking.cancelled",
    "booking.rescheduled",
    "booking.reminder",
];

/// Every part of a booking a webhook delivery can carry, in the order the
/// form lists them.
pub const WEBHOOK_FIELDS: [&str; 22] = [
    "id",
    "status",
    "start_at",
    "end_at",
    "created_at",
    "location_value",
    "cancellation_reason",
    "previous_start_at",
    "previous_end_at",
    "event_type_slug",
    "event_type_name",
    "host_id",
    "host_name",
    "host_email",
    "attendee_name",
    "attendee_email",
    "attendee_timezone",
    "answers",
    "payment_status",
    "amount_paid_cents",
    "amount_paid_currency",
    "hours_before",
];

/// The attendee's own data: left out of a new webhook until chosen.
const ATTENDEE_FIELDS: [&str; 4] = [
    "attendee_name",
    "attendee_email",
    "attendee_timezone",
    "answers",
];

/// Whether `field` is somebody's personal data, which the form marks.
pub fn is_personal_data(field: &str) -> bool {
    field == "host_name" || field == "host_email" || ATTENDEE_FIELDS.contains(&field)
}

/// The section's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DeveloperTab {
    /// API keys.
    #[default]
    Keys,
    /// Connected apps, and the MCP address.
    Apps,
    /// Webhooks and their deliveries.
    Webhooks,
}

impl DeveloperTab {
    /// The tabs in order.
    pub const ALL: [Self; 3] = [Self::Keys, Self::Apps, Self::Webhooks];

    /// Its label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Keys => ss::developer_tab_keys(l()),
            Self::Apps => ss::developer_tab_apps(l()),
            Self::Webhooks => ss::developer_tab_webhooks(l()),
        }
    }
}

/// What a delivery came to, as (label, tone): delivered, waiting, no answer,
/// rejected with the code the endpoint answered, or for a status this build
/// does not know the generic label ("Other"), never the status as it came.
pub fn delivery_outcome(delivery: &SchedulingWebhookDelivery) -> (String, Tone) {
    match delivery.status.as_str() {
        "success" => (
            ss::developer_delivery_delivered(l()).to_owned(),
            Tone::Success,
        ),
        "pending" => (
            ss::developer_delivery_waiting(l()).to_owned(),
            Tone::Neutral,
        ),
        "failed" => match delivery.response_status.filter(|code| *code != 0) {
            None => (
                ss::developer_delivery_no_answer(l()).to_owned(),
                Tone::Danger,
            ),
            Some(code) => (
                ss::developer_delivery_rejected(l(), &code.to_string()),
                Tone::Danger,
            ),
        },
        other => (
            wire_label(WireCategory::SchedulingWebhookDeliveryStatus, other, l()),
            Tone::Neutral,
        ),
    }
}

/// Something to remove, asked once more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Removal {
    /// Revoke an API key.
    Key {
        /// Its id.
        id: String,
        /// Its name.
        name: String,
    },
    /// Revoke a connected app's access.
    App {
        /// The connection's id.
        id: String,
        /// The app's name.
        name: String,
    },
    /// Delete a webhook.
    Webhook {
        /// Its id.
        id: String,
    },
}

impl Removal {
    /// The question, what removing does, and the confirm button.
    pub fn prompt(&self) -> (String, &'static str, &'static str) {
        match self {
            Self::Key { name, .. } => (
                ss::key_revoke_prompt(l(), name),
                ss::key_revoke_confirm(l()),
                ss::key_revoke_action(l()),
            ),
            Self::App { name, .. } => (
                ss::app_revoke_prompt(l(), name),
                ss::app_revoke_confirm(l()),
                ss::app_revoke_action(l()),
            ),
            Self::Webhook { .. } => (
                ss::webhook_delete_prompt(l()).to_owned(),
                ss::webhook_delete_confirm(l()),
                ss::webhook_delete_action(l()),
            ),
        }
    }
}

/// Removing a key, an app or a webhook.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemovalState {
    /// What is being asked about.
    pub asking: Option<Removal>,
    /// Whether a removal is on its way. One at a time.
    pub busy: bool,
    /// Why the last one failed.
    pub failure: Option<FailureText>,
}

/// A key just made: shown once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MintedKey {
    /// Its name.
    pub name: String,
    /// The key itself, redacted in `Debug`.
    pub key: SchedulingSecret,
}

impl MintedKey {
    /// "Your key for …".
    pub fn title(&self) -> String {
        ss::key_reveal_title(l(), &self.name)
    }
}

/// Making an API key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyDialog {
    /// The name, as typed.
    pub name: String,
    /// Why the name cannot be sent.
    pub name_rejected: Option<&'static str>,
    /// The create's state.
    pub write: WriteState,
    /// The key made, until the dialog closes.
    pub minted: Option<MintedKey>,
    /// Whether the key was copied.
    pub copied: bool,
}

impl KeyDialog {
    /// Whether Create can be pressed.
    pub fn can_submit(&self) -> bool {
        !self.write.busy() && !self.name.trim().is_empty()
    }

    /// The create button: "Create key", or "Creating…" while it is sent.
    pub fn submit_label(&self) -> &'static str {
        if self.write.busy() {
            ss::key_creating(l())
        } else {
            ss::key_create(l())
        }
    }

    /// The copy button: "Copy key", then "Copied".
    pub fn copy_label(&self) -> &'static str {
        if self.copied {
            ss::key_copied(l())
        } else {
            ss::key_copy_action(l())
        }
    }

    /// The labels: the title, the name's label and hint, the warning beside
    /// the key, and the button that closes it.
    pub fn labels() -> [&'static str; 5] {
        [
            ss::key_sheet_title(l()),
            ss::key_name_label(l()),
            ss::key_name_hint(l()),
            ss::key_reveal_warning(l()),
            ss::key_done(l()),
        ]
    }
}

/// A webhook's signing secret, just made: shown once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MintedSecret {
    /// The webhook's address.
    pub url: String,
    /// The secret, redacted in `Debug`.
    pub secret: SchedulingSecret,
}

impl MintedSecret {
    /// Where it is used.
    pub fn note(&self) -> String {
        ss::webhook_secret_note(l(), &self.url)
    }

    /// The labels: the title, the warning, the copy button and the button that
    /// closes it.
    pub fn labels() -> [&'static str; 4] {
        [
            ss::webhook_secret_title(l()),
            ss::webhook_secret_reveal_warning(l()),
            ss::webhook_secret_copy_action(l()),
            ss::key_done(l()),
        ]
    }
}

/// Adding a webhook, or changing what one sends. Its address is fixed once
/// added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebhookEditor {
    /// The webhook changed; `None` for an add.
    pub editing: Option<SchedulingWebhook>,
    /// The address, as typed (an add only).
    pub url: String,
    /// The events chosen, in the scheduler's order: those ticked among
    /// [`WEBHOOK_EVENTS`], and any other event the scheduler knows that the
    /// webhook already sends, which a save keeps.
    pub events: Vec<String>,
    /// The fields chosen, in the form's order.
    pub fields: Vec<String>,
    /// Why the address cannot be sent.
    pub url_rejected: Option<&'static str>,
    /// Why the events cannot be sent.
    pub events_rejected: Option<&'static str>,
    /// The save's state.
    pub write: WriteState,
    /// The signing secret of the webhook just added.
    pub minted: Option<MintedSecret>,
}

impl WebhookEditor {
    fn create() -> Self {
        Self {
            editing: None,
            url: String::new(),
            events: Vec::new(),
            fields: WEBHOOK_FIELDS
                .iter()
                .filter(|f| !ATTENDEE_FIELDS.contains(f))
                .map(|f| (*f).to_owned())
                .collect(),
            url_rejected: None,
            events_rejected: None,
            write: WriteState::Idle,
            minted: None,
        }
    }

    fn edit(webhook: SchedulingWebhook) -> Self {
        let events = ordered(&SCHEDULING_WEBHOOK_EVENTS, &webhook.events);
        let fields = if webhook.fields.as_ref().is_some_and(Option::is_some) {
            ordered(&WEBHOOK_FIELDS, webhook.fields())
        } else {
            Self::create().fields
        };
        Self {
            url: webhook.url.clone(),
            events,
            fields,
            editing: Some(webhook),
            ..Self::create()
        }
    }

    /// "Add webhook" or "Edit webhook".
    pub fn title(&self) -> &'static str {
        match self.editing {
            None => ss::webhook_add_title(l()),
            Some(_) => ss::webhook_edit_title(l()),
        }
    }

    /// The save button: "Add webhook", "Save changes", or "Saving…".
    pub fn submit_label(&self) -> &'static str {
        match (&self.editing, self.write.busy()) {
            (_, true) => ss::webhook_saving(l()),
            (None, false) => ss::webhook_add_action(l()),
            (Some(_), false) => ss::webhook_save_action(l()),
        }
    }

    /// Whether the save can be pressed.
    pub fn can_submit(&self) -> bool {
        !self.write.busy() && !self.events.is_empty()
    }

    /// The events this webhook sends that the scheduler knows but the form does
    /// not offer ([`WEBHOOK_EVENTS`]): a save keeps them.
    pub fn kept_events(&self) -> Vec<&str> {
        self.events
            .iter()
            .filter(|e| !WEBHOOK_EVENTS.contains(&e.as_str()))
            .map(String::as_str)
            .collect()
    }

    /// The events this webhook sends that this build does not know: a save
    /// stops them, which the form says.
    pub fn dropped_events(&self) -> Vec<&str> {
        self.editing
            .iter()
            .flat_map(|w| w.events.iter())
            .filter(|e| !SCHEDULING_WEBHOOK_EVENTS.contains(&e.as_str()))
            .map(String::as_str)
            .collect()
    }

    /// The warning about [`dropped_events`](Self::dropped_events), if any.
    pub fn dropped_line(&self) -> Option<String> {
        let dropped = self.dropped_events();
        (!dropped.is_empty()).then(|| ss::webhook_dropped_events_line(l(), &dropped.join(", ")))
    }

    /// The labels: the address's label, its hint, the line saying it is fixed
    /// once added, the events' label and hint, the fields' label and hint,
    /// and the personal data mark.
    pub fn labels() -> [&'static str; 8] {
        [
            ss::webhook_url_label(l()),
            ss::webhook_url_hint(l()),
            ss::webhook_url_fixed(l()),
            ss::webhook_events_label(l()),
            ss::webhook_events_hint(l()),
            ss::webhook_fields_label(l()),
            ss::webhook_fields_hint(l()),
            ss::webhook_personal_data(l()),
        ]
    }
}

/// The values of `all` that `chosen` has, in `all`'s order.
fn ordered(all: &[&str], chosen: &[String]) -> Vec<String> {
    all.iter()
        .filter(|v| chosen.iter().any(|c| c == *v))
        .map(|v| (*v).to_owned())
        .collect()
}

/// Adds `value` to `set` in `all`'s order, or takes it out.
fn toggle(all: &[&str], set: &mut Vec<String>, value: &str) {
    let mut chosen = std::mem::take(set);
    match chosen.iter().position(|c| c == value) {
        Some(index) => {
            chosen.remove(index);
        }
        None => chosen.push(value.to_owned()),
    }
    *set = ordered(all, &chosen);
}

/// Why `value` cannot be a webhook's address: missing, not a URL, not https,
/// or a host the internet cannot reach.
fn url_issue(value: &str) -> Option<&'static str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Some(ss::webhook_url_missing(l()));
    }
    let Some(url) = Url::parse(trimmed)
        .ok()
        .filter(|u| u.host_str().is_some_and(|h| !h.is_empty()))
    else {
        return Some(ss::webhook_url_not_a_url(l()));
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if url.scheme() != "https" {
        Some(ss::webhook_url_not_https(l()))
    } else if is_private_host(&host) {
        Some(ss::webhook_url_private(l()))
    } else {
        None
    }
}

/// Loopback, link-local and private addresses, and `.local` names.
fn is_private_host(host: &str) -> bool {
    let private_172 = host
        .strip_prefix("172.")
        .and_then(|rest| rest.split('.').next())
        .and_then(|second| second.parse::<u8>().ok())
        .is_some_and(|second| (16..=31).contains(&second));
    host == "localhost"
        || host.ends_with(".local")
        || host == "0.0.0.0"
        || host == "[::1]"
        || ["127.", "10.", "192.168.", "169.254."]
            .iter()
            .any(|prefix| host.starts_with(prefix))
        || private_172
}

/// A dialog over the section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeveloperDialog {
    /// Making a key.
    Key(KeyDialog),
    /// Adding or changing a webhook.
    Webhook(WebhookEditor),
}

/// The developer section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeveloperSection {
    /// The tab shown.
    pub tab: DeveloperTab,
    /// The member's time zone, which the times are shown in.
    pub timezone: String,
    /// The scheduler's public address, for the MCP URL; `None` until the
    /// status is read, or when it has none.
    pub public_host: Option<String>,
    /// The keys; `None` until their tab is first shown.
    pub keys: Option<SectionLoad<Vec<SchedulingApiKey>>>,
    /// The connected apps; `None` until their tab is first shown.
    pub apps: Option<SectionLoad<Vec<SchedulingOauthConnection>>>,
    /// The webhooks; `None` until their tab is first shown.
    pub webhooks: Option<SectionLoad<Vec<SchedulingWebhook>>>,
    /// Each webhook's deliveries, by webhook id: only those opened.
    pub deliveries: BTreeMap<String, SectionLoad<Vec<SchedulingWebhookDelivery>>>,
    /// The webhook whose deliveries are open.
    pub open_webhook: Option<String>,
    /// Revoking or deleting.
    pub removal: RemovalState,
    /// The dialog open over the section.
    pub dialog: Option<DeveloperDialog>,
}

impl Default for DeveloperSection {
    fn default() -> Self {
        Self {
            tab: DeveloperTab::Keys,
            timezone: "UTC".to_owned(),
            public_host: None,
            keys: None,
            apps: None,
            webhooks: None,
            deliveries: BTreeMap::new(),
            open_webhook: None,
            removal: RemovalState::default(),
            dialog: None,
        }
    }
}

impl DeveloperSection {
    /// "Times in America/Toronto".
    pub fn times_in(&self) -> String {
        clock::times_in(&self.timezone)
    }

    /// When something happened, in the member's zone; `absent` when it never
    /// did, and "Unknown" for a time that does not read.
    fn stamp(&self, at: Option<&str>, absent: &str) -> String {
        match at.filter(|a| !a.is_empty()) {
            None => absent.to_owned(),
            Some(at) => clock::date_time_or_unknown(at, &self.timezone),
        }
    }

    /// A key's line: when it was made and last used.
    pub fn key_line(&self, key: &SchedulingApiKey) -> String {
        ss::developer_key_value(
            l(),
            &self.stamp(key.created_at.as_deref(), ss::unknown_time(l())),
            &self.stamp(key.last_used_at(), ss::developer_never(l())),
        )
    }

    /// When an app last reached the bookings, or "Never".
    pub fn app_line(&self, app: &SchedulingOauthConnection) -> String {
        self.stamp(app.last_used_at(), ss::developer_never(l()))
    }

    /// A webhook's state: "On" unless it was switched off.
    pub fn webhook_state(webhook: &SchedulingWebhook) -> &'static str {
        if webhook.is_active == Some(false) {
            ss::common_off(l())
        } else {
            ss::common_on(l())
        }
    }

    /// When a delivery was last tried, or "Not tried yet".
    pub fn delivery_line(&self, delivery: &SchedulingWebhookDelivery) -> String {
        self.stamp(
            delivery.last_attempted_at.as_deref(),
            ss::developer_not_tried_yet(l()),
        )
    }

    /// The MCP address and the header to send, as (URL, header value); `None`
    /// while the scheduler has no public address.
    pub fn mcp(&self) -> Option<(String, String)> {
        let host = self.public_host.as_deref().map(str::trim)?;
        (!host.is_empty()).then(|| {
            (
                format!("https://{host}/mcp"),
                ss::developer_mcp_auth_value(l(), ss::developer_mcp_key_placeholder(l())),
            )
        })
    }

    /// The labels: each tab's heading, subtitle and empty line, the MCP card's
    /// heading, its line while there is no address, its two row labels and
    /// its note, the deliveries button and its empty line, and the two add
    /// buttons.
    pub fn labels() -> [&'static str; 18] {
        [
            ss::developer_keys_eyebrow(l()),
            ss::developer_keys_subtitle(l()),
            ss::developer_keys_empty(l()),
            ss::developer_apps_eyebrow(l()),
            ss::developer_apps_subtitle(l()),
            ss::developer_apps_empty(l()),
            ss::developer_webhooks_eyebrow(l()),
            ss::developer_webhooks_subtitle(l()),
            ss::developer_webhooks_empty(l()),
            ss::developer_mcp_eyebrow(l()),
            ss::developer_mcp_no_host(l()),
            ss::developer_mcp_url_label(l()),
            ss::developer_mcp_auth_label(l()),
            ss::developer_mcp_note(l()),
            ss::developer_deliveries(l()),
            ss::developer_deliveries_empty(l()),
            ss::key_create(l()),
            ss::webhook_add_action(l()),
        ]
    }
}

/// What the member does on Developer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeveloperEvent {
    /// Show a tab, read the first time.
    SelectTab(DeveloperTab),
    /// Read the tab shown again.
    Refresh,
    /// Open a webhook's deliveries, read the first time, or close them.
    ToggleDeliveries(String),
    /// Read the open webhook's deliveries again, after their read failed.
    RetryDeliveries,
    /// Open the key form.
    OpenCreateKey,
    /// The key's name changed.
    EditKeyName(String),
    /// Make the key.
    CreateKey,
    /// The key was copied.
    KeyCopied,
    /// Ask about revoking a key, by id.
    AskRevokeKey(String),
    /// Ask about revoking an app's access, by connection id.
    AskRevokeApp(String),
    /// Ask about deleting a webhook, by id.
    AskDeleteWebhook(String),
    /// Do not remove it.
    CancelRemoval,
    /// Remove it.
    ConfirmRemoval,
    /// Open the add form.
    OpenCreateWebhook,
    /// Open a webhook's form, by id.
    OpenEditWebhook(String),
    /// The address changed (an add only).
    EditWebhookUrl(String),
    /// Choose an event, or take it out.
    ToggleEvent(String),
    /// Choose a field, or take it out.
    ToggleField(String),
    /// Add the webhook, or save what it sends.
    SaveWebhook,
    /// Close the dialog.
    CloseDialog,
}

/// What an answer the section awaits is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    Me,
    Keys,
    Apps,
    Webhooks,
    Deliveries(String),
    Removal,
    Key,
    Webhook,
}

fn send(pending: Pending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::Developer(pending), Box::new(request))
}

pub(crate) fn enter(admin: &mut SchedulingAdmin, tickets: &mut Tickets) -> Vec<Out> {
    admin.forget(tickets, |p| matches!(p, AdminPending::Developer(_)));
    let mut section = DeveloperSection::default();
    let mut outs = vec![send(Pending::Me, SchedulingAdminRequest::Me)];
    outs.extend(read_tab(&mut section));
    admin.developer = Some(section);
    outs
}

/// The scheduler's status arrived (or failed to): the MCP address.
pub(crate) fn status_arrived(
    admin: &mut SchedulingAdmin,
    status: Option<&SchedulingStatusResponse>,
) {
    let host = status
        .and_then(|s| s.tenant.as_ref())
        .map(|t| t.public_host.clone());
    let mut host = Some(host);
    admin
        .developer
        .iter_mut()
        .for_each(|section| section.public_host = host.take().flatten());
}

/// The tab shown, read when it has not been.
fn read_tab(section: &mut DeveloperSection) -> Vec<Out> {
    let (slot, pending, request) = match section.tab {
        DeveloperTab::Keys => (
            &mut section.keys,
            Pending::Keys,
            SchedulingAdminRequest::ApiKeys,
        ),
        DeveloperTab::Apps => {
            return read_into(
                &mut section.apps,
                Pending::Apps,
                SchedulingAdminRequest::OauthConnections,
            );
        }
        DeveloperTab::Webhooks => {
            return read_into(
                &mut section.webhooks,
                Pending::Webhooks,
                SchedulingAdminRequest::Webhooks,
            );
        }
    };
    read_into(slot, pending, request)
}

/// `request` sent when `slot` was never read, and `slot` marked as reading.
fn read_into<T>(
    slot: &mut Option<SectionLoad<T>>,
    pending: Pending,
    request: SchedulingAdminRequest,
) -> Vec<Out> {
    if slot.is_some() {
        return Vec::new();
    }
    *slot = Some(SectionLoad::Loading);
    vec![send(pending, request)]
}

/// The tab shown, read again: after a write, or asked for. Every webhook's
/// deliveries are dropped with the webhooks, and the open one's are read again
/// beside them, so an open list never waits on a read nobody sent.
fn reread(section: &mut DeveloperSection) -> Vec<Out> {
    match section.tab {
        DeveloperTab::Keys => section.keys = None,
        DeveloperTab::Apps => section.apps = None,
        DeveloperTab::Webhooks => {
            section.webhooks = None;
            section.deliveries.clear();
        }
    }
    let mut out = read_tab(section);
    if let (DeveloperTab::Webhooks, Some(id)) = (section.tab, section.open_webhook.clone()) {
        out.push(read_deliveries(section, id));
    }
    out
}

/// The deliveries of webhook `id`, marked as reading.
fn read_deliveries(section: &mut DeveloperSection, id: String) -> Out {
    section.deliveries.insert(id.clone(), SectionLoad::Loading);
    send(
        Pending::Deliveries(id.clone()),
        SchedulingAdminRequest::WebhookDeliveries { id },
    )
}

/// The pending read of `tab`.
fn reading(tab: DeveloperTab) -> AdminPending {
    AdminPending::Developer(match tab {
        DeveloperTab::Keys => Pending::Keys,
        DeveloperTab::Apps => Pending::Apps,
        DeveloperTab::Webhooks => Pending::Webhooks,
    })
}

pub(crate) fn event(
    admin: &mut SchedulingAdmin,
    tickets: &mut Tickets,
    event: DeveloperEvent,
    can_change: bool,
) -> Vec<Out> {
    let Some(tab) = admin.developer.as_ref().map(|s| s.tab) else {
        return Vec::new();
    };
    if event == DeveloperEvent::Refresh {
        // A read still on its way is superseded.
        let superseded = reading(tab);
        admin.forget(tickets, |p| *p == superseded);
    }
    admin
        .developer
        .as_mut()
        .map(|section| section_event(section, event, can_change))
        .unwrap_or_default()
}

fn section_event(
    section: &mut DeveloperSection,
    event: DeveloperEvent,
    can_change: bool,
) -> Vec<Out> {
    match event {
        DeveloperEvent::SelectTab(tab) => {
            section.tab = tab;
            return read_tab(section);
        }
        DeveloperEvent::Refresh => return reread(section),
        DeveloperEvent::ToggleDeliveries(id) => {
            if section.open_webhook.as_deref() == Some(id.as_str()) {
                section.open_webhook = None;
            } else {
                section.open_webhook = Some(id.clone());
                if !section.deliveries.contains_key(&id) {
                    return vec![read_deliveries(section, id)];
                }
            }
        }
        DeveloperEvent::RetryDeliveries => {
            let failed = section.open_webhook.clone().filter(|id| {
                section
                    .deliveries
                    .get(id)
                    .is_some_and(|load| load.failure().is_some())
            });
            if let Some(id) = failed {
                return vec![read_deliveries(section, id)];
            }
        }
        DeveloperEvent::AskRevokeKey(id) if can_change && !section.removal.busy => {
            let name = section
                .keys
                .as_ref()
                .and_then(SectionLoad::value)
                .and_then(|keys| keys.iter().find(|k| k.id == id))
                .map(|k| k.name.clone());
            if let Some(name) = name {
                ask(&mut section.removal, Removal::Key { id, name });
            }
        }
        DeveloperEvent::AskRevokeApp(id) if can_change && !section.removal.busy => {
            let name = section
                .apps
                .as_ref()
                .and_then(SectionLoad::value)
                .and_then(|apps| apps.iter().find(|a| a.id == id))
                .map(|a| a.client_name.clone());
            if let Some(name) = name {
                ask(&mut section.removal, Removal::App { id, name });
            }
        }
        DeveloperEvent::AskDeleteWebhook(id) if can_change && !section.removal.busy => {
            let known = section
                .webhooks
                .as_ref()
                .and_then(SectionLoad::value)
                .is_some_and(|hooks| hooks.iter().any(|w| w.id == id));
            if known {
                ask(&mut section.removal, Removal::Webhook { id });
            }
        }
        DeveloperEvent::CancelRemoval => section.removal.asking = None,
        DeveloperEvent::ConfirmRemoval => {
            if let Some(removal) = section.removal.asking.take() {
                section.removal.busy = true;
                let request = match removal {
                    Removal::Key { id, .. } => SchedulingAdminRequest::DeleteApiKey { id },
                    Removal::App { id, .. } => SchedulingAdminRequest::DeleteOauthConnection { id },
                    Removal::Webhook { id } => SchedulingAdminRequest::DeleteWebhook { id },
                };
                return vec![send(Pending::Removal, request)];
            }
        }
        DeveloperEvent::OpenCreateKey if can_change => {
            section.dialog = Some(DeveloperDialog::Key(KeyDialog::default()));
        }
        DeveloperEvent::OpenCreateWebhook if can_change => {
            section.dialog = Some(DeveloperDialog::Webhook(WebhookEditor::create()));
        }
        DeveloperEvent::OpenEditWebhook(id) if can_change => {
            let webhook = section
                .webhooks
                .as_ref()
                .and_then(SectionLoad::value)
                .and_then(|hooks| hooks.iter().find(|w| w.id == id))
                .cloned();
            if let Some(webhook) = webhook {
                section.dialog = Some(DeveloperDialog::Webhook(WebhookEditor::edit(webhook)));
            }
        }
        DeveloperEvent::CloseDialog => section.dialog = None,
        event => {
            return match &mut section.dialog {
                Some(DeveloperDialog::Key(dialog)) => key_event(dialog, event),
                Some(DeveloperDialog::Webhook(editor)) => webhook_event(editor, event),
                None => Vec::new(),
            };
        }
    }
    Vec::new()
}

fn ask(removal: &mut RemovalState, what: Removal) {
    removal.failure = None;
    removal.asking = Some(what);
}

fn key_event(dialog: &mut KeyDialog, event: DeveloperEvent) -> Vec<Out> {
    match event {
        DeveloperEvent::EditKeyName(name) => {
            dialog.name = name;
            dialog.name_rejected = None;
        }
        DeveloperEvent::CreateKey if !dialog.write.busy() => {
            let name = dialog.name.trim().to_owned();
            if name.is_empty() {
                dialog.name_rejected = Some(ss::key_name_missing(l()));
            } else {
                dialog.name_rejected = None;
                dialog.write = WriteState::Working;
                return vec![send(
                    Pending::Key,
                    SchedulingAdminRequest::CreateApiKey { name },
                )];
            }
        }
        DeveloperEvent::KeyCopied => dialog.copied = dialog.minted.is_some(),
        _ => {}
    }
    Vec::new()
}

fn webhook_event(editor: &mut WebhookEditor, event: DeveloperEvent) -> Vec<Out> {
    if editor.write.busy() {
        return Vec::new();
    }
    match event {
        DeveloperEvent::EditWebhookUrl(url) if editor.editing.is_none() => {
            editor.url = url;
            editor.url_rejected = None;
        }
        // Only an offered event can be ticked or unticked; one the webhook
        // already sends and the form does not offer stays as it is.
        DeveloperEvent::ToggleEvent(name) if WEBHOOK_EVENTS.contains(&name.as_str()) => {
            toggle(&SCHEDULING_WEBHOOK_EVENTS, &mut editor.events, &name);
            editor.events_rejected = None;
        }
        DeveloperEvent::ToggleField(name) => toggle(&WEBHOOK_FIELDS, &mut editor.fields, &name),
        DeveloperEvent::SaveWebhook => {
            editor.url_rejected = match editor.editing {
                None => url_issue(&editor.url),
                Some(_) => None,
            };
            editor.events_rejected = editor
                .events
                .is_empty()
                .then(|| ss::webhook_events_missing(l()));
            if editor.url_rejected.is_some() || editor.events_rejected.is_some() {
                return Vec::new();
            }
            editor.write = WriteState::Working;
            let request = match &editor.editing {
                None => SchedulingAdminRequest::CreateWebhook {
                    url: editor.url.trim().to_owned(),
                    events: editor.events.clone(),
                    fields: Some(editor.fields.clone()),
                },
                Some(webhook) => SchedulingAdminRequest::PatchWebhook {
                    id: webhook.id.clone(),
                    events: Some(editor.events.clone()),
                    fields: Some(editor.fields.clone()),
                },
            };
            return vec![send(Pending::Webhook, request)];
        }
        _ => {}
    }
    Vec::new()
}

pub(crate) fn answered(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    pending: Pending,
    answer: Answer,
) -> Vec<Out> {
    admin
        .developer
        .as_mut()
        .map(|section| developer_answer(section, pending, answer))
        .unwrap_or_default()
}

fn developer_answer(section: &mut DeveloperSection, pending: Pending, answer: Answer) -> Vec<Out> {
    match (pending, &mut section.dialog) {
        (Pending::Me, _) => {
            if let Some(me) = answer.ok().and_then(data!(Me)) {
                section.timezone = me.display_timezone().to_owned();
            }
        }
        (Pending::Keys, _) => section.keys = Some(load(answer, data!(ApiKeys))),
        (Pending::Apps, _) => section.apps = Some(load(answer, data!(OauthConnections))),
        (Pending::Webhooks, _) => {
            let webhooks = load(answer, data!(Webhooks));
            // A webhook deleted since its deliveries were opened closes them.
            if let (Some(listed), Some(open)) = (webhooks.value(), section.open_webhook.clone())
                && !listed.iter().any(|w| w.id == open)
            {
                section.open_webhook = None;
                section.deliveries.remove(&open);
            }
            section.webhooks = Some(webhooks);
        }
        (Pending::Deliveries(id), _) => {
            section
                .deliveries
                .insert(id, load(answer, data!(Deliveries)));
        }
        (Pending::Removal, _) => {
            section.removal.busy = false;
            match answer {
                Ok(_) => return reread(section),
                Err(failure) => section.removal.failure = Some(failure),
            }
        }
        (Pending::Key, Some(DeveloperDialog::Key(dialog))) => {
            match answer
                .and_then(|data| data!(ApiKeyCreated)(data).ok_or_else(FailureText::unexpected))
            {
                Ok(created) => {
                    dialog.write = WriteState::Idle;
                    dialog.name.clear();
                    dialog.copied = false;
                    dialog.minted = Some(MintedKey {
                        name: created.name,
                        key: SchedulingSecret::new(created.key),
                    });
                    return reread(section);
                }
                Err(failure) => dialog.write = WriteState::Failed(failure),
            }
        }
        (Pending::Webhook, Some(DeveloperDialog::Webhook(editor))) => match answer {
            Ok(data) => {
                let secret = match (&editor.editing, data) {
                    (None, SchedulingAdminData::WebhookCreated(created)) => {
                        created.secret.map(|secret| MintedSecret {
                            url: created.url,
                            secret: SchedulingSecret::new(secret),
                        })
                    }
                    _ => None,
                };
                editor.write = match (&editor.editing, &secret) {
                    (_, Some(_)) => WriteState::Idle,
                    (None, None) => WriteState::Done(ss::webhook_added(l()).to_owned()),
                    (Some(_), None) => WriteState::Done(ss::webhook_updated(l()).to_owned()),
                };
                editor.minted = secret;
                return reread(section);
            }
            Err(failure) => editor.write = WriteState::Failed(failure),
        },
        // An answer for a dialog that has closed.
        _ => {}
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_hosts_are_refused_and_public_ones_kept() {
        for host in [
            "localhost",
            "printer.local",
            "0.0.0.0",
            "[::1]",
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.4",
            "169.254.1.1",
            "172.16.0.1",
            "172.31.9.9",
        ] {
            assert!(is_private_host(host), "{host}");
        }
        for host in [
            "hooks.example.com",
            "172.15.0.1",
            "172.32.0.1",
            "172.x.0.1",
            "8.8.8.8",
        ] {
            assert!(!is_private_host(host), "{host}");
        }
    }
}
