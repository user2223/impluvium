# SPDX-License-Identifier: 0BSD
from pathlib import Path
import hashlib, importlib.util, io, json, os, contextlib, types, shutil, sys, tarfile, tempfile, unittest, zipfile

HERE = os.path.dirname(os.path.abspath(__file__))
LOADER = os.path.join(HERE, "fetch_corpora.py")
_spec = importlib.util.spec_from_file_location("fetch_corpora", LOADER)
fc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(fc)


def sha(b, a="sha256"):
    return hashlib.new(a, b).hexdigest()


class Loader(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.d)
        self.src = os.path.join(self.d, "src")
        os.makedirs(self.src)
        self.dest = os.path.join(self.d, "dest")
        # fixtures: a zip, a tar.xz, a plain file
        self.zip = os.path.join(self.src, "a.zip")
        with zipfile.ZipFile(self.zip, "w") as z:
            z.writestr("Top/file.bin", b"zip-content")
            z.writestr("Top/sub/x.txt", b"x")
        self.tar = os.path.join(self.src, "b.tar.xz")
        with tarfile.open(self.tar, "w:xz") as t:
            for name, data in (("pkg-1/a.txt", b"aaa"), ("pkg-1/d/b.txt", b"bbb")):
                ti = tarfile.TarInfo(name)
                ti.size = len(data)
                t.addfile(ti, io.BytesIO(data))
        self.plain = os.path.join(self.src, "c.bin")
        Path(self.plain).write_bytes(b"plain" * 1000)

    def manifest(self, **over):
        zb, tb, pb = (Path(p).read_bytes() for p in (self.zip, self.tar, self.plain))
        m = {"inputs": [
            {"id": "z", "kind": "download", "license": {"name": "t"}, "files": [{"name": "a.zip", "url": "file://" + self.zip, "size_bytes": len(zb), "checksum": {"algorithm": "sha512", "value": sha(zb, "sha512")}}],
             "prepare": [{"step": "extract_zip", "src": "a.zip", "dest": "out/z"}]},
            {"id": "t", "kind": "download", "license": {"name": "t"}, "files": [{"name": "b.tar.xz", "url": "file://" + self.tar, "size_bytes": len(tb), "checksum": {"algorithm": "sha256", "value": sha(tb)}}],
             "prepare": [{"step": "extract_tar", "src": "b.tar.xz", "dest": "out/t", "strip": 1}]},
            {"id": "p", "kind": "download", "license": {"name": "t"}, "files": [{"name": "c.bin", "url": "file://" + self.plain, "size_bytes": None, "checksum": {"algorithm": "sha256", "value": sha(pb)}}],
             "prepare": [{"step": "keep", "dest": "out/c.bin"}]},
        ]}
        if "bad" in over:
            m["inputs"][over["bad"]]["files"][0]["checksum"]["value"] = "0" * 64
        path = os.path.join(self.d, "m.json")
        Path(path).write_text(json.dumps(m))
        return path

    def run_loader(self, manifest, *extra):
        m = fc.read_json(manifest)
        m["manifest_version"] = 2
        def convert(v):
            if isinstance(v, dict):
                for k, item in list(v.items()):
                    if k in ("sha256", "tree_digest") and isinstance(item, str):
                        v[k] = fc.digest_record(item)
                    elif k == "value":
                        del v[k]
                        if item is not None:
                            v["hex_parts"] = fc.digest_record(item, v["algorithm"])["hex_parts"]
                    else:
                        convert(item)
            elif isinstance(v, list):
                for item in v:
                    convert(item)
        convert(m)
        with open(manifest, "w") as stream:
            json.dump(m, stream)
        stdout, stderr = io.StringIO(), io.StringIO()
        status = 0
        with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            try:
                fc.main(["--manifest", manifest, "--dest", self.dest, "--allow-file", *extra])
            except SystemExit as error:
                status = error.code if isinstance(error.code, int) else 1
                print(error, file=sys.stderr)
            except (OSError, ValueError) as error:
                status = 1
                print(error, file=sys.stderr)
        return types.SimpleNamespace(returncode=status, stdout=stdout.getvalue(), stderr=stderr.getvalue())

    def test_download_verify_extract(self):
        r = self.run_loader(self.manifest())
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(Path(os.path.join(self.dest, "out/z/Top/file.bin")).read_bytes(), b"zip-content")
        self.assertEqual(Path(os.path.join(self.dest, "out/t/a.txt")).read_bytes(), b"aaa")
        self.assertTrue(os.path.exists(os.path.join(self.dest, "out/c.bin")))
        rec = fc.read_json(os.path.join(self.dest, "recorded-checksums.json"))
        self.assertEqual(fc.hex_digest(rec["c.bin"]["sha256"]), sha(Path(self.plain).read_bytes()))
        # a second run is idempotent
        self.assertEqual(self.run_loader(self.manifest()).returncode, 0)

    def test_checksum_mismatch_aborts_and_removes(self):
        r = self.run_loader(self.manifest(bad=1))
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertIn("CHECKSUM MISMATCH", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.dest, ".downloads/b.tar.xz")))

    def test_upstream_change_detected_by_recorded_sha256(self):
        p = self.manifest()
        self.assertEqual(self.run_loader(p).returncode, 0)
        Path(self.plain).write_bytes(b"changed upstream")
        r = self.run_loader(p, "--only", "p")
        self.assertEqual(r.returncode, 0)  # file already downloaded and unchanged locally: no re-download
        os.remove(os.path.join(self.dest, ".downloads/c.bin"))
        r = self.run_loader(self.manifest(), "--only", "p")
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertIn("CHANGED UPSTREAM", r.stderr)

    def test_path_traversal_rejected(self):
        evil = os.path.join(self.src, "evil.zip")
        with zipfile.ZipFile(evil, "w") as z:
            z.writestr("../escape.txt", b"x")
        m = {"inputs": [{"id": "e", "kind": "download", "license": {"name": "t"}, "files": [{"name": "evil.zip", "url": "file://" + evil, "size_bytes": None, "checksum": {"algorithm": "sha256", "value": sha(Path(evil).read_bytes())}}], "prepare": [{"step": "extract_zip", "src": "evil.zip", "dest": "out/e"}]}]}
        p = os.path.join(self.d, "e.json")
        Path(p).write_text(json.dumps(m))
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 1)
        self.assertIn("unsafe archive member", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.dest, "out/escape.txt")))
        evil_t = os.path.join(self.src, "evil.tar")
        with tarfile.open(evil_t, "w") as t:
            ti = tarfile.TarInfo("../../escape2.txt")
            ti.size = 1
            t.addfile(ti, io.BytesIO(b"x"))
        m["inputs"][0]["files"][0].update(name="evil.tar", url="file://" + evil_t, checksum={"algorithm": "sha256", "value": sha(Path(evil_t).read_bytes())})
        m["inputs"][0]["prepare"] = [{"step": "extract_tar", "src": "evil.tar", "dest": "out/e2", "strip": 0}]
        Path(p).write_text(json.dumps(m))
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 1)
        self.assertIn("unsafe archive member", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.dest, "escape2.txt")))

    def test_refuses_http(self):
        m = {"inputs": [{"id": "h", "kind": "download", "license": {"name": "t"}, "files": [{"name": "x", "url": "http://example.invalid/x", "size_bytes": None, "checksum": {"algorithm": "sha256", "value": "0" * 64}}], "prepare": []}]}
        p = os.path.join(self.d, "h.json")
        Path(p).write_text(json.dumps(m))
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 1)
        self.assertIn("refusing url scheme", r.stderr)


