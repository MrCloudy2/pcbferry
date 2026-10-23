#!/usr/bin/env python3
"""End-to-end check of the plugin package, as KiCad would run it (minus the dialogs).

    plugin/test_plugin.py dist/pcbferry-kicad-plugin-X.zip DIST_DIR PROJECT.PrjPcb [MORE.PrjPcb ...]

Unpacks the package, makes a venv from its requirements.txt (as KiCad does), serves
DIST_DIR over local HTTP so the plugin downloads the engine there instead of from
GitHub, converts each project and checks its summary.
"""
import functools
import http.server
import json
import os
import subprocess
import sys
import tempfile
import threading
import zipfile
from pathlib import Path

package, dist, projects = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3:]
with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    zipfile.ZipFile(package).extractall(tmp)
    plugin = tmp / "plugins"
    subprocess.run([sys.executable, "-m", "venv", tmp / "venv"], check=True)
    py = tmp / "venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    subprocess.run([py, "-m", "pip", "-q", "install", "-r", plugin / "requirements.txt"], check=True)

    server = http.server.ThreadingHTTPServer(
        ("127.0.0.1", 0), functools.partial(http.server.SimpleHTTPRequestHandler, directory=dist))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    manifest = json.loads((plugin / "engine.json").read_text())
    manifest["base_url"] = f"http://127.0.0.1:{server.server_port}/"
    (plugin / "engine.json").write_text(json.dumps(manifest))

    script = """
import json, sys
from pathlib import Path
import engine, import_altium
cli = engine.installed() or engine.install()
failed = 0
for src in map(Path, sys.argv[1:]):
    out = Path("out") / src.stem
    ok, log = import_altium.convert(cli, src, out)
    summary = json.loads((out / "import-reports" / "summary.json").read_text())
    issues = {t: n for st in summary["steps"] for t, n in st.get("library_issues", {}).items() if n}
    print(src.name, "ok" if ok and not issues else f"FAILED {issues}")
    print("\\n".join(log.splitlines()[-3:]))
    failed += not (ok and not issues)
sys.exit(failed)
"""
    env = dict(os.environ, XDG_DATA_HOME=str(tmp / "data"), LOCALAPPDATA=str(tmp / "data"))
    run = subprocess.run([py, "-c", script, *projects], cwd=plugin, env=env)
    server.shutdown()
    sys.exit(run.returncode)
