#!/usr/bin/env python3
"""Fails when Cargo.lock holds a crates.io release younger than the minimum release age.

Every dependency is at least 3 days old across the repository: Renovate waits that long
(renovate.json), pnpm refuses younger packages (pnpm-workspace.yaml), and this does the same for
Cargo, which has no such setting of its own. It covers transitive crates, which `cargo update`
and `cargo add` resolve to the newest release with no wait.

    scripts/release-age.py --since origin/main   # entries new or changed since a revision (CI)
    scripts/release-age.py --all                 # every entry

crates.io asks automated clients for about one request per second, so CI checks only what a
change adds: what is on main was checked when it got there.

Exceptions are exact versions (`name@version`) in release-age.toml, each with its reason: a newer
version of the same crate is not excepted.
"""

import argparse
import datetime
import json
import subprocess
import sys
import time
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MINIMUM_DAYS = 3
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"


def registry_entries(lock_text: str) -> set[tuple[str, str]]:
    packages = tomllib.loads(lock_text).get("package", [])
    return {(p["name"], p["version"]) for p in packages if p.get("source") == REGISTRY}


def published(name: str, version: str) -> datetime.datetime:
    request = urllib.request.Request(
        f"https://crates.io/api/v1/crates/{name}/{version}",
        headers={"User-Agent": "html-to-svg-rs release-age check (github.com/kantord/html-to-svg-rs)"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        created = json.load(response)["version"]["created_at"]
    return datetime.datetime.fromisoformat(created)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    which = parser.add_mutually_exclusive_group(required=True)
    which.add_argument("--all", action="store_true", help="check every crates.io entry of Cargo.lock")
    which.add_argument("--since", metavar="REV", help="check entries not in REV's Cargo.lock")
    args = parser.parse_args()

    entries = registry_entries((ROOT / "Cargo.lock").read_text())
    if args.since:
        try:
            before = subprocess.run(
                ["git", "show", f"{args.since}:Cargo.lock"], cwd=ROOT, capture_output=True, text=True, check=True
            ).stdout
            entries -= registry_entries(before)
        except subprocess.CalledProcessError:
            print(f"{args.since} has no Cargo.lock: checking every entry")

    config = tomllib.loads((ROOT / "release-age.toml").read_text())
    excepted = {entry["crate"] for entry in config.get("cargo", {}).get("exception", [])}

    now = datetime.datetime.now(datetime.timezone.utc)
    too_young = []
    for i, (name, version) in enumerate(sorted(entries)):
        if i:
            time.sleep(1)  # crates.io's crawler policy
        age = (now - published(name, version)).total_seconds() / 86400
        if age < MINIMUM_DAYS and f"{name}@{version}" not in excepted:
            too_young.append(f"{name}@{version} ({age:.1f} days old)")

    print(f"{len(entries)} Cargo.lock entries checked, minimum age {MINIMUM_DAYS} days")
    if too_young:
        print("Younger than that (wait, pin an older version, or add an exception to release-age.toml):")
        for entry in too_young:
            print(f"  {entry}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
