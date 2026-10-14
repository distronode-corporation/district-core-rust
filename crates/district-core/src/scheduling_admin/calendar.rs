//! The member's own calendars: which accounts are connected, which of their
//! calendars are read for conflicts and which receives new bookings; connecting
//! Google or Microsoft through the provider's consent page in the browser, a
//! CalDAV account through a form, and disconnecting one, asked once more.
//!
//! Every write here touches only the member's own calendars, so a viewer may
//! make them too: someone who cannot connect a calendar cannot be booked.
//!
//! A Google or Microsoft connection cannot be an operation: the consent page is
//! the provider's. It starts with a single-use sign-in to the scheduler that
//! lands on the scheduler's calendar connect, opened at once in the browser and
//! kept nowhere ([`OneTimeUrl`]). The browser comes back to the web dashboard,
//! not to the app, so the section says to refresh once done, and a refresh
//! reads what is connected.

use std::collections::BTreeMap;

use district_api::ApiError;
use district_l10n::scheduling_strings as ss;
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingCalendarConnection,
    SchedulingCalendarSelection, SchedulingCalendarStatus, SchedulingSecret, SchedulingZoomStatus,
};
use url::Url;

use super::overview::load;
use super::{Answer, Out, Pending as AdminPending, SchedulingAdmin, SectionLoad, WriteState};
use crate::failure::FailureText;
use crate::model::{CoreConfig, Effect, Tickets};
use crate::scheduling::OneTimeUrl;
use crate::text::l;

/// The web dashboard's calendar page, where the provider's consent page
/// returns the browser.
pub const CALENDAR_WEB_PATH: &str = "/dashboard/district/scheduling/calendar";

/// The two providers connected through their consent page, by the scheduler's
/// names. CalDAV has a form of its own.
pub const OAUTH_PROVIDERS: [&str; 2] = ["google", "microsoft"];

/// A calendar provider's name: `Google Calendar`, or the value as it came.
pub fn provider_label(provider: &str) -> String {
    match provider {
        "google" => ss::calendar_provider_google(l()).to_owned(),
        "microsoft" => ss::calendar_provider_microsoft(l()).to_owned(),
        "caldav" => ss::calendar_provider_caldav(l()).to_owned(),
        "apple" => ss::calendar_provider_apple(l()).to_owned(),
        other => other.to_owned(),
    }
}

/// RFC 3986's unreserved characters kept, every other byte `%XX`.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Where the scheduler sign-in lands: its calendar connect for `provider`,
/// coming back to `return_to`. The return address is encoded inside, so an
/// absolute address can travel in a value the service refuses one in.
pub fn connect_next(provider: &str, return_to: &str) -> String {
    format!(
        "/v1/calendar/connect?provider={}&return_to={}",
        encode(provider),
        encode(return_to)
    )
}

/// One connected account, as the section shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionCard {
    /// Its id, for the actions.
    pub id: String,
    /// Its provider's name, the card's heading.
    pub provider: String,
    /// Its address.
    pub account: String,
    /// Which of its calendars are read for conflicts: their names, "None", or,
    /// while its calendars are not read, "Checked" or "Not checked".
    pub conflicts: String,
    /// Which receives new bookings: a calendar's name, "This account", or "No
    /// calendar in this account".
    pub destination: String,
}

/// Connecting Google or Microsoft: the sign-in on its way, and what came of
/// the last.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectState {
    /// Whether the sign-in is being asked for. One at a time.
    pub busy: bool,
    /// Why the last one failed.
    pub failure: Option<FailureText>,
    /// The provider whose consent page was opened in the browser: until the
    /// next read, the section says to refresh once done there.
    pub opened: Option<String>,
}

/// Disconnecting an account, asked once more.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisconnectState {
    /// The account asked about, by connection id.
    pub asking: Option<String>,
    /// Whether the disconnect is on its way.
    pub busy: bool,
    /// Why the last one failed.
    pub failure: Option<FailureText>,
    /// Whether the account disconnected was the one receiving bookings, so
    /// none does now.
    pub removed_destination: bool,
}

