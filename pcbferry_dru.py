"""Altium design rules -> a KiCad custom rules file (.kicad_dru).

KiCad's Altium importer maps board-wide (All/All) and netclass-scoped rules onto board
settings and netclasses. Everything scoped by an Altium query (OnLayer, InNet, IsVia,
InDifferentialPairClass, AsMils(Hole), ...) is dropped. This translates those queries into
KiCad rule conditions and the rule values into KiCad constraints.

Used by the harness (facts from altium_monkey) and, as pcbferry_dru.py, by the pcbferry
converter: `custom_rules(pcbdoc)` reads everything it needs from the PcbDoc (olefile).
"""

import re

MIL = 0.0254


class Untranslatable(Exception):
    """A query function or rule value KiCad has no counterpart for."""


# ---------- Altium query language -> KiCad condition ----------

_TOKEN = re.compile(r"\s*(?:(\()|(\))|(,)|'((?:[^']|'')*)'|([A-Za-z_][A-Za-z0-9_.]*)|(<=|>=|<>|=|<|>)|(\d+(?:\.\d+)?)|(\S+))")
TRUE = object()   # "All": matches everything
FALSE = object()  # matches nothing (a layer, class or diff pair the board doesn't have)

# Altium class kinds (Classes6 KIND)
NET_CLASS, COMPONENT_CLASS, LAYER_CLASS, DIFF_PAIR_CLASS = 0, 1, 4, 6

_TYPES = {
    "IsVia": "Via", "IsTrack": "Track", "IsArc": "Arc", "IsPad": "Pad",
    "IsRegion": "Zone", "IsPolygon": "Zone", "IsFill": "Zone", "IsText": "*Text",
    "IsComponent": "Footprint",
}
_UNSUPPORTED = {
    "TouchesRoom": "rooms are not imported", "WithinRoom": "rooms are not imported",
    "InRoom": "rooms are not imported", "InPolygonClass": "polygon classes are not imported",
    "InxSignalClass": "xSignals have no KiCad counterpart", "InxSignal": "xSignals have no KiCad counterpart",
    "HasFootprint": "no KiCad condition on a footprint's library name for its items",
}
_CANONICAL = {n.lower(): n for n in [*_TYPES, *_UNSUPPORTED, "All", "IsSMTPad", "IsThruPad", "InAnyNet",
                                      "InNet", "InNetClass", "OnLayer", "InLayerClass", "InComponent",
                                      "InComponentClass", "InDifferentialPairClass", "InDifferentialPair"]}
# Altium numeric properties inside AsMils()/AsMM(): KiCad property per object type
_NUMERIC = {"hole": {"Via": "Hole", "Pad": "Hole_Size_X"}}
_CMP = {"=": "==", "<>": "!=", "<": "<", ">": ">", "<=": "<=", ">=": ">="}


# ---------- which rules the importer itself carries ----------

# Altium rule kind -> [(Altium value, KiCad board setting)] for board-wide (All/All) rules
DEFAULT_RULE_SETTINGS = {
    "Clearance": [("GAP", "min_clearance")],
    "Width": [("MINLIMIT", "track_min_width")],
    "RoutingVias": [("MINWIDTH", "vias_min_size")],
    "HoleSize": [("MINLIMIT", "min_through_drill")],
    "HoleToHoleClearance": [("GAP", "hole_to_hole_min")],
    "BoardOutlineClearance": [("GAP", "copper_edge_clearance")],
    "SolderMaskExpansion": [("EXPANSION", "solder_mask_expansion")],
    "PasteMaskExpansion": [("EXPANSION", "solder_paste_margin")],
    "MinimumAnnularRing": [("MINIMUMRING", "vias_min_annular_width")],
    "SilkToSilkClearance": [("SILKTOSILKCLEARANCE", "silk_clearance")],
    "MinimumSolderMaskSliver": [("MINSOLDERMASKWIDTH", "solder_mask_min_width")],
}
# ... and onto a netclass (Default for All/All, or the one InNetClass('X') names)
NETCLASS_RULE_FIELDS = {
    "Clearance": [("GAP", "clearance")],
    "Width": [("PREFEREDWIDTH", "track_width")],
    "RoutingVias": [("WIDTH", "via_diameter"), ("HOLEWIDTH", "via_drill")],
    "DiffPairsRouting": [("TOPLAYER_PREFWIDTH", "diff_pair_width"), ("MOSTFREQGAP", "diff_pair_gap")],
}
# Carried by each zone rather than the board
ZONE_RULES = {"PlaneClearance", "PolygonConnect"}
# Why an Altium rule kind has no KiCad counterpart in the import
RULE_GAP_REASON = {
    **dict.fromkeys(("ShortCircuit", "UnRoutedNet", "NetAntennae", "UnpouredPolygon"),
                    "KiCad DRC always checks this"),
    **dict.fromkeys(("FanoutControl", "RoutingTopology", "RoutingCorners", "RoutingPriority",
                     "LayerPairs", "SupplyNets", "AssemblyTestpoint", "AssemblyTestPointUsage",
                     "FabricationTestpoint", "FabricationTestPointUsage", "AcuteAngle",
                     "RoomDefinition", "Height", "ComponentClearance", "StubLength"),
                    "no KiCad equivalent"),
    **dict.fromkeys(("Clearance", "Width", "RoutingVias", "DiffPairsRouting", "HoleToHoleClearance",
                     "MinimumAnnularRing", "HoleSize", "MatchedLengths", "RoutingLayers",
                     "SilkToBoardRegionClearance", "SilkToSolderMaskClearance", "ViasUnderSMD",
                     "SilkscreenOverComponentPads", "PlaneConnect", "PlaneClearance",
                     "SolderMaskExpansion"),
                    "expressible as a KiCad custom rule"),
}
_NETCLASS_SCOPE = re.compile(r"InNetClass\('([^']*)'\)")


