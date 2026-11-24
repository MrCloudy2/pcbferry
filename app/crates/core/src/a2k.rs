//! Altium (.PrjPcb, .SchDoc, .PcbDoc) -> a KiCad project folder, with the patched kicad-cli
//! (`sch import`, and a `pcb import` that keeps design rules and links footprints to
//! libraries). A port of the `pcbferry` script: the same steps, the same output and the same
//! `import-reports/summary.json`.

use crate::altium::{self, Listed};
use crate::engine::Engine;
use crate::rules;
use crate::run::{run, Log};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const UNLINKED_SYMBOL: &str = "No library symbol found for schematic symbol";

/// Violation types that say a symbol or footprint is not properly linked to a library; a clean
/// conversion has none.
const LIBRARY_ISSUES: &[&str] = &[
    "lib_symbol_issues",
    "lib_symbol_mismatch",
    "footprint_link_issues",
    "lib_footprint_issues",
    "lib_footprint_mismatch",
];

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// KiCad project name (default: the input's file name)
    pub name: Option<String>,
    /// The PcbDoc to use when a project lists several (default: the first)
    pub board: Option<PathBuf>,
    /// Afterwards, reopen the result: export a netlist and run ERC and DRC
    pub check: bool,
    /// Write into a non-empty output folder
    pub force: bool,
    /// Drop pad and via lands on inner layers where nothing connects, as Altium planes have none
    pub remove_unused_lands: bool,
    /// Don't write the board's scoped Altium rules to NAME.kicad_dru
    pub no_custom_rules: bool,
    /// Fill the zones the import leaves unfilled
    pub fill_zones: bool,
    /// Replace '/' in footprint names with this
    pub footprint_slash: Option<String>,
}

/// What a conversion did: `summary` is what goes into summary.json
pub struct Outcome {
    pub ok: bool,
    pub out: PathBuf,
    pub project: Option<PathBuf>,
    pub summary: Value,
}

/// A failure before any step ran (bad input, output not empty, ...)
#[derive(Debug)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn read_report(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Object(Map::new()))
}

fn messages(report: &Value, key: &str) -> Vec<String> {
    report[key]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|i| match i.get("message") {
                    Some(Value::String(m)) if !m.is_empty() => m.clone(),
                    _ => match i {
                        Value::String(s) => s.clone(),
                        // Python's str() of a dict differs; these never come from kicad-cli
                        v => v.to_string(),
                    },
                })
                .collect()
        })
        .unwrap_or_default()
}

fn by_type(violations: &[Value]) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for v in violations {
        let t = v["type"].as_str().unwrap_or("?").to_string();
        match counts.iter_mut().find(|(k, _)| *k == t) {
            Some(c) => c.1 += 1,
            None => counts.push((t, 1)),
        }
    }
    counts
}

fn library_issues(types: &[(String, usize)]) -> Value {
    let mut m = Map::new();
    for t in LIBRARY_ISSUES {
        if let Some((_, n)) = types.iter().find(|(k, _)| k == t) {
            m.insert(t.to_string(), json!(n));
        }
    }
    Value::Object(m)
}

fn erc_counts(path: &Path) -> Map<String, Value> {
    let r = read_report(path);
    let violations: Vec<Value> = r["sheets"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|s| s["violations"].as_array().cloned().unwrap_or_default())
        .collect();
    let types = by_type(&violations);
    let mut m = Map::new();
    m.insert("violations".into(), json!(violations.len()));
    m.insert("library_issues".into(), library_issues(&types));
    m
}

