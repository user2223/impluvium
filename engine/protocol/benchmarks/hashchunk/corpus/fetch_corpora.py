#!/usr/bin/env python3
# SPDX-License-Identifier: 0BSD
"""Download pinned public benchmark inputs and prepare bounded trees (standard library only).

Data is not redistributed. HTTPS and every redirect are checked; local file URLs are
available only for tests. File checksums and prepared-tree pins bind the first load.
Extraction uses fresh sibling directories and publishes only a fully verified tree.
Checksum verification does not perform PGP or Minisign verification.
"""
import argparse, hashlib, json, os, re, shutil, stat, sys, tarfile, urllib.error, urllib.parse, urllib.request, zipfile

CHUNK = 1 << 20
DEFAULT_MAX_MEMBERS = 1_000_000
DEFAULT_MAX_FILE = 8 << 30
DEFAULT_MAX_TOTAL = 64 << 30
DEFAULT_MAX_DOWNLOAD = 8 << 30


DIGEST_LENGTHS = {"md5": 32, "sha256": 64, "sha512": 128}


def hex_digest(value, algorithm=None):
    """Decode only the version-2, lowercase, fixed-width digest representation."""
    if not isinstance(value, dict):
        raise ValueError("digest must be an object")
    algo = value.get("algorithm")
    if algo not in DIGEST_LENGTHS or (algorithm is not None and algorithm != algo):
        raise ValueError("incorrect digest algorithm")
    parts = value.get("hex_parts")
    if not isinstance(parts, list) or len(parts) != DIGEST_LENGTHS[algo] // 32:
        raise ValueError("incorrect digest parts count")
    if any(not isinstance(p, str) or not re.fullmatch(r"[0-9a-f]{32}", p) for p in parts):
        raise ValueError("digest parts must be 32 lowercase hex characters")
    return "".join(parts)


def digest_record(value, algorithm="sha256"):
    result = {"algorithm": algorithm, "hex_parts": [value[n:n+32] for n in range(0, len(value), 32)]}
    hex_digest(result, algorithm)
    return result


def read_json(path):
    with open(path, encoding="utf-8") as stream:
        return json.load(stream)


def validate_manifest(m):
    if m.get("manifest_version") != 2:
        raise ValueError("unsupported manifest version")
    for inp in m["inputs"]:
        for f in inp.get("files", []):
            hex_digest(f["checksum"])
            if os.path.basename(f["name"]) != f["name"] or f["name"] in ("", ".", ".."):
                raise ValueError("download name must be a basename")
            if f.get("size_bytes") is not None and (type(f["size_bytes"]) is not int or f["size_bytes"] <= 0):
                raise ValueError("download size must be a positive integer")
        for st in inp.get("prepare", []):
            if "checksum" in st:
                hex_digest(st["checksum"])
            safe_join("/target", st["dest"])
            if "src" in st and st["src"] not in {f["name"] for f in inp["files"]}:
                raise ValueError("prepare source must name an input file")
    def walk(v):
        if isinstance(v, dict):
            if v.get("algorithm") in DIGEST_LENGTHS and "status" not in v:
                hex_digest(v)
            for item in v.values():
                walk(item)
        elif isinstance(v, list):
            for item in v:
                walk(item)
    walk(m)


class Budget:
    def __init__(self, members, file_bytes, total_bytes):
        self.members, self.file_bytes, self.total_bytes = members, file_bytes, total_bytes
        self.n = 0
        self.total = 0

    def member(self, size=None):
        self.n += 1
        if self.n > self.members:
            raise SystemExit(f"extraction budget exceeded: more than {self.members} members")
        if size is not None:
            if size > self.file_bytes:
                raise SystemExit(f"extraction budget exceeded: member of {size} bytes > {self.file_bytes}")
            if self.total + size > self.total_bytes:
                raise SystemExit(f"extraction budget exceeded: total unpacked size > {self.total_bytes}")

    def write(self, fileno_written, member_written):
        self.total += fileno_written
        if member_written > self.file_bytes:
            raise SystemExit(f"extraction budget exceeded: member grew beyond {self.file_bytes} bytes")
        if self.total > self.total_bytes:
            raise SystemExit(f"extraction budget exceeded: total unpacked size > {self.total_bytes}")


def digest(path, algo):
    h = hashlib.new(algo)
    with open(path, "rb") as f:
        while True:
            b = f.read(CHUNK)
            if not b:
                break
            h.update(b)
    return h.hexdigest()


