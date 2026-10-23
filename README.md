# pcbferry

[![ci](https://github.com/MrCloudy2/pcbferry/actions/workflows/ci.yml/badge.svg)](https://github.com/MrCloudy2/pcbferry/actions/workflows/ci.yml)

**Altium Designer → KiCad 10 converter.** One command turns an Altium project
(`.PrjPcb`) into a KiCad 10 project: schematics, board, libraries and design
rules, with every symbol and footprint linked to a library.

It is KiCad's own Altium importer with [26 fixes](PATCHES.md), packaged as a
container, so it runs the same on Linux, macOS and Windows. The result is a plain
KiCad 10 project.

**How close is it?** On 21 Altium projects, checked against Altium's own output:
every pin connection, part, footprint, value and placement matches; every drill
hole matches; outer copper differs by 0.15 % of its area and inner copper by 2.7 %;
board connectivity matches Altium's own DRC on 8 of 9 boards checked. Details, per project, and what still differs:
[PARITY.md](PARITY.md).

Not related to the archived
[altium2kicad](https://github.com/thesourcerer8/altium2kicad) Perl scripts.

## Quick start

1. Install [Docker Desktop](https://www.docker.com/products/docker-desktop/)
   (Windows, macOS) or Docker / Podman (Linux), and Python 3.
2. Download the launcher:
   [`pcbferry-container`](https://raw.githubusercontent.com/MrCloudy2/pcbferry/main/pcbferry-container).
3. Convert:

   ```bash
   python3 pcbferry-container --check --fill-zones path/to/Board.PrjPcb out
   ```

   On Windows: `py pcbferry-container --check --fill-zones C:\path\to\Board.PrjPcb out`

4. Open `out/Board.kicad_pro` in KiCad 10.

The first run downloads the image (about 1.3 GB). The launcher mounts the
project read-only, finds libraries the project keeps in other folders, and writes
the output as your user. On Apple Silicon Docker runs the image through
emulation, which is slower but works.

Without the launcher (from the project folder):

```bash
docker run --rm -v "$PWD:/work" -w /work ghcr.io/mrcloudy2/pcbferry Board.PrjPcb out
```

## KiCad plugin (preview, Linux)

No Docker needed: install `pcbferry-kicad-plugin-VERSION.zip` from the
[latest release](https://github.com/MrCloudy2/pcbferry/releases/latest) in KiCad 10 with
**Plugin and Content Manager → Install from File…**, and answer *Yes* when KiCad
asks to enable its API. The PCB and schematic editors then have a pcbferry button with
two ways to convert:

- **Here, in place of the open project.** Make an empty KiCad project where the design
  should go, open it, click the button and pick the `.PrjPcb`. When pcbferry asks, quit
  KiCad: it moves the empty project into `NAME-before-pcbferry/` and starts KiCad again
  with the converted project, named as in Altium.
- **Into a new folder.** Pick a project, a single `.SchDoc` or a single `.PcbDoc`, then a
  folder: the result goes in `NAME-kicad/` inside it.

Install it from a recent KiCad 10 (10.0.7 works): KiCad 10.0.0's package manager drops the plugin
script's executable bit, and the button then does nothing. KiCads that share the same
settings, such as a distribution package and an AppImage, share the installed plugin.

The first use downloads the converter engine (about 150 MB) from the same release. If
nothing happens on a click, `~/.local/share/pcbferry/plugin.log` says why.

The engine is the same patched `kicad-cli` as in the container, so results are
identical. Windows and macOS engines are next; until then use the container there.

## Options

| Option | |
|---|---|
| `--check` | afterwards run ERC and DRC on the result and report unlinked symbols or footprints |
| `--fill-zones` | fill the internal planes (Altium stores no plane copper; without this, fill zones in KiCad before plotting) |
| `--footprint-slash CHAR` | replace `/` in footprint names with `CHAR`, e.g. `_`: a library file can't be named with `/`, so such footprints otherwise stay unlinked |
| `--remove-unused-lands` | drop unconnected pad and via lands on all inner layers: Gerbers closer to Altium's on plane-heavy boards |
| `--no-custom-rules` | don't write the scoped Altium design rules to `NAME.kicad_dru` |
| `--board FILE` | which PcbDoc, when a project has several (default: the first) |
| `--name NAME` | project name (default: the input's) |
| `--force` | write into a non-empty output folder |

Input can be a `.PrjPcb` (recommended), a single `.SchDoc` or a `.PcbDoc`.

## What you get

- `NAME.kicad_pro`, `NAME.kicad_sch` (+ one file per sheet), `NAME.kicad_pcb`
- `NAME.kicad_dru`: Altium's scoped design rules as KiCad custom rules
- libraries: copies of the project's SchLib / PcbLib / IntLib, plus `NAME-import-syms`
  and `NAME-import-fps` for parts with no readable library (DbLib, Altium 365) or
  placed differently from their library; all registered in the project
- `import-reports/`: import warnings, a `summary.json` of every step, and with
  `--check` the ERC/DRC reports

## Before you manufacture

- Fill zones (`--fill-zones`, or B in pcbnew): planes are empty otherwise.
- Run DRC in KiCad. Altium and KiCad check different things with different
  defaults, so the first DRC is a to-do list rather than a verdict on the import.
- Text in fonts you don't have installed is drawn with a substitute; KiCad's
  stroke font is narrower than Altium's.
- Panels made of embedded boards are not converted; convert the board itself.

More in [PARITY.md](PARITY.md#what-still-differs).

## Build it yourself

```bash
./build.sh
```

Builds the image from `patches/` (about 40 minutes with 4 cores, a few GB of RAM),
smoke-tests it and tags it `pcbferry:latest`; use it with
`PCBFERRY_IMAGE=pcbferry:latest`. `JOBS=8 ./build.sh` uses more cores. The base is
Arch Linux pinned by digest and a dated package snapshot; only `kicad-cli` and the
schematic and board libraries are built. The wrapper inside the image (`pcbferry`)
is a standalone Python script that works with any `kicad-cli` built with the
patches (point `KICAD_CLI` at it).

CI runs on every pull request: it applies the patches to the pinned KiCad tag,
builds the image and converts two public Altium projects, failing on any failed
step or unlinked symbol or footprint. A `v*` tag publishes the image and a release
with the source.

## Relation to KiCad and licence

pcbferry is built from KiCad's source with these patches; it is not an official
KiCad project, and the patches are not being submitted upstream for now. They
were written with AI assistance and checked against the measurements in
[PARITY.md](PARITY.md).

KiCad is GPL-3.0-or-later and so are the patches and scripts here
([LICENSE](LICENSE)). The image carries its corresponding source (patched KiCad
tree, patches, build configuration) in `/opt/kicad-altium/share/doc/pcbferry/`,
and every release attaches it.