fn drc_counts(path: &Path) -> Map<String, Value> {
    let r = read_report(path);
    let len = |k: &str| r[k].as_array().map_or(0, Vec::len);
    let violations = r["violations"].as_array().cloned().unwrap_or_default();
    let types = by_type(&violations);
    let mut m = Map::new();
    m.insert("violations".into(), json!(violations.len()));
    m.insert("unconnected_items".into(), json!(len("unconnected_items")));
    m.insert("schematic_parity".into(), json!(len("schematic_parity")));
    m.insert("library_issues".into(), library_issues(&types));
    m
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Reads a check's report into the counts its summary entry gets
type Counts = fn(&Path) -> Map<String, Value>;

struct Run<'a> {
    engine: &'a Engine,
    log: Log<'a>,
    steps: Vec<Value>,
}

impl Run<'_> {
    fn cli(&self, args: &[String]) -> std::process::Command {
        let mut c = std::process::Command::new(&self.engine.cli);
        c.args(args);
        if self.engine.bundled {
            // Set by an AppImage KiCad: it would make the engine load the AppImage's kifaces
            for var in ["APPDIR", "SHARUN_DIR", "KICAD_STOCK_DATA_HOME"] {
                c.env_remove(var);
            }
        }
        c
    }

    fn show(&self, args: &[String]) {
        let mut line = format!("$ {}", s(&self.engine.cli));
        for a in args {
            line.push(' ');
            line += a;
        }
        (self.log)(line);
    }

    fn import(
        &mut self,
        kind: &str,
        src: &Path,
        dest: &Path,
        reports: &Path,
        extra: &[String],
    ) -> bool {
        let report = reports.join(format!("{kind}-import.json"));
        let mut args: Vec<String> = [
            kind,
            "import",
            "--format",
            "altium",
            "--report-format",
            "json",
            "--report-file",
        ]
        .iter()
        .map(|a| a.to_string())
        .collect();
        args.push(s(&report));
        args.extend(extra.iter().cloned());
        args.extend(["--output".into(), s(dest), s(src)]);
        self.show(&args);
        let code = run(self.cli(&args), self.log);
        let data = read_report(&report);
        let ok = code == Some(0) && dest.exists();
        let errors = messages(&data, "errors");
        // Every imported symbol is reported as an error because no KiCad library holds it; the
        // symbols are embedded in the .kicad_sch, so this is expected.
        let unlinked = errors
            .iter()
            .filter(|e| e.starts_with(UNLINKED_SYMBOL))
            .count();
        let rel = report.exists().then(|| {
            format!(
                "{}/{}",
                reports.file_name().unwrap().to_string_lossy(),
                report.file_name().unwrap().to_string_lossy()
            )
        });
        self.steps.push(json!({
            "step": format!("{kind} import"), "input": s(src), "output": s(dest),
            "exit_code": code, "ok": ok,
            "errors": errors.iter().filter(|e| !e.starts_with(UNLINKED_SYMBOL)).collect::<Vec<_>>(),
            "warnings": messages(&data, "warnings"),
            "symbols_without_library": unlinked,
            "report": rel,
        }));
        if !ok {
            let name = src.file_name().unwrap_or_default().to_string_lossy();
            (self.log)(format!(
                "pcbferry: error: {kind} import of {name} failed (exit {})",
                code_text(code)
            ));
        }
        ok
    }

    fn check(
        &mut self,
        name: &str,
        args: Vec<String>,
        output: &Path,
        parse: Option<Counts>,
    ) -> bool {
        self.show(&args);
        let code = run(self.cli(&args), self.log);
        let ok = code == Some(0) && output.exists();
        let mut entry = json!({"step": name, "exit_code": code, "ok": ok, "output": s(output)});
        if let (true, Some(parse)) = (ok, parse) {
            entry.as_object_mut().unwrap().extend(parse(output));
        }
        self.steps.push(entry);
        if !ok {
            (self.log)(format!(
                "pcbferry: error: {name} failed (exit {})",
                code_text(code)
            ));
        }
        ok
    }

    /// Write the board's scoped Altium rules as KiCad custom rules. Never fails the run.
    fn rules(&mut self, pcbdoc: &Path, dru: &Path) {
        let mut entry = json!({"step": "custom rules", "input": s(pcbdoc), "output": s(dru)});
        let e = entry.as_object_mut().unwrap();
        match rules::custom_rules(pcbdoc).and_then(|(text, results)| {
            std::fs::write(dru, text)?;
            Ok(results)
        }) {
            Ok(results) => {
                let mut counts = Map::new();
                for r in &results {
                    let n = counts.get(r.status).and_then(Value::as_u64).unwrap_or(0);
                    counts.insert(r.status.to_string(), json!(n + 1));
                }
                let untranslated: Vec<Value> = results
                    .iter()
                    .filter(|r| matches!(r.status, "untranslatable" | "no_equivalent"))
                    .map(|r| json!({"rule": r.name, "kind": r.kind, "why": r.reason.clone().unwrap_or_default()}))
                    .collect();
                let n = |k: &str| counts.get(k).and_then(Value::as_u64).unwrap_or(0);
                (self.log)(format!(
                    "pcbferry: custom rules: {} written to {}, {} imported as board settings and netclasses",
                    n("translated"),
                    dru.file_name().unwrap_or_default().to_string_lossy(),
                    n("imported")
                ));
                e.insert("ok".into(), json!(true));
                e.insert("rules".into(), Value::Object(counts));
                e.insert("untranslated".into(), json!(untranslated));
            }
            Err(err) => {
                (self.log)(format!(
                    "pcbferry: warning: custom rules not written ({err})"
                ));
                e.insert("ok".into(), json!(false));
                e.insert("errors".into(), json!([err.to_string()]));
            }
        }
        self.steps.push(entry);
    }
}