def importer_carries(rule: dict) -> bool:
    """Whether the importer maps this Altium rule onto board settings, a netclass or zones."""
    if rule["kind"] in ZONE_RULES:
        # The zone carries one clearance: the board-wide plane rule.
        return rule["kind"] != "PlaneClearance" or (rule["scope1"], rule["scope2"]) == ("All", "All")
    if (rule["scope1"], rule["scope2"]) == ("All", "All"):
        return rule["kind"] in DEFAULT_RULE_SETTINGS or rule["kind"] in NETCLASS_RULE_FIELDS
    return bool(rule["kind"] in NETCLASS_RULE_FIELDS and _NETCLASS_SCOPE.fullmatch(rule["scope1"])
                and rule["scope2"] in ("All", rule["scope1"]))


def no_custom_reason(kind: str) -> str | None:
    """Why a rule kind gets no custom rule, or None when it can have one."""
    reason = RULE_GAP_REASON.get(kind)
    return None if reason == "expressible as a KiCad custom rule" else reason or "unclassified"


def _q(text: str) -> str:
    return "'" + text.replace("\\", "\\\\").replace("'", "\\'") + "'"


def _or(parts):
    parts = [p for p in parts if p is not FALSE]
    if any(p is TRUE for p in parts):
        return TRUE
    if not parts:
        return FALSE
    parts = list(dict.fromkeys(parts))
    return parts[0] if len(parts) == 1 else "(" + " || ".join(parts) + ")"


def _and(parts):
    parts = [p for p in parts if p is not TRUE]
    if any(p is FALSE for p in parts):
        return FALSE
    if not parts:
        return TRUE
    parts = list(dict.fromkeys(parts))
    return parts[0] if len(parts) == 1 else "(" + " && ".join(parts) + ")"


class Context:
    def __init__(self, board: dict, classes: list[dict], diff_pairs: list[dict]):
        self.classes = classes or []
        self.diff_pairs = {d["name"]: d for d in diff_pairs or []}
        # Altium layer name or id (normalised) -> KiCad canonical layer name
        self.layers = {}
        for canonical, name in (board.get("layer_names") or {}).items():
            self.layers[_norm(name)] = canonical
        for altium_id, canonical in zip(board.get("altium_copper_ids") or [],
                                        board.get("copper_canonical") or []):
            self.layers.setdefault(_norm(altium_id), canonical)
        canon = board.get("copper_canonical") or []
        if canon:
            self.layers.setdefault("toplayer", canon[0])
            self.layers.setdefault("bottomlayer", canon[-1])
        for altium, canonical in (("topoverlay", "F.SilkS"), ("bottomoverlay", "B.SilkS"),
                                  ("topsolder", "F.Mask"), ("bottomsolder", "B.Mask"),
                                  ("toppaste", "F.Paste"), ("bottompaste", "B.Paste")):
            self.layers.setdefault(altium, canonical)

    def layer(self, name: str) -> str | None:
        """The KiCad layer, or None for a layer the board doesn't have (Altium: no match)."""
        return self.layers.get(_norm(name))

    def members(self, name: str, kind: int) -> list[str]:
        for cls in self.classes:
            if cls["name"] == name and cls["kind"] == kind:
                return cls["members"]
        return []  # a class the board doesn't have matches nothing


