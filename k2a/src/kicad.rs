//! The parts of a .kicad_pcb that the converter carries over, in KiCad's own terms:
//! nanometres, y pointing down, angles in degrees counter-clockwise as seen on screen.

use crate::sexpr::Sexpr;
use std::collections::HashMap;

pub type Nm = i64;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pt {
    pub x: Nm,
    pub y: Nm,
}

impl Pt {
    pub fn new(x: Nm, y: Nm) -> Pt {
        Pt { x, y }
    }
}

fn mm(v: f64) -> Nm {
    (v * 1e6).round() as Nm
}

/// Rotate a point about the origin the way KiCad does (y down, positive = counter-clockwise)
pub fn rotate(p: Pt, deg: f64) -> Pt {
    if deg == 0.0 {
        return p;
    }
    let (s, c) = deg.to_radians().sin_cos();
    let (x, y) = (p.x as f64, p.y as f64);
    Pt {
        x: (x * c + y * s).round() as Nm,
        y: (-x * s + y * c).round() as Nm,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub at: Pt,
    pub rot: f64,
}

impl Placement {
    pub fn apply(&self, p: Pt) -> Pt {
        let r = rotate(p, self.rot);
        Pt {
            x: r.x + self.at.x,
            y: r.y + self.at.y,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line { a: Pt, b: Pt },
    Arc { start: Pt, mid: Pt, end: Pt },
    Circle { center: Pt, radius: Nm },
    Rect { a: Pt, b: Pt },
    Poly(Vec<Pt>),
}

#[derive(Debug, Clone)]
pub struct Drawing {
    pub layer: String,
    pub shape: Shape,
    pub width: Nm,
    pub filled: bool,
    pub net: usize,
}

#[derive(Debug, Clone)]
pub struct Track {
    pub layer: String,
    pub start: Pt,
    pub end: Pt,
    pub width: Nm,
    pub net: usize,
}

#[derive(Debug, Clone)]
pub struct ArcTrack {
    pub layer: String,
    pub start: Pt,
    pub mid: Pt,
    pub end: Pt,
    pub width: Nm,
    pub net: usize,
}

#[derive(Debug, Clone)]
pub struct Via {
    pub at: Pt,
    pub size: Nm,
    pub drill: Nm,
    pub top: String,
    pub bottom: String,
    pub net: usize,
    pub tented_front: bool,
    pub tented_back: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadKind {
    Smd,
    Thru,
    Npth,
    Connect,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadShape {
    Circle,
    Rect,
    Oval,
    RoundRect,
    Trapezoid,
    ChamferedRect,
    Custom,
}

#[derive(Debug, Clone)]
pub struct Pad {
    pub number: String,
    pub kind: PadKind,
    pub shape: PadShape,
    /// Absolute position on the board
    pub at: Pt,
    /// Absolute orientation (KiCad stores it that way)
    pub rot: f64,
    pub size: Pt,
    /// Drill width and height (equal for a round hole); zero for SMD
    pub drill: Pt,
    pub oval_drill: bool,
    /// Offset of the copper from the hole, in the pad's own frame
    pub offset: Pt,
    pub layers: Vec<String>,
    pub rratio: f64,
    pub net: usize,
    pub mask_margin: Option<Nm>,
    pub paste_margin: Option<Nm>,
    /// Custom pads: the shapes beyond the anchor, at their place on the board
    pub primitives: Vec<Drawing>,
    /// Custom pads: the anchor is round (else rectangular)
    pub anchor_circle: bool,
}

impl Pad {
    pub fn on(&self, layer: &str) -> bool {
        self.layers.iter().any(|l| {
            l == layer
                || (l == "*.Cu" && layer.ends_with(".Cu"))
                || (l == "F&B.Cu" && (layer == "F.Cu" || layer == "B.Cu"))
                || (l == "*.Mask" && layer.ends_with(".Mask"))
                || (l == "*.Paste" && layer.ends_with(".Paste"))
        })
    }
}

#[derive(Debug, Clone)]
pub struct Text {
    pub layer: String,
    pub text: String,
    pub at: Pt,
    pub rot: f64,
    pub height: Nm,
    /// Glyph width (KiCad's text size is height then width)
    pub width: Nm,
    pub thickness: Nm,
    pub mirrored: bool,
    pub visible: bool,
    /// -1 left, 0 centre, 1 right
    pub h_justify: i8,
    /// -1 top, 0 centre, 1 bottom
    pub v_justify: i8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// As written: a path with ${VARIABLES}, or kicad-embed://name
    pub path: String,
    /// mm, KiCad's 3D frame (y up)
    pub offset: [f64; 3],
    pub scale: [f64; 3],
    /// Degrees
    pub rotate: [f64; 3],
    pub hide: bool,
    pub opacity: f64,
}

#[derive(Debug, Clone)]
pub struct Footprint {
    pub lib_id: String,
    pub reference: String,
    pub value: String,
    pub place: Placement,
    pub back: bool,
    pub locked: bool,
    pub pads: Vec<Pad>,
    /// Graphics, already at their place on the board
    pub drawings: Vec<Drawing>,
    pub texts: Vec<Text>,
    pub reference_text: Option<Text>,
    pub value_text: Option<Text>,
    pub models: Vec<Model>,
    /// Files embedded in the footprint: name -> base64 of zstd
    pub embedded: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub net: usize,
    pub layers: Vec<String>,
    pub outline: Vec<Pt>,
    pub keepout: bool,
    /// Rule areas: what is kept out (vias, tracks, copper pours, pads, footprints)
    pub no_vias: bool,
    pub no_tracks: bool,
    pub no_pour: bool,
    pub no_pads: bool,
    pub no_footprints: bool,
    pub priority: i64,
    pub clearance: Nm,
    /// How pads join the fill: "thermal" (KiCad's default), "solid", "none", "thru_hole_only"
    pub connect: String,
    pub thermal_gap: Nm,
    pub thermal_bridge: Nm,
    pub min_thickness: Nm,
    /// Filled copper per layer: outline polygons as KiCad saved them
    pub fills: Vec<(String, Vec<Pt>)>,
}

#[derive(Debug, Default)]
pub struct Board {
    /// Copper layers in stack order: F.Cu, In1.Cu, ..., B.Cu
    pub copper: Vec<String>,
    /// Canonical layer name -> the name the user gave it
    pub layer_names: HashMap<String, String>,
    pub thickness: Nm,
    /// Board setup: mask and paste expansion of pads, minimum mask web
    pub mask_expansion: Nm,
    pub paste_expansion: Nm,
    pub paste_ratio: f64,
    pub mask_min_width: Nm,
    /// Net names; index 0 is "no net"
    pub nets: Vec<String>,
    pub tracks: Vec<Track>,
    pub arcs: Vec<ArcTrack>,
    pub vias: Vec<Via>,
    pub footprints: Vec<Footprint>,
    pub drawings: Vec<Drawing>,
    pub texts: Vec<Text>,
    pub zones: Vec<Zone>,
    /// Files embedded in the board: name -> base64 of zstd
    pub embedded: HashMap<String, String>,
}

#[derive(Debug)]
pub struct BoardError(pub String);

impl std::fmt::Display for BoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BoardError {}

fn xy(e: &Sexpr, name: &str) -> Option<Pt> {
    let c = e.child(name)?;
    Some(Pt::new(
        mm(c.args().first()?.num()?),
        mm(c.args().get(1)?.num()?),
    ))
}

fn xy_of(c: &Sexpr) -> Option<Pt> {
    Some(Pt::new(
        mm(c.args().first()?.num()?),
        mm(c.args().get(1)?.num()?),
    ))
}

/// Line width of a graphic: `(stroke (width w))` in current files, `(width w)` in older ones
fn width(e: &Sexpr) -> Nm {
    e.child("stroke")
        .and_then(|s| s.arg_num("width", 0))
        .or_else(|| e.arg_num("width", 0))
        .map(mm)
        .unwrap_or(0)
}

fn filled(e: &Sexpr) -> bool {
    match e.child("fill") {
        None => false,
        Some(f) => match f.args().first().and_then(|a| a.text()) {
            Some("yes") | Some("solid") => true,
            Some("type") | None => f.arg_text("type", 0) == Some("solid"),
            _ => false,
        },
    }
}

/// Points of a `(pts (xy ..) (arc (start ..) (mid ..) (end ..)) ...)`, arcs broken into lines
fn pts(e: &Sexpr) -> Vec<Pt> {
    let mut out = Vec::new();
    let Some(p) = e.child("pts") else { return out };
    for c in p.args() {
        match c.head() {
            Some("xy") => out.extend(xy_of(c)),
            Some("arc") => {
                if let (Some(s), Some(m), Some(en)) = (xy(c, "start"), xy(c, "mid"), xy(c, "end")) {
                    out.extend(crate::geom::arc_to_points(s, m, en, 50_000));
                }
            }
            _ => {}
        }
    }
    out
}

struct Nets {
    index: HashMap<String, usize>,
    names: Vec<String>,
    by_number: HashMap<i64, usize>,
}

impl Nets {
    fn new() -> Nets {
        Nets {
            index: HashMap::from([(String::new(), 0)]),
            names: vec![String::new()],
            by_number: HashMap::new(),
        }
    }

    fn intern(&mut self, name: &str) -> usize {
        if let Some(&i) = self.index.get(name) {
            return i;
        }
        self.names.push(name.to_string());
        self.index.insert(name.to_string(), self.names.len() - 1);
        self.names.len() - 1
    }

    /// `(net "NAME")` (KiCad 10) or `(net 3 "NAME")` / `(net 3)` (older)
    fn of(&mut self, e: &Sexpr) -> usize {
        let Some(n) = e.child("net") else { return 0 };
        let a = n.args();
        match (a.first(), a.get(1)) {
            (Some(Sexpr::Str(s)), None) => self.intern(s),
            (Some(num), Some(name)) => {
                let i = self.intern(name.text().unwrap_or(""));
                if let Some(k) = num.num() {
                    self.by_number.insert(k as i64, i);
                }
                i
            }
            (Some(num @ Sexpr::Atom(_)), None) => num
                .num()
                .and_then(|k| self.by_number.get(&(k as i64)).copied())
                .unwrap_or(0),
            _ => 0,
        }
    }
}

fn layer_of(e: &Sexpr) -> String {
    e.arg_text("layer", 0).unwrap_or("").to_string()
}

fn graphic(e: &Sexpr, place: Option<&Placement>, nets: &mut Nets) -> Option<Drawing> {
    let kind = e.head()?;
    let kind = kind
        .strip_prefix("gr_")
        .or_else(|| kind.strip_prefix("fp_"))?;
    let t = |p: Pt| place.map(|pl| pl.apply(p)).unwrap_or(p);
    let shape = match kind {
        "line" => Shape::Line {
            a: t(xy(e, "start")?),
            b: t(xy(e, "end")?),
        },
        "arc" => {
            // Files before KiCad 6 describe an arc by centre, start and angle
            if let (Some(s), Some(m), Some(en)) = (xy(e, "start"), xy(e, "mid"), xy(e, "end")) {
                Shape::Arc {
                    start: t(s),
                    mid: t(m),
                    end: t(en),
                }
            } else {
                let c = xy(e, "start")?;
                let s = xy(e, "end")?;
                let ang = e.arg_num("angle", 0)?;
                let rel = Pt::new(s.x - c.x, s.y - c.y);
                let m = rotate(rel, -ang / 2.0);
                let en = rotate(rel, -ang);
                Shape::Arc {
                    start: t(s),
                    mid: t(Pt::new(c.x + m.x, c.y + m.y)),
                    end: t(Pt::new(c.x + en.x, c.y + en.y)),
                }
            }
        }
        "circle" => {
            let c = xy(e, "center")?;
            let p = xy(e, "end")?;
            let r = (((p.x - c.x) as f64).hypot((p.y - c.y) as f64)).round() as Nm;
            Shape::Circle {
                center: t(c),
                radius: r,
            }
        }
        "rect" => {
            let a = xy(e, "start")?;
            let b = xy(e, "end")?;
            if place.map(|p| p.rot != 0.0).unwrap_or(false) {
                // A rotated footprint turns its rectangles into general quadrilaterals
                Shape::Poly(vec![t(a), t(Pt::new(b.x, a.y)), t(b), t(Pt::new(a.x, b.y))])
            } else {
                Shape::Rect { a: t(a), b: t(b) }
            }
        }
        "poly" => Shape::Poly(pts(e).into_iter().map(t).collect()),
        "curve" => {
            let p = pts(e);
            if p.len() != 4 {
                return None;
            }
            Shape::Poly(
                crate::geom::bezier_to_points(p[0], p[1], p[2], p[3])
                    .into_iter()
                    .map(t)
                    .collect(),
            )
        }
        _ => return None,
    };
    let mut d = Drawing {
        layer: layer_of(e),
        shape,
        width: width(e),
        filled: filled(e),
        net: nets.of(e),
    };
    if kind == "curve" {
        d.filled = false;
    }
    Some(d)
}

fn text(e: &Sexpr, value: &str, place: Option<&Placement>) -> Option<Text> {
    let at = e.child("at")?;
    let a = at.args();
    let local = Pt::new(mm(a.first()?.num()?), mm(a.get(1)?.num()?));
    let rot = a.get(2).and_then(|v| v.num()).unwrap_or(0.0);
    let font = e.child("effects").and_then(|f| f.child("font"));
    let height = font
        .and_then(|f| f.arg_num("size", 0))
        .map(mm)
        .unwrap_or(1_000_000);
    let width = font
        .and_then(|f| f.arg_num("size", 1))
        .map(mm)
        .unwrap_or(height);
    let thickness = font
        .and_then(|f| f.arg_num("thickness", 0))
        .map(mm)
        .unwrap_or(height / 8);
    let justify = e.child("effects").and_then(|f| f.child("justify"));
    let has = |w: &str| justify.map(|j| j.has_atom(w)).unwrap_or(false);
    let mirrored = has("mirror");
    let h_justify = if has("left") {
        -1
    } else if has("right") {
        1
    } else {
        0
    };
    let v_justify = if has("top") {
        -1
    } else if has("bottom") {
        1
    } else {
        0
    };
    let hidden = e.has_atom("hide")
        || e.flag("hide").unwrap_or(false)
        || e.child("effects")
            .map(|f| f.has_atom("hide") || f.flag("hide").unwrap_or(false))
            .unwrap_or(false);
    Some(Text {
        layer: layer_of(e),
        text: value.to_string(),
        at: place.map(|p| p.apply(local)).unwrap_or(local),
        // Footprint texts store their angle including the footprint's (KiCad 6 and later)
        rot,
        height,
        width,
        thickness,
        mirrored,
        visible: !hidden,
        h_justify,
        v_justify,
    })
}

fn pad(e: &Sexpr, place: &Placement, nets: &mut Nets) -> Option<Pad> {
    let a = e.args();
    let number = a.first()?.text()?.to_string();
    let kind = match a.get(1)?.text()? {
        "smd" => PadKind::Smd,
        "thru_hole" => PadKind::Thru,
        "np_thru_hole" => PadKind::Npth,
        "connect" => PadKind::Connect,
        _ => return None,
    };
    let shape = match a.get(2)?.text()? {
        "circle" => PadShape::Circle,
        "rect" => PadShape::Rect,
        "oval" => PadShape::Oval,
        "roundrect" => PadShape::RoundRect,
        "trapezoid" => PadShape::Trapezoid,
        "chamfered_rect" => PadShape::ChamferedRect,
        _ => PadShape::Custom,
    };
    let at = e.child("at")?.args();
    let local = Pt::new(mm(at.first()?.num()?), mm(at.get(1)?.num()?));
    let rot = at.get(2).and_then(|v| v.num()).unwrap_or(0.0);
    let size = xy(e, "size").unwrap_or_default();
    let (mut drill, mut oval_drill, mut offset) = (Pt::default(), false, Pt::default());
    if let Some(d) = e.child("drill") {
        let mut nums = Vec::new();
        for v in d.args() {
            match v {
                Sexpr::Atom(s) if s == "oval" => oval_drill = true,
                Sexpr::List(_) if v.head() == Some("offset") => {
                    offset = xy_of(v).unwrap_or_default()
                }
                _ => nums.extend(v.num()),
            }
        }
        if let Some(&w) = nums.first() {
            let h = nums.get(1).copied().unwrap_or(w);
            drill = Pt::new(mm(w), mm(h));
        }
    }
    let layers: Vec<String> = e
        .child("layers")
        .map(|l| {
            l.args()
                .iter()
                .filter_map(|v| v.text().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let abs = place.apply(local);
    let pad_place = Placement { at: abs, rot };
    let mut primitives = Vec::new();
    if let Some(pr) = e.child("primitives") {
        for g in pr.args() {
            if let Some(mut d) = graphic(g, Some(&pad_place), nets) {
                // gr_poly in a pad is filled unless it says otherwise; a 0 width outline too
                if g.head() == Some("gr_poly") && g.child("fill").is_none() {
                    d.filled = true;
                }
                primitives.push(d);
            }
        }
    }
    let anchor_circle = e.child("options").and_then(|o| o.arg_text("anchor", 0)) == Some("circle");
    Some(Pad {
        number,
        kind,
        shape,
        at: abs,
        rot,
        size,
        drill,
        oval_drill,
        offset,
        layers,
        rratio: e.arg_num("roundrect_rratio", 0).unwrap_or(0.25),
        net: nets.of(e),
        mask_margin: e.arg_num("solder_mask_margin", 0).map(mm),
        paste_margin: e.arg_num("solder_paste_margin", 0).map(mm),
        primitives,
        anchor_circle,
    })
}

fn footprint(e: &Sexpr, nets: &mut Nets) -> Option<Footprint> {
    let lib_id = e.args().first()?.text()?.to_string();
    let at = e.child("at")?.args();
    let place = Placement {
        at: Pt::new(mm(at.first()?.num()?), mm(at.get(1)?.num()?)),
        rot: at.get(2).and_then(|v| v.num()).unwrap_or(0.0),
    };
    let back = layer_of(e) == "B.Cu";
    let mut fp = Footprint {
        lib_id,
        reference: String::new(),
        value: String::new(),
        place,
        back,
        locked: e.has_atom("locked") || e.flag("locked").unwrap_or(false),
        pads: Vec::new(),
        drawings: Vec::new(),
        texts: Vec::new(),
        reference_text: None,
        value_text: None,
        models: Vec::new(),
        embedded: HashMap::new(),
    };
    for c in e.args() {
        match c.head() {
            Some("pad") => fp.pads.extend(pad(c, &place, nets)),
            Some("property") => {
                let a = c.args();
                let (Some(k), Some(v)) = (
                    a.first().and_then(|x| x.text()),
                    a.get(1).and_then(|x| x.text()),
                ) else {
                    continue;
                };
                match k {
                    "Reference" => {
                        fp.reference = v.to_string();
                        fp.reference_text = text(c, v, Some(&place));
                    }
                    "Value" => {
                        fp.value = v.to_string();
                        fp.value_text = text(c, v, Some(&place));
                    }
                    _ => {}
                }
            }
            Some("fp_text") => {
                let a = c.args();
                let (Some(k), Some(v)) = (
                    a.first().and_then(|x| x.text()),
                    a.get(1).and_then(|x| x.text()),
                ) else {
                    continue;
                };
                match k {
                    "reference" => {
                        fp.reference = v.to_string();
                        fp.reference_text = text(c, v, Some(&place));
                    }
                    "value" => {
                        fp.value = v.to_string();
                        fp.value_text = text(c, v, Some(&place));
                    }
                    _ => fp.texts.extend(text(c, v, Some(&place))),
                }
            }
            Some(h) if h.starts_with("fp_") => fp.drawings.extend(graphic(c, Some(&place), nets)),
            Some("model") => fp.models.extend(model(c)),
            Some("embedded_files") => fp.embedded.extend(embedded_files(c)),
            _ => {}
        }
    }
    Some(fp)
}

fn xyz(e: &Sexpr, name: &str, default: f64) -> [f64; 3] {
    let v = e.child(name).and_then(|c| c.child("xyz"));
    let n = |i: usize| {
        v.and_then(|x| x.args().get(i))
            .and_then(|a| a.num())
            .unwrap_or(default)
    };
    [n(0), n(1), n(2)]
}

fn model(e: &Sexpr) -> Option<Model> {
    Some(Model {
        path: e.args().first()?.text()?.to_string(),
        offset: xyz(e, "offset", 0.0),
        scale: xyz(e, "scale", 1.0),
        rotate: xyz(e, "rotate", 0.0),
        hide: e.has_atom("hide") || e.flag("hide").unwrap_or(false),
        opacity: e.arg_num("opacity", 0).unwrap_or(1.0),
    })
}

/// `(embedded_files (file (name ..) (type ..) (data |..|)) ...)`: name -> data, where present
fn embedded_files(e: &Sexpr) -> Vec<(String, String)> {
    e.children("file")
        .filter_map(|f| {
            Some((
                f.arg_text("name", 0)?.to_string(),
                f.arg_text("data", 0)?.to_string(),
            ))
        })
        .collect()
}

fn zone(e: &Sexpr, nets: &mut Nets) -> Zone {
    let layers = match e.child("layers") {
        Some(l) => l
            .args()
            .iter()
            .filter_map(|v| v.text().map(String::from))
            .collect(),
        None => vec![layer_of(e)],
    };
    let fills = e
        .children("filled_polygon")
        .map(|f| {
            (
                f.arg_text("layer", 0).unwrap_or(&layers[0]).to_string(),
                pts(f),
            )
        })
        .collect();
    let ko = e.child("keepout");
    let banned = |what: &str| {
        ko.and_then(|k| k.arg_text(what, 0))
            .map(|v| v == "not_allowed")
            .unwrap_or(false)
    };
    Zone {
        net: nets.of(e),
        layers,
        outline: e.child("polygon").map(pts).unwrap_or_default(),
        keepout: ko.is_some(),
        no_vias: banned("vias"),
        no_tracks: banned("tracks"),
        no_pour: banned("copperpour"),
        no_pads: banned("pads"),
        no_footprints: banned("footprints"),
        priority: e.arg_num("priority", 0).unwrap_or(0.0) as i64,
        clearance: e
            .child("connect_pads")
            .and_then(|c| c.arg_num("clearance", 0))
            .map(mm)
            .unwrap_or(0),
        connect: match e.child("connect_pads").and_then(|c| c.args().first()) {
            Some(Sexpr::Atom(t)) if t == "yes" => "solid",
            Some(Sexpr::Atom(t)) if t == "no" => "none",
            Some(Sexpr::Atom(t)) if t == "thru_hole_only" => "thru_hole_only",
            _ => "thermal",
        }
        .to_string(),
        thermal_gap: e
            .child("fill")
            .and_then(|f| f.arg_num("thermal_gap", 0))
            .map(mm)
            .unwrap_or(500_000),
        thermal_bridge: e
            .child("fill")
            .and_then(|f| f.arg_num("thermal_bridge_width", 0))
            .map(mm)
            .unwrap_or(500_000),
        min_thickness: e.arg_num("min_thickness", 0).map(mm).unwrap_or(250_000),
        fills,
    }
}

pub fn read(source: &str) -> Result<Board, Box<dyn std::error::Error>> {
    let root = crate::sexpr::parse(source)?;
    if root.head() != Some("kicad_pcb") {
        return Err(Box::new(BoardError("not a KiCad board file".into())));
    }
    let mut b = Board::default();
    let mut nets = Nets::new();

    b.thickness = root
        .child("general")
        .and_then(|g| g.arg_num("thickness", 0))
        .map(mm)
        .unwrap_or(1_600_000);

    if let Some(layers) = root.child("layers") {
        let mut inner = Vec::new();
        for l in layers.args() {
            let a = l.args();
            let Some(name) = a.first().and_then(|v| v.text()) else {
                continue;
            };
            let user = a.get(2).and_then(|v| v.text()).unwrap_or("");
            b.layer_names.insert(name.to_string(), user.to_string());
            if name.starts_with("In") && name.ends_with(".Cu") {
                inner.push(name.to_string());
            }
        }
        inner.sort_by_key(|n| n[2..n.len() - 3].parse::<u32>().unwrap_or(0));
        b.copper.push("F.Cu".into());
        b.copper.extend(inner);
        b.copper.push("B.Cu".into());
    }

    // Board-level net list (files before KiCad 10)
    for n in root.children("net") {
        let a = n.args();
        if let (Some(k), Some(name)) = (
            a.first().and_then(|v| v.num()),
            a.get(1).and_then(|v| v.text()),
        ) {
            let i = nets.intern(name);
            nets.by_number.insert(k as i64, i);
        }
    }

    for e in root.args() {
        match e.head() {
            Some("segment") => {
                if let (Some(s), Some(en)) = (xy(e, "start"), xy(e, "end")) {
                    b.tracks.push(Track {
                        layer: layer_of(e),
                        start: s,
                        end: en,
                        width: e.arg_num("width", 0).map(mm).unwrap_or(0),
                        net: nets.of(e),
                    });
                }
            }
            Some("arc") => {
                if let (Some(s), Some(m), Some(en)) = (xy(e, "start"), xy(e, "mid"), xy(e, "end")) {
                    b.arcs.push(ArcTrack {
                        layer: layer_of(e),
                        start: s,
                        mid: m,
                        end: en,
                        width: e.arg_num("width", 0).map(mm).unwrap_or(0),
                        net: nets.of(e),
                    });
                }
            }
            Some("via") => {
                let Some(at) = xy(e, "at") else { continue };
                let ls: Vec<String> = e
                    .child("layers")
                    .map(|l| {
                        l.args()
                            .iter()
                            .filter_map(|v| v.text().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let tent = e.child("tenting");
                b.vias.push(Via {
                    at,
                    size: e.arg_num("size", 0).map(mm).unwrap_or(0),
                    drill: e.arg_num("drill", 0).map(mm).unwrap_or(0),
                    top: ls.first().cloned().unwrap_or_else(|| "F.Cu".into()),
                    bottom: ls.last().cloned().unwrap_or_else(|| "B.Cu".into()),
                    net: nets.of(e),
                    tented_front: tent.map(|t| t.has_atom("front")).unwrap_or(false),
                    tented_back: tent.map(|t| t.has_atom("back")).unwrap_or(false),
                });
            }
            Some("footprint") | Some("module") => b.footprints.extend(footprint(e, &mut nets)),
            Some("zone") => b.zones.push(zone(e, &mut nets)),
            Some("embedded_files") => b.embedded.extend(embedded_files(e)),
            Some("gr_text") => {
                if let Some(t) = e.args().first().and_then(|v| v.text()) {
                    b.texts.extend(text(e, t, None));
                }
            }
            Some(h) if h.starts_with("gr_") => b.drawings.extend(graphic(e, None, &mut nets)),
            _ => {}
        }
    }
    b.nets = nets.names;
    Ok(b)
}
