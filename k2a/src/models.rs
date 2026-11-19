//! 3D models: each STEP file embedded once in the PcbDoc (Models storage, zlib), and a component
//! body per footprint model saying where it sits (ComponentBodies6).

use crate::altium::{mil, Buf, Doc, Props};
use crate::convert::Report;
use crate::kicad::{rotate, Board, Footprint, Model, Nm, Pt};
use base64::Engine;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Where KiCad installs its 3D models, when no ${KICADn_3DMODEL_DIR} says otherwise
fn default_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    for n in (6..=10).rev() {
        if let Some(d) = std::env::var_os(format!("KICAD{n}_3DMODEL_DIR")) {
            v.push(d.into());
        }
    }
    if let Some(d) = std::env::var_os("KISYS3DMOD") {
        v.push(d.into());
    }
    for d in [
        "/usr/share/kicad/3dmodels",
        "/usr/local/share/kicad/3dmodels",
        "/Applications/KiCad/KiCad.app/Contents/SharedSupport/3dmodels",
    ] {
        v.push(d.into());
    }
    if let Some(home) = std::env::var_os("HOME") {
        for n in (6..=10).rev() {
            v.push(Path::new(&home).join(format!(".local/share/kicad/{n}.0/3dmodels")));
        }
    }
    for n in (6..=10).rev() {
        v.push(format!("C:\\Program Files\\KiCad\\{n}.0\\share\\kicad\\3dmodels").into());
    }
    v
}

/// The candidates for a model path: ${VARIABLES} expanded, and STEP in place of VRML
fn candidates(path: &str, board_dir: &Path) -> Vec<PathBuf> {
    let mut bases: Vec<String> = Vec::new();
    if let Some(rest) = path.strip_prefix("${").and_then(|r| r.split_once('}')) {
        let (var, tail) = rest;
        let tail = tail.trim_start_matches(['/', '\\']);
        if var == "KIPRJMOD" {
            bases.push(board_dir.join(tail).to_string_lossy().into_owned());
        } else {
            if let Some(v) = std::env::var_os(var) {
                bases.push(Path::new(&v).join(tail).to_string_lossy().into_owned());
            }
            for d in default_dirs() {
                bases.push(d.join(tail).to_string_lossy().into_owned());
            }
        }
    } else if Path::new(path).is_absolute() {
        bases.push(path.to_string());
    } else {
        bases.push(board_dir.join(path).to_string_lossy().into_owned());
    }
    let mut out = Vec::new();
    for b in bases {
        let p = PathBuf::from(&b);
        // Altium takes STEP only: a .wrl's STEP twin, which KiCad's libraries ship
        for ext in ["step", "stp", "STEP", "STP"] {
            out.push(p.with_extension(ext));
        }
        out.push(p);
    }
    out
}

/// A model's STEP data, if it can be found
fn load(
    m: &Model,
    fp: &Footprint,
    b: &Board,
    board_dir: &Path,
) -> Result<(String, Vec<u8>), String> {
    if let Some(name) = m.path.strip_prefix("kicad-embed://") {
        let data = fp
            .embedded
            .get(name)
            .or_else(|| b.embedded.get(name))
            .ok_or(format!("embedded {name} has no data"))?;
        let z = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| format!("{name}: {e}"))?;
        let mut out = Vec::new();
        ruzstd::decoding::StreamingDecoder::new(z.as_slice())
            .map_err(|e| format!("{name}: {e}"))?
            .read_to_end(&mut out)
            .map_err(|e| format!("{name}: {e}"))?;
        return Ok((name.to_string(), out));
    }
    for c in candidates(&m.path, board_dir) {
        let is_step = matches!(
            c.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .as_deref(),
            Some("step" | "stp")
        );
        if is_step && c.is_file() {
            let data = std::fs::read(&c).map_err(|e| format!("{}: {e}", c.display()))?;
            let name = c.file_name().unwrap().to_string_lossy().into_owned();
            return Ok((name, data));
        }
    }
    Err(format!("no STEP file found for {}", m.path))
}

