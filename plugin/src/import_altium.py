"""KiCad action: convert an Altium project with pcbferry and open the result.

Asks for a .PrjPcb (or .SchDoc / .PcbDoc), converts it next to the original into
NAME-kicad/, and offers to open the new project. The first run downloads the engine.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import dialogs  # noqa: E402
import engine  # noqa: E402

SETTINGS = engine.data_dir() / "settings.json"
PATTERNS = ["*.PrjPcb", "*.SchDoc", "*.PcbDoc"]


def load_settings() -> dict:
    try:
        return json.loads(SETTINGS.read_text())
    except (OSError, ValueError):
        return {}


def save_settings(settings: dict) -> None:
    try:
        SETTINGS.parent.mkdir(parents=True, exist_ok=True)
        SETTINGS.write_text(json.dumps(settings, indent=1))
    except OSError:
        pass


def output_dir(src: Path) -> Path:
    out = src.parent / f"{src.stem}-kicad"
    n = 2
    while out.exists():
        out = src.parent / f"{src.stem}-kicad-{n}"
        n += 1
    return out


def get_engine() -> Path | None:
    cli = engine.installed()
    if cli:
        return cli
    size = engine.download_size()
    if size is None:
        dialogs.message(f"pcbferry has no converter engine for this system ({engine.platform_key()}) "
                        "yet.\n\nThe container version works everywhere Docker runs:\n"
                        "https://github.com/MrCloudy2/pcbferry", error=True)
        return None
    if not dialogs.ask(f"pcbferry needs to download its converter engine once "
                       f"({size / 1e6:.0f} MB) from GitHub.\n\nDownload it now?",
                       "Download", "Cancel"):
        return None
    try:
        with dialogs.Busy(f"Downloading the pcbferry engine ({size / 1e6:.0f} MB)…"):
            return engine.install()
    except Exception as exc:  # network, disk, checksum
        dialogs.message(f"Couldn't install the engine:\n{exc}", error=True)
        return None


def lock():
    """One conversion at a time: a second click while one runs does nothing."""
    path = Path(tempfile.gettempdir()) / "pcbferry-plugin.lock"
    try:
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
    except FileExistsError:
        try:  # stale after a crash?
            if os.path.getmtime(path) < time.time() - 3600:
                os.unlink(path)
                return lock()
        except OSError:
            pass
        return None
    os.close(fd)
    return path


def convert(cli: Path, src: Path, out: Path) -> tuple[bool, str]:
    env = dict(os.environ, KICAD_CLI=str(cli), PYTHONIOENCODING="utf-8")
    cmd = [sys.executable, str(HERE / "pcbferry"), "--check", "--fill-zones", str(src), str(out)]
    proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace",
                          env=env, creationflags=dialogs.NO_WINDOW)
    log = proc.stdout + proc.stderr
    try:
        (out / "import-reports").mkdir(parents=True, exist_ok=True)
        (out / "import-reports" / "pcbferry.log").write_text(log, encoding="utf-8")
    except OSError:
        pass
    return proc.returncode == 0, log


def main() -> int:
    held = lock()
    if held is None:
        return 0
    try:
        settings = load_settings()
        start = settings.get("last_dir") or str(Path.home())
        chosen = dialogs.choose_file("Altium project to convert", start, PATTERNS)
        if not chosen:
            return 0
        src = Path(chosen)
        settings["last_dir"] = str(src.parent)
        save_settings(settings)

        cli = get_engine()
        if cli is None:
            return 1
        out = output_dir(src)
        with dialogs.Busy(f"Converting {src.name}…"):
            ok, log = convert(cli, src, out)

        project = next(out.glob("*.kicad_pro"), None)
        last = [l for l in log.splitlines() if l.startswith("pcbferry:")][-3:]
        if project is None:
            dialogs.message("The conversion failed.\n\n" + "\n".join(last)
                            + f"\n\nDetails: {out / 'import-reports'}", error=True)
            return 1
        verdict = "Converted" if ok else "Converted, with problems"
        if dialogs.ask(f"{verdict}:\n{project}\n\n" + "\n".join(last)
                       + "\n\nReports are in import-reports/. Open the project in KiCad?",
                       "Open", "Close"):
            dialogs.open_path(str(project))
        return 0
    finally:
        try:
            held.unlink()
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
