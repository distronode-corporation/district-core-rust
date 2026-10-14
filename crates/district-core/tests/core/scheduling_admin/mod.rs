//! The booking scheduler's administration, section by section: what each one
//! reads, what each action sends, and how each answer and refusal lands.

mod bookings;
mod common;
mod event_types;
mod overview;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use district_api::ApiError;
use district_core::scheduling_admin::SchedulingAdmin;
use district_core::{Effect, Event, Model, Route, SchedulingAdminEvent, SchedulingSection, Ticket};
use district_model::{
    SchedulingAdminData, SchedulingAdminFailure, SchedulingAdminReply, SchedulingAdminRequest,
    SchedulingAdminSuccess, SchedulingStatusResponse,
};
use serde::de::DeserializeOwned;

use crate::support::{AGENCY, VIEWER, fixture, loaded, signed_in};

/// 2026-09-14T12:00:00Z: 8 a.m. in Toronto, an hour before the recorded
/// booking starts.
pub(crate) fn now() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_789_387_200)
}

pub(crate) fn admin(model: &Model) -> &SchedulingAdmin {
    &signed_in(model).scheduling_admin
}

/// The `data` of a recorded success.
pub(crate) fn data<T: DeserializeOwned>(name: &str) -> T {
    fixture::<SchedulingAdminSuccess<T>>(name).data
}

/// The scheduling administration requests in `effects`, with their tickets.
pub(crate) fn sent(effects: &[Effect]) -> Vec<(Ticket, SchedulingAdminRequest)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::SchedulingAdmin {
                ticket, request, ..
            } => Some((*ticket, request.clone())),
            _ => None,
        })
        .collect()
}

/// The ticket of the one request in `effects` that `wanted` picks.
pub(crate) fn ticket_for(
    effects: &[Effect],
    wanted: impl Fn(&SchedulingAdminRequest) -> bool,
) -> Ticket {
    let found: Vec<Ticket> = sent(effects)
        .into_iter()
        .filter(|(_, request)| wanted(request))
        .map(|(ticket, _)| ticket)
        .collect();
    match found.as_slice() {
        [ticket] => *ticket,
        _ => panic!("not exactly one such request in {effects:?}"),
    }
}

/// The ticket of the status read in `effects`.
pub(crate) fn status_ticket(effects: &[Effect]) -> Ticket {
    effects
        .iter()
        .find_map(|effect| match effect {
            Effect::LoadSchedulingStatus { ticket, .. } => Some(*ticket),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no status read in {effects:?}"))
}

/// Opens `section` in a workspace where the member has `role`.
pub(crate) fn open(role: &str, section: SchedulingSection) -> (Model, Vec<Effect>) {
    let workspace = if role == "viewer" { VIEWER } else { AGENCY };
    let (mut model, _) = loaded(workspace, role);
    let effects = model.update(Event::Navigate(Route::SchedulingAdmin(section)));
    (model, effects)
}

pub(crate) fn done(data: SchedulingAdminData) -> Result<SchedulingAdminReply, ApiError> {
    Ok(SchedulingAdminReply::Done(data))
}

pub(crate) fn refused(kind: &str) -> Result<SchedulingAdminReply, ApiError> {
    Ok(SchedulingAdminReply::Refused(SchedulingAdminFailure {
        ok: false,
        failure: kind.to_owned(),
        status: 409,
    }))
}

pub(crate) fn answer(
    model: &mut Model,
    ticket: Ticket,
    result: Result<SchedulingAdminReply, ApiError>,
) -> Vec<Effect> {
    model.update(Event::SchedulingAdminAnswered { ticket, result })
}

pub(crate) fn status_answer(
    model: &mut Model,
    effects: &[Effect],
    status: Result<SchedulingStatusResponse, ApiError>,
) -> Vec<Effect> {
    model.update(Event::SchedulingStatusLoaded {
        ticket: status_ticket(effects),
        result: status,
    })
}

pub(crate) fn ready_status() -> SchedulingStatusResponse {
    fixture("district-scheduling-status-ready.json")
}

pub(crate) fn act(model: &mut Model, event: SchedulingAdminEvent) -> Vec<Effect> {
    model.update(Event::SchedulingAdmin(event))
}
