//! The contract gate: this crate's data types held to the server's recorded
//! responses in `contracts/fixtures/` (the Android app's set) and
//! `contracts/desktop/` (the shapes only this client reads), and the vendored
//! localization facts in `contracts/i18n/` accounted for beside them.
//!
//! - `checksums`: the vendored files of every set are exactly the ones
//!   `contracts/SHA256SUMS` lists, byte for byte.
//! - `manifest`: every fixture is decoded by a type, recorded as not yet modelled,
//!   or excluded by a stated decision, and exactly one of those.
//! - `round_trip`: every decoded fixture decodes strictly, and encodes back to the
//!   same JSON.
//! - `fixtures`: what each decoded fixture is supposed to cover, asserted, so a
//!   fixture recorded again against thinner data cannot quietly stop covering it.
//! - `voice_studio`: the Voice Studio read, and the engine chain beyond what its
//!   fixture holds.
//! - `strict`: the `strict-contracts` feature is on in tests, and every type in
//!   `src/` that derives `Deserialize` honours it.
//! - `scheduling_admin`: the booking scheduler's administration, each
//!   operation's answer read by the request that asks for it, and what each
//!   request sends.
//! - `transcript`: the live transcript's frames read into their typed events,
//!   and the ops a client sends.

mod checksums;
mod fixtures;
mod manifest;
mod round_trip;
mod scheduling_admin;
mod strict;
mod support;
mod transcript;
mod voice_studio;
