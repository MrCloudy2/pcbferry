//! pcbferry's reverse direction: a KiCad board (.kicad_pcb) as an Altium PcbDoc, and a
//! KiCad schematic (.kicad_sch) as an Altium SchDoc.

pub mod altium;
pub mod compare;
pub mod convert;
pub mod font;
pub mod geom;
pub mod kicad;
pub mod models;
pub mod outline;
pub mod project;
pub mod rules;
pub mod sch;
pub mod sexpr;
mod templates;
pub mod zones;

use std::path::Path;

/// Convert a .kicad_pcb file into a .PcbDoc file
pub fn convert_file(
    input: &Path,
    output: &Path,
) -> Result<convert::Report, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(input)?;
    let board = kicad::read(&text)?;
    // Net classes and design rules live in the project file next to the board
    let pro = input.with_extension("kicad_pro");
    let project = match std::fs::read_to_string(&pro) {
        Ok(t) => Some(project::parse(&t).map_err(|e| format!("{}: {e}", pro.display()))?),
        Err(_) => None,
    };
    let dir = input.parent().unwrap_or(Path::new("."));
    let (doc, report) = convert::convert(&board, project.as_ref(), dir);
    doc.write(output)?;
    Ok(report)
}

/// What converting a whole KiCad project wrote
pub struct ProjectReport {
    pub board: Option<(std::path::PathBuf, convert::Report)>,
    pub schematic: Option<sch::Report>,
    pub project: std::path::PathBuf,
}

/// Convert a KiCad project (.kicad_pro): its board and its schematic, into `out_dir`, with an
/// Altium project (.PrjPcb) holding them all
pub fn convert_project(
    pro: &Path,
    out_dir: &Path,
) -> Result<ProjectReport, Box<dyn std::error::Error>> {
    let name = pro
        .file_stem()
        .ok_or("no project name")?
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(out_dir)?;
    let pcb = pro.with_extension("kicad_pcb");
    let board = if pcb.exists() {
        let out = out_dir.join(format!("{name}.PcbDoc"));
        let r = convert_file(&pcb, &out).map_err(|e| format!("{}: {e}", pcb.display()))?;
        Some((out, r))
    } else {
        None
    };
    let root = pro.with_extension("kicad_sch");
    let schematic = if root.exists() {
        let out = out_dir.join(format!("{name}.SchDoc"));
        Some(sch::convert_file(&root, &out).map_err(|e| format!("{}: {e}", root.display()))?)
    } else {
        None
    };
    let mut docs: Vec<&Path> = Vec::new();
    if let Some(s) = &schematic {
        docs.extend(s.sheets.iter().map(|(p, _)| p.as_path()));
    }
    if let Some((p, _)) = &board {
        docs.push(p);
    }
    if docs.is_empty() {
        return Err(format!("{}: no board or schematic next to it", pro.display()).into());
    }
    let project = out_dir.join(format!("{name}.PrjPcb"));
    sch::write_project(&project, &docs)?;
    Ok(ProjectReport {
        board,
        schematic,
        project,
    })
}
