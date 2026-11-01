#!/usr/bin/env python3
"""Keeps cargo's build folder from filling the disk.

Every worktree builds into one shared target folder (`.cargo/config.toml`), and cargo never
removes what it built: a build of another branch, feature set or test target leaves its own copy
of every crate beside the last, and incremental builds leave old sessions. This trims it:

- in `deps`, each crate keeps the `KEEP` builds (hashes) touched most recently, and the others go
  with their fingerprints and build-script output, so cargo rebuilds one if it is wanted again;
- in `incremental`, each crate keeps its `KEEP` most recent directories;
- object files a build left beside its binary and nothing links any more go, and build-script
  output and fingerprints untouched for two weeks;
- and if the folder is still over `BUDGET_GB`, a profile folder over its share is removed whole,
  unless a cargo is running.

    scripts/trim-target.py           # trim now and say what was freed
    scripts/trim-target.py --auto    # only when due, quietly: the last trim was over
                                     # EVERY_HOURS ago, or the disk has under MIN_FREE_GB free
    scripts/trim-target.py --hook    # --auto for a Claude Code PreToolUse hook: reads the tool call
                                     # on stdin and acts only for a cargo command or the shoot

The settings can be changed with ARKOS_TRIM_KEEP, ARKOS_TARGET_BUDGET_GB,
ARKOS_TRIM_EVERY_HOURS and ARKOS_MIN_FREE_GB. It only ever deletes inside the target folder.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

KEEP = int(os.environ.get("ARKOS_TRIM_KEEP", "6"))
BUDGET_GB = float(os.environ.get("ARKOS_TARGET_BUDGET_GB", "40"))
EVERY_HOURS = float(os.environ.get("ARKOS_TRIM_EVERY_HOURS", "6"))
MIN_FREE_GB = float(os.environ.get("ARKOS_MIN_FREE_GB", "20"))
# Files younger than this may belong to a build still running, so they are left alone.
SETTLE_SECS = 3600
OLD_DAYS = 14
GB = 1e9

ARTIFACT = re.compile(r"^(?P<crate>[A-Za-z0-9_]+)-(?P<hash>[0-9a-f]{16})(?:[.-]|$)")
INCREMENTAL = re.compile(r"^(?P<crate>[A-Za-z0-9_]+)-[0-9a-z]+$")


def target_dir(start: Path) -> Path:
    """The target folder cargo builds into from `start`: CARGO_TARGET_DIR, or the nearest
    `.cargo/config.toml` that names a `target-dir`, or `target` at the repository's root."""
    if os.environ.get("CARGO_TARGET_DIR"):
        return Path(os.environ["CARGO_TARGET_DIR"])
    for d in [start, *start.parents]:
        config = d / ".cargo" / "config.toml"
        if config.is_file():
            m = re.search(r'^\s*target-dir\s*=\s*"([^"]+)"', config.read_text(), re.M)
            if m:
                return (d / m.group(1)).resolve()
    return start / "target"


def free_gb(path: Path) -> float:
    probe = path if path.exists() else path.parent
    return shutil.disk_usage(probe).free / GB


def size_gb(path: Path) -> float:
    """What a folder holds on disk, each hard-linked file counted once."""
    seen, total = set(), 0
    for root, _, files in os.walk(path):
        for f in files:
            try:
                st = os.lstat(os.path.join(root, f))
            except FileNotFoundError:
                continue
            if (st.st_dev, st.st_ino) not in seen:
                seen.add((st.st_dev, st.st_ino))
                total += st.st_blocks * 512
    return total / GB


def remove(path: Path) -> None:
    if path.is_dir() and not path.is_symlink():
        shutil.rmtree(path, ignore_errors=True)
    else:
        try:
            path.unlink()
        except FileNotFoundError:
            pass


def cargo_running() -> bool:
    out = subprocess.run(["pgrep", "-x", "cargo|rustc"], capture_output=True, text=True)
    return out.returncode == 0


def prune_builds(profile: Path, now: float) -> None:
    """Each crate keeps its KEEP most recently touched builds in `deps`; the rest go, with their
    fingerprints and build-script folders, which carry the same hash."""
    deps = profile / "deps"
    if not deps.is_dir():
        return
    newest: dict[str, dict[str, float]] = {}
    for entry in os.scandir(deps):
        m = ARTIFACT.match(entry.name)
        if not m:
            continue
        try:
            mtime = entry.stat(follow_symlinks=False).st_mtime
        except FileNotFoundError:
            continue
        builds = newest.setdefault(m["crate"], {})
        builds[m["hash"]] = max(builds.get(m["hash"], 0.0), mtime)
    stale = set()
    for builds in newest.values():
        ranked = sorted(builds.items(), key=lambda kv: kv[1], reverse=True)
        stale.update(h for h, mtime in ranked[KEEP:] if now - mtime > SETTLE_SECS)
    if not stale:
        return
    for folder in (deps, profile / ".fingerprint", profile / "build"):
        if not folder.is_dir():
            continue
        for entry in os.scandir(folder):
            m = ARTIFACT.match(entry.name) or re.match(r"^.+-(?P<hash>[0-9a-f]{16})$", entry.name)
            if m and m["hash"] in stale:
                remove(Path(entry.path))


