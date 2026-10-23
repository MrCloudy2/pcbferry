#!/usr/bin/env python3
"""Build the KiCad plugin package (install with Plugin and Content Manager → Install from File).

    plugin/package.py --version 0.2.0 --engine linux-x86_64=dist/pcbferry-engine-linux-x86_64.tar.xz \
        [--engine windows-x86_64=...] [--out dist]

Writes dist/pcbferry-kicad-plugin-VERSION.zip, plus packages.json and repository.json for
a PCM repository. Each --engine is checksummed into engine.json; the plugin downloads the
engine for its platform from the release (--base-url) on first use.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import time
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
ID = "io.github.mrcloudy2.pcbferry"
URL = "https://github.com/MrCloudy2/pcbferry"


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(1 << 20):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--version", required=True)
    ap.add_argument("--engine", action="append", default=[], metavar="PLATFORM=FILE")
    ap.add_argument("--base-url", help="where the engines are published "
                    "(default: the GitHub release for --version)")
    ap.add_argument("--out", type=Path, default=REPO / "dist")
    args = ap.parse_args()
    base_url = args.base_url or f"{URL}/releases/download/v{args.version}/"

    files = {}
    for spec in args.engine:
        key, _, path = spec.partition("=")
        path = Path(path)
        files[key] = {"name": path.name, "sha256": sha256(path), "size": path.stat().st_size}
    platforms = sorted({k.split("-")[0] for k in files}) or ["linux", "macos", "windows"]

    version = {"version": args.version, "status": "stable", "kicad_version": "10.0",
               "runtime": "ipc", "platforms": platforms}
    metadata = {
        "$schema": "https://go.kicad.org/pcm/schemas/v1",
        "name": "pcbferry: Altium importer",
        "description": "Convert Altium Designer projects to KiCad, with library links and rules",
        "description_full": (
            "Converts an Altium project (.PrjPcb) into a KiCad project: schematics, board, "
            "libraries and design rules, every symbol and footprint linked to a library. "
            "Uses KiCad's Altium importer with pcbferry's fixes; on first use it downloads "
            "the converter engine for your system from GitHub.\n\n" + URL),
        "identifier": ID,
        "type": "plugin",
        "author": {"name": "MrCloudy2", "contact": {"github": URL}},
        "license": "GPL-3.0",
        "resources": {"homepage": URL},
        "tags": ["altium", "import", "converter"],
        "versions": [version],
    }

    args.out.mkdir(parents=True, exist_ok=True)
    zpath = args.out / f"pcbferry-kicad-plugin-{args.version}.zip"
    with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("metadata.json", json.dumps(metadata, indent=2))
        z.write(HERE / "src" / "icons" / "pcbferry-64.png", "resources/icon.png")
        for f in sorted((HERE / "src").rglob("*")):
            if f.is_file() and "__pycache__" not in f.parts:
                z.write(f, "plugins/" + f.relative_to(HERE / "src").as_posix())
        for name in ("pcbferry", "pcbferry_dru.py", "LICENSE"):
            z.write(REPO / name, f"plugins/{name}")
        z.writestr("plugins/engine.json", json.dumps(
            {"version": args.version, "base_url": base_url, "files": files}, indent=2))

    # A PCM repository (add its URL under Manage → repositories) listing this package
    version.update(download_url=f"{URL}/releases/download/v{args.version}/{zpath.name}",
                   download_sha256=sha256(zpath), download_size=zpath.stat().st_size,
                   install_size=sum(i.file_size for i in zipfile.ZipFile(zpath).infolist()))
    listing = dict(metadata, versions=[version])
    (args.out / "packages.json").write_text(json.dumps({"packages": [listing]}, indent=2))
    now = int(time.time())
    (args.out / "repository.json").write_text(json.dumps({
        "$schema": "https://go.kicad.org/pcm/schemas/v1#/definitions/Repository",
        "name": "pcbferry",
        "maintainer": {"name": "MrCloudy2", "contact": {"github": URL}},
        "packages": {"url": f"{URL}/releases/latest/download/packages.json",
                     "sha256": sha256(args.out / "packages.json"), "update_timestamp": now},
    }, indent=2))
    print(zpath)


if __name__ == "__main__":
    main()