class Hardening(Loader):
    def one(self, name, path, size=None, steps=None, algo="sha256"):
        m = {"inputs": [{"id": "x", "kind": "download", "license": {"name": "t"}, "files": [{"name": name, "url": "file://" + path, "size_bytes": size, "checksum": {"algorithm": algo, "value": sha(Path(path).read_bytes(), algo)}}], "prepare": steps or []}]}
        p = os.path.join(self.d, "x.json")
        with open(p, "w") as f:
            json.dump(m, f)
        return p

    def test_redirect_to_http_is_refused(self):
        h = fc.HttpsOnlyRedirects()
        req = fc.urllib.request.Request("https://example.invalid/a")
        with self.assertRaises(SystemExit) as cm:
            h.redirect_request(req, None, 302, "Found", {}, "http://example.invalid/b")
        self.assertIn("non-https", str(cm.exception))
        with self.assertRaises(SystemExit):
            h.redirect_request(req, None, 302, "Found", {}, "ftp://example.invalid/b")
        self.assertIsNotNone(h.redirect_request(req, None, 302, "Found", {}, "https://example.invalid/b"))

    def test_download_stops_at_declared_size(self):
        big = os.path.join(self.src, "big.bin")
        with open(big, "wb") as f:
            f.write(b"x" * 100_000)
        r = self.run_loader(self.one("big.bin", big, size=1000))
        self.assertEqual(r.returncode, 1)
        self.assertIn("download exceeds the limit of 1000 bytes", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.dest, ".downloads/big.bin.part")))
        self.assertFalse(os.path.exists(os.path.join(self.dest, ".downloads/big.bin")))
        r = self.run_loader(self.one("big.bin", big), "--max-download-bytes", "5000")
        self.assertIn("download exceeds the limit of 5000 bytes", r.stderr)

    def zip_of(self, name, members):
        p = os.path.join(self.src, name)
        with zipfile.ZipFile(p, "w", zipfile.ZIP_DEFLATED) as z:
            for n, data in members:
                z.writestr(n, data)
        return p

    def test_extraction_budgets_and_cleanup(self):
        steps = [{"step": "extract_zip", "src": "b.zip", "dest": "out/b"}]
        bomb = self.zip_of("b.zip", [("big.bin", b"\0" * 5_000_000)])  # compresses to a few KB
        r = self.run_loader(self.one("b.zip", bomb, steps=steps), "--max-file-bytes", "1000000")
        self.assertEqual(r.returncode, 1)
        self.assertIn("member of 5000000 bytes > 1000000", r.stderr)
        r = self.run_loader(self.one("b.zip", bomb, steps=steps), "--max-total-bytes", "1000000")
        self.assertIn("total unpacked size", r.stderr)
        many = self.zip_of("many.zip", [(f"d/f{i}", b"x") for i in range(50)])
        r = self.run_loader(self.one("many.zip", many, steps=[{"step": "extract_zip", "src": "many.zip", "dest": "out/b"}]), "--max-members", "10")
        self.assertIn("more than 10 members", r.stderr)
        # nothing partial is left behind
        out = os.path.join(self.dest, "out")
        self.assertFalse(os.path.exists(os.path.join(out, "b")))
        self.assertEqual([n for n in os.listdir(out) if ".tmp-" in n] if os.path.exists(out) else [], [])

    def test_declared_size_in_header_is_not_trusted(self):
        # a member whose real data is larger than the budget even if the header said less is cut while writing
        b = fc.Budget(10, 1000, 10_000)
        src = io.BytesIO(b"y" * 5000)
        d = tempfile.mkdtemp(dir=self.d)
        with self.assertRaises(SystemExit) as cm:
            fc.write_member(d, "f", src, 0o644, b, set())
        self.assertIn("grew beyond", str(cm.exception))

    def test_duplicate_member_and_symlink_targets_refused(self):
        d = tempfile.mkdtemp(dir=self.d)
        budget = fc.Budget(10, 1000, 10_000)
        seen = set()
        fc.write_member(d, "a/f", io.BytesIO(b"1"), 0o644, budget, seen)
        with self.assertRaises(SystemExit) as cm:
            fc.write_member(d, "a/f", io.BytesIO(b"2"), 0o644, budget, seen)
        self.assertIn("duplicate archive member", str(cm.exception))
        outside = tempfile.mkdtemp(dir=self.d)
        os.symlink(outside, os.path.join(d, "linkdir"))
        with self.assertRaises(SystemExit) as cm:
            fc.write_member(d, "linkdir/pwned", io.BytesIO(b"x"), 0o644, budget, set())
        self.assertIn("not a plain directory", str(cm.exception))
        self.assertEqual(os.listdir(outside), [], "nothing written through the directory symlink")
        victim = os.path.join(outside, "victim")
        os.symlink(victim, os.path.join(d, "filelink"))
        with self.assertRaises(OSError):
            fc.write_member(d, "filelink", io.BytesIO(b"x"), 0o644, budget, set())
        self.assertFalse(os.path.exists(victim), "nothing written through the file symlink")

    def test_tar_link_outside_is_refused(self):
        p = os.path.join(self.src, "l.tar")
        with tarfile.open(p, "w") as t:
            ti = tarfile.TarInfo("pkg/link")
            ti.type = tarfile.SYMTYPE
            ti.linkname = "../../outside"
            t.addfile(ti)
        steps = [{"step": "extract_tar", "src": "l.tar", "dest": "out/l", "strip": 1}]
        r = self.run_loader(self.one("l.tar", p, steps=steps))
        self.assertEqual(r.returncode, 1)
        self.assertIn("unsafe link in archive", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.dest, "out/l")))

    def test_tree_digest_recorded_and_verified(self):
        steps = [{"step": "extract_zip", "src": "a.zip", "dest": "out/z"}]
        p = self.one("a.zip", self.zip, steps=steps)
        self.assertEqual(self.run_loader(p).returncode, 0)
        rec = fc.read_json(os.path.join(self.dest, "recorded-checksums.json"))
        d0 = fc.hex_digest(rec["tree:out/z"]["tree_digest"])
        self.assertEqual(d0, fc.tree_digest(os.path.join(self.dest, "out/z")))
        # stale extra file and a modified file are repaired by re-preparing from the verified archive
        Path(os.path.join(self.dest, "out/z/Top/stale.txt")).write_text("stale")
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("preparing again", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.dest, "out/z/Top/stale.txt")))
        self.assertEqual(fc.tree_digest(os.path.join(self.dest, "out/z")), d0)
        r = self.run_loader(p)
        self.assertIn("kept", r.stderr)
        # a recorded digest that cannot be reproduced is an error
        rec["tree:out/z"]["tree_digest"] = fc.digest_record("0" * 64)
        Path(os.path.join(self.dest, "recorded-checksums.json")).write_text(json.dumps(rec))
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 2)
        self.assertIn("NOT REPRODUCIBLE", r.stderr)


