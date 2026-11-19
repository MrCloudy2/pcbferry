# kicad2altium in Altium Designer 26.11.1

The check in [ALTIUM-CHECK.md](../ALTIUM-CHECK.md), run on Windows 11 with Altium Designer
Professional 26.11.1. KiCad's demos at 10.0.7 were converted by kicad2altium from branch
`k2a-ad26-open` (each board with its `.kicad_pro`, as CI converts them), then opened, counted,
saved back, repoured and DRC-checked by DelphiScript driven from the shell. The fixes that
branch makes, and what is still open, are in its pull request.

Before those fixes (the PcbDocs of CI run 44501452423) every board failed to open: "The following
exception occurred whilst loading section Board6 / Pads6: Catastrophic failure", and came up with
no pads and no layer stack.

## Boards: open

Every demo board, as Altium reads it. Pads include those in components; all counts equal what
kicad2altium printed for the board, and the copper layer count equals KiCad's.

| board | dialogs | copper | components | nets | pads | vias | polygons |
|---|---|---|---|---|---|---|---|
| CM5_MINIMA_3 | 0 | 6 | 112 | 220 | 633 | 444 | 18 |
| One-Air-Max | 0 | 4 | 210 | 156 | 655 | 410 | 43 |
| RoyalBlue54L-Feather | 0 | 8 | 71 | 95 | 428 | 183 | 273 |
| RoyalBlue54L-NFC-Antenna | 0 | 2 | 2 | 1 | 2 | 2 | 0 |
| StickHub | 0 | 2 | 94 | 47 | 278 | 87 | 5 |
| complex_hierarchy | 0 | 2 | 68 | 52 | 165 | 0 | 166 |
| ecc83-pp | 0 | 2 | 15 | 13 | 33 | 0 | 1 |
| ecc83-pp_v2 | 0 | 2 | 15 | 13 | 34 | 0 | 1 |
| interf_u | 0 | 2 | 25 | 173 | 379 | 84 | 1 |
| jetson-agx-thor-baseboard | 0 | 10 | 1125 | 1387 | 4862 | 3235 | 100 |
| kit-dev-coldfire-xilinx_5213 | 0 | 4 | 160 | 278 | 825 | 253 | 3 |
| microwave | 0 | 2 | 4 | 0 | 8 | 0 | 0 |
| multichannel_mixer | 0 | 2 | 114 | 80 | 327 | 29 | 3 |
| multichannel_mixer-unrouted | 0 | 2 | 114 | 80 | 327 | 19 | 2 |
| pic_programmer | 0 | 2 | 63 | 111 | 247 | 6 | 1 |
| sonde xilinx | 0 | 2 | 25 | 42 | 108 | 3 | 1 |
| tinytapeout-demo | 0 | 4 | 309 | 114 | 757 | 405 | 4 |
| video | 0 | 4 | 189 | 588 | 2118 | 808 | 2 |
| vme-wren | 0 | 12 | 1508 | 1800 | 6883 | 4374 | 134 |

## Boards: the full check

Open, layer stack, counts, Repour All, DRC (default options) and both save-backs, for the boards
the brief names: [ecc83-pp_v2](ecc83-pp_v2/report.md), [pic_programmer](pic_programmer/report.md),
[StickHub](StickHub/report.md), [video](video/report.md), [vme-wren](vme-wren/report.md),
[jetson-agx-thor-baseboard](jetson-agx-thor-baseboard/report.md). In short:

- No dialog on open; Repour All runs without error on all six.
- No short circuit or clearance violation from the conversion on ecc83-pp_v2, StickHub, video
  and jetson. pic_programmer: 2 shorts and 3 clearance violations, all copper texts ("1=>>",
  "PWR ON") touching pads or tracks.
- vme-wren: 463 un-routed connections after Repour All, on power nets between neighbouring
  capacitors and vias that KiCad's fill connects: Altium's repour of those polygons does not.
- Silk To Solder Mask, Minimum Solder Mask Sliver and Board Clearance (silk on the edge) are
  Altium rules with no KiCad counterpart, at Altium's defaults.