def prune_incremental(profile: Path, now: float) -> None:
    """Each crate keeps its KEEP most recent incremental folders."""
    inc = profile / "incremental"
    if not inc.is_dir():
        return
    by_crate: dict[str, list[tuple[float, str]]] = {}
    for entry in os.scandir(inc):
        m = INCREMENTAL.match(entry.name)
        if m and entry.is_dir(follow_symlinks=False):
            by_crate.setdefault(m["crate"], []).append((entry.stat(follow_symlinks=False).st_mtime, entry.path))
    for dirs in by_crate.values():
        dirs.sort(reverse=True)
        for mtime, path in dirs[KEEP:]:
            if now - mtime > SETTLE_SECS:
                remove(Path(path))


def prune_old(profile: Path, now: float) -> None:
    """Build-script output and fingerprints nothing has touched for OLD_DAYS: a build script runs
    again, and a unit rebuilds, if one is wanted after all."""
    for folder in (profile / "build", profile / ".fingerprint"):
        if not folder.is_dir():
            continue
        for entry in os.scandir(folder):
            try:
                mtime = entry.stat(follow_symlinks=False).st_mtime
            except FileNotFoundError:
                continue
            if now - mtime > OLD_DAYS * 86400:
                remove(Path(entry.path))


def prune_orphans(profile: Path, now: float) -> None:
    """Object files a build left in `deps` for its debug info that no incremental session links
    any more: a later build replaced them."""
    deps = profile / "deps"
    if not deps.is_dir():
        return
    for entry in os.scandir(deps):
        if not entry.name.endswith(".o"):
            continue
        try:
            st = entry.stat(follow_symlinks=False)
        except FileNotFoundError:
            continue
        if st.st_nlink == 1 and now - st.st_mtime > SETTLE_SECS:
            remove(Path(entry.path))


def trim(target: Path, quiet: bool) -> None:
    if not target.is_dir():
        return
    before = free_gb(target)
    now = time.time()
    profiles = [p for p in target.iterdir() if p.is_dir() and (p / "deps").is_dir()]
    for profile in profiles:
        prune_builds(profile, now)
        prune_incremental(profile, now)
        prune_old(profile, now)
        prune_orphans(profile, now)
    size = size_gb(target)
    if size > BUDGET_GB:
        if cargo_running():
            print(f"trim-target: {target} is {size:.0f} GB, over its {BUDGET_GB:.0f} GB budget, but a cargo is running; run scripts/trim-target.py again once it is done", file=sys.stderr)
        else:
            for profile in sorted(profiles, key=size_gb, reverse=True):
                remove(profile)
                print(f"trim-target: {target} was over its {BUDGET_GB:.0f} GB budget; removed {profile.name}, which the next build makes again", file=sys.stderr)
                if size_gb(target) <= BUDGET_GB:
                    break
    (target / ".trimmed").touch()
    freed = free_gb(target) - before
    if not quiet or freed >= 1.0:
        print(f"trim-target: freed {max(freed, 0.0):.1f} GB; {target} is {size_gb(target):.1f} GB, {free_gb(target):.0f} GB free on the disk", file=sys.stderr)


def due(target: Path) -> bool:
    stamp = target / ".trimmed"
    stale = not stamp.exists() or time.time() - stamp.stat().st_mtime > EVERY_HOURS * 3600
    return stale or free_gb(target) < MIN_FREE_GB


def main() -> int:
    args = sys.argv[1:]
    if "--hook" in args:
        try:
            call = json.load(sys.stdin)
        except ValueError:
            return 0
        command = str(call.get("tool_input", {}).get("command", ""))
        if not re.search(r"\bcargo\b|shoot\.sh", command):
            return 0
    # The folder this repository builds into, wherever it was run from.
    target = target_dir(Path(__file__).resolve().parent.parent)
    auto = "--auto" in args or "--hook" in args
    if auto and not due(target):
        return 0
    trim(target, quiet=auto)
    return 0


if __name__ == "__main__":
    sys.exit(main())
