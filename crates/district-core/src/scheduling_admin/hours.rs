//! Working hours: the member's weekly windows and the dated overrides of them,
//! each with its editor.
//!
//! The week is saved as the web saves it: the edited week against the rules it
//! was read from, as deletes, then changes, then creates, one at a time,
//! stopping at the first refusal. Whatever happened, the rules are read again
//! after, because a save that stopped half way has changed some of them, and
//! the next save must start from what the scheduler holds.
//!
//! Only the rules for every event type are the member's hours; one event
//! type's own rules are left alone. A day off that is part of a run is deleted
//! with its run, and the confirmation says how many days that is.

use std::time::SystemTime;

use district_l10n::scheduling_strings as ss;
use district_model::{
    SchedulingAdminData, SchedulingAdminRequest, SchedulingAvailabilityOverride,
    SchedulingAvailabilityRule, SchedulingOverrideDraft,
};

use super::clock::{self, SchedulingDay};
use super::overview::{display_index, load, summarize_working_hours};
use super::{Answer, Out, Pending as AdminPending, SchedulingAdmin, SectionLoad, WriteState};
use crate::failure::FailureText;
use crate::model::Tickets;
use crate::text::l;

/// A new window's opening time.
pub const DEFAULT_START: &str = "09:00";
/// A new window's closing time.
pub const DEFAULT_END: &str = "17:00";

/// A weekday's full name, Monday first.
pub fn weekday_name(display_index: usize) -> &'static str {
    match display_index {
        0 => ss::weekday_monday(l()),
        1 => ss::weekday_tuesday(l()),
        2 => ss::weekday_wednesday(l()),
        3 => ss::weekday_thursday(l()),
        4 => ss::weekday_friday(l()),
        5 => ss::weekday_saturday(l()),
        _ => ss::weekday_sunday(l()),
    }
}

/// The wire's day (0 Sunday) for a display row (0 Monday).
fn wire_day(row: usize) -> i64 {
    ((row + 1) % 7) as i64
}

/// `HH:MM`, zero-padded, as the scheduler takes it.
fn well_formed_time(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 5 && bytes[2] == b':' && [0, 1, 3, 4].iter().all(|i| bytes[*i].is_ascii_digit())
}

/// `YYYY-MM-DD`, as the scheduler takes it.
fn well_formed_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|i| bytes[*i].is_ascii_digit())
}

/// One window of a day, as the week editor holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoursRange {
    /// Which window this is, for the edits that name it.
    pub key: u64,
    /// The rule it was read from; `None` for a new one.
    pub rule_id: Option<String>,
    /// When it opens, as typed.
    pub start: String,
    /// When it closes, as typed.
    pub end: String,
}

/// One row of the dated overrides: a day, or a run of days made together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverrideRow {
    /// The row's key, for the delete that names it.
    pub key: String,
    /// Whether it is a run, deleted whole.
    pub is_run: bool,
    /// The id a delete addresses: the day's, or the run's.
    pub target: String,
    /// The first day, `YYYY-MM-DD`.
    pub start: String,
    /// The last day.
    pub end: String,
    /// How many days.
    pub days: i64,
    /// "Sep 24, 2026", or "Dec 24, 2026 to Dec 31, 2026".
    pub dates: String,
    /// "12:00 p.m. to 4:00 p.m.", "Out of office" or "Unavailable".
    pub hours: String,
}

