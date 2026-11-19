# pcbferry app

pcbferry in Rust: a desktop app (`pcbferry-app`) and a command line (`pcbferry`), for Linux,
macOS and Windows, converting Altium → KiCad and KiCad → Altium.

| Crate | |
|---|---|
| `crates/core` (pcbferry-core) | The conversions. `a2k`: Altium → KiCad, a port of the [`pcbferry`](../pcbferry) script (the same kicad-cli steps, files and `import-reports/summary.json`). `rules`: the custom-rules translator, a port of [`pcbferry_dru.py`](../pcbferry_dru.py). `engine`: finding, downloading (the release's `engine.json`, SHA-256 checked) and unpacking the engine. `k2a`: KiCad → Altium with [kicad2altium](../k2a/README.md) and its read-back checks. |
| `crates/cli` (pcbferry) | The script's options, plus KiCad inputs, `--install-engine` and `--custom-rules BOARD.PcbDoc`. |
| `crates/gui` (pcbferry-app) | egui/eframe with native file dialogs (rfd). `pcbferry-app DESIGN` starts with that design chosen; `--convert` also starts converting it. |

Altium → KiCad needs the engine: pcbferry's patched kicad-cli, built for each platform with
every release. The app downloads it on first use into the folder the KiCad plugin uses
(`~/.local/share/pcbferry`, `~/Library/Application Support/pcbferry`,
`%LOCALAPPDATA%\pcbferry`), so the two share it. `KICAD_CLI` points both at another
kicad-cli (one built with the [patches](../PATCHES.md)); `PCBFERRY_ENGINE_DIR` at an unpacked
engine. KiCad → Altium needs no engine, except to check the result.

## Build and test

```bash
cd app
cargo build --release          # target/release/pcbferry-app and target/release/pcbferry
cargo test --release
```

On Linux the app needs nothing beyond a desktop (X11 or Wayland, OpenGL); it opens the
desktop's own file dialogs through the XDG portal.

With an engine archive, `PCBFERRY_TEST_ENGINE_ARCHIVE=…/pcbferry-engine-linux-x86_64.tar.xz
cargo test --release --test engine` unpacks it with the app's own code and runs its kicad-cli.
`tests/script_parity.py` runs the script and the Rust converter on Altium projects with one
engine and checks that they run the same commands and write the same files, summaries and
custom rules (CI does this for every change). kicad-cli's ERC and DRC counts vary by a few
percent between runs of the same command, so those are printed, not compared.