/// The calendars section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CalendarSection {
    /// The connections.
    pub status: SectionLoad<SchedulingCalendarStatus>,
    /// Zoom, when it could be read. Its failure is silent: the connections are
    /// the point.
    pub zoom: Option<SchedulingZoomStatus>,
    /// Each connection's calendars, by connection id: only those read.
    pub calendars: BTreeMap<String, Vec<SchedulingCalendarSelection>>,
    /// Connecting Google or Microsoft.
    pub connect: ConnectState,
    /// Disconnecting.
    pub disconnect: DisconnectState,
    /// The dialog open over the section.
    pub dialog: Option<CalendarDialog>,
}

impl CalendarSection {
    /// The connected accounts.
    pub fn connections(&self) -> Vec<ConnectionCard> {
        let Some(status) = self.status.value() else {
            return Vec::new();
        };
        status
            .connections
            .iter()
            .map(|c| {
                let calendars = self.calendars.get(&c.id);
                ConnectionCard {
                    id: c.id.clone(),
                    provider: provider_label(&c.provider),
                    account: c.account_email.clone(),
                    conflicts: conflict_summary(c, calendars),
                    destination: destination_summary(c, calendars),
                }
            })
            .collect()
    }

    /// The connect card's sentence: none set up on this scheduler, none
    /// connected yet, or another can be added. `None` until read.
    pub fn sentence(&self) -> Option<&'static str> {
        let status = self.status.value()?;
        Some(if !status.configured {
            ss::calendar_not_configured(l())
        } else if status.connections.is_empty() {
            ss::calendar_none_connected(l())
        } else {
            ss::calendar_connect_another(l())
        })
    }

    /// Zoom's line, as (label, value), when the scheduler can connect it.
    pub fn zoom_line(&self) -> Option<(&'static str, &'static str)> {
        let zoom = self.zoom.filter(|z| z.configured)?;
        Some((
            ss::calendar_zoom(l()),
            if zoom.connected {
                ss::calendar_connected(l())
            } else {
                ss::calendar_not_connected(l())
            },
        ))
    }

    /// The providers to offer a Connect button for, as (provider, label):
    /// those the scheduler has credentials for. None when it can connect none,
    /// and then [`connect_note`](Self::connect_note) says so instead.
    pub fn offered_providers(&self) -> Vec<(&'static str, String)> {
        let Some(status) = self.status.value().filter(|s| s.configured) else {
            return Vec::new();
        };
        let missing = status.unconfigured_providers();
        OAUTH_PROVIDERS
            .into_iter()
            .filter(|p| !missing.iter().any(|m| m == p))
            .map(|p| (p, ss::connect_provider_named(l(), &provider_label(p))))
            .collect()
    }

    /// Under the Connect buttons: finish in the browser and refresh, or that no
    /// provider can be connected here.
    pub fn connect_note(&self) -> &'static str {
        if self.offered_providers().is_empty() {
            ss::connect_not_configured(l())
        } else {
            ss::connect_hand_off_note(l())
        }
    }

    /// The labels: the cards' rows (account, conflicts, receives bookings), the
    /// connect card's heading, "Choose calendars", "Disconnect", the
    /// disconnect's confirmation and its way out, and the sentence after the
    /// account receiving bookings went.
    pub fn labels() -> [&'static str; 9] {
        [
            ss::calendar_account(l()),
            ss::calendar_conflicts(l()),
            ss::calendar_destination(l()),
            ss::calendar_connect_eyebrow(l()),
            ss::calendars_open(l()),
            ss::disconnect_action(l()),
            ss::disconnect_confirm(l()),
            ss::disconnect_keep(l()),
            ss::disconnected_last_destination(l()),
        ]
    }

    /// The disconnect's question, naming the account.
    pub fn disconnect_prompt(&self) -> Option<String> {
        let id = self.disconnect.asking.as_ref()?;
        let connection = self.connection(id)?;
        Some(ss::disconnect_prompt(l(), &connection.account_email))
    }

    fn connection(&self, id: &str) -> Option<&SchedulingCalendarConnection> {
        self.status.value()?.connections.iter().find(|c| c.id == id)
    }
}

