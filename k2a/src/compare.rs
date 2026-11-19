//! Compare two KiCad boards object by object: the original and the one KiCad's Altium importer
//! makes of our PcbDoc. The importer centres the board on its page, so the boards are first
//! aligned on their outlines.

use crate::kicad::{Board, Nm, PadKind, PadShape, Pt, Shape};
use std::collections::HashMap;

/// Layers that have no Altium layer kind, so they come back on whichever user layer is free
const UNKINDED: &[&str] = &["Eco1.User", "Eco2.User", "Margin"];

/// Altium stores 2.54 nm steps and KiCad's importer rounds to 10 nm
pub const TOL: Nm = 100;

/// KiCad's importer joins track ends that Altium left this close apart
pub const SNAP: Nm = 10_100;

#[derive(Debug, Default)]
pub struct Category {
    pub name: &'static str,
    pub total: usize,
    pub missing: usize,
    pub extra: usize,
    pub examples: Vec<String>,
}

impl Category {
    fn new(name: &'static str) -> Self {
        Category {
            name,
            ..Default::default()
        }
    }
    fn miss(&mut self, what: String) {
        self.missing += 1;
        if self.examples.len() < 5 {
            self.examples.push(what);
        }
    }
    pub fn ok(&self) -> bool {
        self.missing == 0 && self.extra == 0
    }
}

#[derive(Debug, Default)]
pub struct Comparison {
    pub categories: Vec<Category>,
}

