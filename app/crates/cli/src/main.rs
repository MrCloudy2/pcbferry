//! pcbferry on the command line: Altium -> KiCad (the same flags as the pcbferry script) and
//! KiCad -> Altium.

use pcbferry_core::{a2k, engine, k2a};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
usage: pcbferry [options] INPUT OUTPUT_DIR

Altium -> KiCad: INPUT is a .PrjPcb, .SchDoc or .PcbDoc; OUTPUT_DIR is created (it must be
empty unless --force) and gets NAME.kicad_pro and the rest of a KiCad project.
KiCad -> Altium: INPUT is a .kicad_pro, .kicad_pcb or .kicad_sch; OUTPUT_DIR (default
NAME-altium next to it) gets the PcbDoc, SchDocs and a .PrjPcb.

options:
  --name NAME              KiCad project name (default: the input's file name)
  --board PCBDOC           PcbDoc to use when a project lists several (default: the first)
  --check                  afterwards, check the result (Altium -> KiCad: netlist, ERC, DRC;
                           KiCad -> Altium: read it back with KiCad's importer and compare)
  --force                  write into a non-empty output folder
  --remove-unused-lands    drop pad and via lands on inner layers where nothing connects
  --no-custom-rules        don't write the board's scoped Altium rules to NAME.kicad_dru
  --fill-zones             fill the zones the import leaves unfilled
  --footprint-slash CHAR   replace '/' in footprint names with CHAR
  --install-engine         download the converter engine for this version and exit
  --custom-rules PCBDOC    print the custom rules pcbferry writes for a board, then what
                           happened to each Altium rule, and exit
  --version

The converter engine (pcbferry's patched kicad-cli) is downloaded on first use; KICAD_CLI
points at another one.";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut opts = a2k::Options::default();
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut install_only = false;
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        let r: Result<(), String> = match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            "--version" => {
                println!("pcbferry {}", engine::VERSION);
                return ExitCode::SUCCESS;
            }
            "--name" => value("--name").map(|v| opts.name = Some(v)),
            "--board" => value("--board").map(|v| opts.board = Some(v.into())),
            "--check" => {
                opts.check = true;
                Ok(())
            }
            "--force" => {
                opts.force = true;
                Ok(())
            }
            "--remove-unused-lands" => {
                opts.remove_unused_lands = true;
                Ok(())
            }
            "--no-custom-rules" => {
                opts.no_custom_rules = true;
                Ok(())
            }
            "--fill-zones" => {
                opts.fill_zones = true;
                Ok(())
            }
            "--footprint-slash" => {
                value("--footprint-slash").map(|v| opts.footprint_slash = Some(v))
            }
            "--install-engine" => {
                install_only = true;
                Ok(())
            }
            "--custom-rules" => match value("--custom-rules") {
                Ok(board) => return custom_rules(board.as_ref()),
                Err(e) => Err(e),
            },
            s if s.starts_with('-') && s.len() > 1 => Err(format!("unknown option {s}")),
            s => {
                paths.push(s.into());
                Ok(())
            }
        };
        if let Err(e) = r {
            eprintln!("pcbferry: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    let log = |line: String| eprintln!("{line}");
    if install_only {
        return match get_engine(&log) {
            Some(e) => {
                println!("{}", e.cli.display());
                ExitCode::SUCCESS
            }
            None => ExitCode::FAILURE,
        };
    }
    let Some(input) = paths.first().cloned() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let ext = input
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext.starts_with("kicad_") {
        let out = paths
            .get(1)
            .cloned()
            .unwrap_or_else(|| k2a::default_out(&input));
        let engine = if opts.check {
            match get_engine(&log) {
                Some(e) => Some(e),
                None => return ExitCode::from(2),
            }
        } else {
            None
        };
        return match k2a::convert(&input, &out, engine.as_ref(), &log) {
            Ok(o) if o.ok => ExitCode::SUCCESS,
            Ok(_) => ExitCode::FAILURE,
            Err(e) => {
                eprintln!("pcbferry: error: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let Some(out) = paths.get(1).cloned() else {
        eprintln!("pcbferry: an output folder is needed\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let Some(engine) = get_engine(&log) else {
        return ExitCode::from(2);
    };
    match a2k::convert(&engine, &input, &out, &opts, &log) {
        Ok(o) if o.ok => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("pcbferry: error: {e}");
            ExitCode::from(2)
        }
    }
}

fn custom_rules(board: &std::path::Path) -> ExitCode {
    match pcbferry_core::rules::custom_rules(board) {
        Ok((text, results)) => {
            print!("{text}");
            for r in results {
                println!(
                    "#> {} | {} | {}",
                    r.name,
                    r.status,
                    r.reason.unwrap_or_default()
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("pcbferry: error: {}: {e}", board.display());
            ExitCode::FAILURE
        }
    }
}

fn get_engine(log: &(dyn Fn(String) + Sync)) -> Option<engine::Engine> {
    if let Some(e) = engine::find() {
        return Some(e);
    }
    let info = match engine::download_info() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("pcbferry: error: {e}");
            return None;
        }
    };
    log(format!(
        "pcbferry: downloading the converter engine ({:.0} MB), once",
        info.size as f64 / 1e6
    ));
    let shown = std::sync::atomic::AtomicU64::new(0);
    let progress = |done: u64, total: u64| {
        let pct = (done * 100).checked_div(total).unwrap_or(0);
        if pct >= shown.load(std::sync::atomic::Ordering::Relaxed) + 10 {
            shown.store(pct, std::sync::atomic::Ordering::Relaxed);
            eprintln!("pcbferry:   {pct}%");
        }
    };
    match engine::install(&info, &progress) {
        Ok(e) => Some(e),
        Err(e) => {
            eprintln!("pcbferry: error: {e}");
            None
        }
    }
}
