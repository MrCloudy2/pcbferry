//! KiCad's group buses (`PREFIX{A B C}`, or `PREFIX{ALIAS}`) as Altium signal harnesses.
//!
//! Altium's buses carry vectors only (`D[0..7]`). A group of named members crossing the
//! hierarchy is, in Altium, a harness: a port and a sheet entry of a harness type, a signal
//! harness between sheet entries, and a harness connector where the members leave it. Altium
//! matches harness members by name across the hierarchy, as KiCad matches group members.

use super::kicad::{LabelKind, Schematic, P};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A group label's prefix and its members, after alias expansion, with vector members
/// (`D[9..11]`) spelled out bit by bit. `None` for a plain name or a vector bus
pub fn group(text: &str) -> Option<(String, Vec<String>)> {
    let body = text.strip_suffix('}')?;
    let a = open_brace(text)?;
    // `_{..}`, `^{..}` and `~{..}` are KiCad's markup, not a group
    if text[..a].ends_with(['_', '^', '~']) {
        return None;
    }
    let mut members = Vec::new();
    for part in body[a + 1..].split([' ', ',']).filter(|p| !p.is_empty()) {
        match bits(part) {
            Some(b) => members.extend(b),
            None => members.push(part.to_string()),
        }
    }
    if members.is_empty() {
        return None;
    }
    let prefix = text[..a].to_string();
    // `{DDR4-PS}` naming an alias of `DDR4-PS.A0`...: the members' shared prefix is the group's
    // (its nets are DDR4-PS.A0... either way), so the harness has a name
    if prefix.is_empty() {
        if let Some((p, _)) = members[0].split_once('.') {
            let head = format!("{p}.");
            if members
                .iter()
                .all(|m| m.starts_with(&head) && m.len() > head.len())
            {
                let rest = members
                    .iter()
                    .map(|m| m[head.len()..].to_string())
                    .collect();
                return Some((p.to_string(), rest));
            }
        }
    }
    Some((prefix, members))
}

