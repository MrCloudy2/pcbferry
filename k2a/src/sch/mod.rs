//! KiCad schematics as Altium SchDocs.

pub mod altium;
pub mod check;
pub mod harness;
pub mod kicad;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct Report {
    /// Each SchDoc written, with what went into it
    pub sheets: Vec<(PathBuf, altium::Report)>,
    pub project: Option<PathBuf>,
}

/// Convert a .kicad_sch file and every sheet below it. Each sheet instance becomes a SchDoc in
/// the output's folder (a sheet used twice is written twice, each with its own references), and
/// a hierarchy gets a .PrjPcb listing them.
pub fn convert_file(input: &Path, output: &Path) -> Result<Report, Box<dyn std::error::Error>> {
    let dir = input.parent().unwrap_or(Path::new("."));
    let out_dir = output.parent().unwrap_or(Path::new("."));
    let root = kicad::read(&std::fs::read_to_string(input)?)?;
    let root_name = output
        .file_name()
        .ok_or("no output file name")?
        .to_string_lossy()
        .to_string();
    // Bus aliases hold for the whole design: gather them from every sheet first
    let mut aliases = HashMap::new();
    let mut files: Vec<PathBuf> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<String> = root.sheets.iter().map(|s| s.file.clone()).collect();
    for (n, m) in &root.aliases {
        aliases.insert(n.clone(), m.clone());
    }
    while let Some(f) = stack.pop() {
        let f = dir.join(f);
        if !seen.insert(f.clone()) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&f) {
            let s = kicad::read(&text).map_err(|e| format!("{}: {e}", f.display()))?;
            for (n, m) in &s.aliases {
                aliases.insert(n.clone(), m.clone());
            }
            stack.extend(s.sheets.iter().map(|s| s.file.clone()));
            files.push(f);
        }
    }
    let mut report = Report::default();
    let mut used: HashMap<String, usize> = HashMap::new();
    used.insert(root_name.to_lowercase(), 1);
    let mut cache: HashMap<PathBuf, kicad::Schematic> = HashMap::new();
    // (schematic file, instance path, output name), depth first
    let mut todo: Vec<(Option<PathBuf>, String, String)> =
        vec![(None, format!("/{}", root.uuid), root_name)];
    while let Some((file, path, name)) = todo.pop() {
        let sch = match &file {
            None => &root,
            Some(f) => {
                if !cache.contains_key(f) {
                    let text =
                        std::fs::read_to_string(f).map_err(|e| format!("{}: {e}", f.display()))?;
                    cache.insert(f.clone(), kicad::read(&text)?);
                }
                &cache[f]
            }
        };
        let mut names = Vec::new();
        let mut children = Vec::new();
        for sheet in &sch.sheets {
            let stem = Path::new(&sheet.file)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "sheet".into());
            let n = used
                .entry(format!("{stem}.schdoc").to_lowercase())
                .or_insert(0);
            *n += 1;
            let child = if *n == 1 {
                format!("{stem}.SchDoc")
            } else {
                format!("{stem}_{n}.SchDoc")
            };
            names.push(child.clone());
            children.push((
                Some(dir.join(&sheet.file)),
                format!("{path}/{}", sheet.uuid),
                child,
            ));
        }
        let (doc, r) = altium::convert(sch, &path, &names, !root.sheets.is_empty(), &aliases);
        let target = out_dir.join(&name);
        doc.write(&target)?;
        report.sheets.push((target, r));
        todo.extend(children.into_iter().rev());
    }
    // The harness types the sheets use, as Altium's harness definition file: TYPE=A,B,C
    let mut types = std::collections::BTreeMap::new();
    for (_, r) in &report.sheets {
        for (t, m) in &r.harness_types {
            types.entry(t.clone()).or_insert_with(|| m.clone());
        }
    }
    let harness = output.with_extension("Harness");
    if !types.is_empty() {
        let text: String = types
            .iter()
            .map(|(t, m)| format!("{t}={}\r\n", m.join(",")))
            .collect();
        std::fs::write(&harness, text)?;
    }
    if report.sheets.len() > 1 {
        let prj = output.with_extension("PrjPcb");
        let mut docs: Vec<&Path> = report.sheets.iter().map(|(p, _)| p.as_path()).collect();
        if !types.is_empty() {
            docs.push(&harness);
        }
        write_project(&prj, &docs)?;
        report.project = Some(prj);
    }
    Ok(report)
}

/// An Altium project listing these documents (paths relative to the project's folder)
pub fn write_project(prj: &Path, docs: &[&Path]) -> std::io::Result<()> {
    // HierarchyMode 2, Hierarchical: ports meet sheet entries, net labels stay on their sheet
    // and power ports are global, as KiCad's labels behave. Left automatic, Altium makes the
    // net labels of a sheet used twice one net across both copies
    let mut text = String::from("[Design]\r\nVersion=1.0\r\nHierarchyMode=2\r\n");
    for (i, p) in docs.iter().enumerate() {
        let n = p.file_name().unwrap_or_default().to_string_lossy();
        text += &format!("\r\n[Document{}]\r\nDocumentPath={n}\r\n", i + 1);
    }
    std::fs::write(prj, text)
}