/// The overrides that end today or later, a run as one row, by first day.
fn upcoming(
    overrides: &[SchedulingAvailabilityOverride],
    today: &str,
    zone: &str,
) -> Vec<OverrideRow> {
    let mut rows: Vec<(OverrideRow, &SchedulingAvailabilityOverride)> = Vec::new();
    for item in overrides {
        let group = item.group_id.as_deref().filter(|g| !g.is_empty());
        if let Some(index) =
            group.and_then(|g| rows.iter().position(|(r, _)| r.is_run && r.target == g))
        {
            let row = &mut rows[index].0;
            row.start = row.start.clone().min(item.date.clone());
            row.end = row.end.clone().max(item.date.clone());
            row.days += 1;
            continue;
        }
        let row = OverrideRow {
            key: match group {
                Some(g) => format!("group:{g}"),
                None => format!("id:{}", item.id),
            },
            is_run: group.is_some(),
            target: group.unwrap_or(&item.id).to_owned(),
            start: item.date.clone(),
            end: item.date.clone(),
            days: 1,
            dates: String::new(),
            hours: String::new(),
        };
        rows.push((row, item));
    }
    let mut rows: Vec<OverrideRow> = rows
        .into_iter()
        .filter(|(row, _)| row.end.as_str() >= today)
        .map(|(mut row, first)| {
            row.dates = override_dates(&row.start, &row.end);
            row.hours = override_hours(first, zone);
            row
        })
        .collect();
    // Stable: two that start the same day keep the scheduler's order.
    rows.sort_by(|a, b| a.start.cmp(&b.start));
    rows
}

/// A day as a reader reads it, or as it came when it is not a day.
fn day_label(key: &str) -> String {
    SchedulingDay::parse(key).map_or_else(|| key.to_owned(), SchedulingDay::label)
}

fn override_dates(start: &str, end: &str) -> String {
    if start == end {
        day_label(start)
    } else {
        ss::hours_date_range(l(), &day_label(start), &day_label(end))
    }
}

/// The hours of a day with custom hours, else why it has none.
fn override_hours(item: &SchedulingAvailabilityOverride, zone: &str) -> String {
    let start = item.start_time.as_deref().unwrap_or_default();
    let end = item.end_time.as_deref().unwrap_or_default();
    if item.is_available && !start.is_empty() && !end.is_empty() {
        ss::hours_time_range(
            l(),
            &clock::hour_minute(start, zone),
            &clock::hour_minute(end, zone),
        )
    } else if item.reason == "out_of_office" {
        ss::hours_out_of_office(l()).to_owned()
    } else {
        ss::hours_unavailable(l()).to_owned()
    }
}

/// The week from the rules for every event type, Monday first, each day's
/// windows by opening time. A rule on a day outside the week is left out, as
/// the web's grid leaves it.
fn week_from_rules(rules: &[SchedulingAvailabilityRule]) -> Vec<Vec<&SchedulingAvailabilityRule>> {
    let mut week: Vec<Vec<&SchedulingAvailabilityRule>> = vec![Vec::new(); 7];
    for rule in rules
        .iter()
        .filter(|r| r.event_type_id.is_none() && (0..7).contains(&r.day_of_week))
    {
        week[display_index(rule.day_of_week)].push(rule);
    }
    for day in &mut week {
        day.sort_by(|a, b| a.start_time.cmp(&b.start_time));
    }
    week
}

/// The working hours section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoursSection {
    /// The zone the hours are kept in: the profile's, else UTC.
    pub timezone: String,
    /// The rules.
    pub rules: SectionLoad<Vec<SchedulingAvailabilityRule>>,
    /// The dated overrides, all of them.
    pub overrides: SectionLoad<Vec<SchedulingAvailabilityOverride>>,
    /// The editor open over the section.
    pub dialog: Option<HoursDialog>,
}

impl HoursSection {
    /// "Times in America/Toronto".
    pub fn times_in(&self) -> String {
        clock::times_in(&self.timezone)
    }

    /// "Mon to Fri, 9:00 a.m. to 5:00 p.m.", or that no day is bookable yet.
    pub fn summary(&self) -> String {
        self.rules
            .value()
            .and_then(|rules| summarize_working_hours(rules, &self.timezone))
            .unwrap_or_else(|| ss::hours_none_at_all(l()).to_owned())
    }