/// The brace that opens the group closing the text (`SWD{~{RESET}, CLK}`: the first one, as
/// the overbar's braces nest inside)
pub fn open_brace(text: &str) -> Option<usize> {
    if !text.ends_with('}') {
        return None;
    }
    let mut depth = 0;
    for (i, c) in text.char_indices().rev() {
        match c {
            '}' => depth += 1,
            '{' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A vector `NAME[a..b]` bit by bit, in KiCad's order (from a to b)
pub fn bits(part: &str) -> Option<Vec<String>> {
    let (name, range) = part.strip_suffix(']')?.rsplit_once('[')?;
    let (a, b) = range.split_once("..")?;
    let (a, b): (i64, i64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    let n: Vec<i64> = if a <= b {
        (a..=b).collect()
    } else {
        (b..=a).rev().collect()
    };
    Some(n.into_iter().map(|i| format!("{name}{i}")).collect())
}

/// Whether a group with this prefix can be a harness: it needs a name, and one without KiCad's
/// markup (`PCIe_{C1x1}`), as KiCad's importer tells harness ports apart by the text before the
/// first brace
fn harnessable(prefix: &str) -> bool {
    !prefix.is_empty() && !prefix.contains(['{', '}'])
}

/// The net KiCad gives a group member: `PREFIX.MEMBER`, or the member alone without a prefix
pub fn member_net(prefix: &str, member: &str) -> String {
    if prefix.is_empty() {
        member.to_string()
    } else {
        format!("{prefix}.{member}")
    }
}

/// The harness type of a group: the alias it names, or an alias with the same members, or a
/// name made of the members. Groups of the same members share a type wherever they are
pub fn harness_type(
    written: &str,
    members: &[String],
    aliases: &HashMap<String, Vec<String>>,
) -> String {
    if let Some(body) = written.strip_suffix('}') {
        if let Some(a) = open_brace(written) {
            let name = body[a + 1..].trim();
            if aliases.contains_key(name) {
                return safe(name);
            }
        }
    }
    let set: HashSet<&String> = members.iter().collect();
    let mut same: Vec<&String> = aliases
        .iter()
        .filter(|(_, m)| {
            let expanded: Vec<String> = m
                .iter()
                .flat_map(|x| bits(x).unwrap_or_else(|| vec![x.clone()]))
                .collect();
            expanded.len() == set.len() && expanded.iter().all(|x| set.contains(x))
        })
        .map(|(n, _)| n)
        .collect();
    same.sort();
    if let Some(n) = same.first() {
        return safe(n);
    }
    let mut sorted: Vec<&String> = members.iter().collect();
    sorted.sort();
    let joined = sorted
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join("_");
    if joined.len() <= 40 {
        safe(&joined)
    } else {
        // FNV-1a of the members: short and the same on every sheet
        let mut h: u32 = 0x811c_9dc5;
        for b in joined.bytes() {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
        format!("GROUP_{h:08X}")
    }
}

/// A harness type as one word: Altium allows spaces, KiCad's groups use them as separators
fn safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Whether a point lies on a segment, ends included
pub fn on_segment(p: P, s: (P, P)) -> bool {
    on(p, s)
}

fn on(p: P, (a, b): (P, P)) -> bool {
    let cross =
        (b.x - a.x) as i128 * (p.y - a.y) as i128 - (b.y - a.y) as i128 * (p.x - a.x) as i128;
    cross == 0
        && p.x >= a.x.min(b.x)
        && p.x <= a.x.max(b.x)
        && p.y >= a.y.min(b.y)
        && p.y <= a.y.max(b.y)
}

/// How a sheet's group buses are written
#[derive(Default, Debug)]
pub struct Plan {
    /// Bus segments drawn as signal harness (they join harness ports and sheet entries only)
    pub harness_segments: HashSet<usize>,
    /// Ports (by label index) that carry a harness, with its type
    pub ports: HashMap<usize, String>,
    /// Of those, the ports whose members leave the bus on this sheet: each gets a harness
    /// connector at its free end, with these member nets on its entries
    pub connectors: HashMap<usize, Vec<(String, Vec<String>)>>,
    /// Sheet entries (sheet index, pin index) that carry a harness, with its type
    pub entries: HashMap<(usize, usize), String>,
    /// Connectors on a harness drawn from a bus that also reaches sheet entries: where the
    /// harness line leaves the bus, which way, its type, members and member net names
    #[allow(clippy::type_complexity)]
    pub bus_connectors: Vec<(P, P, String, Vec<String>, Vec<(String, Vec<String>)>)>,
    /// Harness types and their members
    pub types: BTreeMap<String, Vec<String>>,
    pub warnings: Vec<String>,
}

/// The labels `expand` spells out (aliases expanded, escapes resolved)
pub fn plan(
    sch: &Schematic,
    expand: &dyn Fn(&str) -> String,
    aliases: &HashMap<String, Vec<String>>,
) -> Plan {
    let mut plan = Plan::default();
    let buses = &sch.buses;
    // The connected bus networks: segments joined end to end or end to side
    let mut parent: Vec<usize> = (0..buses.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..buses.len() {
        for j in i + 1..buses.len() {
            let (a, b) = (buses[i], buses[j]);
            if on(a.0, b) || on(a.1, b) || on(b.0, a) || on(b.1, a) {
                let (x, y) = (find(&mut parent, i), find(&mut parent, j));
                parent[x] = y;
            }
        }
    }
    let net_of = |parent: &mut Vec<usize>, p: P| -> Option<usize> {
        (0..buses.len())
            .find(|&i| on(p, buses[i]))
            .map(|i| find(parent, i))
    };
    let local: HashSet<String> = sch
        .labels
        .iter()
        .filter(|l| l.kind == LabelKind::Local)
        .map(|l| expand(&l.text))
        .collect();

    #[derive(Default)]
    struct Net {
        ports: Vec<(usize, String, Vec<String>, String)>,
        entries: Vec<((usize, usize), String)>,
        prefixes: Vec<(String, Vec<String>)>,
        breakouts: bool,
    }
    let mut nets: BTreeMap<usize, Net> = BTreeMap::new();
    for (li, l) in sch.labels.iter().enumerate() {
        let text = expand(&l.text);
        // A group without a prefix has no name for its harness: left as KiCad drew it
        let Some((prefix, members)) = group(&text).filter(|g| harnessable(&g.0)) else {
            continue;
        };
        let Some(n) = net_of(&mut parent, l.at) else {
            if l.kind == LabelKind::Hierarchical {
                // A group port on no bus: its members only meet by name
                let t = harness_type(&l.text, &members, aliases);
                plan.types.entry(t.clone()).or_insert(members.clone());
                plan.ports.insert(li, t);
                plan.connectors
                    .insert(li, vec![(prefix.clone(), members.clone())]);
            }
            continue;
        };
        let t = harness_type(&l.text, &members, aliases);
        plan.types.entry(t.clone()).or_insert(members.clone());
        let net = nets.entry(n).or_default();
        net.prefixes.push((prefix.clone(), members.clone()));
        if l.kind == LabelKind::Hierarchical {
            net.ports.push((li, t, members, prefix));
        }
    }
    for (si, s) in sch.sheets.iter().enumerate() {
        for (pi, pin) in s.pins.iter().enumerate() {
            let text = expand(&pin.name);
            let Some((prefix, members)) = group(&text).filter(|g| harnessable(&g.0)) else {
                continue;
            };
            let t = harness_type(&pin.name, &members, aliases);
            plan.types.entry(t.clone()).or_insert(members.clone());
            match net_of(&mut parent, pin.at) {
                Some(n) => {
                    let net = nets.entry(n).or_default();
                    net.entries.push(((si, pi), t));
                    net.prefixes.push((prefix, members));
                }
                None => {
                    plan.warnings.push(format!(
                        "sheet entry {} is on no bus: written as a plain entry",
                        pin.name
                    ));
                }
            }
        }
    }
    for (n, net) in nets.iter_mut() {
        let entry_on = buses
            .iter()
            .enumerate()
            .filter(|(i, _)| find(&mut parent, *i) == *n)
            .any(|(_, seg)| {
                sch.bus_entries
                    .iter()
                    .any(|(a, b)| on(*a, *seg) || on(*b, *seg))
            });
        let named = net.prefixes.iter().any(|(prefix, members)| {
            members
                .iter()
                .any(|m| local.contains(&member_net(prefix, m)))
        });
        net.breakouts = entry_on || named;
    }
    for (n, net) in nets {
        if net.ports.is_empty() && net.entries.is_empty() {
            continue;
        }
        if !net.breakouts {
            for i in 0..buses.len() {
                if find(&mut parent, i) == n {
                    plan.harness_segments.insert(i);
                }
            }
            for (li, t, _, _) in net.ports {
                plan.ports.insert(li, t);
            }
            for (k, t) in net.entries {
                plan.entries.insert(k, t);
            }
            continue;
        }
        // The members leave the bus here: each port gets its own connector, naming the
        // members as every group on this bus names them
        let prefixes: Vec<(String, Vec<String>)> = net
            .prefixes
            .iter()
            .filter(|(p, m)| m.iter().any(|m| local.contains(&member_net(p, m))))
            .cloned()
            .collect();
        for (li, t, members, prefix) in &net.ports {
            plan.ports.insert(*li, t.clone());
            let mut names = prefixes.clone();
            if names.is_empty() {
                names.push((prefix.clone(), members.clone()));
            }
            plan.connectors.insert(*li, names);
        }
        if !net.entries.is_empty() {
            // The bus reaches sheet entries too: it is a harness, and one connector on it
            // takes the members to their wires
            let mut segs = Vec::new();
            for (i, seg) in buses.iter().enumerate() {
                if find(&mut parent, i) == n {
                    plan.harness_segments.insert(i);
                    segs.push(*seg);
                }
            }
            for (li, t, _, _) in &net.ports {
                plan.connectors.remove(li);
                plan.ports.insert(*li, t.clone());
            }
            let mut t = None;
            for (k, et) in &net.entries {
                plan.entries.insert(*k, et.clone());
                t.get_or_insert(et.clone());
            }
            let t = t.unwrap_or_default();
            let members = plan.types.get(&t).cloned().unwrap_or_default();
            // From a bus end nothing else meets, else the first segment's start
            let taken: Vec<P> = sch
                .sheets
                .iter()
                .flat_map(|s| s.pins.iter().map(|p| p.at))
                .chain(sch.labels.iter().map(|l| l.at))
                .chain(sch.bus_entries.iter().flat_map(|(a, b)| [*a, *b]))
                .collect();
            let ends: Vec<(P, P)> = segs
                .iter()
                .flat_map(|&(a, b)| [(a, b), (b, a)])
                .filter(|(e, _)| {
                    segs.iter().filter(|s| on(*e, **s)).count() == 1 && !taken.contains(e)
                })
                .collect();
            let (at, other) = ends.first().copied().unwrap_or(segs[0]);
            // Straight on from a free end; from anywhere else, square off the bus
            let d = at - other;
            let dir = if ends.is_empty() {
                if d.y == 0 {
                    P { x: 0, y: 1 }
                } else {
                    P { x: 1, y: 0 }
                }
            } else {
                P {
                    x: d.x.signum(),
                    y: d.y.signum(),
                }
            };
            let names = if prefixes.is_empty() {
                net.prefixes.clone()
            } else {
                prefixes
            };
            plan.bus_connectors.push((at, dir, t, members, names));
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_spell_out_their_members() {
        let (p, m) = group("DIG{D5 D6 D[9..11]}").unwrap();
        assert_eq!(p, "DIG");
        assert_eq!(m, vec!["D5", "D6", "D9", "D10", "D11"]);
        assert_eq!(group("{SCL, SDA}").unwrap().1, vec!["SCL", "SDA"]);
        assert!(group("~{CS}").is_none());
        assert!(group("D[0..7]").is_none());
        assert_eq!(member_net("ETH", "TRD0_N"), "ETH.TRD0_N");
        let (p, m) = group("{DDR4-PS.A[1..0] DDR4-PS.CKE}").unwrap();
        assert_eq!(p, "DDR4-PS");
        assert_eq!(m, vec!["A1", "A0", "CKE"]);
        let (p, m) = group("SWD_TRG{~{RESET}, SWDIO, SWDCLK}").unwrap();
        assert_eq!(p, "SWD_TRG");
        assert_eq!(m, vec!["~{RESET}", "SWDIO", "SWDCLK"]);
    }

    #[test]
    fn same_members_same_type() {
        let mut a = HashMap::new();
        a.insert(
            "I2C".to_string(),
            vec!["SCL".to_string(), "SDA".to_string()],
        );
        let m = vec!["SDA".to_string(), "SCL".to_string()];
        assert_eq!(harness_type("I2C_54{SDA, SCL}", &m, &a), "I2C");
        assert_eq!(harness_type("X{I2C}", &m, &a), "I2C");
        assert_eq!(harness_type("{A B}", &["A".into(), "B".into()], &a), "A_B");
    }
}
