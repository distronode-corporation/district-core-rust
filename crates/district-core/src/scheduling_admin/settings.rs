//! Settings: the booking page's branding and images, the booking assistant,
//! the member's own scheduler profile and picture, and their email
//! notifications.
//!
//! The profile and the notifications are the member's own, so every member
//! may change them. The booking page and the assistant are the workspace's,
//! and need a member who may change it. Images save as soon as they are
//! chosen or removed; everything else saves on its button.

use district_l10n::scheduling_strings as ss;
use district_model::{
    SchedulingAdminRequest, SchedulingBranding, SchedulingBrandingPatch, SchedulingLlmSettings,
    SchedulingLocaleOption, SchedulingMe, SchedulingProfilePatch, SchedulingUpload,
    SchedulingUploadTarget,
};
use url::Url;

use super::hours::weekday_name;
use super::overview::load;
use super::{Answer, Out, Pending as AdminPending, SchedulingAdmin, SectionLoad, WriteState};
use crate::failure::FailureText;
use crate::model::Tickets;
use crate::text::l;

/// The longest name, business name, in characters.
pub const SETTINGS_NAME_MAX: usize = 200;
/// The longest privacy or terms link, in characters.
pub const LEGAL_URL_MAX: usize = 500;
/// The longest extra instructions for the booking assistant, in characters.
pub const ASSISTANT_INSTRUCTIONS_MAX: usize = 4000;
/// The logo's height, in pixels.
pub const LOGO_HEIGHTS: std::ops::RangeInclusive<i64> = 16..=64;
/// An opacity, in percent.
pub const OPACITIES: std::ops::RangeInclusive<i64> = 20..=100;
/// The largest image the scheduler takes, in bytes.
pub const IMAGE_MAX_BYTES: usize = 5 * 1024 * 1024;
/// The image types the scheduler takes.
pub const IMAGE_TYPES: [&str; 4] = ["image/jpeg", "image/png", "image/gif", "image/webp"];
/// The time formats, as the scheduler stores them.
pub const TIME_FORMATS: [&str; 2] = ["12h", "24h"];
/// The date formats, as the scheduler stores them.
pub const DATE_FORMATS: [&str; 3] = ["dmy", "mdy", "ymd"];
/// The notification switches, as the scheduler names them: the attendee's
/// four, then the host's three.
pub const NOTIFICATIONS: [&str; 7] = [
    "notify_confirmation",
    "notify_cancellation",
    "notify_reschedule",
    "notify_reminder",
    "notify_host_booking",
    "notify_host_cancel",
    "notify_host_reschedule",
];

/// "12-hour (2:30 pm)", or the value as it came.
pub fn time_format_label(value: &str) -> String {
    match value {
        "12h" => ss::settings_time_format_12h(l()).to_owned(),
        "24h" => ss::settings_time_format_24h(l()).to_owned(),
        other => other.to_owned(),
    }
}

/// "31/12/2026", or the value as it came.
pub fn date_format_label(value: &str) -> String {
    match value {
        "dmy" => ss::settings_date_format_dmy(l()).to_owned(),
        "mdy" => ss::settings_date_format_mdy(l()).to_owned(),
        "ymd" => ss::settings_date_format_ymd(l()).to_owned(),
        other => other.to_owned(),
    }
}

/// The day a week starts on, 0 for Sunday, or the number as it came.
pub fn week_start_label(value: i64) -> String {
    match usize::try_from(value).ok().filter(|v| *v < 7) {
        // The scheduler counts from Sunday, the names from Monday.
        Some(day) => weekday_name((day + 6) % 7).to_owned(),
        None => value.to_string(),
    }
}

/// The booking page's language by name, its code when the scheduler did not
/// name it, or "Not set".
pub fn locale_label(code: &str, supported: Option<&[SchedulingLocaleOption]>) -> String {
    let code = code.trim();
    if code.is_empty() {
        return ss::settings_locale_not_set(l()).to_owned();
    }
    supported
        .unwrap_or_default()
        .iter()
        .find(|o| o.code == code && !o.name.is_empty())
        .map_or_else(|| code.to_owned(), |o| o.name.clone())
}

/// A notification switch's label, or the field as it came.
pub fn notification_label(field: &str) -> &str {
    match field {
        "notify_confirmation" => ss::settings_notify_confirmation(l()),
        "notify_cancellation" => ss::settings_notify_cancellation(l()),
        "notify_reschedule" => ss::settings_notify_reschedule(l()),
        "notify_reminder" => ss::settings_notify_reminder(l()),
        "notify_host_booking" => ss::settings_notify_host_booking(l()),
        "notify_host_cancel" => ss::settings_notify_host_cancel(l()),
        "notify_host_reschedule" => ss::settings_notify_host_reschedule(l()),
        other => other,
    }
}

