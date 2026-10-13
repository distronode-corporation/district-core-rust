//! STUB: Hours.

use super::{Answer, Out, SchedulingAdmin};
use crate::model::Tickets;

/// Hours.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HoursSection {}

/// What the member does on Hours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HoursEvent {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {}

pub(crate) fn enter(admin: &mut SchedulingAdmin, _tickets: &mut Tickets) -> Vec<Out> {
    admin.hours = Some(HoursSection::default());
    Vec::new()
}

pub(crate) fn event(
    _admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: HoursEvent,
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
