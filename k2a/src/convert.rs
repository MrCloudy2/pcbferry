//! KiCad board -> Altium PcbDoc records.

use crate::altium::{mil, units, Buf, Doc, Patch, Props};
use crate::geom::{circle_through, cross};
use crate::kicad::{Board, Drawing, Footprint, Nm, Pad, PadKind, PadShape, Pt, Shape, Text};
use crate::templates;

/// Altium's y axis points up; KiCad's down. KiCad's page origin goes to this height so every
/// coordinate stays positive, as Altium's own files are.
pub(crate) const Y_FLIP: Nm = 500_000_000;

const NONE16: u16 = 0xffff;

/// How many pen widths KiCad's importer takes off a text's measured width (found by measuring)
const PEN_FACTOR: f64 = 1.25;

/// KiCad's widest pen for a text, as a share of its size (ClampTextPenSize)
const PEN_CLAMP: f64 = 0.25;

const MULTI_LAYER: u8 = 74;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    pub v6: u8,
    pub v7: u32,
}

/// KiCad's technical layers on Altium mechanical layers, with the kind that makes KiCad (and
/// Altium) treat them as the same thing. KiCad's User.n go to Mechanical n, which KiCad's
/// importer reads back as User.n; these sit above them.
pub const MECH: &[(&str, u16, &str)] = &[
    ("F.Fab", 46, "AssemblyTop"),
    ("B.Fab", 47, "AssemblyBottom"),
    ("F.CrtYd", 48, "CourtyardTop"),
    ("B.CrtYd", 49, "CourtyardBottom"),
    ("Dwgs.User", 50, "Dimensions"),
    ("Cmts.User", 51, "AssemblyNotes"),
    ("F.Adhes", 52, "GluePointsTop"),
    ("B.Adhes", 53, "GluePointsBottom"),
    ("Eco1.User", 54, ""),
    ("Eco2.User", 55, ""),
    ("Margin", 56, ""),
];

/// The Altium mechanical layer for a KiCad layer name, if it goes on one
pub fn mech_of(name: &str) -> Option<(u16, &'static str)> {
    if let Some(n) = name
        .strip_prefix("User.")
        .and_then(|n| n.parse::<u16>().ok())
    {
        return (1..=45).contains(&n).then_some((n, ""));
    }
    MECH.iter().find(|m| m.0 == name).map(|m| (m.1, m.2))
}

pub(crate) fn mech_layer(n: u16) -> Layer {
    // Mechanical 17 and up exist only in the newer layer ids; the old byte stays at 16
    Layer {
        v6: 56 + n.min(16) as u8,
        v7: 0x0102_0000 + n as u32,
    }
}

#[derive(Default, Debug)]
pub struct Report {
    pub warnings: Vec<String>,
    pub counts: Vec<(&'static str, usize)>,
}

impl Report {
    fn warn(&mut self, w: String) {
        if !self.warnings.contains(&w) {
            self.warnings.push(w);
        }
    }
}

struct Ctx<'a> {
    b: &'a Board,
    doc: Doc,
    report: Report,
}

fn ax(x: Nm) -> i32 {
    units(x)
}

fn ay(y: Nm) -> i32 {
    units(Y_FLIP - y)
}

fn net16(net: usize) -> u16 {
    if net == 0 {
        NONE16
    } else {
        (net - 1) as u16
    }
}

impl Ctx<'_> {
    fn layer(&mut self, name: &str) -> Option<Layer> {
        let b = self.b;
        let l = match name {
            "F.Cu" => Layer {
                v6: 1,
                v7: 0x0100_0001,
            },
            "B.Cu" => Layer {
                v6: 32,
                v7: 0x0100_ffff,
            },
            "F.SilkS" => Layer {
                v6: 33,
                v7: 0x0103_0006,
            },
            "B.SilkS" => Layer {
                v6: 34,
                v7: 0x0103_0007,
            },
            "F.Paste" => Layer {
                v6: 35,
                v7: 0x0103_0008,
            },
            "B.Paste" => Layer {
                v6: 36,
                v7: 0x0103_0009,
            },
            "F.Mask" => Layer {
                v6: 37,
                v7: 0x0103_000a,
            },
            "B.Mask" => Layer {
                v6: 38,
                v7: 0x0103_000b,
            },
            n if n.starts_with("In") && n.ends_with(".Cu") => {
                let k = b.copper.iter().position(|c| c == n)?;
                if k > 30 {
                    self.report
                        .warn(format!("{n}: Altium has 30 inner copper layers; left out"));
                    return None;
                }
                Layer {
                    v6: 1 + k as u8,
                    v7: 0x0100_0001 + k as u32,
                }
            }
            n => match mech_of(n) {
                Some((m, _)) => mech_layer(m),
                None => {
                    if n != "Edge.Cuts" {
                        self.report.warn(format!(
                            "objects on {n} have no Altium layer here; left out"
                        ));
                    }
                    return None;
                }
            },
        };
        Some(l)
    }
}

/// Track record (Tracks6), the 49-byte form Altium writes today
#[allow(clippy::too_many_arguments)]
fn track(l: Layer, a: Pt, b: Pt, width: Nm, net: usize, component: u16, keepout: bool) -> Vec<u8> {
    let mut s = Buf::default();
    s.u8(l.v6).u8(0x0c).u8(if keepout { 2 } else { 0 });
    s.u16(net16(net))
        .u16(NONE16)
        .u16(component)
        .u32(0xffff_ffff);
    s.i32(ax(a.x))
        .i32(ay(a.y))
        .i32(ax(b.x))
        .i32(ay(b.y))
        .i32(units(width));
    s.u16(0)
        .u8(0)
        .u32(0)
        .u8(0)
        .u32(l.v7)
        .u8(0)
        .bytes(&[0, 0, 0]);
    let mut r = Buf::default();
    r.u8(4).block(&s.0);
    r.0
}

#[allow(clippy::too_many_arguments)]
/// Arc record (Arcs6), 60 bytes. Altium arcs run counter-clockwise (y up) from start to end angle.
fn arc(
    l: Layer,
    c: (f64, f64),
    r: f64,
    sa: f64,
    ea: f64,
    width: Nm,
    net: usize,
    component: u16,
    keepout: bool,
) -> Vec<u8> {
    let mut s = Buf::default();
    s.u8(l.v6).u8(0x0c).u8(if keepout { 2 } else { 0 });
    s.u16(net16(net))
        .u16(NONE16)
        .u16(component)
        .u32(0xffff_ffff);
    s.i32((c.0 / 2.54).round() as i32)
        .i32(((Y_FLIP as f64 - c.1) / 2.54).round() as i32);
    s.i32((r / 2.54).round() as i32)
        .f64(sa)
        .f64(ea)
        .i32(units(width));
    s.u16(0).u8(0).u32(0).u32(l.v7).u8(0).bytes(&[0, 0, 0]);
    let mut rec = Buf::default();
    rec.u8(1).block(&s.0);
    rec.0
}

