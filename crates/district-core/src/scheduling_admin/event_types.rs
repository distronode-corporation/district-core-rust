//! Event types: the list with its create form, and one event type with its
//! editor, its state actions (on and off, archive and restore, a test email,
//! delete), its hosts and its booking questions.
//!
//! An event type is addressed by its slug, which is the last segment of its
//! public booking page's address: nobody types it. A create makes one from the
//! name, as the web does, and nothing changes it after (links already shared
//! point at it). Every write is a member's who may change things.
//!
//! The hosts are a whole list the scheduler replaces, so their dialog can only
//! save a list it has read: it opens on a read and offers no save until it
//! lands. Deleting an event type is asked once more, naming what survives it.

use district_l10n::scheduling_strings as ss;
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingEventType, SchedulingEventTypeFields,
    SchedulingHost, SchedulingHostAssignment, SchedulingLocationValueKind, SchedulingQuestion,
    SchedulingQuestionFields, SchedulingStatusResponse, SchedulingUser, location_value_kind,
};

use super::overview::{booking_url_for, load};
use super::{
    Answer, Out, Pending as AdminPending, SchedulingAdmin, SchedulingSection, SectionLoad,
    WriteState,
};
use crate::failure::FailureText;
use crate::model::Tickets;
use crate::route::Route;
use crate::text::l;
use crate::workflows::Tone;

/// "30 min", or "Not set".
pub fn minutes_label(minutes: Option<i64>) -> String {
    match minutes {
        Some(minutes) => ss::booking_minutes(l(), minutes),
        None => ss::booking_minutes_not_set(l()).to_owned(),
    }
}

/// A location type's name. A type this build does not know is shown as it
/// came; "Not set" is for an event type that has none.
pub fn location_label(location_type: Option<&str>) -> String {
    match location_type.filter(|t| !t.is_empty()) {
        None => ss::location_not_set(l()).to_owned(),
        Some("google_meet") => ss::location_google_meet(l()).to_owned(),
        Some("teams") => ss::location_teams(l()).to_owned(),
        Some("custom_video") => ss::location_custom_video(l()).to_owned(),
        Some("phone") => ss::location_phone(l()).to_owned(),
        Some("in_person") => ss::location_in_person(l()).to_owned(),
        Some("link") => ss::location_link(l()).to_owned(),
        Some("livekit") => ss::location_built_in_video(l()).to_owned(),
        Some(other) => other.to_owned(),
    }
}

/// An event type's state: archived, switched off, or active. A flag the
/// scheduler left out counts as set.
pub fn state_label(item: &SchedulingEventType) -> (&'static str, Tone) {
    if item.archived == Some(true) {
        (ss::state_archived(l()), Tone::Neutral)
    } else if item.is_active == Some(false) {
        (ss::state_inactive(l()), Tone::Neutral)
    } else {
        (ss::state_active(l()), Tone::Success)
    }
}

/// One row of the event types list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventTypeRow {
    /// Its slug, which its screen is opened by.
    pub slug: String,
    /// Its name.
    pub name: String,
    /// "30 min · Phone".
    pub subtitle: String,
    /// The state badge.
    pub state: &'static str,
    /// Its tone.
    pub tone: Tone,
    /// Its public booking link, or "Not public": archived, switched off,
    /// unlisted, or no address yet.
    pub booking_page: String,
}

/// The event types list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventTypesSection {
    /// The event types.
    pub list: SectionLoad<Vec<SchedulingEventType>>,
    /// The booking pages' address, from the status, for the links.
    pub public_host: Option<String>,
    /// The create form, while it is open.
    pub editor: Option<EventTypeEditor>,
}

impl EventTypesSection {
    /// The rows, once read.
    pub fn rows(&self) -> Vec<EventTypeRow> {
        self.list
            .value()
            .map(|items| items.iter().map(|item| self.row(item)).collect())
            .unwrap_or_default()
    }

    fn row(&self, item: &SchedulingEventType) -> EventTypeRow {
        let (state, tone) = state_label(item);
        let bookable = item.archived != Some(true)
            && item.is_active != Some(false)
            && item.is_public != Some(false);
        EventTypeRow {
            slug: item.slug.clone(),
            name: item.name.clone(),
            subtitle: ss::event_type_subtitle(
                l(),
                &minutes_label(Some(item.duration_minutes)),
                &location_label(item.location_type.as_deref()),
            ),
            state,
            tone,
            booking_page: match self.public_host.as_deref().filter(|h| !h.is_empty()) {
                Some(host) if bookable => booking_url_for(host, &item.slug),
                _ => ss::not_public(l()).to_owned(),
            },
        }
    }

    /// The empty list's heading.
    pub fn empty_title() -> &'static str {
        ss::event_types_empty_title(l())
    }

    /// The empty list's sentence.
    pub fn empty_body() -> &'static str {
        ss::event_types_empty_body(l())
    }

    /// The create button.
    pub fn create_label() -> &'static str {
        ss::editor_create_title(l())
    }
}

/// The location a new event type gets: the built-in video, which the platform
/// provides itself, so a create that chose nothing is still bookable.
pub const DEFAULT_LOCATION: &str = "livekit";

/// The four location types the form offers, in the web's order.
pub const OFFERED_LOCATIONS: [&str; 4] = ["livekit", "phone", "in_person", "link"];

/// An event type's fields as the form holds them: numbers as typed, so a box
/// emptied to retype it stays empty rather than snapping to 0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventTypeForm {
    /// Its name.
    pub name: String,
    /// What the booking page says about it.
    pub description: String,
    /// Minutes.
    pub duration: String,
    /// Minutes between start times.
    pub interval: String,
    /// The location type's wire value.
    pub location: String,
    /// The phone number, address or link.
    pub location_value: String,
    /// Whether it can be booked.
    pub is_active: bool,
    /// Whether it is listed on the booking page.
    pub is_public: bool,
    /// Whether taken times show as taken.
    pub show_taken_slots: bool,
    /// Minutes kept free before.
    pub buffer_before: String,
    /// Minutes kept free after.
    pub buffer_after: String,
    /// Minimum notice, minutes.
    pub min_notice: String,
    /// How many days ahead; 0 for no limit.
    pub max_future_days: String,
    /// Bookings one person may hold; 0 for no limit.
    pub max_active_bookings: String,
}

impl Default for EventTypeForm {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            duration: "30".to_owned(),
            interval: "30".to_owned(),
            location: DEFAULT_LOCATION.to_owned(),
            location_value: String::new(),
            is_active: true,
            is_public: true,
            show_taken_slots: false,
            buffer_before: "0".to_owned(),
            buffer_after: "0".to_owned(),
            min_notice: "0".to_owned(),
            max_future_days: "0".to_owned(),
            max_active_bookings: "0".to_owned(),
        }
    }
}

/// A whole number at or above `floor`, or `None`.
pub(crate) fn whole(raw: &str, floor: i64) -> Option<i64> {
    raw.trim()
        .parse::<i64>()
        .ok()
        .filter(|value| *value >= floor)
}

