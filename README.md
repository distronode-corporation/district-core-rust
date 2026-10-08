# District AI core for Rust

The shared Rust core of the District AI desktop apps: the data types, the HTTP
client, sign-in, live updates, the application state and the call engine of
District AI, the AI voice receptionist from Distronode, with no user interface
toolkit in any of it.

> **Status: 1.0.0 is the current release**
> ([1.0.0](https://github.com/distronode-corporation/district-core-rust/releases/latest)),
> extracted from District AI for Linux 2.1.0 with its history. See the
> [CHANGELOG](CHANGELOG.md).

## Who uses it

Two apps build on these crates, and each pins them by tag:

- [District AI for Linux](https://github.com/distronode-corporation/district-linux),
  a GTK 4 and libadwaita app.
- [District AI for Windows](https://github.com/distronode-corporation/district-windows),
  a WinUI 3 app that reaches the core through a UniFFI layer of its own.

The core is shaped so that an app decides nothing. `Model::update` takes an
`Event` and returns the `Effect`s to run next, plain data; an `EffectRunner` runs
each effect against ten traits (the API, sign-in, settings, opening a link, the
clock, live updates, notifications, presence, the call engine and the ring) and
turns its result into the next event. The app renders the model, forwards what
the user does as events, and implements the few traits only its system can (the
secret store, notifications, the ring, opening a link). The crate documentation
of `district-core` has the whole shape.

## The crates

| Crate | What it holds |
| --- | --- |
| `district-model` | Serde data types for the District AI API, mirroring the Android app's core model, and the live telemetry envelopes. `Platform` and `ClientIdentity` name the app on the wire. No IO. |
| `district-api` | The HTTP client and the endpoint table. A bearer token and an explicit workspace on every call, no cookie store, redirects never followed, rustls. |
| `district-auth` | Sign-in with OAuth 2.0 and PKCE through the system browser, the token exchange, single-flight refresh-token rotation, sign-out. |
| `district-live` | The live telemetry WebSocket client. |
| `district-core` | App state with no user interface toolkit and no IO of its own: the session, routes, role capabilities, a model per screen, and the effect runner with the traits an app implements. |
| `district-host` | What a desktop app keeps on the machine and how it hears the machine sleep, on any operating system: the device id, the refresh marker, the settings file and the sleep protocol. |
| `district-call` | The call engine: `LiveKitCallEngine` behind the optional `livekit` feature (it links libwebrtc), and `UnavailableCallEngine` without it, which joins nothing and says so. |

`contracts/` holds what the crates are tested against: the District AI server's
recorded responses (vendored and sanitised), the Android app's endpoint list and
the brand colours from the design tokens. CONTRIBUTING.md says where each comes
from.

## Using it from an app

None of the crates is published to crates.io. An app depends on them by git tag,
with the exact version beside the tag so that Cargo refuses a tag that does not
hold that version:

```toml
[workspace.dependencies]
district-model = { git = "https://github.com/distronode-corporation/district-core-rust", tag = "v1.0.0", version = "=1.0.0" }
district-core = { git = "https://github.com/distronode-corporation/district-core-rust", tag = "v1.0.0", version = "=1.0.0" }
```

and so on for each crate it uses, all at the same tag. Cargo.lock then records
the commit the tag named, and `--locked` holds the build to it. A breaking change
to anything an app calls or implements takes a new major version (CHANGELOG.md
says what counts).

`log` is a dependency for its `max_level_warn` feature only, which compiles every
log line below warn out of the whole build, the app's own crates included; see
[SECURITY.md](SECURITY.md#logging) for why.

## Build and test

Requirements: Rust 1.92 or newer (via [rustup](https://rustup.rs); the repository
pins the stable channel) and a C compiler and linker for the TLS library
(`build-essential` on Debian and Ubuntu, the Visual Studio Build Tools on
Windows). No OpenSSL is needed: HTTPS is rustls. No user interface toolkit, no
display and no network are needed to run the tests.

```
git clone https://github.com/distronode-corporation/district-core-rust
cd district-core-rust
cargo test --workspace --locked
```

On Windows, set `AWS_LC_SYS_PREBUILT_NASM=1` first, so the TLS library uses its
prebuilt assembly rather than looking for NASM;
[CONTRIBUTING.md](CONTRIBUTING.md#windows) has the details. Building the call
engine (`--features district-call/livekit`) links libwebrtc and needs more;
[CONTRIBUTING.md](CONTRIBUTING.md#building-with-calls) has the steps.

## Contributing, security and conduct

- [CONTRIBUTING.md](CONTRIBUTING.md): the layout, the local checks CI runs, the
  coverage floors, and the commit message rule.
- [SECURITY.md](SECURITY.md): report vulnerabilities privately through
  [GitHub's private vulnerability reporting](https://github.com/distronode-corporation/district-core-rust/security/advisories/new),
  not in a public issue. It also describes what these crates guarantee about
  sign-in, tokens, requests and logging.
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md): everyone taking part is expected to
  follow it.
- [SUPPORT.md](.github/SUPPORT.md): where questions, bugs and product support go.

## License and trademarks

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Third-party
dependencies keep their own licences.

District AI, Distronode and the District AI and Distronode logos are trademarks
of Distronode Corporation. They are not licensed under the Apache License 2.0.

This project is not affiliated with or endorsed by LiveKit.