/// The notifications in two groups, as (heading, description, switches): as
/// an attendee, then as a host.
pub fn notification_groups() -> [(&'static str, &'static str, &'static [&'static str]); 2] {
    [
        (
            ss::settings_notify_attendee_heading(l()),
            ss::settings_notify_attendee_description(l()),
            &NOTIFICATIONS[..4],
        ),
        (
            ss::settings_notify_host_heading(l()),
            ss::settings_notify_host_description(l()),
            &NOTIFICATIONS[4..],
        ),
    ]
}

/// Whether `field` is on in the member's profile.
pub fn notification_value(field: &str, me: &SchedulingMe) -> bool {
    notification_values(me)
        .into_iter()
        .find(|(f, _)| *f == field)
        .is_some_and(|(_, on)| on)
}

fn notification_values(me: &SchedulingMe) -> Vec<(&'static str, bool)> {
    NOTIFICATIONS
        .into_iter()
        .zip([
            me.notify_confirmation,
            me.notify_cancellation,
            me.notify_reschedule,
            me.notify_reminder,
            me.notify_host_booking,
            me.notify_host_cancel,
            me.notify_host_reschedule,
        ])
        .collect()
}

/// "Set" or "Not set", for an image.
pub fn set_label(url: &str) -> &'static str {
    if url.is_empty() {
        ss::settings_not_set(l())
    } else {
        ss::settings_set(l())
    }
}

/// The section's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SettingsTab {
    /// The booking page's branding.
    #[default]
    BookingPage,
    /// The booking assistant.
    Assistant,
    /// The member's profile.
    Profile,
    /// The member's notifications.
    Notifications,
}

impl SettingsTab {
    /// The tabs in order.
    pub const ALL: [Self; 4] = [
        Self::BookingPage,
        Self::Assistant,
        Self::Profile,
        Self::Notifications,
    ];

    /// Its label.
    pub fn label(self) -> &'static str {
        match self {
            Self::BookingPage => ss::settings_tab_booking_page(l()),
            Self::Assistant => ss::settings_tab_assistant(l()),
            Self::Profile => ss::settings_tab_profile(l()),
            Self::Notifications => ss::settings_tab_notifications(l()),
        }
    }
}

/// An image, as the device read it.
#[derive(Clone, PartialEq, Eq)]
pub struct ImageFile {
    /// Its type, `image/png` and the like.
    pub mime_type: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for ImageFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageFile")
            .field("mime_type", &self.mime_type)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

/// Why `file` cannot be sent: unreadable, too large, or not a type the
/// scheduler takes. Otherwise the upload for `target`.
fn upload(
    target: SchedulingUploadTarget,
    file: ImageFile,
) -> Result<SchedulingUpload, &'static str> {
    if file.bytes.is_empty() {
        return Err(ss::settings_image_unreadable(l()));
    }
    if file.bytes.len() > IMAGE_MAX_BYTES {
        return Err(ss::settings_image_too_large(l()));
    }
    let extension = match file.mime_type.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return Err(ss::settings_image_wrong_type(l())),
    };
    Ok(SchedulingUpload {
        target,
        file_name: format!("{}.{extension}", target.as_str()),
        mime_type: file.mime_type,
        bytes: file.bytes,
    })
}

/// An image's name: "Logo", "Banner", "Profile picture".
fn image_label(target: SchedulingUploadTarget) -> &'static str {
    match target {
        SchedulingUploadTarget::Logo => ss::settings_branding_logo(l()),
        SchedulingUploadTarget::Banner => ss::settings_branding_banner(l()),
        SchedulingUploadTarget::Avatar => ss::settings_avatar_label(l()),
    }
}

/// "Logo uploaded", "Banner removed".
fn done(target: SchedulingUploadTarget, uploaded: bool) -> String {
    if uploaded {
        ss::settings_image_uploaded(l(), image_label(target))
    } else {
        ss::settings_image_removed(l(), image_label(target))
    }
}

fn unreadable() -> WriteState {
    WriteState::Failed(FailureText::final_(ss::settings_image_unreadable(l())))
}

/// The profile's fields, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileForm {
    /// The name.
    pub name: String,
    /// The IANA time zone.
    pub timezone: String,
    /// `12h` or `24h`.
    pub time_format: String,
    /// The day the week starts on, 0 for Sunday.
    pub week_start: i64,
    /// `dmy`, `mdy` or `ymd`.
    pub date_format: String,
}

impl ProfileForm {
    fn of(me: &SchedulingMe) -> Self {
        Self {
            name: me.name.clone(),
            timezone: me.timezone.clone(),
            time_format: me.time_format.clone(),
            week_start: me.week_start,
            date_format: me.date_format.clone(),
        }
    }