impl EventTypeForm {
    /// The form for a stored event type. A flag left out reads as set, and an
    /// interval left out as the duration, as the scheduler reads them.
    pub fn from_event_type(row: &SchedulingEventType) -> Self {
        let count = |value: Option<i64>| value.unwrap_or(0).to_string();
        Self {
            name: row.name.clone(),
            description: row.description().unwrap_or_default().to_owned(),
            duration: row.duration_minutes.to_string(),
            interval: row
                .slot_interval_minutes
                .unwrap_or(row.duration_minutes)
                .to_string(),
            location: row
                .location_type
                .clone()
                .unwrap_or_else(|| DEFAULT_LOCATION.to_owned()),
            location_value: row.location_value().unwrap_or_default().to_owned(),
            is_active: row.is_active != Some(false),
            is_public: row.is_public != Some(false),
            show_taken_slots: row.show_taken_slots == Some(true),
            buffer_before: count(row.buffer_before_minutes),
            buffer_after: count(row.buffer_after_minutes),
            min_notice: count(row.min_notice_minutes),
            max_future_days: count(row.max_future_days),
            max_active_bookings: count(row.max_active_bookings),
        }
    }

    fn trimmed_name(&self) -> &str {
        self.name.trim()
    }

    /// Why a create cannot be sent: a name and a duration are all it needs.
    pub fn create_error(&self) -> Option<&'static str> {
        if self.trimmed_name().is_empty() {
            Some(ss::editor_name_missing(l()))
        } else if whole(&self.duration, 1).is_none() {
            Some(ss::editor_create_duration_invalid(l()))
        } else {
            None
        }
    }

    /// Why an edit cannot be sent: the first fault, once.
    pub fn edit_error(&self) -> Option<&'static str> {
        let counts = [
            &self.buffer_before,
            &self.buffer_after,
            &self.min_notice,
            &self.max_future_days,
            &self.max_active_bookings,
        ];
        if self.trimmed_name().is_empty() {
            Some(ss::editor_name_missing(l()))
        } else if whole(&self.duration, 1).is_none() || whole(&self.interval, 1).is_none() {
            Some(ss::editor_at_least_one_minute(l()))
        } else if counts.iter().any(|count| whole(count, 0).is_none()) {
            Some(ss::editor_at_least_zero(l()))
        } else {
            None
        }
    }

    /// What a create sends beside the slug: the name, the duration and the
    /// location, as the web's create does; the rest is edited after.
    fn create_fields(&self) -> SchedulingEventTypeFields {
        SchedulingEventTypeFields {
            name: Some(self.trimmed_name().to_owned()),
            duration_minutes: whole(&self.duration, 1),
            location_type: Some(self.location.clone()),
            ..SchedulingEventTypeFields::default()
        }
    }

    /// What an edit sends: every field, every time, so a cleared address is
    /// sent cleared rather than read as untouched.
    fn edit_fields(&self) -> SchedulingEventTypeFields {
        SchedulingEventTypeFields {
            name: Some(self.trimmed_name().to_owned()),
            description: Some(self.description.clone()),
            duration_minutes: whole(&self.duration, 1),
            slot_interval_minutes: whole(&self.interval, 1),
            location_type: Some(self.location.clone()),
            location_value: Some(self.location_value.clone()),
            is_active: Some(self.is_active),
            is_public: Some(self.is_public),
            show_taken_slots: Some(self.show_taken_slots),
            buffer_before_minutes: whole(&self.buffer_before, 0),
            buffer_after_minutes: whole(&self.buffer_after, 0),
            min_notice_minutes: whole(&self.min_notice, 0),
            max_future_days: whole(&self.max_future_days, 0),
            max_active_bookings: whole(&self.max_active_bookings, 0),
            ..SchedulingEventTypeFields::default()
        }
    }
}

/// The slug a name makes, as the web's `slugFromName` makes it: accents
/// folded, lower case, every run of anything else one hyphen, 200 characters
/// at most, no hyphen at either end, and `event-type` for a name that leaves
/// nothing.
pub fn slug_from_name(name: &str) -> String {
    let mut slug = String::new();
    let mut pending_hyphen = false;
    for c in name.chars().flat_map(fold).flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            if pending_hyphen && !slug.is_empty() {
                slug.push('-');
            }
            pending_hyphen = false;
            slug.push(c);
        } else {
            pending_hyphen = true;
        }
    }
    slug.truncate(200);
    match slug.trim_end_matches('-') {
        "" => "event-type".to_owned(),
        slug => slug.to_owned(),
    }
}

/// A letter with its accents taken off, as compatibility decomposition (NFKD)
/// and the dropping of combining marks leave it, for the Latin letters a name
/// is written in, the ligatures and the full-width forms. Anything else comes
/// back as it was, and a slug reads it as a separator.
fn fold(c: char) -> Vec<char> {
    const FROM: &str = "\u{c0}\u{c1}\u{c2}\u{c3}\u{c4}\u{c5}\u{e0}\u{e1}\u{e2}\u{e3}\u{e4}\u{e5}\u{c7}\u{e7}\u{c8}\u{c9}\u{ca}\u{cb}\u{e8}\u{e9}\u{ea}\u{eb}\u{cc}\u{cd}\u{ce}\u{cf}\u{ec}\u{ed}\u{ee}\u{ef}\u{d1}\u{f1}\u{d2}\u{d3}\u{d4}\u{d5}\u{d6}\u{f2}\u{f3}\u{f4}\u{f5}\u{f6}\u{d9}\u{da}\u{db}\u{dc}\u{f9}\u{fa}\u{fb}\u{fc}\u{dd}\u{fd}\u{ff}\u{178}";
    const TO: &str = "AAAAAAaaaaaaCcEEEEeeeeIIIIiiiiNnOOOOOoooooUUUUuuuuYyyY";
    match c {
        '\u{fb00}' => vec!['f', 'f'],
        '\u{fb01}' => vec!['f', 'i'],
        '\u{fb02}' => vec!['f', 'l'],
        '\u{ff01}'..='\u{ff5e}' => vec![char::from_u32(c as u32 - 0xfee0).unwrap_or(c)],
        _ => vec![
            FROM.chars()
                .position(|from| from == c)
                .and_then(|index| TO.chars().nth(index))
                .unwrap_or(c),
        ],
    }
}

/// A slug for `name` that no row on screen uses: the name's own, else with
/// `-2`, `-3` and so on. The scheduler's own check settles a row made
/// elsewhere meanwhile.
pub fn unique_slug(name: &str, taken: &[String]) -> String {
    let base = slug_from_name(name);
    std::iter::once(base.clone())
        .chain((2..).map(|n| format!("{base}-{n}")))
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or(base)
}