def _norm(name: str) -> str:
    return re.sub(r"[\s_]", "", name or "").lower()


def translate_query(query: str, item: str, ctx: Context):
    """An Altium scope query -> a KiCad condition on `item` ('A' or 'B'), or TRUE."""
    tokens = []
    for m in _TOKEN.finditer(query):
        lpar, rpar, comma, string, ident, cmp, number, other = m.groups()
        if other:
            raise Untranslatable(f"operator {other!r}")
        if cmp or number:
            tokens.append(("cmp", cmp) if cmp else ("num", number))
            continue
        tokens.append(("(" if lpar else ")" if rpar else "," if comma else
                       "str" if string is not None else "id",
                       string.replace("''", "'") if string is not None else ident))
    pos = 0

    def peek():
        return tokens[pos] if pos < len(tokens) else (None, None)

    def take(kind=None):
        nonlocal pos
        tok = peek()
        if kind and tok[0] != kind:
            raise Untranslatable(f"cannot parse {query!r}")
        pos += 1
        return tok

    def expr():
        left = term()
        while peek()[0] == "id" and peek()[1].lower() == "or":
            take()
            left = _or([left, term()])
        return left

    def term():
        left = factor()
        while peek()[0] == "id" and peek()[1].lower() == "and":
            take()
            left = _and([left, factor()])
        return left

    def factor():
        kind, value = peek()
        if kind == "id" and value.lower() == "not":
            take()
            inner = factor()
            return FALSE if inner is TRUE else TRUE if inner is FALSE else f"!({inner})"
        if kind == "(":
            take()
            inner = expr()
            take(")")
            return inner
        if kind != "id":
            raise Untranslatable(f"cannot parse {query!r}")
        if value.lower() in ("asmils", "asmm"):
            return numeric()
        take()
        args = []
        if peek()[0] == "(":
            take()
            while peek()[0] != ")":
                args.append(take("str")[1])
                if peek()[0] == ",":
                    take()
            take(")")
        return call(value, args)

    def numeric():
        """AsMils(Hole) > 50 -> a comparison on the item's KiCad property."""
        unit = "mil" if take()[1].lower() == "asmils" else "mm"
        take("(")
        prop = take("id")[1]
        take(")")
        op, num = take("cmp")[1], take("num")[1]
        by_type = _NUMERIC.get(prop.lower())
        if not by_type:
            raise Untranslatable(f"numeric property {prop}: no KiCad counterpart yet")
        return _or(f"({item}.Type == {_q(t)} && {item}.{kprop} {_CMP[op]} {num}{unit})"
                   for t, kprop in by_type.items())

    def call(name, args):
        # Altium query keywords ignore case (isVia, ISVIA)
        name = _CANONICAL.get(name.lower(), name)
        if name in _UNSUPPORTED:
            raise Untranslatable(f"{name}: {_UNSUPPORTED[name]}")
        if name == "All":
            return TRUE
        if name in _TYPES:
            return f"{item}.Type == {_q(_TYPES[name])}"
        if name == "IsSMTPad":
            return f"({item}.Type == 'Pad' && {item}.Pad_Type == 'SMD')"
        if name == "IsThruPad":
            return f"({item}.Type == 'Pad' && {item}.Pad_Type == 'Through-hole')"
        if name == "InAnyNet":
            return f"{item}.NetName != ''"
        if name == "InNet" and args:
            return f"{item}.NetName == {_q(args[0])}"
        if name == "InNetClass" and args:
            return f"{item}.hasNetclass({_q(args[0])})"
        if name == "OnLayer" and args:
            # An Altium object's own layer: vias and through-hole pads sit on MultiLayer.
            multi = f"({item}.Type == 'Via' || ({item}.Type == 'Pad' && {item}.Pad_Type == 'Through-hole'))"
            if _norm(args[0]) == "multilayer":
                return multi
            layer = ctx.layer(args[0])
            return _and([f"{item}.existsOnLayer({_q(layer)})", f"!{multi}"]) if layer else FALSE  # multi is parenthesised
        if name == "InLayerClass" and args:
            return _or(f"{item}.existsOnLayer({_q(l)})" if l else FALSE
                       for l in map(ctx.layer, ctx.members(args[0], LAYER_CLASS)))
        if name in ("InComponent", "InComponentClass") and args:
            refs = [args[0]] if name == "InComponent" else ctx.members(args[0], COMPONENT_CLASS)
            return _or(f"{item}.memberOfFootprint({_q(r)})" for r in refs)
        if name == "InDifferentialPairClass" and args:
            if args[0] == "All Differential Pairs":
                return f"{item}.inDiffPair('*')"
            return _pair_nets(item, ctx.members(args[0], DIFF_PAIR_CLASS), ctx)
        if name == "InDifferentialPair" and args:
            return _pair_nets(item, args, ctx)
        raise Untranslatable(f"{name}: no KiCad counterpart yet")

    result = expr()
    if pos != len(tokens):
        raise Untranslatable(f"cannot parse {query!r}")
    return result


