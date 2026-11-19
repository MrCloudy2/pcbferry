//! Writing a KiCad schematic as an Altium SchDoc: an OLE compound file whose `FileHeader`
//! stream holds one property list per record (`|RECORD=1|LOCATION.X=...`), children pointing
//! at their owner by record index. Lengths in units of 10 mil with a `_Frac` part in
//! 1/100000 of that, y pointing up from the bottom of the sheet. Everything a component owns
//! is in sheet coordinates, with the component's own placement given alongside.

use super::kicad::{Fill, Item, Iu, LabelKind, LibSymbol, Schematic, Symbol, P};
use crate::altium::{Doc, Props};
use std::collections::HashMap;

/// KiCad's symbol transform: (x, y) -> (x1·x + y1·y, x2·x + y2·y)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tr {
    x1: Iu,
    y1: Iu,
    x2: Iu,
    y2: Iu,
}

const I: Tr = Tr {
    x1: 1,
    y1: 0,
    x2: 0,
    y2: 1,
};
const MIRROR_X: Tr = Tr {
    x1: 1,
    y1: 0,
    x2: 0,
    y2: -1,
};
const MIRROR_Y: Tr = Tr {
    x1: -1,
    y1: 0,
    x2: 0,
    y2: 1,
};

impl Tr {
    pub fn apply(&self, p: P) -> P {
        P {
            x: self.x1 * p.x + self.y1 * p.y,
            y: self.x2 * p.x + self.y2 * p.y,
        }
    }
    /// `t` applied after this one, as SCH_SYMBOL::SetOrientation composes them
    fn then(self, t: Tr) -> Tr {
        Tr {
            x1: self.x1 * t.x1 + self.x2 * t.y1,
            y1: self.y1 * t.x1 + self.y2 * t.y1,
            x2: self.x1 * t.x2 + self.x2 * t.y2,
            y2: self.y1 * t.x2 + self.y2 * t.y2,
        }
    }
}

fn rotation(angle: i32) -> Tr {
    match angle {
        90 => Tr {
            x1: 0,
            y1: 1,
            x2: -1,
            y2: 0,
        },
        180 => Tr {
            x1: -1,
            y1: 0,
            x2: 0,
            y2: -1,
        },
        270 => Tr {
            x1: 0,
            y1: -1,
            x2: 1,
            y2: 0,
        },
        _ => I,
    }
}

/// A placed symbol's transform, as KiCad reads `(at x y angle)` then `(mirror x|y)`
pub fn symbol_transform(s: &Symbol) -> Tr {
    let t = rotation(s.angle);
    match s.mirror {
        Some('x') => t.then(MIRROR_X),
        Some('y') => t.then(MIRROR_Y),
        _ => t,
    }
}

/// Altium's quarter turns and mirror flag giving the same transform once imported: the
/// importer rotates, then mirrors about the y axis
fn altium_orientation(t: Tr) -> (i32, bool) {
    for k in 0..4 {
        let r = rotation(k * 90);
        if r == t {
            return (k, false);
        }
        if r.then(MIRROR_Y) == t {
            return (k, true);
        }
    }
    (0, false)
}

/// The sheet: KiCad's page, and where Altium's origin (bottom left) lands
struct Sheet {
    style: Option<i32>,
    portrait: bool,
    size: P,
    /// KiCad y of Altium y = 0: the page height down to the 50 mil connection grid, as the
    /// importer flips it
    offset: Iu,
}

const MIL: Iu = 254;

fn sheet(s: &Schematic) -> Sheet {
    let mils = |w: Iu, h: Iu| P {
        x: w * MIL,
        y: h * MIL,
    };
    let (style, size) = match s.paper.as_str() {
        "A4" => (Some(0), mils(11693, 8268)),
        "A3" => (Some(1), mils(16535, 11693)),
        "A2" => (Some(2), mils(23386, 16535)),
        "A1" => (Some(3), mils(33110, 23386)),
        "A0" => (Some(4), mils(46811, 33110)),
        "A" => (Some(5), mils(11000, 8500)),
        "B" => (Some(6), mils(17000, 11000)),
        "C" => (Some(7), mils(22000, 17000)),
        "D" => (Some(8), mils(34000, 22000)),
        "E" => (Some(9), mils(44000, 34000)),
        "USLetter" => (Some(10), mils(11000, 8500)),
        "USLegal" => (Some(11), mils(14000, 8500)),
        "USLedger" => (None, mils(17000, 11000)),
        _ => (None, s.user_size.unwrap_or(mils(11693, 8268))),
    };
    let size = if s.portrait && style.is_some() {
        P {
            x: size.y,
            y: size.x,
        }
    } else {
        size
    };
    let grid = 50 * MIL;
    Sheet {
        style,
        portrait: s.portrait,
        size,
        offset: size.y / grid * grid,
    }
}

/// 10 mil units and the 1/100000 fraction
fn units(v: Iu) -> (i64, i64) {
    let t = (v as f64 * 1000.0 / 25.4).round() as i64;
    (t / 100_000, t % 100_000)
}

