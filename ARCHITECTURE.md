# Architecture

This is the high-level design of the District AI core for Rust: the crates, how a user's
action becomes a request and comes back as state, and where state lives. What the design
protects, and how, is in [SECURITY-MODEL.md](SECURITY-MODEL.md) and the security model in
[SECURITY.md](SECURITY.md#security-model). The crate documentation of each crate
(`cargo doc --workspace --no-deps --open`) has the detail.

## The crates and how they depend on each other

| crate | depends on (workspace crates only) |
| --- | --- |
| `district-model` | none |
| `district-l10n` | none |
| `district-api` | `district-model`, `district-l10n` |
| `district-auth`, `district-live` | `district-api`, `district-model` |
| `district-core` | `district-api`, `district-auth`, `district-live`, `district-l10n`, `district-model` |
| `district-host` | `district-core`, `district-auth`, `district-model` |
| `district-call` | `district-core`, `district-model` |
| `district-crash` | none |

Dependencies point one way. `district-model` and `district-l10n` are at the bottom and do
no IO. `district-api`, `district-auth` and `district-live` are the clients that talk to the
District AI service. `district-core` is the application state above them, and
`district-host` and `district-call` sit on top of it. `district-crash` depends on none of
them, so an app can report a crash that happens before the core is set up. No crate
depends on a user interface toolkit or on a crate that builds on only one operating system,
which is what lets District AI for Linux and District AI for Windows build on every crate.

| crate | role |
| --- | --- |
| `district-model` | The wire format: serde types for the REST API and the live telemetry envelopes |
| `district-api` | The HTTP client and the endpoint table (reqwest with rustls) |
| `district-auth` | OAuth 2.0 with PKCE through the system browser, token exchange, refresh, sign-out |
| `district-live` | The live telemetry WebSocket client |
| `district-l10n` | Every sentence and label, in English and Canadian French, and the number, money, date and duration formats |
| `district-core` | The application state: `Model`, `Event`, `Effect`, and the `EffectRunner` with the traits an app implements |
| `district-host` | What a desktop app keeps on the machine: the device id, the refresh marker, the settings file, the sleep protocol |
| `district-call` | The call engine: LiveKit behind the `livekit` feature, `UnavailableCallEngine` without it |
| `district-crash` | Opt-in crash reports, sent only with a DSN and the user's consent |

## The loop

The core is shaped so that an app decides nothing. An app is a loop of three parts, and
only the last touches the outside world:

1. **`Model`** (`district-core`) holds all of the state. `Model::update` takes an `Event`
   and returns the `Effect`s to run next. Effects are plain data, such as "load the
   overview of this workspace" or "open this page in the browser".
2. **The app** (GTK 4 on Linux, WinUI 3 on Windows) renders the model and forwards what the
   user does as events.
3. **`EffectRunner`** runs each effect against ten traits (`DistrictApi`, `Auth`,
   `Settings`, `UrlOpener`, `Clock`, `LiveUpdates`, `Notifier`, `Presence`, `CallEngine`,
   `RingSurface`) and turns each result into the next event.

The core implements the traits that talk to the service (`NativeAuth`, `LiveHub`,
`DesktopPresence`, and `DistrictApi` over `district_api::ApiClient`). The app implements the
ones only its system can: the secret store behind `district_auth::SessionStore`, settings
storage, opening a link, notifications, the ring, and the call engine from `district-call`.

Two inputs do not come from an effect: live updates, which the app reads from the receiver
`LiveHub::new` returns and forwards as `Event::Live`, and the call engine's reports, which
it forwards as `Event::Media`. A live update is only a hint: the model reads what it
changed again through the API rather than show what the update carried.

Because the model is pure and the runner works against traits, the tests drive the model
with events and the runner with fakes and a paused clock, with no display, network or
keyring.

## A request, end to end

A user presses something. The app sends the event to `Model::update`, which returns an
effect such as `Effect::LoadWorkspaceBilling`. The runner calls the matching `DistrictApi` method;
`district-api` sends it to the service with the access token as a bearer header and the
workspace named explicitly, follows no redirect, and decodes the answer into a
`district-model` type. A response that does not decode is an error. The runner turns the
result into an event, the model updates its screen state, and the app renders it.

The access token comes from a `TokenSource` (`district-api`), which `district-auth`
implements: it refreshes the token when it is close to expiry, one refresh at a time,
because the service rotates the refresh token and a second presentation of one reads as
theft. A token the service refuses with a 401 is forgotten only if it is still the cached
one, so two refused requests cannot throw away a fresh token. Sign-in itself is a browser round trip: the core
builds the authorize URL, the app opens it, and the browser returns through
`districtai://auth` to the running app, which hands the URI to the model
(`Event::SignInCallback`).

## Where state lives

- **In memory, in the process**: the `Model` (the session, the routes, each screen's
  state, hand-off states and nonces), the access token, the live telemetry credentials,
  and call credentials and passphrases. All of it goes when the process ends.
- **In the app's secret store**: the refresh token, behind `SessionStore`. The crates never
  write it anywhere else.
- **On disk, through `district-host`**: the device id (a random UUID), the refresh marker (a
  SHA-256 fingerprint of a refresh token in flight, never the token), and the settings file
  ("ring on this computer" and the last workspace chosen). On Unix each is readable by its
  owner only.
- **On the service**: everything else. The core holds no cache of customer data on disk.

## What the crates are tested against

`contracts/` holds the server's recorded responses, its localization facts, the Android
app's endpoint list and the brand palette, each vendored with its source commit and pinned
by `contracts/SHA256SUMS`. `district-model` decodes every recorded response with unknown
fields refused; `district-api` holds its endpoint table to the Android app's; and
`district-l10n` holds its formats to the service's own outputs. CONTRIBUTING.md says how
each set is refreshed.

## Build and release

One Cargo workspace and one `Cargo.lock`, with no build scripts of its own. The crates are
not published to crates.io: an app depends on them by git tag, with the exact version beside
the tag (README.md). Releases are tags on main, made as CONTRIBUTING.md describes.
