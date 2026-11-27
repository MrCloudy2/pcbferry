//! Copper zones as Altium polygons (Polygons6) with their fill (Regions6).

use crate::altium::{mil, Buf, Doc, Props};
use crate::convert::{coords, layer_for_zone, net_index, v7_name, Report};
use crate::kicad::{Board, Pt};

/// Returns, for each zone, the names of the Altium polygons made from it
pub fn write(doc: &mut Doc, b: &Board, report: &mut Report) -> Vec<Vec<String>> {
    let mut made: Vec<Vec<String>> = vec![Vec::new(); b.zones.len()];
    let order = pour_order(&b.zones.iter().map(|z| z.priority).collect::<Vec<_>>());
    for (zi, z) in b.zones.iter().enumerate() {
        if z.keepout {
            keepout(doc, b, z, report);
            continue;
        }
        for layer in &z.layers {
            let names: Vec<String> = if layer == "F&B.Cu" || layer == "*.Cu" {
                if layer == "*.Cu" {
                    b.copper.clone()
                } else {
                    vec!["F.Cu".into(), "B.Cu".into()]
                }
            } else {
                vec![layer.clone()]
            };
            for name in names {
                let Some(l) = layer_for_zone(b, &name) else {
                    continue;
                };
                if z.outline.len() < 3 {
                    continue;
                }
                let index = doc.storage("Polygons6").count as u16;
                let mut p = Props::new();
                p.set("LAYER", v7_name(l));
                p.bool("LOCKED", false)
                    .bool("POLYGONOUTLINE", false)
                    .bool("USERROUTED", true)
                    .bool("KEEPOUT", false);
                p.num("UNIONINDEX", 0).bool("PRIMITIVELOCK", false);
                p.set("POLYGONTYPE", "Polygon");
                // pour over same-net objects, as KiCad does; Altium reads "1" here as FALSE
                p.bool("POUROVER", true)
                    .num("POUROVERSTYLE", 1)
                    .bool("REMOVEDEAD", false);
                p.set("GRIDSIZE", mil(z.min_thickness / 2))
                    .set("TRACKWIDTH", mil(z.min_thickness));
                p.set("HATCHSTYLE", "Solid")
                    .bool("USEOCTAGONS", false)
                    .set("MINPRIMLENGTH", mil(0));
                // Altium repeats the first vertex at the end; without it Altium Designer drops
                // the last edge and repours a smaller polygon (a rectangle comes out a triangle)
                let closed = z.outline.first() != z.outline.last();
                let first = z.outline[0];
                let outline = z.outline.iter().chain(closed.then_some(&first));
                for (i, v) in outline.enumerate() {
                    let (x, y) = coords(*v);
                    let k = i.to_string();
                    p.num(&format!("KIND{k}"), 0);
                    p.set(&format!("VX{k}"), units_mil(x));
                    p.set(&format!("VY{k}"), units_mil(y));
                    p.set(&format!("CX{k}"), "0mil")
                        .set(&format!("CY{k}"), "0mil");
                    p.set(&format!("SA{k}"), "0.000")
                        .set(&format!("EA{k}"), "0.000")
                        .set(&format!("R{k}"), "0mil");
                }
                p.bool("SHELVED", false)
                    .bool("REMOVEISLANDSBYAREA", false)
                    .bool("REMOVENECKS", true);
                p.num("POURINDEX", order[zi])
                    .bool("IGNOREVIOLATIONS", false);
                p.num("NET", net_index(z.net));
                let pname = format!("Polygon {index}");
                p.set("NAME", pname.as_str());
                made[zi].push(pname);
                doc.storage("Polygons6").push(&p.encode());

                for (fl, pts) in z.fills.iter().filter(|(fl, _)| *fl == name) {
                    let _ = fl;
                    if pts.len() >= 3 {
                        fill(doc, l, pts, z.net, index);
                    }
                }
            }
        }
    }
    made
}

/// Each zone's place in Altium's pour order: Altium pours index 1 first and later polygons
/// keep clear of it, KiCad fills its highest priority first. Equal priorities keep file order
fn pour_order(priority: &[i64]) -> Vec<usize> {
    let mut by: Vec<usize> = (0..priority.len()).collect();
    by.sort_by_key(|&i| std::cmp::Reverse(priority[i]));
    let mut order = vec![0; priority.len()];
    for (rank, i) in by.into_iter().enumerate() {
        order[i] = rank + 1;
    }
    order
}

#[test]
fn highest_priority_pours_first() {
    assert_eq!(pour_order(&[0, 1000, 5, 5]), vec![4, 1, 2, 3]);
}

/// A rule area as Altium keep-out regions: on the Keep-Out layer when it covers every copper
/// layer, else one per copper layer
fn keepout(doc: &mut Doc, b: &Board, z: &crate::kicad::Zone, report: &mut Report) {
    if z.outline.len() < 3 {
        return;
    }
    let bits = (z.no_vias as u8)
        | (z.no_tracks as u8) << 1
        | (z.no_pour as u8) << 2
        | if z.no_pads { 0x18 } else { 0 };
    if bits == 0 {
        if !z.no_footprints {
            return;
        }
        let w = "rule areas that only keep out footprints have no Altium keep-out; left out"
            .to_string();
        if !report.warnings.contains(&w) {
            report.warnings.push(w);
        }
        return;
    }
    let mut cu: Vec<String> = Vec::new();
    for l in &z.layers {
        match l.as_str() {
            "*.Cu" => cu.extend(b.copper.iter().cloned()),
            "F&B.Cu" => cu.extend(["F.Cu".to_string(), "B.Cu".to_string()]),
            l if l.ends_with(".Cu") => cu.push(l.to_string()),
            _ => {}
        }
    }
    cu.sort();
    cu.dedup();
    let all = cu.len() == b.copper.len();
    let kind = crate::convert::RegionKind::Keepout(bits);
    if all {
        let l = crate::convert::Layer {
            v6: 56,
            v7: 0x0103_000d,
        };
        crate::convert::region(doc, l, &z.outline, 0, 0xffff, kind);
    } else {
        for c in cu {
            if let Some(l) = layer_for_zone(b, &c) {
                crate::convert::region(doc, l, &z.outline, 0, 0xffff, kind);
            }
        }
    }
}

fn units_mil(u: i32) -> String {
    let m = u as f64 / 10000.0;
    format!("{m:.4}mil")
}

fn fill(doc: &mut Doc, l: crate::convert::Layer, pts: &[Pt], net: usize, polygon: u16) {
    let mut props = Props::new();
    props
        .set("V7_LAYER", v7_name(l))
        .num("KIND", 0)
        .bool("ISBOARDCUTOUT", false)
        .bool("ISSHAPEBASED", false);
    props.num("SUBPOLYINDEX", 0).num("UNIONINDEX", 0);
    let mut s = Buf::default();
    let n = net_index(net);
    s.u8(l.v6)
        .u8(0x0c)
        .u8(0)
        .u16(if n < 0 { 0xffff } else { n as u16 })
        .u16(polygon)
        .u16(0xffff);
    s.bytes(&[0xff, 0xff, 0xff, 0xff, 0]).u16(0).u16(0);
    s.bytes(&props.encode());
    s.u32(pts.len() as u32);
    for p in pts {
        let (x, y) = coords(*p);
        s.f64(x as f64).f64(y as f64);
    }
    let mut r = Buf::default();
    r.u8(11).block(&s.0);
    doc.storage("Regions6").push(&r.0);
}