    /// The week, Monday first, as (day, hours): "9:00 a.m. to 5:00 p.m." or
    /// "Not bookable". Empty until the rules are read.
    pub fn week(&self) -> Vec<(&'static str, String)> {
        let Some(rules) = self.rules.value() else {
            return Vec::new();
        };
        week_from_rules(rules)
            .iter()
            .enumerate()
            .map(|(day, windows)| {
                let hours = if windows.is_empty() {
                    ss::day_not_bookable(l()).to_owned()
                } else {
                    windows
                        .iter()
                        .map(|w| {
                            ss::hours_range(
                                l(),
                                &clock::hour_minute(&w.start_time, &self.timezone),
                                &clock::hour_minute(&w.end_time, &self.timezone),
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                (weekday_name(day), hours)
            })
            .collect()
    }

    /// The overrides still to come at `now`, in the profile's zone.
    pub fn override_rows(&self, now: SystemTime) -> Vec<OverrideRow> {
        let today = SchedulingDay::today(now, &self.timezone).key();
        self.overrides
            .value()
            .map(|items| upcoming(items, &today, &self.timezone))
            .unwrap_or_default()
    }

    /// The headings and words around the two lists: the overrides' heading,
    /// what they are, and their empty sentence; and the two buttons, "Your
    /// working hours" and "Date overrides".
    pub fn labels() -> [&'static str; 5] {
        [
            ss::overrides_eyebrow(l()),
            ss::overrides_subtitle(l()),
            ss::overrides_empty(l()),
            ss::hours_title(l()),
            ss::overrides_title(l()),
        ]
    }
}

/// A dialog over the working hours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HoursDialog {
    /// The week's editor.
    Week(WeekEditor),
    /// The overrides' editor.
    Overrides(OverridesEditor),
}

/// The week's editor: each day's windows, saved as the difference from what
/// was read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeekEditor {
    /// The rules' read, which the save is measured from. No save until it has
    /// landed.
    pub load: SectionLoad<()>,
    /// The week, Monday first.
    pub days: Vec<Vec<HoursRange>>,
    /// The save's state.
    pub write: WriteState,
    stored: Vec<SchedulingAvailabilityRule>,
    queue: Vec<SchedulingAdminRequest>,
    next_key: u64,
}

impl WeekEditor {
    /// Each window of `day`'s fault, or `None`: a time not `HH:MM`, an end not
    /// after its start, or a window that overlaps the one before it.
    pub fn errors(&self, day: usize) -> Vec<Option<&'static str>> {
        let ranges = self.days.get(day).map_or(&[][..], Vec::as_slice);
        let mut errors: Vec<Option<&'static str>> = ranges
            .iter()
            .map(|r| {
                if !well_formed_time(&r.start) || !well_formed_time(&r.end) {
                    Some(ss::time_format_error(l()))
                } else if r.end <= r.start {
                    Some(ss::time_order_error(l()))
                } else {
                    None
                }
            })
            .collect();
        let mut order: Vec<usize> = (0..ranges.len()).collect();
        order.sort_by(|a, b| ranges[*a].start.cmp(&ranges[*b].start));
        for pair in order.windows(2) {
            let (earlier, index) = (pair[0], pair[1]);
            if errors[earlier].is_none()
                && errors[index].is_none()
                && ranges[index].start < ranges[earlier].end
            {
                errors[index] = Some(ss::time_overlap_error(l()));
            }
        }
        errors
    }

    /// Whether the save can be pressed: read, every window sound, nothing on
    /// its way.
    pub fn can_save(&self) -> bool {
        self.load.value().is_some()
            && !self.write.busy()
            && (0..7).all(|day| self.errors(day).iter().all(Option::is_none))
    }

    /// Whether a day may offer "Copy to all weekdays": Monday to Friday, with
    /// a window to copy.
    pub fn offers_copy(&self, day: usize) -> bool {
        day < 5 && self.days.get(day).is_some_and(|d| !d.is_empty())
    }

    /// The labels: the editor's title, "Not bookable", "Add range", "Copy to
    /// all weekdays", "to", "Remove", "Save", "Cancel".
    pub fn labels() -> [&'static str; 8] {
        [
            ss::hours_title(l()),
            ss::hours_not_bookable(l()),
            ss::hours_add_range(l()),
            ss::hours_copy_to_weekdays(l()),
            ss::range_to(l()),
            ss::common_remove(l()),
            ss::common_save(l()),
            ss::common_cancel(l()),
        ]
    }

    fn key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key
    }