trait Rec {
    fn t(&mut self, k: &str) -> &mut Self;
    fn dist(&mut self, k: &str, v: Iu) -> &mut Self;
    fn at(&mut self, kx: &str, ky: &str, p: P, sh: &Sheet) -> &mut Self;
}

impl Rec for Props {
    fn t(&mut self, k: &str) -> &mut Self {
        self.set(k, "T")
    }
    fn dist(&mut self, k: &str, v: Iu) -> &mut Self {
        let (n, f) = units(v);
        if n != 0 {
            self.num(k, n);
        }
        if f != 0 {
            self.num(&format!("{k}_Frac"), f);
        }
        self
    }
    fn at(&mut self, kx: &str, ky: &str, p: P, sh: &Sheet) -> &mut Self {
        self.dist(kx, p.x).dist(ky, sh.offset - p.y)
    }
}

fn record(kind: i32) -> Props {
    let mut p = Props::new();
    p.num("RECORD", kind);
    p
}

/// Altium's eight-letter unique ids, derived from KiCad's uuid so a conversion is repeatable
fn unique_id(seed: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    (0..8)
        .map(|i| (b'A' + ((h >> (i * 5)) % 26) as u8) as char)
        .collect()
}

const BLUE: i32 = 8_388_608; // Altium stores colours as 0xBBGGRR
const LINE: i32 = 16_711_680;
const BODY: i32 = 11_599_871;
const DARK: i32 = 128;

#[derive(Default)]
pub struct Report {
    pub components: usize,
    pub power_ports: usize,
    pub wires: usize,
    pub labels: usize,
    pub warnings: Vec<String>,
}

struct Out<'a> {
    recs: Vec<Props>,
    /// Wire and bus ends and pin connections: where a port's far end must not land, or the
    /// importer joins the two with a wire
    terminals: std::collections::HashSet<P>,
    /// Wires and buses: the importer counts any point on one as a terminal too
    segments: Vec<(P, P)>,
    aliases: &'a HashMap<String, Vec<String>>,
    /// Global labels as power ports: the design has sheets
    global_power: bool,
    sh: Sheet,
    report: Report,
}

