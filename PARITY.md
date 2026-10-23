# Parity with Altium

How close a pcbferry conversion is to the Altium original, measured on 21 Altium
projects against what Altium itself produces. Numbers are from the current
patches ([PATCHES.md](PATCHES.md)) with default options plus `--fill-zones`.

## Corpus

19 public Altium projects (open-hardware boards from GitHub, Altium's own example
designs) and 2 private ones, listed as *Private A* and *Private B*. They range from a
2-layer ATtiny breakout to 12-layer FPGA and BGA boards with internal planes. One
project (FPGA3 Panel) is a panel of embedded boards, one (Nova_34) has schematics but
an empty board.

## How it is measured

| What | Reference | How |
|---|---|---|
| Netlist | Altium's compiled netlist (exported from Altium) | every pin's whole net, every part's reference, footprint, value and placement |
| Fabrication | Altium's Gerbers and drill files | each layer turned into polygons; area present in one and not the other, ignoring slivers under 0.01 mm and copper inside drilled holes |
| Connectivity | Altium's DRC (*Un-Routed Net* count) | KiCad DRC's unconnected items on the converted board, zones filled |
| Design rules | Altium's rule list (Rules6 in the PcbDoc) | each enabled rule classified: imported, translated to `.kicad_dru`, no KiCad equivalent, or untranslatable |
| Libraries | an independent reader of the Altium library files | symbols, pins, footprints and pads by name, number, position, size and drill |

## Netlist and placement

Every project, every column: **100 %**. Pins on the right net, components, footprints,
values, and placements (position, rotation, side).

## Fabrication output

Area that differs, as a share of Altium's area for that layer group:

| Layers | Differs | Notes |
|---|---|---|
| Copper, outer layers | 0.15 % | |
| Copper, inner layers | 2.7 % | most of it on three boards, see below |
| Solder mask | 3.9 % | 2.2 % is one board (keyboard_full) |
| Paste | 0.9 % | |
| Silkscreen | 31 % | fonts, see below |
| Board outline | 0.14 % | one board |
| Drill holes | 0 of 9,514 differ | |
| Slots | 0 of 50 differ | |

Per board, mm² that differ / Altium's mm²:

| Board | Copper | Mask | Paste | Silk | Outline | Holes |
|---|---|---|---|---|---|---|
| ATTiny85 | 0.00 / 246 | 0.32 / 195 | 0.00 / 111 | 34 / 58 | 0 / 455 | 23, all match |
| a1200_keyboard_adapter | 1.63 / 1290 | 0.05 / 280 | 0.00 / 8 | 21 / 62 | 0 / 750 | 61, all match |
| AVR_RCS | 11.4 / 15790 | 44.2 / 1592 | 0.00 / 561 | 386 / 1205 | 0 / 12275 | 216, all match |
| PCB_PatternDesignReference | 89.0 / 408 | 85.7 / 170 | 0.00 / 30 | 1303 / 2223 | 0 / 4492 | 25, all match |
| STLINKV2 | 0.00 / 2030 | 5.82 / 274 | 0.00 / 93 | 51 / 105 | 0 / 1301 | 75, all match |
| STM32F103C8 | 8.17 / 5248 | 43.3 / 982 | 0.00 / 314 | 128 / 452 | 0 / 3682 | 292, all match |
| Flight_Controller_Atmega | 0.00 / 10369 | 12.0 / 511 | 0.00 / 221 | 73 / 206 | 0 / 2958 | 95, all match |
| Private A | 0.00 / 161 | 5.82 / 277 | 0.00 / 0 | 7 / 64 | 0 / 3090 | 62, all match |
| helix_4 | 71.2 / 3959 | 43.9 / 285 | 9.06 / 130 | 17 / 79 | 0 / 1287 | 470, all match |
| Edge_logic_analyzer_P1 | 0.33 / 14145 | 17.6 / 623 | 0.00 / 157 | 323 / 1009 | 0 / 4165 | 840, all match |
| keyboard_full | 57.6 / 106830 | 577 / 5866 | 0.00 / 646 | 712 / 4502 | 0 / 63056 | 1030, all match |
| nova34 | 4.14 / 5833 | 71.5 / 475 | 30.8 / 281 | 26 / 137 | 0 / 974 | 414, all match |
| MISKO3 | 12.6 / 33310 | 23.2 / 1565 | 29.9 / 514 | 505 / 1035 | 0 / 6828 | 622, all match |
| US-Plus-CPM | 857 / 49694 | 43.8 / 3339 | 0.00 / 959 | 332 / 487 | 216 / 13215 | 941, all match |
| FMU3_REV_D | 36.2 / 6476 | 0.59 / 422 | 0.00 / 398 | 7 / 193 | 0 / 1129 | 196, all match |
| Kasli | 3228 / 53802 | 23.9 / 4832 | 0.00 / 1585 | 317 / 1491 | 0.2 / 19994 | 2338, all match |
| FPGA3 | 1080 / 60093 | 0.38 / 2362 | 0.00 / 789 | 142 / 649 | 0 / 7068 | 1404, all match |
| Quectel M66 | 0.02 / 1732 | 1.52 / 954 | 0.00 / 405 | 80 / 358 | 0 / 3680 | 84, all match |
| Private B | 0.00 / 6130 | 3.78 / 744 | 0.00 / 256 | 77 / 162 | 0 / 3355 | 326, all match |

