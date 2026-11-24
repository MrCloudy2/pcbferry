//! KiCad (.kicad_pro, .kicad_pcb, .kicad_sch) -> Altium, with kicad2altium. With `check`,
//! the result is read back with KiCad's own Altium importer (the engine) and compared with the
//! original.

use crate::engine::Engine;
use crate::run::Log;
use pcbferry_k2a as k2a;
use std::path::{Path, PathBuf};

pub struct Outcome {
    pub ok: bool,
    pub out: PathBuf,
    /// The .PrjPcb, when one was written
    pub project: Option<PathBuf>,
    /// Every file written
    pub files: Vec<PathBuf>,
    pub notes: Vec<String>,
}

fn ext(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The default output folder: NAME-altium next to the input
pub fn default_out(input: &Path) -> PathBuf {
    input.with_file_name(format!("{}-altium", stem(input)))
}

/// (the KiCad original, what it became)
type Pair = (PathBuf, PathBuf);

pub fn convert(
    input: &Path,
    out: &Path,
    check: Option<&Engine>,
    log: Log,
) -> Result<Outcome, String> {
    let input = crate::altium::resolve(input).map_err(|e| e.to_string())?;
    if !input.is_file() {
        return Err(format!("{}: no such file", input.display()));
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let name = stem(&input);
    let mut o = Outcome {
        ok: true,
        out: out.to_path_buf(),
        project: None,
        files: Vec::new(),
        notes: Vec::new(),
    };
    let (board, sheet): (Option<Pair>, Option<Pair>);
    match ext(&input).as_str() {
        "kicad_pro" => {
            log(format!(
                "pcbferry: converting the project {}",
                input.display()
            ));
            let r = k2a::convert_project(&input, out).map_err(|e| e.to_string())?;
            if let Some((p, b)) = &r.board {
                board_lines(p, b, log, &mut o);
            }
            if let Some(s) = &r.schematic {
                sheet_lines(s, log, &mut o);
            }
            o.project = Some(r.project.clone());
            o.files.push(r.project.clone());
            log(format!("pcbferry: project: {}", r.project.display()));
            board = r
                .board
                .as_ref()
                .map(|(p, _)| (input.with_extension("kicad_pcb"), p.clone()));
            sheet = r.schematic.as_ref().map(|_| {
                (
                    input.with_extension("kicad_sch"),
                    out.join(format!("{name}.SchDoc")),
                )
            });
        }
        "kicad_pcb" => {
            let to = out.join(format!("{name}.PcbDoc"));
            let r =
                k2a::convert_file(&input, &to).map_err(|e| format!("{}: {e}", input.display()))?;
            board_lines(&to, &r, log, &mut o);
            board = Some((input.clone(), to));
            sheet = None;
        }
        "kicad_sch" => {
            let to = out.join(format!("{name}.SchDoc"));
            let r = k2a::sch::convert_file(&input, &to)
                .map_err(|e| format!("{}: {e}", input.display()))?;
            sheet_lines(&r, log, &mut o);
            if let Some(p) = &r.project {
                o.project = Some(p.clone());
                o.files.push(p.clone());
            }
            board = None;
            sheet = Some((input.clone(), to));
        }
        _ => {
            return Err(format!(
                "{}: expected a .kicad_pro, .kicad_pcb or .kicad_sch",
                input.display()
            ))
        }
    }
    if let Some(engine) = check {
        if let Some((orig, doc)) = &board {
            match check_board(engine, orig, doc) {
                Ok((ok, lines)) => {
                    o.ok &= ok;
                    for l in &lines {
                        log(format!("pcbferry: {l}"));
                    }
                    if !ok {
                        o.notes.push(format!(
                            "The board read back differs: {}",
                            lines.first().cloned().unwrap_or_default()
                        ));
                    }
                }
                Err(e) => {
                    o.ok = false;
                    log(format!("pcbferry: error: board check: {e}"));
                    o.notes.push(format!("The board check failed: {e}"));
                }
            }
        }
        if let Some((orig, doc)) = &sheet {
            match k2a::sch::check::roundtrip(&engine.cli, orig, doc) {
                Ok(d) => {
                    let ok = d.ok();
                    o.ok &= ok;
                    log(format!(
                        "pcbferry: schematic check: {} components, {} nets: {}",
                        d.components,
                        d.nets,
                        if ok {
                            "the same as in KiCad"
                        } else {
                            "DIFFERENT"
                        }
                    ));
                    for p in d.problems.iter().take(20) {
                        log(format!("  {p}"));
                    }
                    if !ok {
                        o.notes.push(format!(
                            "The schematic read back differs: {} problem(s)",
                            d.problems.len()
                        ));
                    }
                }
                Err(e) => {
                    o.ok = false;
                    log(format!("pcbferry: error: schematic check: {e}"));
                    o.notes.push(format!("The schematic check failed: {e}"));
                }
            }
        }
    }
    log(format!(
        "pcbferry: {}: {}",
        if o.ok { "done" } else { "FAILED" },
        out.display()
    ));
    Ok(o)
}

fn board_lines(p: &Path, b: &k2a::convert::Report, log: Log, o: &mut Outcome) {
    let counts: Vec<String> = b
        .counts
        .iter()
        .map(|(k, n)| format!("{n} {}", k.trim_end_matches('6').to_lowercase()))
        .collect();
    log(format!(
        "pcbferry: board -> {}: {}",
        p.display(),
        counts.join(", ")
    ));
    for w in &b.warnings {
        log(format!("  note: {w}"));
        o.notes.push(w.clone());
    }
    o.files.push(p.to_path_buf());
}

fn sheet_lines(s: &k2a::sch::Report, log: Log, o: &mut Outcome) {
    let comps: usize = s.sheets.iter().map(|(_, r)| r.components).sum();
    log(format!(
        "pcbferry: schematic -> {} sheet(s), {comps} components",
        s.sheets.len()
    ));
    for (p, r) in &s.sheets {
        o.files.push(p.clone());
        for w in &r.warnings {
            log(format!("  note: {w}"));
            if !o.notes.contains(w) {
                o.notes.push(w.clone());
            }
        }
    }
}

/// Import the PcbDoc back with the engine and compare it with the original board:
/// (all the same, lines to show)
pub fn check_board(
    engine: &Engine,
    input: &Path,
    doc: &Path,
) -> Result<(bool, Vec<String>), String> {
    let dir = std::env::temp_dir().join(format!(
        "pcbferry-check-{}-{}",
        std::process::id(),
        stem(doc)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let back = dir.join("back.kicad_pcb");
    let mut cmd = std::process::Command::new(&engine.cli);
    cmd.args(["pcb", "import", "--format", "altium"])
        .arg(crate::altium::resolve(doc).map_err(|e| e.to_string())?)
        .arg("-o")
        .arg(&back)
        .current_dir(&dir);
    let quiet = |_: String| {};
    let code = crate::run::run(cmd, &quiet);
    let result = (|| {
        if code != Some(0) || !back.exists() {
            return Err("KiCad could not read the PcbDoc".to_string());
        }
        let read = |p: &Path| -> Result<_, String> {
            let text = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
            k2a::kicad::read(&text).map_err(|e| format!("{}: {e}", p.display()))
        };
        let c = k2a::compare::compare(&read(input)?, &read(&back)?);
        let mut lines = vec![format!("board check: {}", c.summary())];
        for cat in &c.categories {
            for e in &cat.examples {
                lines.push(format!("  {}: {e}", cat.name));
            }
        }
        Ok((c.categories.iter().all(|c| c.ok()), lines))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}