fn conflict_summary(
    connection: &SchedulingCalendarConnection,
    calendars: Option<&Vec<SchedulingCalendarSelection>>,
) -> String {
    match calendars {
        None if connection.check_conflicts => ss::calendar_conflicts_checked(l()).to_owned(),
        None => ss::calendar_conflicts_not_checked(l()).to_owned(),
        Some(calendars) => {
            let names: Vec<&str> = calendars
                .iter()
                .filter(|c| c.check_conflicts == Some(true))
                .map(|c| c.name.as_str())
                .collect();
            if names.is_empty() {
                ss::calendar_conflicts_none(l()).to_owned()
            } else {
                names.join(", ")
            }
        }
    }
}

fn destination_summary(
    connection: &SchedulingCalendarConnection,
    calendars: Option<&Vec<SchedulingCalendarSelection>>,
) -> String {
    match calendars.and_then(|c| c.iter().find(|c| c.is_destination == Some(true))) {
        Some(chosen) => chosen.name.clone(),
        None if connection.is_destination => ss::calendar_destination_this_account(l()).to_owned(),
        None => ss::calendar_destination_none(l()).to_owned(),
    }
}

/// A dialog over the calendars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalendarDialog {
    /// One account's calendars.
    Calendars(CalendarPicker),
    /// The CalDAV form.
    Caldav(CaldavDialog),
}

/// One account's calendars: which are read for conflicts, and which receives
/// new bookings. Saved whole, as read with the edits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarPicker {
    /// The account.
    pub connection: SchedulingCalendarConnection,
    /// Its calendars.
    pub rows: SectionLoad<Vec<SchedulingCalendarSelection>>,
    /// The save's state.
    pub write: WriteState,
}

impl CalendarPicker {
    /// "Calendars in ada@example.com".
    pub fn title(&self) -> String {
        ss::calendars_title(l(), &self.connection.account_email)
    }

    /// The calendars that can receive bookings: every one not read-only.
    pub fn destination_choices(&self) -> Vec<&SchedulingCalendarSelection> {
        self.rows
            .value()
            .map(|rows| rows.iter().filter(|c| c.writable != Some(false)).collect())
            .unwrap_or_default()
    }

    /// The calendar receiving bookings now, by id.
    pub fn destination(&self) -> Option<&str> {
        self.rows
            .value()?
            .iter()
            .find(|c| c.is_destination == Some(true))
            .map(|c| c.id.as_str())
    }

    /// Whether the save can be pressed: read, and nothing on its way.
    pub fn can_save(&self) -> bool {
        self.rows.value().is_some() && !self.write.busy()
    }

    /// The labels: conflicts and its hint, receives bookings and its hint,
    /// loading, save, saving, cancel.
    pub fn labels() -> [&'static str; 8] {
        [
            ss::calendars_conflicts_label(l()),
            ss::calendars_conflicts_hint(l()),
            ss::calendars_destination_label(l()),
            ss::calendars_destination_hint(l()),
            ss::calendars_loading(l()),
            ss::calendars_save_action(l()),
            ss::calendars_saving(l()),
            ss::common_cancel(l()),
        ]
    }
}

/// Where a CalDAV calendar lives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CaldavPreset {
    /// iCloud.
    #[default]
    Icloud,
    /// Fastmail.
    Fastmail,
    /// A Nextcloud server, at its address.
    Nextcloud,
    /// Any other server, at its address.
    Custom,
}

impl CaldavPreset {
    /// Every preset, in the form's order.
    pub const ALL: [Self; 4] = [Self::Icloud, Self::Fastmail, Self::Nextcloud, Self::Custom];

    /// Whether it needs the server's address.
    pub fn needs_server_url(self) -> bool {
        matches!(self, Self::Nextcloud | Self::Custom)
    }

    /// The name the scheduler knows it by.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Icloud => "icloud",
            Self::Fastmail => "fastmail",
            Self::Nextcloud => "nextcloud",
            Self::Custom => "custom",
        }
    }

    /// Its label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Icloud => ss::caldav_preset_icloud(l()),
            Self::Fastmail => ss::caldav_preset_fastmail(l()),
            Self::Nextcloud => ss::caldav_preset_nextcloud(l()),
            Self::Custom => ss::caldav_preset_custom(l()),
        }
    }

    /// Where to get an app password.
    pub fn note(self) -> &'static str {
        match self {
            Self::Icloud => ss::caldav_preset_icloud_note(l()),
            Self::Fastmail => ss::caldav_preset_fastmail_note(l()),
            Self::Nextcloud => ss::caldav_preset_nextcloud_note(l()),
            Self::Custom => ss::caldav_preset_custom_note(l()),
        }
    }
}

