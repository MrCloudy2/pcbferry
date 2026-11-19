# Checking kicad2altium's PcbDocs in Altium Designer

kicad2altium is tested by reading its PcbDocs back with KiCad's Altium importer. Whether
**Altium Designer** opens them, and shows the same board, can only be checked on Windows with
Altium. This is the brief for whoever (or whichever Claude) does that.

## Get the files

The `kicad2altium` CI job converts every KiCad demo board and uploads them, each `.PcbDoc`
next to the `.kicad_pcb` it came from:

```
gh run list -R MrCloudy2/pcbferry -w ci -b kicad2altium -L 1
gh run download <run id> -R MrCloudy2/pcbferry -n kicad2altium-demos -D kicad2altium-demos
```

Or build the converter (Rust) and convert a board yourself:

```
cd k2a
cargo run --release -- path\to\board.kicad_pcb out.PcbDoc
```

Start small and go up: `ecc83-pp_v2`, `pic_programmer`, `StickHub`, `video`, `vme-wren`,
`jetson-agx-thor-baseboard`.

## For each board

1. **Open** it (File » Open). Write down every dialog, warning or error word for word, and what
   the Messages panel says. If Altium refuses the file, stop there for that board: the exact
   message is the result.
2. **Look**: does the board shape show? Components, tracks, vias, polygons (filled?), silkscreen
   and text in the right places? A screenshot of the whole board, and one zoomed into a dense
   area, is worth a lot.
3. **Layer Stack Manager** (Design » Layer Stack Manager): how many copper layers, their names,
   the mechanical layers and their types (Assembly Top, Courtyard Top, ...).
4. **Counts** from the PCB panel or PCB List: components, nets, pads, vias. Compare them with
   what `kicad2altium` printed for that board (it prints them when it converts).
5. **Polygons**: Tools » Polygon Pours » Repour All. Did anything change, or fail?
6. **DRC** (Tools » Design Rule Check, defaults): the number of violations per rule, especially
   Short-Circuit, Un-Routed Net and Clearance. Shorts or unrouted nets mean the copper or the
   nets are wrong; clearance violations may just be KiCad's rules versus Altium's defaults.
7. **Save it back, twice**, without changing anything:
   - File » Save As, *PCB Binary Files* → `<board>.altium-saved.PcbDoc`
   - File » Save As, *PCB ASCII Files* → `<board>.altium-ascii.PcbDoc`

   The ASCII file lists every record with every field Altium writes; comparing it with ours shows
   exactly what Altium fills in or changes.

## Send the results back

On a branch `altium-check-results` of MrCloudy2/pcbferry, under `k2a/altium-check/<board>/`:

- `report.md`: Altium's version, and steps 1 to 6 above
- the screenshots
- the two saved files

Open a draft pull request into `kicad2altium` and leave it unmerged. The demo boards are KiCad's
own (in KiCad's source tree), so they may be committed; do not add anyone's private designs.