## Board connectivity

Unconnected items in KiCad's DRC against Altium's *Un-Routed Net* count:

| Board | KiCad | Altium |
|---|---|---|
| FPGA3 | 0 | 0 |
| FMU3_REV_D | 0 | 0 |
| Edge_logic_analyzer_P1 | 0 | 0 |
| MISKO3 | 0 | 0 |
| helix_4 | 0 | 0 |
| US-Plus-CPM | 2 | 2 |
| Private A | 32 | 32 |
| nova34 | 89 | 91 |
| Kasli | 3 | 0 |

Kasli's three: two GND mounting holes inside keepouts on every layer (Altium doesn't
report a lone pad) and one capacitor on P2V5.

## Design rules

Of 877 enabled Altium rules in the corpus: 251 become KiCad board settings, net classes
or zone settings; 102 are written to `NAME.kicad_dru` as custom rules (scoped
clearances, widths, diff pairs, plane rules, mask expansion); 480 have no KiCad
equivalent or no effect; 44 can't be translated, mostly rules scoped to a named
polygon or a pad class, which KiCad's rule language can't select.

## Libraries

Symbols, pins and footprints by name: 100 % in every library the reference reader could
open (it fails on three of them). Pads: 100 % except two libraries at 99.4 % and 99.8 %.
Every placed symbol and footprint links to a library entry.

## What still differs

- **Silkscreen text.** KiCad has no Altium fonts. Text in Altium's stroke font comes
  out narrower; TrueType text uses whatever substitute is installed. This is most of
  the 31 %. Lines, arcs and graphics on silkscreen match.
- **Inner copper on three boards.**
  - Kasli: copper in small square areas around holes on In1–In3 that Altium pours
    and KiCad keeps clear (about 2,900 mm² of the 3,228). Cause still open.
  - FPGA3 and US-Plus-CPM: about 1,080 and 830 mm² of inner pours and planes come
    out differently filled. Not yet broken down.
- **Solder mask on keyboard_full.** 577 mm² of mask openings differ. Not yet
  broken down.
- **Paste on nova34.** Altium opens a row of bottom paste apertures under a
  top-side connector, and the reason isn't known yet.
- **Board outline on US-Plus-CPM.** 216 mm² differ, probably a footprint
  cut-out overlapping the board edge that ends up on Edge.Cuts.
- **Footprint libraries.** On a few boards, a placed footprint differs from its
  library footprint in keepout zones on rotated parts and in per-layer padstacks
  on bottom-side parts. Both versions are kept: the placed one goes in
  `NAME-import-fps`, so the board is unchanged.
- **Not converted.**
  - Panels of embedded boards: convert the board itself.
  - Several PcbDocs in one project: one per run (`--board`).
  - Rooms.
  - DbLib and Altium 365 libraries: there's no file to read; parts are taken
    from the placed copies.