/// The CalDAV form's fields. The app password is redacted in `Debug`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaldavForm {
    /// Where the calendar lives.
    pub preset: CaldavPreset,
    /// The server's address, for a preset that needs it.
    pub server_url: String,
    /// The account's user name.
    pub username: String,
    /// Its app password, not the account password.
    pub app_password: SchedulingSecret,
}

/// Which CalDAV field a refusal is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaldavField {
    /// The server's address.
    ServerUrl,
    /// The user name.
    Username,
    /// The app password.
    AppPassword,
}

/// Why a CalDAV server address cannot be used: not an address, not https, or
/// a numeric host (the service refuses one, as a way into private networks).
fn server_url_issue(value: &str) -> Option<&'static str> {
    let Some(url) = Url::parse(value)
        .ok()
        .filter(|u| u.host_str().is_some_and(|h| !h.is_empty()))
    else {
        return Some(ss::caldav_server_url_not_a_url(l()));
    };
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let numeric = host.starts_with('[')
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.chars().all(|c| c.is_ascii_digit() || c == '.')
        || host
            .strip_prefix("0x")
            .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_hexdigit()));
    if url.scheme() != "https" {
        Some(ss::caldav_server_url_not_https(l()))
    } else if numeric {
        Some(ss::caldav_server_url_is_ip(l()))
    } else {
        None
    }
}

/// The CalDAV form.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaldavDialog {
    /// The fields.
    pub form: CaldavForm,
    /// The field refused before anything was sent, and why.
    pub refused: Option<(CaldavField, &'static str)>,
    /// The connect's state.
    pub write: WriteState,
    /// The address the server connected, once it has: the server's, not the
    /// user name sent.
    pub connected: Option<String>,
}

impl CaldavDialog {
    /// "Calendar connected: ada@example.com", once connected.
    pub fn connected_line(&self) -> Option<String> {
        self.connected
            .as_ref()
            .map(|account| ss::caldav_connected_to_line(l(), account))
    }

    /// The request, or `None` with the field refused.
    fn submit(&mut self) -> Option<SchedulingAdminRequest> {
        let form = &self.form;
        let server = form.server_url.trim();
        let username = form.username.trim();
        let needs_server = form.preset.needs_server_url();
        self.refused = if needs_server && server.is_empty() {
            Some((CaldavField::ServerUrl, ss::caldav_server_url_missing(l())))
        } else if let Some(issue) = server_url_issue(server).filter(|_| needs_server) {
            Some((CaldavField::ServerUrl, issue))
        } else if username.is_empty() {
            Some((CaldavField::Username, ss::caldav_username_missing(l())))
        } else if form.app_password.expose().is_empty() {
            Some((CaldavField::AppPassword, ss::caldav_password_missing(l())))
        } else {
            None
        };
        if self.refused.is_some() || self.write.busy() {
            return None;
        }
        self.write = WriteState::Working;
        self.connected = None;
        Some(SchedulingAdminRequest::ConnectCaldav {
            preset: (!needs_server).then(|| form.preset.wire().to_owned()),
            server_url: needs_server.then(|| server.to_owned()),
            username: username.to_owned(),
            app_password: form.app_password.clone(),
        })
    }

    /// The labels: the title, the preset's, the server's and its hint, the
    /// user name's, the password's and its hint, connect, connecting, cancel.
    pub fn labels() -> [&'static str; 10] {
        [
            ss::caldav_title(l()),
            ss::caldav_preset_label(l()),
            ss::caldav_server_url_label(l()),
            ss::caldav_server_url_hint(l()),
            ss::caldav_username_label(l()),
            ss::caldav_password_label(l()),
            ss::caldav_password_hint(l()),
            ss::caldav_connect_action(l()),
            ss::caldav_connecting(l()),
            ss::common_cancel(l()),
        ]
    }
}

