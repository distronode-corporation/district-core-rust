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

## [3.1.0] - 2026-10-09

### Added

- `SignedIn::no_workspace_message` and `WorkspacesState::CHOOSE_PLAN_MESSAGE`: an
  account with no workspace that is offered the plans (`SignedIn::offers_plans`) is
  told to choose a plan to set one up, not to contact support. Every other state,
  and every app that does not buy in the app, reads `WorkspacesState::message` as
  before.

### Changed

- `Event::EmbeddedClosed` with no workspace open lists the workspaces again
  (`Effect::LoadWorkspaces`) and opens the one checkout made, without a
  restart. Only in an app with `CoreConfig::in_app_purchases`.

## [3.0.0] - 2026-10-09

### Added

- Buying in the app, for District AI for Windows only, through the service's own
  Stripe checkout and billing page shown inside the app window. Off unless the app
  sets `CoreConfig::in_app_purchases` (see Changed); with it off nothing below
  happens and billing stays read only.
  - `BillingEvent::ChoosePlan(PlanChoice)` with `PlanTier` (the four voice plans,
    by their published keys), `PlanTerm` (monthly or annual) and an optional
    `PromoCode` (taken only in the one shape the checkout reads, refused whole
    otherwise), then `ConfirmPurchase` or `CancelPurchase`; `ManageInApp` for the
    billing page; `DismissPurchaseNotice`. `SignedIn::offers_plans` (purchases on,
    and a role that can change the plan, or no workspace yet) and
    `SignedIn::offers_manage_in_app` say what to show.
  - Each starts the hand-off of 1.1.0's booking pages (S33), with its own `state`:
    the start page opens in a new private view inside the app
    (`Effect::OpenEmbedded` with `EmbeddedView::NewPrivate`), the
    `districtai://handoff` answer the app catches there comes back as
    `Event::HandOffCallback`, as from the browser, and the link is asked for with
    its nonce (`Effect::RequestBillingHandOff`, `Event::BillingHandOffReady`) and
    opened in the same view (`EmbeddedView::Same`). `BillingDestination::next`
    builds the only two destinations the service admits, `/checkout` with `tier`,
    `term`, `promo` and `reason=no-workspace`, or `/dashboard/district/billing`.
  - `UrlOpener::open_embedded`, defaulting to false ("not supported"). Then, or
    on `Event::EmbeddedUnavailable`, the purchase starts again in the system
    browser and `PurchaseState::in_browser` says so. `Event::EmbeddedClosed`
    gives up a start page the closed view held and reads the billing screen again.
  - "Purchases on this computer" (`PurchaseSetting`, `Off` or `SignInEveryTime`,
    the default), read at sign-in (`Effect::ReadPurchaseSetting`,
    `Event::PurchaseSettingRead`), changed with `Event::SetPurchases` and kept with
    `Effect::SavePurchaseSetting`. Off hides every purchase action and drops a
    purchase under way.
  - The words: `PurchaseState`'s constants (the confirmation naming Stripe, the
    in-app note, the browser fallback), `PurchaseSetting::LABEL` and `BODY`, the
    plan and term labels. No price is named; checkout shows them.
  - Each refusal of the hand-off maps to a notice, `invalid_next` among them.
- `district-api`: `ApiClient::billing_hand_off` for `POST /api/district/billing/handoff`,
  `Endpoint::BillingHandOff` (account-scoped, never repeated, on `DESKTOP_ONLY`).
- `district-model`: `BillingHandOffResponse` (the scheduling hand-off's two fields,
  as the service answers both) and `CODE_INVALID_NEXT`.
- `district-host`: the settings file keeps `purchases`, only once it is set, so no
  other app's file changes.

### Changed

- Breaking: `CoreConfig` has a new field, `in_app_purchases`. District AI for Linux
  sets it false, and then every event, effect and view is what 2.0.0 had.
- Breaking: `DistrictApi` has a new required method, `billing_hand_off`.
- Breaking: `Settings` has two new required methods, `purchases` and
  `set_purchases`. `district-host`'s `SettingsFile` implements them, and
  `Preferences` has a new field, `purchases`.
- Breaking: new `Event` variants (`SetPurchases`, `EmbeddedClosed`,
  `PurchaseSettingRead`, `BillingHandOffReady`, `EmbeddedUnavailable`), `Effect`
  variants (`OpenEmbedded`, `RequestBillingHandOff`, `ReadPurchaseSetting`,
  `SavePurchaseSetting`) and `BillingEvent` variants (`ChoosePlan`,
  `ConfirmPurchase`, `CancelPurchase`, `ManageInApp`, `DismissPurchaseNotice`), and
  a new `SignedIn` field, `purchase`.
