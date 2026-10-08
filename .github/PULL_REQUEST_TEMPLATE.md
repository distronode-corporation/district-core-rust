<!-- Thanks for the contribution. Delete any section that genuinely does not apply. -->

## What changed

<!-- One or two sentences. The diff says what; this says it in words. -->

## Why

<!-- The problem, not the patch. If it fixes an issue, link it (Fixes #123). If an app
     needs it, link that app's pull request. If you hit it in practice rather than
     reading the code, say what you saw. -->

## How it was tested

<!-- Commands you actually ran, and what they said. "Should work" is not a test.
     This list is what CI's `rust` and `repo` jobs run (.github/workflows/ci.yml);
     CI also runs the tests on Windows, `cargo check` at the MSRV,
     `cargo deny --locked check` and zizmor. -->

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] Every crate at or above its coverage floor (`python3 scripts/check-coverage.py`
      on the report; the commands are under "Coverage" in CONTRIBUTING.md)
- [ ] `python3 scripts/check-version.py`
- [ ] `python3 scripts/check-public-hygiene.py --self-test` and
      `python3 scripts/check-public-hygiene.py`
- [ ] `python3 scripts/check-coverage.py --self-test`
- [ ] Changes something an app calls or implements, and so CHANGELOG.md says so
      under `[Unreleased]` (a breaking change is a new major version)
- [ ] Touches the call engine, and so the engine's tests were run with
      `--features livekit` (CONTRIBUTING.md, "Building with calls")

## Anything a reviewer should know

<!-- A decision you were unsure about, something you deliberately left out, a follow-up
     you think is needed. Saying "I could not test X" here is useful, not a problem. -->
