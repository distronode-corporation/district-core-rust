# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

`scripts/check-version.py` holds the newest released section's heading to the version in
`Cargo.toml`, and README.md and SECURITY.md to that release, so a release is one edit to
each: rename `[Unreleased]` to the version and date, bump `[workspace.package] version`
to match, and name the new version wherever README.md and SECURITY.md name the current
release. See "Releases" in CONTRIBUTING.md.

What counts as a breaking change here is a change to anything an app that pins these
crates calls or implements: a public type, function or trait, an `Event` or `Effect`
variant, or a field of one. Those take a new major version.

## [Unreleased]

## [1.1.0] - 2026-10-08

### Security

- "Manage on the web" binds the sign-in it hands to the browser to the browser the app
  opened. The core first opens the service's hand-off start page
  (`HAND_OFF_START_PATH`) with a fresh `state`; the browser answers through a
  `districtai://handoff` link carrying a one-time nonce that the service also keeps in
  that browser as a short-lived cookie, and the link is asked for with that nonce, so it
  signs in only the browser holding the cookie. A link sent to someone else's browser no
  longer signs that browser in to your account. A `districtai://handoff` link that does
  not answer the hand-off under way is dropped and leaves it waiting. If the browser
  does not answer within ten seconds (`HAND_OFF_CALLBACK_WAIT`), the link is asked for
  unbound, as before, which the service accepts until it requires the binding; if no
  browser took the start page, the hand-off is given up. The `state` and the nonce are
  redacted from `Debug` and never logged.

### Added

- district-auth: `HandOffState`, `HandOffNonce`, `HandOffError`, `HAND_OFF_START_PATH`,
  `HAND_OFF_HOST`, `HAND_OFF_NONCE_LEN`, `is_valid_hand_off_state` and
  `is_valid_hand_off_nonce`.
- district-model: `CODE_INVALID_NONCE` and `CODE_NONCE_REQUIRED`, the codes of the
  service's two 400s for a hand-off's nonce, each shown in words of the app's own.
- district-core: `Event::HandOffCallback`, and `Event::from_link`, which turns a link
  the desktop hands the app into the sign-in's or the hand-off's event by its host, so
  an app routes every `districtai:` link through one call. `HandOffLeg`,
  `HAND_OFF_CALLBACK_WAIT`, and `SchedulingScreen::opening` and
  `SchedulingScreen::minting`.

### Changed

- These change what an app calls or matches on. No app pins the core yet, so they ship
  as a minor release, as the plan for this release set out:
  - `ApiClient::scheduling_hand_off` and `DistrictApi::scheduling_hand_off` take a
    `nonce: Option<&str>`; without one the key is left out of the body, never sent as
    `null`.
  - `Effect::RequestSchedulingHandOff` has a `nonce` field.
  - `SchedulingScreen::opening`, a field, is now `SchedulingScreen::hand_off`, a
    `HandOffLeg`; `opening()` and `minting()` say what the field said.
  - "Manage on the web" now yields `Effect::OpenOneTimeUrl` for the start page and an
    `Effect::Wait`, rather than `Effect::RequestSchedulingHandOff` at once.

## [1.0.0] - 2026-10-08

### Added

- The shared Rust core of the District AI desktop apps, as its own repository:
  district-model, district-api, district-auth, district-live, district-core,
  district-call and district-host, the contract fixtures they are tested against, and
  the scripts that vendor those fixtures and check this repository. It is extracted
  from District AI for Linux 2.1.0
  ([distronode-corporation/district-linux](https://github.com/distronode-corporation/district-linux))
  with the history of every file it holds; a commit that names a pull request names it
  in that repository (`distronode-corporation/district-linux#NN`). District AI for
  Linux and District AI for Windows depend on it by tag.
- `Platform` and `ClientIdentity` in district-model: the app a request comes from is
  named once, by the app, and the crates write it on the wire (the User-Agent, the
  sign-in's platform, the desktop's presence registration and the identity a room is
  joined with). The User-Agent carries the app's version, not the crate's. Linux's
  bytes on the wire are unchanged.
- district-host: what a desktop app keeps on the machine and how it hears the machine
  sleep, on any operating system (the device id, the refresh marker, the settings file
  and the sleep protocol), split out of District AI for Linux's Linux-only crate.
- CI on Windows (`windows-2025`, MSVC) as well as Linux, and a `deny.toml` that judges
  the crates that build only on Windows as strictly as the rest.

### Changed

- The version starts again at 1.0.0 for this repository; it says nothing about the
  apps' own versions.
- The endpoint table's desktop additions are `DESKTOP_ONLY` (they were `LINUX_ONLY`),
  because a desktop app on another system shares them.
