//! Round trip: KiCad board -> our PcbDoc -> KiCad's own Altium importer -> compared with the
//! original. Needs a kicad-cli that can import Altium (pcbferry's engine or KiCad 10): set
//! KICAD_CLI. The boards come from KiCad's demos: set KICAD_DEMOS to that folder.

use pcbferry_k2a::{compare, kicad};
use std::path::{Path, PathBuf};
use std::process::Command;

fn kicad_cli() -> Option<PathBuf> {
    std::env::var_os("KICAD_CLI").map(PathBuf::from)
}

fn demo(rel: &str) -> Option<PathBuf> {
    let p = Path::new(&std::env::var_os("KICAD_DEMOS")?).join(rel);
    p.exists().then_some(p)
}

fn roundtrip(rel: &str) -> Option<compare::Comparison> {
    let (Some(cli), Some(src)) = (kicad_cli(), demo(rel)) else {
        eprintln!("skipped {rel}: set KICAD_CLI and KICAD_DEMOS");
        return None;
    };
    let dir = std::env::temp_dir().join(format!(
        "k2a-{}-{}",
        std::process::id(),
        rel.replace(['/', ' '], "_")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let doc = dir.join("board.PcbDoc");
    let report = pcbferry_k2a::convert_file(&src, &doc).expect("convert");
    let back = dir.join("back.kicad_pcb");
    let out = Command::new(&cli)
        .args(["pcb", "import", "--format", "altium"])
        .arg(&doc)
        .arg("-o")
        .arg(&back)
        .current_dir(&dir)
        .output()
        .expect("kicad-cli");
    assert!(
        out.status.success() && back.exists(),
        "import failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let orig = kicad::read(&std::fs::read_to_string(&src).unwrap()).unwrap();
    let got = kicad::read(&std::fs::read_to_string(&back).unwrap()).unwrap();
    let c = compare::compare(&orig, &got);
    println!("{rel}: {}", c.summary());
    for w in &report.warnings {
        println!("  note: {w}");
    }
    for cat in &c.categories {
        for e in &cat.examples {
            println!("  {}: {e}", cat.name);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    Some(c)
}

/// Copper must come back exactly: every pad, track, arc, via and zone fill
fn assert_copper(c: &compare::Comparison) {
    for name in [
        "nets",
        "footprints",
        "pads",
        "aperture pads",
        "tracks",
        "arcs",
        "vias",
        "zone fills",
        "keepouts",
        "outline",
        "texts",
        "3d models",
    ] {
        let cat = c.get(name);
        assert!(
            cat.ok(),
            "{name}: {} missing, {} extra: {:?}",
            cat.missing,
            cat.extra,
            cat.examples
        );
    }
}

#[test]
fn pic_programmer() {
    if let Some(c) = roundtrip("pic_programmer/pic_programmer.kicad_pcb") {
        assert_copper(&c);
    }
}

#[test]
fn video() {
    if let Some(c) = roundtrip("video/video.kicad_pcb") {
        assert_copper(&c);
    }
}

#[test]
fn stickhub() {
    if let Some(c) = roundtrip("stickhub/StickHub.kicad_pcb") {
        assert_copper(&c);
    }
}

#[test]
fn ecc83() {
    if let Some(c) = roundtrip("ecc83/ecc83-pp_v2.kicad_pcb") {
        assert_copper(&c);
    }
}

/// Every demo KiCad ships: copper must come back on all of them
#[test]
fn all_demos() {
    let Some(root) = std::env::var_os("KICAD_DEMOS").map(PathBuf::from) else {
        eprintln!("skipped: set KICAD_DEMOS");
        return;
    };
    if kicad_cli().is_none() {
        return;
    }
    let mut boards = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()) == Some("kicad_pcb") {
                boards.push(
                    p.strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    boards.sort();
    let mut failed = Vec::new();
    for b in &boards {
        let c = roundtrip(b).unwrap();
        let bad: Vec<&str> = [
            "nets",
            "footprints",
            "pads",
            "aperture pads",
            "tracks",
            "arcs",
            "vias",
            "zone fills",
            "keepouts",
            "outline",
            "texts",
            "3d models",
        ]
        .into_iter()
        .filter(|n| !c.get(n).ok())
        .collect();
        if !bad.is_empty() {
            failed.push(format!("{b}: {}", bad.join(", ")));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} boards differ:\n{}",
        failed.len(),
        boards.len(),
        failed.join("\n")
    );
}