    fn issue(&self) -> Option<&'static str> {
        let name = self.name.trim();
        if name.is_empty() {
            Some(ss::settings_name_required(l()))
        } else if name.chars().count() > SETTINGS_NAME_MAX {
            Some(ss::settings_name_too_long(l()))
        } else if self.timezone.trim().is_empty() {
            Some(ss::settings_timezone_required(l()))
        } else if !TIME_FORMATS.contains(&self.time_format.as_str()) {
            Some(ss::settings_time_format_required(l()))
        } else if !(0..7).contains(&self.week_start) {
            Some(ss::settings_week_start_required(l()))
        } else if !DATE_FORMATS.contains(&self.date_format.as_str()) {
            Some(ss::settings_date_format_required(l()))
        } else {
            None
        }
    }
}

/// Editing the member's profile and picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileEditor {
    /// The profile as last read.
    pub me: SchedulingMe,
    /// The fields.
    pub form: ProfileForm,
    /// Why the fields cannot be sent.
    pub rejected: Option<&'static str>,
    /// The save's state.
    pub write: WriteState,
    /// The picture's upload or removal.
    pub avatar: WriteState,
    /// The picture just uploaded, until the profile is read again.
    pub avatar_override: Option<String>,
}

impl ProfileEditor {
    /// Whether a save or the picture is on its way.
    pub fn busy(&self) -> bool {
        self.write.busy() || self.avatar.busy()
    }

    /// The picture's address, if there is one.
    pub fn avatar_url(&self) -> Option<&str> {
        self.avatar_override
            .as_deref()
            .or(self.me.avatar_url.as_deref())
            .filter(|u| !u.is_empty())
    }

    /// Every time zone, with the member's first when it is not one of them.
    pub fn timezone_options(&self) -> Vec<String> {
        let mut zones: Vec<String> = jiff::tz::db()
            .available()
            .map(|name| name.to_string())
            .collect();
        zones.sort();
        let current = self.form.timezone.trim();
        if !current.is_empty() && !zones.iter().any(|z| z == current) {
            zones.insert(0, current.to_owned());
        }
        zones
    }

    /// The labels: the title, the five fields' labels, the picture's label,
    /// its hint and buttons, and the save button.
    pub fn labels() -> [&'static str; 11] {
        [
            ss::settings_profile_title(l()),
            ss::settings_profile_name(l()),
            ss::settings_profile_timezone(l()),
            ss::settings_profile_time_format(l()),
            ss::settings_profile_week_start(l()),
            ss::settings_profile_date_format(l()),
            ss::settings_avatar_label(l()),
            ss::settings_image_hint(l()),
            ss::settings_image_choose(l()),
            ss::settings_image_remove(l()),
            ss::settings_profile_save(l()),
        ]
    }
}

/// Editing the member's notifications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotificationsEditor {
    /// Each switch, in [`NOTIFICATIONS`]'s order.
    pub values: Vec<(&'static str, bool)>,
    /// The save's state.
    pub write: WriteState,
}

impl NotificationsEditor {
    /// Whether `field` is switched on.
    pub fn value(&self, field: &str) -> bool {
        self.values.iter().any(|(f, on)| *f == field && *on)
    }

    /// The labels: the title and the save button.
    pub fn labels() -> [&'static str; 2] {
        [
            ss::settings_notifications_title(l()),
            ss::settings_notifications_save(l()),
        ]
    }
}

/// The booking page's fields, as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrandingForm {
    /// The business name.
    pub business_name: String,
    /// The logo's height in pixels.
    pub logo_height: i64,
    /// The logo's opacity in percent.
    pub logo_opacity: i64,
    /// The banner's opacity in percent.
    pub banner_opacity: i64,
    /// The privacy policy's link, or empty.
    pub privacy_url: String,
    /// The terms' link, or empty.
    pub terms_url: String,
    /// The page's language when a visitor's is not offered.
    pub fallback_locale: String,
}

impl BrandingForm {
    fn of(branding: &SchedulingBranding) -> Self {
        Self {
            business_name: branding.business_name.clone(),
            logo_height: branding.logo_height,
            logo_opacity: branding.logo_opacity,
            banner_opacity: branding.banner_opacity,
            privacy_url: branding.privacy_url.clone(),
            terms_url: branding.terms_url.clone(),
            fallback_locale: branding.fallback_locale.clone(),
        }
    }

