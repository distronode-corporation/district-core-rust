//! STUB: Settings.

use super::{Answer, Out, SchedulingAdmin};
use crate::model::Tickets;

/// Settings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SettingsSection {}

/// What the member does on Settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsEvent {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {}

pub(crate) fn enter(admin: &mut SchedulingAdmin, _tickets: &mut Tickets) -> Vec<Out> {
    admin.settings = Some(SettingsSection::default());
    Vec::new()
}

pub(crate) fn event(
    _admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: SettingsEvent,
    _can_change: bool,
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