fn code_text(code: Option<i32>) -> String {
    code.map_or("signal".into(), |c| c.to_string())
}

/// Convert `input` (.PrjPcb, .SchDoc or .PcbDoc) into the KiCad project folder `out`
pub fn convert(
    engine: &Engine,
    input: &Path,
    out: &Path,
    opts: &Options,
    log: Log,
) -> Result<Outcome, Refused> {
    let src = altium::resolve(input).map_err(|e| Refused(e.to_string()))?;
    if !src.is_file() {
        return Err(Refused(format!("{}: no such file", input.display())));
    }
    let kind = ["prjpcb", "schdoc", "pcbdoc"]
        .into_iter()
        .find(|k| altium::is(&src, k))
        .ok_or_else(|| {
            Refused(format!(
                "{}: expected a .PrjPcb, .SchDoc or .PcbDoc",
                input.display()
            ))
        })?;
    let out = altium::resolve(out).map_err(|e| Refused(e.to_string()))?;
    if out.read_dir().is_ok_and(|mut d| d.next().is_some()) && !opts.force {
        return Err(Refused(format!("{} is not empty", out.display())));
    }
    let reports = out.join("import-reports");
    std::fs::create_dir_all(&reports)
        .map_err(|e| Refused(format!("{}: {e}", reports.display())))?;

    let name = opts
        .name
        .clone()
        .unwrap_or_else(|| src.file_stem().unwrap().to_string_lossy().into_owned());
    let (mut sch_src, mut pcb_src) = (None, None);
    match kind {
        "prjpcb" => {
            let docs = altium::project_documents(&src)
                .map_err(|e| Refused(format!("{}: {e}", src.display())))?;
            let mut boards = Vec::new();
            for d in &docs {
                match d {
                    Listed::Found(p) if altium::is(p, "schdoc") => sch_src = Some(src.clone()),
                    Listed::Found(p) if altium::is(p, "pcbdoc") => boards.push(p.clone()),
                    Listed::Found(_) => {}
                    Listed::Missing(rel) => log(format!(
                        "pcbferry: warning: {} lists {rel}, which was not found",
                        src.file_name().unwrap().to_string_lossy()
                    )),
                }
            }
            // sch_src is the project itself: import the whole project (flat designs, variants)
            if let Some(b) = &opts.board {
                pcb_src = Some(altium::resolve(b).map_err(|e| Refused(e.to_string()))?);
            } else if let Some(first) = boards.first() {
                pcb_src = Some(first.clone());
                if boards.len() > 1 {
                    log(format!(
                        "pcbferry: warning: project lists {} boards; converting {} (choose another with --board)",
                        boards.len(),
                        first.file_name().unwrap().to_string_lossy()
                    ));
                }
            }
            if sch_src.is_none() && pcb_src.is_none() {
                return Err(Refused(format!(
                    "{} lists no SchDoc or PcbDoc that exists",
                    src.file_name().unwrap().to_string_lossy()
                )));
            }
        }
        "schdoc" => sch_src = Some(src.clone()),
        _ => pcb_src = Some(src.clone()),
    }

    let mut r = Run {
        engine,
        log,
        steps: Vec::new(),
    };
    let kicad_sch = out.join(format!("{name}.kicad_sch"));
    let kicad_pcb = out.join(format!("{name}.kicad_pcb"));
    let mut ok = true;
    // Board first: the schematic import takes each symbol's footprint library link from the
    // board part with the same reference in NAME.kicad_pcb. Both write their settings (rules,
    // netclasses, libraries) to NAME.kicad_pro.
    if let Some(pcb) = &pcb_src {
        // Before the import, so --fill-zones fills planes with Altium's plane rules
        if !opts.no_custom_rules {
            r.rules(pcb, &out.join(format!("{name}.kicad_dru")));
        }
        let mut extra = Vec::new();
        if opts.remove_unused_lands {
            extra.push("--remove-unused-lands".to_string());
        }
        if opts.fill_zones {
            extra.push("--fill-zones".to_string());
        }
        if let Some(c) = &opts.footprint_slash {
            extra.extend(["--footprint-slash".to_string(), c.clone()]);
        }
        ok &= r.import("pcb", pcb, &kicad_pcb, &reports, &extra);
    }
    if let Some(sch) = &sch_src {
        ok &= r.import("sch", sch, &kicad_sch, &reports, &[]);
    }
    // The second import rewrites the library tables and leaves .bak copies of tables the first
    // one wrote moments earlier.
    if let Ok(rd) = std::fs::read_dir(&out) {
        for e in rd.flatten() {
            if e.file_name().to_string_lossy().ends_with("-lib-table.bak") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    if opts.check && ok {
        if sch_src.is_some() {
            let net = reports.join(format!("{name}.net"));
            let args = ["sch", "export", "netlist", "-o"]
                .map(String::from)
                .into_iter()
                .chain([s(&net), s(&kicad_sch)])
                .collect();
            ok &= r.check("sch export netlist", args, &net, None);
            let erc = reports.join("erc.json");
            let args = ["sch", "erc", "--format", "json", "--severity-all", "-o"]
                .map(String::from)
                .into_iter()
                .chain([s(&erc), s(&kicad_sch)])
                .collect();
            ok &= r.check("sch erc", args, &erc, Some(erc_counts));
        }
        if pcb_src.is_some() {
            let drc = reports.join("drc.json");
            let mut args: Vec<String> = ["pcb", "drc", "--format", "json", "--severity-all", "-o"]
                .map(String::from)
                .into();
            args.push(s(&drc));
            if sch_src.is_some() {
                args.push("--schematic-parity".into());
            }
            args.push(s(&kicad_pcb));
            ok &= r.check("pcb drc", args, &drc, Some(drc_counts));
        }
    }

    let summary = json!({
        "tool": format!("pcbferry {}", crate::engine::VERSION),
        "input": s(&src), "output": s(&out), "project": name,
        "options": {"remove_unused_lands": opts.remove_unused_lands,
                    "footprint_slash": opts.footprint_slash,
                    "fill_zones": opts.fill_zones,
                    "custom_rules": !opts.no_custom_rules},
        "steps": r.steps,
        "ok": ok,
    });
    let _ = std::fs::write(
        reports.join("summary.json"),
        serde_json::to_string_pretty(&summary).unwrap_or_default() + "\n",
    );
    let warnings: usize = summary["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["warnings"].as_array().map_or(0, Vec::len))
        .sum();
    let errors: usize = summary["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["errors"].as_array().map_or(0, Vec::len))
        .sum();
    if opts.check {
        let expected: Vec<&str> = [
            sch_src.is_some().then_some("sch erc"),
            pcb_src.is_some().then_some("pcb drc"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let steps = summary["steps"].as_array().unwrap();
        let checked: Vec<&str> = steps
            .iter()
            .filter(|s| s.get("library_issues").is_some())
            .filter_map(|s| s["step"].as_str())
            .collect();
        if checked.len() != expected.len() || !expected.iter().all(|e| checked.contains(e)) {
            log("pcbferry: library links: not checked (a step failed)".into());
        } else {
            let mut issues: Vec<(String, u64)> = Vec::new();
            for st in steps {
                for (t, n) in st["library_issues"].as_object().into_iter().flatten() {
                    let n = n.as_u64().unwrap_or(0);
                    match issues.iter_mut().find(|(k, _)| k == t) {
                        Some(i) => i.1 += n,
                        None => issues.push((t.clone(), n)),
                    }
                }
            }
            let text: Vec<String> = issues.iter().map(|(t, n)| format!("{n} {t}")).collect();
            log(format!(
                "pcbferry: library links: {}",
                if text.is_empty() {
                    "all clean".into()
                } else {
                    text.join(", ")
                }
            ));
        }
    }
    log(format!(
        "pcbferry: {}: {} ({warnings} warnings, {errors} errors; see {}/import-reports/summary.json)",
        if ok { "done" } else { "FAILED" },
        out.display(),
        out.file_name().unwrap_or_default().to_string_lossy()
    ));
    let project = out.join(format!("{name}.kicad_pro"));
    Ok(Outcome {
        ok,
        project: project.exists().then_some(project),
        out,
        summary,
    })
}

/// What pcbferry's library checks mean, for people
fn library_issue_text(kind: &str) -> &str {
    match kind {
        "lib_footprint_mismatch" => {
            "footprint(s) differ from their library copy; the board keeps its own version"
        }
        "lib_footprint_issues" => "footprint(s) not found in a library",
        "footprint_link_issues" => "symbol(s) link to a footprint that isn't in a library",
        "lib_symbol_issues" => "symbol(s) not found in a library",
        "lib_symbol_mismatch" => "symbol(s) differ from their library copy",
        k => k,
    }
}

/// Things worth a look, from a summary, in plain words
pub fn notes(summary: &Value) -> Vec<String> {
    let mut notes = Vec::new();
    let steps = summary["steps"].as_array().cloned().unwrap_or_default();
    for step in &steps {
        if step["ok"] != json!(true) {
            notes.push(format!(
                "The {} step failed.",
                step["step"].as_str().unwrap_or("?")
            ));
        }
        let missing: Vec<String> = step["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|e| e.starts_with("File not found"))
            .map(|e| {
                e.replace('\\', "/")
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('.')
                    .to_string()
            })
            .collect();
        if !missing.is_empty() {
            notes.push(format!(
                "{} linked file(s) weren't found (they're on the original PC): {}",
                missing.len(),
                missing.join(", ")
            ));
        }
        for (kind, n) in step["library_issues"].as_object().into_iter().flatten() {
            if n.as_u64().unwrap_or(0) > 0 {
                notes.push(format!("{n} {}", library_issue_text(kind)));
            }
        }
    }
    if summary["options"]["custom_rules"] == json!(true)
        && !steps
            .iter()
            .any(|s| s["step"] == "custom rules" && s["ok"] == json!(true))
        && steps.iter().any(|s| s["step"] == "pcb import")
    {
        notes.push("Altium's design rules couldn't be read.".into());
    }
    notes
}
