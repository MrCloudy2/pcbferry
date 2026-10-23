"""Native dialogs without a GUI toolkit: KiCad's plugin Python may have none.

Linux: kdialog (KDE) or zenity, else tkinter. macOS: osascript. Windows: PowerShell.
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys

NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)
TITLE = "pcbferry"


def _run(cmd: list[str]) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, text=True, creationflags=NO_WINDOW)


def _ps(script: str) -> subprocess.CompletedProcess:
    return _run(["powershell", "-NoProfile", "-NonInteractive", "-STA", "-Command",
                 "Add-Type -AssemblyName System.Windows.Forms; " + script])


def _ps_str(text: str) -> str:
    return "'" + text.replace("'", "''") + "'"


def _as_str(text: str) -> str:
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def _linux_tool() -> str | None:
    kde = "KDE" in os.environ.get("XDG_CURRENT_DESKTOP", "")
    order = ("kdialog", "zenity") if kde else ("zenity", "kdialog")
    return next((t for t in order if shutil.which(t)), None)


def _tk():
    import tkinter
    root = tkinter.Tk()
    root.withdraw()
    return root


def choose_file(prompt: str, start: str, patterns: list[str]) -> str | None:
    """A file to open; patterns like '*.PrjPcb'."""
    if sys.platform == "win32":
        flt = "Altium files|" + ";".join(patterns) + "|All files|*.*"
        r = _ps("$d = New-Object System.Windows.Forms.OpenFileDialog; "
                f"$d.Title = {_ps_str(prompt)}; $d.Filter = {_ps_str(flt)}; "
                f"$d.InitialDirectory = {_ps_str(start)}; "
                "if ($d.ShowDialog() -eq 'OK') { [Console]::Out.Write($d.FileName) }")
        return r.stdout.strip() or None
    if sys.platform == "darwin":
        r = _run(["osascript", "-e",
                  f"POSIX path of (choose file with prompt {_as_str(prompt)} "
                  f"default location (POSIX file {_as_str(start)}))"])
        return r.stdout.strip() or None
    tool = _linux_tool()
    if tool == "kdialog":
        r = _run(["kdialog", "--title", TITLE, "--getopenfilename", start,
                  " ".join(patterns) + " |Altium files"])
        return r.stdout.strip() or None
    if tool == "zenity":
        r = _run(["zenity", "--file-selection", "--title", prompt, f"--filename={start}/",
                  "--file-filter=Altium files | " + " ".join(patterns + [p.lower() for p in patterns])])
        return r.stdout.strip() or None
    from tkinter import filedialog
    root = _tk()
    path = filedialog.askopenfilename(parent=root, title=prompt, initialdir=start,
                                      filetypes=[("Altium files", " ".join(patterns))])
    root.destroy()
    return path or None


def message(text: str, error: bool = False) -> None:
    if sys.platform == "win32":
        icon = "Error" if error else "Information"
        _ps(f"[System.Windows.Forms.MessageBox]::Show({_ps_str(text)}, {_ps_str(TITLE)}, 'OK', '{icon}')")
    elif sys.platform == "darwin":
        _run(["osascript", "-e", f"display dialog {_as_str(text)} with title {_as_str(TITLE)} "
              f"buttons {{\"OK\"}} default button 1" + (" with icon stop" if error else "")])
    elif (tool := _linux_tool()) == "kdialog":
        _run(["kdialog", "--title", TITLE, "--error" if error else "--msgbox", text])
    elif tool == "zenity":
        _run(["zenity", "--error" if error else "--info", "--title", TITLE, "--no-markup", "--text", text])
    else:
        from tkinter import messagebox
        root = _tk()
        (messagebox.showerror if error else messagebox.showinfo)(TITLE, text, parent=root)
        root.destroy()


def ask(text: str, yes: str, no: str) -> bool:
    if sys.platform == "win32":
        r = _ps(f"[Console]::Out.Write([System.Windows.Forms.MessageBox]::Show("
                f"{_ps_str(text)}, {_ps_str(TITLE)}, 'YesNo', 'Question'))")
        return r.stdout.strip() == "Yes"
    if sys.platform == "darwin":
        r = _run(["osascript", "-e", f"button returned of (display dialog {_as_str(text)} "
                  f"with title {_as_str(TITLE)} buttons {{{_as_str(no)}, {_as_str(yes)}}} "
                  "default button 2)"])
        return r.stdout.strip() == yes
    tool = _linux_tool()
    if tool == "kdialog":
        return _run(["kdialog", "--title", TITLE, "--yes-label", yes, "--no-label", no,
                     "--yesno", text]).returncode == 0
    if tool == "zenity":
        return _run(["zenity", "--question", "--title", TITLE, "--no-markup", "--text", text,
                     "--ok-label", yes, "--cancel-label", no]).returncode == 0
    from tkinter import messagebox
    root = _tk()
    answer = messagebox.askyesno(TITLE, text, parent=root)
    root.destroy()
    return answer


class Busy:
    """A 'working…' indicator while a long step runs (best effort; never blocks)."""

    def __init__(self, text: str):
        self.text, self.proc = text, None

    def __enter__(self):
        try:
            if sys.platform == "win32":
                self.proc = subprocess.Popen(
                    ["powershell", "-NoProfile", "-NonInteractive", "-STA", "-Command",
                     "Add-Type -AssemblyName System.Windows.Forms; "
                     "$f = New-Object System.Windows.Forms.Form; $f.Text = 'pcbferry'; "
                     "$f.Width = 420; $f.Height = 120; $f.StartPosition = 'CenterScreen'; "
                     "$f.TopMost = $true; $f.ControlBox = $false; "
                     "$l = New-Object System.Windows.Forms.Label; $l.Dock = 'Fill'; "
                     f"$l.TextAlign = 'MiddleCenter'; $l.Text = {_ps_str(self.text)}; "
                     "$f.Controls.Add($l); [System.Windows.Forms.Application]::Run($f)"],
                    creationflags=NO_WINDOW)
            elif sys.platform == "darwin":
                _run(["osascript", "-e", f"display notification {_as_str(self.text)} "
                      f"with title {_as_str(TITLE)}"])
            elif shutil.which("zenity"):
                self.proc = subprocess.Popen(
                    ["zenity", "--progress", "--pulsate", "--no-cancel", "--auto-close",
                     "--title", TITLE, "--text", self.text], stdin=subprocess.PIPE)
            elif shutil.which("kdialog"):
                _run(["kdialog", "--title", TITLE, "--passivepopup", self.text, "8"])
        except OSError:
            self.proc = None
        return self

    def __exit__(self, *exc):
        if self.proc:
            if self.proc.stdin:
                try:
                    self.proc.stdin.write(b"100\n")
                    self.proc.stdin.close()
                except OSError:
                    pass
            try:
                self.proc.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        return False


def open_path(path: str) -> None:
    """Open with the system's default program (a .kicad_pro opens in KiCad)."""
    if sys.platform == "win32":
        os.startfile(path)  # noqa: S606 - the user's own converted project
    else:
        subprocess.Popen(["open" if sys.platform == "darwin" else "xdg-open", path],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