/// The create or edit form for one event type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventTypeEditor {
    /// The slug of the event type edited; `None` for a create.
    pub slug: Option<String>,
    /// The fields.
    pub form: EventTypeForm,
    /// Why the form cannot be sent, before anything was sent.
    pub validation: Option<&'static str>,
    /// The save's state.
    pub write: WriteState,
    /// The location type as loaded, kept on offer even when it is not one the
    /// form offers, so looking at the list does not move it.
    stored_location: Option<String>,
    /// The slugs on screen, which a create's slug avoids.
    taken: Vec<String>,
}

impl EventTypeEditor {
    fn create(taken: Vec<String>) -> Self {
        Self {
            slug: None,
            form: EventTypeForm::default(),
            validation: None,
            write: WriteState::Idle,
            stored_location: None,
            taken,
        }
    }

    fn edit(row: &SchedulingEventType) -> Self {
        Self {
            slug: Some(row.slug.clone()),
            form: EventTypeForm::from_event_type(row),
            validation: None,
            write: WriteState::Idle,
            stored_location: row.location_type.clone(),
            taken: Vec::new(),
        }
    }

    /// Whether this is a create, which shows the short form (name, duration,
    /// location) rather than every field.
    pub fn is_creating(&self) -> bool {
        self.slug.is_none()
    }

    /// "Create event type" or "Edit event type".
    pub fn title(&self) -> &'static str {
        if self.is_creating() {
            ss::editor_create_title(l())
        } else {
            ss::editor_edit_title(l())
        }
    }

    /// The location types to choose from, as (wire value, label): the four the
    /// form offers, and the stored one when it is another, labelled as it
    /// came.
    pub fn location_choices(&self) -> Vec<(String, String)> {
        let mut choices: Vec<(String, String)> = OFFERED_LOCATIONS
            .iter()
            .map(|value| ((*value).to_owned(), location_label(Some(value))))
            .collect();
        if let Some(stored) = self
            .stored_location
            .as_ref()
            .filter(|stored| !OFFERED_LOCATIONS.contains(&stored.as_str()))
        {
            choices.push((stored.clone(), stored.clone()));
        }
        choices
    }

    /// The location value field's label, or `None` when the type makes its own
    /// link and takes no value.
    pub fn location_value_label(&self) -> Option<&'static str> {
        match location_value_kind(&self.form.location)? {
            SchedulingLocationValueKind::Generated => None,
            SchedulingLocationValueKind::Url => Some(ss::editor_meeting_link(l())),
            SchedulingLocationValueKind::Phone => Some(ss::editor_phone_number(l())),
            SchedulingLocationValueKind::Address => Some(ss::editor_address(l())),
        }
    }

    /// The request, or `None` while one is on its way or the form is refused
    /// (then saying why).
    fn submit(&mut self) -> Option<SchedulingAdminRequest> {
        if self.write.busy() {
            return None;
        }
        self.validation = if self.is_creating() {
            self.form.create_error()
        } else {
            self.form.edit_error()
        };
        if self.validation.is_some() {
            return None;
        }
        self.write = WriteState::Working;
        Some(match &self.slug {
            None => SchedulingAdminRequest::CreateEventType {
                slug: unique_slug(self.form.trimmed_name(), &self.taken),
                fields: self.form.create_fields(),
            },
            Some(slug) => SchedulingAdminRequest::PatchEventType {
                slug: slug.clone(),
                fields: self.form.edit_fields(),
            },
        })
    }

    /// The labels: name and its placeholder, booking page and its hint,
    /// description and its hint, duration and its hint, interval and its hint,
    /// location, the three switches, save, saving and cancel.
    pub fn labels() -> [&'static str; 17] {
        [
            ss::editor_name_label(l()),
            ss::editor_name_placeholder(l()),
            ss::editor_booking_page_label(l()),
            ss::editor_booking_page_hint(l()),
            ss::editor_description_label(l()),
            ss::editor_description_hint(l()),
            ss::editor_duration_label(l()),
            ss::editor_duration_hint(l()),
            ss::editor_interval_label(l()),
            ss::editor_interval_hint(l()),
            ss::editor_location_label(l()),
            ss::editor_active_toggle(l()),
            ss::editor_listed_toggle(l()),
            ss::editor_show_taken_toggle(l()),
            ss::common_save(l()),
            ss::common_saving(l()),
            ss::common_cancel(l()),
        ]
    }

    /// The five counts, as (label, hint): buffer before, buffer after, minimum
    /// notice, how far ahead, active bookings per attendee.
    pub fn count_labels() -> [(&'static str, &'static str); 5] {
        [
            (
                ss::editor_buffer_before_label(l()),
                ss::editor_buffer_before_hint(l()),
            ),
            (
                ss::editor_buffer_after_label(l()),
                ss::editor_buffer_after_hint(l()),
            ),
            (
                ss::editor_min_notice_label(l()),
                ss::editor_min_notice_hint(l()),
            ),
            (
                ss::editor_max_future_label(l()),
                ss::editor_no_limit_hint(l()),
            ),
            (
                ss::editor_max_active_label(l()),
                ss::editor_no_limit_hint(l()),
            ),
        ]
    }
}

/// What the member does on the event types list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventTypesEvent {
    /// Open the create form.
    OpenCreate,
    /// The form's fields changed.
    EditForm(EventTypeForm),
    /// Create it.
    Save,
    /// Close the form.
    CloseEditor,
}

/// The state actions' dialog: on or off, archive or restore, a test email,
/// and delete, asked once more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionsDialog {
    /// The event type, as the last write answered it.
    pub event_type: SchedulingEventType,
    /// The last action's state.
    pub write: WriteState,
    /// Whether the delete is being asked about.
    pub confirming_delete: bool,
}

impl ActionsDialog {
    /// "Turn off" for an active one, "Turn on" otherwise.
    pub fn toggle_label(&self) -> &'static str {
        if self.event_type.is_active != Some(false) {
            ss::actions_turn_off(l())
        } else {
            ss::actions_turn_on(l())
        }
    }

    /// "Restore" for an archived one, "Archive" otherwise.
    pub fn archive_label(&self) -> &'static str {
        if self.event_type.archived == Some(true) {
            ss::actions_restore(l())
        } else {
            ss::actions_archive(l())
        }
    }

    /// The test emails' heading.
    pub fn test_email_heading() -> &'static str {
        ss::actions_send_test_email(l())
    }

    /// The test emails, as (kind sent, label).
    pub fn test_emails() -> [(&'static str, &'static str); 4] {
        [
            ("confirmation", ss::email_template_confirmation(l())),
            ("cancellation", ss::email_template_cancellation(l())),
            ("reschedule", ss::email_template_reschedule(l())),
            ("reminder", ss::email_template_reminder(l())),
        ]
    }

    /// The delete button, and the confirmation's.
    pub fn delete_label() -> &'static str {
        ss::delete_event_type_confirm(l())
    }

    /// The confirmation's heading.
    pub fn delete_title() -> &'static str {
        ss::delete_event_type_title(l())
    }

    /// What the confirmation says survives.
    pub fn delete_body() -> &'static str {
        ss::delete_event_type_body(l())
    }
}