def tree_digest(root):
    """sha256 over sorted lines 'F NUL path NUL size NUL octal-mode NUL sha256(file)' and symlink lines
    'L NUL path NUL target', identical to `chunkbench digest` (files first, symlinks after, both by raw path bytes)."""
    files, links = [], []

    def walk(d, prefix):
        with os.scandir(d) as it:
            for e in it:
                rel = prefix + e.name
                if e.is_symlink():
                    links.append((rel, os.readlink(e.path)))
                elif e.is_dir(follow_symlinks=False):
                    walk(e.path, rel + "/")
                elif e.is_file(follow_symlinks=False):
                    files.append((rel, e.path))

    if os.path.isfile(root) and not os.path.islink(root):
        files.append(("file", root))
    else:
        walk(root, "")
    files.sort(key=lambda x: x[0].encode())
    links.sort(key=lambda x: x[0].encode())
    h = hashlib.sha256()
    for rel, p in files:
        st = os.lstat(p)
        h.update(f"F\0{rel}\0{st.st_size}\0{(0o644 if os.path.isfile(root) else stat.S_IMODE(st.st_mode)):o}\0{digest(p, 'sha256')}\n".encode())
    for rel, tgt in links:
        h.update(f"L\0{rel}\0{tgt}\n".encode())
    return h.hexdigest()


