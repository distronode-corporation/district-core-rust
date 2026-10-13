//! STUB: Developer.

use super::{Answer, Out, SchedulingAdmin};
use crate::model::Tickets;

/// Developer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeveloperSection {}

/// What the member does on Developer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeveloperEvent {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {}

pub(crate) fn enter(admin: &mut SchedulingAdmin, _tickets: &mut Tickets) -> Vec<Out> {
    admin.developer = Some(DeveloperSection::default());
    Vec::new()
}

pub(crate) fn event(
    _admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: DeveloperEvent,
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