    fn issue(&self) -> Option<String> {
        if self.business_name.trim().chars().count() > SETTINGS_NAME_MAX {
            return Some(ss::settings_business_name_too_long(l()).to_owned());
        }
        if !LOGO_HEIGHTS.contains(&self.logo_height) {
            return Some(ss::settings_whole_number_between(
                l(),
                *LOGO_HEIGHTS.start(),
                *LOGO_HEIGHTS.end(),
            ));
        }
        if !OPACITIES.contains(&self.logo_opacity) || !OPACITIES.contains(&self.banner_opacity) {
            return Some(ss::settings_whole_number_between(
                l(),
                *OPACITIES.start(),
                *OPACITIES.end(),
            ));
        }
        if let Some(issue) = legal_url_issue(&self.privacy_url).or(legal_url_issue(&self.terms_url))
        {
            return Some(issue.to_owned());
        }
        self.fallback_locale
            .trim()
            .is_empty()
            .then(|| ss::settings_fallback_locale_required(l()).to_owned())
    }
}

/// Why a privacy or terms link cannot be sent: too long, or not a full http
/// or https address. Empty is none, which is fine.
fn legal_url_issue(value: &str) -> Option<&'static str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() > LEGAL_URL_MAX {
        return Some(ss::settings_url_too_long(l()));
    }
    let absolute = Url::parse(trimmed).ok().is_some_and(|u| {
        matches!(u.scheme(), "http" | "https") && u.host_str().is_some_and(|h| !h.is_empty())
    });
    (!absolute).then(|| ss::settings_url_not_absolute(l()))
}

/// Editing the booking page and its images.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrandingEditor {
    /// The branding as last read.
    pub branding: SchedulingBranding,
    /// The fields.
    pub form: BrandingForm,
    /// Why the fields cannot be sent.
    pub rejected: Option<String>,
    /// The save's state.
    pub write: WriteState,
    /// The logo's upload or removal.
    pub logo: WriteState,
    /// The banner's upload or removal.
    pub banner: WriteState,
}

impl BrandingEditor {
    /// Whether a save or an image is on its way.
    pub fn busy(&self) -> bool {
        self.write.busy() || self.logo.busy() || self.banner.busy()
    }

    /// The languages the page can fall back to.
    pub fn locale_options(&self) -> &[SchedulingLocaleOption] {
        self.branding
            .supported_locales
            .as_deref()
            .unwrap_or_default()
    }

    fn image(&mut self, target: SchedulingUploadTarget) -> &mut WriteState {
        match target {
            SchedulingUploadTarget::Logo => &mut self.logo,
            _ => &mut self.banner,
        }
    }

    /// The labels: the title, each field's label and hint, the images' line,
    /// and the save button.
    pub fn labels() -> [&'static str; 15] {
        [
            ss::settings_branding_title(l()),
            ss::settings_branding_business_name(l()),
            ss::settings_business_name_hint(l()),
            ss::settings_logo_height_label(l()),
            ss::settings_logo_height_hint(l()),
            ss::settings_logo_opacity_label(l()),
            ss::settings_banner_opacity_label(l()),
            ss::settings_opacity_hint(l()),
            ss::settings_branding_privacy(l()),
            ss::settings_branding_terms(l()),
            ss::settings_legal_url_hint(l()),
            ss::settings_branding_locale(l()),
            ss::settings_fallback_locale_hint(l()),
            ss::settings_image_immediate(l()),
            ss::settings_branding_save(l()),
        ]
    }
}

/// Editing the booking assistant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistantEditor {
    /// The settings as last read.
    pub llm: SchedulingLlmSettings,
    /// Whether it is on.
    pub enabled: bool,
    /// The extra instructions.
    pub instructions: String,
    /// Why they cannot be sent.
    pub rejected: Option<&'static str>,
    /// The save's state.
    pub write: WriteState,
}

impl AssistantEditor {
    /// Whether anything changed, so Save can be pressed.
    pub fn is_dirty(&self) -> bool {
        self.enabled != self.llm.enabled || self.instructions != self.llm.extra_instructions
    }

    /// The labels: the title, the hint, the switch, the instructions' label,
    /// hint and placeholder, and the save button.
    pub fn labels() -> [&'static str; 7] {
        [
            ss::settings_automation_title(l()),
            ss::settings_assistant_hint(l()),
            ss::settings_assistant_toggle(l()),
            ss::settings_assistant_instructions_label(l()),
            ss::settings_assistant_instructions_hint(l()),
            ss::settings_assistant_instructions_placeholder(l()),
            ss::settings_automation_save(l()),
        ]
    }
}

/// A dialog over the section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsDialog {
    /// The member's profile.
    Profile(ProfileEditor),
    /// The member's notifications.
    Notifications(NotificationsEditor),
    /// The booking page.
    Branding(BrandingEditor),
    /// The booking assistant.
    Assistant(AssistantEditor),
}