def _pair_nets(item, pairs, ctx):
    nets = []
    for name in pairs:
        pair = ctx.diff_pairs.get(name)  # a pair the board doesn't have matches nothing
        if pair:
            nets += [pair["positive"], pair["negative"]]
    return _or(f"{item}.NetName == {_q(n)}" for n in nets)


# ---------- rule values -> constraints ----------

def _mm(value) -> str:
    return f"{max(value, 0):.4f}mm".replace(".0000mm", "mm")


# Altium stores lengths in 1/10000 mil; converted to mm they land a few nanometres off the
# value the designer typed, which KiCad's DRC (no tolerance) then flags. Allow 0.1 um.
TOLERANCE_MM = 0.0001


def _limits(name, values, lo=None, opt=None, hi=None):
    def value(kind, field):
        v = values[field]
        return v - TOLERANCE_MM if kind == "min" else v + TOLERANCE_MM if kind == "max" else v
    parts = [f"({k} {_mm(value(k, f))})" for k, f in (("min", lo), ("opt", opt), ("max", hi))
             if f and values.get(f) is not None]
    if not parts:
        raise Untranslatable(f"no value for {name}")
    return f"(constraint {name} {' '.join(parts)})"


# Altium clearance object kinds -> KiCad item tests. Fills, polygons and regions all come
# in as zones; holes are left to hole_clearance.
_OBJ = {
    "Arc": lambda i: f"{i}.Type == 'Arc'",
    "Track": lambda i: f"{i}.Type == 'Track'",
    "SMDPad": lambda i: f"({i}.Type == 'Pad' && {i}.Pad_Type == 'SMD')",
    "THPad": lambda i: f"({i}.Type == 'Pad' && {i}.Pad_Type == 'Through-hole')",
    "Via": lambda i: f"{i}.Type == 'Via'",
    "Fill": lambda i: f"{i}.Type == 'Zone'",
    "Poly": lambda i: f"{i}.Type == 'Zone'",
    "Region": lambda i: f"{i}.Type == 'Zone'",
    "Text": lambda i: f"{i}.Type == '*Text'",
}
_ZONE_KINDS = {"Fill": "Zone", "Poly": "Zone", "Region": "Zone"}
ALTIUM_UNIT_MM = 0.0254 / 10000  # OBJECTCLEARANCES are in 1/10000 mil


def clearance_matrix(rule: dict) -> dict[tuple[str, str], float]:
    """Altium's per object-type pair clearances, in mm. Fills, polygons and regions map to one
    KiCad kind (zone), so they keep the smallest of their gaps."""
    out = {}
    for entry in ((rule.get("flags") or {}).get("OBJECTCLEARANCES") or "").split(";"):
        m = re.fullmatch(r"ClearanceObj_(\w+)-ClearanceObj_(\w+):(\d+)", entry.strip())
        if not m or "Hole" in (m.group(1), m.group(2)) or m.group(1) not in _OBJ or m.group(2) not in _OBJ:
            continue
        a, b = (_ZONE_KINDS.get(x, x) for x in (m.group(1), m.group(2)))
        a, b = sorted((a if a != "Zone" else "Region", b if b != "Zone" else "Region"))
        gap = int(m.group(3)) * ALTIUM_UNIT_MM
        out[(a, b)] = min(gap, out.get((a, b), gap))
    return out


BINARY = {"Clearance", "HoleToHoleClearance", "SilkToSilkClearance", "SilkToSolderMaskClearance",
          "SilkscreenOverComponentPads"}