    fn fill(&mut self, rules: Vec<SchedulingAvailabilityRule>) {
        let week: Vec<Vec<(String, String, String)>> = week_from_rules(&rules)
            .into_iter()
            .map(|day| {
                day.into_iter()
                    .map(|r| (r.id.clone(), r.start_time.clone(), r.end_time.clone()))
                    .collect()
            })
            .collect();
        self.days = week
            .into_iter()
            .map(|day| {
                day.into_iter()
                    .map(|(id, start, end)| HoursRange {
                        key: self.key(),
                        rule_id: Some(id),
                        start,
                        end,
                    })
                    .collect()
            })
            .collect();
        self.stored = rules;
        self.load = SectionLoad::Ready(());
    }

    /// What the save sends: deletes, changes, creates. A window carrying the id
    /// of a rule no longer read is created anew rather than lost.
    fn diff(&self) -> Vec<SchedulingAdminRequest> {
        let global: Vec<&SchedulingAvailabilityRule> = self
            .stored
            .iter()
            .filter(|r| r.event_type_id.is_none())
            .collect();
        let (mut patches, mut creates, mut kept) = (Vec::new(), Vec::new(), Vec::new());
        for (row, ranges) in self.days.iter().enumerate() {
            let day = wire_day(row);
            for range in ranges {
                let before = range
                    .rule_id
                    .as_ref()
                    .and_then(|id| global.iter().find(|r| r.id == *id));
                match before {
                    Some(before) => {
                        kept.push(before.id.clone());
                        if before.day_of_week != day
                            || before.start_time != range.start
                            || before.end_time != range.end
                        {
                            patches.push(SchedulingAdminRequest::PatchRule {
                                id: before.id.clone(),
                                day_of_week: Some(day),
                                start_time: Some(range.start.clone()),
                                end_time: Some(range.end.clone()),
                            });
                        }
                    }
                    None => creates.push(SchedulingAdminRequest::CreateRule {
                        event_type_id: None,
                        day_of_week: day,
                        start_time: range.start.clone(),
                        end_time: range.end.clone(),
                    }),
                }
            }
        }
        global
            .iter()
            .filter(|r| !kept.contains(&r.id))
            .map(|r| SchedulingAdminRequest::DeleteRule { id: r.id.clone() })
            .chain(patches)
            .chain(creates)
            .collect()
    }
}

/// A dated override as the add form holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverrideForm {
    /// `day_off` or `custom_hours`.
    pub reason: String,
    /// The (first) day, `YYYY-MM-DD`.
    pub date: String,
    /// The last day of a run, or empty for one day. Not for custom hours.
    pub end_date: String,
    /// Custom hours' opening, `HH:MM`.
    pub start: String,
    /// Custom hours' closing.
    pub end: String,
}

impl Default for OverrideForm {
    fn default() -> Self {
        Self {
            reason: "day_off".to_owned(),
            date: String::new(),
            end_date: String::new(),
            start: DEFAULT_START.to_owned(),
            end: DEFAULT_END.to_owned(),
        }
    }
}

impl OverrideForm {
    /// Whether it gives the day custom hours, which takes one day only.
    pub fn is_custom_hours(&self) -> bool {
        self.reason == "custom_hours"
    }

