//! pcbferry's reverse direction: a KiCad board (.kicad_pcb) as an Altium PcbDoc.

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