/// What the member does on the calendars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalendarEvent {
    /// Connect Google or Microsoft through its consent page in the browser.
    Connect(String),
    /// Put away the connect's failure.
    DismissConnectFailure,
    /// Ask about disconnecting an account, by connection id.
    AskDisconnect(String),
    /// Do not disconnect it.
    CancelDisconnect,
    /// Disconnect it.
    ConfirmDisconnect,
    /// Put away the disconnect's outcome.
    DismissDisconnect,
    /// Open an account's calendars, by connection id.
    OpenCalendars(String),
    /// A calendar is read for conflicts, or not.
    SetCheckConflicts {
        /// The calendar's id.
        calendar_id: String,
        /// Whether.
        on: bool,
    },
    /// A calendar receives new bookings, and no other does.
    ChooseDestination(String),
    /// Save the calendars.
    SaveCalendars,
    /// Open the CalDAV form.
    OpenCaldav,
    /// The CalDAV form's fields changed.
    EditCaldav(CaldavForm),
    /// Connect the CalDAV account.
    ConnectCaldav,
    /// Close the dialog.
    CloseDialog,
}

/// What an answer the calendars await is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    Status,
    Zoom,
    Calendars(String),
    SignIn,
    Disconnect { was_destination: bool },
    PickerRows,
    PickerSave { destination: bool },
    PickerDestination,
    Caldav,
}

fn send(pending: Pending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::Calendar(pending), Box::new(request))
}

fn reads() -> Vec<Out> {
    vec![
        send(Pending::Status, SchedulingAdminRequest::CalendarStatus),
        send(Pending::Zoom, SchedulingAdminRequest::ZoomStatus),
    ]
}

pub(crate) fn enter(admin: &mut SchedulingAdmin, tickets: &mut Tickets) -> Vec<Out> {
    admin.forget(tickets, |p| matches!(p, AdminPending::Calendar(_)));
    admin.calendar = Some(CalendarSection::default());
    reads()
}

/// The section read again, its dialog and outcomes kept.
fn reread(section: &mut CalendarSection) -> Vec<Out> {
    section.status = SectionLoad::Loading;
    section.calendars.clear();
    section.connect.opened = None;
    reads()
}

fn calendars_request(connection: &SchedulingCalendarConnection) -> SchedulingAdminRequest {
    SchedulingAdminRequest::Calendars {
        id: connection.id.clone(),
        provider: connection.provider.clone(),
        account: Some(connection.account_email.clone()),
    }
}

pub(crate) fn event(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: CalendarEvent,
    config: &CoreConfig,
) -> Vec<Out> {
    let Some(section) = admin.calendar.as_mut() else {
        return Vec::new();
    };
    match event {
        CalendarEvent::Connect(provider) => {
            let offered = section
                .offered_providers()
                .iter()
                .any(|(p, _)| *p == provider);
            if !offered || section.connect.busy {
                return Vec::new();
            }
            section.connect.busy = true;
            section.connect.failure = None;
            let next = connect_next(&provider, &config.web_url(CALENDAR_WEB_PATH));
            section.connect.opened = Some(provider);
            return vec![Out::SignIn(AdminPending::Calendar(Pending::SignIn), next)];
        }
        CalendarEvent::DismissConnectFailure => section.connect.failure = None,
        CalendarEvent::AskDisconnect(id) if !section.disconnect.busy => {
            if section.connection(&id).is_some() {
                section.disconnect.asking = Some(id);
                section.disconnect.failure = None;
                section.disconnect.removed_destination = false;
            }
        }
        CalendarEvent::CancelDisconnect => section.disconnect.asking = None,
        CalendarEvent::ConfirmDisconnect if !section.disconnect.busy => {
            let connection = section
                .disconnect
                .asking
                .take()
                .and_then(|id| section.connection(&id).cloned());
            if let Some(connection) = connection {
                section.disconnect.busy = true;
                return vec![send(
                    Pending::Disconnect {
                        was_destination: connection.is_destination,
                    },
                    SchedulingAdminRequest::DeleteConnection {
                        id: connection.id,
                        provider: connection.provider,
                        account: Some(connection.account_email),
                    },
                )];
            }
        }
        CalendarEvent::DismissDisconnect => {
            section.disconnect.failure = None;
            section.disconnect.removed_destination = false;
        }
        CalendarEvent::OpenCalendars(id) => {
            if let Some(connection) = section.connection(&id).cloned() {
                let request = calendars_request(&connection);
                section.dialog = Some(CalendarDialog::Calendars(CalendarPicker {
                    connection,
                    rows: SectionLoad::Loading,
                    write: WriteState::Idle,
                }));
                return vec![send(Pending::PickerRows, request)];
            }
        }
        CalendarEvent::OpenCaldav => {
            section.dialog = Some(CalendarDialog::Caldav(CaldavDialog::default()));
        }
        CalendarEvent::CloseDialog => section.dialog = None,
        event => return dialog_event(section.dialog.as_mut(), event),
    }
    Vec::new()
}

