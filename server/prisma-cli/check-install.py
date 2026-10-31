#!/usr/bin/env python3
"""Check that the self-contained prisma-cli install matches the workspace lockfile.

Usage: check-install.py <workspace pnpm-lock.yaml> <package dir>

The server image installs server/prisma-cli with its package store inside the
package (node_modules/.pnpm). Every package installed there (one per entry of
node_modules/.pnpm) must exist with exactly that name and version in the
`packages:` section of the workspace lockfile, the direct dependencies must be
the versions the lockfile records for the prisma-cli importer, and
node_modules/.bin/prisma must exist.
Standard library only; the lockfile is read as text (pnpm lockfile v9).
"""

import json
import os
import re
import sys

IMPORTER = "server/prisma-cli"


def fail(msg):
    print(f"check-install: FAIL: {msg}")
    sys.exit(1)


def read_lock(path):
    packages = set()
    importer = {}
    section = None
    imp = None
    dep = None
    with open(path, encoding="utf-8") as f:
        for raw in f:
            line = raw.rstrip("\n")
            top = re.match(r"^(\S[^:]*):\s*$", line)
            if top:
                section = top.group(1)
                continue
            if section == "packages":
                m = re.match(r"^  '?(.+?)'?:\s*$", line)
                if m:
                    packages.add(m.group(1))
            elif section == "importers":
                m = re.match(r"^  (\S+):\s*$", line)
                if m:
                    imp = m.group(1)
                    continue
                m = re.match(r"^      '?(.+?)'?:\s*$", line)
                if m:
                    dep = m.group(1)
                    continue
                m = re.match(r"^        version: (\S+)\s*$", line)
                if m and imp == IMPORTER:
                    # strip the peer suffix: 7.7.0(react@19.2.5) -> 7.7.0
                    importer[dep] = m.group(1).split("(", 1)[0]
    return packages, importer


def package_json(path):
    with open(os.path.join(path, "package.json"), encoding="utf-8") as f:
        data = json.load(f)
    return data["name"], data["version"]


def main():
    if len(sys.argv) != 3:
        fail("usage: check-install.py <pnpm-lock.yaml> <package dir>")
    lock_path, package = sys.argv[1], sys.argv[2]
    packages, importer = read_lock(lock_path)
    if not packages:
        fail(f"no packages found in {lock_path}")
    if set(importer) != {"prisma", "dotenv"}:
        fail(f"importer {IMPORTER} in the lockfile has {sorted(importer)}, want dotenv and prisma")

    store = os.path.join(package, "node_modules", ".pnpm")
    checked = 0
    mismatches = []
    for entry in sorted(os.listdir(store)):
        entry_dir = os.path.join(store, entry, "node_modules")
        if not os.path.isdir(entry_dir) or entry == "node_modules":
            continue
        # The entry is "<name, with / as +>@<version>[_<peers>]"; the package itself lives
        # under node_modules/<name>. Names contain no "@" except the scope prefix.
        m = re.match(r"^(@[^+@]+\+[^@]+|[^@]+)@", entry)
        if not m:
            mismatches.append(f"{entry}: unexpected entry name")
            continue
        pkg_dir = os.path.join(entry_dir, *m.group(1).replace("+", "/", 1).split("/"))
        if not os.path.isfile(os.path.join(pkg_dir, "package.json")):
            mismatches.append(f"{entry}: no package.json at {pkg_dir}")
            continue
        name, version = package_json(pkg_dir)
        checked += 1
        if f"{name}@{version}" not in packages:
            mismatches.append(f"{name}@{version} (from {entry}) is not in the workspace lockfile")

    for dep, want in sorted(importer.items()):
        link = os.path.join(package, "node_modules", dep)
        if not os.path.isfile(os.path.join(link, "package.json")):
            mismatches.append(f"direct dependency {dep} is missing")
            continue
        _, have = package_json(link)
        if have != want:
            mismatches.append(f"direct dependency {dep} is {have}, the lockfile has {want}")

    if not os.path.exists(os.path.join(package, "node_modules", ".bin", "prisma")):
        mismatches.append("node_modules/.bin/prisma is missing")
    if checked == 0:
        fail(f"no packages found under {store}")
    if mismatches:
        for m in mismatches:
            print(f"check-install: {m}")
        fail(f"{len(mismatches)} problem(s) in {package}")
    print(f"check-install: OK: {checked} packages in {package} match {lock_path}")


if __name__ == "__main__":
    main()
