# pcbferry

**Altium Designer → KiCad 10 converter.** Converts a whole Altium project
(schematics, board, libraries, design rules) into a ready-to-open KiCad 10
project from the command line.

pcbferry is KiCad 10.0.7's own Altium importer with 25 patches that fix what it
gets wrong, built headless (`kicad-cli` only), plus a small wrapper that turns a
`.PrjPcb` into a KiCad project folder. The output is a plain KiCad 10 project:
open it with any KiCad 10.0.x.

Not related to the archived [altium2kicad](https://github.com/thesourcerer8/altium2kicad)
Perl scripts.

## How good is it

Measured on 21 Altium projects (19 public open-hardware designs from 2 to 789
parts, 2 private) against Altium's own output: its compiled netlist, and the
Gerber and NC drill files it writes.

- **Schematic and board data:** 100% on all 21 projects for pin connectivity,
  components, footprints, component values and placements (position, rotation,
  side). Stock KiCad 10 splits cross-sheet nets, shorts harness members, drops
  multichannel sheets and variants, and more: see [what the patches fix](#what-the-patches-fix).
- **Libraries:** every symbol and footprint ends up linked to a library (the
  project's own SchLib/PcbLib/IntLib, or a generated one). Stock `kicad-cli`
  leaves all of them unlinked.
- **Fabrication output** (19 boards, KiCad's plots against Altium's):

  | | Area differing from Altium's |
  |---|---|
  | Drill holes and slots | 0 of 9,514 holes |
  | Board outline | 0.14% (18 of 19 boards identical) |
  | Copper, signal layers | 0.10% |
  | Paste | 1.2% |
  | Solder mask | 1.8% |
  | Internal planes (after filling zones) | ~10%, see [planes](#fabrication-output) |
  | Silkscreen | ~30%: text drawn with KiCad's fonts |

- **Design rules:** of 877 enabled Altium rules, 251 become board settings and
  netclasses, 102 scoped rules (clearances by net or layer, widths, diff pairs,
  matched lengths, plane clearances by hole size, via tenting, ...) are written
  as KiCad custom rules (`NAME.kicad_dru`), 38 are no-ops (KiCad's default).
  Most of the rest have no KiCad counterpart (autorouter settings, testpoint
  rules, supply nets) or are checks KiCad always runs; `summary.json` lists them.

## Quick start

Releases are published as a container image:

```bash
docker pull ghcr.io/mrcloudy2/pcbferry:latest
```

Convert a project (the image's entry point is the `pcbferry` wrapper):

```bash
docker run --rm -u "$(id -u):$(id -g)" -v "$PWD:/work" -w /work ghcr.io/mrcloudy2/pcbferry Board.PrjPcb out
```

Or use `pcbferry-container` from this repository, which mounts the project's
folder read-only, writes the output folder as your user, and works with podman
or docker:

```bash
PCBFERRY_IMAGE=ghcr.io/mrcloudy2/pcbferry ./pcbferry-container --check path/to/Board.PrjPcb out/Board
```

If the `.PrjPcb` lists documents outside its own folder (`..\Libraries\...`),
set `PCBFERRY_ROOT` to a folder containing all of them.

### Build it yourself

About 40 minutes with 4 build jobs and a few GB of RAM; the image is ~1.3 GB
compressed, ~4 GB on disk:

```bash
./build.sh
```

`build.sh` builds the image from `patches/`, smoke-tests it and tags it
`pcbferry:latest`. `JOBS=8 ./build.sh` compiles with more cores. Without the
script: `docker build -t pcbferry -f Containerfile .` (or `podman build`).

## Usage

```
pcbferry [options] INPUT OUTPUT_DIR
```

Input can be a `.PrjPcb` (recommended), a single `.SchDoc` or a `.PcbDoc`.

| Option | |
|---|---|
| `--check` | afterwards export a netlist and run ERC and DRC on the result, and report unlinked symbols/footprints |
| `--name NAME` | KiCad project name (default: the input's) |
| `--board FILE` | which PcbDoc, when a project has several (default: the first) |
| `--force` | write into a non-empty output folder |
| `--fill-zones` | fill the planes the import leaves unfilled, using the converted rules (see [planes](#fabrication-output)) |
| `--no-custom-rules` | don't write the scoped Altium rules to `NAME.kicad_dru` |
| `--footprint-slash CHAR` | replace `/` in footprint names with `CHAR` (e.g. `_`), see [limitations](#limitations) |
| `--remove-unused-lands` | drop unconnected pad and via lands on inner layers, see [planes](#fabrication-output) |

The exit status is non-zero if an import or check fails to run; DRC violations
alone do not fail it. The wrapper is a standalone Python 3 script and works with
any `kicad-cli` that has the patches: point `KICAD_CLI` at it.

For a project, pcbferry imports **the board first, then the schematic**, into
the same folder under the same project name: the schematic import takes each
symbol's footprint library link from the board part with the same reference.

The output folder gets:

| File | From |
|---|---|
| `NAME.kicad_pro` | board and schematic settings, incl. the imported design rules and netclasses |
| `NAME.kicad_dru` | the scoped Altium rules as KiCad custom rules |
| `NAME.kicad_pcb` | `kicad-cli pcb import --format altium` of the project's (first) PcbDoc |
| `NAME.kicad_sch` + one `.kicad_sch` per sheet | `kicad-cli sch import` of the whole project |
| `NAME-import-fps.pretty/` | footprints placed on the board that no project library holds as placed |
| `NAME-import-syms.kicad_sym` | symbols with no readable library (DbLib, server parts) or differing from it |
| copies of the project's SchLib / PcbLib / IntLib | the project's own libraries |
| `fp-lib-table`, `sym-lib-table` | register all of the above, so every symbol and footprint is linked |
| `import-reports/pcb-import.json`, `sch-import.json` | kicad-cli's import reports (warnings, layer mapping, counts) |
| `import-reports/summary.json` | every step, its exit code, errors and warnings |
| `import-reports/NAME.net`, `erc.json`, `drc.json` | with `--check`: netlist, ERC and DRC (+ schematic parity) of the result |

A footprint placed on the board in several different forms under one name gets
one library entry per form: `NAME`, `NAME (2)`, `NAME (3)`, ...

### Reading the reports

- With `--check`, `library_issues` in the summary (and the last line of output)
  counts ERC `lib_symbol_issues`/`footprint_link_issues` and DRC
  `lib_footprint_issues`/`lib_footprint_mismatch`. A clean conversion has none.
- `symbols_without_library` counts "No library symbol found" messages the
  importer emits while reading, before it links symbols to the libraries it
  writes. They are informational; ERC's `lib_symbol_issues` tells whether
  symbols ended up linked.
- Schematic parity in `drc.json` lists `net_conflict` for nets Altium never
  named: Altium calls them `NetC1_2`, KiCad `Net-(C1-Pad2)`. Same connection;
  "Update PCB from Schematic" renames them.

## Fabrication output

- **Fill zones before plotting.** Altium computes internal planes when it
  writes output, so the PcbDoc holds no plane copper and the imported plane
  zones are unfilled. Convert with `--fill-zones`, or press B in pcbnew, or the
  inner planes come out empty.
- **Plane clearances.** Altium draws no pad lands on plane layers and measures
  plane clearance from the drilled hole; KiCad keeps the lands and measures
  from them. Electrically it is the same board; filled planes differ from
  Altium's by about 5–10 % of their area. `--remove-unused-lands` drops pad and
  via lands on inner layers where nothing connects, which brings planes close to
  Altium's but also removes lands Altium keeps on inner *signal* layers, and
  through-hole footprints then differ from the project's library copies (they
  go to `NAME-import-fps`). It pays off only where planes outweigh inner signal
  layers (copper differing from Altium's, default → with the option: 6 planes:
  4,959 → 1,477 mm²; 2 planes: 1,489 → 867 mm²; 1 plane and 3 inner signal
  layers: 1,777 → 3,240 mm²; no planes: 12.6 → 758 mm²). Off by default.
- **Text:** KiCad's stroke font is narrower than Altium's, and TrueType text
  needs the font installed (Liberation Sans stands in for Arial in the image).
- Footprint "board cutout" regions that overlap the board edge are added to
  Edge.Cuts; Altium's outline output leaves them out.
- Altium panels built from embedded boards are not supported.

## Limitations

- Altium rules with no KiCad counterpart (rooms, fanout and routing-style
  settings, component height and clearance, testpoints, ...) are not carried;
  the `custom rules` step in `summary.json` lists each one and why.
- Board DRC on an imported board reports many violations: Altium and KiCad
  check different things, with different defaults. Treat the first DRC as a
  to-do list, not a measure of the import.
- One board per project per run (`--board` picks it).
- Footprint names containing `/` (e.g. `CAP 0805/2012`) can't be library file
  names, so by default those footprints stay unlinked. `--footprint-slash _`
  renames them (`CAP 0805_2012`) on the board, in the libraries and in the
  schematic's footprint fields, and they link.
- A few footprints with keepout zones on rotated parts still differ from their
  library copy (a KiCad-side comparison issue; 6 across the 21 test projects).
- `kicad-cli version` reports `10.0.7-dirty-pcbferry`: "dirty" because the
  patches are applied on top of the release tag.

## What the patches fix

Each patch is a plain `git diff` against KiCad 10.0.7 in `patches/`, applied in
order.

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
| `0020` `pcb import --footprint-slash` | footprints named with `/` can't be linked to a library | opt-in rename; the schematic import follows the board's names (also footprints named like `RESC2012M (0805)`) |
| `0021` mask: vias tented by rule, bare holes | vias tented by an Altium solder-mask rule (`IsVia`, negative expansion) come out open (US-Plus: 1,150 mm² of extra mask openings); bare holes (zero-size pads) get mask openings Altium doesn't draw | tented; no opening |
| `0022` board: arc ends, stackup, extra layers | arc ends a few nm off the copper they continue are not connected (Kasli: 499 unconnected items after refill, now 50); dielectrics alternate core/prepreg instead of Altium's; layers beyond Mechanical 45 and Route Guide dropped | ends snapped within 10 µm; Altium's core/prepreg; kept on a free user layer |
| `0023` `pcb import --fill-zones` | – | fills the zones the import leaves unfilled |
| `0024` schematic Parameter Sets and Blankets | rejected ("not currently supported"): net classes and differential-pair directives set in the schematic are lost | directive labels (`ClassName` → Netclass field) and rule areas |
| `0025` paste percentage | Altium's paste rule as a percentage of pad area ignored, and its −0.02 mm expansion used instead (MISKO3: 47 mm² of extra paste) | KiCad paste ratio; pads with their own value keep it |

## Relation to KiCad

pcbferry is built from KiCad's source with these patches on top; it is not an
official KiCad project. The patches are not being submitted upstream for now.
Moving to a newer KiCad tag means updating `KICAD_TAG`/`KICAD_COMMIT` in the
`Containerfile` and refreshing the patches. The patches were written with AI
assistance and checked against the measurements above.

## Licensing

KiCad is GPL-3.0-or-later. The patches and the wrapper scripts here are
distributed under the same terms (see `LICENSE`).

The image carries its corresponding source in
`/opt/kicad-altium/share/doc/pcbferry/`: the patches, the exact patched
KiCad tree it was built from (`kicad-10.0.7-pcbferry-source.tar.xz`), the
build configuration (`SOURCE.txt`) and KiCad's license files. To get them out:

```bash
docker create --name pcbferry-src pcbferry
```

```bash
docker cp pcbferry-src:/opt/kicad-altium/share/doc/pcbferry ./pcbferry-source
```

```bash
docker rm pcbferry-src
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
- CI (`.github/workflows/ci.yml`), on every pull request and push to `main`:
  checks the wrapper scripts, applies the patches to the pinned KiCad tag,
  builds the image, and converts two public Altium projects (Digispark ATTiny85
  and Sinara Kasli, pinned) with `--check`, failing on any failed step or any
  unlinked symbol or footprint. On a `v*` tag it also pushes
  `ghcr.io/mrcloudy2/pcbferry` and creates a release with the source.
