# video in Altium Designer 26.11.1

Converted by kicad2altium from branch `k2a-ad26-open` (KiCad demos at 10.0.7, with their `.kicad_pro`), opened, checked and saved back by script in Altium Designer Professional 26.11.1 on Windows 11.

## 1. Open

No dialog, warning or error.

## 2. Look

![board](board-view.png)

## 3. Layer stack

4 copper layers: top_copper, Mid-Layer 1, Mid-Layer 2, Bottom Layer.

Physical stack, top to bottom: Top Paste, Top Overlay, Top Solder, top_copper, Dielectric 1, Mid-Layer 1, Dielectric 2, Mid-Layer 2, Dielectric 3, Bottom Layer, Bottom Solder, Bottom Overlay, Bottom Paste.

Mechanical layers in use: 1 Mechanical 1, 46 F.Fab, 47 B.Fab, 48 F.Courtyard, 49 B.Courtyard, 50 User.Drawings, 51 User.Comments, 52 F.Adhesive, 53 B.Adhesive, 54 User.Eco1, 55 User.Eco2, 56 Margin.

## 4. Counts

kicad2altium printed: 189 components, 2118 pads, 808 vias, 10254 tracks, 26 arcs, 544 texts, 2 polygons, 28 regions, 588 nets, 0 models, 0 componentbodies

| | Altium (all) | not in a component |
|---|---|---|
| components | 189 | 189 |
| nets | 588 | 588 |
| pads | 2118 | 0 |
| vias | 808 | 808 |
| tracks | 8634 | 7932 |
| arcs | 12 | 0 |
| texts | 209 | 1 |
| polygons | 2 | 2 |
| regions | 25 | 9 |
| component bodies | 0 | 0 |
| rules | 44 | 44 |
| classes | 18 | 18 |

Inside components (group iterator): pads=2118 tracks=2306 arcs=26 texts=165 regions=20 fills=0 bodies=0.

Altium's board iterator does not visit every primitive inside components; their tracks, texts and pads are counted by the group iterator above, and the ASCII save below lists every record.

## 5. Polygons

Repour All: before polygons=2 pour primitives=8; after polygons=2 pour primitives=89. No error or dialog.

## 6. DRC

Default options, after the repour. Violations per rule (Altium stops at 500):

```
0  Clearance Constraint (Gap=7.874mil) (All),(All)
     0  Clearance Constraint (Gap=7.874mil) (InNetClass('pwr')),(All)
     0  Clearance Constraint (Gap=15.748mil) (InNamedPolygon('Polygon 1')),(All)
     0  Clearance Constraint (Gap=15.748mil) (InNamedPolygon('Polygon 0')),(All)
     0  Short-Circuit Constraint (Allowed=No) (All),(All)
     0  Un-Routed Net Constraint ( (All) )
     0  Modified Polygon (Allow modified: No), (Allow shelved: No)
     0  Width Constraint (Min=7.874mil) (Max=10mil) (Preferred=9.842mil) (InNetClass('pwr'))
     0  Width Constraint (Min=7.874mil) (Max=10mil) (Preferred=7.874mil) (All)
     0  Routing Topology Rule(Topology=Shortest) (All)
     0  Power Plane Connect Rule(Relief Connect )(Expansion=20mil) (Conductor Width=10mil) (Air Gap=10mil) (Entries=4) (All)
     0  Minimum Annular Ring (Minimum=1.968mil) (All)
     0  Hole Size Constraint (Min=15.748mil) (Max=125.984mil) (All)
     0  Hole To Hole Clearance (Gap=9.842mil) (All),(All)
   203  Minimum Solder Mask Sliver (Gap=10mil) (All),(All)
   297  Silk To Solder Mask (Clearance=10mil) (IsPad),(All)
     0  Silk to Silk (Clearance=0mil) (All),(All)
     0  Net Antennae (Tolerance=0mil) (All)
     0  Board Clearance Constraint (Gap=0mil) (All)
     0  Height Constraint (Min=0mil) (Max=1000mil) (Prefered=500mil) (All)
   500  TOTAL
```

## 7. Saved back

`video.altium-saved.PcbDoc` (PCB Binary) and `video.altium-ascii.PcbDoc` (PCB ASCII), saved without changes after opening (files over 20 MB gzipped).
