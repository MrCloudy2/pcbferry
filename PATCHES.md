# What the patches fix

pcbferry is KiCad 10.0.7 with these patches applied in order (`patches/NNNN-*.patch`, each a
plain `git diff`). The middle column is what stock KiCad 10.0.7 does with the same Altium
project; numbers come from the test projects in [PARITY.md](PARITY.md).

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
| `0026` board connectivity | vias and pads keep lands on Altium plane layers, so KiCad's clearances cut planes apart at tight pitch (FPGA3: 25 unconnected items, Kasli connector GND pins); vias that drop lands lose their connection to imported pours; pads smaller than their hole connect to nothing | lands only where connected on plane layers; pours and off-centre track ends stay connected; equal to Altium's DRC on 8 of 9 boards |