/// The settings section.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsSection {
    /// The tab shown.
    pub tab: SettingsTab,
    /// The booking page; `None` until its tab is first shown.
    pub branding: Option<SectionLoad<SchedulingBranding>>,
    /// The assistant; `None` until its tab is first shown.
    pub assistant: Option<SectionLoad<SchedulingLlmSettings>>,
    /// The member's profile, for the profile and the notifications; `None`
    /// until one of their tabs is first shown.
    pub profile: Option<SectionLoad<SchedulingMe>>,
    /// The dialog open over the section.
    pub dialog: Option<SettingsDialog>,
}

impl SettingsSection {
    /// The labels: the assistant's hint and two row labels, and the On and
    /// Off of a switch.
    pub fn labels() -> [&'static str; 5] {
        [
            ss::settings_assistant_hint(l()),
            ss::settings_assistant_enabled(l()),
            ss::settings_assistant_instructions(l()),
            ss::common_on(l()),
            ss::common_off(l()),
        ]
    }
}

/// What the member does on Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsEvent {
    /// Show a tab, read the first time.
    SelectTab(SettingsTab),
    /// Read the tab shown again.
    Refresh,
    /// Open the profile's editor.
    OpenProfile,
    /// The profile's fields changed.
    EditProfile(ProfileForm),
    /// Save the profile.
    SaveProfile,
    /// Open the notifications' editor.
    OpenNotifications,
    /// A notification switched.
    SetNotification {
        /// One of [`NOTIFICATIONS`].
        field: String,
        /// On or off.
        on: bool,
    },
    /// Save the notifications.
    SaveNotifications,
    /// Open the booking page's editor.
    OpenBranding,
    /// The booking page's fields changed.
    EditBranding(BrandingForm),
    /// Save the booking page.
    SaveBranding,
    /// Upload an image: the logo or banner from the booking page's editor, the
    /// picture from the profile's.
    UploadImage {
        /// Which.
        target: SchedulingUploadTarget,
        /// The file.
        file: ImageFile,
    },
    /// The image chosen could not be read.
    ImageUnreadable(SchedulingUploadTarget),
    /// Remove an image.
    RemoveImage(SchedulingUploadTarget),
    /// Open the assistant's editor.
    OpenAssistant,
    /// The assistant's fields changed.
    EditAssistant {
        /// Whether it is on.
        enabled: bool,
        /// The extra instructions.
        instructions: String,
    },
    /// Save the assistant.
    SaveAssistant,
    /// Close the dialog.
    CloseDialog,
}

/// What an answer the section awaits is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    Branding,
    Assistant,
    Profile,
    ProfileSave,
    Notifications,
    /// The picture uploaded (`true`) or removed.
    Avatar(bool),
    Me,
    BrandingSave,
    /// An image uploaded (`true`) or removed.
    Image(SchedulingUploadTarget, bool),
    BrandingRead,
    AssistantSave,
}

fn send(pending: Pending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::Settings(pending), Box::new(request))
}

pub(crate) fn enter(admin: &mut SchedulingAdmin, tickets: &mut Tickets) -> Vec<Out> {
    admin.forget(tickets, |p| matches!(p, AdminPending::Settings(_)));
    let mut section = SettingsSection::default();
    let outs = read_tab(&mut section);
    admin.settings = Some(section);
    outs
}

/// The tab shown, read when it has not been.
fn read_tab(section: &mut SettingsSection) -> Vec<Out> {
    let (unread, pending, request) = match section.tab {
        SettingsTab::BookingPage => (
            section.branding.is_none(),
            Pending::Branding,
            SchedulingAdminRequest::Branding,
        ),
        SettingsTab::Assistant => (
            section.assistant.is_none(),
            Pending::Assistant,
            SchedulingAdminRequest::Llm,
        ),
        SettingsTab::Profile | SettingsTab::Notifications => (
            section.profile.is_none(),
            Pending::Profile,
            SchedulingAdminRequest::Me,
        ),
    };
    if !unread {
        return Vec::new();
    }
    match pending {
        Pending::Branding => section.branding = Some(SectionLoad::Loading),
        Pending::Assistant => section.assistant = Some(SectionLoad::Loading),
        _ => section.profile = Some(SectionLoad::Loading),
    }
    vec![send(pending, request)]
}

/// The pending read of `tab`.
fn reading(tab: SettingsTab) -> AdminPending {
    AdminPending::Settings(match tab {
        SettingsTab::BookingPage => Pending::Branding,
        SettingsTab::Assistant => Pending::Assistant,
        SettingsTab::Profile | SettingsTab::Notifications => Pending::Profile,
    })
}

