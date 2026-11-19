//! The board outline: Edge.Cuts pieces joined into closed loops.

use crate::kicad::{Board, Nm, Pt, Shape};

#[derive(Debug, Clone, PartialEq)]
pub enum Vertex {
    /// A straight edge starting here
    Line(Pt),
    /// An arc edge
    Arc { start: Pt, mid: Pt, end: Pt },
}

impl Vertex {
    pub fn start(&self) -> Pt {
        match self {
            Vertex::Line(p) => *p,
            Vertex::Arc { start, .. } => *start,
        }
    }

    /// Points along the edge, its end left to the next one
    pub fn points(&self) -> Vec<Pt> {
        match self {
            Vertex::Line(p) => vec![*p],
            Vertex::Arc { start, mid, end } => {
                let mut v = crate::geom::arc_to_points(*start, *mid, *end, 100_000);
                v.pop();
                v
            }
        }
    }
}

/// Endpoints closer than this are the same point
const JOIN: Nm = 10_000;

#[derive(Clone)]
struct Piece {
    a: Pt,
    b: Pt,
    mid: Option<Pt>,
}

fn pieces(b: &Board) -> (Vec<Piece>, Vec<Vec<Vertex>>) {
    let mut open = Vec::new();
    let mut closed = Vec::new();
    let drawings = b
        .drawings
        .iter()
        .chain(b.footprints.iter().flat_map(|f| f.drawings.iter()));
    for d in drawings.filter(|d| d.layer == "Edge.Cuts") {
        match &d.shape {
            Shape::Line { a, b } => open.push(Piece {
                a: *a,
                b: *b,
                mid: None,
            }),
            Shape::Arc { start, mid, end } => open.push(Piece {
                a: *start,
                b: *end,
                mid: Some(*mid),
            }),
            Shape::Circle { center, radius } => {
                let (c, r) = (*center, *radius);
                let p = |x: Nm, y: Nm| Pt::new(c.x + x, c.y + y);
                closed.push(vec![
                    Vertex::Arc {
                        start: p(r, 0),
                        mid: p(0, -r),
                        end: p(-r, 0),
                    },
                    Vertex::Arc {
                        start: p(-r, 0),
                        mid: p(0, r),
                        end: p(r, 0),
                    },
                ]);
            }
            Shape::Rect { a, b } => closed.push(vec![
                Vertex::Line(*a),
                Vertex::Line(Pt::new(b.x, a.y)),
                Vertex::Line(*b),
                Vertex::Line(Pt::new(a.x, b.y)),
            ]),
            Shape::Poly(pts) if pts.len() >= 3 => {
                closed.push(pts.iter().map(|p| Vertex::Line(*p)).collect())
            }
            Shape::Poly(_) => {}
        }
    }
    (open, closed)
}

fn near(p: Pt, q: Pt) -> bool {
    (p.x - q.x).abs() <= JOIN && (p.y - q.y).abs() <= JOIN
}

fn chain(open: Vec<Piece>) -> (Vec<Vec<Vertex>>, usize) {
    // Zero-length pieces (an importer's joints between arcs and lines) join nothing
    let mut open: Vec<Piece> = open
        .into_iter()
        .filter(|p| p.mid.is_some() || !near(p.a, p.b))
        .collect();
    let mut loops = Vec::new();
    while let Some(first) = open.pop() {
        let start = first.a;
        let mut cur = first.b;
        let mut lp = vec![first];
        let mut ok = false;
        loop {
            if near(cur, start) {
                ok = true;
                break;
            }
            let Some(i) = open.iter().position(|p| near(p.a, cur) || near(p.b, cur)) else {
                break;
            };
            let mut p = open.swap_remove(i);
            if !near(p.a, cur) {
                std::mem::swap(&mut p.a, &mut p.b);
            }
            cur = p.b;
            lp.push(p);
        }
        if ok {
            loops.push(
                lp.into_iter()
                    .map(|p| match p.mid {
                        None => Vertex::Line(p.a),
                        Some(m) => Vertex::Arc {
                            start: p.a,
                            mid: m,
                            end: p.b,
                        },
                    })
                    .collect(),
            );
        } else {
            // Put back all but the first piece so a later start can still use them
            let n = lp.len();
            open.extend(lp.into_iter().skip(1));
            if n == 1 {
                continue;
            }
            // Mark the first piece as loose by counting it in a sentinel loop of length 0
            loops.push(Vec::new());
        }
    }
    let loose = loops.iter().filter(|l| l.is_empty()).count();
    (loops.into_iter().filter(|l| !l.is_empty()).collect(), loose)
}

fn area(lp: &[Vertex]) -> f64 {
    let pts: Vec<Pt> = lp.iter().flat_map(|v| v.points()).collect();
    let mut a = 0.0;
    for i in 0..pts.len() {
        let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
        a += p.x as f64 * q.y as f64 - q.x as f64 * p.y as f64;
    }
    (a / 2.0).abs()
}

/// Closed loops on Edge.Cuts, largest first (the outline), the rest cut-outs
pub fn loops(b: &Board) -> Vec<Vec<Vertex>> {
    let (open, mut closed) = pieces(b);
    let (mut l, _) = chain(open);
    closed.append(&mut l);
    closed.sort_by(|x, y| {
        area(y)
            .partial_cmp(&area(x))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    closed
}

/// How many Edge.Cuts pieces belong to no closed loop
pub fn loose_edges(b: &Board) -> usize {
    let (open, _) = pieces(b);
    let n = open
        .iter()
        .filter(|p| p.mid.is_some() || !near(p.a, p.b))
        .count();
    let (l, _) = chain(open);
    let used: usize = l.iter().map(|x| x.len()).sum();
    n - used
}
