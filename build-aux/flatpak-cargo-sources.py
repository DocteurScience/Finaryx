#!/usr/bin/env python3
"""Writes build-aux/cargo-sources.json from Cargo.lock.

Flathub builds without network access: every crate is declared as a
flatpak-builder source (the .crate archive from crates.io, checked with the
SHA-256 recorded in Cargo.lock) and unpacked into cargo/vendor, with a Cargo
configuration that points crates.io to that folder.

Run it again after any change of Cargo.lock:

    python3 build-aux/flatpak-cargo-sources.py

Only crates from crates.io are supported (Finaryx has no git dependency).
"""

import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"
VENDOR = "cargo/vendor"


def main() -> int:
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    sources = []
    for package in lock["package"]:
        source = package.get("source")
        if source is None:
            continue  # A crate of this workspace.
        if source != CRATES_IO:
            print(f"Unsupported source for {package['name']}: {source}", file=sys.stderr)
            return 1
        name, version, checksum = package["name"], package["version"], package["checksum"]
        dest = f"{VENDOR}/{name}-{version}"
        sources.append(
            {
                "type": "archive",
                "archive-type": "tar-gzip",
                "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
                "sha256": checksum,
                "dest": dest,
            }
        )
        # Cargo checks vendored crates against this file; an empty file list
        # skips the per-file check, the archive is already verified.
        sources.append(
            {
                "type": "inline",
                "contents": json.dumps({"package": checksum, "files": {}}),
                "dest": dest,
                "dest-filename": ".cargo-checksum.json",
            }
        )
    sources.append(
        {
            "type": "inline",
            "contents": (
                '[source.crates-io]\nreplace-with = "vendored-sources"\n\n'
                f'[source.vendored-sources]\ndirectory = "{VENDOR}"\n'
            ),
            "dest": "cargo",
            "dest-filename": "config.toml",
        }
    )
    out = ROOT / "build-aux" / "cargo-sources.json"
    out.write_text(json.dumps(sources, indent=4) + "\n", encoding="utf-8")
    print(f"{out.relative_to(ROOT)}: {len(sources) // 2} crates")
    return 0


if __name__ == "__main__":
    sys.exit(main())