    /// Why it cannot be sent.
    pub fn error(&self) -> Option<&'static str> {
        if !well_formed_date(&self.date) {
            Some(ss::override_date_missing(l()))
        } else if self.is_custom_hours() {
            if !well_formed_time(&self.start) || !well_formed_time(&self.end) {
                Some(ss::time_format_error(l()))
            } else if self.end <= self.start {
                Some(ss::time_order_error(l()))
            } else {
                None
            }
        } else if self.end_date.is_empty() {
            None
        } else if !well_formed_date(&self.end_date) {
            Some(ss::override_date_missing(l()))
        } else if self.end_date < self.date {
            Some(ss::override_end_before_start(l()))
        } else {
            None
        }
    }

    fn draft(&self) -> SchedulingOverrideDraft {
        let custom = self.is_custom_hours();
        SchedulingOverrideDraft {
            date: self.date.clone(),
            end_date: Some(self.end_date.clone())
                .filter(|end| !custom && !end.is_empty() && *end != self.date),
            reason: self.reason.clone(),
            start_time: custom.then(|| self.start.clone()),
            end_time: custom.then(|| self.end.clone()),
        }
    }

    /// The kinds of day the form offers, as (reason sent, label).
    pub fn kinds() -> [(&'static str, &'static str); 2] {
        [
            ("day_off", ss::override_unavailable(l())),
            ("custom_hours", ss::override_custom_hours(l())),
        ]
    }

    /// The date field's label: "Date" for custom hours, "First day" else.
    pub fn date_label(&self) -> &'static str {
        if self.is_custom_hours() {
            ss::override_date_label(l())
        } else {
            ss::override_first_day_label(l())
        }
    }
}

/// The overrides' editor: the days still to come, an add form, and the
/// delete, asked once more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverridesEditor {
    /// The overrides' read.
    pub load: SectionLoad<()>,
    /// The add form, while it is open.
    pub adding: Option<OverrideForm>,
    /// The row whose delete is being asked about, by key.
    pub confirming_delete: Option<String>,
    /// Why the form cannot be sent, before anything was sent.
    pub validation: Option<&'static str>,
    /// The last write's state.
    pub write: WriteState,
    overrides: Vec<SchedulingAvailabilityOverride>,
    today: String,
    timezone: String,
}

impl OverridesEditor {
    /// The days still to come, a run as one row.
    pub fn rows(&self) -> Vec<OverrideRow> {
        upcoming(&self.overrides, &self.today, &self.timezone)
    }

    /// What deleting the row asked about does: a run of several days goes
    /// whole.
    pub fn delete_body(&self) -> String {
        let days = self
            .confirming_delete
            .as_ref()
            .and_then(|key| self.rows().into_iter().find(|r| r.key == *key))
            .map_or(1, |row| row.days);
        if days > 1 {
            ss::override_delete_group_body(l(), days)
        } else {
            ss::override_delete_single_body(l()).to_owned()
        }
    }

    /// The labels: the editor's title and what it is for, its empty sentence,
    /// the add button, the add form's title, the kind's label, the last day's
    /// label and hint, the hours', the delete's title and button.
    pub fn labels() -> [&'static str; 11] {
        [
            ss::overrides_title(l()),
            ss::overrides_hint(l()),
            ss::overrides_write_empty(l()),
            ss::override_add_button(l()),
            ss::override_add_title(l()),
            ss::override_kind_label(l()),
            ss::override_last_day_label(l()),
            ss::override_last_day_hint(l()),
            ss::override_hours_label(l()),
            ss::override_delete_title(l()),
            ss::override_delete_confirm(l()),
        ]
    }
}