for _n in ("test_download_verify_extract", "test_checksum_mismatch_aborts_and_removes", "test_upstream_change_detected_by_recorded_sha256", "test_path_traversal_rejected", "test_refuses_http"):
    setattr(Hardening, _n, None)  # inherited base tests run once, in Loader


class FakeBody:
    """A redirect response whose body must never be read."""
    def __init__(self):
        self.closed = False

    def read(self, *a):
        raise AssertionError("redirect body was read")

    def close(self):
        self.closed = True


class FakeParent:
    def open(self, req, timeout=None):
        return ("followed", req.full_url)


def hdrs(location):
    import email.message
    m = email.message.Message()
    m["Location"] = location
    return m


class RedirectsAndRanges(unittest.TestCase):
    def check_handler(self, handler_cls):
        for follow in ("https://example.invalid/next",):
            h = handler_cls()
            h.parent = FakeParent()
            body = FakeBody()
            req = fc.urllib.request.Request("https://example.invalid/a")
            res = h.http_error_302(req, body, 302, "Found", hdrs(follow))
            self.assertEqual(res, ("followed", follow))
            self.assertTrue(body.closed, "the redirect response is closed without reading its (possibly huge) body")
        for bad in ("http://example.invalid/x", "ftp://example.invalid/x"):
            h = handler_cls()
            h.parent = FakeParent()
            body = FakeBody()
            with self.assertRaises(SystemExit) as cm:
                h.http_error_302(fc.urllib.request.Request("https://example.invalid/a"), body, 302, "Found", hdrs(bad))
            self.assertIn("non-https", str(cm.exception))
            self.assertTrue(body.closed)
        # at most 5 redirects
        h = handler_cls()
        h.parent = FakeParent()
        req = fc.urllib.request.Request("https://example.invalid/a")
        req._redirect_depth = 4
        self.assertEqual(h.http_error_302(req, FakeBody(), 302, "Found", hdrs("/n")), ("followed", "https://example.invalid/n"))
        req._redirect_depth = 5
        with self.assertRaises(SystemExit) as cm:
            h.http_error_302(req, FakeBody(), 302, "Found", hdrs("https://example.invalid/n"))
        self.assertIn("too many redirects", str(cm.exception))

    def test_loader_redirect_body_not_read(self):
        self.check_handler(fc.HttpsOnlyRedirects)