/// One host row of the hosts dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRow {
    /// Their scheduler user id.
    pub user_id: String,
    /// Their name, or their address for a blank one.
    pub name: String,
    /// Their address.
    pub email: String,
    /// `required`, `rotation` or `optional`.
    pub role: String,
    /// Their place in the rotation, as typed.
    pub priority: String,
}

/// The hosts dialog: who takes this event type's bookings, and how they are
/// routed among them. Saved whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostsDialog {
    /// The read of the hosts. No save until it has landed.
    pub load: SectionLoad<()>,
    /// The hosts, as edited.
    pub rows: Vec<HostRow>,
    /// The scheduler users who could be added: not hosts already, not
    /// archived. Empty when they could not be read.
    pub candidates: Vec<SchedulingUser>,
    /// `fixed`, `round_robin` or `collective`.
    pub routing_mode: String,
    /// For round robin: `even`, `soonest` or `priority`.
    pub rotation: String,
    /// Why the hosts cannot be saved, before anything was sent.
    pub validation: Option<&'static str>,
    /// The save's state.
    pub write: WriteState,
}

impl HostsDialog {
    /// Whether to show the rotation's choice: for round robin only.
    pub fn shows_rotation(&self) -> bool {
        self.routing_mode == "round_robin"
    }

    /// Whether the save can be pressed: the hosts were read and nothing is on
    /// its way.
    pub fn can_save(&self) -> bool {
        self.load.value().is_some() && !self.write.busy()
    }

    /// The routing modes, as (wire value, label).
    pub fn routing_modes() -> [(&'static str, &'static str); 3] {
        [
            ("fixed", ss::routing_mode_fixed(l())),
            ("round_robin", ss::routing_mode_round_robin(l())),
            ("collective", ss::routing_mode_collective(l())),
        ]
    }

    /// The rotation strategies, as (wire value, label).
    pub fn rotations() -> [(&'static str, &'static str); 3] {
        [
            ("even", ss::rotation_strategy_even(l())),
            ("soonest", ss::rotation_strategy_soonest(l())),
            ("priority", ss::rotation_strategy_priority(l())),
        ]
    }

    /// The host roles, as (wire value, label).
    pub fn roles() -> [(&'static str, &'static str); 3] {
        [
            ("required", ss::host_role_required(l())),
            ("rotation", ss::host_role_rotation(l())),
            ("optional", ss::host_role_optional(l())),
        ]
    }

    /// The labels: the dialog's title, its empty sentence (nobody can be
    /// booked), the routing's, the rotation's and its hint, the priority's,
    /// "Add" and "Remove".
    pub fn labels() -> [&'static str; 8] {
        [
            ss::hosts_title(l()),
            ss::hosts_empty(l()),
            ss::routing_mode_label(l()),
            ss::rotation_label(l()),
            ss::rotation_hint(l()),
            ss::priority_label(l()),
            ss::common_add(l()),
            ss::common_remove(l()),
        ]
    }
}

/// One booking question as the editor holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuestionForm {
    /// The question's id, for an edit; `None` for a new one.
    pub id: Option<String>,
    /// The question.
    pub label: String,
    /// `text`, `select` or `checkbox`.
    pub kind: String,
    /// A select's choices.
    pub options: Vec<String>,
    /// Whether it must be answered.
    pub required: bool,
    /// Its place, as typed.
    pub position: String,
}

impl QuestionForm {
    /// Why it cannot be sent.
    pub fn error(&self) -> Option<&'static str> {
        if self.label.trim().is_empty() {
            Some(ss::question_label_missing(l()))
        } else if self.kind == "select" && self.options.is_empty() {
            Some(ss::question_needs_option(l()))
        } else if whole(&self.position, 0).is_none() {
            Some(ss::question_position_invalid(l()))
        } else {
            None
        }
    }

    fn fields(&self) -> SchedulingQuestionFields {
        SchedulingQuestionFields {
            label: Some(self.label.trim().to_owned()),
            kind: Some(self.kind.clone()),
            // Choices only for a select; another kind sends none.
            options: (self.kind == "select").then(|| self.options.clone()),
            required: Some(self.required),
            position: whole(&self.position, 0),
        }
    }
}

/// The booking questions dialog: the list, one question's editor, and the
/// delete, asked once more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuestionsDialog {
    /// The questions, in the order the booker sees them.
    pub questions: SectionLoad<Vec<SchedulingQuestion>>,
    /// The question being added or edited.
    pub editing: Option<QuestionForm>,
    /// The question whose delete is being asked about, by id.
    pub confirming_delete: Option<String>,
    /// Why the question cannot be sent, before anything was sent.
    pub validation: Option<&'static str>,
    /// The last write's state.
    pub write: WriteState,
}

impl QuestionsDialog {
    /// "Add question" or "Edit question".
    pub fn editor_title(&self) -> &'static str {
        match self.editing.as_ref().and_then(|form| form.id.as_ref()) {
            None => ss::question_add_title(l()),
            Some(_) => ss::question_edit_title(l()),
        }
    }

    /// The question kinds, as (wire value, label).
    pub fn kinds() -> [(&'static str, &'static str); 3] {
        [
            ("text", ss::question_type_text(l())),
            ("select", ss::question_type_select(l())),
            ("checkbox", ss::question_type_checkbox(l())),
        ]
    }

    /// The labels: the dialog's title, the empty list's sentence, the label's
    /// label and placeholder, the type's, "Required", the position's and its
    /// hint, the options' and their hint and placeholder, "Add option", "Save
    /// question", the delete's title, body and button, "Edit", "Delete".
    pub fn labels() -> [&'static str; 18] {
        [
            ss::questions_title(l()),
            ss::questions_empty(l()),
            ss::question_label_label(l()),
            ss::question_label_placeholder(l()),
            ss::question_type_label(l()),
            ss::question_required_toggle(l()),
            ss::question_position_label(l()),
            ss::question_position_hint(l()),
            ss::question_options_label(l()),
            ss::question_options_hint(l()),
            ss::question_option_placeholder(l()),
            ss::question_add_option(l()),
            ss::question_save_button(l()),
            ss::question_delete_title(l()),
            ss::question_delete_body(l()),
            ss::question_delete_confirm(l()),
            ss::common_edit(l()),
            ss::common_delete(l()),
        ]
    }
}

/// A dialog over one event type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventTypeDialog {
    /// The editor.
    Edit(EventTypeEditor),
    /// The state actions.
    Actions(ActionsDialog),
    /// The hosts.
    Hosts(HostsDialog),
    /// The booking questions.
    Questions(QuestionsDialog),
}

/// One event type: its details, hosts and questions, each read on its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventTypeSection {
    /// Its slug.
    pub slug: String,
    /// The event type.
    pub event_type: SectionLoad<SchedulingEventType>,
    /// Its hosts.
    pub hosts: SectionLoad<Vec<SchedulingHost>>,
    /// Its booking questions.
    pub questions: SectionLoad<Vec<SchedulingQuestion>>,
    /// The dialog open over it.
    pub dialog: Option<EventTypeDialog>,
}