impl Out<'_> {
    fn push(&mut self, p: Props) -> usize {
        self.recs.push(p);
        self.recs.len() - 1
    }

    fn owned(&self, kind: i32, owner: usize, part: i64) -> Props {
        let mut p = record(kind);
        p.num("OwnerIndex", owner)
            .t("IsNotAccesible")
            .num("OwnerPartId", part);
        p
    }

    fn poly(&self, p: &mut Props, pts: &[P]) {
        p.num("LocationCount", pts.len());
        for (i, q) in pts.iter().enumerate() {
            p.at(&format!("X{}", i + 1), &format!("Y{}", i + 1), *q, &self.sh);
        }
    }

    fn component(&mut self, s: &Symbol, lib: &LibSymbol, reference: &str) {
        let tr = symbol_transform(s);
        let place = |q: P| tr.apply(q) + s.at;
        let (orient, mirrored) = altium_orientation(tr);
        let (lib_name, name) = s.lib_id.split_once(':').unwrap_or(("", &s.lib_id));
        let mut c = record(1);
        c.set("LibReference", name);
        if !lib.description.is_empty() {
            c.set("ComponentDescription", lib.description.as_str());
        }
        c.num("PartCount", lib.units + 1)
            .num("DisplayModeCount", lib.styles)
            .num("IndexInSheet", self.report.components)
            .at("Location.X", "Location.Y", s.at, &self.sh)
            .num("Orientation", orient);
        if mirrored {
            c.t("IsMirrored");
        }
        if s.style > 1 {
            c.num("DisplayMode", s.style - 1);
        }
        c.num("CurrentPartId", s.unit);
        if !lib_name.is_empty() {
            c.set("SourceLibraryName", format!("{lib_name}.SchLib"));
        }
        c.set("TargetFileName", "*")
            .set("UniqueID", unique_id(&s.uuid))
            .set("PartIDLocked", "F")
            .set("DesignItemId", name);
        if !s.in_bom || !s.on_board {
            // Standard (no BOM) or Graphical
            c.num("ComponentKind", if s.on_board { 3 } else { 2 });
        }
        let ci = self.push(c);
        self.report.components += 1;
        let part = s.unit as i64;
        let mode = |p: &mut Props| {
            if s.style > 1 {
                p.num("OwnerPartDisplayMode", s.style - 1);
            }
        };

        for d in &lib.items {
            if (d.unit != 0 && d.unit != s.unit) || (d.style != 0 && d.style != s.style) {
                continue;
            }
            match &d.item {
                Item::Pin(pin) => {
                    let dir = match pin.angle {
                        90 => P {
                            x: 0,
                            y: -pin.length,
                        },
                        180 => P {
                            x: -pin.length,
                            y: 0,
                        },
                        270 => P {
                            x: 0,
                            y: pin.length,
                        },
                        _ => P {
                            x: pin.length,
                            y: 0,
                        },
                    };
                    let hot = place(pin.at);
                    let body = place(pin.at + dir);
                    self.terminals.insert(hot);
                    // Altium: from the body end towards the connection, y up
                    let (dx, dy) = (hot.x - body.x, body.y - hot.y);
                    let o = if dx.abs() >= dy.abs() {
                        if dx > 0 {
                            0
                        } else {
                            2
                        }
                    } else if dy > 0 {
                        1
                    } else {
                        3
                    };
                    let mut flags = o;
                    if pin.hidden {
                        flags |= 4;
                    }
                    if !lib.hide_pin_names && pin.name != "~" {
                        flags |= 8;
                    }
                    if !lib.hide_pin_numbers {
                        flags |= 16;
                    }
                    let electrical = match pin.kind.as_str() {
                        "input" => 0,
                        "bidirectional" => 1,
                        "output" => 2,
                        "open_collector" => 3,
                        "tri_state" => 5,
                        "open_emitter" => 6,
                        "power_in" | "power_out" => 7,
                        _ => 4,
                    };
                    let mut p = self.owned(2, ci, part);
                    mode(&mut p);
                    match pin.shape.as_str() {
                        "inverted" | "inverted_clock" => {
                            p.num("Symbol_OuterEdge", 1);
                        }
                        _ => {}
                    }
                    if pin.shape.contains("clock") {
                        p.num("Symbol_InnerEdge", 3);
                    }
                    p.num("FormalType", 1)
                        .num("Electrical", electrical)
                        .num("PinConglomerate", flags)
                        .dist("PinLength", pin.length)
                        .at("Location.X", "Location.Y", body, &self.sh)
                        .set(
                            "Name",
                            if pin.name == "~" {
                                ""
                            } else {
                                pin.name.as_str()
                            },
                        )
                        .set("Designator", pin.number.as_str())
                        .set("UniqueID", unique_id(&format!("{}/{}", s.uuid, pin.number)));
                    let pi = self.push(p);
                    // KiCad ties an invisible power input to the net of its name; Altium says
                    // so with the pin's hidden net name
                    if pin.hidden && pin.kind == "power_in" && !pin.name.is_empty() {
                        let mut h = self.owned(41, pi, -1);
                        h.set("Name", "HiddenNetName")
                            .set("Text", pin.name.as_str())
                            .t("IsHidden");
                        self.push(h);
                    }
                }
                Item::Poly { pts, fill, width } => {
                    let pts: Vec<P> = pts.iter().map(|q| place(*q)).collect();
                    let closed = pts.len() > 2 && *fill != Fill::None;
                    let mut p = self.owned(if closed { 7 } else { 6 }, ci, part);
                    mode(&mut p);
                    p.num("LineWidth", line_width(*width)).num("Color", LINE);
                    if closed {
                        p.num(
                            "AreaColor",
                            if *fill == Fill::Outline { LINE } else { BODY },
                        )
                        .t("IsSolid");
                    }
                    self.poly(&mut p, &pts);
                    self.push(p);
                }
                Item::Rect { a, b, fill, width } => {
                    let (a, b) = (place(*a), place(*b));
                    let lo = P {
                        x: a.x.min(b.x),
                        y: a.y.max(b.y),
                    };
                    let hi = P {
                        x: a.x.max(b.x),
                        y: a.y.min(b.y),
                    };
                    let mut p = self.owned(14, ci, part);
                    mode(&mut p);
                    p.at("Location.X", "Location.Y", lo, &self.sh)
                        .at("Corner.X", "Corner.Y", hi, &self.sh)
                        .num("LineWidth", line_width(*width))
                        .num("Color", LINE)
                        .num(
                            "AreaColor",
                            if *fill == Fill::Outline { LINE } else { BODY },
                        );
                    if *fill != Fill::None {
                        p.t("IsSolid");
                    } else {
                        p.t("Transparent");
                    }
                    self.push(p);
                }
                Item::Circle {
                    c: centre,
                    r,
                    fill,
                    width,
                } => {
                    let mut p = self.owned(8, ci, part);
                    mode(&mut p);
                    p.at("Location.X", "Location.Y", place(*centre), &self.sh)
                        .dist("Radius", *r)
                        .dist("SecondaryRadius", *r)
                        .num("LineWidth", line_width(*width))
                        .num("Color", LINE)
                        .num(
                            "AreaColor",
                            if *fill == Fill::Outline { LINE } else { BODY },
                        );
                    if *fill != Fill::None {
                        p.t("IsSolid");
                    }
                    self.push(p);
                }
                Item::Arc { s: a, m, e, width } => {
                    if let Some(arc) = arc(place(*a), place(*m), place(*e), self.sh.offset) {
                        let mut p = self.owned(12, ci, part);
                        mode(&mut p);
                        p.at("Location.X", "Location.Y", arc.0, &self.sh)
                            .dist("Radius", arc.1)
                            .set("StartAngle", format!("{:.3}", arc.2))
                            .set("EndAngle", format!("{:.3}", arc.3))
                            .num("LineWidth", line_width(*width))
                            .num("Color", LINE);
                        self.push(p);
                    }
                }
                Item::Bezier { pts, width } if pts.len() == 4 => {
                    let pts: Vec<P> = pts.iter().map(|q| place(*q)).collect();
                    let mut p = self.owned(5, ci, part);
                    mode(&mut p);
                    p.num("LineWidth", line_width(*width)).num("Color", LINE);
                    self.poly(&mut p, &pts);
                    self.push(p);
                }
                Item::Text {
                    text, at, hidden, ..
                } if !hidden => {
                    let mut p = self.owned(4, ci, part);
                    mode(&mut p);
                    p.at("Location.X", "Location.Y", place(*at), &self.sh)
                        .num("Color", LINE)
                        .num("FontID", 1)
                        .set("Text", text.as_str());
                    self.push(p);
                }
                _ => {}
            }
        }

        // Fields: the designator, KiCad's value as Altium's comment, then the rest
        let field_rec =
            |out: &Out, kind: i32, name: &str, text: &str, f: Option<&super::kicad::Field>| {
                let mut p = record(kind);
                p.num("OwnerIndex", ci)
                    .num("IndexInSheet", -1)
                    .num("OwnerPartId", -1);
                if let Some(f) = f {
                    p.at("Location.X", "Location.Y", f.at, &out.sh);
                    if (f.angle.round() as i32).rem_euclid(180) == 90 {
                        p.num("Orientation", 1);
                    }
                }
                p.num("Color", BLUE).num("FontID", 1);
                if f.is_none_or(|f| f.hidden) {
                    p.t("IsHidden");
                }
                p.set("Text", text).set("Name", name);
                p
            };
        let d = field_rec(self, 34, "Designator", reference, s.field("Reference"));
        self.push(d);
        let v = field_rec(self, 41, "Comment", s.value_of("Value"), s.field("Value"));
        self.push(v);
        for f in &s.fields {
            if matches!(
                f.name.as_str(),
                "Reference" | "Value" | "Footprint" | "Description"
            ) || f.name.starts_with("ki_")
                || f.value.is_empty()
            {
                continue;
            }
            // Altium's own Comment and Designator would take these over
            let name = if f.name.eq_ignore_ascii_case("Comment")
                || f.name.eq_ignore_ascii_case("Designator")
            {
                format!("{}_1", f.name)
            } else {
                f.name.clone()
            };
            let p = field_rec(self, 41, &name, &f.value, Some(f));
            self.push(p);
        }

        let fp = s.value_of("Footprint");
        if !fp.is_empty() {
            let (fp_lib, fp_name) = fp.split_once(':').unwrap_or(("", fp));
            let mut l = record(44);
            l.num("OwnerIndex", ci);
            let li = self.push(l);
            let mut m = record(45);
            m.num("OwnerIndex", li)
                .num("IndexInSheet", -1)
                .set("ModelName", fp_name)
                .set("ModelType", "PCBLIB")
                .num("DatafileCount", 1)
                .set("ModelDatafileEntity0", fp_name)
                .set("ModelDatafileKind0", "PCBLib");
            if !fp_lib.is_empty() {
                m.set("ModelDataFile0", format!("{fp_lib}.PcbLib"));
            }
            m.t("IsCurrent")
                .t("DatalinksLocked")
                .t("DatabaseDatalinksLocked");
            m.set("UniqueID", unique_id(&format!("{}/fp", s.uuid)));
            let mi = self.push(m);
            for kind in [46, 48] {
                let mut r = record(kind);
                r.num("OwnerIndex", mi);
                self.push(r);
            }
        }
    }

    /// A KiCad power symbol as an Altium power port: its net (KiCad's value), at its pin
    fn power_port(&mut self, s: &Symbol, lib: &LibSymbol) {
        let tr = symbol_transform(s);
        let place = |q: P| tr.apply(q) + s.at;
        let Some(pin) = lib.items.iter().find_map(|d| match &d.item {
            Item::Pin(p) => Some(p),
            _ => None,
        }) else {
            return;
        };
        let at = place(pin.at);
        // Which way the graphic points from the connection
        let mut sum = P::default();
        let mut n = 0;
        for d in &lib.items {
            let pts: Vec<P> = match &d.item {
                Item::Poly { pts, .. } => pts.clone(),
                Item::Rect { a, b, .. } => vec![*a, *b],
                Item::Circle { c, .. } => vec![*c],
                Item::Arc { s, m, e, .. } => vec![*s, *m, *e],
                _ => vec![],
            };
            for q in pts {
                sum = sum + place(q);
                n += 1;
            }
        }
        let (dx, dy) = if n > 0 {
            (sum.x / n - at.x, at.y - sum.y / n)
        } else {
            (0, 1)
        };
        let orient = if dx.abs() > dy.abs() {
            if dx > 0 {
                0
            } else {
                2
            }
        } else if dy >= 0 {
            1
        } else {
            3
        };
        let value = s.value_of("Value");
        let ground = orient == 3 || value.to_uppercase().contains("GND");
        let mut p = record(17);
        p.num("IndexInSheet", -1)
            .num("OwnerPartId", -1)
            .num("Style", if ground { 4 } else { 2 });
        if !s.field("Value").is_some_and(|f| f.hidden) {
            p.t("ShowNetName");
        }
        p.at("Location.X", "Location.Y", at, &self.sh)
            .num("Orientation", orient)
            .num("Color", DARK)
            .num("FontID", 1)
            .set("Text", value)
            .set("UniqueID", unique_id(&s.uuid));
        self.push(p);
        self.report.power_ports += 1;
    }

    fn label(&mut self, l: &super::kicad::Label) {
        // Sized by the label as written: an alias spelled out is far longer
        let written = l.text.clone();
        let text = expand_alias(&l.text, self.aliases);
        let l = &super::kicad::Label { text, ..l.clone() };
        let horizontal = l.angle == 0 || l.angle == 180;
        match l.kind {
            LabelKind::Local => {
                let mut p = record(25);
                p.num("IndexInSheet", -1).num("OwnerPartId", -1).at(
                    "Location.X",
                    "Location.Y",
                    l.at,
                    &self.sh,
                );
                if !horizontal {
                    p.num("Orientation", 1);
                }
                if l.angle == 180 || l.angle == 270 {
                    p.num("Justification", 2);
                }
                p.num("Color", DARK)
                    .num("FontID", 1)
                    .set("Text", l.text.as_str());
                self.push(p);
            }
            LabelKind::Global if self.global_power => {
                // Ports are hierarchical in a hierarchy, so a net global across sheets is a
                // power port in Altium
                let mut p = record(17);
                p.num("IndexInSheet", -1)
                    .num("OwnerPartId", -1)
                    .num("Style", 1)
                    .t("ShowNetName")
                    .at("Location.X", "Location.Y", l.at, &self.sh)
                    .num("Orientation", l.angle / 90)
                    .num("Color", DARK)
                    .num("FontID", 1)
                    .set("Text", l.text.as_str())
                    .set("UniqueID", unique_id(&format!("{}{:?}", l.text, l.at)));
                self.push(p);
            }
            LabelKind::Global | LabelKind::Hierarchical => {
                // The importer puts the label at whichever end of the port meets a wire
                let width = crate::font::text_width(&written, l.size) + 2 * l.size;
                let mut width = width.max(20 * MIL);
                let far = |w: Iu| match l.angle {
                    180 => l.at - P { x: w, y: 0 },
                    270 => l.at + P { x: 0, y: w },
                    90 => l.at - P { x: 0, y: w },
                    _ => l.at + P { x: w, y: 0 },
                };
                let taken = |q: P| {
                    self.terminals.contains(&q)
                        || self.segments.iter().any(|(a, b)| {
                            let cross = (b.x - a.x) as i128 * (q.y - a.y) as i128
                                - (b.y - a.y) as i128 * (q.x - a.x) as i128;
                            cross == 0
                                && q.x >= a.x.min(b.x)
                                && q.x <= a.x.max(b.x)
                                && q.y >= a.y.min(b.y)
                                && q.y <= a.y.max(b.y)
                        })
                };
                while taken(far(width)) {
                    width += 10 * MIL;
                }
                let location = match l.angle {
                    180 | 270 => far(width),
                    _ => l.at,
                };
                let io = match l.shape.as_str() {
                    "output" => 1,
                    "input" => 2,
                    "bidirectional" | "tri_state" => 3,
                    _ => 0,
                };
                let style = match (io, horizontal) {
                    (0, true) => 0,
                    (3, true) => 3,
                    (_, true) => 2,
                    (0, false) => 4,
                    (3, false) => 7,
                    (_, false) => 5,
                };
                let mut p = record(18);
                p.num("IndexInSheet", -1)
                    .num("OwnerPartId", -1)
                    .num("Style", style)
                    .num("IOType", io)
                    .num("Alignment", 1)
                    .dist("Width", width)
                    .dist("Height", 100 * MIL)
                    .at("Location.X", "Location.Y", location, &self.sh)
                    .num("Color", DARK)
                    .num("AreaColor", 8_454_143)
                    .num("TextColor", DARK)
                    .num("FontID", 1)
                    .num("TextFontID", 1)
                    .set("Name", l.text.as_str())
                    .set("UniqueID", unique_id(&format!("{}{:?}", l.text, l.at)));
                self.push(p);
            }
        }
        self.report.labels += 1;
    }
}

