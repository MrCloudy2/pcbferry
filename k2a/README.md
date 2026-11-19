# kicad2altium

pcbferry's other direction: a KiCad board (`.kicad_pcb`) as an Altium Designer PCB (binary
`.PcbDoc`), written natively. No Gerbers in between.

```
kicad2altium board.kicad_pcb [out.PcbDoc]
kicad2altium --check board.kicad_pcb        # read it back with KiCad and compare
```

## What it carries over

- Board outline (with arcs) and cut-outs, copper layer stack, layer names
- Nets, components (reference, footprint name, value, side, rotation, lock)
- Pads: round, oval, rectangular, rounded rectangle, octagonal (chamfered), offset copper, round
  and slotted holes, plated and not; custom pads as their anchor plus copper of the same net;
  paste or mask only pads; tenting and manual mask/paste expansion
- Vias (through, blind and buried by their layer pair), tracks, arcs
- Graphics on every layer: silkscreen, fab, courtyard, adhesive, drawings and user layers
  (User.n on Mechanical n; F.Fab, F.CrtYd and the like on Mechanical 46-56 with Altium's layer
  kinds: Assembly Top, Courtyard Top, ...)
- Texts, centred as KiCad places them
- Copper zones as polygons, with their fill
- Rule areas as keep-outs (vias, tracks, copper, pads)
- Net classes (from the `.kicad_pro` next to the board: classes, patterns, assignments) as
  Altium net classes, and design rules: clearance, width and via rules per class ahead of the
  board-wide ones, hole size, hole to hole, annular ring, board outline and silk clearance, mask
  and paste expansion, each zone's clearance and pad connection (thermal relief or direct)

Not yet: 3D models, dimensions, teardrops as such, trapezoid pads (written as rectangles), rule
areas that only keep out footprints, custom DRC rules (`.kicad_dru`).

## How it is tested

`tests/roundtrip.rs` converts each of KiCad's demo boards, reads the PcbDoc back with KiCad's
Altium importer (pcbferry's engine) and compares the two boards object by object: every net,
footprint, pad, track, arc, via, zone fill, keep-out and outline loop, within 0.1 µm (Altium
stores 2.54 nm steps). All 19 demos come back the same.

```
KICAD_CLI=path/to/kicad-cli KICAD_DEMOS=path/to/kicad/demos cargo test --release
```

Rules cannot come back that way (the importer writes no project file); unit tests read the
rule records back instead, and pcbferry's own rule translator reads them as it reads Altium's.

The importer has opinions of its own, which the comparison allows for: it joins track ends less
than 10 µm apart, makes footprint copper into board tracks or pads, and turns a full circle into
two half arcs.

That KiCad reads the files is not proof that Altium does: [ALTIUM-CHECK.md](ALTIUM-CHECK.md) is
the check to run in Altium Designer.

## How the files are written

A PcbDoc is an OLE compound file with a storage per object kind (`Tracks6`, `Pads6`, ...), each
holding a record count (`Header`) and the records (`Data`). The record layouts follow KiCad's
Altium reader; for the bytes nobody has decoded, via, pad and text records start from records
Altium Designer itself wrote (`src/templates.rs`), so those parts are as Altium expects them.
