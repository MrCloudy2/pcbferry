//! Comparing two schematics by their KiCad netlists (`kicad-cli sch export netlist`): the
//! same components with the same values and footprints, and the same pins on each net.

use crate::sexpr::{self, Sexpr};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Default)]
pub struct Netlist {
    /// reference -> (value, footprint)
    pub components: BTreeMap<String, (String, String)>,
    /// net name -> its pins (reference, pin number)
    pub nets: Vec<(String, BTreeSet<(String, String)>)>,
}

pub fn parse_netlist(text: &str) -> Result<Netlist, Box<dyn std::error::Error>> {
    let root = sexpr::parse(text)?;
    let mut n = Netlist::default();
    let t = |e: &Sexpr, k: &str| e.arg_text(k, 0).unwrap_or("").to_string();
    if let Some(cs) = root.child("components") {
        for c in cs.children("comp") {
            n.components
                .insert(t(c, "ref"), (t(c, "value"), t(c, "footprint")));
        }
    }
    if let Some(ns) = root.child("nets") {
        for net in ns.children("net") {
            let pins = net
                .children("node")
                .map(|p| (t(p, "ref"), t(p, "pin")))
                .collect();
            n.nets.push((t(net, "name"), pins));
        }
    }
    Ok(n)
}

fn named(name: &str) -> bool {
    !name.starts_with("Net-(") && !name.starts_with("unconnected-(")
}

#[derive(Debug, Default)]
pub struct Diff {
    pub components: usize,
    pub nets: usize,
    pub problems: Vec<String>,
}

impl Diff {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// What `got` (read back from the SchDoc) has different from `want` (KiCad's own)
pub fn compare(want: &Netlist, got: &Netlist) -> Diff {
    let mut d = Diff {
        components: want.components.len(),
        nets: want.nets.len(),
        ..Default::default()
    };
    for (r, (v, f)) in &want.components {
        match got.components.get(r) {
            None => d.problems.push(format!("{r}: missing")),
            Some((gv, gf)) => {
                if gv != v {
                    d.problems.push(format!("{r}: value {gv:?}, want {v:?}"));
                }
                // The importer files footprints under its own library name: compare names
                let name = |s: &str| s.rsplit(':').next().unwrap_or("").to_string();
                if name(gf) != name(f) {
                    d.problems
                        .push(format!("{r}: footprint {gf:?}, want {f:?}"));
                }
            }
        }
    }
    for r in got.components.keys() {
        if !want.components.contains_key(r) {
            d.problems.push(format!("{r}: not in the original"));
        }
    }
    let mut net_of = BTreeMap::new();
    for (i, (_, pins)) in got.nets.iter().enumerate() {
        for p in pins {
            net_of.insert(p, i);
        }
    }
    for (name, pins) in &want.nets {
        let Some(first) = pins.iter().next() else {
            continue;
        };
        let Some(&i) = net_of.get(first) else {
            d.problems
                .push(format!("net {name}: {}.{} on no net", first.0, first.1));
            continue;
        };
        let (gname, gpins) = &got.nets[i];
        if gpins != pins {
            let extra: Vec<_> = gpins
                .difference(pins)
                .map(|(r, p)| format!("{r}.{p}"))
                .collect();
            let lost: Vec<_> = pins
                .difference(gpins)
                .map(|(r, p)| format!("{r}.{p}"))
                .collect();
            d.problems.push(format!(
                "net {name} (as {gname}): missing [{}] extra [{}]",
                lost.join(" "),
                extra.join(" ")
            ));
        } else if named(name) && name.trim_start_matches('/') != gname.trim_start_matches('/') {
            d.problems.push(format!("net {name} is named {gname}"));
        }
    }
    d
}

/// Import the SchDoc with KiCad, export both netlists and compare them
pub fn roundtrip(cli: &Path, input: &Path, doc: &Path) -> Result<Diff, Box<dyn std::error::Error>> {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "kicad2altium-sch-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir)?;
    let back = dir.join("back.kicad_sch");
    let run = |args: &[&std::ffi::OsStr]| -> Result<(), Box<dyn std::error::Error>> {
        let out = Command::new(cli)
            .args(args)
            .current_dir(&dir)
            .output()
            .map_err(|e| format!("{}: {e}", cli.display()))?;
        if !out.status.success() {
            return Err(format!(
                "kicad-cli {:?}: {}",
                args,
                String::from_utf8_lossy(&out.stderr).trim()
            )
            .into());
        }
        Ok(())
    };
    let doc = std::path::absolute(doc)?;
    run(&[
        "sch".as_ref(),
        "import".as_ref(),
        "--format".as_ref(),
        "altium".as_ref(),
        "--output".as_ref(),
        back.as_os_str(),
        doc.as_os_str(),
    ])?;
    let want = dir.join("want.net");
    let got = dir.join("got.net");
    let input = std::path::absolute(input)?;
    run(&[
        "sch".as_ref(),
        "export".as_ref(),
        "netlist".as_ref(),
        "-o".as_ref(),
        want.as_os_str(),
        input.as_os_str(),
    ])?;
    run(&[
        "sch".as_ref(),
        "export".as_ref(),
        "netlist".as_ref(),
        "-o".as_ref(),
        got.as_os_str(),
        back.as_os_str(),
    ])?;
    let w = parse_netlist(&std::fs::read_to_string(&want)?)?;
    let g = parse_netlist(&std::fs::read_to_string(&got)?)?;
    if std::env::var_os("K2A_KEEP").is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    } else {
        eprintln!("kept {}", dir.display());
    }
    Ok(compare(&w, &g))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NET: &str = r#"(export (components (comp (ref "R1") (value "10k") (footprint "R:R0603"))
        (comp (ref "R2") (value "1k") (footprint "")))
      (nets (net (code "1") (name "/SDA") (node (ref "R1") (pin "1")) (node (ref "R2") (pin "2")))
        (net (code "2") (name "Net-(R1-Pad2)") (node (ref "R1") (pin "2")))))"#;

    #[test]
    fn same_netlist_is_equal() {
        let n = parse_netlist(NET).unwrap();
        assert!(compare(&n, &n).ok());
    }

    #[test]
    fn moved_pin_is_reported() {
        let want = parse_netlist(NET).unwrap();
        let mut got = parse_netlist(NET).unwrap();
        let p = got.nets[0].1.pop_last().unwrap();
        got.nets[1].1.insert(p);
        let d = compare(&want, &got);
        assert!(!d.ok());
        assert!(d.problems[0].contains("missing [R2.2]"), "{:?}", d.problems);
    }
}