impl Out<'_> {
    /// A KiCad sheet as an Altium sheet symbol: its name, its file and its entries
    fn sheet_symbol(&mut self, sheet: &super::kicad::Sheet, file: &str) {
        let mut p = record(15);
        p.num("IndexInSheet", -1)
            .num("OwnerPartId", -1)
            .at("Location.X", "Location.Y", sheet.at, &self.sh)
            .dist("XSize", sheet.size.x)
            .dist("YSize", sheet.size.y)
            .num("LineWidth", 1)
            .num("Color", DARK)
            .num("AreaColor", 8_454_016)
            .t("IsSolid")
            .set("UniqueID", unique_id(&sheet.uuid));
        let si = self.push(p);
        for (kind, text, dy) in [
            (32, sheet.name.as_str(), -MIL * 20),
            (33, file, sheet.size.y + MIL * 80),
        ] {
            let mut p = record(kind);
            p.num("OwnerIndex", si)
                .num("IndexInSheet", -1)
                .num("OwnerPartId", -1)
                .at(
                    "Location.X",
                    "Location.Y",
                    sheet.at + P { x: 0, y: dy },
                    &self.sh,
                )
                .num("Color", BLUE)
                .num("FontID", 1)
                .set("Text", text);
            self.push(p);
        }
        for pin in &sheet.pins {
            let (side, along) = if pin.at.x <= sheet.at.x {
                (0, pin.at.y - sheet.at.y)
            } else if pin.at.x >= sheet.at.x + sheet.size.x {
                (1, pin.at.y - sheet.at.y)
            } else if pin.at.y <= sheet.at.y {
                (2, pin.at.x - sheet.at.x)
            } else {
                (3, pin.at.x - sheet.at.x)
            };
            let io = match pin.shape.as_str() {
                "output" => 1,
                "input" => 2,
                "bidirectional" | "tri_state" => 3,
                _ => 0,
            };
            // In 100 mil steps and 1/10000 mil
            let step = 100 * MIL;
            let (n, rest) = (along.div_euclid(step), along.rem_euclid(step));
            let mut p = record(16);
            p.num("OwnerIndex", si)
                .num("IndexInSheet", -1)
                .num("OwnerPartId", -1)
                .num("Side", side)
                .num("DistanceFromTop", n);
            let frac = (rest as f64 * 10_000.0 / 254.0).round() as i64;
            if frac != 0 {
                p.num("DistanceFromTop_Frac1", frac);
            }
            p.num("Color", DARK)
                .num("AreaColor", 8_454_143)
                .num("TextColor", DARK)
                .num("TextFontID", 1)
                .num("TextStyle", 0)
                .set("Name", expand_alias(&pin.name, self.aliases))
                .num("IOType", io)
                .num("Style", if side < 2 { 3 } else { 7 });
            self.push(p);
        }
    }
}

