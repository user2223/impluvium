#!/usr/bin/env python3
"""Check that the self-contained prisma-cli install is exactly the locked closure.

Usage: check-install.py <workspace pnpm-lock.yaml> <package dir>

The server image installs server/prisma-cli with its package store inside the
package (node_modules/.pnpm). This script walks the dependency graph twice in
parallel: in the lockfile, from the server/prisma-cli importer through the
`snapshots:` section, and on disk, following the links pnpm created. It checks:

- every locked dependency edge exists on disk and leads to a package with the
  locked name and version (an optional dependency may be missing only when the
  lockfile restricts it to other platforms);
- every package directory in node_modules/.pnpm belongs to exactly one locked
  snapshot of that closure (no extra packages, no duplicates);
- the top level of node_modules holds only the importer's direct dependencies;
- every symbolic link under node_modules resolves inside node_modules (no
  dangling links, no links out of the directory, so it can be copied anywhere);
- node_modules/.bin/prisma exists.

Standard library only; the lockfile is read as text (pnpm lockfile v9).
"""

import json
import os
import re
import sys

IMPORTER = "server/prisma-cli"
DEP_SECTIONS = ("dependencies", "optionalDependencies")


def fail(msg):
    print(f"check-install: FAIL: {msg}")
    sys.exit(1)


def unquote(text):
    text = text.strip()
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "'\"":
        return text[1:-1]
    return text


def read_lock(path):
    """Return (importer deps, snapshots, platform-restricted packages).

    The importer and each snapshot map to {"dependencies": {...}, "optionalDependencies": {...}},
    dependency name -> locked reference (version with peer suffix, or an alias).
    """
    importer = {s: {} for s in DEP_SECTIONS}
    snapshots = {}
    restricted = set()
    section = imp = key = sub = None
    with open(path, encoding="utf-8") as f:
        for raw in f:
            line = raw.rstrip("\n")
            if not line.strip():
                continue
            top = re.match(r"^(\S[^:]*):", line)
            if top:
                section, imp, key, sub = top.group(1), None, None, None
                continue
            if section == "importers":
                m = re.match(r"^  (\S.*?):\s*$", line)
                if m:
                    imp, sub, key = unquote(m.group(1)), None, None
                    continue
                if imp != IMPORTER:
                    continue
                m = re.match(r"^    (\w+):\s*$", line)
                if m:
                    sub = m.group(1) if m.group(1) in DEP_SECTIONS else None
                    continue
                m = re.match(r"^      (\S.*?):\s*$", line)
                if m and sub:
                    key = unquote(m.group(1))
                    continue
                m = re.match(r"^        version: (.+)$", line)
                if m and sub and key:
                    importer[sub][key] = unquote(m.group(1))
            elif section == "packages":
                m = re.match(r"^  (\S.*?):(\s*\{\})?\s*$", line)
                if m:
                    key = unquote(m.group(1))
                    continue
                if key and re.match(r"^    (os|cpu|libc):", line):
                    restricted.add(key)
            elif section == "snapshots":
                m = re.match(r"^  (\S.*?):(\s*\{\})?\s*$", line)
                if m:
                    key = unquote(m.group(1))
                    snapshots[key] = {s: {} for s in DEP_SECTIONS}
                    sub = None
                    continue
                m = re.match(r"^    (\w+):\s*$", line)
                if m:
                    sub = m.group(1) if m.group(1) in DEP_SECTIONS else None
                    continue
                m = re.match(r"^      (\S.*?): (.+)$", line)
                if m and sub and key:
                    snapshots[key][sub][unquote(m.group(1))] = unquote(m.group(2))
    return importer, snapshots, restricted


def snapshot_key(dep, ref):
    """Lockfile reference of dependency `dep` -> snapshot key (handles aliases)."""
    if ref.startswith(("link:", "file:")):
        return None
    if not ref[:1].isdigit():
        # alias: "string-width-cjs: string-width@4.2.3" -> key "string-width@4.2.3"
        return ref
    return f"{dep}@{ref}"


def split_key(key):
    """'@scope/name@1.2.3(peer@1)' -> ('@scope/name', '1.2.3')."""
    base = key.split("(", 1)[0]
    name, _, version = base.rpartition("@")
    return name, version


def package_json(path):
    with open(os.path.join(path, "package.json"), encoding="utf-8") as f:
        data = json.load(f)
    return data.get("name"), data.get("version")


def modules_dir_of(pkg_real, name):
    """Real package dir .pnpm/<entry>/node_modules/<name> -> .pnpm/<entry>/node_modules."""
    path = pkg_real
    for _ in name.split("/"):
        path = os.path.dirname(path)
    return path