def constraints(rule: dict, ctx: Context) -> list[tuple[str | None, str, str | None]]:
    """[(extra condition, constraint, layer clause)] for one Altium rule."""
    v, kind = rule["values"], rule["kind"]
    if kind == "Clearance":
        if (rule.get("flags") or {}).get("NETSCOPE") == "SameNet":
            raise Untranslatable("same-net clearance")
        out = [(None, _limits("clearance", v, lo="GAP"), None)]
        # Per object-type pair gaps; pairs not listed use the generic gap. Later rules win.
        for (a, b), gap in sorted(clearance_matrix(rule).items()):
            if abs(gap - v["GAP"]) < 1e-6:
                continue
            pair = _or([_and([_OBJ[a]("A"), _OBJ[b]("B")]), _and([_OBJ[b]("A"), _OBJ[a]("B")])])
            out.append((pair, f"(constraint clearance (min {_mm(gap - TOLERANCE_MM)}))", None))
        return out
    if kind == "Width":
        return [(None, _limits("track_width", v, "MINLIMIT", "PREFEREDWIDTH", "MAXLIMIT"), None)]
    if kind == "RoutingVias":
        return [(None, _limits("via_diameter", v, "MINWIDTH", "WIDTH", "MAXWIDTH"), None),
                ("A.Type == 'Via'", _limits("hole_size", v, "MINHOLEWIDTH", "HOLEWIDTH", "MAXHOLEWIDTH"), None)]
    if kind == "DiffPairsRouting":
        # Widths and gaps are per layer (TOPLAYER_*, MIDLAYERn_*, BOTTOMLAYER_*); the
        # MIN/MAXLIMIT and MOSTFREQGAP fields are the fallback gap.
        out = [(None, _limits("diff_pair_gap", v, "MINLIMIT", "MOSTFREQGAP", "MAXLIMIT"), None)]
        for prefix in sorted({k.rsplit("_", 1)[0] for k in v if k.endswith("_MINWIDTH")}):
            altium = {"TOPLAYER": "TopLayer", "BOTTOMLAYER": "BottomLayer"}.get(
                prefix, prefix.replace("MIDLAYER", "MidLayer"))
            layer = ctx.layer(altium)
            if not layer:
                continue
            out.append(("A.inDiffPair('*')", _limits("track_width", v, f"{prefix}_MINWIDTH",
                                                     f"{prefix}_PREFWIDTH", f"{prefix}_MAXWIDTH"), layer))
            if v.get(f"{prefix}_MINGAP") is not None:
                out.append((None, _limits("diff_pair_gap", v, f"{prefix}_MINGAP", f"{prefix}_PREFGAP",
                                          f"{prefix}_MAXGAP"), layer))
        if v.get("MAXUNCOUPLEDLENGTH"):
            out.append((None, _limits("diff_pair_uncoupled", v, hi="MAXUNCOUPLEDLENGTH"), None))
        return out
    if kind == "HoleToHoleClearance":
        return [(None, _limits("hole_to_hole", v, lo="GAP"), None)]
    if kind == "MinimumAnnularRing":
        return [(None, _limits("annular_width", v, lo="MINIMUMRING"), None)]
    if kind == "HoleSize":
        if not rule.get("absolute", True):
            raise Untranslatable("hole limits given as percentages")
        return [(None, _limits("hole_size", v, "MINLIMIT", None, "MAXLIMIT"), None)]
    if kind == "MatchedLengths":
        flags = rule.get("flags", {})
        within = " (within_diff_pairs)" if flags.get("CHECKNETSINDIFFPAIR") == "TRUE" and \
            flags.get("CHECKOTHERS") != "TRUE" and flags.get("CHECKXSIGNALS") != "TRUE" else ""
        return [(None, _limits("skew", v, hi="TOLERANCE")[:-1] + within + ")", None)]
    if kind in ("SilkToSilkClearance",):
        return [(None, _limits("silk_clearance", v, lo="SILKTOSILKCLEARANCE"), None)]
    if kind in ("SilkToSolderMaskClearance", "SilkscreenOverComponentPads"):
        return [("B.Type == 'Pad'", _limits("silk_clearance", v, lo="MINSILKSCREENTOMASKGAP"), None)]
    if kind == "RoutingLayers":
        out = []
        for key, allowed in (rule.get("flags") or {}).items():
            if not key.endswith("_V5") or allowed != "FALSE":
                continue
            layer = ctx.layer(key[:-3])
            if not layer:
                continue  # a layer the board doesn't have
            out.append((None, "(constraint disallow track via)", layer))
        if not out:
            raise Untranslatable("every layer is allowed: nothing to restrict")
        return out
    if kind == "SolderMaskExpansion":
        # A large negative expansion is how Altium designs tent vias by rule.
        if v.get("EXPANSION") is None:
            raise Untranslatable("the rule record carries no expansion")
        return [(None, f"(constraint solder_mask_expansion (opt {v['EXPANSION']:.4f}mm))", None)]
    if kind == "PlaneClearance":
        # Altium clears a plane around the hole of an unconnected pad or via (planes carry
        # no pad lands); KiCad keeps the land, so the gap applies to the land and the hole.
        planes = sorted({canonical for name, canonical in ctx.layers.items() if "plane" in name})
        if not planes:
            raise Untranslatable("the board has no plane layers")
        on_plane = _and(["B.Type == 'Zone'", _or(f"B.existsOnLayer({_q(p)})" for p in planes)])
        return [(on_plane, _limits("clearance", v, lo="CLEARANCE"), None),
                (on_plane, _limits("hole_clearance", v, lo="CLEARANCE"), None)]
    if kind == "PlaneConnect":
        planes = sorted({canonical for name, canonical in ctx.layers.items() if "plane" in name})
        if not planes:
            raise Untranslatable("the board has no plane layers")
        on_plane = _and(["B.Type == 'Zone'", _or(f"B.existsOnLayer({_q(p)})" for p in planes)])
        flags = rule.get("flags") or {}
        out = []
        # Older records hold one setting; newer ones one for pads and one for vias.
        for prefix, item_type in (("", None), ("PAD.", "Pad"), ("VIA.", "Via")):
            style = {"Relief": "thermal_reliefs", "Direct": "solid", "NoConnect": "none"}.get(
                flags.get(prefix + "PLANECONNECTSTYLE"))
            if not style:
                continue
            cond = on_plane if item_type is None else _and([f"A.Type == {_q(item_type)}", on_plane])
            out.append((cond, f"(constraint zone_connection {style})", None))
            if style == "thermal_reliefs":
                if v.get(prefix + "RELIEFAIRGAP") is not None:
                    out.append((cond, _limits("thermal_relief_gap", v, lo=prefix + "RELIEFAIRGAP"), None))
                if v.get(prefix + "RELIEFCONDUCTORWIDTH") is not None:
                    out.append((cond, _limits("thermal_spoke_width", v, opt=prefix + "RELIEFCONDUCTORWIDTH"), None))
        if not out:
            raise Untranslatable("unknown plane connect style")
        return out
    if kind == "ViasUnderSMD":
        raise Untranslatable("KiCad has no via-in-SMD-pad constraint")
    if kind == "SilkToBoardRegionClearance":
        raise Untranslatable("the rule record carries no clearance value")
    raise Untranslatable(f"rule kind {kind}")