class LinksAbove(Hardening):
    def test_dry_run_has_no_network_or_preparation(self):
        from unittest.mock import patch
        p = self.one("c.bin", self.plain, steps=[{"step": "keep", "dest": "out/c.bin"}])
        with patch.object(fc, "download", side_effect=AssertionError("download")), patch.object(fc, "prepare_tree", side_effect=AssertionError("prepare")):
            self.assertEqual(self.run_loader(p, "--dry-run").returncode, 0)
        self.assertFalse(os.path.exists(self.dest))

    def test_symlink_above_the_extraction_target(self):
        outside = tempfile.mkdtemp(dir=self.d)
        os.makedirs(self.dest)
        os.symlink(outside, os.path.join(self.dest, "xonotic"))
        steps = [{"step": "extract_zip", "src": "a.zip", "dest": "xonotic/0.8.5"}]
        r = self.run_loader(self.one("a.zip", self.zip, steps=steps))
        self.assertEqual(r.returncode, 1)
        self.assertIn("not a plain directory", r.stderr)
        self.assertEqual(os.listdir(outside), [], "nothing was written through the link above the target")

    def test_symlink_above_keep_target_and_downloads(self):
        outside = tempfile.mkdtemp(dir=self.d)
        os.makedirs(self.dest)
        os.symlink(outside, os.path.join(self.dest, "archives"))
        r = self.run_loader(self.one("c.bin", self.plain, steps=[{"step": "keep", "dest": "archives/c.bin"}]))
        self.assertEqual(r.returncode, 1)
        self.assertIn("not a plain directory", r.stderr)
        self.assertEqual(os.listdir(outside), [])
        os.remove(os.path.join(self.dest, "archives"))
        import shutil
        shutil.rmtree(os.path.join(self.dest, ".downloads"), ignore_errors=True)
        os.symlink(outside, os.path.join(self.dest, ".downloads"))
        r = self.run_loader(self.one("c.bin", self.plain))
        self.assertEqual(r.returncode, 1)
        self.assertIn("not a plain directory", r.stderr)
        self.assertEqual(os.listdir(outside), [], "the download folder is not followed either")

    def test_link_at_the_target_itself_is_replaced_not_followed(self):
        outside = tempfile.mkdtemp(dir=self.d)
        os.makedirs(os.path.join(self.dest, "out"))
        os.symlink(outside, os.path.join(self.dest, "out", "z"))
        steps = [{"step": "extract_zip", "src": "a.zip", "dest": "out/z"}]
        r = self.run_loader(self.one("a.zip", self.zip, steps=steps))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(os.listdir(outside), [], "the link target was not written to")
        self.assertFalse(os.path.islink(os.path.join(self.dest, "out", "z")))
        self.assertTrue(os.path.exists(os.path.join(self.dest, "out/z/Top/file.bin")))


