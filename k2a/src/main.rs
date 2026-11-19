use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: kicad2altium [--check] BOARD.kicad_pcb [OUT.PcbDoc]

Writes the board as an Altium Designer PCB (binary PcbDoc). Without OUT, next to the input.

  --check   read the PcbDoc back with KiCad's Altium importer (kicad-cli, or $KICAD_CLI)
            and compare it with the board, object by object";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!("{USAGE}");
        return if args.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }
    let check = args.iter().any(|a| a == "--check");
    args.retain(|a| a != "--check");
    let input = PathBuf::from(&args[0]);
    let output = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| input.with_extension("PcbDoc"));
    let report = match pcbferry_k2a::convert_file(&input, &output) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("kicad2altium: {}: {e}", input.display());
            return ExitCode::FAILURE;
        }
    };
    let counts: Vec<String> = report
        .counts
        .iter()
        .map(|(k, n)| format!("{n} {}", k.trim_end_matches('6').to_lowercase()))
        .collect();
    println!(
        "{} -> {}: {}",
        input.display(),
        output.display(),
        counts.join(", ")
    );
    for w in &report.warnings {
        println!("  note: {w}");
    }
    if check {
        return match run_check(&input, &output) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(1),
            Err(e) => {
                eprintln!("kicad2altium: check: {e}");
                ExitCode::FAILURE
            }
        };
    }
    ExitCode::SUCCESS
}

fn run_check(input: &Path, doc: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let cli = std::env::var_os("KICAD_CLI")
        .map(PathBuf::from)
        .unwrap_or_else(|| "kicad-cli".into());
    let dir = std::env::temp_dir().join(format!("kicad2altium-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let back = dir.join("back.kicad_pcb");
    let out = Command::new(&cli)
        .args(["pcb", "import", "--format", "altium"])
        .arg(std::path::absolute(doc)?)
        .arg("-o")
        .arg(&back)
        .current_dir(&dir)
        .output()
        .map_err(|e| format!("{}: {e}", cli.display()))?;
    if !out.status.success() || !back.exists() {
        return Err(format!(
            "KiCad could not read the PcbDoc: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into());
    }
    let orig = pcbferry_k2a::kicad::read(&std::fs::read_to_string(input)?)?;
    let got = pcbferry_k2a::kicad::read(&std::fs::read_to_string(&back)?)?;
    let c = pcbferry_k2a::compare::compare(&orig, &got);
    let _ = std::fs::remove_dir_all(&dir);
    println!("check: {}", c.summary());
    for cat in &c.categories {
        for e in &cat.examples {
            println!("  {}: {e}", cat.name);
        }
    }
    Ok(c.categories.iter().all(|c| c.ok()))
}