impl EventTypeSection {
    /// The screen's title: the name once read, the slug before.
    pub fn title(&self) -> String {
        self.event_type
            .value()
            .map_or_else(|| self.slug.clone(), |item| item.name.clone())
    }

    /// The details, as (label, value): duration, interval, location, its
    /// value, notice, how far ahead, the buffers (when set) and the state.
    pub fn facts(&self) -> Vec<(&'static str, String)> {
        let Some(item) = self.event_type.value() else {
            return Vec::new();
        };
        let minutes = |m: i64| minutes_label(Some(m));
        let mut facts = vec![(ss::fact_duration(l()), minutes(item.duration_minutes))];
        if let Some(interval) = item.slot_interval_minutes {
            facts.push((ss::fact_interval(l()), minutes(interval)));
        }
        facts.push((
            ss::fact_location(l()),
            location_label(item.location_type.as_deref()),
        ));
        if let Some(value) = item.location_value().filter(|v| !v.is_empty()) {
            facts.push((ss::fact_location_value(l()), value.to_owned()));
        }
        if let Some(notice) = item.min_notice_minutes {
            facts.push((ss::fact_notice(l()), minutes(notice)));
        }
        if let Some(days) = item.max_future_days {
            facts.push((ss::fact_horizon(l()), ss::days_label(l(), days)));
        }
        if let Some(before) = item.buffer_before_minutes.filter(|m| *m > 0) {
            facts.push((ss::fact_buffer_before(l()), minutes(before)));
        }
        if let Some(after) = item.buffer_after_minutes.filter(|m| *m > 0) {
            facts.push((ss::fact_buffer_after(l()), minutes(after)));
        }
        facts.push((ss::fact_state(l()), state_label(item).0.to_owned()));
        facts
    }

    /// The hosts, as (name, role): an archived host is still on the event
    /// type, and says so.
    pub fn host_rows(&self) -> Vec<(String, String)> {
        self.hosts
            .value()
            .map(|hosts| {
                hosts
                    .iter()
                    .map(|host| {
                        let name = if host.name.is_empty() {
                            &host.email
                        } else {
                            &host.name
                        };
                        let role = if host.archived {
                            ss::host_archived(l(), &host.role)
                        } else {
                            host.role.clone()
                        };
                        (name.clone(), role)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The questions in the order the booker sees them, as (question, kind):
    /// "text · required" for a compulsory one.
    pub fn question_rows(&self) -> Vec<(String, String)> {
        let mut questions: Vec<&SchedulingQuestion> = self
            .questions
            .value()
            .map_or_else(Vec::new, |q| q.iter().collect());
        questions.sort_by_key(|q| q.position);
        questions
            .into_iter()
            .map(|q| {
                let kind = if q.required {
                    ss::question_required(l(), &q.kind)
                } else {
                    q.kind.clone()
                };
                (q.label.clone(), kind)
            })
            .collect()
    }

    /// The headings: details, hosts, questions.
    pub fn headings() -> [&'static str; 3] {
        [
            ss::event_type_details_eyebrow(l()),
            ss::event_type_hosts_eyebrow(l()),
            ss::event_type_questions_eyebrow(l()),
        ]
    }

    /// Said with no host, and with no question.
    pub fn empties() -> [&'static str; 2] {
        [
            ss::event_type_no_hosts(l()),
            ss::event_type_no_questions(l()),
        ]
    }

    /// The write buttons: "Edit", "Manage", "Hosts", "Questions".
    pub fn write_labels() -> [&'static str; 4] {
        [
            ss::common_edit(l()),
            ss::actions_manage(l()),
            ss::hosts_title(l()),
            ss::questions_title(l()),
        ]
    }
}

/// What the member does on one event type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventTypeEvent {
    /// Open the editor, once the event type is read.
    OpenEditor,
    /// The editor's fields changed.
    EditForm(EventTypeForm),
    /// Save the editor.
    Save,
    /// Open the state actions.
    OpenActions,
    /// Turn it on or off.
    ToggleActive,
    /// Archive or restore it.
    ToggleArchived,
    /// Send a test email of this kind to the member.
    SendTestEmail(String),
    /// Ask about deleting it.
    AskDelete,
    /// Do not delete it.
    CancelDelete,
    /// Delete it, and go back to the list.
    ConfirmDelete,
    /// Open the hosts.
    OpenHosts,
    /// The routing mode changed.
    SetRoutingMode(String),
    /// The rotation changed.
    SetRotation(String),
    /// Add a candidate as a host, by scheduler user id.
    AddHost(String),
    /// Take a host off.
    RemoveHost(String),
    /// A host's role changed.
    SetHostRole {
        /// The host's id.
        user_id: String,
        /// The role.
        role: String,
    },
    /// A host's priority changed.
    SetHostPriority {
        /// The host's id.
        user_id: String,
        /// The priority, as typed.
        priority: String,
    },
    /// Save the hosts.
    SaveHosts,
    /// Open the questions.
    OpenQuestions,
    /// Start a new question, placed last.
    AddQuestion,
    /// Edit a question, by id.
    EditQuestion(String),
    /// The question's fields changed.
    EditQuestionForm(QuestionForm),
    /// Add a select's choice.
    AddOption(String),
    /// Take a choice off.
    RemoveOption(String),
    /// Close the question's editor.
    CancelQuestion,
    /// Save the question.
    SaveQuestion,
    /// Ask about deleting a question, by id.
    AskDeleteQuestion(String),
    /// Do not delete it.
    CancelDeleteQuestion,
    /// Delete it.
    ConfirmDeleteQuestion,
    /// Put away the last outcome.
    DismissNotice,
    /// Close the dialog.
    CloseDialog,
}

/// What an answer the list awaits is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    List,
    Create,
}

/// What an answer one event type awaits is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DetailPending {
    EventType,
    Hosts,
    Questions,
    Save,
    Action(Action),
    DialogHosts,
    Candidates,
    PutHosts {
        routing_mode: String,
        rotation: String,
    },
    Routing,
    DialogQuestions,
    QuestionWrite(&'static str),
}

/// A state action whose answer is awaited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Toggle { on: bool },
    Archive { archived: bool },
    TestEmail,
    Delete,
}

fn send(pending: Pending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::EventTypes(pending), request)
}

fn send_detail(pending: DetailPending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::EventType(pending), request)
}

pub(crate) fn enter(admin: &mut SchedulingAdmin, tickets: &mut Tickets) -> Vec<Out> {
    admin.forget(tickets, |p| matches!(p, AdminPending::EventTypes(_)));
    admin.event_types = Some(EventTypesSection::default());
    vec![send(Pending::List, SchedulingAdminRequest::EventTypes)]
}

