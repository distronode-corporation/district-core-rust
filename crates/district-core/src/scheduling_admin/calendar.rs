//! STUB: calendars.

use district_api::ApiError;
use district_l10n::scheduling_strings as ss;
use district_model::SchedulingSecret;

use super::{Answer, Out, SchedulingAdmin};
use crate::model::{CoreConfig, Tickets};
use crate::text::l;

/// The calendars.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CalendarSection {}

/// What the member does on the calendars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalendarEvent {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {}

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

pub(crate) fn enter(admin: &mut SchedulingAdmin, _tickets: &mut Tickets) -> Vec<Out> {
    admin.calendar = Some(CalendarSection::default());
    Vec::new()
}

pub(crate) fn event(
    _admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: CalendarEvent,
    _config: &CoreConfig,
) -> Vec<Out> {
    match event {}
}

pub(crate) fn answered(
    _admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    pending: Pending,
    _answer: Answer,
) -> Vec<Out> {
    match pending {}
}

pub(crate) fn signed_in(
    _admin: &mut SchedulingAdmin,
    _pending: super::Pending,
    _result: Result<SchedulingSecret, ApiError>,
) -> Vec<Out> {
    Vec::new()
}
