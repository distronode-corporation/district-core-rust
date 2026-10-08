#!/usr/bin/env python3
"""Assert that the version is written down consistently, and optionally that it
matches a release tag.

    python3 scripts/check-version.py            # the workspace, CHANGELOG.md,
                                                # README.md and SECURITY.md agree
    python3 scripts/check-version.py v1.0.0     # ...and with this tag
    python3 scripts/check-version.py 1.0.0      # a bare version works too

Where the version lives:

    Cargo.toml           [workspace.package] version   the one literal
    crates/*/Cargo.toml  version.workspace = true      inherit it, never restate it
    CHANGELOG.md         the newest `## [x.y.z] - date` once a release exists
    README.md and        the current release, the tag  the newest release in
    SECURITY.md          the apps pin, the series      CHANGELOG.md

A member crate that writes its own version literal is an error even when the
number agrees today, because nothing would keep it agreeing after the next bump.

The changelog check starts to bite at the first release. Until then
`## [Unreleased]` is the only heading and there is nothing to compare.

README.md and SECURITY.md tell people which release to pin and which is
supported: the release they call current, the tag in the dependency lines an
app writes, and the supported `x.y.x` series. Every such mention must name the
newest release in CHANGELOG.md, so the pull request that makes a release changes
them with it. Each file must still hold at least one mention, so that rewording
it cannot quietly retire the check.

With a tag, the version must equal the tag and CHANGELOG.md must have a section
for it, because that section is what the release notes are made from.

Run by the `repo` job in .github/workflows/ci.yml on every push and pull request,
with no argument.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# `## [1.2.3] - 2026-01-31`, the Keep a Changelog release heading. The first
# capture is whatever sits between the brackets (`Unreleased` is filtered out
# afterwards), the second the date after it, when there is one.
HEADING = re.compile(r"^## \[([^\]]+)\](?: - (\S+))?", re.MULTILINE)
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
SEMVER = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$")

# Where README.md and SECURITY.md name the current release. Each pattern's one
# group is a version, except SERIES, whose group is the `x.y` of `x.y.x`. The
# third is the tag in the dependency lines an app writes (README.md, "Using it
# from an app"), `tag = "vX.Y.Z"`, and the fourth the exact version beside it,
# `version = "=X.Y.Z"`.
RELEASE_MENTIONS = (
    re.compile(r"\b(\d+\.\d+\.\d+\S*?) is the current release"),
    re.compile(r"\[(\d[^\]]*)\]\(https://github\.com/distronode-corporation/district-core-rust/releases/latest\)"),
    re.compile(r"\btag = \"v(\d[^\"]*)\""),
    re.compile(r"\bversion = \"=(\d[^\"]*)\""),
)
SERIES = re.compile(r"\b(\d+\.\d+)\.x \(the current release")
RELEASE_DOCS = ("README.md", "SECURITY.md")


def load_toml(path: Path) -> dict:
    with path.open("rb") as fh:
        return tomllib.load(fh)


def workspace() -> tuple[str, list[str]]:
    manifest = load_toml(ROOT / "Cargo.toml")["workspace"]
    return manifest["package"]["version"], manifest["members"]


def members_restating_a_version(members: list[str]) -> list[str]:
    """Members whose `version` is anything other than `{ workspace = true }`."""
    bad = []
    for member in members:
        package = load_toml(ROOT / member / "Cargo.toml")["package"]
        if package.get("version") != {"workspace": True}:
            bad.append(f"{member}/Cargo.toml: version = {package.get('version')!r}")
    return bad


def changelog_releases() -> list[tuple[str, str]]:
    """(version, date) of each release in CHANGELOG.md, newest first (the file's
    own order). The date is empty when the heading has none."""
    text = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    return [(v, d) for v, d in HEADING.findall(text) if v.lower() != "unreleased"]


def release_mention_errors(released: str, name: str, text: str) -> list[str]:
    """Each mention of the current release in `text` (the file `name`) that is
    not `released`, or one error if the file mentions no release at all."""
    errors = []
    found = 0
    for number, line in enumerate(text.splitlines(), start=1):
        for pattern in RELEASE_MENTIONS:
            for match in pattern.finditer(line):
                found += 1
                if match.group(1) != released:
                    errors.append(
                        f"{name}:{number} names {match.group(1)} as the current release,"
                        f" but CHANGELOG.md's newest release is {released}"
                    )
        for match in SERIES.finditer(line):
            found += 1
            series = released.rsplit(".", 1)[0]
            if match.group(1) != series:
                errors.append(
                    f"{name}:{number} supports {match.group(1)}.x as the current series,"
                    f" but CHANGELOG.md's newest release is {released}"
                )
    if not found:
        errors.append(
            f"{name} no longer names the current release anywhere this script looks"
            " (RELEASE_MENTIONS); change the patterns with the wording"
        )
    return errors


def main(argv: list[str]) -> int:
    version, members = workspace()
    errors: list[str] = []

    if not SEMVER.match(version):
        errors.append(f"Cargo.toml: {version!r} is not a semantic version")

    errors.extend(members_restating_a_version(members))

    dated_releases = changelog_releases()
    releases = [release for release, _ in dated_releases]
    latest = releases[0] if releases else None
    for release, date in dated_releases:
        if not SEMVER.match(release):
            errors.append(f"CHANGELOG.md: heading [{release}] is not a semantic version")
        if not DATE.match(date):
            errors.append(f"CHANGELOG.md: heading [{release}] needs its date, `- YYYY-MM-DD`")

    print(f"  Cargo.toml    {version} ({len(members)} workspace members)")
    print(f"  CHANGELOG.md  {latest or '(no release yet)'}")

    if latest is not None and latest != version:
        errors.append(
            f"CHANGELOG.md's newest release is {latest} but Cargo.toml says {version}"
        )

    if latest is not None:
        for name in RELEASE_DOCS:
            text = (ROOT / name).read_text(encoding="utf-8")
            errors.extend(release_mention_errors(latest, name, text))

    if len(argv) > 1:
        # Accept `v0.1.0` and `0.1.0`. Anything else is a mistake worth stopping
        # on rather than normalising away.
        tag = argv[1]
        expected = tag[1:] if tag.startswith("v") else tag
        if expected != version:
            errors.append(f"the tree says {version} but the tag says {tag}")
        if expected not in releases:
            errors.append(f"CHANGELOG.md has no ## [{expected}] section for tag {tag}")

    if errors:
        print("", file=sys.stderr)
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1

    suffix = f" and tag {argv[1]}" if len(argv) > 1 else ""
    print(f"\nversion {version} is consistent across the workspace{suffix}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
