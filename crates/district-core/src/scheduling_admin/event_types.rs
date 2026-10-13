//! STUB: event types.

use super::{Answer, Out, SchedulingAdmin};
use crate::model::Tickets;

/// The event types list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventTypesSection {}

/// One event type.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventTypeSection {
    /// Its slug.
    pub slug: String,
}

/// What the member does on the event types list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventTypesEvent {}

/// What the member does on one event type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventTypeEvent {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Pending {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DetailPending {}

pub(crate) fn enter(admin: &mut SchedulingAdmin, _tickets: &mut Tickets) -> Vec<Out> {
    admin.event_types = Some(EventTypesSection::default());
    Vec::new()
}

pub(crate) fn enter_detail(
    admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    slug: String,
) -> Vec<Out> {
    admin.event_type = Some(EventTypeSection { slug });
    Vec::new()
}

pub(crate) fn event(
    _admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: EventTypesEvent,
    _can_change: bool,
) -> Vec<Out> {
    match event {}
}

pub(crate) fn detail_event(
    _admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    event: EventTypeEvent,
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

pub(crate) fn detail_answered(
    _admin: &mut SchedulingAdmin,
    _tickets: &mut Tickets,
    pending: DetailPending,
    _answer: Answer,
) -> Vec<Out> {
    match pending {}
}