fn dialog_event(dialog: Option<&mut CalendarDialog>, event: CalendarEvent) -> Vec<Out> {
    match (dialog, event) {
        (
            Some(CalendarDialog::Calendars(picker)),
            CalendarEvent::SetCheckConflicts { calendar_id, on },
        ) => {
            if let SectionLoad::Ready(rows) = &mut picker.rows {
                for row in rows.iter_mut().filter(|r| r.id == calendar_id) {
                    row.check_conflicts = Some(on);
                }
            }
        }
        (Some(CalendarDialog::Calendars(picker)), CalendarEvent::ChooseDestination(id)) => {
            if let SectionLoad::Ready(rows) = &mut picker.rows {
                for row in rows.iter_mut() {
                    row.is_destination = Some(row.id == id);
                }
            }
        }
        (Some(CalendarDialog::Calendars(picker)), CalendarEvent::SaveCalendars)
            if picker.can_save() =>
        {
            let calendars = picker.rows.value().cloned().unwrap_or_default();
            let destination = calendars.iter().any(|c| c.is_destination == Some(true));
            picker.write = WriteState::Working;
            let c = &picker.connection;
            return vec![send(
                Pending::PickerSave { destination },
                SchedulingAdminRequest::SetCalendars {
                    id: c.id.clone(),
                    provider: c.provider.clone(),
                    account_email: Some(c.account_email.clone()),
                    calendars,
                },
            )];
        }
        (Some(CalendarDialog::Caldav(caldav)), CalendarEvent::EditCaldav(form)) => {
            caldav.form = form;
            caldav.refused = None;
        }
        (Some(CalendarDialog::Caldav(caldav)), CalendarEvent::ConnectCaldav) => {
            if let Some(request) = caldav.submit() {
                return vec![send(Pending::Caldav, request)];
            }
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
        .calendar
        .as_mut()
        .map(|section| calendar_answer(section, pending, answer))
        .unwrap_or_default()
}

fn calendar_answer(section: &mut CalendarSection, pending: Pending, answer: Answer) -> Vec<Out> {
    match (pending, &mut section.dialog) {
        // The connections, then each one's calendars.
        (Pending::Status, _) => {
            section.status = load(answer, data!(CalendarStatus));
            if let Some(status) = section.status.value() {
                return status
                    .connections
                    .iter()
                    .map(|c| send(Pending::Calendars(c.id.clone()), calendars_request(c)))
                    .collect();
            }
        }
        (Pending::Zoom, _) => section.zoom = answer.ok().and_then(data!(ZoomStatus)),
        // Only a read that landed is kept: "could not read" is not "none
        // chosen".
        (Pending::Calendars(id), _) => {
            if let Ok(SchedulingAdminData::Calendars(rows)) = answer {
                section.calendars.insert(id, rows);
            }
        }
        (Pending::Disconnect { was_destination }, _) => {
            section.disconnect.busy = false;
            match answer {
                Ok(_) => {
                    section.disconnect.removed_destination = was_destination;
                    return reread(section);
                }
                Err(failure) => section.disconnect.failure = Some(failure),
            }
        }
        (Pending::PickerRows, Some(CalendarDialog::Calendars(picker))) => {
            picker.rows = load(answer, data!(Calendars));
        }
        // The calendars saved: the account receiving bookings next, when one of
        // its calendars was chosen, then the section read again.
        (Pending::PickerSave { destination }, Some(CalendarDialog::Calendars(picker))) => {
            match answer {
                Ok(_) if destination => {
                    let c = &picker.connection;
                    return vec![send(
                        Pending::PickerDestination,
                        SchedulingAdminRequest::SetDestination {
                            id: c.id.clone(),
                            provider: c.provider.clone(),
                            account_email: Some(c.account_email.clone()),
                        },
                    )];
                }
                Ok(_) => return saved(section),
                Err(failure) => picker.write = WriteState::Failed(failure),
            }
        }
        (Pending::PickerDestination, Some(CalendarDialog::Calendars(picker))) => match answer {
            Ok(_) => return saved(section),
            Err(failure) => picker.write = WriteState::Failed(failure),
        },
        // A CalDAV account connected: the password goes, and the section is
        // read again. Refused without a reason the five sentences give, it
        // says what to check.
        (Pending::Caldav, Some(CalendarDialog::Caldav(caldav))) => match answer {
            Ok(data) => {
                caldav.connected = data!(CaldavConnected)(data).map(|c| c.account_email);
                caldav.form = CaldavForm {
                    preset: caldav.form.preset,
                    ..CaldavForm::default()
                };
                caldav.write = WriteState::Done(ss::caldav_connected(l()).to_owned());
                return reread(section);
            }
            Err(failure) => {
                let unknown = failure.message == ss::failure_unknown(l());
                caldav.write = WriteState::Failed(if unknown {
                    FailureText::final_(ss::caldav_refused(l()))
                } else {
                    failure
                });
            }
        },
        // An answer for a dialog that has closed.
        _ => {}
    }
    Vec::new()
}

fn saved(section: &mut CalendarSection) -> Vec<Out> {
    if let Some(CalendarDialog::Calendars(picker)) = &mut section.dialog {
        picker.write = WriteState::Done(ss::calendars_saved(l()).to_owned());
    }
    reread(section)
}

/// The scheduler sign-in arrived: opened at once in the browser, if it is an
/// https link, and kept nowhere.
pub(crate) fn signed_in(
    admin: &mut SchedulingAdmin,
    _pending: super::Pending,
    result: Result<SchedulingSecret, ApiError>,
) -> Vec<Out> {
    // The sign-in is forgotten when the section closes.
    admin
        .calendar
        .as_mut()
        .map(|section| opened(section, result))
        .unwrap_or_default()
}

fn opened(section: &mut CalendarSection, result: Result<SchedulingSecret, ApiError>) -> Vec<Out> {
    section.connect.busy = false;
    let failure = match result {
        Ok(link)
            if link
                .expose()
                .get(..8)
                .is_some_and(|s| s.eq_ignore_ascii_case("https://")) =>
        {
            return vec![Out::Effect(Effect::OpenOneTimeUrl {
                url: OneTimeUrl::new(link.expose()),
            })];
        }
        Ok(_) => FailureText::unexpected(),
        // The booking pages are not ready: not a fault, and not the member's.
        Err(ApiError::Conflict(_)) => FailureText::final_(ss::connect_not_ready_yet(l())),
        Err(error) => FailureText::from_api_error(&error),
    };
    section.connect.opened = None;
    section.connect.failure = Some(failure);
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_return_address_travels_encoded_inside_next() {
        assert_eq!(
            connect_next("google", "https://www.example.com/dashboard/x?a=b c"),
            "/v1/calendar/connect?provider=google&return_to=https%3A%2F%2Fwww.example.com%2Fdashboard%2Fx%3Fa%3Db%20c"
        );
    }

    #[test]
    fn a_server_address_must_be_https_with_a_name() {
        let issue = |v: &str| server_url_issue(v);
        assert_eq!(issue("https://dav.example.com/remote.php/dav"), None);
        for numeric in [
            "https://192.168.1.4/dav",
            "https://[::1]/dav",
            "https://localhost/dav",
            "https://a.localhost/dav",
            "https://0x7f000001/dav",
        ] {
            assert_eq!(
                issue(numeric),
                Some(ss::caldav_server_url_is_ip(l())),
                "{numeric}"
            );
        }
        assert_eq!(
            issue("http://dav.example.com"),
            Some(ss::caldav_server_url_not_https(l()))
        );
        for bad in ["dav.example.com", "mailto:a@example.com", ""] {
            assert_eq!(
                issue(bad),
                Some(ss::caldav_server_url_not_a_url(l())),
                "{bad}"
            );
        }
    }
}
