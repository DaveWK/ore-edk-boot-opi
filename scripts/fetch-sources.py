#!/usr/bin/env python3
"""Fetch every pinned source tree this repository builds from (no git submodules).

Each patches/<group>/pin.env pins one or more upstream projects. For a project
with prefix <P> (say EDK2_PLATFORMS) it holds:

  <P>_URL         upstream repository
  <P>_BASE        base commit
  <P>_REF         optional ref to fetch when the host refuses fetch-by-commit
                  (e.g. refs/tags/v7.2-dts); the result must still be <P>_BASE
  <P>_TREE        tree the checkout must have after the patches are applied
  <P>_DIR         optional checkout directory (default: <p> with _ -> -)
  <P>_SUBMODULES  optional: the upstream project's own submodules to check out,
                  "all", "recursive" (all, nested too) or a list of paths
  <P>_PATHS       optional: check out only these directories (sparse, blobs
                  fetched on demand), for large read-only reference trees; the
                  commit and tree are still checked in full. No patches.

Patches are `git format-patch` files of real commits on the base, in
patches/<group>/<project>/ (or patches/<group>/ when the group is the project).
For each project, a checkout already at <P>_TREE is left alone; otherwise the
base is fetched (shallow) into <root>/<dir>, the patches are applied with
`git am --keep-cr` (EDK2 keeps CRLF sources) under a fixed committer so the
commits are reproducible, and the tree is checked against the pin.

SOURCES_REFERENCE=<dir> (optional) borrows objects from <dir>/<dir-name>, e.g.
a local mirror, instead of downloading them.

Usage: fetch-sources.py [--root DIR] [PROJECT-DIR...]
"""

import argparse
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent
COMMITTER = {
    "GIT_COMMITTER_NAME": "ore-edk-boot",
    "GIT_COMMITTER_EMAIL": "ore-edk-boot@localhost",
}


def die(message):
    print(f"fetch-sources: {message}", file=sys.stderr)
    sys.exit(1)


def read_env(path):
    env = {}
    for line in path.read_text().splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, value = line.split("=", 1)
            env[key.strip()] = value.strip().strip('"')
    return env


def git(cwd, *args, env=None, capture=True):
    result = subprocess.run(
        ["git", "-C", str(cwd), *args],
        capture_output=capture,
        text=True,
        env={**os.environ, **(env or {})},
    )
    if result.returncode:
        detail = result.stderr.strip() if capture else ""
        die(f"git {' '.join(args)} in {cwd} failed\n{detail}")
    return result.stdout.strip() if capture else ""


def current_tree(dest):
    if not (dest / ".git").exists():
        return None
    result = subprocess.run(
        ["git", "-C", str(dest), "rev-parse", "HEAD^{tree}"],
        capture_output=True,
        text=True,
    )
    if result.returncode:
        return None
    # Changes inside the project's own submodules do not count (its build may
    # patch them, as edk2-rk3588 does); a submodule at another commit does.
    dirty = git(
        dest,
        "status",
        "--porcelain",
        "--untracked-files=no",
        "--ignore-submodules=dirty",
    )
    return None if dirty else result.stdout.strip()


def projects():
    """Yield (group, prefix, pin) for every project in patches/*/pin.env."""
    for pin_file in sorted(HERE.glob("patches/*/pin.env")):
        pin = read_env(pin_file)
        for key in pin:
            if key.endswith("_URL"):
                yield pin_file.parent.name, key[: -len("_URL")], pin


def fetch(group, prefix, pin, root):
    url, base, tree = (pin.get(f"{prefix}_{k}") for k in ("URL", "BASE", "TREE"))
    if not (url and base and tree):
        die(f"patches/{group}/pin.env needs {prefix}_URL, _BASE and _TREE")
    name = pin.get(f"{prefix}_DIR", prefix.lower().replace("_", "-"))
    patch_dir = HERE / "patches" / group
    if name != group:
        patch_dir = patch_dir / name
    dest = root / name
    paths = pin.get(f"{prefix}_PATHS", "").split()
    if current_tree(dest) == tree:
        print(f"{name}: already at tree {tree[:12]}")
    else:
        if dest.exists() and any(dest.iterdir()):
            die(f"{dest} exists but is not the pinned tree; move it aside first")
        dest.mkdir(parents=True, exist_ok=True)
        git(dest, "init", "-q")
        if paths:
            git(dest, "sparse-checkout", "set", *paths)
        reference = os.environ.get("SOURCES_REFERENCE")
        objects = Path(reference, name, ".git", "objects") if reference else None
        if objects and objects.is_dir():
            (dest / ".git/objects/info/alternates").write_text(f"{objects}\n")
        present = subprocess.run(
            ["git", "-C", str(dest), "cat-file", "-e", f"{base}^{{commit}}"],
            capture_output=True,
        )
        if present.returncode:
            want = pin.get(f"{prefix}_REF", base)
            blobless = ["--filter=blob:none"] if paths else []
            git(
                dest, "fetch", "-q", "--depth", "1", *blobless, url, want, capture=False
            )
            got = git(dest, "rev-parse", "FETCH_HEAD^{commit}")
            if got != base:
                die(f"{name}: {want} is {got}, pin wants {base}")
        git(dest, "checkout", "-q", "--detach", base)
        patches = sorted(patch_dir.glob("*.patch")) if patch_dir.is_dir() else []
        if patches and paths:
            die(f"{name}: a sparse checkout ({prefix}_PATHS) cannot take patches")
        if patches:
            git(
                dest,
                "am",
                "-q",
                "--keep-cr",
                "--committer-date-is-author-date",
                *map(str, patches),
                env=COMMITTER,
            )
        got = git(dest, "rev-parse", "HEAD^{tree}")
        if got != tree:
            die(f"{name}: tree {got} after {len(patches)} patches, pin wants {tree}")
        print(f"{name}: base {base[:12]} + {len(patches)} patches = tree {tree[:12]}")
    submodules = pin.get(f"{prefix}_SUBMODULES", "").split()
    if submodules:
        recursive = ["--recursive"] if submodules == ["recursive"] else []
        paths = [] if submodules in (["all"], ["recursive"]) else submodules
        git(
            dest,
            "submodule",
            "update",
            "--init",
            *recursive,
            "--depth",
            "1",
            "--",
            *paths,
            capture=False,
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=HERE)
    parser.add_argument("only", nargs="*", help="project directories to fetch")
    args = parser.parse_args()
    found = list(projects())
    if not found:
        die("no patches/*/pin.env")
    for group, prefix, pin in found:
        name = pin.get(f"{prefix}_DIR", prefix.lower().replace("_", "-"))
        if not args.only or name in args.only:
            fetch(group, prefix, pin, args.root.resolve())


if __name__ == "__main__":
    main()