/// Angles of an arc through start, mid, end (KiCad frame) as Altium wants them
fn arc_angles(s: Pt, m: Pt, e: Pt) -> Option<((f64, f64), f64, f64, f64)> {
    let (cx, cy, r) = circle_through(s, m, e)?;
    // In Altium's frame (y up)
    let up = |p: Pt| (p.x as f64, -(p.y as f64));
    let c = (cx, -cy);
    let ang = |p: Pt| {
        let (x, y) = up(p);
        (y - c.1).atan2(x - c.0).to_degrees().rem_euclid(360.0)
    };
    let ccw = cross(up(s), up(m), up(e)) > 0.0;
    let (sa, ea) = if ccw {
        (ang(s), ang(e))
    } else {
        (ang(e), ang(s))
    };
    Some(((cx, cy), r, sa, ea))
}

impl Ctx<'_> {
    fn push_line(&mut self, l: Layer, a: Pt, b: Pt, w: Nm, net: usize, comp: u16) {
        self.doc
            .storage("Tracks6")
            .push(&track(l, a, b, w, net, comp, false));
    }

    #[allow(clippy::too_many_arguments)]
    fn push_arc3(&mut self, l: Layer, s: Pt, m: Pt, e: Pt, w: Nm, net: usize, comp: u16) {
        match arc_angles(s, m, e) {
            Some((c, r, sa, ea)) => self
                .doc
                .storage("Arcs6")
                .push(&arc(l, c, r, sa, ea, w, net, comp, false)),
            None => self.push_line(l, s, e, w, net, comp),
        }
    }

    /// A graphic on a non-copper or copper layer, as tracks, arcs or a filled region
    fn drawing(&mut self, d: &Drawing, comp: u16) {
        let Some(l) = self.layer(&d.layer) else {
            return;
        };
        let w = d.width;
        match &d.shape {
            Shape::Line { a, b } => self.push_line(l, *a, *b, w, d.net, comp),
            Shape::Arc { start, mid, end } => self.push_arc3(l, *start, *mid, *end, w, d.net, comp),
            Shape::Circle { center, radius } => {
                if d.filled {
                    // A filled circle is an arc as wide as the circle
                    let c = (center.x as f64, center.y as f64);
                    let r = *radius as f64 + w as f64 / 2.0;
                    self.doc.storage("Arcs6").push(&arc(
                        l,
                        (c.0, c.1),
                        r / 2.0,
                        0.0,
                        360.0,
                        (r).round() as Nm,
                        d.net,
                        comp,
                        false,
                    ));
                } else {
                    let c = (center.x as f64, center.y as f64);
                    self.doc.storage("Arcs6").push(&arc(
                        l,
                        c,
                        *radius as f64,
                        0.0,
                        360.0,
                        w,
                        d.net,
                        comp,
                        false,
                    ));
                }
            }
            Shape::Rect { a, b } => {
                let pts = vec![*a, Pt::new(b.x, a.y), *b, Pt::new(a.x, b.y)];
                self.outline_or_region(l, &pts, w, d.filled, d.net, comp);
            }
            Shape::Poly(pts) => self.outline_or_region(l, pts, w, d.filled, d.net, comp),
        }
    }

    fn outline_or_region(
        &mut self,
        l: Layer,
        pts: &[Pt],
        w: Nm,
        filled: bool,
        net: usize,
        comp: u16,
    ) {
        if pts.len() < 2 {
            return;
        }
        if filled && pts.len() >= 3 {
            self.region(l, pts, net, comp, RegionKind::Plain);
        }
        if w > 0 || !filled {
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if a != b {
                    self.push_line(l, a, b, w, net, comp);
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum RegionKind {
    Plain,
    BoardCutout,
    /// A keep-out with Altium's restriction bits
    Keepout(u8),
}

impl Ctx<'_> {
    /// A region: one record in ShapeBasedRegions6 (what KiCad and Altium draw) and its twin in
    /// Regions6, as Altium writes both
    fn region(&mut self, l: Layer, pts: &[Pt], net: usize, comp: u16, kind: RegionKind) {
        region(&mut self.doc, l, pts, net, comp, kind);
    }
}

pub(crate) fn region(doc: &mut Doc, l: Layer, pts: &[Pt], net: usize, comp: u16, kind: RegionKind) {
    {
        let mut props = Props::new();
        props.set("V7_LAYER", layer_name_v7(l));
        props.num("KIND", 0);
        props.bool("ISBOARDCUTOUT", kind == RegionKind::BoardCutout);
        props.bool("ISSHAPEBASED", false);
        props.num("SUBPOLYINDEX", -1);
        props.num("UNIONINDEX", 0);
        let keepout = match kind {
            RegionKind::Keepout(bits) => {
                props.num("KEEPOUTRESTRICTIONS", bits);
                true
            }
            _ => false,
        };
        let head = |b: &mut Buf| {
            b.u8(l.v6).u8(0x0c).u8(if keepout { 2 } else { 0 });
            b.u16(net16(net)).u16(NONE16).u16(comp);
            b.bytes(&[0xff, 0xff, 0xff, 0xff, 0]).u16(0).u16(0);
            b.bytes(&props.encode());
        };
        // Shape-based: n vertices plus a closing one, each with arc data
        let mut s = Buf::default();
        head(&mut s);
        s.u32(pts.len() as u32);
        for p in pts.iter().chain(std::iter::once(&pts[0])) {
            s.u8(0)
                .i32(ax(p.x))
                .i32(ay(p.y))
                .i32(0)
                .i32(0)
                .i32(0)
                .f64(0.0)
                .f64(0.0);
        }
        let mut r = Buf::default();
        r.u8(11).block(&s.0);
        doc.storage("ShapeBasedRegions6").push(&r.0);

        let mut s = Buf::default();
        head(&mut s);
        s.u32(pts.len() as u32);
        for p in pts {
            s.f64(ax(p.x) as f64).f64(ay(p.y) as f64);
        }
        let mut r = Buf::default();
        r.u8(11).block(&s.0);
        doc.storage("Regions6").push(&r.0);
    }
}

fn layer_name_v7(l: Layer) -> String {
    match l.v6 {
        1 => "TOP".into(),
        32 => "BOTTOM".into(),
        2..=31 => format!("MID{}", l.v6 - 1),
        33 => "TOPOVERLAY".into(),
        34 => "BOTTOMOVERLAY".into(),
        35 => "TOPPASTE".into(),
        36 => "BOTTOMPASTE".into(),
        37 => "TOPSOLDER".into(),
        38 => "BOTTOMSOLDER".into(),
        56 => "KEEPOUT".into(),
        57..=72 => format!("MECHANICAL{}", l.v7 - 0x0102_0000),
        74 => "MULTILAYER".into(),
        _ => "UNKNOWN".into(),
    }
}

impl Ctx<'_> {
    fn pad(&mut self, p: &Pad, comp: u16) {
        let smd = matches!(p.kind, PadKind::Smd | PadKind::Connect);
        let on_copper = p.on("F.Cu") || p.on("B.Cu") || p.layers.iter().any(|l| l.ends_with(".Cu"));
        if smd && !on_copper {
            // An aperture: Altium has pads on a single non-copper layer for this
            let mut ls = Vec::new();
            for l in &p.layers {
                match l.as_str() {
                    "*.Mask" => ls.extend(["F.Mask".to_string(), "B.Mask".to_string()]),
                    "*.Paste" => ls.extend(["F.Paste".to_string(), "B.Paste".to_string()]),
                    _ => ls.push(l.clone()),
                }
            }
            for l in ls {
                if let Some(layer) = self.layer(&l) {
                    self.pad_record(p, comp, layer.v6);
                }
            }
            return;
        }
        let back = smd && p.on("B.Cu") && !p.on("F.Cu");
        let layer: u8 = if smd {
            if back {
                32
            } else {
                1
            }
        } else {
            MULTI_LAYER
        };
        if p.shape == PadShape::Custom {
            // The anchor is the pad; the rest is copper of the same net in the component
            let cu: Vec<String> = if smd {
                vec![if back {
                    "B.Cu".to_string()
                } else {
                    "F.Cu".to_string()
                }]
            } else {
                self.b.copper.iter().filter(|c| p.on(c)).cloned().collect()
            };
            for d in &p.primitives {
                for c in &cu {
                    let mut d = d.clone();
                    d.layer = c.clone();
                    d.net = p.net;
                    self.drawing(&d, comp);
                }
            }
        }
        self.pad_record(p, comp, layer);
    }

    fn pad_record(&mut self, p: &Pad, comp: u16, layer: u8) {
        let smd = layer != MULTI_LAYER;
        let back = layer == 32 || layer == 34 || layer == 36 || layer == 38;
        let (shape, alt, corner) = match p.shape {
            PadShape::Circle | PadShape::Oval => (1u8, 1u8, 0u8),
            PadShape::Rect => (2, 2, 0),
            PadShape::RoundRect => (1, 9, (p.rratio * 200.0).round().clamp(0.0, 100.0) as u8),
            PadShape::ChamferedRect => (3, 3, 0),
            PadShape::Custom if p.anchor_circle => (1, 1, 0),
            PadShape::Custom => (2, 2, 0),
            PadShape::Trapezoid => {
                self.report.warn(format!(
                    "pad shape {:?} is written as a rectangle of its size",
                    p.shape
                ));
                (2, 2, 0)
            }
        };

        // A slot: hole size is the short side, slot size the long one
        let (hole, slot, slot_rot) = if p.drill.x == p.drill.y || !p.oval_drill {
            (p.drill.x, 0, 0.0)
        } else if p.drill.x > p.drill.y {
            (p.drill.y, p.drill.x, 0.0)
        } else {
            (p.drill.x, p.drill.y, 90.0)
        };
        let size = if p.kind == PadKind::Npth && (p.size.x == 0 || p.size.y == 0) {
            p.drill
        } else {
            p.size
        };

        let mut b = templates::PAD5;
        let net = net16(p.net);
        let mut flags1 = 0x0cu8;
        if !p.on(if back { "B.Mask" } else { "F.Mask" }) && (smd || !p.on("F.Mask")) {
            flags1 |= 0x20; // tented top
        }
        if !smd && !p.on("B.Mask") {
            flags1 |= 0x40;
        }
        if smd && back && !p.on("B.Mask") {
            flags1 = (flags1 & !0x20) | 0x40;
        }
        let mask_manual = p.mask_margin.is_some();
        // No paste on an SMD pad: Altium has no switch for it; a paste expansion that swallows
        // the pad does the same
        let paste_on = p.on(if back { "B.Paste" } else { "F.Paste" });
        let (paste_manual, paste) = match p.paste_margin {
            Some(m) => (true, m),
            None if smd && !paste_on => (true, -(size.x.max(size.y))),
            None => (false, 0),
        };
        Patch(&mut b)
            .u8(0, layer)
            .u8(1, flags1)
            .u8(2, 0)
            .u16(3, net)
            .u16(5, NONE16)
            .u16(7, comp)
            .u32(9, 0xffff_ffff)
            .i32(13, ax(p.at.x))
            .i32(17, ay(p.at.y));
        for k in 0..3 {
            Patch(&mut b)
                .i32(21 + 8 * k, units(size.x))
                .i32(25 + 8 * k, units(size.y))
                .u8(49 + k, shape);
        }
        Patch(&mut b)
            .i32(45, units(hole))
            .f64(52, p.rot)
            .u8(60, (p.kind != PadKind::Npth) as u8)
            .u8(62, 0)
            .i32(86, units(paste))
            .i32(90, units(p.mask_margin.unwrap_or(0)))
            .u8(101, if paste_manual { 2 } else { 1 })
            .u8(102, if mask_manual { 2 } else { 1 })
            .f64(106, 0.0)
            // The pad's copper as Altium Designer writes it, whatever the layer count: an SMD
            // pad's layer as the low half of its layer stack id (top 0x0001, mid n 0x00nn,
            // bottom 0xFFFF), a through-hole pad 0x000F with byte 116 = 3. AD26 reads an SMD
            // 0x0020 as a Mid-Layer 31 (adds it to the stack and drops the pad), and a
            // through-hole 0x0001 as a pad on the top layer only, which no bottom track reaches
            .u16(
                114,
                match layer {
                    _ if !smd => 0x000F,
                    2..=31 => layer as u16,
                    32 | 34 | 36 | 38 => 0xFFFF,
                    _ => 1,
                },
            )
            .u8(116, if smd { 0 } else { 3 })
            // KiCad's importer reads byte 117 as a "from layer", but Altium Designer writes 1 here
            // on every pad (top, bottom and through-hole alike) and AD26 refuses the whole Pads6
            // storage ("Catastrophic failure") when it is anything else
            .u8(117, 1);

        let needs6 = corner != 0 || slot != 0 || p.offset != Pt::default() || shape != alt;
        let mut sub6 = Buf::default();
        if needs6 {
            for _ in 0..29 {
                sub6.i32(units(size.x));
            }
            for _ in 0..29 {
                sub6.i32(units(size.y));
            }
            for _ in 0..29 {
                sub6.u8(shape);
            }
            sub6.u8(0);
            sub6.u8(if slot != 0 { 2 } else { 0 })
                .i32(units(slot))
                .f64(slot_rot);
            for _ in 0..32 {
                sub6.i32(units(p.offset.x));
            }
            for _ in 0..32 {
                sub6.i32(-units(p.offset.y));
            }
            sub6.u8(0);
            for _ in 0..32 {
                sub6.u8(alt);
            }
            for _ in 0..32 {
                sub6.u8(corner);
            }
        }

        let mut r = Buf::default();
        r.u8(2);
        let mut name = Buf::default();
        name.pascal(&p.number);
        r.block(&name.0)
            .block(&[0])
            .block(b"\x04|&|0")
            .block(&[0])
            .block(&b)
            .block(&sub6.0);
        self.doc.storage("Pads6").push(&r.0);
    }
}

impl Ctx<'_> {
    fn via(&mut self, v: &crate::kicad::Via) {
        let (Some(top), Some(bot)) = (self.layer(&v.top.clone()), self.layer(&v.bottom.clone()))
        else {
            return;
        };
        let mut b = templates::VIA;
        let mut flags1 = 0x0cu8;
        if v.tented_front {
            flags1 |= 0x20;
        }
        if v.tented_back {
            flags1 |= 0x40;
        }
        Patch(&mut b)
            .u8(0, MULTI_LAYER)
            .u8(1, flags1)
            .u8(2, 0)
            .u16(3, net16(v.net))
            .i32(13, ax(v.at.x))
            .i32(17, ay(v.at.y))
            .i32(21, units(v.size))
            .i32(25, units(v.drill))
            .u8(29, top.v6)
            .u8(30, bot.v6)
            .u8(66, 0) // mask expansion from the rules, not manual
            .u8(74, 0); // simple: one diameter on every layer
        for k in 0..32 {
            Patch(&mut b).i32(75 + 4 * k, units(v.size));
        }
        let mut r = Buf::default();
        r.u8(3).block(&b);
        self.doc.storage("Vias6").push(&r.0);
    }

    /// `fp`: the footprint's reference and value, which its texts may refer to
    fn text(
        &mut self,
        t: &Text,
        comp: u16,
        designator: bool,
        comment: bool,
        fp: Option<(&str, &str)>,
    ) {
        let Some(l) = self.layer(&t.layer) else {
            return;
        };
        // KiCad's text variables for the footprint are Altium's special strings; KiCad measures
        // (and Altium draws) them as their value
        let (written, shown) = match fp {
            Some((r, v)) => (
                t.text
                    .replace("${REFERENCE}", ".Designator")
                    .replace("${VALUE}", ".Comment"),
                t.text.replace("${REFERENCE}", r).replace("${VALUE}", v),
            ),
            None => (t.text.clone(), t.text.clone()),
        };
        // KiCad's markup (~{overbar}, ^{super}, _{sub}) has no Altium form: its plain text
        let (written, shown) = (strip_markup(&written), strip_markup(&shown));
        // Altium's glyphs are as wide as they are high. On copper a KiCad text narrower than it
        // is high would come out wider than in KiCad, and its copper would reach what KiCad
        // keeps clear of it (Altium's DRC flags shorts on pic_programmer), so there the text
        // is as high as KiCad's glyphs are wide: its copper stays inside KiCad's
        let copper = (1..=32).contains(&l.v6);
        let height = if copper && t.width > 0 && t.width < t.height {
            t.width
        } else {
            t.height
        };
        let t = &Text {
            text: written,
            height,
            ..t.clone()
        };
        let mut b = templates::TEXT;
        // Altium stores the lower left corner of a text's box, and (since AD19) how the text is
        // justified in it; KiCad its anchor. Written with KiCad's justification, the corner
        // worked out as KiCad's importer works back from it: the box as wide as the text in
        // KiCad's stroke font at Altium's square glyphs, less the pen, and KiCad's baseline nudge
        // KiCad draws no pen wider than a share of the text size
        let pen = t.thickness.min((t.height as f64 * PEN_CLAMP).round() as Nm);
        // Altium text has a height only; its glyphs are as wide as they are high
        let w = crate::font::text_width(&shown, t.height) - (PEN_FACTOR * pen as f64).round() as Nm;
        let w = if t.mirrored { -w } else { w };
        let h = t.height;
        let dx = match t.h_justify {
            -1 => 0,
            1 => -w,
            _ => -w / 2,
        };
        let (dy, nudge) = match t.v_justify {
            -1 => (h, -0.0407),
            1 => (0, 0.1225),
            _ => (h / 2, 0.0355),
        };
        let local = Pt::new(dx, dy - (h as f64 * nudge).round() as Nm);
        let off = crate::kicad::rotate(local, t.rot);
        let at = Pt::new(t.at.x + off.x, t.at.y + off.y);
        // ALTIUM_TEXT_POSITION: 1-3 left (top, centre, bottom), 4-6 centre, 7-9 right
        let justification = (3 * (t.h_justify + 1) + (t.v_justify + 1) + 1) as u8;
        let index = self.doc.storage("Texts6").count;
        Patch(&mut b)
            .u8(0, l.v6)
            .u16(7, comp)
            .i32(13, ax(at.x))
            .i32(17, ay(at.y))
            .i32(21, units(t.height))
            .u16(25, 1) // default stroke font
            .f64(27, t.rot.rem_euclid(360.0))
            .u8(35, t.mirrored as u8)
            .i32(36, units(t.thickness))
            .u8(40, comment as u8)
            .u8(41, designator as u8)
            .u8(43, 0) // stroke font, not TrueType
            .u8(110, 0)
            .u32(115, index)
            .u8(123, 0)
            .u8(132, justification)
            .u8(160, 0)
            .u32(226, l.v7)
            .u8(230, 0)
            .u8(240, 1) // the justification is meant
            .i32(244, ax(at.x))
            .i32(248, ay(at.y));
        // The text itself, in full, in the wide-string table
        // An empty string has not even its terminator (KiCad's reader and Altium agree)
        let mut w16 = Buf::default();
        if !t.text.is_empty() {
            for ch in t.text.replace('\n', "\r\n").encode_utf16() {
                w16.u16(ch);
            }
            w16.u16(0);
        }
        let ws = self.doc.storage("WideStrings6");
        ws.data.extend(index.to_le_bytes());
        ws.data.extend((w16.0.len() as u32).to_le_bytes());
        ws.data.extend(&w16.0);
        ws.count += 1;
        let mut r = Buf::default();
        let mut s = Buf::default();
        s.pascal(&t.text);
        r.u8(5).block(&b).block(&s.0);
        self.doc.storage("Texts6").push(&r.0);
    }
}