for _cls in (LinksAbove,):
    for _n in dir(Hardening):
        if _n.startswith("test_") and _n not in LinksAbove.__dict__:
            setattr(_cls, _n, None)


class PinnedValues(Hardening):
    def pinned(self, manifest_path, values):
        m = fc.read_json(manifest_path)
        m["historical_verification"] = {"values": values}
        with open(manifest_path, "w") as f:
            json.dump(m, f)

    def test_wrong_pinned_tree_digest_fails_on_the_very_first_load(self):
        steps = [{"step": "extract_zip", "src": "a.zip", "dest": "out/z"}]
        p = self.one("a.zip", self.zip, steps=steps)
        self.pinned(p, {"tree:out/z": {"tree_digest": "0" * 64}})
        self.assertFalse(os.path.exists(os.path.join(self.dest, "recorded-checksums.json")), "first load: no local record")
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 2)
        self.assertIn("DIFFERS FROM THE MANIFEST", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.dest, "out/z")), "the unverified tree is not put in place")
        # the right digest passes the first load, and a tampered tree is repaired against the pinned value
        good = fc.tree_digest(self.extracted_reference())
        self.pinned(p, {"tree:out/z": {"tree_digest": good}})
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 0, r.stderr)
        Path(os.path.join(self.dest, "out/z/Top/extra")).write_text("x")
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("preparing again", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.dest, "out/z/Top/extra")))

    def extracted_reference(self):
        ref = os.path.join(self.d, "ref")
        fc.extract_zip(self.zip, ref, None, fc.Budget(100, 10**6, 10**7))
        return ref

    def test_wrong_pinned_archive_checksum_fails_on_the_first_load(self):
        p = self.one("c.bin", self.plain)
        self.pinned(p, {"c.bin": {"sha256": "f" * 64}})
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 2)
        self.assertIn("DIFFERS FROM MANIFEST", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.dest, ".downloads/c.bin")), "the file is removed")

    def test_local_record_contradicting_the_manifest_is_refused(self):
        steps = [{"step": "extract_zip", "src": "a.zip", "dest": "out/z"}]
        p = self.one("a.zip", self.zip, steps=steps)
        self.assertEqual(self.run_loader(p).returncode, 0)
        rec = fc.read_json(os.path.join(self.dest, "recorded-checksums.json"))
        self.pinned(p, {"tree:out/z": {"tree_digest": "1" * 64}})
        r = self.run_loader(p)
        self.assertEqual(r.returncode, 2)
        self.assertIn("CONTRADICTS THE MANIFEST", r.stderr)


for _n in dir(Hardening):
    if _n.startswith("test_") and _n not in PinnedValues.__dict__:
        setattr(PinnedValues, _n, None)


class ManifestFile(unittest.TestCase):
    def test_manifest_is_complete(self):
        m = fc.read_json(os.path.join(HERE, "corpus-manifest.json"))
        ids = {i["id"] for i in m["inputs"]}
        self.assertNotIn("loader-selftest", ids)
        for i in m["inputs"]:
            self.assertIn("license", i)
            if i["kind"] == "download":
                for f in i["files"]:
                    self.assertTrue(f["url"].startswith("https://"))
                    self.assertIn(f["checksum"]["algorithm"], ("md5", "sha256", "sha512"))
                    self.assertTrue(fc.hex_digest(f["checksum"]) and f["checksum"]["source"])
        for p in m["pairs"]:
            self.assertIn(p["class"], ("many-small-files", "compressed-archive", "pak-files", "diagnostic-full-replacement", "diagnostic-no-boundary", "diagnostic-boundary-filtered", "diagnostic-real-small-files"))
        # Sintel (independent encodings, not versions) must not be part of the deciding class 2
        sintel = [p for p in m["pairs"] if "Sintel" in p["label"]]
        self.assertTrue(sintel and all(p["class"] == "diagnostic-full-replacement" for p in sintel))
        decisive = {p["class"] for p in m["pairs"] if not p["class"].startswith("diagnostic")}
        self.assertEqual(decisive, {"many-small-files", "compressed-archive", "pak-files"})
        for i in m["inputs"]:
            if i["kind"] == "download":
                for st in i.get("prepare", []):
                    self.assertNotIn("tree_digest", st)
        self.assertTrue({"generator-v1v2", "xonotic-0.8.5", "xonotic-0.8.6", "linux-7.3"} <= ids)




