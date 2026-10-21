#!/usr/bin/env python3
"""Package an already verified macOS executable with docs and exact provenance."""
import argparse
import hashlib
import json
import platform
import shutil
import subprocess
import tarfile
from datetime import datetime, timezone
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/loh"))
    parser.add_argument("--output", type=Path, default=Path("dist"))
    parser.add_argument("--replay", type=Path, help="include a selected synthetic smoke event export")
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("this package requires a macOS build")
    binary = args.binary.resolve(strict=True)
    info = json.loads(subprocess.check_output([binary, "doctor", "--json"], text=True))
    name = f"loh-macos-{platform.machine()}-{info['version']}"
    directory = args.output / name
    if directory.exists() or (args.output / f"{name}.tar.gz").exists():
        parser.error("output already exists; choose a new output directory to preserve provenance")
    directory.mkdir(parents=True)
    shutil.copy2(binary, directory / "loh")
    shutil.copy2("README.md", directory / "README.md")
    shutil.copytree("docs", directory / "docs")
    (directory / "scripts").mkdir()
    shutil.copy2("scripts/replay.py", directory / "scripts/replay.py")
    if args.replay:
        replay = args.replay.resolve(strict=True)
        destination = directory / "artifacts" / replay.name
        destination.mkdir(parents=True)
        # Explicit allowlist: never copy the credential, campaign database or
        # arbitrary provider logs into a distributable replay example.
        for filename in ("events.json", "summary.json", "acceptance.json", "initial-acceptance.json"):
            shutil.copy2(replay / filename, destination / filename)
    # Only executable provenance is exported; doctor environment/config details
    # and the caller's API key are deliberately absent from the distributable.
    record = {
        "product": "Logit- Harness",
        "command": "loh",
        "version": info["version"],
        "source_hash": info["source_hash"],
        "platform": platform.platform(),
        "created": datetime.now(timezone.utc).isoformat(),
        "binary_sha256": hashlib.file_digest(binary.open("rb"), "sha256").hexdigest(),
    }
    (directory / "manifest.json").write_text(json.dumps(record, indent=2) + "\n")
    archive = args.output / f"{name}.tar.gz"
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(directory, arcname=name)
    digest = hashlib.file_digest(archive.open("rb"), "sha256").hexdigest()
    (args.output / f"{archive.name}.sha256").write_text(f"{digest}  {archive.name}\n")
    print(archive.resolve())


if __name__ == "__main__":
    main()
