"""KiCad action: convert Altium designs with pcbferry.

Asks what to do:
- import an Altium project into the project open in KiCad: the converted files take the
  open project's name; once the project is closed in KiCad they replace its files (the
  old ones are kept in NAME-before-pcbferry/), and reopening it shows the design;
- or convert a project, schematic or board into FOLDER/NAME-kicad/ and open that.
The first run downloads the engine.
"""
from __future__ import annotations

import json
import os
import shutil
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
INTO_PROJECT = ("Import an Altium project into this project ({name})", ["*.PrjPcb"],
                "Altium project to import")
ACTIONS = [
    ("Convert an Altium project (.PrjPcb) into a new folder", ["*.PrjPcb"],
     "Altium project to convert"),
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


def convert(cli: Path, src: Path, out: Path, name: str | None = None) -> tuple[bool, str]:
    env = dict(os.environ, KICAD_CLI=str(cli), PYTHONIOENCODING="utf-8")
    cmd = [sys.executable, str(HERE / "pcbferry"), "--check", "--fill-zones",
           *(["--name", name] if name else []), str(src), str(out)]
    proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace",
                          env=env, creationflags=dialogs.NO_WINDOW)
    log = proc.stdout + proc.stderr
    try:
        (out / "import-reports").mkdir(parents=True, exist_ok=True)
        (out / "import-reports" / "pcbferry.log").write_text(log, encoding="utf-8")
    except OSError:
        pass
    return proc.returncode == 0, log


def open_project() -> Path | None:
    """The .kicad_pro open in the KiCad that started us (KiCad sets KIPRJMOD to its folder)."""
    folder = os.environ.get("KIPRJMOD")
    projects = sorted(Path(folder).glob("*.kicad_pro")) if folder else []
    return projects[0] if len(projects) == 1 else None


def has_design(project: Path) -> bool:
    """Does the project already hold a schematic or board with something in it?"""
    for ext, marker in ((".kicad_sch", "(lib_id "), (".kicad_pcb", "(footprint ")):
        try:
            if marker in project.with_suffix(ext).read_text(encoding="utf-8", errors="replace"):
                return True
        except OSError:
            pass
    return False


def unique(path: Path) -> Path:
    n, candidate = 2, path
    while candidate.exists():
        candidate, n = path.with_name(f"{path.name}-{n}"), n + 1
    return candidate


def finish_message(notes: list[str], head: str, tail: str) -> str:
    text = f"{head}\n\n"
    if notes:
        text += "Worth a look:\n" + "\n".join(f"• {n}" for n in notes) + "\n\n"
    return text + ("Run DRC in KiCad before manufacturing: Altium and KiCad check different "
                   "things. Reports are in import-reports/.\n\n" + tail)


def import_into(cli: Path, src: Path, project: Path) -> int:
    name, folder = project.stem, project.parent
    if has_design(project) and not dialogs.ask(
            f"{name} already has a schematic or board. Importing replaces them; the current "
            f"files are kept in {name}-before-pcbferry/.\n\nImport anyway?", "Import", "Cancel"):
        return 0
    staged = unique(folder / f"{name}-pcbferry-import")
    with dialogs.Busy(f"Converting {src.name}…"):
        convert(cli, src, staged, name=name)
    notes = report(staged)
    if not (staged / f"{name}.kicad_pro").is_file():
        dialogs.message(f"{src.name} couldn't be converted.\n\n" + "\n".join(notes)
                        + f"\n\nDetails: {staged / 'import-reports'}", error=True)
        return 1

    # KiCad holds the project's files while it is open and would write its own (empty)
    # copies back over ours, so they are swapped in only once the project is closed.
    lockfile = folder / f"~{name}.kicad_pro.lck"
    if lockfile.exists():
        with dialogs.Busy(f"{src.name} is converted. To finish, close {name} in KiCad: "
                          "File → Close Project in the main KiCad window (if it asks to "
                          "save the old schematic or board, don't).", cancellable=True) as busy:
            deadline = time.time() + 1800
            while lockfile.exists() and not busy.cancelled and time.time() < deadline:
                time.sleep(0.5)
        if lockfile.exists():
            dialogs.message(f"{name} is still open, so nothing was replaced. The converted "
                            f"project is in:\n{staged}")
            return 0

    backup = unique(folder / f"{name}-before-pcbferry")
    for item in sorted(staged.iterdir()):
        target = folder / item.name
        if target.exists():
            backup.mkdir(exist_ok=True)
            shutil.move(str(target), str(backup / item.name))
        shutil.move(str(item), str(target))
    staged.rmdir()
    kept = f"\nThe previous files are in {backup.name}/." if backup.exists() else ""
    dialogs.message(finish_message(
        notes, f"{src.name} is imported into {name}.{kept}",
        f"Open {name} again in KiCad (File → Open Recent) to see it."))
    return 0


def convert_to_folder(cli: Path, src: Path, settings: dict) -> int:
    dest = dialogs.choose_dir(f"Where to put the KiCad project (a folder {src.stem}-kicad "
                              "is made inside)", settings.get("last_out_dir") or str(src.parent))
    if not dest:
        return 0
    settings["last_out_dir"] = dest
    save_settings(settings)
    out = output_dir(Path(dest), src.stem)
    with dialogs.Busy(f"Converting {src.name}…"):
        convert(cli, src, out)
    project = next(out.glob("*.kicad_pro"), None)
    notes = report(out)
    if project is None:
        dialogs.message(f"{src.name} couldn't be converted.\n\n" + "\n".join(notes)
                        + f"\n\nDetails: {out / 'import-reports'}", error=True)
        return 1
    if dialogs.ask(finish_message(notes, f"{src.name} is converted:\n{project}",
                                  "Open the project in KiCad?"), "Open", "Close"):
        dialogs.open_path(str(project))
    return 0


def main() -> int:
    held = lock()
    if held is None:
        return 0
    try:
        project = open_project()
        actions = ([(INTO_PROJECT[0].format(name=project.stem), *INTO_PROJECT[1:])]
                   if project else []) + ACTIONS
        choice = dialogs.menu("What do you want to do?", [a[0] for a in actions])
        if choice is None:
            return 0
        _, patterns, title = actions[choice]
        into = project is not None and choice == 0

        settings = load_settings()
        chosen = dialogs.choose_file(title, settings.get("last_dir") or str(Path.home()), patterns)
        if not chosen:
            return 0
        src = Path(chosen)
        settings["last_dir"] = str(src.parent)
        save_settings(settings)

        cli = get_engine()
        if cli is None:
            return 1
        return import_into(cli, src, project) if into else convert_to_folder(cli, src, settings)
    finally:
        try:
            held.unlink()
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