def inside(path, root):
    return (path + os.sep).startswith(root + os.sep)


def main():
    if len(sys.argv) != 3:
        fail("usage: check-install.py <pnpm-lock.yaml> <package dir>")
    lock_path, package = sys.argv[1], sys.argv[2]
    importer, snapshots, restricted = read_lock(lock_path)
    if not snapshots:
        fail(f"no snapshots found in {lock_path}")
    direct = {**importer["optionalDependencies"], **importer["dependencies"]}
    if set(direct) != {"prisma", "dotenv"}:
        fail(f"importer {IMPORTER} in the lockfile has {sorted(direct)}, want dotenv and prisma")

    modules = os.path.realpath(os.path.join(package, "node_modules"))
    store = os.path.join(modules, ".pnpm")
    if not os.path.isdir(store):
        fail(f"no package store at {store}")
    problems = []
    skipped = set()
    owner = {}  # .pnpm/<entry> dir -> snapshot key
    seen = set()

    # Work list of (parent snapshot key or None, node_modules dir holding the link, name, reference, optional?)
    work = [(None, modules, dep, direct[dep], dep not in importer["dependencies"]) for dep in sorted(direct)]
    while work:
        parent, holder, dep, ref, optional = work.pop()
        key = snapshot_key(dep, ref)
        where = f"{parent or IMPORTER} -> {dep}"
        if key is None:
            problems.append(f"{where}: unsupported lockfile reference {ref}")
            continue
        if key not in snapshots:
            problems.append(f"{where}: {key} is not in the lockfile snapshots")
            continue
        name, version = split_key(key)
        link = os.path.join(holder, *dep.split("/"))
        if not os.path.lexists(link):
            if optional and f"{name}@{version}" in restricted:
                skipped.add(key)
            else:
                problems.append(f"{where}: {key} is missing")
            continue
        real = os.path.realpath(link)
        if not inside(real, store) or not os.path.isfile(os.path.join(real, "package.json")):
            problems.append(f"{where}: {link} does not lead to a package in {store}")
            continue
        have = package_json(real)
        if have != (name, version):
            problems.append(f"{where}: installed {have[0]}@{have[1]}, the lockfile has {name}@{version}")
            continue
        entry_dir = os.path.join(store, os.path.relpath(real, store).split(os.sep, 1)[0])
        if owner.setdefault(entry_dir, key) != key:
            problems.append(f"{where}: {os.path.basename(entry_dir)} holds both {owner[entry_dir]} and {key}")
            continue
        if key in seen:
            continue
        seen.add(key)
        nm = modules_dir_of(real, name)
        for sub in DEP_SECTIONS:
            for child, child_ref in sorted(snapshots[key][sub].items()):
                work.append((key, nm, child, child_ref, sub == "optionalDependencies"))

    for entry in sorted(os.listdir(store)):
        entry_dir = os.path.join(store, entry)
        if entry == "node_modules" or not os.path.isdir(entry_dir) or os.path.islink(entry_dir):
            continue
        if entry_dir not in owner:
            problems.append(f"extra package directory not in the locked closure: .pnpm/{entry}")

    allowed_top = set(direct) | {".bin", ".pnpm", ".modules.yaml"}
    for entry in sorted(os.listdir(modules)):
        names = [entry]
        if entry.startswith("@") and os.path.isdir(os.path.join(modules, entry)):
            names = [f"{entry}/{s}" for s in sorted(os.listdir(os.path.join(modules, entry)))]
        for top in names:
            if top not in allowed_top:
                problems.append(f"extra top-level entry in node_modules: {top}")

    links = 0
    for root, dirs, files in os.walk(modules):
        for item in dirs + files:
            path = os.path.join(root, item)
            if not os.path.islink(path):
                continue
            links += 1
            rel = os.path.relpath(path, modules)
            if not os.path.exists(path):
                problems.append(f"dangling link: {rel} -> {os.readlink(path)}")
            elif not inside(os.path.realpath(path), modules):
                problems.append(f"link leaves node_modules: {rel} -> {os.readlink(path)}")

    if not os.path.exists(os.path.join(modules, ".bin", "prisma")):
        problems.append("node_modules/.bin/prisma is missing")
    if not seen:
        fail(f"no packages found under {store}")
    if problems:
        for p in problems:
            print(f"check-install: {p}")
        fail(f"{len(problems)} problem(s) in {package}")
    note = f", {len(skipped)} optional for other platforms not installed" if skipped else ""
    print(f"check-install: OK: {len(seen)} locked snapshots in {package} match {lock_path}{note}; "
          f"{links} links, all inside node_modules")


if __name__ == "__main__":
    main()
