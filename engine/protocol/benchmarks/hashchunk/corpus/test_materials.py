# SPDX-License-Identifier: 0BSD
"""Small publication checks; no corpus downloads or external dependencies."""
import copy
import itertools
import json
from pathlib import Path
import re
import unittest

import fetch_corpora as fc

ROOT = Path(__file__).resolve().parent.parent
FIELDS = {"readers", "workers", "file_hash", "cache_state"}


def check_index(index, root):
    if index["model_id"] != "historical-provisional-manifest" or index["new_result_contract"]["model_id"] != "protocol-manifest-v3":
        raise ValueError("historical and current models must be distinct")
    if index["result_format_version"] != "historical-unversioned/1" or index["generator_version"] != "corpus-model/1":
        raise ValueError("missing historical format or generator version")
    rows = index["records"]
    ids = [r["id"] for r in rows]
    files = [r["file"] for r in rows if r["file"] is not None]
    actual = {p.relative_to(root).as_posix() for p in (root / "historical").rglob("*") if p.is_file()}
    if len(rows) != 54 or len(set(ids)) != 54 or len(set(files)) != len(files) or set(files) != actual:
        raise ValueError("incomplete or duplicate historical inventory")
    matrix = {f"historical/hash-set-2026-11-03/hash-{tree}-r{reader}-{cache}.json"
              for tree, reader, cache in itertools.product(("generator", "0ad", "xonotic", "kernel-archive"), (1, 4), ("warm", "cold"))}
    if {f for f in files if "/hash-set-" in f and f.endswith(".json")} != matrix:
        raise ValueError("incomplete 16-file hash matrix")
    for row in rows:
        if not row["tool_revision"] or not row["revision_evidence"] or set(row["measurement_fields"]) != FIELDS:
            raise ValueError("required measurement metadata missing")
        if row["kind"] == "hash":
            raw = fc.read_json(root / row["file"])
            params = raw["parameters"]
            expected = {"readers": params["readers"], "workers": params["workers"],
                        "file_hash": params["file_hash_of_split_files"],
                        "cache_state": sorted({c["cache_state"] for c in raw["conditions"]})}
            if row["measurement_fields"] != expected or row["original_quality"] != raw["decision_grade"]:
                raise ValueError("index must preserve original metadata and quality")
            revision = raw["machine"]["source_revision"]
            if revision and (row["tool_revision"] != revision or row["revision_evidence"] != "raw_result"):
                raise ValueError("incorrect embedded revision association")
            if not revision and (row["revision_evidence"] != "measurement-notes-2026-11-09" or "embedded_tool_revision" not in row["missing_metadata"]):
                raise ValueError("documentary revision must not be represented as embedded")
            if "-vm-" in row["file"] or "windows" in row["file"]:
                if row["assessment"] != "informative" or not row.get("assessment_reason"):
                    raise ValueError("VM and Windows results are informative")
            if row["file"] in matrix:
                conditions = raw["conditions"]
                expected_conditions = {(a, w, s) for a, w, s in itertools.product(("sha256", "blake3"), (1, 4), ("build", "verify"))} | {("sha256", 0, "chunk-only")}
                if len(conditions) != 9 or {(c["algorithm"], c["workers"], c["stage"]) for c in conditions} != expected_conditions:
                    raise ValueError("incomplete hash conditions")
                reader = int(re.search(r"-r([14])-", row["file"])[1])
                cache = "warm" if "-warm" in row["file"] else "cold"
                if params["readers"] != reader or params["reps"] != 5 or not params["file_hash_of_split_files"]:
                    raise ValueError("incorrect hash parameters")
                for c in conditions:
                    if c["readers"] != reader or c["cache"] != cache or len(c["seconds_each_rep"]) != 5 or c["mismatches"] != 0 or c["cache_state"] != "valid":
                        raise ValueError("invalid historical repetitions or cache")
        elif row["kind"] == "parser-model":
            if set(row["measurement_fields"].values()) != {"not_recorded"} or not FIELDS <= set(row["missing_metadata"]):
                raise ValueError("missing parser metadata cannot be invented")


def check_manifest(m):
    fc.validate_manifest(m)
    pins = m["historical_verification"]["values"]
    for inp in m["inputs"]:
        signature = inp["signature_verification"]
        if inp["id"].startswith(("linux-", "0ad-")) and signature["status"] != "not_verified":
            raise ValueError("PGP and Minisign remain unverified")
        if inp["kind"] == "generated":
            if inp["checksum_verification"]["status"] != "not_applicable":
                raise ValueError("generated data has no upstream checksum")
            if inp["id"] == "generator-boundary-filtered-v1v2" and (inp["tree_verification"]["status"] != "not_recorded" or inp["expected_tree_digest"] is not None):
                raise ValueError("missing filter digest must remain missing")
        for f in inp.get("files", []):
            if f["signature_verification"] != signature or "checksum_verification" not in f:
                raise ValueError("separate signature and checksum status required")
            status = f["checksum_verification"]
            if f["name"] in pins:
                pin = pins[f["name"]]
                if status["status"] != "verified_historically" or not status["source"] or not status["evidence"] or not status["expected"]:
                    raise ValueError("historical checksum evidence missing")
                expected = pin["official"]
                if expected is None:
                    if f["checksum"].get("source_kind") != "historical_pin":
                        raise ValueError("historical pin must not be presented as an official checksum")
                    expected = pin["sha256"]
                if fc.hex_digest(f["checksum"]) != fc.hex_digest(expected):
                    raise ValueError("checksum disagrees with historical record")
        for st in inp.get("prepare", []):
            tree = st["tree_verification"]
            if "checksum_verification" not in st or st["signature_verification"] != signature:
                raise ValueError("prepared tree signature status missing")
            key = "tree:" + st["dest"]
            if key in pins and (tree["expected"] != pins[key]["tree_digest"] or tree["status"] != "recorded_historically" or not tree["evidence"]):
                raise ValueError("prepared tree pin missing")