/// A bus label naming an alias, `{ALIAS}` or `PREFIX{ALIAS}`, with the alias's members spelled
/// out: an alias is a KiCad definition the SchDoc has no place for
pub fn expand_alias(text: &str, aliases: &HashMap<String, Vec<String>>) -> String {
    // The group is the last brace pair, closing the text; `_{..}`, `^{..}` and `~{..}` are
    // KiCad's subscript, superscript and overbar markup
    if let Some(body) = text.strip_suffix('}') {
        if let Some(a) = body.rfind('{') {
            let markup = text[..a].ends_with(['_', '^', '~']);
            if let (false, Some(m)) = (markup, aliases.get(body[a + 1..].trim())) {
                return format!("{}{{{}}}", &text[..a], m.join(" "));
            }
        }
    }
    text.to_string()
}

#[cfg(test)]
#[test]
fn aliases_expand_in_bus_labels() {
    let a: HashMap<String, Vec<String>> = [(
        "I2C".to_string(),
        vec!["SDA".to_string(), "SCL".to_string()],
    )]
    .into();
    assert_eq!(expand_alias("{I2C}", &a), "{SDA SCL}");
    assert_eq!(expand_alias("I2C_{CAM}{I2C}", &a), "I2C_{CAM}{SDA SCL}");
    assert_eq!(expand_alias("X_{I2C}", &a), "X_{I2C}");
    assert_eq!(expand_alias("D[0..7]", &a), "D[0..7]");
}

