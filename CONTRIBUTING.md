# Contributing to the District AI core for Rust

Issues and pull requests are welcome. This guide covers how the repository is
laid out, how to build and test it, and what CI holds every change to.

These crates are the core of two apps, District AI for Linux and District AI for
Windows, and a change here reaches users only when an app moves its pin to a
release that holds it. A change that an app needs usually comes with a pull
request in that app's repository, linked from this one.

Questions, build trouble and ideas go to
[Discussions](https://github.com/distronode-corporation/district-core-rust/discussions);
the issue tracker is for reproducible bugs.

## Layout

```
crates/district-model/    Serde data types for the District AI API, mirroring the
                          Android app's core model (district-android, public),
                          and the live telemetry envelopes; `Platform` and
                          `ClientIdentity`, which name the app on the wire. No IO.
crates/district-api/      The HTTP client and the endpoint table. A bearer token and
                          an explicit workspace on every call, no cookie store,
                          redirects never followed.
crates/district-auth/     Sign-in with OAuth 2.0 and PKCE through the system
                          browser, the token exchange, single-flight refresh-token
                          rotation, sign-out.
crates/district-live/     The live telemetry WebSocket client.
crates/district-core/     App state with no user interface toolkit and no IO of its
                          own: the session, routes, role capabilities, a model per
                          screen, and the effect runner with the traits an app
                          implements, `CallEngine` among them.
crates/district-host/     What a desktop app keeps on the machine and how it
                          hears the machine sleep, on any operating system: the
                          device id, the refresh marker, the settings file and
                          the sleep protocol.
crates/district-call/     The call engine: `LiveKitCallEngine`, behind the
                          optional `livekit` feature, off by default because it
                          links libwebrtc; without it a build has
                          `UnavailableCallEngine`, which joins nothing and says
                          so. See "Building with calls" below.
contracts/                What these crates are checked against: the server's
                          recorded responses, the Android set and the desktop-only
                          set (vendored and sanitised by sync-contracts.py), the
                          Android app's endpoint snapshot (sync-endpoints.py), and
                          the brand colours from the design tokens
                          (sync-palette.py).
scripts/                  check-version.py, check-public-hygiene.py and
                          check-coverage.py, run by CI; fetch-libwebrtc.sh, run by
                          voice.yml and by hand for a build with calls;
                          sync-contracts.py, sync-endpoints.py and
                          sync-palette.py (which share _common.py), run by hand,
                          and sync-endpoints.py weekly by endpoints.yml.
coverage-floors.toml      Each crate's line coverage floor (see Coverage below).
```

Dependencies point one way: `district-model` at the bottom, `district-core` above
the client crates, and `district-host` and `district-call` on top of the core.
Nothing here depends on a user interface toolkit or on a crate that builds on only
one operating system, which is what lets both apps build on every crate, and lets
every test run without a display, a keyring or a network.

## Setup

You need Rust 1.92 or newer (via [rustup](https://rustup.rs);
`rust-toolchain.toml` pins the stable channel with rustfmt and clippy) and a C
compiler and linker, which the TLS library (aws-lc-rs, through rustls) needs.
Python 3.11 or newer runs the scripts.

```
cargo test --workspace --locked
```

## Windows

Everything builds and every test runs on Windows x64 with the MSVC toolchain
(`x86_64-pc-windows-msvc`), which is what District AI for Windows ships, and CI
runs the tests there (the `windows` job). You need the Visual Studio Build Tools
with the "Desktop development with C++" workload, and one environment variable:

```
$env:AWS_LC_SYS_PREBUILT_NASM = "1"
cargo test --workspace --locked
```

aws-lc-sys, the C library under rustls's crypto provider, assembles its
optimised code with NASM on Windows x64, and NASM is not part of the Build Tools.
`AWS_LC_SYS_PREBUILT_NASM=1` makes it use the object files it ships already
assembled instead, so no NASM is needed. Without the variable, and without NASM
on the `PATH`, its build script fails.

Two things differ on Windows by design:

- `district-host` creates its files with the default permissions of the
  directory they are in (the app's own data directory), because the owner-only
  modes it sets on Unix have no Windows equivalent in the standard library.
- Line endings: `.gitattributes` keeps every text file LF in every checkout,
  Windows included, and never converts anything under `contracts/`, whose bytes
  `contracts/SHA256SUMS` pins. A checkout made before `.gitattributes` existed,
  or with a global setting that overrides it, fails the checksum test; clone
  again.

The call engine (`livekit`) is not built on Windows here: District AI for Windows
builds its own libwebrtc, and its repository tests the engine with it.

## Building with calls

Calls, meeting rooms and auditions need the LiveKit call engine, which a default
build leaves out: district-call's `livekit` feature turns it on. It statically
links libwebrtc, a prebuilt C++ library that District AI for Linux builds from
LiveKit's recipe without the H.264 and H.265 codecs (see NOTICE), so on Linux it
needs a little more than the default build:

- clang and clang++ 21.1 or newer. The prebuilt library is built against
  Chromium's own libc++, which needs it, and the build refuses GCC, which would
  compile but miscall it. Ubuntu 26.04's `clang` is 21; on Ubuntu 24.04 install
  `clang-21` from the LLVM project's apt repository
  (`.github/actions/install-clang-21` has the lines) and
  set `CXX=clang++-21`.
- The headers it compiles against: `libglib2.0-dev libx11-dev libxext-dev
  libxfixes-dev libxdamage-dev libxrandr-dev libxcomposite-dev libgl1-mesa-dev
  libdrm-dev libgbm-dev libva-dev libpulse-dev`. It links none of them; the
  X11, DRM, VA and PulseAudio libraries are loaded when used.
- The library itself, checked against a pinned SHA-256:

  ```
  export LK_CUSTOM_WEBRTC="$(scripts/fetch-libwebrtc.sh ~/.cache/district-libwebrtc)"
  cargo build -p district-call --features livekit --locked
  ```

  Without `LK_CUSTOM_WEBRTC` the LiveKit SDK's build downloads LiveKit's own
  prebuilt instead, which carries the codecs this project leaves out, and
  checks nothing, so always set it. The script refuses to run when
  Cargo.lock names a different `webrtc-sys-build`, whose libwebrtc would not link;
  its header says how to move the pin with the SDK.

The engine's tests (`crates/district-call/tests/engine/`) run it against a real
media server on this machine, so they also need `livekit-server` (voice.yml
downloads a pinned release; `LIVEKIT_SERVER` names the binary) and, for the test
of the desktop's own audio path, `pulseaudio` and its tools (`pactl`, `parec`),
which the test starts privately with a null sink and a sine source, so nothing is
heard and no real device is touched:

```
LIVEKIT_SERVER=/path/to/livekit-server \
  cargo test -p district-call --features livekit --locked -- --test-threads=1
```

One at a time, because each test measures audio in real time. They print what
they measure with `--nocapture`. libwebrtc never gathers network candidates on
loopback, so the machine needs a network interface other than `lo`, even though
every test stays on it: an ordinary desktop or CI runner has one, and a container
started with `--network none` needs a dummy interface added
(`ip link add lan0 type dummy`, an address, `up`).

A process never holds the desktop's devices and a frame microphone at once, or one
after the other: the engine refuses whichever comes second, because of defects in
the libwebrtc the LiveKit SDK links that have been reported privately upstream
(SECURITY.md says what is refused; details will be published once upstream has
published a fix). The tests keep to it: each test of the devices runs in a child
process of its own, and whoever it talks to is a `FarEnd`, the test binary run
again as an engine on frame audio in a third process, which prints what it hears
for the test to read. A test that put a frame microphone in the child would be
refused, as the last two `devices::` tests show.

## Logging

Nothing in these crates logs or prints: no `println!`, `eprintln!` or `dbg!`
outside tests, and no `log` macro. `log` is a dependency only for its
`max_level_warn` feature, which compiles every line below warn out of a whole
build; SECURITY.md says why, and a test in `district-live` (and, with the engine,
one in `district-call`) fails if it goes. Keep it that way: an error is returned,
not logged, and what reaches a user is the app's decision.

## Releases

A release is a `vX.Y.Z` tag on a commit on main. The apps pin the tag, so a tag
never moves and is never deleted once pushed. Before tagging, in one pull
request:

1. Bump `[workspace.package] version` in `Cargo.toml`. A change to anything an
   app calls or implements is a new major version (CHANGELOG.md says what
   counts), a new feature a new minor one.
2. Rename `## [Unreleased]` in CHANGELOG.md to `## [X.Y.Z] - YYYY-MM-DD` and
   start a new empty `[Unreleased]` above it. That section becomes the release
   notes.
3. Name the new version wherever README.md and SECURITY.md name the current
   release: the status line, the tag and version in "Using it from an app", and
   the supported series.

`python3 scripts/check-version.py` checks the three agree, and with the tag as
its argument checks them against the tag. Once the pull request is merged, tag
the merge commit, push the tag, and create the GitHub release from the
CHANGELOG.md section. Then move each app's pin in a pull request of its own.

## The whole local gate

This is what CI's `rust` and `repo` jobs run:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
python3 scripts/check-version.py
python3 scripts/check-public-hygiene.py --self-test
python3 scripts/check-public-hygiene.py
python3 scripts/check-coverage.py --self-test
```

The call engine is built and tested by its own workflow,
`.github/workflows/voice.yml`, when what it is made of changes, weekly, and by
hand (see "Building with calls"); the default jobs never download libwebrtc.

CI runs those tests with line coverage measured and checks it against the floors;
[Coverage](#coverage) below has the commands to do the same. CI also runs
`cargo test` on Windows (the `windows` job, see [Windows](#windows)),
`cargo check` at the declared minimum Rust version (the `msrv` job),
`cargo deny --locked check` against [deny.toml](deny.toml), which judges both the
Linux and the Windows target, and [zizmor](https://docs.zizmor.sh) over the
workflows. Clippy runs with `-D warnings`, so a warning is a failure. There is
no formatting bot; run `cargo fmt --all` yourself.

## Coverage

CI measures line coverage while the tests run, with
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov), and
`scripts/check-coverage.py` holds every workspace crate to its floor in
[coverage-floors.toml](coverage-floors.toml). To run the same check, install the
tool and the LLVM tools that match your compiler once:

```
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --locked
```

then, as CI does:

```
source <(cargo llvm-cov show-env --sh)
cargo llvm-cov clean --workspace
cargo test --workspace --locked
cargo llvm-cov report --json --summary-only --output-path target/coverage.json
python3 scripts/check-coverage.py target/coverage.json
```

The check prints a table of every crate. `cargo llvm-cov report
--show-missing-lines` lists the lines no test runs, and `cargo llvm-cov report
--open` shows them in a browser.

What the floors mean:

- A floor is the whole-number percentage of a crate's lines under `src/` that the
  tests must run; 100 means every line. Integration tests under `tests/` are not
  measured. Unit tests inside `src/` are, because stable Rust has no way to leave
  them out. Only lines: branch coverage needs a nightly compiler.
- Every crate is held at 100 in its default build, because every line in it can
  be made to run in a test without a desktop session, a display or a live call.
  For `district-call` that build is the crate root and `UnavailableCallEngine`.
- There is no exclusion list. Code CI cannot run stays in the measurement and
  holds its crate's floor down, where everyone can see it.
- A crate whose optional feature builds code the default build does not has a
  second floor for that build: `[features."district-call/livekit"]` is the
  LiveKit engine's, measured by voice.yml and checked with
  `python3 scripts/check-coverage.py --feature district-call/livekit <report>`.
  The two measure different code and are not combined. The engine's floor is
  measured, not a target: what its tests cannot reach is races (a room left in
  the instant its join ends, a report racing a hang-up) and states libwebrtc
  never reports with the web client's key settings.
- A crate missing from the report fails whatever its floor, and a floor above 0
  with no measurable lines fails too, so a run that skipped a crate can never
  read as covered. A new crate needs its entry in the change that adds it.

Floors only ratchet up. Raise a crate's floor in the same change that raises its
coverage; the check prints a note when a crate is a whole point or more above
its floor. Never lower a floor without a stated reason in review.

`python3 scripts/check-coverage.py --self-test` proves each rule still fails what
it should, and CI runs it with the other repository checks.

## Public hygiene

This repository is public, and `scripts/check-public-hygiene.py` fails CI on four
things in any tracked or new file:

- An em dash or an en dash. Use commas, periods or parentheses.
- A phone number in E.164 form. Use the fictional range +1 NPA 555-0100 to
  555-0199 (for example +1 212 555 0142) in tests, fixtures and docs.
- A host name under distronode.com or distronode.ca other than the public
  website's.
- An email address other than the project's own contact addresses, the
  commit-attribution forms, and addresses at example.com (for tests, fixtures
  and docs).

`--self-test` proves each rule still catches what it claims to. If a rule gets in
the way of a legitimate change, change the rule in the same pull request and say
why.

## Contract fixtures

`contracts/` holds JSON bodies recorded from the District AI server by tests in the
server repository, in two sets. `contracts/fixtures/` is the Android app's set,
recorded from the server's own route handlers, which these crates read too.
`contracts/desktop/` holds the shapes only the desktop apps read and no Android
fixture records: the live telemetry credential, one frame of the telemetry socket
per event type, the call hang-up, the booking-pages hand-off and the desktop's
presence registration.

`crates/district-model` decodes every file in both sets in its tests with unknown
fields refused (the `strict-contracts` feature, which its tests always enable), so
a field the server renames or adds fails here rather than in an app. The files
are a snapshot: `contracts/SOURCE.toml` says which server commit they came from,
with a `[sets.<name>]` table for each set, and lists every substitution made to
keep real-looking data out of this public repository; `contracts/SHA256SUMS` pins
the bytes of both sets. Do not edit them by hand; maintainers with access to the
server repository re-run

```
python3 scripts/sync-contracts.py --monorepo <path to the server repository>
```

which vendors both sets from the one commit, and refuses while either source
directory there has changes that commit does not hold.

The fixture manifest in `crates/district-model/tests/contracts/manifest.rs`
accounts for every file, each set against its own pinned count: each is decoded
by a data type, recorded as not yet modelled (a list that may only shrink), or
excluded by a stated decision.

The brand colours come from the design tokens in the website's repository too: a
maintainer with access refreshes `contracts/palette.snapshot.json` with
`python3 scripts/sync-palette.py --monorepo <checkout>`, and a test in
`district-core` then holds `district_core::palette` to it.

## The endpoint table

`crates/district-api` calls the same endpoints as the District AI Android app, which
is the reference client, apart from a named list of exclusions and desktop-only
additions (`EXCLUDED` and `DESKTOP_ONLY`), each with its reason.
`contracts/endpoints.snapshot.json` is the Android app's endpoint list, and
`crates/district-api/tests/endpoint_parity.rs` fails on any difference that is not
on one of those two lists.

The Android app is public, at
[district-android](https://github.com/distronode-corporation/district-android), and
the snapshot is read from its
[`core/core-network/src/main/kotlin/com/distronode/districtai/core/network`](https://github.com/distronode-corporation/district-android/tree/main/core/core-network/src/main/kotlin/com/distronode/districtai/core/network)
directory (with the auth API and the core model beside it). Anyone can refresh it
from a checkout of that repository with
`python3 scripts/sync-endpoints.py --android <checkout>` and commit the result
together with whatever change to the table it calls for; like the other sync
scripts it refuses a checkout with changes its commit does not hold, unless
`--allow-dirty`. The snapshot is committed and CI never regenerates it, but
`.github/workflows/endpoints.yml` checks it against the Android app's main branch
every week, with `--check`, and fails when it is stale.

## Commits and pull requests

Conventional Commits are not required. What is required is that the message says
**why**, not just what: the diff already says what. A good message names the thing
that was wrong, the evidence, and what would have caught it.

Pull requests run `.github/workflows/ci.yml`, and all of it must be green.

## Reporting bugs

Open an issue with the bug report form. For anything security-relevant, do not
open an issue; see [SECURITY.md](SECURITY.md). Questions go to
[Discussions](https://github.com/distronode-corporation/district-core-rust/discussions).

## Licence of contributions

By contributing you agree that your contribution is licensed under the Apache
License 2.0, as section 5 of the licence provides. There is no CLA and no
sign-off requirement.