class AdditionalSafety(Hardening):
    def test_duplicate_zip_directory_rejected(self):
        import warnings
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            archive = self.zip_of("duplicate.zip", [("directory/", b""), ("directory/", b"")])
        with self.assertRaises(SystemExit):
            fc.prepare_tree("extract_zip", {"src": "duplicate.zip", "dest": "out/duplicate"}, self.src, self.d, (10, 100, 100), {})
        self.assertFalse(os.path.exists(os.path.join(self.d, "out/duplicate")))

    def test_checksum_and_signature_status_are_separate(self):
        records = {}
        checksum = {**fc.digest_record(sha(Path(self.plain).read_bytes())), "source": "fixture"}
        fc.verify(self.plain, checksum, records, "file")
        self.assertEqual(records["file"]["checksum_verification"]["status"], "verified")
        fc.verify(self.plain, checksum, records, "file")
        self.assertEqual(records["file"]["checksum_verification"]["status"], "verified")
        self.assertEqual(records["file"]["signature_verification"]["status"], "not_verified")
        checksum["source_kind"] = "historical_pin"
        fc.verify(self.plain, checksum, records, "file")
        self.assertIsNone(records["file"]["official"])
        self.assertEqual(records["file"]["checksum_verification"]["status"], "verified")

    def test_failed_preparation_preserves_existing_tree(self):
        final = os.path.join(self.dest, "out", "z")
        os.makedirs(final)
        from pathlib import Path
        (Path(final) / "valid").write_text("keep")
        p = self.one("a.zip", self.zip, steps=[{"step": "extract_zip", "src": "a.zip", "dest": "out/z"}])
        m = fc.read_json(p)
        m["historical_verification"] = {"values": {"tree:out/z": {"tree_digest": fc.digest_record("0" * 64)}}}
        with open(p, "w") as stream:
            json.dump(m, stream)
        self.assertEqual(self.run_loader(p).returncode, 2)
        self.assertEqual((Path(final) / "valid").read_text(), "keep")
        self.assertEqual(os.listdir(os.path.dirname(final)), ["z"])

    def test_tar_duplicate_and_special_members_rejected(self):
        for special in (False, True):
            p = os.path.join(self.src, "bad.tar")
            with tarfile.open(p, "w") as archive:
                for _ in range(2):
                    item = tarfile.TarInfo("pkg/file")
                    item.size = 1
                    if special:
                        item.type = tarfile.FIFOTYPE
                        item.size = 0
                    archive.addfile(item, io.BytesIO(b"x"))
            with self.assertRaises(SystemExit):
                fc.prepare_tree("extract_tar", {"src": "bad.tar", "dest": "out/bad", "strip": 1}, self.src, self.d, (10, 100, 100), {})
            self.assertFalse(os.path.exists(os.path.join(self.d, "out/bad")))

    def test_local_record_and_download_symlinks_rejected(self):
        os.makedirs(os.path.join(self.dest, ".downloads"))
        victim = os.path.join(self.d, "victim")
        from pathlib import Path
        Path(victim).write_text("unchanged")
        for name in ("c.bin", "c.bin.part"):
            link = os.path.join(self.dest, ".downloads", name)
            os.symlink(victim, link)
            self.assertNotEqual(self.run_loader(self.one("c.bin", self.plain)).returncode, 0)
            self.assertEqual(Path(victim).read_text(), "unchanged")
            if os.path.lexists(link):
                os.remove(link)
        os.symlink(victim, os.path.join(self.dest, "recorded-checksums.json"))
        self.assertNotEqual(self.run_loader(self.one("c.bin", self.plain)).returncode, 0)
        self.assertEqual(Path(victim).read_text(), "unchanged")


for _n in dir(Hardening):
    if _n.startswith("test_") and _n not in AdditionalSafety.__dict__:
        setattr(AdditionalSafety, _n, None)


class DigestFormat(unittest.TestCase):
    def test_round_trip_and_strict_rejection(self):
        for algo, length in fc.DIGEST_LENGTHS.items():
            value = "a" * length
            self.assertEqual(fc.hex_digest(fc.digest_record(value, algo), algo), value)
        for value in (None, "a" * 64, {"algorithm": "sha1", "hex_parts": ["a" * 32]},
                      {"algorithm": "sha256", "hex_parts": ["a" * 32]},
                      {"algorithm": "sha256", "hex_parts": ["a" * 31, "b" * 33]},
                      {"algorithm": "sha256", "hex_parts": ["A" * 32] * 2},
                      {"algorithm": "sha256", "hex_parts": ["/" * 32] * 2}):
            with self.subTest(value=value), self.assertRaises(ValueError):
                fc.hex_digest(value)
        with self.assertRaises(ValueError):
            fc.hex_digest(fc.digest_record("a" * 64), "sha512")