fn line_width(w: Iu) -> i32 {
    // Altium's widths: 0 smallest, 1 small (10 mil), 2 medium, 3 large
    match w {
        w if w <= 6 * MIL => 0,
        w if w <= 15 * MIL => 1,
        w if w <= 30 * MIL => 2,
        _ => 3,
    }
}

/// An arc through three sheet points as Altium's centre, radius and angles, counter-clockwise
/// in Altium's y-up frame
fn arc(s: P, m: P, e: P, offset: Iu) -> Option<(P, Iu, f64, f64)> {
    let f = |p: P| (p.x as f64, (offset - p.y) as f64);
    let ((ax, ay), (bx, by), (cx, cy)) = (f(s), f(m), f(e));
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-9 {
        return None;
    }
    let sq = |x: f64, y: f64| x * x + y * y;
    let ux = (sq(ax, ay) * (by - cy) + sq(bx, by) * (cy - ay) + sq(cx, cy) * (ay - by)) / d;
    let uy = (sq(ax, ay) * (cx - bx) + sq(bx, by) * (ax - cx) + sq(cx, cy) * (bx - ax)) / d;
    let r = sq(ax - ux, ay - uy).sqrt();
    let ang = |x: f64, y: f64| (y - uy).atan2(x - ux).to_degrees().rem_euclid(360.0);
    let (a0, am, a1) = (ang(ax, ay), ang(bx, by), ang(cx, cy));
    // Counter-clockwise from a0 passes the middle point before a1?
    let ccw = (am - a0).rem_euclid(360.0) < (a1 - a0).rem_euclid(360.0);
    let (start, end) = if ccw { (a0, a1) } else { (a1, a0) };
    let centre = P {
        x: ux.round() as Iu,
        y: offset - uy.round() as Iu,
    };
    Some((centre, r.round() as Iu, start, end))
}