/// Altium's GUID form: {XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}, derived from the content
fn guid(data: &[u8]) -> String {
    let mut h: [u64; 2] = [0xcbf2_9ce4_8422_2325, 0x8422_2325_cbf2_9ce4];
    for (i, &b) in data.iter().enumerate() {
        let k = i & 1;
        h[k] ^= b as u64;
        h[k] = h[k].wrapping_mul(0x0100_0000_01b3);
    }
    let x = format!("{:016X}{:016X}", h[0], h[1]);
    format!(
        "{{{}-{}-{}-{}-{}}}",
        &x[0..8],
        &x[8..12],
        &x[12..16],
        &x[16..20],
        &x[20..32]
    )
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = flate2::Crc::new();
    c.update(data);
    c.sum()
}

/// The footprint's extent for the body's outline: its courtyard, else its pads
fn outline(fp: &Footprint) -> Vec<Pt> {
    let mut pts: Vec<Pt> = fp
        .drawings
        .iter()
        .filter(|d| d.layer.ends_with("CrtYd"))
        .flat_map(|d| match &d.shape {
            crate::kicad::Shape::Line { a, b } | crate::kicad::Shape::Rect { a, b } => vec![*a, *b],
            crate::kicad::Shape::Poly(p) => p.clone(),
            crate::kicad::Shape::Arc { start, mid, end } => vec![*start, *mid, *end],
            crate::kicad::Shape::Circle { center, radius } => {
                vec![
                    Pt::new(center.x - radius, center.y - radius),
                    Pt::new(center.x + radius, center.y + radius),
                ]
            }
        })
        .collect();
    if pts.is_empty() {
        for p in &fp.pads {
            let r = p.size.x.max(p.size.y) / 2;
            pts.push(Pt::new(p.at.x - r, p.at.y - r));
            pts.push(Pt::new(p.at.x + r, p.at.y + r));
        }
    }
    if pts.is_empty() {
        let a = fp.place.at;
        pts = vec![
            Pt::new(a.x - 500_000, a.y - 500_000),
            Pt::new(a.x + 500_000, a.y + 500_000),
        ];
    }
    let (x0, x1) = (
        pts.iter().map(|p| p.x).min().unwrap(),
        pts.iter().map(|p| p.x).max().unwrap(),
    );
    let (y0, y1) = (
        pts.iter().map(|p| p.y).min().unwrap(),
        pts.iter().map(|p| p.y).max().unwrap(),
    );
    vec![
        Pt::new(x0, y0),
        Pt::new(x1, y0),
        Pt::new(x1, y1),
        Pt::new(x0, y1),
    ]
}

/// Mechanical layers the bodies go on, with Altium's 3D body kinds
pub const BODY_TOP: u16 = 57;
pub const BODY_BOTTOM: u16 = 58;

pub fn write(doc: &mut Doc, b: &Board, board_dir: &Path, report: &mut Report) {
    // Each STEP file once, by content
    let mut index: HashMap<u32, (String, String)> = HashMap::new();
    let mut missing = Vec::new();
    for (ci, fp) in b.footprints.iter().enumerate() {
        for m in fp.models.iter().filter(|m| !m.hide) {
            let (name, data) = match load(m, fp, b, board_dir) {
                Ok(x) => x,
                Err(e) => {
                    if !missing.contains(&e) {
                        missing.push(e);
                    }
                    continue;
                }
            };
            if m.scale.iter().any(|s| (s - 1.0).abs() > 1e-9) {
                report.warnings.push(format!(
                    "{}: 3D model scaled in KiCad; Altium has no scale, written at 1:1",
                    fp.reference
                ));
            }
            let crc = crc32(&data);
            let (id, name) = match index.get(&crc) {
                Some(x) => x.clone(),
                None => {
                    let id = guid(&data);
                    let n = doc.storage("Models").count;
                    let mut z =
                        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    z.write_all(&data).unwrap();
                    doc.streams
                        .insert(format!("Models/{n}"), z.finish().unwrap());
                    let mut p = Props::new();
                    p.bool("EMBED", true)
                        .set("MODELSOURCE", "Undefined")
                        .set("ID", id.as_str());
                    p.set("ROTX", "0.000")
                        .set("ROTY", "0.000")
                        .set("ROTZ", "0.000")
                        .set("DZ", "0");
                    p.num("CHECKSUM", crc as i32).set("NAME", name.as_str());
                    doc.storage("Models").push(&p.encode());
                    index.insert(crc, (id.clone(), name.clone()));
                    (id, name)
                }
            };
            body(doc, fp, ci as u16, m, &id, &name, crc, b.thickness);
        }
    }
    match missing.len() {
        0 => {}
        1 => report
            .warnings
            .push(format!("3D model left out: {}", missing[0])),
        n => report.warnings.push(format!(
            "{n} 3D models left out: no STEP file found for them (is KiCad's 3D library installed? \
             KICADn_3DMODEL_DIR points to it); first: {}",
            missing[0]
        )),
    }
}

