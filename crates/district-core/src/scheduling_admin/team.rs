//! STUB: Team.

use super::{Answer, Out, SchedulingAdmin};
use crate::model::Tickets;

/// Team.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TeamSection {}

/// What the member does on Team.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TeamEvent {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {}

pub(crate) fn enter(admin: &mut SchedulingAdmin, _tickets: &mut Tickets) -> Vec<Out> {
    admin.team = Some(TeamSection::default());
    Vec::new()
}

pub(crate) fn event(
    _admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: TeamEvent,
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