- `UrlOpener` has a new method, `open_embedded`, with a default, so an existing
  implementation keeps compiling and never shows a page inside the app.

## [2.0.0] - 2026-10-09

### Added

- `district-auth`: the authorize URL names the app's platform (`platform=linux` or
  `platform=windows`, `Platform::wire`), after `redirect_uri`. The service offers
  account creation on its sign-in page to the Windows app only, binding the value to
  the attempt's challenge and state; it reads any other value, or none, as before.
- The live transcript of the call on this desktop, with the semantics of
  district-core-swift 6.0.0's `TranscriptReducer`. Once the call has an id (a placed
  call's from the dial's answer, an answered call's from its ring), the core asks the
  call's workspace socket for it with the new `Effect::WatchTranscript`, and applies the
  `transcript_*` frames that socket delivers for that call to
  `ActiveCall::transcript()`, a `LiveTranscript`: `lines()` in (epoch, index) order with
  their speaker and text, interim lines (`is_final` false) replaced in place by later
  revisions and their finals, `phase()` (`Subscribing`, `Live`, `Reconnecting`,
  `Ended`, `Unavailable`), `is_complete()`, and `final_transcript()`, the full
  transcript read with backoff once the live one ends. `status()`, `speaker_label()` and
  the `LiveTranscript` constants are the words both apps show. Duplicates, gaps (healed
  after `GAP_HEAL` with one subscribe in flight), snapshots in parts, retractions,
  `agent_error`, `rate_limited` and `not_live` (retried only on a call event showing the
  call in progress, at most once per `NOT_LIVE_RETRY`) follow the contract. A frame for
  another call or workspace, one that cannot be read, and one after the call ended
  (except a retraction, which still takes lines off) change nothing.
- `district-model`: `TelemetryEventType` names the five transcript events
  (`TranscriptSnapshot`, `TranscriptSegment`, `TranscriptEnded`, `TranscriptRetracted`,
  `TranscriptError`, and `is_transcript()`), `TelemetryEnvelope::transcript_event()`
  reads their data into `TranscriptEvent` (refusing another `v`, a call id that
  disagrees with the envelope's, and text over `TRANSCRIPT_TEXT_MAX_UTF16`), with the open
  vocabularies `TranscriptSpeaker`, `TranscriptEndReason`, `TranscriptRetractReason` and
  `TranscriptErrorCode`; a segment's text stays out of `Debug`. `TranscriptClientOp`
  writes the client's ops.
- `district-live`: a connection keeps the calls whose transcript it receives
  (`subscribe_transcript`, `unsubscribe_transcript`, `resubscribe_transcript`, on
  `TelemetryConnection` and per workspace on `TelemetryHub`) and sends a
  `transcript.subscribe` for each on every socket it opens, renewals included. An op it
  sends does not count as the server speaking. `is_transient` is public.
- `LiveHub::transcripts()` names the calls whose transcript the sockets are asked for.

### Changed

- Breaking: `TelemetryEventType` gains five variants, so an exhaustive `match` over it
  needs them; `Effect` gains `WatchTranscript`; and `LiveUpdates` gains
  `watch_transcript`, which `LiveHub` implements. The six transcript fixtures are now
  decoded with their typed data in the contract gate, not as envelopes of an unknown
  type.

### Fixed

- `Event::Quitting` saves the open thread's reply when its save is still waiting for
  the typing to stop, and sends every draft write queued behind the one on its way.
  Before, a reply typed in the last two seconds before quitting, or queued behind a
  slow save, was lost.

## [1.2.0] - 2026-10-08

### Changed

- The contract fixtures follow the service's current shapes. The Android set gains nine
  files and loses none: the live call transcript's six frames on the telemetry socket,
  the telemetry token (the service now writes the desktop set's file to both sets, byte
  for byte, because the mobile apps open the same socket), and the two answers of the
  Apple apps' native second sign-in step. The desktop set is unchanged.
- The live transcript's frames decode as `TelemetryEnvelope`s of an event type this
  client does not name (`TelemetryEventType::Unknown`), on purpose: the desktop apps
  show no live transcript, and the core reads nothing again for them, so a call's
  stream of transcript frames costs no requests. Tests hold both to that.
- The two native second-step fixtures are excluded by decision: a desktop signs in
  through the browser, which asks for the second factor itself.

### Removed

- `AiPersona::avatar_recording` (`videoRecording` on the wire). It named a recording of
  avatar calls the service has never made, and the service no longer stores it. A shipped
  build still reads a persona that carries the key, which it ignores. This is a change
  to a public type; no app pins the core yet, so it ships in a minor release.
  `CallSummary::recording_url` stays, always `None`, because the service still sends
  `recordingUrl: null` for older clients that require the key.

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
