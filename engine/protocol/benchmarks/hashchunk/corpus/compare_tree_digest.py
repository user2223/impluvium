# SPDX-License-Identifier: 0BSD
"""Offline Rust/Python digest comparison; run with the built tool in a container."""
import argparse
from pathlib import Path
import subprocess
import tempfile

from fetch_corpora import tree_digest


def compare(tool, tree):
    result = subprocess.run([tool, "digest", str(tree)], check=True, capture_output=True, text=True)
    expected = tree_digest(str(tree))
    if result.stdout.strip() != expected:
        raise SystemExit(f"Rust/Python tree digest mismatch: {tree.name}")
    print(f"{tree.name}: Rust/Python digest equal ({expected})")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tool", required=True)
    parser.add_argument("--tree", type=Path)
    args = parser.parse_args()
    tool = str(Path(args.tool).resolve(strict=True))
    if args.tree is not None:
        compare(tool, args.tree)
        return
    with tempfile.TemporaryDirectory() as folder:
        root = Path(folder)
        subprocess.run([tool, "gen", "--out", str(root / "gen"), "--files", "12",
                        "--symlinks", "2", "--no-boundary"], check=True)
        for version in ("v1", "v2"):
            compare(tool, root / "gen" / version)
        # The single-file contract uses a fixed mode, even for an executable.
        executable = root / "executable"
        executable.write_text("fixture\n")
        executable.chmod(0o755)
        compare(tool, executable)


if __name__ == "__main__":
    main()
