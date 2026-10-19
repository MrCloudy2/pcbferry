# altium2kicad

Convert Altium Designer projects to KiCad 10 from the command line, with an
Altium importer that is noticeably more faithful than the one in stock KiCad.

This is **KiCad 10.0.7 with the altium-kicad-compat importer patches** (19 so far), built
headless (only `kicad-cli` and the schematic/board kifaces), plus a small wrapper that turns
an Altium `.PrjPcb` into a ready KiCad project folder. The patches are not being
submitted to KiCad for now, so this is the way to use them without building
KiCad.

The output is a plain KiCad 10 project; open it with any KiCad 10.0.x.

## Usage

Build the image (about 40 minutes with the default 4 build jobs, a few GB of
RAM; the image is ~1.3 GB compressed, ~4 GB on disk):

```bash
./build.sh
```

`build.sh` is also how to update: whenever the patches change, run it again.
It syncs `patches/` with `../altium-kicad-compat/kicad-patches` (every
`NNNN-*.patch`: new ones added, changed ones replaced, removed ones deleted),
builds the image with podman or docker, smoke-tests it, and tags it
`altium2kicad:latest` and `altium2kicad:patches-<hash of the patch set>`. Then
commit `patches/`. Options: `--no-sync` builds from the committed `patches/`
(as CI does), `--patches DIR` syncs from another folder, `JOBS=8 ./build.sh`
compiles with more cores. Any change to the patches recompiles KiCad from
scratch.

Without the script: `podman build -t altium2kicad -f Containerfile .`
(or `docker build`).

Convert a project:

```bash
./altium2kicad-container --check path/to/Board.PrjPcb out/Board
```

`altium2kicad-container` mounts the project's folder read-only and the output
folder read-write, and runs as your user so the files are yours. If the
`.PrjPcb` lists documents outside its own folder (`..\Libraries\...`), set
`ALTIUM2KICAD_ROOT` to a folder containing all of them. Or run the image
directly (bash syntax):

```bash
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/work" altium2kicad Board.PrjPcb out
```

Input can be a `.PrjPcb` (recommended), a single `.SchDoc` or a `.PcbDoc`.

For a project the wrapper imports **the board first, then the schematic**, into
the same folder under the same project name. The order matters: the schematic
import takes each symbol's footprint library link from the board part with the
same reference in `NAME.kicad_pcb`. Both imports write their settings to the
shared `NAME.kicad_pro`.

The output folder gets:

| File | From |
|---|---|
| `NAME.kicad_pro` | board and schematic settings, incl. the imported design rules and netclasses |
| `NAME.kicad_pcb` | `kicad-cli pcb import --format altium` of the project's (first) PcbDoc |
| `NAME.kicad_sch` + one `.kicad_sch` per sheet | `kicad-cli sch import` of the whole project |
| `NAME-import-fps.pretty/` | footprints placed on the board that no project library holds as placed |
| `NAME-import-syms.kicad_sym` | symbols with no readable library (DbLib, server parts) or differing from it |
| copies of the project's SchLib / PcbLib / IntLib | the project's own libraries, converted |
| `fp-lib-table`, `sym-lib-table` | register all of the above, so every symbol and footprint is linked |
| `import-reports/pcb-import.json`, `sch-import.json` | kicad-cli's import reports (warnings, layer mapping, counts) |
| `import-reports/summary.json` | every step, its exit code, errors and warnings |
| `import-reports/NAME.net`, `erc.json`, `drc.json` | with `--check`: netlist, ERC and DRC (+ schematic parity) of the result |

A footprint placed on the board in several different forms under one name gets
one library entry per form: `NAME`, `NAME (2)`, `NAME (3)`, ...

The board import finds the project's footprint libraries through the `.PrjPcb`
next to the PcbDoc or one folder up. With `altium2kicad-container` and a lone
`.PcbDoc` as input, set `ALTIUM2KICAD_ROOT` so that folder is mounted too.

Options: `--name` (project name, default the input's), `--board` (which PcbDoc
when a project has several), `--check`, `--force` (write into a non-empty
folder), `--remove-unused-lands` (see "Fabrication output" below). The exit status is non-zero if an import or check fails to run; DRC
violations alone do not fail it.

The wrapper is a standalone Python 3 script. It works with any kicad-cli that
has the patches; point `KICAD_CLI` at it.

### Reading the reports

- With `--check`, `library_issues` in the summary (and the last line of output)
  counts ERC `lib_symbol_issues`/`footprint_link_issues` and DRC
  `lib_footprint_issues`/`lib_footprint_mismatch`. ERC and DRC run with
  `--severity-all`. A clean conversion has none, apart from the limits below.
