//! Design rules (Rules6) and net classes (Classes6) from the KiCad project and board setup.
//!
//! Altium rules have a scope and a priority (1 = first); KiCad's net classes become Altium net
//! classes with their own Clearance, Width and RoutingVias rules ahead of the board-wide ones.

use crate::altium::{mil, Buf, Doc, Props};
use crate::kicad::{Board, Nm};
use crate::project::Project;

/// Altium's rule kinds as stored ahead of each rule's properties
mod kind {
    pub const CLEARANCE: (u16, &str) = (0, "Clearance");
    pub const WIDTH: (u16, &str) = (2, "Width");
    pub const ROUTING_VIAS: (u16, &str) = (11, "RoutingVias");
    pub const SOLDER_MASK_EXPANSION: (u16, &str) = (13, "SolderMaskExpansion");
    pub const PASTE_MASK_EXPANSION: (u16, &str) = (14, "PasteMaskExpansion");
    pub const SHORT_CIRCUIT: (u16, &str) = (15, "ShortCircuit");
    pub const UNROUTED_NET: (u16, &str) = (16, "UnRoutedNet");
    pub const MINIMUM_ANNULAR_RING: (u16, &str) = (19, "MinimumAnnularRing");
    pub const POLYGON_CONNECT: (u16, &str) = (20, "PolygonConnect");
    pub const HOLE_SIZE: (u16, &str) = (42, "HoleSize");
    pub const DIFF_PAIRS_ROUTING: (u16, &str) = (51, "DiffPairsRouting");
    pub const HOLE_TO_HOLE: (u16, &str) = (52, "HoleToHoleClearance");
    pub const MASK_SLIVER: (u16, &str) = (53, "MinimumSolderMaskSliver");
    pub const SILK_TO_SILK: (u16, &str) = (55, "SilkToSilkClearance");
    pub const BOARD_OUTLINE: (u16, &str) = (63, "BoardOutlineClearance");
}

struct Writer<'a> {
    doc: &'a mut Doc,
    n: usize,
}

impl Writer<'_> {
    /// One rule: the kind's id, then its properties
    fn rule(
        &mut self,
        k: (u16, &str),
        name: &str,
        scope1: &str,
        scope2: &str,
        priority: usize,
        f: impl FnOnce(&mut Props),
    ) {
        let mut p = Props::new();
        p.bool("SELECTION", false)
            .set("LAYER", "UNKNOWN")
            .bool("LOCKED", false);
        p.bool("POLYGONOUTLINE", false)
            .bool("USERROUTED", true)
            .bool("KEEPOUT", false)
            .num("UNIONINDEX", 0);
        p.set("RULEKIND", k.1).set(
            "NETSCOPE",
            if k.0 == kind::CLEARANCE.0 {
                "DifferentNets"
            } else {
                "AnyNet"
            },
        );
        p.set("LAYERKIND", "SameLayer")
            .set("SCOPE1EXPRESSION", scope1)
            .set("SCOPE2EXPRESSION", scope2);
        p.set("NAME", name)
            .bool("ENABLED", true)
            .num("PRIORITY", priority)
            .set("COMMENT", "");
        p.set("UNIQUEID", crate::convert::unique_id(3_000_000 + self.n))
            .bool("DEFINEDBYLOGICALDOCUMENT", false);
        f(&mut p);
        let mut b = Buf::default();
        b.u16(k.0).bytes(&p.encode());
        self.doc.storage("Rules6").push(&b.0);
        self.n += 1;
    }
}

fn scope_class(name: &str) -> String {
    format!("InNetClass('{}')", name.replace('\'', "''"))
}

/// Sizes the board actually uses, so maximum limits never flag what is already there
struct Used {
    max_track: Nm,
    max_via: Nm,
    max_via_drill: Nm,
    max_hole: Nm,
    min_hole: Nm,
}

fn used(b: &Board) -> Used {
    let max_track = b
        .tracks
        .iter()
        .map(|t| t.width)
        .chain(b.arcs.iter().map(|a| a.width))
        .max()
        .unwrap_or(0);
    let max_via = b.vias.iter().map(|v| v.size).max().unwrap_or(0);
    let max_via_drill = b.vias.iter().map(|v| v.drill).max().unwrap_or(0);
    let holes = b
        .footprints
        .iter()
        .flat_map(|f| f.pads.iter())
        .filter(|p| p.drill.x > 0)
        .map(|p| p.drill.x.max(p.drill.y))
        .chain(b.vias.iter().map(|v| v.drill));
    let (mut min_hole, mut max_hole) = (Nm::MAX, 0);
    for h in holes {
        min_hole = min_hole.min(h);
        max_hole = max_hole.max(h);
    }
    Used {
        max_track,
        max_via,
        max_via_drill,
        max_hole,
        min_hole: if min_hole == Nm::MAX { 0 } else { min_hole },
    }
}