pub(crate) fn event(
    admin: &mut SchedulingAdmin,
    tickets: &mut Tickets,
    event: SettingsEvent,
    can_change: bool,
) -> Vec<Out> {
    let Some(tab) = admin.settings.as_ref().map(|s| s.tab) else {
        return Vec::new();
    };
    if event == SettingsEvent::Refresh {
        // A read still on its way is superseded.
        let superseded = reading(tab);
        admin.forget(tickets, |p| *p == superseded);
    }
    admin
        .settings
        .as_mut()
        .map(|section| section_event(section, event, can_change))
        .unwrap_or_default()
}

fn section_event(
    section: &mut SettingsSection,
    event: SettingsEvent,
    can_change: bool,
) -> Vec<Out> {
    match event {
        SettingsEvent::SelectTab(tab) => {
            section.tab = tab;
            return read_tab(section);
        }
        SettingsEvent::Refresh => {
            match section.tab {
                SettingsTab::BookingPage => section.branding = None,
                SettingsTab::Assistant => section.assistant = None,
                SettingsTab::Profile | SettingsTab::Notifications => section.profile = None,
            }
            return read_tab(section);
        }
        SettingsEvent::OpenProfile => {
            if let Some(me) = profile(section) {
                section.dialog = Some(SettingsDialog::Profile(ProfileEditor {
                    form: ProfileForm::of(&me),
                    me,
                    rejected: None,
                    write: WriteState::Idle,
                    avatar: WriteState::Idle,
                    avatar_override: None,
                }));
            }
        }
        SettingsEvent::OpenNotifications => {
            if let Some(me) = profile(section) {
                section.dialog = Some(SettingsDialog::Notifications(NotificationsEditor {
                    values: notification_values(&me),
                    write: WriteState::Idle,
                }));
            }
        }
        SettingsEvent::OpenBranding if can_change => {
            let branding = section
                .branding
                .as_ref()
                .and_then(SectionLoad::value)
                .cloned();
            if let Some(branding) = branding {
                section.dialog = Some(SettingsDialog::Branding(BrandingEditor {
                    form: BrandingForm::of(&branding),
                    branding,
                    rejected: None,
                    write: WriteState::Idle,
                    logo: WriteState::Idle,
                    banner: WriteState::Idle,
                }));
            }
        }
        SettingsEvent::OpenAssistant if can_change => {
            let llm = section
                .assistant
                .as_ref()
                .and_then(SectionLoad::value)
                .cloned();
            if let Some(llm) = llm {
                section.dialog = Some(SettingsDialog::Assistant(AssistantEditor {
                    enabled: llm.enabled,
                    instructions: llm.extra_instructions.clone(),
                    llm,
                    rejected: None,
                    write: WriteState::Idle,
                }));
            }
        }
        SettingsEvent::CloseDialog => section.dialog = None,
        event => {
            return match &mut section.dialog {
                Some(SettingsDialog::Profile(editor)) => profile_event(editor, event),
                Some(SettingsDialog::Notifications(editor)) => notifications_event(editor, event),
                Some(SettingsDialog::Branding(editor)) => branding_event(editor, event),
                Some(SettingsDialog::Assistant(editor)) => assistant_event(editor, event),
                None => Vec::new(),
            };
        }
    }
    Vec::new()
}

fn profile(section: &SettingsSection) -> Option<SchedulingMe> {
    section
        .profile
        .as_ref()
        .and_then(SectionLoad::value)
        .cloned()
}

/// A field changed: the last refusal is dropped.
fn edited(write: &mut WriteState) {
    if write.failure().is_some() {
        *write = WriteState::Idle;
    }
}

fn profile_event(editor: &mut ProfileEditor, event: SettingsEvent) -> Vec<Out> {
    if editor.busy() {
        return Vec::new();
    }
    match event {
        SettingsEvent::EditProfile(form) => {
            editor.form = form;
            editor.rejected = None;
            edited(&mut editor.write);
        }
        SettingsEvent::SaveProfile => {
            editor.rejected = editor.form.issue();
            if editor.rejected.is_none() {
                editor.write = WriteState::Working;
                let form = &editor.form;
                return vec![send(
                    Pending::ProfileSave,
                    SchedulingAdminRequest::PatchMe(SchedulingProfilePatch {
                        name: Some(form.name.trim().to_owned()),
                        timezone: Some(form.timezone.trim().to_owned()),
                        time_format: Some(form.time_format.clone()),
                        week_start: Some(form.week_start),
                        date_format: Some(form.date_format.clone()),
                        ..SchedulingProfilePatch::default()
                    }),
                )];
            }
        }
        SettingsEvent::UploadImage {
            target: SchedulingUploadTarget::Avatar,
            file,
        } => match upload(SchedulingUploadTarget::Avatar, file) {
            Err(issue) => editor.avatar = WriteState::Failed(FailureText::final_(issue)),
            Ok(upload) => {
                editor.avatar = WriteState::Working;
                return vec![send(
                    Pending::Avatar(true),
                    SchedulingAdminRequest::Upload(upload),
                )];
            }
        },
        SettingsEvent::ImageUnreadable(SchedulingUploadTarget::Avatar) => {
            editor.avatar = unreadable();
        }
        SettingsEvent::RemoveImage(SchedulingUploadTarget::Avatar) => {
            editor.avatar = WriteState::Working;
            return vec![send(
                Pending::Avatar(false),
                SchedulingAdminRequest::DeleteAvatar,
            )];
        }
        _ => {}
    }
    Vec::new()
}