/// What the member does on the working hours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HoursEvent {
    /// Open the week's editor.
    OpenWeek,
    /// Add a window to a day (0 Monday), 9 to 5.
    AddRange {
        /// The day.
        day: usize,
    },
    /// Take a window off.
    RemoveRange {
        /// The day.
        day: usize,
        /// The window.
        key: u64,
    },
    /// A window's opening changed.
    SetRangeStart {
        /// The day.
        day: usize,
        /// The window.
        key: u64,
        /// `HH:MM`, as typed.
        value: String,
    },
    /// A window's closing changed.
    SetRangeEnd {
        /// The day.
        day: usize,
        /// The window.
        key: u64,
        /// `HH:MM`, as typed.
        value: String,
    },
    /// Give every other weekday this day's windows.
    CopyToWeekdays {
        /// The day.
        day: usize,
    },
    /// Save the week.
    SaveWeek,
    /// Open the overrides' editor, the days still to come as of `now`.
    OpenOverrides {
        /// The time now.
        now: SystemTime,
    },
    /// Open the add form.
    AddOverride,
    /// The add form's fields changed.
    EditOverride(OverrideForm),
    /// Close the add form.
    CancelOverride,
    /// Add the override.
    SaveOverride,
    /// Ask about deleting a row, by key.
    AskDeleteOverride(String),
    /// Do not delete it.
    CancelDeleteOverride,
    /// Delete it (a run whole).
    ConfirmDeleteOverride,
    /// Put away the last outcome.
    DismissNotice,
    /// Read the open editor's hours again, after its read failed.
    RetryRead,
    /// Close the editor.
    CloseDialog,
}

/// What an answer the working hours await is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {
    Me,
    Rules,
    Overrides,
    WeekRules,
    WeekStep,
    EditorOverrides,
    OverrideWrite(&'static str),
}

fn send(pending: Pending, request: SchedulingAdminRequest) -> Out {
    Out::Send(AdminPending::Hours(pending), Box::new(request))
}

fn section_reads() -> Vec<Out> {
    vec![
        send(Pending::Me, SchedulingAdminRequest::Me),
        send(
            Pending::Rules,
            SchedulingAdminRequest::Rules {
                event_type_id: None,
            },
        ),
        send(Pending::Overrides, SchedulingAdminRequest::Overrides),
    ]
}

pub(crate) fn enter(admin: &mut SchedulingAdmin, tickets: &mut Tickets) -> Vec<Out> {
    admin.forget(tickets, |p| matches!(p, AdminPending::Hours(_)));
    admin.hours = Some(HoursSection {
        timezone: "UTC".to_owned(),
        rules: SectionLoad::Loading,
        overrides: SectionLoad::Loading,
        dialog: None,
    });
    section_reads()
}

pub(crate) fn event(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: HoursEvent,
    can_change: bool,
) -> Vec<Out> {
    let Some(section) = admin.hours.as_mut() else {
        return Vec::new();
    };
    match (event, &mut section.dialog) {
        (HoursEvent::OpenWeek, dialog) if can_change => {
            *dialog = Some(HoursDialog::Week(WeekEditor {
                load: SectionLoad::Loading,
                days: vec![Vec::new(); 7],
                write: WriteState::Idle,
                stored: Vec::new(),
                queue: Vec::new(),
                next_key: 0,
            }));
            vec![week_rules()]
        }
        (HoursEvent::OpenOverrides { now }, dialog) if can_change => {
            *dialog = Some(HoursDialog::Overrides(OverridesEditor {
                load: SectionLoad::Loading,
                adding: None,
                confirming_delete: None,
                validation: None,
                write: WriteState::Idle,
                overrides: Vec::new(),
                today: SchedulingDay::today(now, &section.timezone).key(),
                timezone: section.timezone.clone(),
            }));
            vec![send(
                Pending::EditorOverrides,
                SchedulingAdminRequest::Overrides,
            )]
        }
        (HoursEvent::CloseDialog, dialog) => {
            *dialog = None;
            Vec::new()
        }
        (event, Some(HoursDialog::Week(week))) => week_event(week, event),
        (event, Some(HoursDialog::Overrides(editor))) => overrides_event(editor, event),
        _ => Vec::new(),
    }
}

fn week_rules() -> Out {
    send(
        Pending::WeekRules,
        SchedulingAdminRequest::Rules {
            event_type_id: None,
        },
    )
}

fn range(week: &mut WeekEditor, day: usize, key: u64) -> Option<&mut HoursRange> {
    week.days.get_mut(day)?.iter_mut().find(|r| r.key == key)
}