pub fn write(doc: &mut Doc, b: &Board, project: Option<&Project>, polygons: &[Vec<String>]) {
    let empty = Project::default();
    let pr = project.unwrap_or(&empty);
    let u = used(b);
    let def = pr.default_class();
    // Net classes other than Default, with at least one net in them
    let classes: Vec<_> = pr
        .classes
        .iter()
        .filter(|c| c.name != "Default")
        .filter(|c| b.nets.iter().skip(1).any(|n| pr.class_of(n) == c.name))
        .collect();

    for (i, c) in classes.iter().enumerate() {
        let mut p = Props::new();
        p.bool("SELECTION", false)
            .set("LAYER", "MULTILAYER")
            .bool("LOCKED", false);
        p.bool("POLYGONOUTLINE", false)
            .bool("USERROUTED", true)
            .bool("KEEPOUT", false)
            .num("UNIONINDEX", 0);
        p.set("NAME", c.name.as_str())
            .num("KIND", 0)
            .bool("SUPERCLASS", false);
        for (k, n) in b
            .nets
            .iter()
            .skip(1)
            .filter(|n| pr.class_of(n) == c.name)
            .enumerate()
        {
            p.set(&format!("M{k}"), n.as_str());
        }
        p.bool("SELECTED", false)
            .set("UNIQUEID", crate::convert::unique_id(2_000_000 + i));
        doc.storage("Classes6").push(&p.encode());
    }

    let mut w = Writer { doc, n: 0 };
    let last = classes.len() + 1;

    // Clearance: per class first, the board-wide one last
    let base_clear = def
        .and_then(|d| d.clearance)
        .or(pr.rules.min_clearance)
        .unwrap_or(200_000);
    for (i, c) in classes.iter().enumerate() {
        if let Some(g) = c.clearance {
            w.rule(
                kind::CLEARANCE,
                &format!("Clearance_{}", c.name),
                &scope_class(&c.name),
                "All",
                i + 1,
                |p| {
                    p.set("GAP", mil(g)).set("GENERICCLEARANCE", mil(g));
                },
            );
        }
    }
    w.rule(kind::CLEARANCE, "Clearance", "All", "All", last, |p| {
        p.set("GAP", mil(base_clear))
            .set("GENERICCLEARANCE", mil(base_clear));
    });

    // Polygon clearance where a zone asks for more than the board does
    let mut prio = 1;
    for (zi, z) in b.zones.iter().enumerate() {
        if z.keepout || z.clearance <= base_clear {
            continue;
        }
        for name in polygons.get(zi).into_iter().flatten() {
            w.rule(
                kind::CLEARANCE,
                &format!("Clearance_{name}"),
                &format!("InNamedPolygon('{name}')"),
                "All",
                prio,
                |p| {
                    p.set("GAP", mil(z.clearance))
                        .set("GENERICCLEARANCE", mil(z.clearance));
                },
            );
            prio += 1;
        }
    }

    // Track width: preferred from the class, the minimum from the board, the maximum what is used
    let min_w = pr.rules.min_track_width.unwrap_or(0);
    let widths: Vec<Nm> = pr.classes.iter().filter_map(|c| c.track_width).collect();
    let max_w = widths
        .iter()
        .copied()
        .chain([u.max_track, min_w])
        .max()
        .unwrap_or(0)
        .max(254_000);
    for (i, c) in classes.iter().enumerate() {
        if let Some(t) = c.track_width {
            w.rule(
                kind::WIDTH,
                &format!("Width_{}", c.name),
                &scope_class(&c.name),
                "All",
                i + 1,
                |p| {
                    p.set("MAXLIMIT", mil(max_w))
                        .set("MINLIMIT", mil(min_w.min(t)))
                        .set("PREFEREDWIDTH", mil(t));
                },
            );
        }
    }
    let pref = def.and_then(|d| d.track_width).unwrap_or(250_000);
    w.rule(kind::WIDTH, "Width", "All", "All", last, |p| {
        p.set("MAXLIMIT", mil(max_w))
            .set("MINLIMIT", mil(min_w.min(pref)))
            .set("PREFEREDWIDTH", mil(pref));
    });

    // Vias
    let min_via = pr.rules.min_via_diameter.unwrap_or(0);
    let min_drill = pr.rules.min_through_hole.unwrap_or(0);
    let via_rule = |p: &mut Props, d: Nm, h: Nm| {
        p.set("HOLEWIDTH", mil(h))
            .set("MINHOLEWIDTH", mil(min_drill.min(h)))
            .set("MAXHOLEWIDTH", mil(u.max_via_drill.max(h)));
        p.set("WIDTH", mil(d))
            .set("MINWIDTH", mil(min_via.min(d)))
            .set("MAXWIDTH", mil(u.max_via.max(d)));
        p.set("VIASTYLE", "Through Hole");
    };
    for (i, c) in classes.iter().enumerate() {
        if let (Some(d), Some(h)) = (c.via_diameter, c.via_drill) {
            w.rule(
                kind::ROUTING_VIAS,
                &format!("RoutingVias_{}", c.name),
                &scope_class(&c.name),
                "All",
                i + 1,
                |p| via_rule(p, d, h),
            );
        }
    }
    let (d, h) = (
        def.and_then(|c| c.via_diameter).unwrap_or(600_000),
        def.and_then(|c| c.via_drill).unwrap_or(300_000),
    );
    w.rule(kind::ROUTING_VIAS, "RoutingVias", "All", "All", last, |p| {
        via_rule(p, d, h)
    });

    // Differential pairs, where the Default class says how
    if let (Some(dw), Some(dg)) = (
        def.and_then(|c| c.diff_pair_width),
        def.and_then(|c| c.diff_pair_gap),
    ) {
        w.rule(
            kind::DIFF_PAIRS_ROUTING,
            "DiffPairsRouting",
            "All",
            "All",
            1,
            |p| {
                p.set("MAXLIMIT", mil(dg))
                    .set("MINLIMIT", mil(dg))
                    .set("MOSTFREQGAP", mil(dg));
                p.set("TOPLAYER_PREFWIDTH", mil(dw))
                    .set("MAXWIDTH", mil(dw))
                    .set("MINWIDTH", mil(dw));
            },
        );
    }

    // Holes
    let lo = if min_drill > 0 {
        min_drill.min(u.min_hole.max(1))
    } else {
        u.min_hole.min(25_400)
    };
    w.rule(kind::HOLE_SIZE, "HoleSize", "All", "All", 1, |p| {
        p.bool("ABSOLUTEVALUES", true)
            .set("MINLIMIT", mil(lo))
            .set("MAXLIMIT", mil(u.max_hole.max(2_540_000)));
    });
    if let Some(g) = pr.rules.min_hole_to_hole {
        w.rule(
            kind::HOLE_TO_HOLE,
            "HoleToHoleClearance",
            "All",
            "All",
            1,
            |p| {
                p.set("GAP", mil(g)).bool("ALLOWSTACKEDMICROVIAS", false);
            },
        );
    }
    if let Some(r) = pr.rules.min_annular {
        w.rule(
            kind::MINIMUM_ANNULAR_RING,
            "MinimumAnnularRing",
            "All",
            "All",
            1,
            |p| {
                p.set("MINIMUMRING", mil(r));
            },
        );
    }
    if let Some(g) = pr.rules.min_copper_edge {
        w.rule(
            kind::BOARD_OUTLINE,
            "BoardOutlineClearance",
            "All",
            "All",
            1,
            |p| {
                p.set("GAP", mil(g)).set("GENERICCLEARANCE", mil(g));
            },
        );
    }
    if let Some(g) = pr.rules.min_silk_clearance {
        w.rule(
            kind::SILK_TO_SILK,
            "SilkToSilkClearance",
            "All",
            "All",
            1,
            |p| {
                p.set("SILKTOSILKCLEARANCE", mil(g));
            },
        );
    }

    // Mask and paste, from the board's setup
    w.rule(
        kind::SOLDER_MASK_EXPANSION,
        "SolderMaskExpansion",
        "All",
        "All",
        1,
        |p| {
            p.set("EXPANSION", mil(b.mask_expansion))
                .bool("ISTENTINGTOP", false)
                .bool("ISTENTINGBOTTOM", false);
        },
    );
    w.rule(
        kind::PASTE_MASK_EXPANSION,
        "PasteMaskExpansion",
        "All",
        "All",
        1,
        |p| {
            p.set("EXPANSION", mil(b.paste_expansion));
            if b.paste_ratio != 0.0 {
                // Altium's percentage is of the pad area; KiCad's ratio of its side
                let area = ((1.0 + 2.0 * b.paste_ratio).powi(2) - 1.0) * 100.0;
                p.set("PERCENTAGE", format!("{area:.3}"))
                    .set("USEPASTE", "Percent");
            }
        },
    );
    if b.mask_min_width > 0 {
        w.rule(
            kind::MASK_SLIVER,
            "MinimumSolderMaskSliver",
            "All",
            "All",
            1,
            |p| {
                p.set("MINSOLDERMASKWIDTH", mil(b.mask_min_width));
            },
        );
    }

    // How pads join polygons: each zone as KiCad set it
    let mut prio = 1;
    for (zi, z) in b.zones.iter().enumerate() {
        if z.keepout {
            continue;
        }
        for name in polygons.get(zi).into_iter().flatten() {
            w.rule(
                kind::POLYGON_CONNECT,
                &format!("PolygonConnect_{name}"),
                &format!("InNamedPolygon('{name}')"),
                "All",
                prio,
                |p| match z.connect.as_str() {
                    "solid" => {
                        p.set("CONNECTSTYLE", "Direct");
                    }
                    "none" => {
                        p.set("CONNECTSTYLE", "NoConnect");
                    }
                    _ => {
                        p.set("CONNECTSTYLE", "Relief")
                            .set("RELIEFCONDUCTORWIDTH", mil(z.thermal_bridge));
                        p.set("AIRGAPWIDTH", mil(z.thermal_gap))
                            .num("RELIEFENTRIES", 4)
                            .set("POLYGONRELIEFANGLE", "90 Angle");
                    }
                },
            );
            prio += 1;
        }
    }
    w.rule(
        kind::POLYGON_CONNECT,
        "PolygonConnect",
        "All",
        "All",
        prio,
        |p| {
            p.set("CONNECTSTYLE", "Relief")
                .set("RELIEFCONDUCTORWIDTH", mil(500_000))
                .set("AIRGAPWIDTH", mil(500_000));
            p.num("RELIEFENTRIES", 4)
                .set("POLYGONRELIEFANGLE", "90 Angle");
        },
    );

    w.rule(kind::SHORT_CIRCUIT, "ShortCircuit", "All", "All", 1, |p| {
        p.bool("ALLOWED", false);
    });
    w.rule(kind::UNROUTED_NET, "UnRoutedNet", "All", "All", 1, |p| {
        p.bool("CHECKBADCONNECTIONS", false);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rules of a written doc as (kind id, properties)
    fn rules(doc: &Doc) -> Vec<(u16, std::collections::HashMap<String, String>)> {
        let d = &doc.storages["Rules6"].data;
        let mut out = Vec::new();
        let mut i = 0;
        while i < d.len() {
            let id = u16::from_le_bytes([d[i], d[i + 1]]);
            let n = u32::from_le_bytes(d[i + 2..i + 6].try_into().unwrap()) as usize;
            let text = String::from_utf8_lossy(&d[i + 6..i + 6 + n - 1]).into_owned();
            let kv = text
                .split('|')
                .filter_map(|f| f.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            out.push((id, kv));
            i += 6 + n;
        }
        out
    }

    #[test]
    fn classes_get_rules_ahead_of_the_board() {
        let b = Board {
            nets: vec!["".into(), "+5V".into(), "SIG".into()],
            ..Default::default()
        };
        let p = crate::project::parse(
            r#"{"board": {"design_settings": {"rules": {"min_track_width": 0.15, "min_hole_to_hole": 0.25}}},
                "net_settings": {"classes": [{"name": "Default", "clearance": 0.2, "track_width": 0.25, "via_diameter": 0.6, "via_drill": 0.3},
                                             {"name": "pwr", "clearance": 0.3, "track_width": 0.5}],
                                 "netclass_patterns": [{"netclass": "pwr", "pattern": "+*V"}]}}"#,
        )
        .unwrap();
        let mut doc = Doc::default();
        write(&mut doc, &b, Some(&p), &[]);
        let r = rules(&doc);
        let find = |name: &str| {
            r.iter()
                .find(|(_, kv)| kv["NAME"] == name)
                .unwrap_or_else(|| panic!("{name}"))
        };
        let (id, c) = find("Clearance_pwr");
        assert_eq!(
            (*id, c["SCOPE1EXPRESSION"].as_str(), c["PRIORITY"].as_str()),
            (0, "InNetClass('pwr')", "1")
        );
        assert_eq!(c["GAP"], "11.811mil");
        let (_, c) = find("Clearance");
        assert_eq!(
            (
                c["SCOPE1EXPRESSION"].as_str(),
                c["PRIORITY"].as_str(),
                c["GAP"].as_str()
            ),
            ("All", "2", "7.874mil")
        );
        let (id, w) = find("Width_pwr");
        assert_eq!(
            (*id, w["PREFEREDWIDTH"].as_str(), w["MINLIMIT"].as_str()),
            (2, "19.685mil", "5.9055mil")
        );
        assert_eq!(find("HoleToHoleClearance").1["GAP"], "9.8425mil");
        // The class lists its nets
        let c = String::from_utf8_lossy(&doc.storages["Classes6"].data).into_owned();
        assert!(
            c.contains("|NAME=pwr|KIND=0") && c.contains("|M0=+5V|") && !c.contains("SIG"),
            "{c}"
        );
    }
}