fn component(fp: &Footprint, index: usize) -> Vec<u8> {
    let (lib, name) = match fp.lib_id.split_once(':') {
        Some((l, n)) => (l, n),
        None => ("", fp.lib_id.as_str()),
    };
    let mut p = Props::new();
    p.set("LAYER", if fp.back { "BOTTOM" } else { "TOP" });
    p.bool("LOCKED", fp.locked);
    // Altium's component type: Standard, Standard (No BOM) or Graphical (board only)
    let kind = match (fp.board_only, fp.exclude_from_bom) {
        (true, _) => 2,
        (false, true) => 5,
        _ => 0,
    };
    if kind != 0 {
        p.num("COMPONENTKIND", kind);
    }
    p.bool("POLYGONOUTLINE", false);
    p.bool("USERROUTED", true);
    p.bool("KEEPOUT", false);
    p.bool("PRIMITIVELOCK", true);
    p.set("X", mil(fp.place.at.x));
    p.set("Y", mil(Y_FLIP - fp.place.at.y));
    p.set("PATTERN", name);
    p.bool(
        "NAMEON",
        fp.reference_text
            .as_ref()
            .map(|t| t.visible)
            .unwrap_or(true),
    );
    p.bool(
        "COMMENTON",
        fp.value_text.as_ref().map(|t| t.visible).unwrap_or(false),
    );
    p.num("GROUPNUM", 0);
    p.num("COUNT", 0);
    p.set("ROTATION", format!("{:.6}", fp.place.rot.rem_euclid(360.0)));
    p.num("NAMEAUTOPOSITION", 0);
    p.num("COMMENTAUTOPOSITION", 0);
    p.num("UNIONINDEX", 0);
    p.set("SOURCEDESIGNATOR", fp.reference.as_str());
    p.set(
        "SOURCEFOOTPRINTLIBRARY",
        if lib.is_empty() {
            String::new()
        } else {
            format!("{lib}.PcbLib")
        },
    );
    p.set("FOOTPRINTDESCRIPTION", "");
    p.set("COMMENT", fp.value.as_str());
    p.set("UNIQUEID", unique_id(index));
    p.encode()
}

