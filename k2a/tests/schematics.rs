//! Round trip for schematics: KiCad schematic -> our SchDocs -> KiCad's own Altium importer,
//! with the two netlists compared: the same components, values and footprints, and the same
//! pins on every net. Needs a kicad-cli that can import Altium (KICAD_CLI) and KiCad's demos
//! (KICAD_DEMOS).

use std::path::{Path, PathBuf};

fn setup() -> Option<(PathBuf, PathBuf)> {
    let cli = std::env::var_os("KICAD_CLI").map(PathBuf::from)?;
    let demos = std::env::var_os("KICAD_DEMOS").map(PathBuf::from)?;
    Some((cli, demos))
}

/// Every demo project's root sheet
fn roots(demos: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![demos.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "kicad_pro") {
                let sch = p.with_extension("kicad_sch");
                if sch.exists() {
                    out.push(sch);
                }
            }
        }
    }
    out.sort();
    out
}

fn roundtrip(cli: &Path, root: &Path) -> Result<(), String> {
    let name = root.file_stem().unwrap().to_string_lossy().to_string();
    let dir = std::env::temp_dir().join(format!(
        "k2a-sch-{}-{}",
        std::process::id(),
        name.replace(' ', "_")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let doc = dir.join(format!("{name}.SchDoc"));
    let report =
        pcbferry_k2a::sch::convert_file(root, &doc).map_err(|e| format!("convert: {e}"))?;
    let d =
        pcbferry_k2a::sch::check::roundtrip(cli, root, &doc).map_err(|e| format!("check: {e}"))?;
    let _ = std::fs::remove_dir_all(&dir);
    println!(
        "{name}: {} sheets, {} components, {} nets: {}",
        report.sheets.len(),
        d.components,
        d.nets,
        if d.ok() { "same" } else { "DIFFERENT" }
    );
    if d.ok() {
        Ok(())
    } else {
        Err(format!(
            "{name}:\n  {}",
            d.problems
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        ))
    }
}

#[test]
fn single_sheet() {
    let Some((cli, demos)) = setup() else {
        eprintln!("skipped: set KICAD_CLI and KICAD_DEMOS");
        return;
    };
    roundtrip(&cli, &demos.join("interf_u/interf_u.kicad_sch")).unwrap();
}

#[test]
fn hierarchy_with_a_sheet_used_twice() {
    let Some((cli, demos)) = setup() else {
        eprintln!("skipped: set KICAD_CLI and KICAD_DEMOS");
        return;
    };
    roundtrip(
        &cli,
        &demos.join("complex_hierarchy/complex_hierarchy.kicad_sch"),
    )
    .unwrap();
}

#[test]
fn all_demos() {
    let Some((cli, demos)) = setup() else {
        eprintln!("skipped: set KICAD_CLI and KICAD_DEMOS");
        return;
    };
    let roots = roots(&demos);
    assert!(
        !roots.is_empty(),
        "no demo projects under {}",
        demos.display()
    );
    let failures: Vec<String> = roots
        .iter()
        .filter_map(|r| roundtrip(&cli, r).err())
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        roots.len(),
        failures.join("\n")
    );
}