/// The component body: the inverse of how KiCad's importer places an Altium body
#[allow(clippy::too_many_arguments)]
fn body(
    doc: &mut Doc,
    fp: &Footprint,
    comp: u16,
    m: &Model,
    id: &str,
    name: &str,
    crc: u32,
    thickness: Nm,
) {
    let theta = fp.place.rot;
    let mm = |v: f64| (v * 1e6).round() as Nm;
    // KiCad's offset is in the footprint's frame, y up; the importer turns it by the footprint's
    // orientation (negated for the back) after flipping y
    let (ox, oy) = (mm(m.offset[0]), mm(m.offset[1]));
    let (pos, rz) = if fp.back {
        let d = rotate(Pt::new(ox, oy), theta);
        (
            Pt::new(fp.place.at.x + d.x, fp.place.at.y + d.y),
            -theta - m.rotate[2],
        )
    } else {
        let d = rotate(Pt::new(ox, oy), -theta);
        (
            Pt::new(fp.place.at.x + d.x, fp.place.at.y - d.y),
            theta - m.rotate[2],
        )
    };
    let norm = |a: f64| {
        let a = a.rem_euclid(360.0);
        format!(
            "{:.3}",
            if a.abs() < 5e-4 || (360.0 - a) < 5e-4 {
                0.0
            } else {
                a
            }
        )
    };
    let layer = if fp.back { BODY_BOTTOM } else { BODY_TOP };
    let mut p = Props::new();
    p.set("V7_LAYER", format!("MECHANICAL{layer}"))
        .set("NAME", " ")
        .num("KIND", 0)
        .num("SUBPOLYINDEX", -1);
    p.num("UNIONINDEX", 0)
        .set("ARCRESOLUTION", "0.5mil")
        .bool("ISSHAPEBASED", false);
    p.set("CAVITYHEIGHT", "0mil")
        .set("STANDOFFHEIGHT", mil(0))
        .set("OVERALLHEIGHT", mil(thickness.max(1_000_000)));
    p.num("BODYPROJECTION", fp.back as u8)
        .set("BODYCOLOR3D", "8421504");
    p.set("BODYOPACITY3D", format!("{:.3}", m.opacity));
    p.set("MODELID", id)
        .num("MODEL.CHECKSUM", crc)
        .bool("MODEL.EMBED", true)
        .set("MODEL.NAME", name);
    p.set("MODEL.2D.X", mil(pos.x))
        .set("MODEL.2D.Y", mil(crate::convert::Y_FLIP - pos.y));
    p.set("MODEL.2D.ROTATION", "0.000");
    p.set("MODEL.3D.ROTX", norm(-m.rotate[0]))
        .set("MODEL.3D.ROTY", norm(-m.rotate[1]))
        .set("MODEL.3D.ROTZ", norm(rz));
    p.set("MODEL.3D.DZ", mil(mm(m.offset[2])))
        .num("MODEL.MODELTYPE", 1)
        .set("MODEL.MODELSOURCE", "Undefined");

    let l = crate::convert::mech_layer(layer);
    let mut s = Buf::default();
    s.u8(l.v6).u8(0x0c).u8(0).u16(0xffff).u16(0xffff).u16(comp);
    s.bytes(&[0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0]);
    s.bytes(&p.encode());
    let o = outline(fp);
    s.u32(o.len() as u32);
    for q in &o {
        let (x, y) = crate::convert::coords(*q);
        s.f64(x as f64).f64(y as f64);
    }
    let mut r = Buf::default();
    r.u8(12).block(&s.0);
    doc.storage("ComponentBodies6").push(&r.0);
}
