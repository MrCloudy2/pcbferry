//! A KiCad schematic (.kicad_sch) read into what an Altium SchDoc needs. Lengths in KiCad's
//! schematic unit (100 nm); sheet coordinates have y pointing down, library coordinates are
//! stored y up and read here y down, as KiCad itself does.

use crate::sexpr::{self, Sexpr};
use std::collections::HashMap;

pub type Iu = i64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct P {
    pub x: Iu,
    pub y: Iu,
}

impl std::ops::Add for P {
    type Output = P;
    fn add(self, o: P) -> P {
        P {
            x: self.x + o.x,
            y: self.y + o.y,
        }
    }
}

impl std::ops::Sub for P {
    type Output = P;
    fn sub(self, o: P) -> P {
        P {
            x: self.x - o.x,
            y: self.y - o.y,
        }
    }
}

pub fn iu(mm: f64) -> Iu {
    (mm * 10_000.0).round() as Iu
}

fn xy(e: &Sexpr, n: usize) -> P {
    let a = e.args();
    let num = |i: usize| a.get(i).and_then(|v| v.num()).unwrap_or(0.0);
    P {
        x: iu(num(n)),
        y: iu(num(n + 1)),
    }
}

/// `(at x y [angle])` of an element: the point and the angle in degrees
fn at(e: &Sexpr) -> (P, f64) {
    match e.child("at") {
        Some(a) => (
            xy(a, 0),
            a.args().get(2).and_then(|v| v.num()).unwrap_or(0.0),
        ),
        None => (P::default(), 0.0),
    }
}

fn lib_at(e: &Sexpr) -> (P, f64) {
    let (p, a) = at(e);
    (P { x: p.x, y: -p.y }, a)
}