## Schematics

Each demo project's SchDocs, opened as a project (a `.PrjPcb` listing the sheet where the
converter wrote none) and compiled. Altium shows its Messages panel by itself only for errors:
"no errors" means the compile raised none (warnings, if any, were not captured).
The top sheet of each is in `schematics/<project>/sheet.png`, the messages in `messages.png`.

| project | sheets | components | nets | compile |
|---|---|---|---|---|
| RoyalBlue54L-NFC-Antenna | 1 | 1 | 1 | no errors (Messages panel not shown) |
| a-multi | 1 | 4 | 4 | no errors (Messages panel not shown) |
| up-down-c | 1 | 3 | 6 | errors, see messages.png |
| rectifier | 1 | 5 | 4 | no errors (Messages panel not shown) |
| mult_vca810 | 1 | 5 | 6 | errors, see messages.png |
| buck_conv | 1 | 8 | 6 | no errors (Messages panel not shown) |
| sallen_key | 1 | 8 | 7 | no errors (Messages panel not shown) |
| laser_driver | 1 | 13 | 9 | no errors (Messages panel not shown) |
| generic_opamp_bip | 1 | 14 | 10 | no errors (Messages panel not shown) |
| ecc83-pp | 1 | 15 | 13 | no errors (Messages panel not shown) |
| power_supply | 1 | 14 | 9 | no errors (Messages panel not shown) |
| ecc83-pp_v2 | 1 | 15 | 13 | no errors (Messages panel not shown) |
| royer1 | 1 | 17 | 10 | no errors (Messages panel not shown) |
| ibis | 1 | 19 | 9 | no errors (Messages panel not shown) |
| smps-com | 1 | 21 | 15 | no errors (Messages panel not shown) |
| amplifier-ac | 1 | 23 | 13 | no errors (Messages panel not shown) |
| hv_converter | 1 | 25 | 14 | no errors (Messages panel not shown) |
| mainsheet | 3 | 13 | 8 | no errors (Messages panel not shown) |
| Class-D | 1 | 39 | 23 | errors, see messages.png |
| v_i_sources | 1 | 36 | 19 | no errors (Messages panel not shown) |
| sonde xilinx | 1 | 25 | 42 | no errors (Messages panel not shown) |
| interf_u | 1 | 24 | 175 | no errors (Messages panel not shown) |
| complex_hierarchy | 3 | 68 | 48 | no errors (Messages panel not shown) |
| pic_programmer | 2 | 66 | 115 | errors, see messages.png |
| Q17ng | 1 | 84 | 46 | no errors (Messages panel not shown) |
| StickHub | 1 | 94 | 47 | no errors (Messages panel not shown) |
| kit-dev-coldfire-xilinx_5213 | 3 | 160 | 304 | errors, see messages.png |
| tinytapeout-demo | 2 | 150 | 113 | no errors (Messages panel not shown) |
| One-Air-Max | 3 | 204 | 155 | errors, see messages.png |
| CM5_MINIMA_3 | 8 | 84 | 273 | errors, see messages.png |
| multichannel_mixer | 5 | 114 | 80 | no errors (Messages panel not shown) |
| video | 8 | 189 | 845 | errors, see messages.png |
| RoyalBlue54L-Feather | 5 | 71 | 153 | errors, see messages.png |
| jetson-agx-thor-baseboard | 15 | 1123 | 1658 | errors, see messages.png |
| vme-wren | 73 | 1507 | 2629 | errors, see messages.png |

Errors left: the single-sheet ones are Altium's ERC on simulation and test circuits (nets with one
pin, missing footprints of SPICE sources). The hierarchical ones (video, kit-dev-coldfire,
CM5_MINIMA_3, One-Air-Max, RoyalBlue54L-Feather, jetson, vme-wren) report "Duplicate Net Names"
and "Net ... has only one pin" for signals and bus members that cross sheets: Altium counts more
nets than the board has (video 845 for 588). pic_programmer also has a label `VPP{slash}MCLR`
left escaped and custom VPP power symbols written as components.
