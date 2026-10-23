"""The converter engine: pcbferry's patched kicad-cli, downloaded once per version.

KiCad's own importer can't be replaced from a plugin, so the plugin runs the patched
kicad-cli that the container image also uses, built for this platform and published with
each release. engine.json (written by package.py) names the file and its SHA-256 for
every platform.

Environment: PCBFERRY_ENGINE_DIR  an unpacked engine to use instead (development)
"""
from __future__ import annotations

import hashlib
import json
import os
import platform
import shutil
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path, PurePosixPath

HERE = Path(__file__).resolve().parent


def platform_key() -> str:
    machine = platform.machine().lower()
    machine = {"amd64": "x86_64", "x64": "x86_64", "aarch64": "arm64"}.get(machine, machine)
    system = {"win32": "windows", "darwin": "macos"}.get(sys.platform, "linux")
    return f"{system}-{machine}"


def data_dir() -> Path:
    if sys.platform == "win32":
        base = Path(os.environ.get("LOCALAPPDATA") or Path.home() / "AppData" / "Local")
    elif sys.platform == "darwin":
        base = Path.home() / "Library" / "Application Support"
    else:
        base = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local" / "share")
    return base / "pcbferry"


def manifest() -> dict:
    return json.loads((HERE / "engine.json").read_text())


def cli_path(root: Path) -> Path:
    return root / "pcbferry-engine" / "bin" / ("kicad-cli.exe" if sys.platform == "win32" else "kicad-cli")


def installed() -> Path | None:
    """kicad-cli of the engine for this plugin version, if it is already here."""
    if os.environ.get("PCBFERRY_ENGINE_DIR"):
        cli = cli_path(Path(os.environ["PCBFERRY_ENGINE_DIR"]).parent)
        return cli if cli.is_file() else None
    cli = cli_path(data_dir() / f"engine-{manifest()['version']}")
    return cli if cli.is_file() else None


def download_size() -> int | None:
    entry = manifest()["files"].get(platform_key())
    return entry and entry["size"]


def _safe_extract(archive: Path, dest: Path) -> None:
    with tarfile.open(archive) as tar:
        for member in tar.getmembers():
            path = PurePosixPath(member.name)
            if path.is_absolute() or ".." in path.parts or member.issym() or member.islnk() \
                    or not (member.isfile() or member.isdir()) or path.parts[0] != "pcbferry-engine":
                raise RuntimeError(f"unexpected entry in the engine archive: {member.name}")
        if sys.version_info >= (3, 12):
            tar.extractall(dest, filter="data")
        else:
            tar.extractall(dest)


def install(progress=lambda done, total: None) -> Path:
    """Download, check and unpack the engine; returns its kicad-cli."""
    info = manifest()
    entry = info["files"].get(platform_key())
    if entry is None:
        raise RuntimeError(f"no pcbferry engine for {platform_key()} yet "
                           f"(available: {', '.join(sorted(info['files'])) or 'none'})")
    root = data_dir()
    root.mkdir(parents=True, exist_ok=True)
    target = root / f"engine-{info['version']}"

    with tempfile.TemporaryDirectory(dir=root) as tmp:
        archive = Path(tmp) / entry["name"]
        digest = hashlib.sha256()
        with urllib.request.urlopen(info["base_url"] + entry["name"], timeout=60) as response, \
                archive.open("wb") as out:
            done = 0
            while chunk := response.read(1 << 20):
                out.write(chunk)
                digest.update(chunk)
                done += len(chunk)
                progress(done, entry["size"])
        if digest.hexdigest() != entry["sha256"]:
            raise RuntimeError("the downloaded engine doesn't match its checksum; try again")
        unpacked = Path(tmp) / "unpacked"
        _safe_extract(archive, unpacked)
        shutil.rmtree(target, ignore_errors=True)
        unpacked.rename(target)

    for old in root.glob("engine-*"):  # earlier versions
        if old != target:
            shutil.rmtree(old, ignore_errors=True)
    return cli_path(target)
