//! Altium design rules -> a KiCad custom rules file (.kicad_dru).
//!
//! KiCad's Altium importer maps board-wide (All/All) and netclass-scoped rules onto board
//! settings and netclasses. Everything scoped by an Altium query (OnLayer, InNet, IsVia,
//! InDifferentialPairClass, AsMils(Hole), ...) is dropped. This translates those queries into
//! KiCad rule conditions and the rule values into KiCad constraints.
//!
//! A port of `pcbferry_dru.py`; both write the same file for the same board (the tests compare
//! them). [`custom_rules`] reads everything it needs from the PcbDoc.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

/// A query function or rule value KiCad has no counterpart for
#[derive(Debug, Clone)]
pub struct Untranslatable(pub String);

type R<T> = Result<T, Untranslatable>;

fn fail<T>(why: impl Into<String>) -> R<T> {
    Err(Untranslatable(why.into()))
}

/// A KiCad condition, or one of the two constants
#[derive(Debug, Clone, PartialEq)]
pub enum Cond {
    /// "All": matches everything
    True,
    /// Matches nothing (a layer, class or diff pair the board doesn't have)
    False,
    Expr(String),
}

fn e(s: impl Into<String>) -> Cond {
    Cond::Expr(s.into())
}

fn dedup(parts: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    parts
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

fn or(parts: impl IntoIterator<Item = Cond>) -> Cond {
    let mut out = Vec::new();
    for p in parts {
        match p {
            Cond::False => {}
            Cond::True => return Cond::True,
            Cond::Expr(s) => out.push(s),
        }
    }
    join(dedup(out), " || ", Cond::False)
}

fn and(parts: impl IntoIterator<Item = Cond>) -> Cond {
    let mut out = Vec::new();
    let mut nothing = false;
    for p in parts {
        match p {
            Cond::True => {}
            Cond::False => nothing = true,
            Cond::Expr(s) => out.push(s),
        }
    }
    if nothing {
        return Cond::False;
    }
    join(dedup(out), " && ", Cond::True)
}

fn join(parts: Vec<String>, op: &str, empty: Cond) -> Cond {
    match parts.len() {
        0 => empty,
        1 => Cond::Expr(parts.into_iter().next().unwrap()),
        _ => Cond::Expr(format!("({})", parts.join(op))),
    }
}

// Altium class kinds (Classes6 KIND)
const COMPONENT_CLASS: i64 = 1;
const LAYER_CLASS: i64 = 4;
const DIFF_PAIR_CLASS: i64 = 6;

const TYPES: &[(&str, &str)] = &[
    ("IsVia", "Via"),
    ("IsTrack", "Track"),
    ("IsArc", "Arc"),
    ("IsPad", "Pad"),
    ("IsRegion", "Zone"),
    ("IsPolygon", "Zone"),
    ("IsFill", "Zone"),
    ("IsText", "*Text"),
    ("IsComponent", "Footprint"),
];
const UNSUPPORTED: &[(&str, &str)] = &[
    ("TouchesRoom", "rooms are not imported"),
    ("WithinRoom", "rooms are not imported"),
    ("InRoom", "rooms are not imported"),
    ("InPolygonClass", "polygon classes are not imported"),
    ("InxSignalClass", "xSignals have no KiCad counterpart"),
    ("InxSignal", "xSignals have no KiCad counterpart"),
    (
        "HasFootprint",
        "no KiCad condition on a footprint's library name for its items",
    ),
];
const OTHER_FUNCTIONS: &[&str] = &[
    "All",
    "IsSMTPad",
    "IsThruPad",
    "InAnyNet",
    "InNet",
    "InNetClass",
    "OnLayer",
    "InLayerClass",
    "InComponent",
    "InComponentClass",
    "InDifferentialPairClass",
    "InDifferentialPair",
];

/// Altium query keywords ignore case (isVia, ISVIA)
fn canonical(name: &str) -> String {
    let lower = name.to_lowercase();
    TYPES
        .iter()
        .map(|t| t.0)
        .chain(UNSUPPORTED.iter().map(|t| t.0))
        .chain(OTHER_FUNCTIONS.iter().copied())
        .find(|n| n.to_lowercase() == lower)
        .map(str::to_string)
        .unwrap_or_else(|| name.to_string())
}

fn lookup<'a>(table: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    table.iter().find(|t| t.0 == key).map(|t| t.1)
}

fn cmp_op(op: &str) -> &'static str {
    match op {
        "=" => "==",
        "<>" => "!=",
        "<" => "<",
        ">" => ">",
        "<=" => "<=",
        _ => ">=",
    }
}

// ---------- which rules the importer itself carries ----------

/// Altium rule kinds the importer maps onto board settings for board-wide (All/All) rules
const DEFAULT_RULE_SETTINGS: &[&str] = &[
    "Clearance",
    "Width",
    "RoutingVias",
    "HoleSize",
    "HoleToHoleClearance",
    "BoardOutlineClearance",
    "SolderMaskExpansion",
    "PasteMaskExpansion",
    "MinimumAnnularRing",
    "SilkToSilkClearance",
    "MinimumSolderMaskSliver",
];
/// ... and onto a netclass (Default for All/All, or the one InNetClass('X') names)
const NETCLASS_RULE_FIELDS: &[&str] = &["Clearance", "Width", "RoutingVias", "DiffPairsRouting"];
/// Carried by each zone rather than the board
const ZONE_RULES: &[&str] = &["PlaneClearance", "PolygonConnect"];

/// Why an Altium rule kind has no KiCad counterpart in the import
fn rule_gap_reason(kind: &str) -> Option<&'static str> {
    const ALWAYS: &[&str] = &[
        "ShortCircuit",
        "UnRoutedNet",
        "NetAntennae",
        "UnpouredPolygon",
    ];
    const NONE: &[&str] = &[
        "FanoutControl",
        "RoutingTopology",
        "RoutingCorners",
        "RoutingPriority",
        "LayerPairs",
        "SupplyNets",
        "AssemblyTestpoint",
        "AssemblyTestPointUsage",
        "FabricationTestpoint",
        "FabricationTestPointUsage",
        "AcuteAngle",
        "RoomDefinition",
        "Height",
        "ComponentClearance",
        "StubLength",
    ];
    const CUSTOM: &[&str] = &[
        "Clearance",
        "Width",
        "RoutingVias",
        "DiffPairsRouting",
        "HoleToHoleClearance",
        "MinimumAnnularRing",
        "HoleSize",
        "MatchedLengths",
        "RoutingLayers",
        "SilkToBoardRegionClearance",
        "SilkToSolderMaskClearance",
        "ViasUnderSMD",
        "SilkscreenOverComponentPads",
        "PlaneConnect",
        "PlaneClearance",
        "SolderMaskExpansion",
    ];
    if ALWAYS.contains(&kind) {
        Some("KiCad DRC always checks this")
    } else if NONE.contains(&kind) {
        Some("no KiCad equivalent")
    } else if CUSTOM.contains(&kind) {
        Some("expressible as a KiCad custom rule")
    } else {
        None
    }
}

