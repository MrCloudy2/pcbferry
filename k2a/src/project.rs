//! The KiCad project file next to a board (.kicad_pro, JSON): net classes, which nets are in
//! them, and the board's design rules. Lengths in nanometres.

use crate::kicad::Nm;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct NetClass {
    pub name: String,
    pub clearance: Option<Nm>,
    pub track_width: Option<Nm>,
    pub via_diameter: Option<Nm>,
    pub via_drill: Option<Nm>,
    pub diff_pair_width: Option<Nm>,
    pub diff_pair_gap: Option<Nm>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rules {
    pub min_clearance: Option<Nm>,
    pub min_track_width: Option<Nm>,
    pub min_via_diameter: Option<Nm>,
    pub min_through_hole: Option<Nm>,
    pub min_hole_to_hole: Option<Nm>,
    pub min_copper_edge: Option<Nm>,
    pub min_annular: Option<Nm>,
    pub min_silk_clearance: Option<Nm>,
}

#[derive(Debug, Clone, Default)]
pub struct Project {
    pub classes: Vec<NetClass>,
    /// (pattern, class), in the file's order
    pub patterns: Vec<(String, String)>,
    /// (net, class) set net by net
    pub assignments: Vec<(String, String)>,
    pub rules: Rules,
}

fn len(v: &Value, key: &str) -> Option<Nm> {
    v.get(key)?.as_f64().map(|mm| (mm * 1e6).round() as Nm)
}

pub fn parse(text: &str) -> Result<Project, serde_json::Error> {
    let v: Value = serde_json::from_str(text)?;
    let mut p = Project::default();
    let ns = &v["net_settings"];
    for c in ns["classes"].as_array().into_iter().flatten() {
        let Some(name) = c["name"].as_str() else {
            continue;
        };
        p.classes.push(NetClass {
            name: name.to_string(),
            clearance: len(c, "clearance"),
            track_width: len(c, "track_width"),
            via_diameter: len(c, "via_diameter"),
            via_drill: len(c, "via_drill"),
            diff_pair_width: len(c, "diff_pair_width"),
            diff_pair_gap: len(c, "diff_pair_gap"),
        });
    }
    for pat in ns["netclass_patterns"].as_array().into_iter().flatten() {
        if let (Some(c), Some(pt)) = (pat["netclass"].as_str(), pat["pattern"].as_str()) {
            p.patterns.push((pt.to_string(), c.to_string()));
        }
    }
    if let Some(a) = ns["netclass_assignments"].as_object() {
        for (net, c) in a {
            // A string, or (KiCad 8 and later) a list of classes
            let class = c.as_str().or_else(|| {
                c.as_array()
                    .and_then(|l| l.first())
                    .and_then(|x| x.as_str())
            });
            if let Some(class) = class {
                p.assignments.push((net.clone(), class.to_string()));
            }
        }
    }
    let r = &v["board"]["design_settings"]["rules"];
    p.rules = Rules {
        min_clearance: len(r, "min_clearance"),
        min_track_width: len(r, "min_track_width"),
        min_via_diameter: len(r, "min_via_diameter"),
        min_through_hole: len(r, "min_through_hole_diameter"),
        min_hole_to_hole: len(r, "min_hole_to_hole"),
        min_copper_edge: len(r, "min_copper_edge_clearance"),
        min_annular: len(r, "min_via_annular_width"),
        min_silk_clearance: len(r, "min_silk_clearance"),
    };
    Ok(p)
}

/// KiCad's wildcard match: * any run, ? one character, case sensitive
pub fn wildcard(pattern: &str, s: &str) -> bool {
    let (p, s): (Vec<char>, Vec<char>) = (pattern.chars().collect(), s.chars().collect());
    let (mut i, mut j, mut star, mut mark) = (0, 0, None, 0);
    while j < s.len() {
        if i < p.len() && (p[i] == '?' || p[i] == s[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            mark = j;
            i += 1;
        } else if let Some(st) = star {
            i = st + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}

impl Project {
    pub fn class(&self, name: &str) -> Option<&NetClass> {
        self.classes.iter().find(|c| c.name == name)
    }

    pub fn default_class(&self) -> Option<&NetClass> {
        self.class("Default")
    }

    /// The class a net is in: its own assignment, else the first pattern it matches
    pub fn class_of(&self, net: &str) -> &str {
        if let Some((_, c)) = self.assignments.iter().find(|(n, _)| n == net) {
            return c;
        }
        self.patterns
            .iter()
            .find(|(pat, _)| wildcard(pat, net))
            .map(|(_, c)| c.as_str())
            .unwrap_or("Default")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard("+3.3V", "+3.3V"));
        assert!(!wildcard("+3.3V", "+3.3VA"));
        assert!(wildcard("/USB_D*", "/USB_D+"));
        assert!(wildcard("D?", "D1"));
        assert!(!wildcard("D?", "D12"));
        assert!(wildcard("*", ""));
    }

    #[test]
    fn classes_and_rules() {
        let p = parse(
            r#"{"board": {"design_settings": {"rules": {"min_clearance": 0.1, "min_track_width": 0.2}}},
                "net_settings": {"classes": [{"name": "Default", "clearance": 0.2, "track_width": 0.25},
                                             {"name": "pwr", "clearance": 0.3, "track_width": 0.5}],
                                 "netclass_patterns": [{"netclass": "pwr", "pattern": "+*V"}],
                                 "netclass_assignments": {"GND": ["pwr"]}}}"#,
        )
        .unwrap();
        assert_eq!(p.rules.min_clearance, Some(100_000));
        assert_eq!(p.class("pwr").unwrap().track_width, Some(500_000));
        assert_eq!(p.class_of("+5V"), "pwr");
        assert_eq!(p.class_of("GND"), "pwr");
        assert_eq!(p.class_of("SIG"), "Default");
    }
}