def condition(rule: dict, ctx: Context):
    s1 = translate_query(rule["scope1"] or "All", "A", ctx)
    if rule["kind"] not in BINARY:
        return s1
    # Altium applies a binary rule when one object matches each scope, in either order.
    s2 = translate_query(rule["scope2"] or "All", "B", ctx)
    s1b = translate_query(rule["scope1"] or "All", "B", ctx)
    s2a = translate_query(rule["scope2"] or "All", "A", ctx)
    return _or([_and([s1, s2]), _and([s2a, s1b])])


# ---------- whole rule set ----------

def translate(rules: list[dict], ctx: Context, carried, no_custom_reason=lambda kind: None
              ) -> tuple[str, list[dict]]:
    """(.kicad_dru text, one entry per Altium rule: status and reason).

    `carried(rule)` tells whether the importer already maps the rule onto board settings or a
    netclass. For any rule kind that needs custom rules, every enabled rule of that kind is
    written, highest Altium precedence last: KiCad applies the last matching custom rule,
    and custom rules override board settings and netclasses."""
    enabled = [r for r in rules if r["enabled"]]
    results, blocks = [], []
    needs_custom = {r["kind"] for r in enabled if not carried(r)}
    # The importer carries only the generic gap of a clearance rule; a per object-type matrix
    # needs custom rules even for the board-wide rule.
    if any(r["kind"] == "Clearance" and any(abs(g - r["values"].get("GAP", g)) > 1e-6
                                            for g in clearance_matrix(r).values()) for r in enabled):
        needs_custom.add("Clearance")
    for rule in sorted(enabled, key=lambda r: -r["priority"]):
        entry = {"kind": rule["kind"], "name": rule["name"], "scope1": rule["scope1"],
                 "scope2": rule["scope2"], "carried": carried(rule)}
        if rule["kind"] not in needs_custom:
            entry["status"] = "imported"
            results.append(entry)
            continue
        if (reason := no_custom_reason(rule["kind"])) and not entry["carried"]:
            entry["status"], entry["reason"] = "no_equivalent", reason
            results.append(entry)
            continue
        try:
            cond = condition(rule, ctx)
            if cond is FALSE:
                entry["status"], entry["reason"] = "matches_nothing", "its scope matches nothing on this board"
                results.append(entry)
                continue
            for extra, constraint, layer in constraints(rule, ctx):
                full = _and([cond, extra if extra is not None else TRUE])
                lines = [f"(rule {_q_rule(rule['name'])}"]
                if layer:
                    lines.append(f"  (layer \"{layer}\")")
                lines.append(f"  {constraint}")
                if full is not TRUE:
                    lines.append(f"  (condition \"{full}\")")
                blocks.append("\n".join(lines) + ")")
            entry["status"] = "translated"
        except Untranslatable as why:
            if str(why).startswith(("every layer is allowed", "the board has no plane layers")):
                entry["status"] = "no_op"
            else:
                entry["status"] = "imported" if entry["carried"] else "untranslatable"
            entry["reason"] = str(why)
        results.append(entry)
    header = ("(version 1)\n"
              "# Translated from the Altium design rules. Lower Altium precedence first:\n"
              "# KiCad applies the last matching rule.\n")
    return header + "\n\n".join(blocks) + "\n", results


