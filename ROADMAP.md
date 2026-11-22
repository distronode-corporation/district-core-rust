# Roadmap

The near-term plan for District Core for Rust. This repository has no open issues, so the list
is taken from the repository's own unfinished items, each named with where it comes from. This roadmap covers the next twelve months, to November 2027. Items have no dates of their own:
an item is done when the change that does it is merged and released, and it then leaves this
list for [CHANGELOG.md](CHANGELOG.md). Anything here can change; open an issue to
discuss an item or to propose a new one.

## Near term

- **The next major release, the first with a signed tag.** The setup wizard, "Works offline,
  read-only", signed release tags and the repeatable-build check are merged and listed under
  `[Unreleased]` in [CHANGELOG.md](CHANGELOG.md). The setup wizard adds variants to `Event`,
  `Effect`, `Endpoint` and `Route`, so the release is a new major version. Tags up to and
  including v13.0.0 are not signed (README, "Verify a release"); this release's tag is the first
  that can be checked with `gitsign verify-tag`.
- **Types for the responses that have none yet.** The contract tests list seven of the
  service's recorded responses that the crates will use but have no type for yet:
  `district-dial-dnc`, `district-dial-dormant`, `district-dial-subscription`,
  `district-enrich-disabled`, `district-member-duplicate`, `district-member-last-agency` and
  `district-workspace-list-degraded` (`NOT_YET_MODELLED` in
  [`crates/district-model/tests/contracts/manifest.rs`](crates/district-model/tests/contracts/manifest.rs)).
  The list may only shrink: each entry leaves it with its type.
- **The apps on the current release.** District AI for Linux 3.4.0 and District AI for Windows
  3.4.0 are built on 12.0.0 (README, the status note). Each app moves to a newer release by
  bumping its tag in a pull request of its own, in that app's repository.

## Open questions

- **Using the apps without a District AI account.** Today the apps built on these crates need a
  District AI account to sign in. Whether they can work without one, and what that would look
  like, is an open question we are asking about before building anything (README, "Without an
  account with us";
  [the discussion](https://github.com/distronode-corporation/.github/discussions/1)).

## Not planned

What the crates will not do, as the README and [ARCHITECTURE.md](ARCHITECTURE.md) set it out.

- **Publishing to crates.io.** Apps depend on the crates by git tag, with the exact version
  beside the tag.
- **A user interface toolkit, or a crate that builds on only one operating system.** The apps
  (GTK 4 on Linux, WinUI 3 on Windows) render the model; every crate builds on both.
- **A cache of customer data on disk.** The crates keep only the device id, the refresh marker
  and the settings file on disk, and everything else on the service.
- **A cookie store or following redirects.** `district-api` sends a bearer token and follows no
  redirect.
- **Crash reports by default.** `district-crash` sends a report only once the app passes a DSN
  and the user's consent.
- **The District AI service.** The service the crates talk to is not open source and is not
  part of this repository.

## How the project is run

Who decides what goes on this list, and how, is in the organization's
[GOVERNANCE.md](https://github.com/distronode-corporation/.github/blob/main/GOVERNANCE.md).