class ReviewRegressions(Hardening):
    def link_archive(self, name, members, reverse=False):
        archive = Path(self.src) / name
        with tarfile.open(archive, "w") as stream:
            for path, value in reversed(members) if reverse else members:
                member = tarfile.TarInfo(path)
                if value is None:
                    member.type = tarfile.DIRTYPE
                    stream.addfile(member)
                elif isinstance(value, bytes):
                    member.size = len(value)
                    stream.addfile(member, io.BytesIO(value))
                else:
                    member.type = tarfile.SYMTYPE
                    member.linkname = value
                    stream.addfile(member)
        return archive

    def reject_links_after_preparation(self, members, followup=False):
        for reverse in (False, True):
            with self.subTest(reverse=reverse):
                archive = self.link_archive("links.tar", members, reverse)
                data = Path(tempfile.mkdtemp(dir=self.d))
                final = data / "out/tree"
                final.mkdir(parents=True)
                (final / "valid").write_text("keep")
                error = None
                try:
                    fc.prepare_tree("extract_tar", {"src": archive.name, "dest": "out/tree", "strip": 1},
                                    self.src, str(data), (20, 100, 100), {})
                except SystemExit as caught:
                    error = caught
                if followup:
                    # A later ordinary preparation must not change a published link's meaning.
                    second = self.link_archive("second.tar", [("pkg/c", None)])
                    fc.prepare_tree("extract_tar", {"src": second.name, "dest": f"out/tree.tmp-{os.getpid()}", "strip": 1},
                                    self.src, str(data), (20, 100, 100), {})
                self.assertIsNotNone(error, "unsafe archive was published")
                self.assertIn("unsafe link", str(error))
                self.assertEqual((final / "valid").read_text(), "keep")
                self.assertEqual(list(final.iterdir()), [final / "valid"])
                if not followup:
                    self.assertEqual(list(final.parent.iterdir()), [final])

    def test_tar_symlink_chain_escape_rejected_in_both_orders(self):
        self.reject_links_after_preparation([("pkg/a", "."), ("pkg/b", "a/a/a/../../..")])

    def test_tar_temporary_tree_reentry_rejected_in_both_orders(self):
        self.reject_links_after_preparation([("pkg/a", "."), ("pkg/b", f"a/../tree.tmp-{os.getpid()}")])

    def test_tar_reentry_rejected_even_with_later_preparation(self):
        self.reject_links_after_preparation([
            ("pkg/d/e/f/g", None), ("pkg/a", "."), ("pkg/c", "d/e/f/g"),
            ("pkg/b", f"a/a/a/../tree.tmp-{os.getpid()}/c/../../../..")], followup=True)

    def test_tar_target_cannot_be_another_symlink(self):
        self.reject_links_after_preparation([("pkg/inside", None), ("pkg/a", "inside"), ("pkg/b", "a")])

    def test_tar_intermediate_component_cannot_be_a_symlink(self):
        self.reject_links_after_preparation([("pkg/inside/file", b"x"), ("pkg/a", "inside"), ("pkg/b", "a/file")])

    def test_tar_parent_escape_and_return_rejected_in_both_orders(self):
        self.reject_links_after_preparation([("pkg/file", b"x"), ("pkg/x", f"../tree.tmp-{os.getpid()}/file")])

    def test_tar_link_over_real_directories_survives_publication(self):
        for reverse in (False, True):
            with self.subTest(reverse=reverse):
                archive = self.link_archive("allowed.tar", [
                    ("pkg/a/b", None), ("pkg/c/file", b"x"), ("pkg/a/b/link", "../../c/file")], reverse)
                data = Path(self.dest) / str(reverse)
                data.mkdir(parents=True)
                fc.prepare_tree("extract_tar", {"src": archive.name, "dest": "out/tree", "strip": 1},
                                self.src, str(data), (20, 100, 100), {})
                link = data / "out/tree/a/b/link"
                self.assertTrue(link.is_symlink())
                self.assertEqual(os.readlink(link), "../../c/file")
                self.assertEqual(link.read_bytes(), b"x")
                self.assertEqual(list((data / "out").iterdir()), [data / "out/tree"])

    def test_tar_non_relative_link_prefixes_rejected(self):
        backslash = chr(92)
        for target in ("/file", "C:file", "C:/file", backslash * 2 + "server" + backslash + "share",
                       backslash * 2 + "?" + backslash + "C:" + backslash + "file", "file:target"):
            with self.subTest(target=target):
                self.reject_links_after_preparation([("pkg/file", b"x"), ("pkg/link", target)])

    def test_tar_link_text_normalization_matches_historical_tree_pin(self):
        for reverse in (False, True):
            with self.subTest(reverse=reverse):
                reference = Path(self.d) / f"reference-{reverse}"
                (reference / "data").mkdir(parents=True)
                (reference / "data/file").write_bytes(b"x")
                (reference / "data/file").chmod(0o644)
                (reference / "a/b").mkdir(parents=True)
                os.symlink("../../data/file", reference / "a/b/link")
                expected = fc.tree_digest(reference)
                archive = self.link_archive("normalized.tar", [
                    ("pkg/data/file", b"x"), ("pkg/a/b/link", "../.././data/file")], reverse)
                data = Path(self.dest) / str(reverse)
                data.mkdir(parents=True)
                pinned = {"tree:out/tree": {"tree_digest": fc.digest_record(expected)}}
                try:
                    result = fc.prepare_tree("extract_tar", {"src": archive.name, "dest": "out/tree", "strip": 1},
                                             self.src, str(data), (20, 100, 100), {}, pinned)
                except SystemExit as error:
                    self.fail(f"historical tree pin was not reproduced: {error}")
                self.assertEqual(result, "prepared")
                self.assertEqual(os.readlink(data / "out/tree/a/b/link"), "../../data/file")
                self.assertEqual(fc.tree_digest(data / "out/tree"), expected)

    def test_tar_hardlinks_are_skipped_in_both_orders(self):
        for reverse in (False, True):
            with self.subTest(reverse=reverse):
                archive = Path(self.src) / "hard.tar"
                links = [("pkg/a", "."), ("pkg/b", "a/a/a/../../..")]
                with tarfile.open(archive, "w") as stream:
                    for name, target in reversed(links) if reverse else links:
                        member = tarfile.TarInfo(name)
                        member.type = tarfile.LNKTYPE
                        member.linkname = target
                        stream.addfile(member)
                dest = Path(self.d) / ("hard-reverse" if reverse else "hard")
                budget = fc.Budget(10, 100, 100)
                fc.extract_tar(archive, dest, 1, budget)
                self.assertEqual(list(dest.iterdir()), [])
                self.assertEqual(budget.n, 2)

    def test_zip_link_attributes_never_create_links(self):
        import stat
        for reverse in (False, True):
            with self.subTest(reverse=reverse):
                archive = Path(self.src) / "links.zip"
                links = [("a", "."), ("b", "a/a/a/../../..")]
                with zipfile.ZipFile(archive, "w") as stream:
                    for name, target in reversed(links) if reverse else links:
                        member = zipfile.ZipInfo(name)
                        member.create_system = 3
                        member.external_attr = (stat.S_IFLNK | 0o777) << 16
                        stream.writestr(member, target)
                dest = Path(self.d) / ("zip-reverse" if reverse else "zip")
                fc.extract_zip(archive, dest, None, fc.Budget(10, 100, 100))
                for name, target in links:
                    self.assertFalse((dest / name).is_symlink())
                    self.assertEqual((dest / name).read_text(), target)

    def test_explicit_download_limit_smaller_than_declared_size(self):
        r = self.run_loader(self.one("c.bin", self.plain, size=5000), "--max-download-bytes", "1000")
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("download exceeds the limit of 1000 bytes", r.stderr)
        self.assertFalse((Path(self.dest) / ".downloads/c.bin").exists())
        self.assertFalse((Path(self.dest) / ".downloads/c.bin.part").exists())

    def test_stripped_tar_members_count_towards_limit(self):
        archive = Path(self.src) / "roots.tar"
        with tarfile.open(archive, "w") as stream:
            for _ in range(11):
                member = tarfile.TarInfo("pkg")
                member.type = tarfile.DIRTYPE
                stream.addfile(member)
        with self.assertRaisesRegex(SystemExit, "more than 10 members"):
            fc.extract_tar(archive, Path(self.d) / "roots", 1, fc.Budget(10, 100, 100))

    def test_unpinned_checksum_mode_rejected_before_loading(self):
        p = self.one("c.bin", self.plain)
        m = fc.read_json(p)
        m["inputs"][0]["files"][0]["checksum"] = {"algorithm": "record-on-first-load"}
        Path(p).write_text(json.dumps(m))
        r = self.run_loader(p)
        self.assertNotEqual(r.returncode, 0)
        self.assertFalse(Path(self.dest).exists())

    def test_wrong_content_rejected_on_first_response_and_cached_file(self):
        for cached in (False, True):
            with self.subTest(cached=cached):
                p = self.one("wrong.bin", self.plain)
                m = fc.read_json(p)
                m["inputs"][0]["files"][0]["checksum"] = fc.digest_record(sha(b"expected"))
                Path(p).write_text(json.dumps(m))
                if cached:
                    downloads = Path(self.dest) / ".downloads"
                    downloads.mkdir(parents=True, exist_ok=True)
                    (downloads / "wrong.bin").write_text("not a checksum\n")
                r = self.run_loader(p)
                self.assertEqual(r.returncode, 2, r.stderr)
                self.assertIn("CHECKSUM MISMATCH", r.stderr)
                self.assertFalse((Path(self.dest) / ".downloads/wrong.bin").exists())


for _n in dir(Hardening):
    if _n.startswith("test_") and _n not in ReviewRegressions.__dict__:
        setattr(ReviewRegressions, _n, None)


if __name__ == "__main__":
    unittest.main()