/// `InNetClass('X')`: X
fn netclass_scope(scope: &str) -> Option<&str> {
    let inner = scope.strip_prefix("InNetClass('")?.strip_suffix("')")?;
    (!inner.contains('\'')).then_some(inner)
}

/// Whether the importer maps this Altium rule onto board settings, a netclass or zones
pub fn importer_carries(rule: &Rule) -> bool {
    let kind = rule.kind.as_str();
    if ZONE_RULES.contains(&kind) {
        // The zone carries one clearance: the board-wide plane rule.
        return kind != "PlaneClearance" || (rule.scope1 == "All" && rule.scope2 == "All");
    }
    if rule.scope1 == "All" && rule.scope2 == "All" {
        return DEFAULT_RULE_SETTINGS.contains(&kind) || NETCLASS_RULE_FIELDS.contains(&kind);
    }
    NETCLASS_RULE_FIELDS.contains(&kind)
        && netclass_scope(&rule.scope1).is_some()
        && (rule.scope2 == "All" || rule.scope2 == rule.scope1)
}

/// Why a rule kind gets no custom rule, or None when it can have one
pub fn no_custom_reason(kind: &str) -> Option<&'static str> {
    match rule_gap_reason(kind) {
        Some("expressible as a KiCad custom rule") => None,
        Some(r) => Some(r),
        None => Some("unclassified"),
    }
}

