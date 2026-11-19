#!/usr/bin/env python3
"""The Rust converter (app/) against the pcbferry script, on the same Altium projects with the
same engine.

    python3 -I app/tests/script_parity.py --engine KICAD_CLI --rust app/target/release/pcbferry \
        --out OUT_DIR PROJECT.PrjPcb...

For each project: both convert it (--check --fill-zones); they must run the same kicad-cli
commands, write the same files and the same summary.json. kicad-cli itself is not
deterministic (ERC and DRC counts move by a few percent between runs of the same command), so
violation counts are printed, not compared. For each PcbDoc in the project's folder, both
translators' custom rules must be byte-identical.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]


def commands(log: str, root: str) -> list[str]:
    out = []
    for line in log.splitlines():
        m = re.match(r"(?:pcbferry: )?\$ \S+ (.*)", line.strip())
        if m:
            out.append(m.group(1).replace(root, "OUT"))
    return out


def norm(value, root):
    if isinstance(value, dict):
        return {k: norm(v, root) for k, v in value.items() if k not in ("tool", "violations")}
    if isinstance(value, list):
        return [norm(v, root) for v in value]
    if isinstance(value, str):
        return value.replace(root, "OUT")
    return value


def files(root: Path) -> list[str]:
    return sorted(str(p.relative_to(root)) for p in root.rglob("*") if p.is_file())


def python_rules(board: Path) -> str:
    code = ("import sys; sys.path[:0] = [sys.argv[1], sys.argv[1] + '/plugin/src']\n"
            "import pcbferry_dru\n"
            "text, results = pcbferry_dru.custom_rules(sys.argv[2])\n"
            "sys.stdout.write(text)\n"
            "for r in results: print(f\"#> {r['name']} | {r['status']} | {r.get('reason', '')}\")")
    return subprocess.run([sys.executable, "-I", "-c", code, str(REPO), str(board)],
                          capture_output=True, text=True, check=True).stdout


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--engine", required=True, help="the kicad-cli both use")
    ap.add_argument("--rust", required=True, help="the Rust pcbferry binary")
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("projects", nargs="+", type=Path)
    args = ap.parse_args()
    env = dict(os.environ, KICAD_CLI=args.engine, PYTHONPATH=str(REPO / "plugin" / "src"))
    failed = []
    for prj in args.projects:
        name = prj.stem
        py_out, rs_out = args.out / f"{name}-script", args.out / f"{name}-rust"
        # The script finds olefile (for the custom rules) next to the plugin
        py = subprocess.run([sys.executable, str(REPO / "pcbferry"), "--check", "--fill-zones",
                             str(prj), str(py_out)], capture_output=True, text=True, env=env)
        rs = subprocess.run([args.rust, "--check", "--fill-zones", str(prj), str(rs_out)],
                            capture_output=True, text=True, env=env)
        problems = []
        if py.returncode != rs.returncode:
            problems.append(f"exit codes: script {py.returncode}, rust {rs.returncode}")
        if commands(py.stderr, str(py_out)) != commands(rs.stderr, str(rs_out)):
            problems.append(f"commands differ:\n  script {commands(py.stderr, str(py_out))}\n"
                            f"  rust   {commands(rs.stderr, str(rs_out))}")
        if files(py_out) != files(rs_out):
            problems.append(f"files differ: only script {set(files(py_out)) - set(files(rs_out))}, "
                            f"only rust {set(files(rs_out)) - set(files(py_out))}")
        a = json.loads((py_out / "import-reports" / "summary.json").read_text())
        b = json.loads((rs_out / "import-reports" / "summary.json").read_text())
        if norm(a, str(py_out)) != norm(b, str(rs_out)):
            for x, y in zip(norm(a, str(py_out))["steps"], norm(b, str(rs_out))["steps"]):
                if x != y:
                    problems.append(f"summary step differs:\n  script {json.dumps(x)[:500]}\n"
                                    f"  rust   {json.dumps(y)[:500]}")
            if len(a["steps"]) != len(b["steps"]) or a["ok"] != b["ok"]:
                problems.append("summary differs")
        counts = ", ".join(f"{x['step']} {x['violations']}/{y.get('violations')}"
                           for x, y in zip(a["steps"], b["steps"]) if "violations" in x)
        boards = sorted(p for p in prj.parent.rglob("*") if p.suffix.lower() == ".pcbdoc")
        for board in boards:
            rust = subprocess.run([args.rust, "--custom-rules", str(board)], capture_output=True,
                                  text=True, check=True).stdout
            if rust != python_rules(board):
                problems.append(f"custom rules differ for {board.name}")
        print(f"{name}: {'same' if not problems else 'DIFFERENT'} "
              f"({len(boards)} board(s) of rules; violations script/rust: {counts})")
        for p in problems:
            print("  " + p)
        if problems:
            failed.append(name)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