fn week_event(week: &mut WeekEditor, event: HoursEvent) -> Vec<Out> {
    match event {
        HoursEvent::AddRange { day } if day < 7 => {
            let key = week.key();
            week.days[day].push(HoursRange {
                key,
                rule_id: None,
                start: DEFAULT_START.to_owned(),
                end: DEFAULT_END.to_owned(),
            });
        }
        HoursEvent::RemoveRange { day, key } if day < 7 => {
            week.days[day].retain(|r| r.key != key);
        }
        HoursEvent::SetRangeStart { day, key, value } => {
            if let Some(range) = range(week, day, key) {
                range.start = value;
            }
        }
        HoursEvent::SetRangeEnd { day, key, value } => {
            if let Some(range) = range(week, day, key) {
                range.end = value;
            }
        }
        // Each other weekday takes the day's windows, keeping its own rules'
        // ids in order, so a copy changes rules rather than replacing them.
        HoursEvent::CopyToWeekdays { day } if day < 5 => {
            let source = week.days[day].clone();
            for target in (0..5).filter(|t| *t != day) {
                let ids: Vec<Option<String>> = week.days[target]
                    .iter()
                    .map(|r| r.rule_id.clone())
                    .collect();
                let copied: Vec<HoursRange> = source
                    .iter()
                    .enumerate()
                    .map(|(index, r)| (index, r.start.clone(), r.end.clone()))
                    .collect::<Vec<_>>()
                    .into_iter()
                    .map(|(index, start, end)| HoursRange {
                        key: week.key(),
                        rule_id: ids.get(index).cloned().flatten(),
                        start,
                        end,
                    })
                    .collect();
                week.days[target] = copied;
            }
        }
        HoursEvent::SaveWeek if week.can_save() => {
            let mut queue = week.diff();
            if queue.is_empty() {
                // Nothing changed: no request, which would still spend a write.
                week.write = WriteState::Done(ss::hours_nothing_to_save(l()).to_owned());
                return Vec::new();
            }
            week.write = WriteState::Working;
            let first = queue.remove(0);
            week.queue = queue;
            return vec![send(Pending::WeekStep, first)];
        }
        HoursEvent::DismissNotice => week.write = WriteState::Idle,
        HoursEvent::RetryRead if week.load.failure().is_some() => {
            week.load = SectionLoad::Loading;
            return vec![week_rules()];
        }
        _ => {}
    }
    Vec::new()
}