/// The reference a symbol has on this sheet instance
pub fn reference(s: &Symbol, path: &str) -> String {
    s.refs
        .iter()
        .find(|(p, _, _)| p == path)
        .or(s.refs.first())
        .map(|(_, r, _)| r.clone())
        .unwrap_or_else(|| s.value_of("Reference").to_string())
}

/// One sheet as a SchDoc. `path` is the sheet instance path the references are taken from,
/// `sheet_files` the SchDoc each of its sub-sheets is written as.
pub fn convert(
    sch: &Schematic,
    path: &str,
    sheet_files: &[String],
    hierarchical: bool,
    aliases: &HashMap<String, Vec<String>>,
) -> (Doc, Report) {
    let sh = sheet(sch);
    let mut out = Out {
        recs: Vec::new(),
        terminals: sch
            .wires
            .iter()
            .chain(&sch.buses)
            .chain(&sch.bus_entries)
            .flat_map(|(a, b)| [*a, *b])
            .chain(sch.sheets.iter().flat_map(|s| s.pins.iter().map(|p| p.at)))
            .chain(sch.junctions.iter().copied())
            .collect(),
        segments: sch
            .wires
            .iter()
            .chain(&sch.buses)
            .chain(&sch.bus_entries)
            .copied()
            .collect(),
        aliases,
        global_power: hierarchical,
        sh,
        report: Report::default(),
    };

    let mut s = record(31);
    s.num("FontIdCount", 1)
        .num("Size1", 10)
        .set("FontName1", "Times New Roman")
        .t("UseMBCS")
        .t("IsBOC")
        .t("HotSpotGridOn")
        .num("HotSpotGridSize", 4)
        .num("SystemFont", 1)
        .t("BorderOn")
        .num("SheetNumberSpaceSize", 12)
        .num("AreaColor", 16_317_695)
        .t("SnapGridOn")
        .num("SnapGridSize", 5)
        .t("VisibleGridOn")
        .num("VisibleGridSize", 10);
    match out.sh.style {
        Some(style) => {
            s.num("SheetStyle", style);
            if out.sh.portrait {
                s.num("WorkspaceOrientation", 1);
            }
        }
        None => {
            s.t("UseCustomSheet")
                .dist("CustomX", out.sh.size.x)
                .dist("CustomY", out.sh.size.y)
                .num("CustomXZones", 6)
                .num("CustomYZones", 4)
                .num("CustomMarginWidth", 20);
        }
    }
    s.num("Display_Unit", 4);
    out.push(s);

    for (k, v) in &sch.title {
        let name = match k.as_str() {
            "title" => "Title",
            "rev" => "Revision",
            "date" => "Date",
            "company" => "CompanyName",
            _ => continue,
        };
        let mut p = record(41);
        p.num("OwnerPartId", -1)
            .t("IsHidden")
            .set("Text", v.as_str())
            .set("Name", name);
        out.push(p);
    }

    let mut missing = std::collections::BTreeSet::new();
    for sym in &sch.symbols {
        let Some(lib) = sch.lib.get(&sym.lib_key) else {
            missing.insert(sym.lib_key.clone());
            continue;
        };
        if lib.power {
            // PWR_FLAG only quiets ERC; Altium has no equivalent symbol
            if !sym.value_of("Value").contains("PWR_FLAG") && !sym.lib_id.ends_with(":PWR_FLAG") {
                out.power_port(sym, lib);
            }
            continue;
        }
        let r = reference(sym, path);
        out.component(sym, lib, &r);
    }
    for m in missing {
        out.report
            .warnings
            .push(format!("symbol {m} is not in the schematic's library"));
    }

    for (kind, segs) in [(27, &sch.wires), (26, &sch.buses)] {
        for (a, b) in segs.iter() {
            let mut p = record(kind);
            p.num("IndexInSheet", -1)
                .num("OwnerPartId", -1)
                .num("LineWidth", if kind == 26 { 2 } else { 1 })
                .num("Color", BLUE);
            out.poly(&mut p, &[*a, *b]);
            out.push(p);
            out.report.wires += 1;
        }
    }
    for (a, b) in &sch.bus_entries {
        let mut p = record(37);
        p.num("OwnerPartId", -1)
            .at("Location.X", "Location.Y", *a, &out.sh)
            .at("Corner.X", "Corner.Y", *b, &out.sh)
            .num("LineWidth", 1)
            .num("Color", BLUE);
        out.push(p);
    }
    for j in &sch.junctions {
        let mut p = record(29);
        p.num("IndexInSheet", -1)
            .num("OwnerPartId", -1)
            .at("Location.X", "Location.Y", *j, &out.sh)
            .num("Color", DARK);
        out.push(p);
    }
    for n in &sch.no_connects {
        let mut p = record(22);
        p.num("IndexInSheet", -1)
            .num("OwnerPartId", -1)
            .at("Location.X", "Location.Y", *n, &out.sh)
            .num("Color", 255)
            .t("IsActive")
            .t("SuppressAll");
        out.push(p);
    }
    let labels = sch.labels.clone();
    for l in &labels {
        out.label(l);
    }
    for n in &sch.notes {
        let lines: Vec<&str> = n.text.lines().collect();
        let mut p;
        if lines.len() > 1 {
            // A text frame, from the text's top left down by its lines
            let h = n.size * 3 / 2 * lines.len() as Iu;
            let w = lines
                .iter()
                .map(|l| crate::font::text_width(l, n.size))
                .max()
                .unwrap_or(0);
            p = record(28);
            p.num("OwnerPartId", -1)
                .at("Location.X", "Location.Y", n.at + P { x: 0, y: h }, &out.sh)
                .at("Corner.X", "Corner.Y", n.at + P { x: w, y: 0 }, &out.sh)
                .num("AreaColor", 16_777_215)
                .num("FontID", 1)
                .num("Alignment", 1)
                .t("WordWrap")
                .set("Text", lines.join("~1"));
        } else {
            p = record(4);
            p.num("OwnerPartId", -1)
                .at("Location.X", "Location.Y", n.at, &out.sh);
            if (n.angle.round() as i32).rem_euclid(180) == 90 {
                p.num("Orientation", 1);
            }
            let j = (n.justify.1 + 1) * 3 + (n.justify.0 + 1);
            // KiCad's vertical: -1 top, 1 bottom; Altium counts from the bottom row
            let j = (2 - (j / 3)) * 3 + j % 3;
            if j != 0 {
                p.num("Justification", j);
            }
            p.num("Color", BLUE)
                .num("FontID", 1)
                .set("Text", n.text.as_str());
        }
        out.push(p);
    }
    for l in &sch.lines {
        if l.len() < 2 {
            continue;
        }
        let mut p = record(6);
        p.num("IndexInSheet", -1)
            .num("OwnerPartId", -1)
            .num("LineWidth", 1)
            .num("Color", BLUE);
        out.poly(&mut p, l);
        out.push(p);
    }
    for (sheet, file) in sch.sheets.iter().zip(sheet_files) {
        out.sheet_symbol(sheet, file);
    }

    let mut header = Props::new();
    header
        .set(
            "HEADER",
            "Protel for Windows - Schematic Capture Binary File Version 5.0",
        )
        .num("Weight", out.recs.len())
        .num("MinorVersion", 9)
        .set("UniqueID", unique_id(&sch.uuid));
    let mut stream = header.encode();
    for r in &out.recs {
        stream.extend(r.encode());
    }
    let mut storage = Props::new();
    storage.set("HEADER", "Icon storage");
    let mut doc = Doc::default();
    doc.streams.insert("FileHeader".into(), stream);
    doc.streams.insert("Storage".into(), storage.encode());
    (doc, out.report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importer_orientation_matches_kicad_transform() {
        for angle in [0, 90, 180, 270] {
            for mirror in [None, Some('x'), Some('y')] {
                let s = Symbol {
                    lib_key: String::new(),
                    lib_id: String::new(),
                    at: P::default(),
                    angle,
                    mirror,
                    unit: 1,
                    style: 1,
                    uuid: String::new(),
                    fields: vec![],
                    in_bom: true,
                    on_board: true,
                    refs: vec![],
                };
                let t = symbol_transform(&s);
                let (k, m) = altium_orientation(t);
                let back = if m {
                    rotation(k * 90).then(MIRROR_Y)
                } else {
                    rotation(k * 90)
                };
                assert_eq!(back, t, "{angle} {mirror:?}");
            }
        }
    }

    #[test]
    fn units_split_into_fraction() {
        assert_eq!(units(2540), (1, 0)); // 10 mil
        assert_eq!(units(254), (0, 10_000)); // 1 mil
        assert_eq!(units(-2540 * 3 - 254), (-3, -10_000));
    }

    #[test]
    fn arc_is_counter_clockwise() {
        // A quarter circle from (10, 0) to (0, 10) about the origin, y up (offset 0 flips)
        let (c, r, s, e) = arc(
            P { x: 10_000, y: 0 },
            P { x: 7071, y: -7071 },
            P { x: 0, y: -10_000 },
            0,
        )
        .unwrap();
        assert_eq!((c, r), (P { x: 0, y: 0 }, 10_000));
        assert!((s - 0.0).abs() < 0.01 && (e - 90.0).abs() < 0.01, "{s} {e}");
    }
}