/// Altium's 8-letter unique ids, derived from the position so a rerun gives the same file
pub(crate) fn unique_id(i: usize) -> String {
    let mut x = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5bd1_e995;
    (0..8)
        .map(|_| {
            x ^= x >> 13;
            x = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
            (b'A' + (x % 26) as u8) as char
        })
        .collect()
}

impl Ctx<'_> {
    fn board6(&mut self) {
        let b = self.b;
        let mut p = Props::new();
        p.set("FILENAME", "");
        p.set("KIND", "Protel_Advanced_PCB");
        p.set("VERSION", "3.00");
        let bb = outline_bbox(b);
        if let Some((min, max)) = bb {
            p.set("ORIGINX", mil(min.x));
            p.set("ORIGINY", mil(Y_FLIP - max.y));
        }
        p.set("SHEETX", mil(0));
        p.set("SHEETY", mil(Y_FLIP - 297_000_000));
        p.set("SHEETWIDTH", mil(420_000_000));
        p.set("SHEETHEIGHT", mil(297_000_000));
        p.set("DISPLAYUNIT", "1");

        // Board outline: the largest closed loop on Edge.Cuts
        let loops = crate::outline::loops(b);
        if let Some(main) = loops.first() {
            let mut n = 0usize;
            for v in main.iter() {
                let k = n.to_string();
                match v {
                    crate::outline::Vertex::Line(pt) => {
                        p.num(&format!("KIND{k}"), 0);
                        p.set(&format!("VX{k}"), mil(pt.x));
                        p.set(&format!("VY{k}"), mil(Y_FLIP - pt.y));
                        p.set(&format!("CX{k}"), mil(0));
                        p.set(&format!("CY{k}"), mil(0));
                        p.num(&format!("SA{k}"), "0.000");
                        p.num(&format!("EA{k}"), "0.000");
                        p.set(&format!("R{k}"), mil(0));
                    }
                    crate::outline::Vertex::Arc { start, mid, end } => {
                        let Some(((cx, cy), r, sa, ea)) = arc_angles(*start, *mid, *end) else {
                            continue;
                        };
                        p.num(&format!("KIND{k}"), 1);
                        p.set(&format!("VX{k}"), mil(start.x));
                        p.set(&format!("VY{k}"), mil(Y_FLIP - start.y));
                        p.set(&format!("CX{k}"), mil(cx.round() as Nm));
                        p.set(&format!("CY{k}"), mil(Y_FLIP - cy.round() as Nm));
                        p.set(&format!("SA{k}"), format!("{sa:.6}"));
                        p.set(&format!("EA{k}"), format!("{ea:.6}"));
                        p.set(&format!("R{k}"), mil(r.round() as Nm));
                    }
                }
                n += 1;
            }
            // Altium closes the outline by repeating its first point as the last vertex; without
            // it Altium Designer drops the last edge (a rectangle comes out a triangle)
            let first = match main.first() {
                Some(crate::outline::Vertex::Line(pt)) => Some(*pt),
                Some(crate::outline::Vertex::Arc { start, .. }) => Some(*start),
                None => None,
            };
            if let Some(pt) = first.filter(|_| n > 0) {
                let k = n.to_string();
                p.num(&format!("KIND{k}"), 0);
                p.set(&format!("VX{k}"), mil(pt.x));
                p.set(&format!("VY{k}"), mil(Y_FLIP - pt.y));
                p.set(&format!("CX{k}"), mil(0));
                p.set(&format!("CY{k}"), mil(0));
                p.num(&format!("SA{k}"), "0.000");
                p.num(&format!("EA{k}"), "0.000");
                p.set(&format!("R{k}"), mil(0));
            }
        } else {
            self.report
                .warn("no closed board outline on Edge.Cuts".into());
        }

        // Layer stack: every Altium layer id 1..82, copper chained TOP -> MIDn -> BOTTOM
        let ncu = b.copper.len().max(2);
        let mids: Vec<u8> = (2..(ncu as u8)).collect();
        let chain: Vec<u8> = std::iter::once(1)
            .chain(mids.iter().copied())
            .chain(std::iter::once(32))
            .collect();
        let diel = if ncu > 1 {
            b.thickness / (ncu as i64 - 1)
        } else {
            b.thickness
        };
        for id in 1u8..=82 {
            let k = format!("LAYER{id}");
            let pos = chain.iter().position(|&c| c == id);
            let name = match id {
                1 => self.user_name("F.Cu", "Top Layer"),
                32 => self.user_name("B.Cu", "Bottom Layer"),
                2..=31 => match b.copper.get(id as usize - 1) {
                    Some(n) if pos.is_some() => self.user_name(n, &format!("Mid-Layer {}", id - 1)),
                    _ => format!("Mid-Layer {}", id - 1),
                },
                33 => "Top Overlay".into(),
                34 => "Bottom Overlay".into(),
                35 => "Top Paste".into(),
                36 => "Bottom Paste".into(),
                37 => "Top Solder".into(),
                38 => "Bottom Solder".into(),
                39..=54 => format!("Internal Plane {}", id - 38),
                55 => "Drill Guide".into(),
                56 => "Keep-Out Layer".into(),
                57..=72 => self.mech_name(id as u16 - 56),
                73 => "Drill Drawing".into(),
                74 => "Multi-Layer".into(),
                _ => format!("Layer {id}"),
            };
            p.set(&format!("{k}NAME"), name);
            let (prev, next) = match pos {
                Some(i) => (
                    if i == 0 { 0 } else { chain[i - 1] },
                    if i + 1 == chain.len() {
                        0
                    } else {
                        chain[i + 1]
                    },
                ),
                None => (0, 0),
            };
            p.num(&format!("{k}PREV"), prev);
            p.num(&format!("{k}NEXT"), next);
            let mech = (57..=72).contains(&id) && self.mech_used(id as u16 - 56);
            p.bool(&format!("{k}MECHENABLED"), mech);
            p.set(&format!("{k}COPTHICK"), "1.4mil");
            // Dielectric under each copper layer but the last: core and prepreg alternating
            let dieltype = match pos {
                Some(i) if next != 0 => 1 + i % 2,
                _ => 0,
            };
            p.num(&format!("{k}DIELTYPE"), dieltype);
            p.set(&format!("{k}DIELCONST"), "4.500");
            p.set(&format!("{k}DIELHEIGHT"), mil(diel));
            p.set(&format!("{k}DIELMATERIAL"), "FR-4");
            if mech {
                if let Some(kind) = self.mech_kind(id as u16 - 56) {
                    p.set(&format!("{k}MECHKIND"), kind);
                }
            }
        }
        // Mechanical 17 and up: the V7 layer list
        for (i, m) in (17u16..=crate::models::BODY_BOTTOM).enumerate() {
            let k = format!("LAYERV7_{i}");
            p.num(&format!("{k}LAYERID"), 0x0102_0000u32 + m as u32);
            p.set(&format!("{k}NAME"), self.mech_name(m));
            p.num(&format!("{k}PREV"), 0).num(&format!("{k}NEXT"), 0);
            p.bool(&format!("{k}MECHENABLED"), self.mech_used(m));
            if let Some(kind) = self.mech_kind(m) {
                p.set(&format!("{k}MECHKIND"), kind);
            }
            p.set(&format!("{k}COPTHICK"), "1.4mil")
                .num(&format!("{k}DIELTYPE"), 0);
            p.set(&format!("{k}DIELCONST"), "4.500")
                .set(&format!("{k}DIELHEIGHT"), "0mil");
            p.set(&format!("{k}DIELMATERIAL"), "FR-4");
        }
        self.layer_stack_v8(&mut p, &chain, diel);
        p.num("LAYERSETSCOUNT", 1);
        self.doc.storage("Board6").push(&p.encode());
    }

    /// The layer stack as Altium Designer 19 and later keep it: a master stack, one sub-stack,
    /// and the stack's layers top to bottom (paste, overlay, mask, copper with dielectric between,
    /// mask, overlay, paste), then the mechanical layers in use. AD26 reads layers only from
    /// these; a Board6 without LAYERMASTERSTACK_V8ID fails to load ("Catastrophic failure"), and
    /// one with the master stack but no LAYER_V8 list loads with no layers at all.
    fn layer_stack_v8(&self, p: &mut Props, chain: &[u8], diel: Nm) {
        let sub = guid("substack 0");
        p.num("LAYERMASTERSTACK_V8STYLE", 0)
            .set("LAYERMASTERSTACK_V8ID", guid("master stack"))
            .set("LAYERMASTERSTACK_V8NAME", "Master layer stack")
            .bool("LAYERMASTERSTACK_V8SHOWTOPDIELECTRIC", false)
            .bool("LAYERMASTERSTACK_V8SHOWBOTTOMDIELECTRIC", false)
            .bool("LAYERMASTERSTACK_V8ISFLEX", false);
        p.set("LAYERSUBSTACK_V8_0ID", sub.clone())
            .set("LAYERSUBSTACK_V8_0NAME", "Board Layer Stack")
            .bool("LAYERSUBSTACK_V8_0SHOWTOPDIELECTRIC", false)
            .bool("LAYERSUBSTACK_V8_0SHOWBOTTOMDIELECTRIC", false)
            .bool("LAYERSUBSTACK_V8_0ISFLEX", false)
            .bool("LAYERSUBSTACK_V8_0SERVICE", false)
            .bool("LAYERSUBSTACK_V8_0USEDBYPRIMS", false)
            .num("LAYERSUBSTACK_V8_0TYPE", 1);

        enum L {
            Plain,
            Mask,
            Copper(bool),
            Dielectric(usize),
        }
        // (name, V8 layer id, kind); copper ids: top 0x01000001, mid n 0x01000000+n, bottom
        // 0x0100FFFF; dielectrics 0x01040000+n; paste/overlay/mask 0x01030006..0x0103000B
        let mut stack: Vec<(String, u32, L)> = vec![
            ("Top Paste".into(), 0x0103_0008, L::Plain),
            ("Top Overlay".into(), 0x0103_0006, L::Plain),
            ("Top Solder".into(), 0x0103_000A, L::Mask),
        ];
        for (i, &id) in chain.iter().enumerate() {
            let (name, v8) = match id {
                1 => (self.user_name("F.Cu", "Top Layer"), 0x0100_0001),
                32 => (self.user_name("B.Cu", "Bottom Layer"), 0x0100_FFFF),
                n => (
                    match self.b.copper.get(n as usize - 1) {
                        Some(c) => self.user_name(c, &format!("Mid-Layer {}", n - 1)),
                        None => format!("Mid-Layer {}", n - 1),
                    },
                    0x0100_0000 + n as u32,
                ),
            };
            stack.push((name, v8, L::Copper(i == 0)));
            if i + 1 < chain.len() {
                stack.push((
                    format!("Dielectric {}", i + 1),
                    0x0104_0001 + i as u32,
                    L::Dielectric(i),
                ));
            }
        }
        stack.extend([
            ("Bottom Solder".into(), 0x0103_000B, L::Mask),
            ("Bottom Overlay".into(), 0x0103_0007, L::Plain),
            ("Bottom Paste".into(), 0x0103_0009, L::Plain),
        ]);
        let mut n = 0usize;
        for (name, id, kind) in &stack {
            let k = format!("LAYER_V8_{n}");
            p.num(&format!("{k}_{sub}CONTEXT"), 0)
                .bool(&format!("{k}_{sub}USEDBYPRIMS"), false)
                .set(&format!("{k}ID"), guid(&format!("layer {id}")))
                .set(&format!("{k}NAME"), name.clone())
                .num(&format!("{k}LAYERID"), id)
                .bool(&format!("{k}USEDBYPRIMS"), true);
            match kind {
                L::Plain => {}
                L::Mask => {
                    p.num(&format!("{k}DIELTYPE"), 3)
                        .set(&format!("{k}DIELCONST"), "3.500")
                        .set(&format!("{k}DIELHEIGHT"), "0.4mil")
                        .set(&format!("{k}DIELMATERIAL"), "Solder Resist")
                        .set(&format!("{k}COVERLAY_EXPANSION"), "0mil");
                }
                L::Copper(top) => {
                    let bottom = *id == 0x0100_FFFF;
                    p.set(&format!("{k}COPTHICK"), "1.4mil").num(
                        &format!("{k}COMPONENTPLACEMENT"),
                        if *top {
                            1
                        } else if bottom {
                            2
                        } else {
                            0
                        },
                    );
                }
                L::Dielectric(i) => {
                    // core and prepreg alternating, as in the LAYERn records
                    p.num(&format!("{k}DIELTYPE"), 1 + i % 2)
                        .set(&format!("{k}DIELCONST"), "4.500")
                        .set(&format!("{k}DIELHEIGHT"), mil(diel))
                        .set(&format!("{k}DIELMATERIAL"), "FR-4");
                }
            }
            n += 1;
        }
        for m in 1u16..=crate::models::BODY_BOTTOM {
            if !self.mech_used(m) {
                continue;
            }
            let k = format!("LAYER_V8_{n}");
            let id = 0x0102_0000u32 + m as u32;
            p.set(&format!("{k}ID"), guid(&format!("layer {id}")))
                .set(&format!("{k}NAME"), self.mech_name(m))
                .num(&format!("{k}LAYERID"), id)
                .bool(&format!("{k}USEDBYPRIMS"), true)
                .bool(&format!("{k}MECHENABLED"), true);
            if let Some(kind) = self.mech_kind(m) {
                p.set(&format!("{k}MECHKIND"), kind);
            }
            n += 1;
        }
    }

    /// The KiCad layer on mechanical layer m, if any
    fn mech_kicad(&self, m: u16) -> Option<String> {
        if m <= 45 {
            let name = format!("User.{m}");
            return self.b.layer_names.contains_key(&name).then_some(name);
        }
        MECH.iter().find(|x| x.1 == m).map(|x| x.0.to_string())
    }

    fn mech_used(&self, m: u16) -> bool {
        self.mech_kicad(m).is_some()
    }

    fn mech_kind(&self, m: u16) -> Option<&'static str> {
        MECH.iter()
            .find(|x| x.1 == m)
            .map(|x| x.2)
            .filter(|k| !k.is_empty())
    }

    fn mech_name(&self, m: u16) -> String {
        match self.mech_kicad(m) {
            Some(k) => self.user_name(&k, &k),
            None => format!("Mechanical {m}"),
        }
    }

    fn user_name(&self, canonical: &str, fallback: &str) -> String {
        match self.b.layer_names.get(canonical) {
            Some(n) if !n.is_empty() => n.clone(),
            _ if canonical.contains('.') && fallback == canonical => canonical.to_string(),
            _ => fallback.to_string(),
        }
    }
}