/// The status, for the booking links.
pub(crate) fn status_arrived(
    admin: &mut SchedulingAdmin,
    status: Option<&SchedulingStatusResponse>,
) {
    if let Some(section) = admin.event_types.as_mut() {
        section.public_host = status
            .and_then(|s| s.tenant.as_ref())
            .map(|t| t.public_host.clone());
    }
}

pub(crate) fn event(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: EventTypesEvent,
    can_change: bool,
) -> Vec<Out> {
    let Some(section) = admin.event_types.as_mut() else {
        return Vec::new();
    };
    match event {
        EventTypesEvent::OpenCreate if can_change => {
            if let Some(items) = section.list.value() {
                let taken = items.iter().map(|i| i.slug.clone()).collect();
                section.editor = Some(EventTypeEditor::create(taken));
            }
            Vec::new()
        }
        EventTypesEvent::EditForm(form) => {
            if let Some(editor) = &mut section.editor {
                editor.form = form;
            }
            Vec::new()
        }
        EventTypesEvent::Save => section
            .editor
            .as_mut()
            .and_then(EventTypeEditor::submit)
            .map(|request| vec![send(Pending::Create, request)])
            .unwrap_or_default(),
        EventTypesEvent::CloseEditor => {
            section.editor = None;
            Vec::new()
        }
        EventTypesEvent::OpenCreate => Vec::new(),
    }
}

pub(crate) fn answered(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    pending: Pending,
    answer: Answer,
) -> Vec<Out> {
    admin
        .event_types
        .as_mut()
        .map(|section| match (pending, answer) {
            (Pending::List, answer) => {
                section.list = load(answer, data!(EventTypes));
                Vec::new()
            }
            // A create closes the form and reads the list again; a refusal
            // keeps the form, with why.
            (Pending::Create, Ok(_)) => {
                section.editor = None;
                section.list = SectionLoad::Loading;
                vec![send(Pending::List, SchedulingAdminRequest::EventTypes)]
            }
            (Pending::Create, Err(failure)) => {
                if let Some(editor) = &mut section.editor {
                    editor.write = WriteState::Failed(failure);
                }
                Vec::new()
            }
        })
        .unwrap_or_default()
}

/// The three reads of one event type.
fn detail_reads(slug: &str) -> Vec<Out> {
    let slug = || slug.to_owned();
    vec![
        send_detail(
            DetailPending::EventType,
            SchedulingAdminRequest::EventType { slug: slug() },
        ),
        send_detail(
            DetailPending::Hosts,
            SchedulingAdminRequest::Hosts { slug: slug() },
        ),
        send_detail(
            DetailPending::Questions,
            SchedulingAdminRequest::Questions { slug: slug() },
        ),
    ]
}

pub(crate) fn enter_detail(
    admin: &mut SchedulingAdmin,
    tickets: &mut Tickets,
    slug: String,
) -> Vec<Out> {
    admin.forget(tickets, |p| matches!(p, AdminPending::EventType(_)));
    let outs = detail_reads(&slug);
    admin.event_type = Some(EventTypeSection {
        slug,
        event_type: SectionLoad::Loading,
        hosts: SectionLoad::Loading,
        questions: SectionLoad::Loading,
        dialog: None,
    });
    outs
}

/// The detail read again, its dialog left as it is.
fn reread(section: &mut EventTypeSection) -> Vec<Out> {
    section.event_type = SectionLoad::Loading;
    section.hosts = SectionLoad::Loading;
    section.questions = SectionLoad::Loading;
    detail_reads(&section.slug)
}

pub(crate) fn detail_event(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: EventTypeEvent,
    can_change: bool,
) -> Vec<Out> {
    let Some(section) = admin.event_type.as_mut() else {
        return Vec::new();
    };
    let slug = section.slug.clone();
    // Every dialog needs a member who may change things, and the event type
    // read.
    let loaded = section.event_type.value().cloned().filter(|_| can_change);
    match (event, &mut section.dialog, loaded) {
        (EventTypeEvent::OpenEditor, dialog, Some(row)) => {
            *dialog = Some(EventTypeDialog::Edit(EventTypeEditor::edit(&row)));
            Vec::new()
        }
        (EventTypeEvent::OpenActions, dialog, Some(event_type)) => {
            *dialog = Some(EventTypeDialog::Actions(ActionsDialog {
                event_type,
                write: WriteState::Idle,
                confirming_delete: false,
            }));
            Vec::new()
        }
        (EventTypeEvent::OpenHosts, dialog, Some(row)) => {
            *dialog = Some(EventTypeDialog::Hosts(HostsDialog {
                load: SectionLoad::Loading,
                rows: Vec::new(),
                candidates: Vec::new(),
                routing_mode: row.routing_mode.unwrap_or_else(|| "fixed".to_owned()),
                rotation: row.rr_strategy.unwrap_or_else(|| "even".to_owned()),
                validation: None,
                write: WriteState::Idle,
            }));
            vec![send_detail(
                DetailPending::DialogHosts,
                SchedulingAdminRequest::Hosts { slug },
            )]
        }
        (EventTypeEvent::OpenQuestions, dialog, Some(_)) => {
            *dialog = Some(EventTypeDialog::Questions(QuestionsDialog {
                questions: SectionLoad::Loading,
                editing: None,
                confirming_delete: None,
                validation: None,
                write: WriteState::Idle,
            }));
            vec![send_detail(
                DetailPending::DialogQuestions,
                SchedulingAdminRequest::Questions { slug },
            )]
        }
        (EventTypeEvent::CloseDialog, dialog, _) => {
            *dialog = None;
            Vec::new()
        }
        (EventTypeEvent::DismissNotice, Some(dialog), _) => {
            dismiss(dialog);
            Vec::new()
        }
        (event, Some(EventTypeDialog::Edit(editor)), _) => editor_event(editor, event),
        (event, Some(EventTypeDialog::Actions(actions)), _) => actions_event(actions, event, slug),
        (event, Some(EventTypeDialog::Hosts(hosts)), _) => hosts_event(hosts, event, slug),
        (event, Some(EventTypeDialog::Questions(questions)), _) => {
            questions_event(questions, event, slug)
        }
        _ => Vec::new(),
    }
}