def check_text(path):
    if path.suffix not in {".json", ".md", ".txt", ".patch", ".py"}:
        raise ValueError("only textual publication formats allowed")
    text = path.read_bytes().decode("ascii")
    if any(ord(c) < 32 and c not in "\n\r\t" for c in text):
        raise ValueError("binary data is not a publication record")
    if re.search(r"[A-Za-z0-9+/]{64,}={0,2}", text):
        raise ValueError("long encoded block or unstructured digest")
    if re.search(r"(?:[A-Za-z]:\\|(?<![A-Za-z0-9_.])/(?:home|Users|data[0-9]*)/|~[/])", text):
        raise ValueError("private host path")
    if re.search(r"\b(?:E-\d{4}|P[1-9]-\d\w*|E1_[A-Z_]+)\b", text):
        raise ValueError("internal identifier")


class Materials(unittest.TestCase):
    def test_reduced_environment_metadata(self):
        def walk(value):
            if isinstance(value, dict):
                for key, item in value.items():
                    if key == "kernel" and item:
                        self.assertIsNotNone(re.fullmatch(r"\d+(?:\.\d+)*", item))
                    if key.startswith("loadavg") and item:
                        self.assertEqual(len(item.split()), 4, "load average must omit the last process number")
                    walk(item)
            elif isinstance(value, list):
                for item in value:
                    walk(item)

        for path in (ROOT / "results").rglob("*"):
            if not path.is_file():
                continue
            with self.subTest(path=path.relative_to(ROOT)):
                text = path.read_text(encoding="ascii")
                for offset in re.findall(r"[+-]\d{2}:\d{2}", text):
                    self.assertIn(offset, ("+00:00", "-00:00"), "timestamps must use UTC")
                if path.suffix == ".json":
                    walk(json.loads(text))

    def test_manifest_and_independent_verification_status(self):
        m = fc.read_json(ROOT / "corpus/corpus-manifest.json")
        check_manifest(m)
        records = fc.read_json(ROOT / "results/historical/recorded-checksums-free-corpora.json")
        self.assertEqual(m["historical_verification"]["values"], records)
        for field in ("checksum_verification", "signature_verification"):
            bad = copy.deepcopy(m)
            del next(i for i in bad["inputs"] if i["id"] == "linux-7.2")["files"][0][field]
            with self.assertRaises((ValueError, KeyError)):
                check_manifest(bad)
        bad = copy.deepcopy(m)
        next(i for i in bad["inputs"] if i["id"] == "linux-7.2")["signature_verification"]["status"] = "verified"
        with self.assertRaises(ValueError):
            check_manifest(bad)

    def test_index_and_complete_historical_matrix(self):
        index = fc.read_json(ROOT / "results/index.json")
        check_index(index, ROOT / "results")
        for mutate in (lambda j: j["records"].pop(),
                       lambda j: j["records"][0]["measurement_fields"].pop("readers"),
                       lambda j: j.update(model_id="protocol-manifest-v3"),
                       lambda j: next(r for r in j["records"] if "hash-vm" in r["id"]).update(assessment="decisive")):
            bad = copy.deepcopy(index)
            mutate(bad)
            with self.assertRaises((ValueError, KeyError)):
                check_index(bad, ROOT / "results")

    def test_relative_configs_and_publication_text(self):
        configs = list((ROOT / "corpus/configs").glob("*.json"))
        self.assertEqual(len(configs), 6)
        for path in configs:
            for cls in fc.read_json(path)["classes"]:
                for pair in cls["pairs"]:
                    for key in ("old", "new"):
                        self.assertFalse(Path(pair[key]).is_absolute())
                        self.assertEqual(Path(pair[key]).parts[:2], ("..", "data"))
        files = list((ROOT / "results").rglob("*")) + list((ROOT / "corpus").rglob("*")) + [ROOT / "README.md", ROOT / "PROVENANCE.md", ROOT.parent.parent / "spec/decisions/hash-chunking.md"]
        for path in files:
            if path.is_file():
                with self.subTest(path=path.relative_to(ROOT.parent.parent)):
                    check_text(path)

    def test_publication_rejects_archive_payload_and_host_path(self):
        import tempfile
        with tempfile.TemporaryDirectory() as folder:
            for name, text in (("data.zip", "archive"), ("record.txt", "A" * 64),
                               ("path.txt", "/" + "home" + "/example/data"),
                               ("binary.txt", "x" + chr(0))):
                path = Path(folder) / name
                path.write_text(text)
                with self.subTest(name=name), self.assertRaises(ValueError):
                    check_text(path)