- `symbols_without_library` counts "No library symbol found" import messages.
  The importer emits them while reading, before it links symbols to the
  libraries it writes, so they are informational; ERC's `lib_symbol_issues` is
  what tells whether symbols ended up linked.
- Schematic parity in `drc.json` lists `net_conflict` for nets Altium never
  named. Altium calls them `NetC1_2`, KiCad `Net-(C1-Pad2)`; it is the same
  connection. "Update PCB from Schematic" renames them.

## What is better than stock KiCad 10

Measured on 21 public Altium projects, against Altium's own compiled netlist
where one was available, otherwise against the independent `altium-monkey`
reader. With the patches, all 21 match 100% on pins, components, footprints,
values and placements. Board copper, dimensions and mapped design rules match
too.

| Patch | Stock KiCad 10.0.7 | With the patch |
|---|---|---|
| `0001` `kicad-cli sch import` | no headless schematic import; every `kicad-cli sch` command fails on `.SchDoc` | imports a `.SchDoc` or a whole `.PrjPcb` (flat designs, variants) |
| `0002` net identifier scope | project's Flat/Global scope ignored; cross-sheet nets split (up to 240 pins per design) | scope read from the `.PrjPcb`; nothing split |
| `0003` harness entries | wires at harness entries left open | connected as `HARNESS.member` |
| `0004` harness bus vs member sub-bus | vector members shorted (every LVDS P/N pair on Kasli) | correct |
| `0005` vector harness members and ports | bits of vector members stop at the sheet | connected |
| `0006` hidden net names, untyped sheet entries | hidden GND pins open; harness doesn't cross the sheet | connected |
| `0007` harness names with spaces, type case | invalid bus groups, types not matched | normalised |
| `0008` net name case | `RX signal` ≠ `RX SIGNAL` (in Altium: one net) | merged as in Altium |
| `0009` ports placed on symbol pins | label lands on the port's other end, net lost | connected |
| `0010` multichannel sheets | only the last channel survives (313 of 783 parts missing on one design) | all channels, per-channel designators |
| `0011` `pcb import` report | always `"warnings": []` | importer warnings in the report |
| `0012` parameters, component kinds, variants | backslashes eaten; `Footprint`/`Description` parameters clobber KiCad fields; graphical parts in BOM and board; most variants lost | kept, mapped, excluded, applied |
| `0013` board: arcs, keepouts, datum dimensions, rules | gaps where very shallow arcs were; one rule area per keepout primitive (8,393 on Kasli); datum dimensions dropped; several board rules ignored | arcs as tracks; merged keepouts (Kasli: 38); datum dimensions; rules applied |
| `0014` `pcb import` saves the project | design rules and netclasses lost (only `.kicad_pcb` saved) | `.kicad_pro` saved with them |
| `0015` linked libraries | CLI imports leave every symbol and footprint unlinked (Kasli: 1,465 ERC + 788 DRC library errors); DbLib/server parts have no library at all | project SchLib/PcbLib/IntLib copied and registered, `-import-syms` / `-import-fps` libraries for the rest: 0 symbol library errors on all 21 projects |
| `0016` footprint variants | most placed footprints differ from their library copy (Kasli: 592 of 786) because of Altium's rounding and several footprints under one name | copies grouped within 50 nm, distinct forms named `NAME (2)`...: 1,282 → 6 mismatches over 21 projects (with 0017) |
| `0017` fabrication output | inner layers of per-layer padstacks get zero-height pads (nothing plotted); inverted TrueType text imported as plain text; Board/Board Shape/V-Cut mechanical layers drawn on Edge.Cuts (milled) | inner pads plotted; knock-out text; those layers on their own user layer |
| `0018` removed via lands, key legends | lands Altium removed ("Remove Unused Pad Shapes") kept on every inner layer; a Comment of just `\` (a key legend) emptied | vias drop unconnected lands as in Altium; `\` kept |
| `0019` `pcb import --remove-unused-lands` | – | opt-in, see "Fabrication output" |

Each patch has a draft upstream issue with the problem, the fix and the
measurements (`kicad-patches/ISSUES.md` in the altium-kicad-compat repository,
which also holds the measuring harness).

## Fabrication output

Checked by plotting each converted board with `kicad-cli` and comparing it, layer by
layer and hole by hole, with the Gerbers and NC drill files Altium itself writes
(19 boards): every drilled hole and slot matches, board outlines match, paste and
signal-layer copper match exactly or within a few mm². Differences that remain:

- **Planes are imported unfilled.** Altium computes planes when it writes output, so
  the PcbDoc has no plane copper. Fill all zones (B in pcbnew) before plotting, or
  the inner planes come out empty.
- **Plane clearances.** Altium draws no pad lands on plane layers and measures the
  clearance from the drilled hole; KiCad keeps the lands and measures from them.
  Filled planes differ from Altium's by about 5–10 % of their area; electrically the
  same board. `--remove-unused-lands` makes the import drop pad and via lands on
  inner layers where nothing connects: planes come out close to Altium's (FPGA3:
  ~720 → ~100 mm² differing per plane), but lands Altium keeps on inner *signal*
  layers go too, and through-hole footprints then differ from the project's own
  library copies, so they are stored in the `-import-fps` library instead. Default
  off. It pays off only where planes outweigh inner signal layers (copper differing
  from Altium's, default → with the option: FPGA3, 6 planes: 4,959 → 1,477 mm²;
  US-Plus, 2 planes: 1,489 → 867; Kasli, 1 plane and 3 inner signal layers:
  1,777 → 3,240; boards without planes get worse, e.g. MISKO3 12.6 → 758).
- Text: KiCad's stroke font is narrower than Altium's, and TrueType text needs the
  font installed (otherwise a substitute is used).
- Footprint "board cutout" regions that overlap the board edge are added to
  Edge.Cuts; Altium's outline output leaves them out.
- Altium panels built from embedded boards are not supported.

### Known limitations

- Altium rules KiCad can only express as custom rules (scoped clearances,
  matched lengths, routing layers) are not written to a `.kicad_dru` yet.
- Board DRC on an imported board reports many violations: Altium and KiCad
  check different things, with different defaults. Treat the first DRC as a
  to-do list, not a measure of the import.
- On some boards (e.g. Kasli) DRC reports unconnected GND tracks and vias,
  even after refilling zones. Check plane and zone connectivity of multi-layer
  boards in pcbnew.
- One board per project per run (`--board` picks it).
- Footprint names containing `/` (e.g. `CAP 0805/2012`) can't be written as
  library files, so those footprints stay unlinked (85 across the 21 test
  projects).
- 6 footprints across the 21 test projects still differ from their library
  copy: keepout zones on rotated parts (KiCad side).
- `kicad-cli version` reports `10.0.7-dirty-altium2kicad`: "dirty" because the
  patches are applied on top of the release tag.

## Relation to upstream KiCad

KiCad accepts changes issue-first, and AI-assisted code needs a human owner
who stands behind it. None of these patches has been submitted yet. Once one
is merged upstream it is dropped from `patches/`. When they all are, this
project is obsolete: use stock `kicad-cli`. Moving to a newer KiCad tag means
updating `KICAD_TAG`/`KICAD_COMMIT` in the Containerfile and refreshing the
patches.

## Licensing

KiCad is GPL-3.0-or-later. The patches and the wrapper scripts here are
distributed under the same terms (see `LICENSE`).

The image carries its corresponding source in
`/opt/kicad-altium/share/doc/altium2kicad/`: the patches, the exact patched
KiCad tree it was built from (`kicad-10.0.7-altium2kicad-source.tar.xz`), the
build configuration (`SOURCE.txt`) and KiCad's license files. To get them out:

```bash
docker create --name a2k-src altium2kicad
```

```bash
docker cp a2k-src:/opt/kicad-altium/share/doc/altium2kicad ./altium2kicad-source
```

```bash
docker rm a2k-src
```

The release workflow also attaches them to every tagged release. Anyone
redistributing the image must keep them available as GPL-3.0 §6 requires.

## Build details

- Base: Arch Linux, pinned by image digest. Packages come from the dated Arch
  Linux Archive snapshot (`ARCH_SNAPSHOT`), so a rebuild resolves the same
  library versions. `--build-arg ARCH_SNAPSHOT=` uses the live mirrors.
- KiCad is cloned shallowly at the tag, the commit hash is checked, and the
  patches are applied with `git apply`: every `patches/NNNN-*.patch`, in sorted
  order, so a new patch only needs a new number. `SOURCE.txt` in the image lists
  the applied patches with their SHA-256.
- Only `kicad-cli`, `_eeschema.kiface`, `_pcbnew.kiface` and `_cvpcb.kiface`
  (footprint library links of schematics) and their shared
  libraries) are built: no GUI programs, QA tests, wxPython or translations.
  They are installed by hand under `/opt/kicad-altium`, because `ninja install`
  would build everything.
- `ttf-liberation` is installed because Altium boards use Arial for TrueType
  text and Liberation Sans has the same metrics (DRC and plots of such text).
- `.github/workflows/image.yml` builds and smoke-tests the image on every push.
  On a `v*` tag it pushes `ghcr.io/<owner>/altium2kicad` and creates a release
  with the source.