fn notifications_event(editor: &mut NotificationsEditor, event: SettingsEvent) -> Vec<Out> {
    if editor.write.busy() {
        return Vec::new();
    }
    match event {
        SettingsEvent::SetNotification { field, on } => {
            for entry in editor.values.iter_mut().filter(|(f, _)| *f == field) {
                entry.1 = on;
            }
            edited(&mut editor.write);
        }
        SettingsEvent::SaveNotifications => {
            editor.write = WriteState::Working;
            let on = |field| editor.value(field);
            return vec![send(
                Pending::Notifications,
                SchedulingAdminRequest::PatchMe(SchedulingProfilePatch {
                    notify_confirmation: Some(on("notify_confirmation")),
                    notify_cancellation: Some(on("notify_cancellation")),
                    notify_reschedule: Some(on("notify_reschedule")),
                    notify_reminder: Some(on("notify_reminder")),
                    notify_host_booking: Some(on("notify_host_booking")),
                    notify_host_cancel: Some(on("notify_host_cancel")),
                    notify_host_reschedule: Some(on("notify_host_reschedule")),
                    ..SchedulingProfilePatch::default()
                }),
            )];
        }
        _ => {}
    }
    Vec::new()
}

fn branding_event(editor: &mut BrandingEditor, event: SettingsEvent) -> Vec<Out> {
    if editor.busy() {
        return Vec::new();
    }
    match event {
        SettingsEvent::EditBranding(form) => {
            editor.form = form;
            editor.rejected = None;
            edited(&mut editor.write);
        }
        SettingsEvent::SaveBranding => {
            editor.rejected = editor.form.issue();
            if editor.rejected.is_none() {
                editor.write = WriteState::Working;
                let form = &editor.form;
                return vec![send(
                    Pending::BrandingSave,
                    SchedulingAdminRequest::PatchBranding(SchedulingBrandingPatch {
                        business_name: form.business_name.trim().to_owned(),
                        logo_height: form.logo_height,
                        logo_opacity: form.logo_opacity,
                        banner_opacity: form.banner_opacity,
                        privacy_url: form.privacy_url.trim().to_owned(),
                        terms_url: form.terms_url.trim().to_owned(),
                        fallback_locale: form.fallback_locale.clone(),
                    }),
                )];
            }
        }
        SettingsEvent::UploadImage { target, file } if target != SchedulingUploadTarget::Avatar => {
            match upload(target, file) {
                Err(issue) => {
                    *editor.image(target) = WriteState::Failed(FailureText::final_(issue))
                }
                Ok(upload) => {
                    *editor.image(target) = WriteState::Working;
                    return vec![send(
                        Pending::Image(target, true),
                        SchedulingAdminRequest::Upload(upload),
                    )];
                }
            }
        }
        SettingsEvent::ImageUnreadable(target) if target != SchedulingUploadTarget::Avatar => {
            *editor.image(target) = unreadable();
        }
        SettingsEvent::RemoveImage(target) if target != SchedulingUploadTarget::Avatar => {
            *editor.image(target) = WriteState::Working;
            let request = match target {
                SchedulingUploadTarget::Logo => SchedulingAdminRequest::DeleteLogo,
                _ => SchedulingAdminRequest::DeleteBanner,
            };
            return vec![send(Pending::Image(target, false), request)];
        }
        _ => {}
    }
    Vec::new()
}