fn editor_event(editor: &mut EventTypeEditor, event: EventTypeEvent) -> Vec<Out> {
    match event {
        EventTypeEvent::EditForm(form) => {
            editor.form = form;
            Vec::new()
        }
        EventTypeEvent::Save => editor
            .submit()
            .map(|request| vec![send_detail(DetailPending::Save, request)])
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn patch(slug: String, fields: SchedulingEventTypeFields) -> SchedulingAdminRequest {
    SchedulingAdminRequest::PatchEventType { slug, fields }
}

/// The state actions, one at a time.
fn actions_event(actions: &mut ActionsDialog, event: EventTypeEvent, slug: String) -> Vec<Out> {
    if actions.write.busy() {
        return Vec::new();
    }
    let (action, request) = match event {
        EventTypeEvent::ToggleActive => {
            let on = actions.event_type.is_active == Some(false);
            let fields = SchedulingEventTypeFields {
                is_active: Some(on),
                ..SchedulingEventTypeFields::default()
            };
            (Action::Toggle { on }, patch(slug, fields))
        }
        EventTypeEvent::ToggleArchived => {
            let archived = actions.event_type.archived != Some(true);
            let fields = SchedulingEventTypeFields {
                archived: Some(archived),
                ..SchedulingEventTypeFields::default()
            };
            (Action::Archive { archived }, patch(slug, fields))
        }
        EventTypeEvent::SendTestEmail(kind) => (
            Action::TestEmail,
            SchedulingAdminRequest::SendTestEmail { slug, kind },
        ),
        EventTypeEvent::AskDelete | EventTypeEvent::CancelDelete => {
            actions.confirming_delete = event == EventTypeEvent::AskDelete;
            return Vec::new();
        }
        EventTypeEvent::ConfirmDelete if actions.confirming_delete => {
            actions.confirming_delete = false;
            (
                Action::Delete,
                SchedulingAdminRequest::DeleteEventType { slug },
            )
        }
        _ => return Vec::new(),
    };
    actions.write = WriteState::Working;
    vec![send_detail(DetailPending::Action(action), request)]
}

fn hosts_event(hosts: &mut HostsDialog, event: EventTypeEvent, slug: String) -> Vec<Out> {
    match event {
        EventTypeEvent::SetRoutingMode(mode) => hosts.routing_mode = mode,
        EventTypeEvent::SetRotation(rotation) => hosts.rotation = rotation,
        EventTypeEvent::AddHost(id) => {
            if let Some(index) = hosts.candidates.iter().position(|u| u.id == id) {
                let user = hosts.candidates.remove(index);
                hosts.rows.push(HostRow {
                    name: if user.name.is_empty() {
                        user.email.clone()
                    } else {
                        user.name
                    },
                    user_id: user.id,
                    email: user.email,
                    role: "required".to_owned(),
                    priority: "0".to_owned(),
                });
            }
        }
        EventTypeEvent::RemoveHost(id) => hosts.rows.retain(|row| row.user_id != id),
        EventTypeEvent::SetHostRole { user_id, role } => {
            if let Some(row) = host_row(hosts, &user_id) {
                row.role = role;
            }
        }
        EventTypeEvent::SetHostPriority { user_id, priority } => {
            if let Some(row) = host_row(hosts, &user_id) {
                row.priority = priority;
            }
        }
        EventTypeEvent::SaveHosts if hosts.can_save() => return save_hosts(hosts, slug),
        _ => {}
    }
    Vec::new()
}

fn host_row<'a>(hosts: &'a mut HostsDialog, id: &str) -> Option<&'a mut HostRow> {
    hosts.rows.iter_mut().find(|r| r.user_id == id)
}

/// The hosts' save: one host at least, each priority a whole number, then the
/// whole list put and the routing patched after it.
fn save_hosts(hosts: &mut HostsDialog, slug: String) -> Vec<Out> {
    let assignments: Option<Vec<SchedulingHostAssignment>> = hosts
        .rows
        .iter()
        .map(|row| {
            whole(&row.priority, 0).map(|priority| SchedulingHostAssignment {
                user_id: row.user_id.clone(),
                role: row.role.clone(),
                priority,
            })
        })
        .collect();
    hosts.validation = if hosts.rows.is_empty() {
        Some(ss::hosts_need_one(l()))
    } else if assignments.is_none() {
        Some(ss::host_priority_invalid(l()))
    } else {
        None
    };
    match assignments.filter(|_| hosts.validation.is_none()) {
        Some(assignments) => {
            hosts.write = WriteState::Working;
            vec![send_detail(
                DetailPending::PutHosts {
                    routing_mode: hosts.routing_mode.clone(),
                    rotation: hosts.rotation.clone(),
                },
                SchedulingAdminRequest::SetHosts {
                    slug,
                    hosts: assignments,
                },
            )]
        }
        None => Vec::new(),
    }
}

fn questions_event(
    questions: &mut QuestionsDialog,
    event: EventTypeEvent,
    slug: String,
) -> Vec<Out> {
    match event {
        EventTypeEvent::AddQuestion => {
            let count = questions.questions.value().map_or(0, Vec::len);
            questions.editing = Some(QuestionForm {
                kind: "text".to_owned(),
                position: count.to_string(),
                ..QuestionForm::default()
            });
            questions.validation = None;
        }
        EventTypeEvent::EditQuestion(id) => {
            let found = questions
                .questions
                .value()
                .and_then(|qs| qs.iter().find(|q| q.id == id))
                .map(|q| QuestionForm {
                    id: Some(q.id.clone()),
                    label: q.label.clone(),
                    kind: q.kind.clone(),
                    options: q.options().to_vec(),
                    required: q.required,
                    position: q.position.to_string(),
                });
            if found.is_some() {
                questions.editing = found;
                questions.validation = None;
            }
        }
        EventTypeEvent::EditQuestionForm(form) => {
            if questions.editing.is_some() {
                questions.editing = Some(form);
            }
        }
        EventTypeEvent::AddOption(option) => {
            let option = option.trim().to_owned();
            if let Some(form) = &mut questions.editing
                && !option.is_empty()
                && !form.options.contains(&option)
            {
                form.options.push(option);
            }
        }
        EventTypeEvent::RemoveOption(option) => {
            if let Some(form) = &mut questions.editing {
                form.options.retain(|o| *o != option);
            }
        }
        EventTypeEvent::CancelQuestion => {
            questions.editing = None;
            questions.validation = None;
        }
        EventTypeEvent::SaveQuestion if !questions.write.busy() => {
            return save_question(questions, slug);
        }
        EventTypeEvent::AskDeleteQuestion(id) => questions.confirming_delete = Some(id),
        EventTypeEvent::CancelDeleteQuestion => questions.confirming_delete = None,
        EventTypeEvent::ConfirmDeleteQuestion if !questions.write.busy() => {
            if let Some(id) = questions.confirming_delete.take() {
                questions.write = WriteState::Working;
                return vec![send_detail(
                    DetailPending::QuestionWrite(ss::question_deleted(l())),
                    SchedulingAdminRequest::DeleteQuestion { slug, id },
                )];
            }
        }
        _ => {}
    }
    Vec::new()
}

fn save_question(questions: &mut QuestionsDialog, slug: String) -> Vec<Out> {
    let Some(form) = questions.editing.as_ref() else {
        return Vec::new();
    };
    questions.validation = form.error();
    if questions.validation.is_some() {
        return Vec::new();
    }
    let fields = form.fields();
    let (notice, request) = match &form.id {
        None => (
            ss::question_added(l()),
            SchedulingAdminRequest::CreateQuestion { slug, fields },
        ),
        Some(id) => (
            ss::question_saved(l()),
            SchedulingAdminRequest::PatchQuestion {
                slug,
                id: id.clone(),
                fields,
            },
        ),
    };
    questions.write = WriteState::Working;
    vec![send_detail(DetailPending::QuestionWrite(notice), request)]
}

