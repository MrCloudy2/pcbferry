"""KiCad action: convert Altium designs with pcbferry.

Asks what to do:
- import an Altium project in place of the project open in KiCad (typically a new, empty
  one made where the design should go): the design is converted under its Altium name
  into that folder; once KiCad is closed the open project's files are moved aside into
  NAME-before-pcbferry/ and KiCad is started again with the converted project;
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
# (menu title, menu description, file patterns, file dialog title)
INTO_PROJECT = ("Import an Altium project here",
                "Converts an Altium project into this folder in place of {name} (moved "
                "aside, not deleted). KiCad then restarts with the converted project.",
                ["*.PrjPcb"], "Altium project to import")
ACTIONS = [
    ("Convert an Altium project to a new folder",
     "Schematics, board, libraries and design rules from a .PrjPcb, into a folder you pick.",
     ["*.PrjPcb"], "Altium project to convert"),
    ("Convert a single schematic", "One .SchDoc sheet, into a folder you pick.",
     ["*.SchDoc"], "Altium schematic to convert"),
    ("Convert a single board", "One .PcbDoc, into a folder you pick.",
     ["*.PcbDoc"], "Altium board to convert"),
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
    # From an AppImage KiCad: APPDIR would make the engine load the AppImage's kifaces
    for var in ("APPDIR", "SHARUN_DIR", "KICAD_STOCK_DATA_HOME"):
        env.pop(var, None)
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


def kicad_program() -> list[str] | None:
    """How to start the KiCad that started us (the project manager, even when we were
    started from a standalone editor). Must be called while that KiCad still runs."""
    if sys.platform == "darwin":
        return ["open"]  # .kicad_pro opens in KiCad
    if sys.platform == "win32":
        return None  # os.startfile
    try:
        ppid = os.getppid()
        argv0 = Path(Path(f"/proc/{ppid}/cmdline").read_bytes().split(b"\0")[0].decode())
        exe = Path(os.readlink(f"/proc/{ppid}/exe"))
    except OSError:
        return ["xdg-open"]
    # AppImages start through a launcher (argv[0]); distro builds are the binary itself
    program = argv0 if argv0.is_absolute() else Path(shutil.which(str(argv0)) or exe)
    kicad = program.with_name("kicad")
    return [str(kicad if kicad.exists() else program)]


def start_kicad(program: list[str] | None, project: Path) -> None:
    if program is None:
        os.startfile(project)  # noqa: S606
        return
    subprocess.Popen([*program, str(project)], stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                     start_new_session=True, cwd=project.parent)


def parent_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


def import_into(cli: Path, src: Path, current: Path, program: list[str] | None) -> int:
    old, folder = current.stem, current.parent
    if has_design(current) and not dialogs.ask(
            f"{old} already has a schematic or board. The import moves it into "
            f"{old}-before-pcbferry/.\n\nImport anyway?", "Import", "Cancel"):
        return 0
    staged = unique(folder / f"{src.stem}-pcbferry-import")
    with dialogs.Busy(f"Converting {src.name}…"):
        convert(cli, src, staged)
    notes = report(staged)
    converted = next(staged.glob("*.kicad_pro"), None)
    if converted is None:
        dialogs.message(f"{src.name} couldn't be converted.\n\n" + "\n".join(notes)
                        + f"\n\nDetails: {staged / 'import-reports'}", error=True)
        return 1
    name = converted.stem

    # KiCad holds the open project's files and would write its own copies back over ours,
    # so the swap waits until the project is closed (KiCad deletes its lock file).
    lockfile = folder / f"~{old}.kicad_pro.lck"
    kicad_pid = os.getppid()
    if lockfile.exists():
        notice = dialogs.Notice(
            f"✔ {src.name} is converted.\n\nTo finish, quit KiCad (File → Quit). pcbferry "
            f"then moves {old} aside and opens {name} in KiCad.\n\n"
            "If KiCad asks to save changes, choose not to.", "Cancel import")
        deadline = time.time() + 3600
        while lockfile.exists() and not notice.closed and time.time() < deadline:
            time.sleep(0.5)
        notice.close()
        if lockfile.exists():
            dialogs.message(f"The import was cancelled; {old} is unchanged. The converted "
                            f"project is in:\n{staged}")
            return 0

    # The open project's own files, and anything the converted project would overwrite
    backup = unique(folder / f"{old}-before-pcbferry")
    move_aside = [p for p in folder.iterdir()
                  if (p.stem == old and p.suffix.startswith(".kicad_"))
                  or p.name in ("sym-lib-table", "fp-lib-table")
                  or (staged / p.name).exists()]
    for path in move_aside:
        backup.mkdir(exist_ok=True)
        shutil.move(str(path), str(backup / path.name))
    for item in sorted(staged.iterdir()):
        shutil.move(str(item), str(folder / item.name))
    staged.rmdir()
    project = folder / f"{name}.kicad_pro"

    # Quit KiCad: start it again with the new project. Only closed the project: KiCad is
    # still running, so let the user open it there (a second KiCad would be confusing).
    for _ in range(10):
        if not parent_alive(kicad_pid):
            break
        time.sleep(0.5)
    kept = f"\n{old}'s files are in {backup.name}/." if backup.exists() else ""
    if not parent_alive(kicad_pid):
        start_kicad(program, project)
        dialogs.message(finish_message(notes, f"{src.name} is imported as {name}.{kept}",
                                       "KiCad is opening it."))
    elif dialogs.ask(finish_message(notes, f"{src.name} is imported as {name}.{kept}",
                                    f"Open it from KiCad (File → Open Project…):\n{project}"),
                     "Open in a new KiCad window", "Close"):
        start_kicad(program, project)
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
        program = kicad_program()  # now, while the KiCad that started us is running
        project = open_project()
        actions = ([(INTO_PROJECT[0], INTO_PROJECT[1].format(name=project.stem), *INTO_PROJECT[2:])]
                   if project else []) + ACTIONS
        choice = dialogs.menu("Altium to KiCad. What do you want to do?",
                              [(a[0], a[1]) for a in actions])
        if choice is None:
            return 0
        _, _, patterns, title = actions[choice]
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
        if into:
            return import_into(cli, src, project, program)
        return convert_to_folder(cli, src, settings)
    finally:
        try:
            held.unlink()
        except OSError:
            pass


if __name__ == "__main__":
    sys.exit(main())