/// A GUID in Altium's spelling ({8-4-4-4-12}, upper case), derived from `seed` so that the same
/// board always converts to the same bytes
fn guid(seed: &str) -> String {
    let mut h: [u64; 2] = [0xcbf2_9ce4_8422_2325, 0x9e37_79b9_7f4a_7c15];
    for (j, x) in h.iter_mut().enumerate() {
        for b in seed.bytes().chain(std::iter::once(j as u8)) {
            *x ^= b as u64;
            *x = x.wrapping_mul(0x0100_0000_01b3);
        }
        // final mix, so that similar seeds give unrelated ids
        *x ^= *x >> 33;
        *x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        *x ^= *x >> 33;
    }
    let v = ((h[0] as u128) << 64) | h[1] as u128;
    // version 4 / variant 1 bits, as a random GUID has
    let v = (v & !(0xf << 76) | (0x4 << 76)) & !(0x3 << 62) | (0x2 << 62);
    let s = format!("{v:032X}");
    format!(
        "{{{}-{}-{}-{}-{}}}",
        &s[0..8],
        &s[8..12],
        &s[12..16],
        &s[16..20],
        &s[20..32]
    )
}

fn outline_bbox(b: &Board) -> Option<(Pt, Pt)> {
    let loops = crate::outline::loops(b);
    let main = loops.first()?;
    let mut min = Pt::new(i64::MAX, i64::MAX);
    let mut max = Pt::new(i64::MIN, i64::MIN);
    for v in main {
        for p in v.points() {
            min = Pt::new(min.x.min(p.x), min.y.min(p.y));
            max = Pt::new(max.x.max(p.x), max.y.max(p.y));
        }
    }
    Some((min, max))
}