/// Puts away a dialog's last outcome and its validation.
fn dismiss(dialog: &mut EventTypeDialog) {
    match dialog {
        EventTypeDialog::Edit(editor) => {
            editor.validation = None;
            editor.write = WriteState::Idle;
        }
        EventTypeDialog::Actions(actions) => actions.write = WriteState::Idle,
        EventTypeDialog::Hosts(hosts) => {
            hosts.validation = None;
            hosts.write = WriteState::Idle;
        }
        EventTypeDialog::Questions(questions) => {
            questions.validation = None;
            questions.write = WriteState::Idle;
        }
    }
}

pub(crate) fn detail_answered(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    pending: DetailPending,
    answer: Answer,
) -> Vec<Out> {
    admin
        .event_type
        .as_mut()
        .map(|section| detail_answer(section, pending, answer))
        .unwrap_or_default()
}

fn detail_answer(
    section: &mut EventTypeSection,
    pending: DetailPending,
    answer: Answer,
) -> Vec<Out> {
    let slug = section.slug.clone();
    match pending {
        DetailPending::EventType => section.event_type = load(answer, data!(EventType)),
        DetailPending::Hosts => section.hosts = load(answer, data!(Hosts)),
        DetailPending::Questions => section.questions = load(answer, data!(Questions)),
        // The editor closes on a save, and the event type is read again.
        DetailPending::Save => match answer {
            Ok(_) => {
                section.dialog = None;
                return reread(section);
            }
            Err(failure) => {
                if let Some(EventTypeDialog::Edit(editor)) = &mut section.dialog {
                    editor.write = WriteState::Failed(failure);
                }
            }
        },
        DetailPending::Action(action) => return action_answered(section, action, answer),
        DetailPending::DialogHosts => {
            if let Some(EventTypeDialog::Hosts(hosts)) = &mut section.dialog {
                match answer.and_then(|data| data!(Hosts)(data).ok_or_else(FailureText::unexpected))
                {
                    Ok(list) => {
                        hosts.rows = list
                            .into_iter()
                            .map(|host| HostRow {
                                name: if host.name.is_empty() {
                                    host.email.clone()
                                } else {
                                    host.name
                                },
                                user_id: host.user_id,
                                email: host.email,
                                role: host.role,
                                priority: host.priority.to_string(),
                            })
                            .collect();
                        hosts.load = SectionLoad::Ready(());
                        return vec![send_detail(
                            DetailPending::Candidates,
                            SchedulingAdminRequest::Users {
                                include_archived: None,
                            },
                        )];
                    }
                    Err(failure) => hosts.load = SectionLoad::Failed(failure),
                }
            }
        }
        // Who could be added: nobody when the users could not be read.
        DetailPending::Candidates => {
            if let Some(EventTypeDialog::Hosts(hosts)) = &mut section.dialog {
                let assigned: Vec<String> = hosts.rows.iter().map(|r| r.user_id.clone()).collect();
                hosts.candidates = match answer {
                    Ok(SchedulingAdminData::Users(users)) => users
                        .into_iter()
                        .filter(|u| !u.archived && !assigned.contains(&u.id))
                        .collect(),
                    _ => Vec::new(),
                };
            }
        }
        // The hosts went in: now the routing, which they are routed by.
        DetailPending::PutHosts {
            routing_mode,
            rotation,
        } => match answer {
            Ok(_) => {
                let fields = SchedulingEventTypeFields {
                    routing_mode: Some(routing_mode),
                    rr_strategy: Some(rotation),
                    ..SchedulingEventTypeFields::default()
                };
                return vec![send_detail(DetailPending::Routing, patch(slug, fields))];
            }
            Err(failure) => hosts_failed(section, failure),
        },
        // Both saved: the dialog closes and the event type is read again.
        DetailPending::Routing => match answer {
            Ok(_) => {
                section.dialog = None;
                return reread(section);
            }
            Err(failure) => hosts_failed(section, failure),
        },
        DetailPending::DialogQuestions => {
            let questions = load(answer, |data| {
                data!(Questions)(data).map(|mut questions| {
                    questions.sort_by_key(|q| q.position);
                    questions
                })
            });
            if let SectionLoad::Ready(list) = &questions {
                section.questions = SectionLoad::Ready(list.clone());
            }
            if let Some(EventTypeDialog::Questions(dialog)) = &mut section.dialog {
                dialog.questions = questions;
            }
        }
        // A question written: its editor closes and the list is read again,
        // which the event type's own list follows.
        DetailPending::QuestionWrite(notice) => {
            if let Some(EventTypeDialog::Questions(dialog)) = &mut section.dialog {
                match answer {
                    Ok(_) => {
                        dialog.editing = None;
                        dialog.write = WriteState::Done(notice.to_owned());
                        return vec![send_detail(
                            DetailPending::DialogQuestions,
                            SchedulingAdminRequest::Questions { slug },
                        )];
                    }
                    Err(failure) => dialog.write = WriteState::Failed(failure),
                }
            }
        }
    }
    Vec::new()
}

fn hosts_failed(section: &mut EventTypeSection, failure: FailureText) {
    if let Some(EventTypeDialog::Hosts(hosts)) = &mut section.dialog {
        hosts.write = WriteState::Failed(failure);
    }
}

/// A state action landed or failed. A delete goes back to the list (read
/// again as it opens); a change of state says so, keeps the dialog on the
/// event type as answered, and reads the event type again.
fn action_answered(section: &mut EventTypeSection, action: Action, answer: Answer) -> Vec<Out> {
    if action == Action::Delete && answer.is_ok() {
        section.dialog = None;
        return vec![Out::Show(Route::SchedulingAdmin(
            SchedulingSection::EventTypes,
        ))];
    }
    let notice = match (&action, &answer) {
        (Action::Toggle { on: true }, _) => ss::actions_turned_on(l()).to_owned(),
        (Action::Toggle { on: false }, _) => ss::actions_turned_off(l()).to_owned(),
        (Action::Archive { archived: true }, _) => ss::actions_archived(l()).to_owned(),
        (Action::Archive { archived: false }, _) => ss::actions_restored(l()).to_owned(),
        (_, Ok(SchedulingAdminData::TestEmail(sent))) if sent.sent => {
            ss::test_email_sent(l(), &sent.to)
        }
        _ => ss::test_email_not_sent(l()).to_owned(),
    };
    let changed = matches!(action, Action::Toggle { .. } | Action::Archive { .. });
    if let Some(EventTypeDialog::Actions(actions)) = &mut section.dialog {
        match &answer {
            Ok(data) => {
                if let SchedulingAdminData::EventType(row) = data {
                    actions.event_type = row.clone();
                }
                actions.write = WriteState::Done(notice);
            }
            Err(failure) => actions.write = WriteState::Failed(failure.clone()),
        }
    }
    if changed && answer.is_ok() {
        reread(section)
    } else {
        Vec::new()
    }
}
