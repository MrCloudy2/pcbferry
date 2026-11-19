//! Geometry helpers shared by the reader and the converter.

use crate::kicad::{Nm, Pt};

/// Centre and radius of the circle through three points, None if they are in a line
pub fn circle_through(a: Pt, b: Pt, c: Pt) -> Option<(f64, f64, f64)> {
    let (ax, ay, bx, by, cx, cy) = (
        a.x as f64, a.y as f64, b.x as f64, b.y as f64, c.x as f64, c.y as f64,
    );
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-6 {
        return None;
    }
    let a2 = ax * ax + ay * ay;
    let b2 = bx * bx + by * by;
    let c2 = cx * cx + cy * cy;
    let ux = (a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d;
    let uy = (a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d;
    Some((ux, uy, (ax - ux).hypot(ay - uy)))
}

/// Signed area twice over, positive when a->b->c turns counter-clockwise in a y-up frame
pub fn cross(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Points along an arc from start through mid to end, at most `step` nm apart
pub fn arc_to_points(s: Pt, m: Pt, e: Pt, step: Nm) -> Vec<Pt> {
    let Some((cx, cy, r)) = circle_through(s, m, e) else {
        return vec![s, e];
    };
    let ang = |p: Pt| (p.y as f64 - cy).atan2(p.x as f64 - cx);
    let a0 = ang(s);
    let tau = std::f64::consts::TAU;
    let norm = |x: f64| x.rem_euclid(tau);
    // Sweep in the direction that passes the mid point
    let (dm, de) = (norm(ang(m) - a0), norm(ang(e) - a0));
    let sweep = if dm <= de { de } else { de - tau };
    let n = ((sweep.abs() * r / step as f64).ceil() as usize).clamp(2, 720);
    (0..=n)
        .map(|i| {
            if i == n {
                return e;
            }
            let a = a0 + sweep * i as f64 / n as f64;
            Pt::new(
                (cx + r * a.cos()).round() as Nm,
                (cy + r * a.sin()).round() as Nm,
            )
        })
        .collect()
}

pub fn bezier_to_points(p0: Pt, p1: Pt, p2: Pt, p3: Pt) -> Vec<Pt> {
    let n = 32;
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let f = |a: Nm, b: Nm, c: Nm, d: Nm| {
                (u * u * u * a as f64
                    + 3.0 * u * u * t * b as f64
                    + 3.0 * u * t * t * c as f64
                    + t * t * t * d as f64)
                    .round() as Nm
            };
            Pt::new(f(p0.x, p1.x, p2.x, p3.x), f(p0.y, p1.y, p2.y, p3.y))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_of_three_points() {
        let (x, y, r) =
            circle_through(Pt::new(1000, 0), Pt::new(0, 1000), Pt::new(-1000, 0)).unwrap();
        assert!(x.abs() < 1e-6 && y.abs() < 1e-6 && (r - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn arc_points_pass_the_mid_point_side() {
        // Upper half (negative y) of the unit circle, going through (0, -1000)
        let p = arc_to_points(Pt::new(1000, 0), Pt::new(0, -1000), Pt::new(-1000, 0), 100);
        assert!(p.iter().all(|q| q.y <= 1));
        assert_eq!(*p.last().unwrap(), Pt::new(-1000, 0));
    }
}