fn pts(e: &Sexpr, lib: bool) -> Vec<P> {
    e.child("pts")
        .map(|p| {
            p.children("xy")
                .map(|v| {
                    let q = xy(v, 0);
                    if lib {
                        P { x: q.x, y: -q.y }
                    } else {
                        q
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn point(e: &Sexpr, name: &str, lib: bool) -> P {
    let p = e.child(name).map(|c| xy(c, 0)).unwrap_or_default();
    if lib {
        P { x: p.x, y: -p.y }
    } else {
        p
    }
}

/// A `(hide yes)` child or a bare `hide` word, in effects or on the element itself
fn hidden(e: &Sexpr) -> bool {
    let own = e.has_atom("hide") || e.flag("hide") == Some(true);
    own || e
        .child("effects")
        .is_some_and(|f| f.has_atom("hide") || f.flag("hide") == Some(true))
}

fn text_size(e: &Sexpr) -> Iu {
    e.child("effects")
        .and_then(|f| f.child("font"))
        .and_then(|f| f.arg_num("size", 0))
        .map(iu)
        .unwrap_or(12_700)
}

/// `(justify left top)` -> (-1|0|1 horizontal, -1|0|1 vertical: top is -1)
fn justify(e: &Sexpr) -> (i32, i32) {
    let j = e.child("effects").and_then(|f| f.child("justify"));
    let has = |w: &str| j.is_some_and(|j| j.has_atom(w));
    let h = if has("left") {
        -1
    } else if has("right") {
        1
    } else {
        0
    };
    let v = if has("top") {
        -1
    } else if has("bottom") {
        1
    } else {
        0
    };
    (h, v)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    None,
    Outline,
    Background,
}

fn fill(e: &Sexpr) -> Fill {
    match e.child("fill").and_then(|f| f.arg_text("type", 0)) {
        Some("outline") => Fill::Outline,
        Some("background") | Some("color") => Fill::Background,
        _ => Fill::None,
    }
}

fn stroke(e: &Sexpr) -> Iu {
    e.child("stroke")
        .and_then(|s| s.arg_num("width", 0))
        .map(iu)
        .unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct Pin {
    /// The connection point and the direction from it to the body, both library-local
    pub at: P,
    pub angle: i32,
    pub length: Iu,
    pub kind: String,
    pub shape: String,
    pub name: String,
    pub number: String,
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub enum Item {
    Pin(Pin),
    Poly {
        pts: Vec<P>,
        fill: Fill,
        width: Iu,
    },
    Rect {
        a: P,
        b: P,
        fill: Fill,
        width: Iu,
    },
    Circle {
        c: P,
        r: Iu,
        fill: Fill,
        width: Iu,
    },
    Arc {
        s: P,
        m: P,
        e: P,
        width: Iu,
    },
    Bezier {
        pts: Vec<P>,
        width: Iu,
    },
    Text {
        text: String,
        at: P,
        angle: f64,
        size: Iu,
        hidden: bool,
    },
}

#[derive(Clone, Debug)]
pub struct Drawn {
    /// 0: common to every unit / body style
    pub unit: u32,
    pub style: u32,
    pub item: Item,
}

#[derive(Clone, Debug, Default)]
pub struct LibSymbol {
    pub name: String,
    pub power: bool,
    pub units: u32,
    pub styles: u32,
    pub hide_pin_names: bool,
    pub hide_pin_numbers: bool,
    pub description: String,
    pub items: Vec<Drawn>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub value: String,
    pub at: P,
    pub angle: f64,
    pub size: Iu,
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub lib_key: String,
    pub lib_id: String,
    pub at: P,
    pub angle: i32,
    pub mirror: Option<char>,
    pub unit: u32,
    pub style: u32,
    pub uuid: String,
    pub fields: Vec<Field>,
    pub in_bom: bool,
    pub on_board: bool,
    /// Reference per instance path, from `(instances ...)`
    pub refs: Vec<(String, String, u32)>,
}

impl Symbol {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|f| f.name == name)
    }
    pub fn value_of(&self, name: &str) -> &str {
        self.field(name).map(|f| f.value.as_str()).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    Local,
    Global,
    Hierarchical,
}

#[derive(Clone, Debug)]
pub struct Label {
    pub kind: LabelKind,
    pub text: String,
    pub at: P,
    pub angle: i32,
    pub shape: String,
    pub size: Iu,
}

#[derive(Clone, Debug)]
pub struct Note {
    pub text: String,
    pub at: P,
    pub angle: f64,
    pub size: Iu,
    pub justify: (i32, i32),
}

#[derive(Clone, Debug)]
pub struct SheetPin {
    pub name: String,
    pub shape: String,
    pub at: P,
    pub angle: i32,
}

#[derive(Clone, Debug)]
pub struct Sheet {
    pub at: P,
    pub size: P,
    pub name: String,
    pub file: String,
    pub uuid: String,
    pub pins: Vec<SheetPin>,
}

#[derive(Clone, Debug, Default)]
pub struct Schematic {
    pub uuid: String,
    pub paper: String,
    pub portrait: bool,
    pub user_size: Option<P>,
    pub title: Vec<(String, String)>,
    pub lib: HashMap<String, LibSymbol>,
    pub symbols: Vec<Symbol>,
    pub wires: Vec<(P, P)>,
    pub buses: Vec<(P, P)>,
    pub bus_entries: Vec<(P, P)>,
    pub junctions: Vec<P>,
    pub no_connects: Vec<P>,
    pub labels: Vec<Label>,
    pub notes: Vec<Note>,
    pub lines: Vec<Vec<P>>,
    pub sheets: Vec<Sheet>,
    /// Bus aliases: name -> members
    pub aliases: Vec<(String, Vec<String>)>,
}

fn quarter(angle: f64) -> i32 {
    (((angle / 90.0).round() as i32) % 4 + 4) % 4 * 90
}

fn lib_item(e: &Sexpr) -> Option<Item> {
    Some(match e.head()? {
        "pin" => {
            let a = e.args();
            let (p, angle) = lib_at(e);
            Item::Pin(Pin {
                at: p,
                angle: quarter(angle),
                length: e.arg_num("length", 0).map(iu).unwrap_or(0),
                kind: a
                    .first()
                    .and_then(|v| v.text())
                    .unwrap_or("passive")
                    .to_string(),
                shape: a
                    .get(1)
                    .and_then(|v| v.text())
                    .unwrap_or("line")
                    .to_string(),
                name: e.arg_text("name", 0).unwrap_or("").to_string(),
                number: e.arg_text("number", 0).unwrap_or("").to_string(),
                hidden: e.has_atom("hide") || e.flag("hide") == Some(true),
            })
        }
        "polyline" => Item::Poly {
            pts: pts(e, true),
            fill: fill(e),
            width: stroke(e),
        },
        "rectangle" => Item::Rect {
            a: point(e, "start", true),
            b: point(e, "end", true),
            fill: fill(e),
            width: stroke(e),
        },
        "circle" => Item::Circle {
            c: point(e, "center", true),
            r: e.arg_num("radius", 0).map(iu).unwrap_or(0),
            fill: fill(e),
            width: stroke(e),
        },
        "arc" => Item::Arc {
            s: point(e, "start", true),
            m: point(e, "mid", true),
            e: point(e, "end", true),
            width: stroke(e),
        },
        "bezier" => Item::Bezier {
            pts: pts(e, true),
            width: stroke(e),
        },
        "text" => {
            let (p, angle) = lib_at(e);
            Item::Text {
                text: e
                    .args()
                    .first()
                    .and_then(|v| v.text())
                    .unwrap_or("")
                    .to_string(),
                at: p,
                angle,
                size: text_size(e),
                hidden: hidden(e),
            }
        }
        _ => return None,
    })
}

fn lib_symbol(e: &Sexpr, name: &str) -> LibSymbol {
    let mut s = LibSymbol {
        name: name.to_string(),
        power: e.child("power").is_some(),
        units: 1,
        styles: 1,
        ..Default::default()
    };
    let hides =
        |c: Option<&Sexpr>| c.is_some_and(|c| c.has_atom("hide") || c.flag("hide") == Some(true));
    s.hide_pin_names = hides(e.child("pin_names"));
    s.hide_pin_numbers = hides(e.child("pin_numbers"));
    for p in e.children("property") {
        if p.args().first().and_then(|v| v.text()) == Some("Description") {
            s.description = p
                .args()
                .get(1)
                .and_then(|v| v.text())
                .unwrap_or("")
                .to_string();
        }
    }
    for sub in e.children("symbol") {
        // "NAME_unit_style"
        let id = sub.args().first().and_then(|v| v.text()).unwrap_or("");
        let mut parts = id.rsplitn(3, '_');
        let style: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let unit: u32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        s.units = s.units.max(unit);
        s.styles = s.styles.max(style);
        for c in sub.args() {
            if let Some(item) = lib_item(c) {
                s.items.push(Drawn { unit, style, item });
            }
        }
    }
    s
}

fn field(p: &Sexpr) -> Field {
    let (at, angle) = at(p);
    let a = p.args();
    Field {
        name: a.first().and_then(|v| v.text()).unwrap_or("").to_string(),
        value: a.get(1).and_then(|v| v.text()).unwrap_or("").to_string(),
        at,
        angle,
        size: text_size(p),
        hidden: hidden(p),
    }
}

fn symbol(e: &Sexpr) -> Symbol {
    let (p, angle) = at(e);
    let lib_id = e.arg_text("lib_id", 0).unwrap_or("").to_string();
    let mut refs = Vec::new();
    if let Some(inst) = e.child("instances") {
        for proj in inst.children("project") {
            for path in proj.children("path") {
                refs.push((
                    path.args()
                        .first()
                        .and_then(|v| v.text())
                        .unwrap_or("")
                        .to_string(),
                    path.arg_text("reference", 0).unwrap_or("").to_string(),
                    path.arg_num("unit", 0).unwrap_or(1.0) as u32,
                ));
            }
        }
    }
    Symbol {
        lib_key: e
            .arg_text("lib_name", 0)
            .map(str::to_string)
            .unwrap_or_else(|| lib_id.clone()),
        lib_id,
        at: p,
        angle: quarter(angle),
        mirror: e.arg_text("mirror", 0).and_then(|m| m.chars().next()),
        unit: e.arg_num("unit", 0).unwrap_or(1.0) as u32,
        style: e
            .arg_num("body_style", 0)
            .or_else(|| e.arg_num("convert", 0))
            .unwrap_or(1.0) as u32,
        uuid: e.arg_text("uuid", 0).unwrap_or("").to_string(),
        fields: e.children("property").map(field).collect(),
        in_bom: e.flag("in_bom").unwrap_or(true),
        on_board: e.flag("on_board").unwrap_or(true),
        refs,
    }
}

fn segments(e: &Sexpr, out: &mut Vec<(P, P)>) {
    let p = pts(e, false);
    for w in p.windows(2) {
        out.push((w[0], w[1]));
    }
}

pub fn read(text: &str) -> Result<Schematic, Box<dyn std::error::Error>> {
    let root = sexpr::parse(text)?;
    if root.head() != Some("kicad_sch") {
        return Err("not a KiCad schematic".into());
    }
    let mut s = Schematic {
        uuid: root.arg_text("uuid", 0).unwrap_or("").to_string(),
        paper: "A4".into(),
        ..Default::default()
    };
    if let Some(p) = root.child("paper") {
        let a = p.args();
        s.paper = a.first().and_then(|v| v.text()).unwrap_or("A4").to_string();
        s.portrait = p.has_atom("portrait");
        if s.paper == "User" {
            let n = |i: usize| a.get(i).and_then(|v| v.num()).map(iu).unwrap_or(0);
            s.user_size = Some(P { x: n(1), y: n(2) });
        }
    }
    if let Some(tb) = root.child("title_block") {
        for c in tb.args() {
            if let (Some(h), Some(v)) = (c.head(), c.args().last().and_then(|v| v.text())) {
                s.title.push((h.to_string(), v.to_string()));
            }
        }
    }
    if let Some(libs) = root.child("lib_symbols") {
        for e in libs.children("symbol") {
            let name = e
                .args()
                .first()
                .and_then(|v| v.text())
                .unwrap_or("")
                .to_string();
            s.lib.insert(name.clone(), lib_symbol(e, &name));
        }
    }
    for e in root.args() {
        match e.head() {
            Some("symbol") => s.symbols.push(symbol(e)),
            Some("wire") => segments(e, &mut s.wires),
            Some("bus") => segments(e, &mut s.buses),
            Some("bus_entry") => {
                let (p, _) = at(e);
                let d = point(e, "size", false);
                s.bus_entries.push((p, p + d));
            }
            Some("junction") => s.junctions.push(at(e).0),
            Some("no_connect") => s.no_connects.push(at(e).0),
            Some(h @ ("label" | "global_label" | "hierarchical_label")) => {
                let (p, angle) = at(e);
                s.labels.push(Label {
                    kind: match h {
                        "label" => LabelKind::Local,
                        "global_label" => LabelKind::Global,
                        _ => LabelKind::Hierarchical,
                    },
                    text: e
                        .args()
                        .first()
                        .and_then(|v| v.text())
                        .unwrap_or("")
                        .to_string(),
                    at: p,
                    angle: quarter(angle),
                    shape: e.arg_text("shape", 0).unwrap_or("passive").to_string(),
                    size: text_size(e),
                });
            }
            Some("text") => {
                let (p, angle) = at(e);
                s.notes.push(Note {
                    text: e
                        .args()
                        .first()
                        .and_then(|v| v.text())
                        .unwrap_or("")
                        .to_string(),
                    at: p,
                    angle,
                    size: text_size(e),
                    justify: justify(e),
                });
            }
            Some("polyline") => s.lines.push(pts(e, false)),
            Some("bus_alias") => {
                let name = e
                    .args()
                    .first()
                    .and_then(|v| v.text())
                    .unwrap_or("")
                    .to_string();
                let members = e
                    .child("members")
                    .map(|m| {
                        m.args()
                            .iter()
                            .filter_map(|v| v.text().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                s.aliases.push((name, members));
            }
            Some("sheet") => {
                let (p, _) = at(e);
                let prop = |n: &str| {
                    e.children("property")
                        .find(|c| c.args().first().and_then(|v| v.text()) == Some(n))
                        .and_then(|c| c.args().get(1).and_then(|v| v.text()))
                        .unwrap_or("")
                        .to_string()
                };
                s.sheets.push(Sheet {
                    at: p,
                    size: point(e, "size", false),
                    name: prop("Sheetname"),
                    file: prop("Sheetfile"),
                    uuid: e.arg_text("uuid", 0).unwrap_or("").to_string(),
                    pins: e
                        .children("pin")
                        .map(|c| {
                            let (p, a) = at(c);
                            SheetPin {
                                name: c
                                    .args()
                                    .first()
                                    .and_then(|v| v.text())
                                    .unwrap_or("")
                                    .to_string(),
                                shape: c
                                    .args()
                                    .get(1)
                                    .and_then(|v| v.text())
                                    .unwrap_or("passive")
                                    .to_string(),
                                at: p,
                                angle: quarter(a),
                            }
                        })
                        .collect(),
                });
            }
            _ => {}
        }
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_symbols_wires_and_labels() {
        let s = read(
            r#"(kicad_sch (version 20250114) (uuid "u") (paper "A3")
              (lib_symbols (symbol "Device:R" (property "Description" "Resistor")
                (symbol "R_0_1" (rectangle (start -1 2.5) (end 1 -2.5) (fill (type none))))
                (symbol "R_1_1" (pin passive line (at 0 3.81 270) (length 1.27)
                  (name "~") (number "1")))))
              (symbol (lib_id "Device:R") (at 10 20 90) (mirror x) (unit 1)
                (property "Reference" "R1" (at 12 20 0)) (property "Value" "10k" (at 8 20 0)))
              (wire (pts (xy 0 0) (xy 10 0)))
              (global_label "SDA" (shape bidirectional) (at 10 0 180)))"#,
        )
        .unwrap();
        assert_eq!(s.paper, "A3");
        let r = &s.lib["Device:R"];
        assert_eq!((r.units, r.styles), (1, 1));
        assert_eq!(r.description, "Resistor");
        let Item::Pin(p) = &r.items[1].item else {
            panic!()
        };
        assert_eq!(
            (p.at, p.angle, p.length),
            (P { x: 0, y: -38_100 }, 270, 12_700)
        );
        let sym = &s.symbols[0];
        assert_eq!(
            (sym.at, sym.angle, sym.mirror),
            (
                P {
                    x: 100_000,
                    y: 200_000
                },
                90,
                Some('x')
            )
        );
        assert_eq!(sym.value_of("Reference"), "R1");
        assert_eq!(s.wires.len(), 1);
        assert_eq!(s.labels[0].kind, LabelKind::Global);
    }
}