fn overrides_event(editor: &mut OverridesEditor, event: HoursEvent) -> Vec<Out> {
    match event {
        HoursEvent::AddOverride => {
            editor.adding = Some(OverrideForm::default());
            editor.validation = None;
        }
        // An edit answers the last save's complaint: it goes, and the next
        // save checks again (visual QA LC20).
        HoursEvent::EditOverride(form) => {
            if editor.adding.is_some() {
                editor.adding = Some(form);
                editor.validation = None;
            }
        }
        HoursEvent::CancelOverride => {
            editor.adding = None;
            editor.validation = None;
        }
        HoursEvent::SaveOverride if !editor.write.busy() => {
            if let Some(form) = &editor.adding {
                editor.validation = form.error();
                if editor.validation.is_none() {
                    editor.write = WriteState::Working;
                    return vec![send(
                        Pending::OverrideWrite(ss::override_added(l())),
                        SchedulingAdminRequest::CreateOverride(form.draft()),
                    )];
                }
            }
        }
        HoursEvent::AskDeleteOverride(key) => editor.confirming_delete = Some(key),
        HoursEvent::CancelDeleteOverride => editor.confirming_delete = None,
        HoursEvent::ConfirmDeleteOverride if !editor.write.busy() => {
            let row = editor
                .confirming_delete
                .take()
                .and_then(|key| editor.rows().into_iter().find(|r| r.key == key));
            if let Some(row) = row {
                editor.write = WriteState::Working;
                let request = if row.is_run {
                    SchedulingAdminRequest::DeleteOverrideGroup {
                        group_id: row.target,
                    }
                } else {
                    SchedulingAdminRequest::DeleteOverride { id: row.target }
                };
                return vec![send(
                    Pending::OverrideWrite(ss::override_deleted(l())),
                    request,
                )];
            }
        }
        HoursEvent::DismissNotice => {
            editor.validation = None;
            editor.write = WriteState::Idle;
        }
        HoursEvent::RetryRead if editor.load.failure().is_some() => {
            editor.load = SectionLoad::Loading;
            return vec![send(
                Pending::EditorOverrides,
                SchedulingAdminRequest::Overrides,
            )];
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
        .hours
        .as_mut()
        .map(|section| hours_answer(section, pending, answer))
        .unwrap_or_default()
}

fn hours_answer(section: &mut HoursSection, pending: Pending, answer: Answer) -> Vec<Out> {
    match (pending, &mut section.dialog) {
        // The profile gives the zone; without it the hours read in UTC.
        (Pending::Me, _) => {
            if let Ok(SchedulingAdminData::Me(me)) = answer {
                section.timezone = me.display_timezone().to_owned();
            }
        }
        (Pending::Rules, _) => section.rules = load(answer, data!(Rules)),
        (Pending::Overrides, _) => section.overrides = load(answer, data!(Overrides)),
        (Pending::WeekRules, Some(HoursDialog::Week(week))) => {
            match answer.and_then(|data| data!(Rules)(data).ok_or_else(FailureText::unexpected)) {
                Ok(rules) => week.fill(rules),
                Err(failure) => week.load = SectionLoad::Failed(failure),
            }
        }
        // One step of the save landed: the next, or the end. A refusal ends it
        // too. Either way the week and the section are read again.
        (Pending::WeekStep, Some(HoursDialog::Week(week))) => {
            if let (Ok(_), false) = (&answer, week.queue.is_empty()) {
                let next = week.queue.remove(0);
                return vec![send(Pending::WeekStep, next)];
            }
            week.queue.clear();
            week.write = match answer {
                Ok(_) => WriteState::Done(ss::hours_saved(l()).to_owned()),
                Err(failure) => WriteState::Failed(failure),
            };
            week.load = SectionLoad::Loading;
            let mut outs = vec![week_rules()];
            outs.extend(section_reads());
            return outs;
        }
        (Pending::EditorOverrides, Some(HoursDialog::Overrides(editor))) => {
            match answer.and_then(|data| data!(Overrides)(data).ok_or_else(FailureText::unexpected))
            {
                Ok(overrides) => {
                    section.overrides = SectionLoad::Ready(overrides.clone());
                    editor.overrides = overrides;
                    editor.load = SectionLoad::Ready(());
                }
                Err(failure) => editor.load = SectionLoad::Failed(failure),
            }
        }
        (Pending::OverrideWrite(notice), Some(HoursDialog::Overrides(editor))) => match answer {
            Ok(_) => {
                editor.adding = None;
                editor.write = WriteState::Done(notice.to_owned());
                return vec![send(
                    Pending::EditorOverrides,
                    SchedulingAdminRequest::Overrides,
                )];
            }
            Err(failure) => editor.write = WriteState::Failed(failure),
        },
        // An answer for an editor that has closed.
        _ => {}
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_dates_are_checked_as_the_scheduler_takes_them() {
        assert!(well_formed_time("09:30"));
        for bad in ["9:30", "09:3", "0930x", "ab:cd", "09-30"] {
            assert!(!well_formed_time(bad), "{bad}");
        }
        assert!(well_formed_date("2026-09-14"));
        for bad in ["2026-9-14", "2026/09/14", "20260914xx", "abcd-ef-gh"] {
            assert!(!well_formed_date(bad), "{bad}");
        }
        assert_eq!(
            (0..7).map(wire_day).collect::<Vec<_>>(),
            [1, 2, 3, 4, 5, 6, 0]
        );
        assert_eq!(day_label("someday"), "someday");
    }
}
