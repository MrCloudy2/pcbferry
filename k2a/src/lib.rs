//! pcbferry's reverse direction: a KiCad board (.kicad_pcb) as an Altium PcbDoc.

pub mod altium;
pub mod compare;
pub mod convert;
pub mod geom;
pub mod kicad;
pub mod outline;
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
    let (doc, report) = convert::convert(&board);
    doc.write(output)?;
    Ok(report)
}