def _q_rule(name: str) -> str:
    return '"' + name.replace('"', "'") + '"'


# ---------- straight from the files (pcbferry: no Altium, no altium_monkey) ----------

def _length_mm(text: str) -> float | None:
    """An Altium length ("5.9055mil", "0.1mm") in mm."""
    m = re.fullmatch(r"\s*(-?[\d.]+)\s*(mil|mm)?\s*", text or "")
    if not m:
        return None
    value = float(m.group(1))
    return value * 0.0254 if m.group(2) != "mm" else value


def _props(record: bytes) -> dict[str, str]:
    text = record.decode("latin-1").strip("|\x00")
    return dict(item.split("=", 1) for item in text.split("|") if "=" in item)


def _records(ole, stream: str, header: int = 0) -> list[dict[str, str]]:
    """Length-prefixed text records of a PcbDoc stream (`header` bytes before each length)."""
    import struct
    if not ole.exists(stream + "/Data"):
        return []
    data, i, out = ole.openstream(stream + "/Data").read(), 0, []
    while i + header + 4 <= len(data):
        i += header
        n = struct.unpack_from("<I", data, i)[0]
        i += 4
        out.append(_props(data[i:i + n]))
        i += n
    return out


def rule_record(props: dict[str, str]) -> dict:
    """A Rules6 record in the form `translate` takes."""
    return {
        "kind": props.get("RULEKIND", ""),
        "name": props.get("NAME", ""),
        "enabled": props.get("ENABLED", "TRUE").upper() == "TRUE",
        "priority": int(props.get("PRIORITY", "1") or 1),
        "scope1": (props.get("SCOPE1EXPRESSION") or "").strip(),
        "scope2": (props.get("SCOPE2EXPRESSION") or "").strip(),
        "values": {k: v for k, v in ((k, _length_mm(v)) for k, v in props.items()) if v is not None
                   and k not in ("PRIORITY", "UNIONINDEX", "NETSCOPE", "LAYERKIND")},
        "absolute": props.get("ABSOLUTEVALUES", "TRUE").upper() == "TRUE",
        "flags": {k: v for k, v in props.items() if _length_mm(v) is None
                  and k not in ("RULEKIND", "NAME", "SCOPE1EXPRESSION", "SCOPE2EXPRESSION", "COMMENT")},
    }


def _altium_copper_name(layer_id: int) -> str | None:
    """Altium's default name of a copper layer id (V7 ids or the old 1..54 numbering)."""
    if layer_id >= 0x01000000:
        group, low = layer_id >> 16, layer_id & 0xFFFF
        if group == 0x0100:
            return "Top Layer" if low == 1 else "Bottom Layer" if low == 0xFFFF else f"Mid Layer {low - 1}"
        if group == 0x0101:
            return f"Internal Plane {low}"
        return None
    if layer_id == 1:
        return "Top Layer"
    if layer_id == 32:
        return "Bottom Layer"
    if 2 <= layer_id <= 31:
        return f"Mid Layer {layer_id - 1}"
    if 39 <= layer_id <= 54:
        return f"Internal Plane {layer_id - 38}"
    return None