fn q(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// Python's repr() of a str, for messages that match pcbferry_dru.py's
fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in s.chars() {
        match c {
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            '\r' => out += "\\r",
            '\t' => out += "\\t",
            c if c == quote => {
                out.push('\\');
                out.push(c)
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out += &format!("\\x{:02x}", c as u32),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// What the board offers the queries: layers, classes and differential pairs
pub struct Context {
    classes: Vec<Class>,
    diff_pairs: HashMap<String, DiffPair>,
    /// Altium layer name or id (normalised) -> KiCad canonical layer name
    layers: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct BoardLayers {
    /// (KiCad canonical name, the board's name for it), in board order
    pub layer_names: Vec<(String, String)>,
    pub copper_canonical: Vec<String>,
    pub altium_copper_ids: Vec<String>,
}

impl Context {
    pub fn new(board: &BoardLayers, classes: Vec<Class>, diff_pairs: Vec<DiffPair>) -> Self {
        let mut layers = HashMap::new();
        for (canonical, name) in &board.layer_names {
            layers.insert(norm(name), canonical.clone());
        }
        for (id, canonical) in board.altium_copper_ids.iter().zip(&board.copper_canonical) {
            layers.entry(norm(id)).or_insert_with(|| canonical.clone());
        }
        if let (Some(first), Some(last)) = (
            board.copper_canonical.first(),
            board.copper_canonical.last(),
        ) {
            layers
                .entry("toplayer".into())
                .or_insert_with(|| first.clone());
            layers
                .entry("bottomlayer".into())
                .or_insert_with(|| last.clone());
        }
        for (altium, canonical) in [
            ("topoverlay", "F.SilkS"),
            ("bottomoverlay", "B.SilkS"),
            ("topsolder", "F.Mask"),
            ("bottomsolder", "B.Mask"),
            ("toppaste", "F.Paste"),
            ("bottompaste", "B.Paste"),
        ] {
            layers
                .entry(altium.into())
                .or_insert_with(|| canonical.into());
        }
        Context {
            classes,
            diff_pairs: diff_pairs
                .into_iter()
                .map(|d| (d.name.clone(), d))
                .collect(),
            layers,
        }
    }

    /// The KiCad layer, or None for a layer the board doesn't have (Altium: no match)
    fn layer(&self, name: &str) -> Option<String> {
        self.layers.get(&norm(name)).cloned()
    }

    fn members(&self, name: &str, kind: i64) -> Vec<String> {
        self.classes
            .iter()
            .find(|c| c.name == name && c.kind == kind)
            .map(|c| c.members.clone())
            .unwrap_or_default() // a class the board doesn't have matches nothing
    }

    fn planes(&self) -> Vec<String> {
        let set: BTreeSet<&String> = self
            .layers
            .iter()
            .filter(|(n, _)| n.contains("plane"))
            .map(|(_, c)| c)
            .collect();
        set.into_iter().cloned().collect()
    }
}

fn norm(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

// ---------- Altium query language -> KiCad condition ----------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LPar,
    RPar,
    Comma,
    Str(String),
    Id(String),
    Cmp(String),
    Num(String),
}

fn tokenize(query: &str) -> R<Vec<Tok>> {
    let b: Vec<char> = query.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    loop {
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        if i >= b.len() {
            return Ok(out);
        }
        let c = b[i];
        let start = i;
        match c {
            '(' => {
                out.push(Tok::LPar);
                i += 1;
                continue;
            }
            ')' => {
                out.push(Tok::RPar);
                i += 1;
                continue;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
                continue;
            }
            '\'' => {
                let mut s = String::new();
                let mut j = i + 1;
                let mut closed = false;
                while j < b.len() {
                    if b[j] == '\'' {
                        if j + 1 < b.len() && b[j + 1] == '\'' {
                            s.push('\'');
                            j += 2;
                            continue;
                        }
                        closed = true;
                        break;
                    }
                    s.push(b[j]);
                    j += 1;
                }
                if closed {
                    out.push(Tok::Str(s));
                    i = j + 1;
                    continue;
                }
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == '_' || b[i] == '.') {
                    i += 1;
                }
                out.push(Tok::Id(b[start..i].iter().collect()));
                continue;
            }
            '<' | '>' | '=' => {
                let two: String = b[i..(i + 2).min(b.len())].iter().collect();
                let op = if ["<=", ">=", "<>"].contains(&two.as_str()) {
                    two
                } else {
                    c.to_string()
                };
                i += op.len();
                out.push(Tok::Cmp(op));
                continue;
            }
            c if c.is_ascii_digit() => {
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                if i + 1 < b.len() && b[i] == '.' && b[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < b.len() && b[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                out.push(Tok::Num(b[start..i].iter().collect()));
                continue;
            }
            _ => {}
        }
        // Anything else up to the next space
        let mut j = start;
        while j < b.len() && !b[j].is_whitespace() {
            j += 1;
        }
        let other: String = b[start..j].iter().collect();
        return fail(format!("operator {}", py_repr(&other)));
    }
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    query: &'a str,
    item: &'a str,
    ctx: &'a Context,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn peek_id(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Id(s)) if s.to_lowercase() == word)
    }

    fn bad<T>(&self) -> R<T> {
        fail(format!("cannot parse {}", py_repr(self.query)))
    }

    fn take(&mut self) -> Option<Tok> {
        let t = self.peek().cloned();
        self.pos += 1;
        t
    }

    fn expr(&mut self) -> R<Cond> {
        let mut left = self.term()?;
        while self.peek_id("or") {
            self.take();
            left = or([left, self.term()?]);
        }
        Ok(left)
    }

    fn term(&mut self) -> R<Cond> {
        let mut left = self.factor()?;
        while self.peek_id("and") {
            self.take();
            left = and([left, self.factor()?]);
        }
        Ok(left)
    }

    fn factor(&mut self) -> R<Cond> {
        if self.peek_id("not") {
            self.take();
            return Ok(match self.factor()? {
                Cond::True => Cond::False,
                Cond::False => Cond::True,
                Cond::Expr(s) => e(format!("!({s})")),
            });
        }
        match self.peek().cloned() {
            Some(Tok::LPar) => {
                self.take();
                let inner = self.expr()?;
                if self.take() != Some(Tok::RPar) {
                    return self.bad();
                }
                Ok(inner)
            }
            Some(Tok::Id(name)) => {
                if self.peek_id("asmils") || self.peek_id("asmm") {
                    return self.numeric();
                }
                self.take();
                let mut args = Vec::new();
                if self.peek() == Some(&Tok::LPar) {
                    self.take();
                    while self.peek() != Some(&Tok::RPar) {
                        match self.take() {
                            Some(Tok::Str(s)) => args.push(s),
                            _ => return self.bad(),
                        }
                        if self.peek() == Some(&Tok::Comma) {
                            self.take();
                        }
                    }
                    self.take();
                }
                self.call(&name, &args)
            }
            _ => self.bad(),
        }
    }

    /// AsMils(Hole) > 50 -> a comparison on the item's KiCad property
    fn numeric(&mut self) -> R<Cond> {
        let unit = if self.peek_id("asmils") { "mil" } else { "mm" };
        self.take();
        if self.take() != Some(Tok::LPar) {
            return self.bad();
        }
        let Some(Tok::Id(prop)) = self.take() else {
            return self.bad();
        };
        if self.take() != Some(Tok::RPar) {
            return self.bad();
        }
        let Some(Tok::Cmp(op)) = self.take() else {
            return self.bad();
        };
        let Some(Tok::Num(num)) = self.take() else {
            return self.bad();
        };
        if prop.to_lowercase() != "hole" {
            return fail(format!("numeric property {prop}: no KiCad counterpart yet"));
        }
        let item = self.item;
        let op = cmp_op(&op);
        Ok(or([("Via", "Hole"), ("Pad", "Hole_Size_X")].iter().map(
            |(t, p)| {
                e(format!(
                    "({item}.Type == {} && {item}.{p} {op} {num}{unit})",
                    q(t)
                ))
            },
        )))
    }

    fn call(&self, name: &str, args: &[String]) -> R<Cond> {
        let name = canonical(name);
        let item = self.item;
        let ctx = self.ctx;
        if let Some(why) = lookup(UNSUPPORTED, &name) {
            return fail(format!("{name}: {why}"));
        }
        if name == "All" {
            return Ok(Cond::True);
        }
        if let Some(t) = lookup(TYPES, &name) {
            return Ok(e(format!("{item}.Type == {}", q(t))));
        }
        let arg = args.first();
        Ok(match (name.as_str(), arg) {
            ("IsSMTPad", _) => e(format!(
                "({item}.Type == 'Pad' && {item}.Pad_Type == 'SMD')"
            )),
            ("IsThruPad", _) => e(format!(
                "({item}.Type == 'Pad' && {item}.Pad_Type == 'Through-hole')"
            )),
            ("InAnyNet", _) => e(format!("{item}.NetName != ''")),
            ("InNet", Some(a)) => e(format!("{item}.NetName == {}", q(a))),
            ("InNetClass", Some(a)) => e(format!("{item}.hasNetclass({})", q(a))),
            ("OnLayer", Some(a)) => {
                // An Altium object's own layer: vias and through-hole pads sit on MultiLayer.
                let multi = format!(
                    "({item}.Type == 'Via' || ({item}.Type == 'Pad' && {item}.Pad_Type == 'Through-hole'))"
                );
                if norm(a) == "multilayer" {
                    e(multi)
                } else {
                    match ctx.layer(a) {
                        Some(l) => and([
                            e(format!("{item}.existsOnLayer({})", q(&l))),
                            e(format!("!{multi}")),
                        ]),
                        None => Cond::False,
                    }
                }
            }
            ("InLayerClass", Some(a)) => or(ctx.members(a, LAYER_CLASS).iter().map(|m| match ctx
                .layer(m)
            {
                Some(l) => e(format!("{item}.existsOnLayer({})", q(&l))),
                None => Cond::False,
            })),
            ("InComponent" | "InComponentClass", Some(a)) => {
                let refs = if name == "InComponent" {
                    vec![a.clone()]
                } else {
                    ctx.members(a, COMPONENT_CLASS)
                };
                or(refs
                    .iter()
                    .map(|r| e(format!("{item}.memberOfFootprint({})", q(r)))))
            }
            ("InDifferentialPairClass", Some(a)) => {
                if a == "All Differential Pairs" {
                    e(format!("{item}.inDiffPair('*')"))
                } else {
                    pair_nets(item, &ctx.members(a, DIFF_PAIR_CLASS), ctx)
                }
            }
            ("InDifferentialPair", Some(_)) => pair_nets(item, args, ctx),
            _ => return fail(format!("{name}: no KiCad counterpart yet")),
        })
    }
}

fn pair_nets(item: &str, pairs: &[String], ctx: &Context) -> Cond {
    let mut nets = Vec::new();
    for name in pairs {
        // a pair the board doesn't have matches nothing
        if let Some(p) = ctx.diff_pairs.get(name) {
            nets.push(p.positive.clone());
            nets.push(p.negative.clone());
        }
    }
    or(nets
        .iter()
        .map(|n| e(format!("{item}.NetName == {}", q(n)))))
}

/// An Altium scope query -> a KiCad condition on `item` ('A' or 'B'), or True
pub fn translate_query(query: &str, item: &str, ctx: &Context) -> R<Cond> {
    let mut p = Parser {
        toks: tokenize(query)?,
        pos: 0,
        query,
        item,
        ctx,
    };
    let result = p.expr()?;
    if p.pos != p.toks.len() {
        return p.bad();
    }
    Ok(result)
}

// ---------- rule values -> constraints ----------

fn mm(value: f64) -> String {
    let v = if 0.0 > value { 0.0 } else { value };
    format!("{v:.4}mm").replace(".0000mm", "mm")
}

/// Altium stores lengths in 1/10000 mil; converted to mm they land a few nanometres off the
/// value the designer typed, which KiCad's DRC (no tolerance) then flags. Allow 0.1 um.
const TOLERANCE_MM: f64 = 0.0001;

fn limits(
    name: &str,
    v: &Props<f64>,
    lo: Option<&str>,
    opt: Option<&str>,
    hi: Option<&str>,
) -> R<String> {
    let mut parts = Vec::new();
    for (kind, field) in [("min", lo), ("opt", opt), ("max", hi)] {
        let Some(f) = field else { continue };
        let Some(&x) = v.get(f) else { continue };
        let x = match kind {
            "min" => x - TOLERANCE_MM,
            "max" => x + TOLERANCE_MM,
            _ => x,
        };
        parts.push(format!("({kind} {})", mm(x)));
    }
    if parts.is_empty() {
        return fail(format!("no value for {name}"));
    }
    Ok(format!("(constraint {name} {})", parts.join(" ")))
}

/// Altium clearance object kinds -> KiCad item tests. Fills, polygons and regions all come in
/// as zones; holes are left to hole_clearance.
fn obj(kind: &str, i: &str) -> Option<String> {
    Some(match kind {
        "Arc" => format!("{i}.Type == 'Arc'"),
        "Track" => format!("{i}.Type == 'Track'"),
        "SMDPad" => format!("({i}.Type == 'Pad' && {i}.Pad_Type == 'SMD')"),
        "THPad" => format!("({i}.Type == 'Pad' && {i}.Pad_Type == 'Through-hole')"),
        "Via" => format!("{i}.Type == 'Via'"),
        "Fill" | "Poly" | "Region" => format!("{i}.Type == 'Zone'"),
        "Text" => format!("{i}.Type == '*Text'"),
        _ => return None,
    })
}

/// OBJECTCLEARANCES are in 1/10000 mil
const ALTIUM_UNIT_MM: f64 = 0.0254 / 10000.0;

/// Altium's per object-type pair clearances, in mm. Fills, polygons and regions map to one
/// KiCad kind (zone), so they keep the smallest of their gaps.
pub fn clearance_matrix(rule: &Rule) -> BTreeMap<(String, String), f64> {
    let mut out: BTreeMap<(String, String), f64> = BTreeMap::new();
    let text = rule
        .flags
        .get("OBJECTCLEARANCES")
        .map(String::as_str)
        .unwrap_or("");
    for entry in text.split(';') {
        let entry = entry.trim();
        let Some((pair, gap)) = entry.rsplit_once(':') else {
            continue;
        };
        if gap.is_empty() || !gap.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Some(rest) = pair.strip_prefix("ClearanceObj_") else {
            continue;
        };
        let Some((a, b)) = rest.split_once("-ClearanceObj_") else {
            continue;
        };
        let word = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_');
        if !word(a) || !word(b) {
            continue;
        }
        if a == "Hole" || b == "Hole" || obj(a, "A").is_none() || obj(b, "A").is_none() {
            continue;
        }
        let zone = |x: &str| {
            if matches!(x, "Fill" | "Poly" | "Region") {
                "Region".to_string()
            } else {
                x.to_string()
            }
        };
        let (mut a, mut b) = (zone(a), zone(b));
        if b < a {
            std::mem::swap(&mut a, &mut b);
        }
        let Ok(n) = gap.parse::<f64>() else { continue };
        let gap = n * ALTIUM_UNIT_MM;
        let e = out.entry((a, b)).or_insert(gap);
        *e = e.min(gap);
    }
    out
}

const BINARY: &[&str] = &[
    "Clearance",
    "HoleToHoleClearance",
    "SilkToSilkClearance",
    "SilkToSolderMaskClearance",
    "SilkscreenOverComponentPads",
];

/// (extra condition, constraint, layer clause)
type Constraint = (Option<Cond>, String, Option<String>);

fn constraints(rule: &Rule, ctx: &Context) -> R<Vec<Constraint>> {
    let v = &rule.values;
    let flags = &rule.flags;
    let flag = |k: &str| flags.get(k).map(String::as_str);
    let lim = |name, lo, opt, hi| limits(name, v, lo, opt, hi);
    Ok(match rule.kind.as_str() {
        "Clearance" => {
            if flag("NETSCOPE") == Some("SameNet") {
                return fail("same-net clearance");
            }
            let mut out = vec![(None, lim("clearance", Some("GAP"), None, None)?, None)];
            let generic = v.get("GAP").copied().unwrap_or_default();
            // Per object-type pair gaps; pairs not listed use the generic gap. Later rules win.
            for ((a, b), gap) in clearance_matrix(rule) {
                if (gap - generic).abs() < 1e-6 {
                    continue;
                }
                let (oa, ob) = (|i| obj(&a, i).unwrap(), |i| obj(&b, i).unwrap());
                let pair = or([and([e(oa("A")), e(ob("B"))]), and([e(ob("A")), e(oa("B"))])]);
                out.push((
                    Some(pair),
                    format!("(constraint clearance (min {}))", mm(gap - TOLERANCE_MM)),
                    None,
                ));
            }
            out
        }
        "Width" => vec![(
            None,
            lim(
                "track_width",
                Some("MINLIMIT"),
                Some("PREFEREDWIDTH"),
                Some("MAXLIMIT"),
            )?,
            None,
        )],
        "RoutingVias" => vec![
            (
                None,
                lim(
                    "via_diameter",
                    Some("MINWIDTH"),
                    Some("WIDTH"),
                    Some("MAXWIDTH"),
                )?,
                None,
            ),
            (
                Some(e("A.Type == 'Via'")),
                lim(
                    "hole_size",
                    Some("MINHOLEWIDTH"),
                    Some("HOLEWIDTH"),
                    Some("MAXHOLEWIDTH"),
                )?,
                None,
            ),
        ],
        "DiffPairsRouting" => {
            // Widths and gaps are per layer (TOPLAYER_*, MIDLAYERn_*, BOTTOMLAYER_*); the
            // MIN/MAXLIMIT and MOSTFREQGAP fields are the fallback gap.
            let mut out = vec![(
                None,
                lim(
                    "diff_pair_gap",
                    Some("MINLIMIT"),
                    Some("MOSTFREQGAP"),
                    Some("MAXLIMIT"),
                )?,
                None,
            )];
            let prefixes: BTreeSet<String> = v
                .keys()
                .filter(|k| k.ends_with("_MINWIDTH"))
                .map(|k| k.rsplit_once('_').unwrap().0.to_string())
                .collect();
            for prefix in prefixes {
                let altium = match prefix.as_str() {
                    "TOPLAYER" => "TopLayer".to_string(),
                    "BOTTOMLAYER" => "BottomLayer".to_string(),
                    p => p.replace("MIDLAYER", "MidLayer"),
                };
                let Some(layer) = ctx.layer(&altium) else {
                    continue;
                };
                let f = |s: &str| format!("{prefix}_{s}");
                out.push((
                    Some(e("A.inDiffPair('*')")),
                    limits(
                        "track_width",
                        v,
                        Some(&f("MINWIDTH")),
                        Some(&f("PREFWIDTH")),
                        Some(&f("MAXWIDTH")),
                    )?,
                    Some(layer.clone()),
                ));
                if v.get(&f("MINGAP")).is_some() {
                    out.push((
                        None,
                        limits(
                            "diff_pair_gap",
                            v,
                            Some(&f("MINGAP")),
                            Some(&f("PREFGAP")),
                            Some(&f("MAXGAP")),
                        )?,
                        Some(layer),
                    ));
                }
            }
            if v.get("MAXUNCOUPLEDLENGTH").is_some_and(|x| *x != 0.0) {
                out.push((
                    None,
                    lim(
                        "diff_pair_uncoupled",
                        None,
                        None,
                        Some("MAXUNCOUPLEDLENGTH"),
                    )?,
                    None,
                ));
            }
            out
        }
        "HoleToHoleClearance" => vec![(None, lim("hole_to_hole", Some("GAP"), None, None)?, None)],
        "MinimumAnnularRing" => {
            vec![(
                None,
                lim("annular_width", Some("MINIMUMRING"), None, None)?,
                None,
            )]
        }
        "HoleSize" => {
            if !rule.absolute {
                return fail("hole limits given as percentages");
            }
            vec![(
                None,
                lim("hole_size", Some("MINLIMIT"), None, Some("MAXLIMIT"))?,
                None,
            )]
        }
        "MatchedLengths" => {
            let within = if flag("CHECKNETSINDIFFPAIR") == Some("TRUE")
                && flag("CHECKOTHERS") != Some("TRUE")
                && flag("CHECKXSIGNALS") != Some("TRUE")
            {
                " (within_diff_pairs)"
            } else {
                ""
            };
            let c = lim("skew", None, None, Some("TOLERANCE"))?;
            vec![(None, format!("{}{within})", &c[..c.len() - 1]), None)]
        }
        "SilkToSilkClearance" => {
            vec![(
                None,
                lim("silk_clearance", Some("SILKTOSILKCLEARANCE"), None, None)?,
                None,
            )]
        }
        "SilkToSolderMaskClearance" | "SilkscreenOverComponentPads" => vec![(
            Some(e("B.Type == 'Pad'")),
            lim("silk_clearance", Some("MINSILKSCREENTOMASKGAP"), None, None)?,
            None,
        )],
        "RoutingLayers" => {
            let mut out = Vec::new();
            for (key, allowed) in flags.iter() {
                let Some(name) = key.strip_suffix("_V5") else {
                    continue;
                };
                if allowed != "FALSE" {
                    continue;
                }
                // a layer the board doesn't have
                let Some(layer) = ctx.layer(name) else {
                    continue;
                };
                out.push((
                    None,
                    "(constraint disallow track via)".to_string(),
                    Some(layer),
                ));
            }
            if out.is_empty() {
                return fail("every layer is allowed: nothing to restrict");
            }
            out
        }
        "SolderMaskExpansion" => {
            // A large negative expansion is how Altium designs tent vias by rule.
            let Some(x) = v.get("EXPANSION") else {
                return fail("the rule record carries no expansion");
            };
            vec![(
                None,
                format!("(constraint solder_mask_expansion (opt {x:.4}mm))"),
                None,
            )]
        }
        "PlaneClearance" => {
            // Altium clears a plane around the hole of an unconnected pad or via (planes carry
            // no pad lands); KiCad keeps the land, so the gap applies to the land and the hole.
            let on_plane = on_plane(ctx)?;
            vec![
                (
                    Some(on_plane.clone()),
                    lim("clearance", Some("CLEARANCE"), None, None)?,
                    None,
                ),
                (
                    Some(on_plane),
                    lim("hole_clearance", Some("CLEARANCE"), None, None)?,
                    None,
                ),
            ]
        }
        "PlaneConnect" => {
            let on_plane = on_plane(ctx)?;
            let mut out = Vec::new();
            // Older records hold one setting; newer ones one for pads and one for vias.
            for (prefix, item_type) in [("", None), ("PAD.", Some("Pad")), ("VIA.", Some("Via"))] {
                let style = match flag(&format!("{prefix}PLANECONNECTSTYLE")) {
                    Some("Relief") => "thermal_reliefs",
                    Some("Direct") => "solid",
                    Some("NoConnect") => "none",
                    _ => continue,
                };
                let cond = match item_type {
                    None => on_plane.clone(),
                    Some(t) => and([e(format!("A.Type == {}", q(t))), on_plane.clone()]),
                };
                out.push((
                    Some(cond.clone()),
                    format!("(constraint zone_connection {style})"),
                    None,
                ));
                if style == "thermal_reliefs" {
                    let gap = format!("{prefix}RELIEFAIRGAP");
                    if v.get(&gap).is_some() {
                        out.push((
                            Some(cond.clone()),
                            limits("thermal_relief_gap", v, Some(&gap), None, None)?,
                            None,
                        ));
                    }
                    let width = format!("{prefix}RELIEFCONDUCTORWIDTH");
                    if v.get(&width).is_some() {
                        out.push((
                            Some(cond.clone()),
                            limits("thermal_spoke_width", v, None, Some(&width), None)?,
                            None,
                        ));
                    }
                }
            }
            if out.is_empty() {
                return fail("unknown plane connect style");
            }
            out
        }
        "ViasUnderSMD" => return fail("KiCad has no via-in-SMD-pad constraint"),
        "SilkToBoardRegionClearance" => return fail("the rule record carries no clearance value"),
        k => return fail(format!("rule kind {k}")),
    })
}

fn on_plane(ctx: &Context) -> R<Cond> {
    let planes = ctx.planes();
    if planes.is_empty() {
        return fail("the board has no plane layers");
    }
    Ok(and([
        e("B.Type == 'Zone'"),
        or(planes
            .iter()
            .map(|p| e(format!("B.existsOnLayer({})", q(p))))),
    ]))
}

fn condition(rule: &Rule, ctx: &Context) -> R<Cond> {
    let s1q = if rule.scope1.is_empty() {
        "All"
    } else {
        &rule.scope1
    };
    let s2q = if rule.scope2.is_empty() {
        "All"
    } else {
        &rule.scope2
    };
    let s1 = translate_query(s1q, "A", ctx)?;
    if !BINARY.contains(&rule.kind.as_str()) {
        return Ok(s1);
    }
    // Altium applies a binary rule when one object matches each scope, in either order.
    let s2 = translate_query(s2q, "B", ctx)?;
    let s1b = translate_query(s1q, "B", ctx)?;
    let s2a = translate_query(s2q, "A", ctx)?;
    Ok(or([and([s1, s2]), and([s2a, s1b])]))
}

// ---------- whole rule set ----------

/// What happened to one enabled Altium rule
#[derive(Debug, Clone)]
pub struct Outcome {
    pub kind: String,
    pub name: String,
    pub scope1: String,
    pub scope2: String,
    pub carried: bool,
    /// translated, imported, no_equivalent, matches_nothing, no_op or untranslatable
    pub status: &'static str,
    pub reason: Option<String>,
}

/// (.kicad_dru text, one entry per enabled Altium rule).
///
/// For any rule kind that needs custom rules, every enabled rule of that kind is written,
/// highest Altium precedence last: KiCad applies the last matching custom rule, and custom
/// rules override board settings and netclasses.
pub fn translate(rules: &[Rule], ctx: &Context) -> (String, Vec<Outcome>) {
    let mut enabled: Vec<&Rule> = rules.iter().filter(|r| r.enabled).collect();
    let mut results = Vec::new();
    let mut blocks = Vec::new();
    let mut needs_custom: BTreeSet<&str> = enabled
        .iter()
        .filter(|r| !importer_carries(r))
        .map(|r| r.kind.as_str())
        .collect();
    // The importer carries only the generic gap of a clearance rule; a per object-type matrix
    // needs custom rules even for the board-wide rule.
    if enabled.iter().any(|r| {
        r.kind == "Clearance"
            && clearance_matrix(r)
                .values()
                .any(|g| (g - r.values.get("GAP").copied().unwrap_or(*g)).abs() > 1e-6)
    }) {
        needs_custom.insert("Clearance");
    }
    enabled.sort_by_key(|r| std::cmp::Reverse(r.priority));
    for rule in enabled {
        let carried = importer_carries(rule);
        let mut entry = Outcome {
            kind: rule.kind.clone(),
            name: rule.name.clone(),
            scope1: rule.scope1.clone(),
            scope2: rule.scope2.clone(),
            carried,
            status: "imported",
            reason: None,
        };
        if !needs_custom.contains(rule.kind.as_str()) {
            results.push(entry);
            continue;
        }
        if let (Some(reason), false) = (no_custom_reason(&rule.kind), carried) {
            entry.status = "no_equivalent";
            entry.reason = Some(reason.into());
            results.push(entry);
            continue;
        }
        let written = condition(rule, ctx).and_then(|cond| {
            if cond == Cond::False {
                return Ok(None);
            }
            let mut out = Vec::new();
            for (extra, constraint, layer) in constraints(rule, ctx)? {
                let full = and([cond.clone(), extra.unwrap_or(Cond::True)]);
                let mut lines = vec![format!("(rule \"{}\"", rule.name.replace('"', "'"))];
                if let Some(l) = layer {
                    lines.push(format!("  (layer \"{l}\")"));
                }
                lines.push(format!("  {constraint}"));
                if let Cond::Expr(c) = full {
                    lines.push(format!("  (condition \"{c}\")"));
                }
                out.push(lines.join("\n") + ")");
            }
            Ok(Some(out))
        });
        match written {
            Ok(None) => {
                entry.status = "matches_nothing";
                entry.reason = Some("its scope matches nothing on this board".into());
            }
            Ok(Some(b)) => {
                blocks.extend(b);
                entry.status = "translated";
            }
            Err(Untranslatable(why)) => {
                entry.status = if why.starts_with("every layer is allowed")
                    || why.starts_with("the board has no plane layers")
                {
                    "no_op"
                } else if carried {
                    "imported"
                } else {
                    "untranslatable"
                };
                entry.reason = Some(why);
            }
        }
        results.push(entry);
    }
    // The importer gives vias a land the size of their hole on Altium planes (Altium draws
    // none there and clears the plane from the hole); Altium checks no annular ring there.
    for plane in ctx.planes() {
        blocks.push(format!(
            "(rule \"Altium plane {plane}: vias have no land\"\n  (layer \"{plane}\")\n  \
             (constraint annular_width (min 0mm))\n  (condition \"A.Type == 'Via'\"))"
        ));
    }
    let header = "(version 1)\n# Translated from the Altium design rules. Lower Altium precedence first:\n# KiCad applies the last matching rule.\n";
    (format!("{header}{}\n", blocks.join("\n\n")), results)
}

// ---------- straight from the PcbDoc ----------

/// A map that keeps insertion order, as a Python dict does (rule flags are walked in order)
#[derive(Debug, Clone)]
pub struct Props<V>(Vec<(String, V)>);

impl<V> Default for Props<V> {
    fn default() -> Self {
        Props(Vec::new())
    }
}

impl<V> Props<V> {
    pub fn get(&self, k: &str) -> Option<&V> {
        self.0.iter().find(|(key, _)| key == k).map(|(_, v)| v)
    }
    pub fn insert(&mut self, k: String, v: V) {
        match self.0.iter_mut().find(|(key, _)| *key == k) {
            Some(slot) => slot.1 = v,
            None => self.0.push((k, v)),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.0.iter().map(|(k, v)| (k, v))
    }
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(k, _)| k)
    }
}

/// An Altium length ("5.9055mil", "0.1mm", "12") in mm
fn length_mm(text: &str) -> Option<f64> {
    let t = text.trim();
    let (num, unit) = if let Some(n) = t.strip_suffix("mil") {
        (n.trim_end(), "mil")
    } else if let Some(n) = t.strip_suffix("mm") {
        (n.trim_end(), "mm")
    } else {
        (t, "mil")
    };
    let digits = num.strip_prefix('-').unwrap_or(num);
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let value: f64 = num.parse().ok()?;
    Some(if unit == "mm" { value } else { value * 0.0254 })
}

/// One Rules6 record
#[derive(Debug, Clone)]
pub struct Rule {
    pub kind: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i64,
    pub scope1: String,
    pub scope2: String,
    /// Fields that read as lengths, in mm
    pub values: Props<f64>,
    pub absolute: bool,
    /// Every other field
    pub flags: Props<String>,
}

impl Rule {
    pub fn from_props(p: &Props<String>) -> Rule {
        let get = |k: &str| p.get(k).map(String::as_str);
        let mut values = Props::default();
        let mut flags = Props::default();
        for (k, v) in p.iter() {
            match length_mm(v) {
                Some(x) => {
                    if !["PRIORITY", "UNIONINDEX", "NETSCOPE", "LAYERKIND"].contains(&k.as_str()) {
                        values.insert(k.clone(), x);
                    }
                }
                None => {
                    if ![
                        "RULEKIND",
                        "NAME",
                        "SCOPE1EXPRESSION",
                        "SCOPE2EXPRESSION",
                        "COMMENT",
                    ]
                    .contains(&k.as_str())
                    {
                        flags.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        Rule {
            kind: get("RULEKIND").unwrap_or("").into(),
            name: get("NAME").unwrap_or("").into(),
            enabled: get("ENABLED").unwrap_or("TRUE").to_uppercase() == "TRUE",
            priority: get("PRIORITY")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(1),
            scope1: get("SCOPE1EXPRESSION").unwrap_or("").trim().into(),
            scope2: get("SCOPE2EXPRESSION").unwrap_or("").trim().into(),
            values,
            absolute: get("ABSOLUTEVALUES").unwrap_or("TRUE").to_uppercase() == "TRUE",
            flags,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Class {
    pub name: String,
    pub kind: i64,
    pub members: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiffPair {
    pub name: String,
    pub positive: String,
    pub negative: String,
}

/// `|KEY=VALUE|KEY=VALUE` text, as the PcbDoc's text records hold it (Windows-1252, read as
/// Latin-1)
pub fn props(record: &[u8]) -> Props<String> {
    let text: String = record.iter().map(|&b| b as char).collect();
    let text = text.trim_matches(|c| c == '|' || c == '\0');
    let mut out = Props::default();
    for item in text.split('|') {
        if let Some((k, v)) = item.split_once('=') {
            out.insert(k.to_string(), v.to_string());
        }
    }
    out
}

/// Length-prefixed text records of a PcbDoc stream (`header` bytes before each length)
pub fn records<F: std::io::Read + std::io::Seek>(
    ole: &mut cfb::CompoundFile<F>,
    stream: &str,
    header: usize,
) -> std::io::Result<Vec<Props<String>>> {
    use std::io::Read;
    let path = format!("/{stream}/Data");
    if !ole.is_stream(&path) {
        return Ok(Vec::new());
    }
    let mut data = Vec::new();
    ole.open_stream(&path)?.read_to_end(&mut data)?;
    let mut i = 0;
    let mut out = Vec::new();
    while i + header + 4 <= data.len() {
        i += header;
        let n = u32::from_le_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        i += 4;
        let end = (i + n).min(data.len());
        out.push(props(&data[i..end]));
        i = i.saturating_add(n);
    }
    Ok(out)
}

/// Altium's default name of a copper layer id (V7 ids or the old 1..54 numbering)
fn altium_copper_name(id: i64) -> Option<String> {
    if id >= 0x0100_0000 {
        let (group, low) = (id >> 16, id & 0xFFFF);
        return match group {
            0x0100 => Some(match low {
                1 => "Top Layer".into(),
                0xFFFF => "Bottom Layer".into(),
                l => format!("Mid Layer {}", l - 1),
            }),
            0x0101 => Some(format!("Internal Plane {low}")),
            _ => None,
        };
    }
    match id {
        1 => Some("Top Layer".into()),
        32 => Some("Bottom Layer".into()),
        2..=31 => Some(format!("Mid Layer {}", id - 1)),
        39..=54 => Some(format!("Internal Plane {}", id - 38)),
        _ => None,
    }
}

/// What the rules need from a PcbDoc
#[derive(Debug, Default)]
pub struct PcbDocRules {
    pub rules: Vec<Rule>,
    pub classes: Vec<Class>,
    pub diff_pairs: Vec<DiffPair>,
    /// Altium default names of the copper layers, in stack order
    pub copper: Vec<String>,
    /// The board's names for them
    pub copper_names: Vec<String>,
    /// Mechanical k -> name
    pub mechanical: Props<String>,
}

fn int(s: Option<&String>) -> i64 {
    s.and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

pub fn read_pcbdoc(path: &Path) -> std::io::Result<PcbDocRules> {
    let mut ole = cfb::open(path)?;
    let board = records(&mut ole, "Board6", 0)?
        .into_iter()
        .next()
        .unwrap_or_default();
    let rules = records(&mut ole, "Rules6", 2)?
        .iter()
        .map(Rule::from_props)
        .collect();
    let classes = records(&mut ole, "Classes6", 0)?
        .iter()
        .map(|p| {
            let mut members: Vec<(u64, String)> = p
                .iter()
                .filter_map(|(k, v)| {
                    let n = k.strip_prefix('M')?;
                    (!n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
                        .then(|| (n.parse().unwrap_or(u64::MAX), v.clone()))
                })
                .collect();
            members.sort_by_key(|m| m.0);
            Class {
                name: p.get("NAME").cloned().unwrap_or_default(),
                kind: int(p.get("KIND")),
                members: members.into_iter().map(|m| m.1).collect(),
            }
        })
        .collect();
    let diff_pairs = records(&mut ole, "DifferentialPairs6", 0)?
        .iter()
        .map(|p| DiffPair {
            name: p.get("NAME").cloned().unwrap_or_default(),
            positive: p.get("POSITIVENETNAME").cloned().unwrap_or_default(),
            negative: p.get("NEGATIVENETNAME").cloned().unwrap_or_default(),
        })
        .collect();
    let mut doc = PcbDocRules {
        rules,
        classes,
        diff_pairs,
        ..Default::default()
    };
    // Copper order: the V9 stack lists every layer in board order; older files chain LAYERn.
    let mut n = 1;
    while let Some(id) = board.get(&format!("V9_STACK_LAYER{n}_LAYERID")) {
        let id: i64 = id.trim().parse().unwrap_or(0);
        let name_key = format!("V9_STACK_LAYER{n}_NAME");
        if let Some(name) = altium_copper_name(id) {
            doc.copper_names.push(
                board
                    .get(&name_key)
                    .cloned()
                    .unwrap_or_else(|| name.clone()),
            );
            doc.copper.push(name);
        } else if id >> 16 == 0x0102 {
            let k = id & 0xFFFF;
            let name = board
                .get(&name_key)
                .cloned()
                .unwrap_or_else(|| format!("Mechanical {k}"));
            doc.mechanical.insert(k.to_string(), name);
        }
        n += 1;
    }
    if doc.copper.is_empty() {
        let mut layer = 1;
        let mut seen = BTreeSet::new();
        while layer != 0 && seen.insert(layer) {
            if let Some(name) = altium_copper_name(layer) {
                let n = board.get(&format!("LAYER{layer}NAME")).cloned();
                doc.copper_names.push(n.unwrap_or_else(|| name.clone()));
                doc.copper.push(name);
            }
            layer = int(board.get(&format!("LAYER{layer}NEXT")));
        }
        for k in 1..=16 {
            if let Some(name) = board.get(&format!("LAYER{}NAME", 56 + k)) {
                doc.mechanical.insert(k.to_string(), name.clone());
            }
        }
    }
    Ok(doc)
}

/// The KiCad layer names the importer gives a PcbDoc's layers: copper in stack order (F.Cu,
/// In1.Cu, ..., B.Cu) under the Altium names, Mechanical k as User.k
pub fn pcbdoc_board_layers(doc: &PcbDocRules) -> BoardLayers {
    let count = doc.copper.len();
    let mut canonical = vec!["F.Cu".to_string()];
    canonical.extend((1..count.saturating_sub(1)).map(|i| format!("In{i}.Cu")));
    if count > 1 {
        canonical.push("B.Cu".into());
    }
    let mut names: Props<String> = Props::default();
    for (c, n) in canonical.iter().zip(&doc.copper_names) {
        names.insert(c.clone(), n.clone());
    }
    for (k, name) in doc.mechanical.iter() {
        if k.parse::<i64>().is_ok_and(|k| k <= 45) {
            names.insert(format!("User.{k}"), name.clone());
        }
    }
    BoardLayers {
        layer_names: names.0,
        copper_canonical: canonical,
        altium_copper_ids: doc.copper.clone(),
    }
}

/// The .kicad_dru for a PcbDoc's import, and what happened to each enabled Altium rule. Needs
/// only the PcbDoc, so it can be written before the import fills zones.
pub fn custom_rules(pcbdoc: &Path) -> std::io::Result<(String, Vec<Outcome>)> {
    let doc = read_pcbdoc(pcbdoc)?;
    let ctx = Context::new(
        &pcbdoc_board_layers(&doc),
        doc.classes.clone(),
        doc.diff_pairs.clone(),
    );
    Ok(translate(&doc.rules, &ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Context {
        let board = BoardLayers {
            layer_names: vec![
                ("F.Cu".into(), "Top Layer".into()),
                ("In1.Cu".into(), "GND Plane".into()),
                ("B.Cu".into(), "Bottom Layer".into()),
            ],
            copper_canonical: vec!["F.Cu".into(), "In1.Cu".into(), "B.Cu".into()],
            altium_copper_ids: vec![
                "Top Layer".into(),
                "Internal Plane 1".into(),
                "Bottom Layer".into(),
            ],
        };
        let classes = vec![Class {
            name: "Power".into(),
            kind: COMPONENT_CLASS,
            members: vec!["U1".into(), "U2".into()],
        }];
        Context::new(&board, classes, vec![])
    }

    #[test]
    fn queries() {
        let c = ctx();
        let t = |s| translate_query(s, "A", &c).unwrap();
        assert_eq!(t("All"), Cond::True);
        assert_eq!(t("isvia"), e("A.Type == 'Via'"));
        assert_eq!(
            t("InNet('GND') or InNet('VCC')"),
            e("(A.NetName == 'GND' || A.NetName == 'VCC')")
        );
        assert_eq!(t("OnLayer('Nope')"), Cond::False);
        assert_eq!(t("Not All"), Cond::False);
        assert_eq!(
            t("InComponentClass('Power')"),
            e("(A.memberOfFootprint('U1') || A.memberOfFootprint('U2'))")
        );
        assert_eq!(t("AsMils(Hole) > 50"), e("((A.Type == 'Via' && A.Hole > 50mil) || (A.Type == 'Pad' && A.Hole_Size_X > 50mil))"));
        assert_eq!(t("InNet('it''s')"), e("A.NetName == 'it\\'s'"));
        assert!(translate_query("InNet('a') && x", "A", &c)
            .unwrap_err()
            .0
            .starts_with("operator '&&'"));
        assert_eq!(
            translate_query("InRoom('R')", "A", &c).unwrap_err().0,
            "InRoom: rooms are not imported"
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(mm(0.1524), "0.1524mm");
        assert_eq!(mm(1.0), "1mm");
        assert_eq!(mm(-3.0), "0mm");
        assert_eq!(length_mm("10mil"), Some(10.0 * 0.0254));
        assert_eq!(length_mm("0.2mm"), Some(0.2));
        assert_eq!(length_mm("TRUE"), None);
    }
}
