//! pcbferry's conversions, for the command line and the app: Altium -> KiCad through the
//! patched kicad-cli (the engine), KiCad -> Altium through kicad2altium.

pub mod a2k;
pub mod altium;
pub mod engine;
pub mod k2a;
pub mod rules;
pub mod run;