def read_pcbdoc(path) -> dict:
    """Rules, classes, diff pairs and the copper layer order of a PcbDoc."""
    import olefile
    with olefile.OleFileIO(str(path)) as ole:
        board = (_records(ole, "Board6") or [{}])[0]
        rules = [rule_record(p) for p in _records(ole, "Rules6", header=2)]
        classes = [{"name": p.get("NAME", ""), "kind": int(p.get("KIND", "0") or 0),
                    "members": [p[k] for k in sorted((k for k in p if re.fullmatch(r"M\d+", k)),
                                                     key=lambda k: int(k[1:]))]}
                   for p in _records(ole, "Classes6")]
        pairs = [{"name": p.get("NAME", ""), "positive": p.get("POSITIVENETNAME", ""),
                  "negative": p.get("NEGATIVENETNAME", "")} for p in _records(ole, "DifferentialPairs6")]
    # Copper order: the V9 stack lists every layer in board order; older files chain LAYERn.
    copper, names, mech = [], [], {}
    n = 1
    while f"V9_STACK_LAYER{n}_LAYERID" in board:
        layer_id = int(board[f"V9_STACK_LAYER{n}_LAYERID"])
        name = _altium_copper_name(layer_id)
        if name:
            copper.append(name)
            names.append(board.get(f"V9_STACK_LAYER{n}_NAME", name))
        elif layer_id >> 16 == 0x0102:  # Mechanical k
            mech[layer_id & 0xFFFF] = board.get(f"V9_STACK_LAYER{n}_NAME", f"Mechanical {layer_id & 0xFFFF}")
        n += 1
    if not copper:
        layer, seen = 1, set()
        while layer and layer not in seen:
            seen.add(layer)
            name = _altium_copper_name(layer)
            if name:
                copper.append(name)
                names.append(board.get(f"LAYER{layer}NAME", name))
            layer = int(board.get(f"LAYER{layer}NEXT", "0") or 0)
        for k in range(1, 17):
            if f"LAYER{56 + k}NAME" in board:
                mech[k] = board[f"LAYER{56 + k}NAME"]
    return {"rules": rules, "classes": classes, "diff_pairs": pairs, "copper": copper,
            "copper_names": names, "mechanical": mech}


def pcbdoc_board_layers(doc: dict) -> dict:
    """The KiCad layer names the importer gives a PcbDoc's layers: copper in stack order
    (F.Cu, In1.Cu, ..., B.Cu) under the Altium names, Mechanical k as User.k."""
    count = len(doc["copper"])
    canonical = ["F.Cu"] + [f"In{i}.Cu" for i in range(1, count - 1)] + (["B.Cu"] if count > 1 else [])
    names = dict(zip(canonical, doc["copper_names"]))
    names.update({f"User.{k}": name for k, name in doc["mechanical"].items() if k <= 45})
    return {"layer_names": names, "copper_canonical": canonical, "altium_copper_ids": doc["copper"]}


def kicad_board_layers(kicad_pcb_text: str) -> dict:
    """{"layer_names": {canonical: name}, "copper_canonical": [...]} from a .kicad_pcb header."""
    head = kicad_pcb_text[:40000]
    block = head[head.find("(layers"):]
    block = block[:block.find("\n\t)")]
    names = {m[0]: m[1] or m[0] for m in re.findall(r'\(\d+ "([^"]+)" \w+(?: "([^"]*)")?', block)}
    copper = [l for l in names if l.endswith(".Cu")]
    order = {"F.Cu": -1, "B.Cu": 1000}
    copper.sort(key=lambda l: order.get(l, int(re.sub(r"\D", "", l) or 0)))
    return {"layer_names": names, "copper_canonical": copper}


def custom_rules(pcbdoc) -> tuple[str, list[dict]]:
    """The .kicad_dru for a PcbDoc's import, and what happened to each enabled Altium rule.
    Needs only the PcbDoc, so it can be written before the import fills zones."""
    doc = read_pcbdoc(pcbdoc)
    ctx = Context(pcbdoc_board_layers(doc), doc["classes"], doc["diff_pairs"])
    return translate(doc["rules"], ctx, importer_carries, no_custom_reason)