impl Comparison {
    pub fn get(&self, name: &str) -> &Category {
        self.categories
            .iter()
            .find(|c| c.name == name)
            .expect("category")
    }
    pub fn summary(&self) -> String {
        self.categories
            .iter()
            .map(|c| {
                if c.ok() {
                    format!("{} {}/{}", c.name, c.total, c.total)
                } else {
                    format!(
                        "{} {}/{} (-{} +{})",
                        c.name,
                        c.total - c.missing,
                        c.total,
                        c.missing,
                        c.extra
                    )
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn close(a: Pt, b: Pt) -> bool {
    (a.x - b.x).abs() <= TOL && (a.y - b.y).abs() <= TOL
}

fn near(a: Pt, b: Pt, tol: Nm) -> bool {
    (a.x - b.x).abs() <= tol && (a.y - b.y).abs() <= tol
}

fn closen(a: Nm, b: Nm) -> bool {
    (a - b).abs() <= TOL
}

fn angle_eq(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(360.0);
    d < 0.01 || d > 359.99
}

/// Bounding box of everything on Edge.Cuts
fn edge_box(b: &Board) -> Option<(Pt, Pt)> {
    let mut min = Pt::new(Nm::MAX, Nm::MAX);
    let mut max = Pt::new(Nm::MIN, Nm::MIN);
    let mut any = false;
    for lp in crate::outline::loops(b) {
        for v in lp {
            for p in v.points() {
                min = Pt::new(min.x.min(p.x), min.y.min(p.y));
                max = Pt::new(max.x.max(p.x), max.y.max(p.y));
                any = true;
            }
        }
    }
    any.then_some((min, max))
}

/// A simple grid index over points for tolerant lookups
type Cells<T> = HashMap<(Nm, Nm), Vec<(Pt, T, bool)>>;

struct Grid<T> {
    cells: Cells<T>,
}

const CELL: Nm = 10_000;

impl<T: Clone> Grid<T> {
    fn new() -> Self {
        Grid {
            cells: HashMap::new(),
        }
    }
    fn insert(&mut self, p: Pt, v: T) {
        self.cells
            .entry((p.x.div_euclid(CELL), p.y.div_euclid(CELL)))
            .or_default()
            .push((p, v, false));
    }
    /// Take the first unused entry near p that satisfies f
    fn take(&mut self, p: Pt, f: impl FnMut(&T) -> bool) -> Option<T> {
        self.take_within(p, TOL, f)
    }

    fn take_within(&mut self, p: Pt, tol: Nm, mut f: impl FnMut(&T) -> bool) -> Option<T> {
        let (cx, cy) = (p.x.div_euclid(CELL), p.y.div_euclid(CELL));
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(v) = self.cells.get_mut(&(cx + dx, cy + dy)) {
                    for e in v.iter_mut() {
                        if !e.2 && near(e.0, p, tol) && f(&e.1) {
                            e.2 = true;
                            return Some(e.1.clone());
                        }
                    }
                }
            }
        }
        None
    }
    fn unused(&self) -> usize {
        self.cells.values().flatten().filter(|e| !e.2).count()
    }
}

pub fn compare(orig: &Board, back: &Board) -> Comparison {
    let shift = match (edge_box(orig), edge_box(back)) {
        (Some((a, _)), Some((b, _))) => Pt::new(b.x - a.x, b.y - a.y),
        _ => Pt::default(),
    };
    let mv = |p: Pt| Pt::new(p.x + shift.x, p.y + shift.y);
    if std::env::var_os("K2A_DEBUG").is_some() {
        eprintln!("shift {shift:?}");
    }
    let net = |b: &Board, i: usize| b.nets.get(i).cloned().unwrap_or_default();
    let mut out = Comparison::default();

    // Nets: every named net of the original, by name (unconnected single-pad nets aside)
    let mut c = Category::new("nets");
    let back_nets: std::collections::HashSet<&String> = back.nets.iter().collect();
    for n in orig.nets.iter().skip(1) {
        if n.starts_with("unconnected-")
            || n.starts_with("Net-(")
                && !back_nets.contains(n)
                && back.nets.len() >= orig.nets.len()
        {
            // KiCad's generated names may be re-generated differently
            c.total += 1;
            continue;
        }
        c.total += 1;
        if !back_nets.contains(n) {
            c.miss(n.clone());
        }
    }
    out.categories.push(c);

    // Footprints by reference; then their pads
    let mut cf = Category::new("footprints");
    let mut cp = Category::new("pads");
    // By reference; several with one reference (or none) by the nearest still unmatched
    let mut by_ref: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, f) in back.footprints.iter().enumerate() {
        by_ref.entry(f.reference.as_str()).or_default().push(i);
    }
    let dist = |a: Pt, b: Pt| ((a.x - b.x) as f64).hypot((a.y - b.y) as f64);
    for f in &orig.footprints {
        cf.total += 1;
        let copper = |p: &&crate::kicad::Pad| !is_aperture(p);
        cp.total += f.pads.iter().filter(copper).count();
        let pick = by_ref.get_mut(f.reference.as_str()).and_then(|v| {
            let at = mv(f.place.at);
            let k = (0..v.len()).min_by(|&a, &b| {
                dist(back.footprints[v[a]].place.at, at)
                    .total_cmp(&dist(back.footprints[v[b]].place.at, at))
            })?;
            Some(v.swap_remove(k))
        });
        let Some(g) = pick.map(|i| &back.footprints[i]) else {
            cf.miss(format!("{} not found", f.reference));
            cp.missing += f.pads.iter().filter(copper).count();
            continue;
        };
        if !close(mv(f.place.at), g.place.at)
            || !angle_eq(f.place.rot, g.place.rot)
            || f.back != g.back
        {
            cf.miss(format!(
                "{}: at {:?} rot {} back {} -> {:?} rot {} back {}",
                f.reference,
                mv(f.place.at),
                f.place.rot,
                f.back,
                g.place.at,
                g.place.rot,
                g.back
            ));
        }
        let mut grid = Grid::new();
        for (i, p) in g.pads.iter().enumerate() {
            grid.insert(p.at, i);
        }
        for p in f.pads.iter().filter(copper) {
            let found = grid.take(mv(p.at), |&i| g.pads[i].number == p.number);
            let Some(i) = found else {
                cp.miss(format!(
                    "{} pad {} at {:?} not found",
                    f.reference,
                    p.number,
                    mv(p.at)
                ));
                continue;
            };
            let q = &g.pads[i];
            let mut why = Vec::new();
            let (ps, qs) = (shape_class(p), shape_class(q));
            // A circle has no orientation
            let rot_ok = angle_eq(p.rot, q.rot)
                || (ps == "circle" && qs == "circle")
                || (p.size.x == p.size.y && angle_eq(p.rot, q.rot + 90.0))
                || angle_eq(p.rot, q.rot + 180.0) && ps != "custom";
            if !rot_ok {
                why.push(format!("rot {} -> {}", p.rot, q.rot));
            }
            let size_ok = (closen(p.size.x, q.size.x) && closen(p.size.y, q.size.y))
                || (p.kind == PadKind::Npth && closen(q.size.x, p.drill.x));
            if !size_ok {
                why.push(format!("size {:?} -> {:?}", p.size, q.size));
            }
            if !(closen(p.drill.x, q.drill.x) && closen(p.drill.y, q.drill.y)) {
                why.push(format!("drill {:?} -> {:?}", p.drill, q.drill));
            }
            // A custom pad comes back as its anchor plus copper shapes
            let anchor = if p.anchor_circle { "circle" } else { "rect" };
            if ps == "custom" && qs == anchor {
            } else if ps != qs && !(ps == "roundrect" && qs == "roundrect") {
                why.push(format!("shape {ps} -> {qs}"));
            } else if ps == "roundrect" && (p.rratio - q.rratio).abs() > 0.006 {
                why.push(format!("corner ratio {} -> {}", p.rratio, q.rratio));
            }
            if kind_class(p.kind) != kind_class(q.kind) {
                why.push(format!("kind {:?} -> {:?}", p.kind, q.kind));
            }
            if net(orig, p.net) != net(back, q.net)
                && !(p.net == 0 && net(back, q.net).starts_with("unconnected-"))
            {
                why.push(format!("net {} -> {}", net(orig, p.net), net(back, q.net)));
            }
            if !why.is_empty() {
                cp.miss(format!(
                    "{} pad {}: {}",
                    f.reference,
                    p.number,
                    why.join(", ")
                ));
            }
        }
    }
    cf.extra = back.footprints.len().saturating_sub(orig.footprints.len());
    out.categories.push(cf);
    out.categories.push(cp);

    // Tracks, as unordered segments
    let mut ct = Category::new("tracks");
    let mut grid = Grid::new();
    for (i, t) in back.tracks.iter().enumerate() {
        grid.insert(t.start, (i, false));
        grid.insert(t.end, (i, true));
    }
    let mut used = vec![false; back.tracks.len()];
    // Tracks shorter than the importer's snap distance can merge into their neighbours
    let tiny = |a: Pt, b: Pt| (a.x - b.x).abs() < 2 * SNAP && (a.y - b.y).abs() < 2 * SNAP;
    for t in orig.tracks.iter().filter(|t| !tiny(t.start, t.end)) {
        ct.total += 1;
        let (s, e) = (mv(t.start), mv(t.end));
        let mut found = None;
        for tol in [TOL, SNAP] {
            found = grid.take_within(s, tol, |&(i, rev)| {
                let q = &back.tracks[i];
                let other = if rev { q.start } else { q.end };
                !used[i]
                    && near(other, e, tol)
                    && q.layer == t.layer
                    && closen(q.width, t.width)
                    && net(back, q.net) == net(orig, t.net)
            });
            if found.is_some() {
                break;
            }
        }
        match found {
            Some((i, _)) => used[i] = true,
            None => ct.miss(format!(
                "{} {:?}-{:?} w {} net {}",
                t.layer,
                s,
                e,
                t.width,
                net(orig, t.net)
            )),
        }
    }
    // Footprint copper drawn as lines, arcs or circles (custom pad shapes too) comes back from
    // KiCad's importer as board tracks: those are expected, segment by segment
    let mut expect: Vec<(String, Pt, Pt)> = Vec::new();
    let mut allow_arcs = 0usize;
    for f in &orig.footprints {
        let mut drawn: Vec<crate::kicad::Drawing> = f
            .drawings
            .iter()
            .filter(|d| d.layer.ends_with(".Cu"))
            .cloned()
            .collect();
        for p in f.pads.iter().filter(|p| p.shape == PadShape::Custom) {
            let smd = matches!(p.kind, PadKind::Smd | PadKind::Connect);
            let cu: Vec<String> = if smd {
                vec![if p.on("B.Cu") && !p.on("F.Cu") {
                    "B.Cu".into()
                } else {
                    "F.Cu".into()
                }]
            } else {
                orig.copper.iter().filter(|c| p.on(c)).cloned().collect()
            };
            for c in cu {
                for d in &p.primitives {
                    let mut d = d.clone();
                    d.layer = c.clone();
                    drawn.push(d);
                }
            }
        }
        for d in drawn {
            let edges = |pts: &[Pt]| {
                (0..pts.len())
                    .map(|i| (pts[i], pts[(i + 1) % pts.len()]))
                    .collect::<Vec<_>>()
            };
            match &d.shape {
                Shape::Line { a, b } => expect.push((d.layer.clone(), mv(*a), mv(*b))),
                Shape::Arc { start, end, .. } => {
                    // A very flat arc comes back straight
                    allow_arcs += 1;
                    expect.push((d.layer.clone(), mv(*start), mv(*end)));
                }
                Shape::Circle { .. } => allow_arcs += 2,
                Shape::Rect { a, b } if !d.filled || d.width > 0 => {
                    for (u, v) in edges(&[*a, Pt::new(b.x, a.y), *b, Pt::new(a.x, b.y)]) {
                        expect.push((d.layer.clone(), mv(u), mv(v)));
                    }
                }
                Shape::Poly(p) if !d.filled || d.width > 0 => {
                    for (u, v) in edges(p) {
                        expect.push((d.layer.clone(), mv(u), mv(v)));
                    }
                }
                _ => {}
            }
        }
    }
    // Board drawings on copper come back as tracks as well
    for d in orig.drawings.iter().filter(|d| d.layer.ends_with(".Cu")) {
        let edges = |pts: &[Pt]| {
            (0..pts.len())
                .map(|i| (pts[i], pts[(i + 1) % pts.len()]))
                .collect::<Vec<_>>()
        };
        match &d.shape {
            Shape::Line { a, b } => expect.push((d.layer.clone(), mv(*a), mv(*b))),
            Shape::Arc { start, end, .. } => {
                allow_arcs += 1;
                expect.push((d.layer.clone(), mv(*start), mv(*end)));
            }
            Shape::Circle { .. } => allow_arcs += 2,
            Shape::Rect { a, b } if !d.filled || d.width > 0 => {
                for (u, v) in edges(&[*a, Pt::new(b.x, a.y), *b, Pt::new(a.x, b.y)]) {
                    expect.push((d.layer.clone(), mv(u), mv(v)));
                }
            }
            Shape::Poly(p) if !d.filled || d.width > 0 => {
                for (u, v) in edges(p) {
                    expect.push((d.layer.clone(), mv(u), mv(v)));
                }
            }
            _ => {}
        }
    }
    // Short tracks the importer snapped to their neighbours
    for t in orig.tracks.iter().filter(|t| tiny(t.start, t.end)) {
        expect.push((t.layer.clone(), mv(t.start), mv(t.end)));
    }
    let expected = |q: &crate::kicad::Track| {
        tiny(q.start, q.end)
            || expect.iter().any(|(l, u, v)| {
                *l == q.layer
                    && ((near(*u, q.start, SNAP) && near(*v, q.end, SNAP))
                        || (near(*u, q.end, SNAP) && near(*v, q.start, SNAP)))
            })
    };
    ct.extra = (0..back.tracks.len())
        .filter(|&i| !used[i] && !expected(&back.tracks[i]))
        .count();
    out.categories.push(ct);

    // Arc tracks
    let mut ca = Category::new("arcs");
    let mut agrid = Grid::new();
    for (i, a) in back.arcs.iter().enumerate() {
        agrid.insert(a.mid, i);
    }
    for a in &orig.arcs {
        ca.total += 1;
        let (s, m, e) = (mv(a.start), mv(a.mid), mv(a.end));
        let mut found = None;
        for tol in [TOL, SNAP] {
            found = agrid.take_within(m, tol, |&i| {
                let q = &back.arcs[i];
                ((near(q.start, s, tol) && near(q.end, e, tol))
                    || (near(q.start, e, tol) && near(q.end, s, tol)))
                    && q.layer == a.layer
                    && closen(q.width, a.width)
                    && net(back, q.net) == net(orig, a.net)
            });
            if found.is_some() {
                break;
            }
        }
        if found.is_none() {
            let straight = back.tracks.iter().enumerate().position(|(i, q)| {
                !used[i]
                    && q.layer == a.layer
                    && ((near(q.start, s, SNAP) && near(q.end, e, SNAP))
                        || (near(q.start, e, SNAP) && near(q.end, s, SNAP)))
            });
            match straight {
                Some(i) => used[i] = true,
                None => ca.miss(format!("{} {:?} {:?} {:?}", a.layer, s, m, e)),
            }
        }
    }
    ca.extra = agrid.unused().saturating_sub(allow_arcs);
    // Arcs that came back straight used up some tracks
    let extra_now = (0..back.tracks.len())
        .filter(|&i| !used[i] && !expected(&back.tracks[i]))
        .count();
    if let Some(c) = out.categories.iter_mut().find(|c| c.name == "tracks") {
        c.extra = extra_now;
        if c.extra > 0 && std::env::var_os("K2A_DEBUG").is_some() {
            for q in (0..back.tracks.len())
                .filter(|&i| !used[i] && !expected(&back.tracks[i]))
                .map(|i| &back.tracks[i])
            {
                c.examples.push(format!(
                    "extra {} {:?}-{:?} w {} net {}",
                    q.layer,
                    q.start,
                    q.end,
                    q.width,
                    net(back, q.net)
                ));
            }
        }
    }
    out.categories.push(ca);

    // Vias
    let mut cv = Category::new("vias");
    let mut vgrid = Grid::new();
    for (i, v) in back.vias.iter().enumerate() {
        vgrid.insert(v.at, i);
    }
    for v in &orig.vias {
        cv.total += 1;
        let found = vgrid.take(mv(v.at), |&i| {
            let q = &back.vias[i];
            closen(q.size, v.size)
                && closen(q.drill, v.drill)
                && net(back, q.net) == net(orig, v.net)
                && q.top == v.top
                && q.bottom == v.bottom
        });
        if found.is_none() {
            cv.miss(format!(
                "{:?} {} / {} {}",
                mv(v.at),
                v.size,
                v.drill,
                net(orig, v.net)
            ));
        }
    }
    cv.extra = vgrid.unused();
    out.categories.push(cv);

    // Zones: same layer and net, and the same filled area within 0.1 %
    let mut cz = Category::new("zone fills");
    let area_of = |b: &Board, layer: &str, n: &str| -> f64 {
        b.zones
            .iter()
            .filter(|z| !z.keepout && net(b, z.net) == n)
            .flat_map(|z| z.fills.iter())
            .filter(|(l, _)| l == layer)
            .map(|(_, p)| poly_area(p))
            .sum()
    };
    let mut seen = std::collections::HashSet::new();
    for z in orig.zones.iter().filter(|z| !z.keepout) {
        for (l, _) in &z.fills {
            let key = (l.clone(), net(orig, z.net));
            if !seen.insert(key.clone()) {
                continue;
            }
            cz.total += 1;
            let (a, b) = (area_of(orig, &key.0, &key.1), area_of(back, &key.0, &key.1));
            if (a - b).abs() > a.max(1.0) * 1e-3 {
                cz.miss(format!(
                    "{} {}: {:.3} mm2 -> {:.3} mm2",
                    key.0,
                    key.1,
                    a / 1e12,
                    b / 1e12
                ));
            }
        }
    }
    out.categories.push(cz);

    // Rule areas: layers, restrictions and outline area
    let mut ck = Category::new("keepouts");
    let sig = |b: &Board, z: &crate::kicad::Zone| {
        let mut ls: Vec<String> = Vec::new();
        for l in &z.layers {
            match l.as_str() {
                "*.Cu" => ls.extend(b.copper.iter().cloned()),
                "F&B.Cu" => ls.extend(["F.Cu".to_string(), "B.Cu".to_string()]),
                l if l.ends_with(".Cu") => ls.push(l.to_string()),
                _ => {}
            }
        }
        ls.sort();
        ls.dedup();
        (ls, z.no_vias, z.no_tracks, z.no_pour, z.no_pads)
    };
    let mut kb: Vec<_> = back
        .zones
        .iter()
        .filter(|z| z.keepout)
        .map(|z| (sig(back, z), poly_area(&z.outline)))
        .collect();
    for z in orig
        .zones
        .iter()
        .filter(|z| z.keepout && (z.no_vias || z.no_tracks || z.no_pour || z.no_pads))
    {
        let (s, a) = (sig(orig, z), poly_area(&z.outline));
        // Altium keep-outs are on one copper layer or on all of them
        let parts: Vec<_> = if s.0.len() == orig.copper.len() {
            vec![s]
        } else {
            s.0.iter()
                .map(|l| (vec![l.clone()], s.1, s.2, s.3, s.4))
                .collect()
        };
        for s in parts {
            ck.total += 1;
            match kb
                .iter()
                .position(|(t, b)| *t == s && (a - b).abs() <= a.max(1.0) * 1e-4)
            {
                Some(i) => {
                    kb.remove(i);
                }
                None => ck.miss(format!("{:?} {:.3} mm2", s, a / 1e12)),
            }
        }
    }
    ck.extra = kb.len();
    out.categories.push(ck);

    // Outline: the same loops, compared by area and length
    let mut co = Category::new("outline");
    let (lo, lb) = (crate::outline::loops(orig), crate::outline::loops(back));
    co.total = lo.len();
    let measure = |lp: &Vec<crate::outline::Vertex>| {
        let pts: Vec<Pt> = lp.iter().flat_map(|v| v.points()).collect();
        poly_area(&pts)
    };
    let mut areas_b: Vec<f64> = lb.iter().map(measure).collect();
    for l in &lo {
        let a = measure(l);
        match areas_b
            .iter()
            .position(|&b| (a - b).abs() <= a.max(1.0) * 1e-4)
        {
            Some(i) => {
                areas_b.remove(i);
            }
            None => co.miss(format!("loop of {:.3} mm2", a / 1e12)),
        }
    }
    co.extra = areas_b.len();
    out.categories.push(co);

    // Footprint graphics: count per layer
    let mut cg = Category::new("footprint graphics");
    let count = |b: &Board, imported: bool| {
        let mut m: HashMap<String, usize> = HashMap::new();
        for f in &b.footprints {
            for d in &f.drawings {
                // Copper comes back as pads and tracks (see above); Eco and Margin have no
                // Altium kind, so KiCad's importer puts them on a free user layer
                // A pad on paste or mask alone becomes shapes on that layer in KiCad's import;
                // those are compared as aperture pads below
                if imported && aperture_at(orig, &d.layer, &d.shape, &mv) {
                    continue;
                }
                if d.layer != "Edge.Cuts"
                    && !d.layer.ends_with(".Cu")
                    && !UNKINDED.contains(&d.layer.as_str())
                {
                    *m.entry(d.layer.clone()).or_default() += segments(&d.shape, d.filled, d.width);
                }
            }
        }
        m
    };
    let (mo, mb) = (count(orig, false), count(back, true));

    // Aperture pads (paste or mask only): something drawn there on that layer
    let mut cap = Category::new("aperture pads");
    for f in &orig.footprints {
        for p in f.pads.iter().filter(|p| is_aperture(p)) {
            let ls: Vec<String> = p
                .layers
                .iter()
                .flat_map(|l| match l.as_str() {
                    "*.Mask" => vec!["F.Mask".to_string(), "B.Mask".to_string()],
                    "*.Paste" => vec!["F.Paste".to_string(), "B.Paste".to_string()],
                    _ => vec![l.clone()],
                })
                .collect();
            for l in &ls {
                cap.total += 1;
                let c = mv(p.at);
                let hit = back
                    .footprints
                    .iter()
                    .flat_map(|g| g.drawings.iter())
                    .any(|d| &d.layer == l && covers(&d.shape, d.width, c));
                if !hit {
                    cap.miss(format!("{} pad on {l} at {c:?}", f.reference));
                }
            }
        }
    }
    out.categories.push(cap);
    for (l, n) in &mo {
        cg.total += n;
        let k = mb.get(l).copied().unwrap_or(0);
        if k < *n {
            cg.missing += n - k;
            cg.examples.push(format!("{l}: {n} -> {k}"));
        } else if k > *n {
            cg.extra += k - n;
            cg.examples.push(format!("{l}: {n} -> {k}"));
        }
    }
    out.categories.push(cg);

    out
}

/// Graphic pieces an Altium file carries for a KiCad shape: a polygon edge by edge, and a
/// filled one with an outline width as the region plus its edges (Altium regions have no width)
fn segments(s: &Shape, filled: bool, width: Nm) -> usize {
    let edges = match s {
        Shape::Rect { .. } => 4,
        Shape::Poly(p) => (0..p.len())
            .filter(|&i| p[i] != p[(i + 1) % p.len()])
            .count(),
        _ => return 1,
    };
    match (filled, width > 0) {
        (true, true) => 1 + edges,
        (true, false) => 1,
        (false, _) => edges,
    }
}

fn shape_class(p: &crate::kicad::Pad) -> &'static str {
    match p.shape {
        PadShape::Circle => "circle",
        PadShape::Oval if p.size.x == p.size.y => "circle",
        PadShape::Oval => "oval",
        PadShape::Rect => "rect",
        PadShape::RoundRect if p.rratio <= 0.0 => "rect",
        PadShape::RoundRect => "roundrect",
        PadShape::ChamferedRect => "chamfered",
        PadShape::Trapezoid => "trapezoid",
        PadShape::Custom => "custom",
    }
}

fn kind_class(k: PadKind) -> u8 {
    match k {
        PadKind::Smd | PadKind::Connect => 0,
        PadKind::Thru => 1,
        PadKind::Npth => 2,
    }
}

pub fn poly_area(p: &[Pt]) -> f64 {
    let mut a = 0.0;
    for i in 0..p.len() {
        let (u, v) = (p[i], p[(i + 1) % p.len()]);
        a += u.x as f64 * v.y as f64 - v.x as f64 * u.y as f64;
    }
    (a / 2.0).abs()
}

/// Per footprint and layer, how many graphic pieces each board has (for finding differences)
pub fn graphics_by_footprint(orig: &Board, back: &Board) -> Vec<(String, String, usize, usize)> {
    let count = |b: &Board| {
        let mut m: HashMap<(String, String), usize> = HashMap::new();
        for f in &b.footprints {
            for d in &f.drawings {
                if d.layer != "Edge.Cuts" {
                    *m.entry((f.reference.clone(), d.layer.clone())).or_default() +=
                        segments(&d.shape, d.filled, d.width);
                }
            }
        }
        m
    };
    let (a, b) = (count(orig), count(back));
    let mut out: Vec<_> = a
        .iter()
        .filter_map(|(k, &n)| {
            let m = b.get(k).copied().unwrap_or(0);
            (m != n).then(|| (k.0.clone(), k.1.clone(), n, m))
        })
        .collect();
    out.sort();
    out
}

fn bbox(s: &Shape) -> (Pt, Pt) {
    let pts: Vec<Pt> = match s {
        Shape::Line { a, b } | Shape::Rect { a, b } => vec![*a, *b],
        Shape::Arc { start, mid, end } => vec![*start, *mid, *end],
        Shape::Circle { center, radius } => vec![
            Pt::new(center.x - radius, center.y - radius),
            Pt::new(center.x + radius, center.y + radius),
        ],
        Shape::Poly(p) => p.clone(),
    };
    let min = Pt::new(
        pts.iter().map(|p| p.x).min().unwrap_or(0),
        pts.iter().map(|p| p.y).min().unwrap_or(0),
    );
    let max = Pt::new(
        pts.iter().map(|p| p.x).max().unwrap_or(0),
        pts.iter().map(|p| p.y).max().unwrap_or(0),
    );
    (min, max)
}

/// Whether a shape (with its line width) covers a point, by bounding box
fn covers(s: &Shape, width: Nm, p: Pt) -> bool {
    let (a, b) = bbox(s);
    let w = width / 2 + TOL;
    p.x >= a.x - w && p.x <= b.x + w && p.y >= a.y - w && p.y <= b.y + w
}

/// Whether a shape in the imported board sits on an aperture pad of the original
fn aperture_at(orig: &Board, layer: &str, s: &Shape, mv: &dyn Fn(Pt) -> Pt) -> bool {
    let (a, b) = bbox(s);
    orig.footprints.iter().flat_map(|f| f.pads.iter()).any(|p| {
        is_aperture(p)
            && p.layers.iter().any(|l| {
                l == layer
                    || (l == "*.Mask" && layer.ends_with(".Mask"))
                    || (l == "*.Paste" && layer.ends_with(".Paste"))
            })
            && {
                let c = mv(p.at);
                let r = p.size.x.max(p.size.y);
                a.x >= c.x - r && b.x <= c.x + r && a.y >= c.y - r && b.y <= c.y + r
            }
    })
}

/// A pad on paste or mask alone (a hole is a pad even without copper)
fn is_aperture(p: &crate::kicad::Pad) -> bool {
    matches!(p.kind, PadKind::Smd | PadKind::Connect)
        && !p.layers.iter().any(|l| l.ends_with(".Cu"))
}
