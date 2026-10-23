"""Native dialogs without a GUI toolkit: KiCad's plugin Python may have none.

Linux: kdialog (KDE) or zenity, else tkinter. macOS: osascript. Windows: PowerShell.
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
from pathlib import Path

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


def choose_dir(prompt: str, start: str) -> str | None:
    if sys.platform == "win32":
        r = _ps("$d = New-Object System.Windows.Forms.FolderBrowserDialog; "
                f"$d.Description = {_ps_str(prompt)}; $d.SelectedPath = {_ps_str(start)}; "
                "if ($d.ShowDialog() -eq 'OK') { [Console]::Out.Write($d.SelectedPath) }")
        return r.stdout.strip() or None
    if sys.platform == "darwin":
        r = _run(["osascript", "-e", f"POSIX path of (choose folder with prompt {_as_str(prompt)} "
                  f"default location (POSIX file {_as_str(start)}))"])
        return r.stdout.strip() or None
    tool = _linux_tool()
    if tool == "kdialog":
        r = _run(["kdialog", "--title", prompt, "--getexistingdirectory", start])
        return r.stdout.strip() or None
    if tool == "zenity":
        r = _run(["zenity", "--file-selection", "--directory", "--title", prompt,
                  f"--filename={start}/"])
        return r.stdout.strip() or None
    from tkinter import filedialog
    root = _tk()
    path = filedialog.askdirectory(parent=root, title=prompt, initialdir=start)
    root.destroy()
    return path or None


def dark_mode() -> bool:
    """Best guess at whether the desktop uses a dark theme."""
    try:
        if sys.platform == "win32":
            import winreg
            key = winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                                 r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
            return winreg.QueryValueEx(key, "AppsUseLightTheme")[0] == 0
        if sys.platform == "darwin":
            return "Dark" in _run(["defaults", "read", "-g", "AppleInterfaceStyle"]).stdout
        kde = Path.home() / ".config" / "kdeglobals"
        if "KDE" in os.environ.get("XDG_CURRENT_DESKTOP", "") and kde.is_file():
            section = ""
            for line in kde.read_text(errors="replace").splitlines():
                if line.startswith("["):
                    section = line.strip()
                elif section == "[Colors:Window]" and line.startswith("BackgroundNormal="):
                    r, g, b = (int(v) for v in line.split("=", 1)[1].split(",")[:3])
                    return r * 0.299 + g * 0.587 + b * 0.114 < 128
        if shutil.which("gsettings"):
            out = _run(["gsettings", "get", "org.gnome.desktop.interface", "color-scheme"]).stdout
            return "dark" in out
    except Exception:  # noqa: BLE001 - a guess; light is fine
        pass
    return False


def _tk_menu(heading: str, prompt: str, items: list[tuple[str, str]]) -> int | None:
    """The menu as a window of cards: title + one line each, keyboard and mouse."""
    import tkinter
    from tkinter import font as tkfont

    dark = dark_mode()
    c = ({"bg": "#202326", "card": "#2a2e32", "hover": "#33393f", "sel": "#1d4f6e",
          "edge": "#3d4349", "accent": "#3daee9", "text": "#eff0f1", "dim": "#a8b0b7"}
         if dark else
         {"bg": "#f4f5f7", "card": "#ffffff", "hover": "#eef3f8", "sel": "#dcebf7",
          "edge": "#d5d9de", "accent": "#0b74b9", "text": "#1c1f22", "dim": "#5c6670"})
    root = tkinter.Tk()
    root.title(TITLE)
    root.configure(bg=c["bg"])
    root.resizable(False, False)
    base = tkfont.nametofont("TkDefaultFont").actual()["family"]
    f_head = (base, 15, "bold")
    f_title = (base, 11, "bold")
    f_text = (base, 10)

    picked: list[int] = []
    current = [0]
    cards = []

    def choose(i: int) -> None:
        picked.append(i)
        root.destroy()

    def paint() -> None:
        for i, (card, bar, labels) in enumerate(cards):
            colour = c["sel"] if i == current[0] else c["card"]
            card.configure(bg=colour, highlightbackground=c["accent"] if i == current[0] else c["edge"])
            bar.configure(bg=c["accent"] if i == current[0] else colour)
            for label in labels:
                label.configure(bg=colour)

    head = tkinter.Frame(root, bg=c["bg"])
    head.pack(fill="x", padx=24, pady=(20, 4))
    tkinter.Label(head, text=heading, font=f_head, fg=c["text"], bg=c["bg"]).pack(anchor="w")
    tkinter.Label(head, text=prompt, font=f_text, fg=c["dim"], bg=c["bg"]).pack(anchor="w", pady=(2, 0))

    body = tkinter.Frame(root, bg=c["bg"])
    body.pack(fill="both", padx=24, pady=(12, 8))
    for i, (title, text) in enumerate(items):
        card = tkinter.Frame(body, bg=c["card"], highlightthickness=1, cursor="hand2")
        card.pack(fill="x", pady=5)
        bar = tkinter.Frame(card, width=4, bg=c["card"])
        bar.pack(side="left", fill="y")
        inner = tkinter.Frame(card, bg=c["card"])
        inner.pack(side="left", fill="both", expand=True, padx=14, pady=10)
        t = tkinter.Label(inner, text=title, font=f_title, fg=c["text"], bg=c["card"], anchor="w")
        t.pack(fill="x")
        d = tkinter.Label(inner, text=text, font=f_text, fg=c["dim"], bg=c["card"], anchor="w",
                          justify="left", wraplength=470)
        d.pack(fill="x", pady=(2, 0))
        cards.append((card, bar, (card, inner, t, d)))
        for w in (card, bar, inner, t, d):
            w.bind("<Button-1>", lambda _e, i=i: choose(i))
            w.bind("<Enter>", lambda _e, i=i: (current.__setitem__(0, i), paint()))

    foot = tkinter.Frame(root, bg=c["bg"])
    foot.pack(fill="x", padx=24, pady=(4, 18))
    tkinter.Label(foot, text="↑ ↓ to pick · Enter to continue · Esc to cancel", font=(base, 9),
                  fg=c["dim"], bg=c["bg"]).pack(side="left")
    tkinter.Button(foot, text="Cancel", command=root.destroy, relief="flat", padx=14, pady=4,
                   bg=c["card"], fg=c["text"], activebackground=c["hover"],
                   activeforeground=c["text"], highlightthickness=0, bd=0).pack(side="right")

    root.bind("<Up>", lambda _e: (current.__setitem__(0, max(0, current[0] - 1)), paint()))
    root.bind("<Down>", lambda _e: (current.__setitem__(0, min(len(items) - 1, current[0] + 1)), paint()))
    root.bind("<Return>", lambda _e: choose(current[0]))
    root.bind("<Escape>", lambda _e: root.destroy())
    paint()
    root.update_idletasks()
    w, h = max(root.winfo_reqwidth(), 560), root.winfo_reqheight()
    root.geometry(f"{w}x{h}+{(root.winfo_screenwidth() - w) // 2}+{(root.winfo_screenheight() - h) // 3}")
    root.attributes("-topmost", True)
    root.focus_force()
    root.mainloop()
    return picked[0] if picked else None


def menu(prompt: str, entries: list[tuple[str, str]], heading: str = "pcbferry") -> int | None:
    """Index of the entry (title, description) picked, or None if cancelled."""
    try:
        return _tk_menu(heading, prompt, entries)
    except Exception:  # noqa: BLE001 - no tkinter or no display: the plain menus below
        pass
    items = [title for title, _ in entries]
    if sys.platform == "win32":
        listed = ", ".join(_ps_str(i) for i in items)
        r = _ps("$f = New-Object System.Windows.Forms.Form; $f.Text = 'pcbferry'; "
                "$f.Width = 460; $f.Height = 260; $f.StartPosition = 'CenterScreen'; "
                "$l = New-Object System.Windows.Forms.Label; $l.Dock = 'Top'; $l.Height = 30; "
                f"$l.Text = {_ps_str(prompt)}; "
                "$b = New-Object System.Windows.Forms.ListBox; $b.Dock = 'Fill'; "
                f"[void]$b.Items.AddRange(@({listed})); $b.SelectedIndex = 0; "
                "$ok = New-Object System.Windows.Forms.Button; $ok.Text = 'Continue'; "
                "$ok.Dock = 'Bottom'; $ok.DialogResult = 'OK'; $f.AcceptButton = $ok; "
                "$f.Controls.Add($b); $f.Controls.Add($l); $f.Controls.Add($ok); "
                "$b.Add_DoubleClick({ $f.DialogResult = 'OK' }); "
                "if ($f.ShowDialog() -eq 'OK') { [Console]::Out.Write($b.SelectedIndex) }")
        return int(r.stdout) if r.stdout.strip().isdigit() else None
    if sys.platform == "darwin":
        listed = ", ".join(_as_str(i) for i in items)
        r = _run(["osascript", "-e", f"choose from list {{{listed}}} with title {_as_str(TITLE)} "
                  f"with prompt {_as_str(prompt)} default items {{{_as_str(items[0])}}}"])
        picked = r.stdout.strip()
        return items.index(picked) if picked in items else None
    tool = _linux_tool()
    if tool == "kdialog":
        args = [x for i, item in enumerate(items) for x in (str(i), item)]
        r = _run(["kdialog", "--title", TITLE, "--menu", prompt, *args])
        return int(r.stdout) if r.stdout.strip().isdigit() else None
    if tool == "zenity":
        r = _run(["zenity", "--list", "--title", TITLE, "--text", prompt, "--hide-header",
                  "--column", "n", "--column", "action", "--hide-column", "1",
                  "--print-column", "1", "--width", "460", "--height", "260",
                  *[x for i, item in enumerate(items) for x in (str(i), item)]])
        return int(r.stdout) if r.stdout.strip().isdigit() else None
    import tkinter
    root = tkinter.Tk()
    root.title(TITLE)
    picked: list[int] = []
    tkinter.Label(root, text=prompt, padx=12, pady=8).pack(anchor="w")
    box = tkinter.Listbox(root, height=len(items), width=50)
    for item in items:
        box.insert("end", item)
    box.selection_set(0)
    box.pack(padx=12)

    def go(*_):
        picked.extend(box.curselection())
        root.destroy()
    box.bind("<Double-Button-1>", go)
    tkinter.Button(root, text="Continue", command=go).pack(pady=8)
    root.mainloop()
    return picked[0] if picked else None


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
                     "--width", "460", "--title", TITLE, "--text", self.text],
                    stdin=subprocess.PIPE)
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


class Notice:
    """A message with one button that stays up while we wait for something; `closed`
    turns true if the user clicks the button (or closes the window)."""

    def __init__(self, text: str, button: str):
        self.proc = None
        try:
            if sys.platform == "win32":
                self.proc = subprocess.Popen(
                    ["powershell", "-NoProfile", "-NonInteractive", "-STA", "-Command",
                     "Add-Type -AssemblyName System.Windows.Forms; "
                     "$f = New-Object System.Windows.Forms.Form; $f.Text = 'pcbferry'; "
                     "$f.Width = 460; $f.Height = 200; $f.StartPosition = 'CenterScreen'; "
                     "$f.TopMost = $true; "
                     "$l = New-Object System.Windows.Forms.Label; $l.Dock = 'Fill'; "
                     f"$l.Padding = 12; $l.Text = {_ps_str(text)}; "
                     "$b = New-Object System.Windows.Forms.Button; $b.Dock = 'Bottom'; "
                     f"$b.Text = {_ps_str(button)}; $b.Add_Click({{ $f.Close() }}); "
                     "$f.Controls.Add($l); $f.Controls.Add($b); "
                     "[System.Windows.Forms.Application]::Run($f)"],
                    creationflags=NO_WINDOW)
            elif sys.platform == "darwin":
                self.proc = subprocess.Popen(
                    ["osascript", "-e", f"display dialog {_as_str(text)} with title "
                     f"{_as_str(TITLE)} buttons {{{_as_str(button)}}}"])
            elif (tool := _linux_tool()) == "zenity":
                self.proc = subprocess.Popen(
                    ["zenity", "--info", "--title", TITLE, "--no-markup", "--width", "460",
                     "--text", text, "--ok-label", button])
            elif tool == "kdialog":
                self.proc = subprocess.Popen(
                    ["kdialog", "--title", TITLE, "--ok-label", button, "--msgbox", text])
        except OSError:
            self.proc = None

    @property
    def closed(self) -> bool:
        return self.proc is not None and self.proc.poll() is not None

    def close(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()


def open_path(path: str) -> None:
    """Open with the system's default program (a .kicad_pro opens in KiCad)."""
    if sys.platform == "win32":
        os.startfile(path)  # noqa: S606 - the user's own converted project
    else:
        subprocess.Popen(["open" if sys.platform == "darwin" else "xdg-open", path],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