fn assistant_event(editor: &mut AssistantEditor, event: SettingsEvent) -> Vec<Out> {
    if editor.write.busy() {
        return Vec::new();
    }
    match event {
        SettingsEvent::EditAssistant {
            enabled,
            instructions,
        } => {
            editor.enabled = enabled;
            editor.instructions = instructions;
            editor.rejected = None;
            edited(&mut editor.write);
        }
        SettingsEvent::SaveAssistant if editor.is_dirty() => {
            if editor.instructions.chars().count() > ASSISTANT_INSTRUCTIONS_MAX {
                editor.rejected = Some(ss::settings_assistant_instructions_too_long(l()));
            } else {
                editor.write = WriteState::Working;
                return vec![send(
                    Pending::AssistantSave,
                    SchedulingAdminRequest::PatchLlm {
                        enabled: Some(editor.enabled),
                        extra_instructions: Some(editor.instructions.clone()),
                    },
                )];
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
        .settings
        .as_mut()
        .map(|section| settings_answer(section, pending, answer))
        .unwrap_or_default()
}

/// What `answer` holds, picked by `pick`, or why not.
fn picked<T>(
    answer: Answer,
    pick: impl FnOnce(district_model::SchedulingAdminData) -> Option<T>,
) -> Result<T, FailureText> {
    answer.and_then(|data| pick(data).ok_or_else(FailureText::unexpected))
}

fn settings_answer(section: &mut SettingsSection, pending: Pending, answer: Answer) -> Vec<Out> {
    match (pending, &mut section.dialog) {
        (Pending::Branding, _) => section.branding = Some(load(answer, data!(Branding))),
        (Pending::Assistant, _) => section.assistant = Some(load(answer, data!(Llm))),
        (Pending::Profile, _) => section.profile = Some(load(answer, data!(Me))),
        (Pending::ProfileSave, Some(SettingsDialog::Profile(editor))) => {
            match picked(answer, data!(Me)) {
                Ok(me) => {
                    editor.me = me.clone();
                    editor.avatar_override = None;
                    editor.write = WriteState::Done(ss::settings_profile_done(l()).to_owned());
                    section.profile = Some(SectionLoad::Ready(me));
                }
                Err(failure) => editor.write = WriteState::Failed(failure),
            }
        }
        (Pending::Notifications, Some(SettingsDialog::Notifications(editor))) => {
            match picked(answer, data!(Me)) {
                Ok(me) => {
                    editor.write =
                        WriteState::Done(ss::settings_notifications_done(l()).to_owned());
                    section.profile = Some(SectionLoad::Ready(me));
                }
                Err(failure) => editor.write = WriteState::Failed(failure),
            }
        }
        // The picture uploaded or removed: the address it was published at,
        // or the profile read again.
        (Pending::Avatar(uploaded), Some(SettingsDialog::Profile(editor))) => match answer {
            Ok(data) => {
                let published = data!(Uploaded)(data)
                    .and_then(|r| r.avatar_url)
                    .filter(|u| !u.is_empty());
                editor.avatar = WriteState::Done(done(SchedulingUploadTarget::Avatar, uploaded));
                match published {
                    Some(url) => editor.avatar_override = Some(url),
                    None => return vec![send(Pending::Me, SchedulingAdminRequest::Me)],
                }
            }
            Err(failure) => editor.avatar = WriteState::Failed(failure),
        },
        // The profile read again after a picture: kept quietly when it fails.
        (Pending::Me, Some(SettingsDialog::Profile(editor))) => {
            if let Ok(me) = picked(answer, data!(Me)) {
                editor.me = me.clone();
                editor.avatar_override = None;
                section.profile = Some(SectionLoad::Ready(me));
            }
        }
        (Pending::BrandingSave, Some(SettingsDialog::Branding(editor))) => {
            match picked(answer, data!(Branding)) {
                Ok(branding) => {
                    editor.branding = branding.clone();
                    editor.write = WriteState::Done(ss::settings_branding_done(l()).to_owned());
                    section.branding = Some(SectionLoad::Ready(branding));
                }
                Err(failure) => editor.write = WriteState::Failed(failure),
            }
        }
        // An image uploaded or removed: the branding read again.
        (Pending::Image(target, uploaded), Some(SettingsDialog::Branding(editor))) => {
            match answer {
                Ok(_) => {
                    *editor.image(target) = WriteState::Done(done(target, uploaded));
                    return vec![send(
                        Pending::BrandingRead,
                        SchedulingAdminRequest::Branding,
                    )];
                }
                Err(failure) => *editor.image(target) = WriteState::Failed(failure),
            }
        }
        // The branding read again after an image: kept quietly when it fails.
        (Pending::BrandingRead, Some(SettingsDialog::Branding(editor))) => {
            if let Ok(branding) = picked(answer, data!(Branding)) {
                editor.branding = branding.clone();
                section.branding = Some(SectionLoad::Ready(branding));
            }
        }
        (Pending::AssistantSave, Some(SettingsDialog::Assistant(editor))) => {
            match picked(answer, data!(Llm)) {
                Ok(llm) => {
                    editor.enabled = llm.enabled;
                    editor.instructions = llm.extra_instructions.clone();
                    editor.llm = llm.clone();
                    editor.write = WriteState::Done(ss::settings_automation_done(l()).to_owned());
                    section.assistant = Some(SectionLoad::Ready(llm));
                }
                Err(failure) => editor.write = WriteState::Failed(failure),
            }
        }
        // An answer for a dialog that has closed.
        _ => {}
    }
    Vec::new()
}
