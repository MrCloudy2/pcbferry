"""KiCad action: convert Altium designs with pcbferry and open the result.

Asks what to convert (a project, a schematic or a board), which file, and where to put
it; converts it into FOLDER/NAME-kicad/ and offers to open the new project. The first
run downloads the engine.
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
# (menu entry, file patterns, file dialog title)
ACTIONS = [
    ("Convert an Altium project (.PrjPcb): schematics, board, libraries and rules",
     ["*.PrjPcb"], "Altium project to convert"),
    ("Convert a single Altium schematic (.SchDoc)", ["*.SchDoc"], "Altium schematic to convert"),
    ("Convert a single Altium board (.PcbDoc)", ["*.PcbDoc"], "Altium board to convert"),
]


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


def output_dir(folder: Path, name: str) -> Path:
    out = folder / f"{name}-kicad"
    n = 2
    while out.exists():
        out = folder / f"{name}-kicad-{n}"
        n += 1
    return out


# What pcbferry's library checks mean, for people
LIBRARY_ISSUES = {
    "lib_footprint_mismatch": "footprint(s) differ from their library copy; the board keeps "
                              "its own version",
    "lib_footprint_issues": "footprint(s) not found in a library",
    "footprint_link_issues": "symbol(s) link to a footprint that isn't in a library",
    "lib_symbol_issues": "symbol(s) not found in a library",
    "lib_symbol_mismatch": "symbol(s) differ from their library copy",
}


def report(out: Path) -> list[str]:
    """Things worth a look, from pcbferry's summary.json, in plain words."""
    try:
        summary = json.loads((out / "import-reports" / "summary.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return ["No summary was written; see import-reports/pcbferry.log."]
    notes = []
    for step in summary.get("steps", []):
        if not step.get("ok"):
            notes.append(f"The {step['step']} step failed.")
        missing = [e for e in step.get("errors", []) if e.startswith("File not found")]
        if missing:
            names = ", ".join(e.replace("\\", "/").rsplit("/", 1)[-1].rstrip(".") for e in missing)
            notes.append(f"{len(missing)} linked file(s) weren't found (they're on the original "
                         f"PC): {names}")
        for kind, n in step.get("library_issues", {}).items():
            if n:
                notes.append(f"{n} {LIBRARY_ISSUES.get(kind, kind)}")
    if summary.get("options", {}).get("custom_rules") and not any(
            st["step"] == "custom rules" and st.get("ok") for st in summary.get("steps", [])):
        notes.append("Altium's design rules couldn't be read.")
    return notes


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
        choice = dialogs.menu("What do you want to do?", [a[0] for a in ACTIONS])
        if choice is None:
            return 0
        _, patterns, title = ACTIONS[choice]

        settings = load_settings()
        chosen = dialogs.choose_file(title, settings.get("last_dir") or str(Path.home()), patterns)
        if not chosen:
            return 0
        src = Path(chosen)
        dest = dialogs.choose_dir(f"Where to put the KiCad project (a folder {src.stem}-kicad "
                                  "is made inside)", settings.get("last_out_dir") or str(src.parent))
        if not dest:
            return 0
        settings.update(last_dir=str(src.parent), last_out_dir=dest)
        save_settings(settings)

        cli = get_engine()
        if cli is None:
            return 1
        out = output_dir(Path(dest), src.stem)
        with dialogs.Busy(f"Converting {src.name}…"):
            convert(cli, src, out)

        project = next(out.glob("*.kicad_pro"), None)
        notes = report(out)
        if project is None:
            dialogs.message(f"{src.name} couldn't be converted.\n\n" + "\n".join(notes)
                            + f"\n\nDetails: {out / 'import-reports'}", error=True)
            return 1
        text = f"{src.name} is converted:\n{project}\n\n"
        if notes:
            text += "Worth a look:\n" + "\n".join(f"• {n}" for n in notes) + "\n\n"
        text += ("Run DRC in KiCad before manufacturing: Altium and KiCad check different "
                 "things. Reports are in import-reports/.\n\nOpen the project in KiCad?")
        if dialogs.ask(text, "Open", "Close"):
            dialogs.open_path(str(project))
        return 0
    finally:
        try:
            held.unlink()
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