pub fn convert(
    b: &Board,
    project: Option<&crate::project::Project>,
    board_dir: &std::path::Path,
) -> (Doc, Report) {
    let mut c = Ctx {
        b,
        doc: Doc::default(),
        report: Report::default(),
    };

    // FileHeader as Altium writes it (its length counts characters of a UTF-16 string it cuts off)
    let mut fh = Buf::default();
    fh.u32(0x13);
    for ch in "PCB 5.0 Bi".encode_utf16() {
        fh.u16(ch);
    }
    c.doc.streams.insert("FileHeader".into(), fh.0);
    let mut six = Buf::default();
    six.u32(0x13).pascal("PCB 6.0 Binary File");
    c.doc.streams.insert("FileHeaderSix".into(), six.0);

    // Every storage KiCad's reader expects, empty when there is nothing for it
    for s in [
        "Arcs6",
        "Board6",
        "Classes6",
        "ComponentBodies6",
        "Components6",
        "Dimensions6",
        "Fills6",
        "Nets6",
        "Pads6",
        "Polygons6",
        "Regions6",
        "Rules6",
        "ShapeBasedRegions6",
        "Texts6",
        "Tracks6",
        "Vias6",
        "WideStrings6",
    ] {
        c.doc.storage(s);
    }

    c.board6();

    for (i, name) in b.nets.iter().enumerate().skip(1) {
        let mut p = Props::new();
        p.set("LAYER", "TOP")
            .bool("LOCKED", false)
            .bool("POLYGONOUTLINE", false)
            .bool("USERROUTED", true);
        p.bool("KEEPOUT", false)
            .num("UNIONINDEX", 0)
            .bool("PRIMITIVELOCK", false);
        p.set("NAME", name.as_str())
            .bool("VISIBLE", true)
            .set("UNIQUEID", unique_id(1_000_000 + i));
        c.doc.storage("Nets6").push(&p.encode());
    }

    for (i, fp) in b.footprints.iter().enumerate() {
        c.doc.storage("Components6").push(&component(fp, i));
        let comp = i as u16;
        for p in &fp.pads {
            c.pad(p, comp);
        }
        for d in &fp.drawings {
            c.drawing(d, comp);
        }
        if let Some(t) = &fp.reference_text {
            c.text(t, comp, true, false, Some((&fp.reference, &fp.value)));
        }
        if let Some(t) = &fp.value_text {
            c.text(t, comp, false, true, Some((&fp.reference, &fp.value)));
        }
        for t in &fp.texts {
            c.text(t, comp, false, false, Some((&fp.reference, &fp.value)));
        }
    }

    for t in &b.tracks {
        if let Some(l) = c.layer(&t.layer) {
            c.push_line(l, t.start, t.end, t.width, t.net, NONE16);
        }
    }
    for a in &b.arcs {
        if let Some(l) = c.layer(&a.layer) {
            c.push_arc3(l, a.start, a.mid, a.end, a.width, a.net, NONE16);
        }
    }
    for v in &b.vias {
        c.via(v);
    }
    for d in &b.drawings {
        if d.layer != "Edge.Cuts" {
            c.drawing(d, NONE16);
        }
    }
    for t in &b.texts {
        c.text(t, NONE16, false, false, None);
    }

    // Edge.Cuts loops inside the outline are cut-outs
    let loops = crate::outline::loops(b);
    for lp in loops.iter().skip(1) {
        let pts: Vec<Pt> = lp.iter().flat_map(|v| v.points()).collect();
        if pts.len() >= 3 {
            c.region(
                Layer {
                    v6: 1,
                    v7: 0x0100_0001,
                },
                &pts,
                0,
                NONE16,
                RegionKind::BoardCutout,
            );
        }
    }
    if crate::outline::loose_edges(b) > 0 {
        c.report.warn(format!(
            "{} Edge.Cuts pieces form no closed loop; left out",
            crate::outline::loose_edges(b)
        ));
    }

    crate::models::write(&mut c.doc, b, board_dir, &mut c.report);
    let polygons = crate::zones::write(&mut c.doc, b, &mut c.report);
    crate::rules::write(&mut c.doc, b, project, &polygons);
    if project.is_none() {
        c.report.warn(
            "no .kicad_pro next to the board: net classes and design rules are Altium's defaults"
                .into(),
        );
    }

    let counts = [
        "Components6",
        "Pads6",
        "Vias6",
        "Tracks6",
        "Arcs6",
        "Texts6",
        "Polygons6",
        "Regions6",
        "Nets6",
        "Models",
        "ComponentBodies6",
    ]
    .iter()
    .map(|&s| {
        (
            s,
            c.doc.storages.get(s).map(|x| x.count as usize).unwrap_or(0),
        )
    })
    .collect();
    c.report.counts = counts;
    (c.doc, c.report)
}