class HttpsOnlyRedirects(urllib.request.HTTPRedirectHandler):
    """Follows redirects only to https, at most 5 times, and never reads a redirect response body (the inherited
    handler reads it without a limit before the caller's bounded read loop is reached)."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if not newurl.startswith("https://"):
            raise SystemExit(f"refusing redirect to non-https url: {newurl}")
        return super().redirect_request(req, fp, code, msg, headers, newurl)

    def http_error_302(self, req, fp, code, msg, headers):
        newurl = headers.get("location") or headers.get("uri")
        if not newurl:
            fp.close()
            raise SystemExit("redirect without a target")
        newurl = urllib.parse.urljoin(req.full_url, newurl)
        depth = getattr(req, "_redirect_depth", 0) + 1
        if depth > 5:
            fp.close()
            raise SystemExit("too many redirects")
        try:
            new = self.redirect_request(req, fp, code, msg, headers, newurl)
        finally:
            fp.close()  # body discarded unread, including rejected redirects
        if new is None:
            return None
        new._redirect_depth = depth
        return self.parent.open(new, timeout=getattr(req, "timeout", 60))

    http_error_301 = http_error_303 = http_error_307 = http_error_308 = http_error_302


def download(url, dst, allow_file, limit):
    scheme = url.split(":", 1)[0]
    if scheme != "https" and not (allow_file and scheme == "file"):
        raise SystemExit(f"refusing url scheme {scheme!r}: {url}")
    part = dst + ".part"
    opener = urllib.request.build_opener(HttpsOnlyRedirects)
    req = urllib.request.Request(url, headers={"User-Agent": "chunkbench-corpus-loader/1"})
    got = 0
    try:
        if os.path.islink(dst) or os.path.islink(part):
            raise SystemExit("unsafe download target symlink")
        with opener.open(req, timeout=60) as r, open(part, "xb") as out:
            while True:
                b = r.read(CHUNK)
                if not b:
                    break
                got += len(b)
                if got > limit:
                    raise SystemExit(f"download exceeds the limit of {limit} bytes: {url}")
                out.write(b)
    except BaseException:
        if os.path.exists(part):
            os.remove(part)
        raise
    os.replace(part, dst)


def safe_join(base, name):
    target = os.path.normpath(os.path.join(base, name))
    if not name or os.path.isabs(name) or ".." in name.split("/") or not target.startswith(base + os.sep):
        raise SystemExit(f"unsafe archive member: {name!r}")
    return target


def make_dirs(base, rel_dir):
    """Creates rel_dir below base one component at a time and refuses symlinks or non-directories on the way."""
    cur = base
    for comp in [c for c in rel_dir.split(os.sep) if c]:
        cur = os.path.join(cur, comp)
        try:
            st = os.lstat(cur)
        except FileNotFoundError:
            os.mkdir(cur)
            continue
        if not stat.S_ISDIR(st.st_mode):
            raise SystemExit(f"unsafe target: {cur!r} is not a plain directory (symlink or file)")


def secure_dir(root, path):
    """Creates `path` (a folder below the data root `root`) component by component, refusing symlinks and non-directories
    on the way, starting at the root itself (which is resolved once). Returns the path."""
    root = os.path.realpath(root)
    rel = os.path.relpath(os.path.abspath(path), root)
    if rel.startswith(".."):
        raise SystemExit(f"path outside the data root: {path!r}")
    make_dirs(root, "" if rel == "." else rel)
    return os.path.join(root, rel) if rel != "." else root


def write_member(base, rel, src, mode, budget, seen):
    target = safe_join(base, rel)
    if target in seen:
        raise SystemExit(f"duplicate archive member: {rel!r}")
    seen.add(target)
    make_dirs(base, os.path.relpath(os.path.dirname(target), base) if os.path.dirname(target) != base else "")
    fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    written = 0
    with os.fdopen(fd, "wb") as out:
        while True:
            b = src.read(CHUNK)
            if not b:
                break
            written += len(b)
            budget.write(len(b), written)
            out.write(b)
    os.chmod(target, mode if mode else 0o644)


def extract_zip(src, dest, member, budget):
    base = os.path.abspath(dest)
    os.mkdir(base)
    seen, n = set(), 0
    with zipfile.ZipFile(src) as z:
        for info in z.infolist():
            if member is not None and info.filename != member:
                continue
            budget.member(info.file_size)
            if info.is_dir():
                target = safe_join(base, info.filename)
                if target in seen:
                    raise SystemExit(f"duplicate archive member: {info.filename!r}")
                seen.add(target)
                make_dirs(base, safe_join(base, info.filename)[len(base):].lstrip(os.sep))
                continue
            rel = info.filename if member is None else os.path.basename(info.filename)
            with z.open(info) as i:
                write_member(base, rel, i, (info.external_attr >> 16) & 0o777, budget, seen)
            n += 1
    if member is not None and n != 1:
        raise SystemExit(f"member {member!r} not found exactly once")
    return n


def validate_tar_links(links):
    """Check relative targets lexically against the complete archive link set."""
    link_paths = {tuple(rel.split("/")) for rel, _ in links}
    for rel, target in links:
        if not target or target.startswith("/") or "\\" in target or ":" in target.split("/")[0]:
            raise SystemExit(f"unsafe link in archive: {rel!r} -> {target!r}")
        path = rel.split("/")[:-1]
        if any(tuple(path[:depth]) in link_paths for depth in range(1, len(path) + 1)):
            raise SystemExit(f"unsafe link in archive: {rel!r} -> {target!r}")
        for component in target.split("/"):
            if component in ("", "."):
                continue
            if component == "..":
                if not path:
                    raise SystemExit(f"unsafe link in archive: {rel!r} -> {target!r}")
                path.pop()
            else:
                path.append(component)
            if tuple(path) in link_paths:
                raise SystemExit(f"unsafe link in archive: {rel!r} -> {target!r}")


def extract_tar(src, dest, strip, budget):
    base = os.path.abspath(dest)
    os.mkdir(base)
    n, seen, links = 0, set(), []
    with tarfile.open(src, "r:*") as t:
        for m in t:
            # Check the original name before stripping a release-directory prefix.
            safe_join(base, m.name)
            parts = [p for p in m.name.split("/") if p not in ("", ".")]
            if len(parts) <= strip:
                budget.member()
                continue
            rel = "/".join(parts[strip:])
            target = safe_join(base, rel)
            budget.member(m.size if m.isfile() else 0)
            if target in seen:
                raise SystemExit(f"duplicate archive member: {rel!r}")
            if m.islnk():
                seen.add(target)
                continue  # historical preparation does not use hard links
            if m.isfile():
                # Same regular-file mode policy as tarfile's data filter.
                mode = m.mode & 0o755
                if not mode & 0o100:
                    mode &= ~0o111
                with t.extractfile(m) as stream:
                    write_member(base, rel, stream, mode | 0o600, budget, seen)
            elif m.isdir():
                seen.add(target)
                make_dirs(base, rel)
            elif m.issym():
                seen.add(target)
                links.append((rel, m.linkname))
            else:
                raise SystemExit(f"unsupported archive member: {rel!r}")
            n += 1
    # Validate all targets before creating any links, including forward references.
    validate_tar_links(links)
    for rel, target in links:
        make_dirs(base, os.path.dirname(rel))
        # Match the historical tar data filter's link text only after validating
        # the original path, so normalization cannot hide an unsafe step.
        os.symlink(os.path.normpath(target), safe_join(base, rel))
    return n


def verify(path, chk, recorded, key, pinned=None):
    algo = chk["algorithm"]
    value = hex_digest(chk)
    sha256 = digest(path, "sha256")
    if algo in ("md5", "sha256", "sha512"):
        got = sha256 if algo == "sha256" else digest(path, algo)
        if got != value:
            os.remove(path)
            print(f"CHECKSUM MISMATCH {key}: expected {algo} {value}, got {got}", file=sys.stderr)
            raise SystemExit(2)
    pin_record = (pinned or {}).get(key, {}).get("sha256")
    pin = hex_digest(pin_record, "sha256") if pin_record is not None else None
    if pin is not None and pin != sha256:
        os.remove(path)
        print(f"DIFFERS FROM MANIFEST {key}: the manifest pins sha256 {pin}, got {sha256}", file=sys.stderr)
        raise SystemExit(2)
    prev = recorded.get(key)
    if prev is not None and hex_digest(prev["sha256"], "sha256") != sha256:
        os.remove(path)
        print(f"CHANGED UPSTREAM {key}: recorded sha256 {prev['sha256']}, got {sha256}", file=sys.stderr)
        raise SystemExit(2)
    recorded[key] = {"sha256": digest_record(sha256), "size": os.path.getsize(path),
                     "official": None if chk.get("source_kind") == "historical_pin" else digest_record(value, algo),
                     "checksum_verification": {"status": "verified", "algorithm": algo, "source": chk.get("source"),
                                               "expected": chk},
                     "signature_verification": {"status": "not_verified", "method": "not_performed"}}
    return sha256


def prepare_tree(kind, st, dl, dest, budget_args, recorded, pinned=None):
    """Runs one extract step into a fresh sibling folder and moves it into place; verifies/records the tree digest."""
    final = os.path.join(dest, st["dest"])
    secure_dir(dest, os.path.dirname(final))
    key = f"tree:{st['dest']}"
    expected = (pinned or {}).get(key, {}).get("tree_digest")  # the manifest's pinned value is binding
    if expected is None and key in recorded:
        expected = recorded[key]["tree_digest"]
    if key in recorded and (pinned or {}).get(key, {}).get("tree_digest") not in (None, recorded[key]["tree_digest"]):
        print(f"LOCAL RECORD CONTRADICTS THE MANIFEST for {key}", file=sys.stderr)
        raise SystemExit(2)
    expected = hex_digest(expected, "sha256") if expected is not None else None
    if os.path.lexists(final) and not os.path.islink(final) and expected is not None:
        if tree_digest(final) == expected:
            return "kept"
        print(f"  prepared tree {st['dest']} differs from the recorded digest: preparing again", file=sys.stderr)
    tmp = final + f".tmp-{os.getpid()}"
    shutil.rmtree(tmp, ignore_errors=True)
    budget = Budget(*budget_args)
    try:
        if kind == "extract_zip":
            extract_zip(os.path.join(dl, st["src"]), tmp, None, budget)
        else:
            extract_tar(os.path.join(dl, st["src"]), tmp, st.get("strip", 0), budget)
        d = tree_digest(tmp)
        if expected is not None and expected != d:
            print(f"PREPARED TREE DIFFERS FROM THE {'MANIFEST' if (pinned or {}).get(key, {}).get('tree_digest') else 'RECORD'} (NOT REPRODUCIBLE) {st['dest']}: expected {expected}, got {d}", file=sys.stderr)
            raise SystemExit(2)
    except BaseException:
        shutil.rmtree(tmp, ignore_errors=True)
        raise
    if os.path.islink(final) or os.path.isfile(final):
        os.remove(final)
    else:
        shutil.rmtree(final, ignore_errors=True)
    os.rename(tmp, final)
    recorded[key] = {"tree_digest": digest_record(d), "members": budget.n, "unpacked_bytes": budget.total,
                     "tree_verification": {"status": "verified" if expected else "recorded", "expected": digest_record(expected) if expected else None},
                     "signature_verification": {"status": "not_verified", "method": "not_performed"}}
    return "prepared"


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", required=True)
    ap.add_argument("--dest", required=True)
    ap.add_argument("--only", action="append", default=[])
    ap.add_argument("--with-optional", action="store_true", help="also run prepare steps marked optional")
    ap.add_argument("--allow-file", action="store_true", help="allow file:// URLs (tests only)")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--max-members", type=int, default=DEFAULT_MAX_MEMBERS)
    ap.add_argument("--max-file-bytes", type=int, default=DEFAULT_MAX_FILE)
    ap.add_argument("--max-total-bytes", type=int, default=DEFAULT_MAX_TOTAL)
    ap.add_argument("--max-download-bytes", type=int, default=DEFAULT_MAX_DOWNLOAD)
    a = ap.parse_args(argv)
    m = read_json(a.manifest)
    validate_manifest(m)
    selected = [i for i in m["inputs"] if i.get("kind") == "download" and (not a.only or i["id"] in a.only)]
    if a.dry_run:
        for inp in selected:
            print(f"[{inp['id']}] {inp['license'].get('spdx') or inp['license'].get('name')}")
        return  # no directories, downloads or preparations
    dest = os.path.abspath(a.dest)
    os.makedirs(dest, exist_ok=True)
    dest = os.path.realpath(dest)
    dl = secure_dir(dest, os.path.join(dest, ".downloads"))
    rec_path = os.path.join(dest, "recorded-checksums.json")
    if os.path.islink(rec_path):
        raise SystemExit("unsafe local record symlink")
    recorded = read_json(rec_path) if os.path.exists(rec_path) else {}
    budget_args = (a.max_members, a.max_file_bytes, a.max_total_bytes)
    # Values pinned in the (checked-in) manifest are binding on every load, including the first one on a machine.
    pinned = (m.get("historical_verification") or {}).get("values", {})

    def save():
        with open(rec_path, "w", encoding="utf-8") as stream:
            json.dump(recorded, stream, indent=2, sort_keys=True)

    for inp in m["inputs"]:
        if inp.get("kind") != "download" or (a.only and inp["id"] not in a.only):
            continue
        print(f"[{inp['id']}] {inp['license'].get('spdx') or inp['license'].get('name')}", file=sys.stderr)
        for f in inp["files"]:
            path = os.path.join(dl, f["name"])
            key = f["name"]
            if os.path.islink(path) or os.path.islink(path + ".part"):
                raise SystemExit("unsafe download target symlink")
            if not os.path.exists(path) or (f.get("size_bytes") and os.path.getsize(path) != f["size_bytes"]):
                print(f"  download {f['url']}", file=sys.stderr)
                download(f["url"], path, a.allow_file, min(f.get("size_bytes") or a.max_download_bytes, a.max_download_bytes))
            if f.get("size_bytes") and os.path.getsize(path) != f["size_bytes"]:
                os.remove(path)
                raise SystemExit(f"size mismatch for {f['name']}")
            verify(path, f["checksum"], recorded, key, pinned)
            save()
        for st in inp.get("prepare", []):
            if st.get("optional") and not a.with_optional:
                continue
            kind = st["step"]
            if kind == "keep":
                src = os.path.join(dl, inp["files"][0]["name"])
                dst = os.path.join(dest, st["dest"])
                secure_dir(dest, os.path.dirname(dst))
                if os.path.islink(dst):
                    os.remove(dst)  # never follow a link at the target
                if os.path.lexists(dst) and digest(dst, "sha256") != hex_digest(recorded[inp["files"][0]["name"]]["sha256"], "sha256"):
                    os.remove(dst)  # stale or modified copy: replace from the verified download
                if not os.path.lexists(dst):
                    try:
                        os.link(src, dst)
                    except OSError:
                        shutil.copyfile(src, dst)
            elif kind in ("extract_zip", "extract_tar"):
                print(f"  {kind} -> {st['dest']}: {prepare_tree(kind, st, dl, dest, budget_args, recorded, pinned)}", file=sys.stderr)
            elif kind == "extract_zip_member":
                out_dir = os.path.dirname(os.path.join(dest, st["dest"]))
                out = os.path.join(dest, st["dest"])
                secure_dir(dest, out_dir)
                if os.path.islink(out):
                    os.remove(out)
                if not os.path.exists(out):
                    tmp = out_dir + f".tmp-{os.getpid()}"
                    shutil.rmtree(tmp, ignore_errors=True)
                    try:
                        secure_dir(dest, os.path.dirname(out_dir))
                        extract_zip(os.path.join(dl, st["src"]), tmp, st["member"], Budget(*budget_args))
                        secure_dir(dest, out_dir)
                        os.replace(os.path.join(tmp, os.path.basename(st["member"])), out)
                    finally:
                        shutil.rmtree(tmp, ignore_errors=True)
                if "checksum" in st:
                    verify(out, st["checksum"], recorded, st["dest"], pinned)
            else:
                raise SystemExit(f"unknown prepare step {kind!r}")
            save()
    print("done", file=sys.stderr)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, tarfile.TarError, zipfile.BadZipFile) as error:
        raise SystemExit(str(error))