pub(crate) fn layer_for_zone(b: &Board, name: &str) -> Option<Layer> {
    let mut c = Ctx {
        b,
        doc: Doc::default(),
        report: Report::default(),
    };
    c.layer(name)
}

pub(crate) fn coords(p: Pt) -> (i32, i32) {
    (ax(p.x), ay(p.y))
}

pub(crate) fn net_index(net: usize) -> i64 {
    if net == 0 {
        -1
    } else {
        net as i64 - 1
    }
}

pub(crate) fn v7_name(l: Layer) -> String {
    layer_name_v7(l)
}

/// KiCad's text markup as plain text: ~{X}, ^{X}, _{X} -> X
pub(crate) fn strip_markup(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '~' | '^' | '_' if it.peek() == Some(&'{') => {
                it.next();
                depth += 1;
            }
            '}' if depth > 0 => depth -= 1,
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_direction_follows_the_mid_point() {
        // KiCad frame: from (1,0) over (0,-1) (which is up on screen) to (-1,0)
        let ((cx, cy), r, sa, ea) =
            arc_angles(Pt::new(1000, 0), Pt::new(0, -1000), Pt::new(-1000, 0)).unwrap();
        assert!(cx.abs() < 1e-6 && cy.abs() < 1e-6 && (r - 1000.0).abs() < 1e-6);
        // Up on screen is up in Altium too: counter-clockwise from 0 to 180
        assert!(
            (sa - 0.0).abs() < 1e-6 && (ea - 180.0).abs() < 1e-6,
            "{sa} {ea}"
        );
        let (_, _, sa, ea) =
            arc_angles(Pt::new(1000, 0), Pt::new(0, 1000), Pt::new(-1000, 0)).unwrap();
        assert!(
            (sa - 180.0).abs() < 1e-6 && (ea - 0.0).abs() < 1e-6 || (ea - 360.0).abs() < 1e-6,
            "{sa} {ea}"
        );
    }

    #[test]
    fn unique_ids_are_letters_and_stable() {
        let a = unique_id(3);
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_uppercase()));
        assert_eq!(a, unique_id(3));
        assert_ne!(a, unique_id(4));
    }
}
