# Counts what app code writes by hand that the design system, or a narrower redraw, already covers, and how
# long each file is (src/ui/rules.tsv). Holds every file to its count in src/ui/rules.baseline; counts only fall.
#
#   python3 tools/ui_lint.py              warnings: what's left, by rule and by file
#   python3 tools/ui_lint.py FILE...      warnings for those files; exit 1 if one went over its count
#   python3 tools/ui_lint.py --check      the whole tree and the catalog; exit 1 on anything new (cargo test runs this)
#   python3 tools/ui_lint.py --update     lower the baseline to today's counts; regenerate src/ui/CATALOG.md and ONE_OFFS.md
#   python3 tools/ui_lint.py --against REF   CI: no rule dropped, no rule's baseline total risen since REF
#   python3 tools/ui_lint.py --hook pre|post   for .claude/settings.json: reads the hook's JSON on stdin
#
# A line is exempt from a rule when it, or the line above it, carries `lint-allow(<rule>): <reason>` in a
# comment; several rules go comma-separated. An allow without a reason is itself reported. A file-wide rule
# (`file-lines`) is waived by the allow in one of the file's first lines.
import json, os, re, sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ui_chains
import ui_find
import ui_perf

# Scopes whose rules ui_lint's line loop leaves to a later pass over the whole file.
WHOLE_FILE = ("file", "chain", "fn", "render", "ui-thread", "hot-notify", "growth", "probe")

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RULES = "src/ui/rules.tsv"
BASELINE = "src/ui/rules.baseline"
CATALOG = "src/ui/CATALOG.md"
# The design system itself: where the raw values live.
EXEMPT = ("src/ui.rs", "src/ui/", "src/palette.rs", "src/motion.rs", "src/icons.rs")
ALLOW = re.compile(r"lint-allow\(([a-z0-9_, -]+)\)\s*:?\s*(.*)")
NO_REASON = "allow-without-reason"
# An allow whose reason is too thin to tell the next reader anything ("needed", "custom").
WEAK_REASON = "allow-weak-reason"
# An allow for a component rule that doesn't name the design-system item it considered first.
NO_REFERENCE = "allow-without-reference"
WEAK_WORDS = {"needed", "custom", "easier", "simpler", "special", "one-off", "oneoff", "temporary", "todo", "hack", "legacy", "fine", "ok", "works", "it", "is", "a", "the"}
ONE_OFFS = "src/ui/ONE_OFFS.md"
# An unvirtualized list's allow says how big the list can get, so whoever grows it knows to virtualize it.
NO_BOUND = "allow-without-bound"
LIST_RULE = "unvirtualized-list"
BOUND = re.compile(r"\d|\b(at most|up to|few|handful|dozen|bounded|capped|never more|no more|fixed|only)\b", re.I)
VIRTUAL = re.compile(r"(?<![A-Za-z0-9_])(uniform_list|list)\s*\(")
# Calls that don't make a function a use of a component: layout, text and tooltips.
NOT_COMPONENTS = {"row", "col", "line", "sized", "visible_rows", "density_pad", "mix", "tip"}
# Two allowed one-offs of one rule this alike are the same component built twice.
DUPLICATE_AT = 0.8
IMPL_OPEN = re.compile(r"^\s*impl\b[^{]*\b(\w+)\s*\{")
TEST_ATTR = re.compile(r"#\[cfg\(test\)\]")
MOD_OPEN = re.compile(r"^\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{")
# How many of a file's first lines may hold its file-wide allow.
HEAD_LINES = 5
# A file past this share of its limit gets a warning before each edit.
WARN_AT = 0.75
RAW_STRING = re.compile(r'r(#*)"')


def load_rules():
    rules = []
    for line in open(os.path.join(ROOT, RULES), encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        name, pattern, scope, fix = line.rstrip("\n").split("\t")
        # A `file` rule's second column is its limit and a `chain` rule's its clauses (tools/ui_chains.py).
        match scope:
            case "file":
                pattern = int(pattern)
            case "chain":
                pattern = ui_chains.compile_pattern(pattern)
            case "hot-notify" | "growth" | "probe":
                pattern = None
            case _:
                pattern = re.compile(pattern)
        rules.append({"name": name, "pattern": pattern, "scope": scope, "fix": fix})
    return rules


_SPLIT = {}


def split_code(lines):
    """Memoized: the lexer is the slow part, and the corpus and the scan both need every file."""
    key = chr(10).join(lines)
    if key not in _SPLIT:
        _SPLIT[key] = _split_code(lines)
    return _SPLIT[key]


def _split_code(lines):
    """Each line's code with string and char contents blanked, and its comment text.
    A small Rust lexer: enough that braces and patterns inside strings and comments don't count."""
    out, state, raw_hashes = [], None, 0  # state: None, "str", "raw", "block"
    for line in lines:
        code, comment, i = [], [], 0
        while i < len(line):
            c, rest = line[i], line[i:]
            if state == "block":
                end = line.find("*/", i)
                comment.append(line[i:] if end < 0 else line[i:end])
                if end < 0:
                    break
                state, i = None, end + 2
            elif state == "str":
                if c == "\\":
                    i += 2
                    continue
                if c == '"':
                    state = None
                    code.append('"')
                i += 1
            elif state == "raw":
                close = '"' + "#" * raw_hashes
                end = line.find(close, i)
                if end < 0:
                    break
                state, i = None, end + len(close)
                code.append('"')
            elif rest.startswith("//"):
                comment.append(line[i + 2 :])
                break
            elif rest.startswith("/*"):
                state, i = "block", i + 2
            elif (m := RAW_STRING.match(line, i)) and (i == 0 or not (line[i - 1].isalnum() or line[i - 1] == "_")):
                state, raw_hashes, i = "raw", len(m.group(1)), m.end()
                code.append('"')
            elif c == '"':
                state, i = "str", i + 1
                code.append('"')
            elif c == "'":
                # A char literal ('x', '\n', '{') or a lifetime ('a): only the first closes two or three on.
                end = line.find("'", i + 2) if rest[1:2] == "\\" else (i + 2 if rest[2:3] == "'" else -1)
                if end < 0:
                    code.append(c)
                    i += 1
                else:
                    code.append("' '")
                    i = end + 1
            else:
                code.append(c)
                i += 1
        out.append(("".join(code), " ".join(comment)))
    return out


def allowed(comment):
    m = ALLOW.search(comment)
    if not m:
        return set(), True
    return {r.strip() for r in m.group(1).split(",")}, bool(m.group(2).strip())


class Hits(defaultdict):
    """{rule: [(line number, line)]} for one file, how many lines of code it has outside its tests, and its
    allows: (line, rules, reason, the methods of the element it exempts or None)."""

    def __init__(self):
        super().__init__(list)
        self.code_lines = 0
        self.allows = []


# Component rules (element shapes, functions named like components) want the design-system item an allow
# considered, so whoever comes next finds both the one-off and why the kit didn't fit it.
def needs_reference(rule):
    return rule["scope"] in ("chain", "fn") and rule["name"] != LIST_RULE


_ITEMS = None


def catalog_items():
    global _ITEMS
    if _ITEMS is None:
        _ITEMS = set(re.findall(r"`(?:ui|palette|motion)::([A-Za-z_][A-Za-z0-9_]*)`", catalog()))
    return _ITEMS


def weak(reason):
    words = re.findall(r"[a-z0-9'-]+", reason.lower())
    return len(words) < 3 or all(w in WEAK_WORDS for w in words)


def scan(path, text, rules):
    hits = Hits()
    lines = text.splitlines()
    parsed = split_code(lines)
    in_design_system = path.startswith(EXEMPT)
    depth, impl_stack = 0, []  # impl_stack: (type, depth its body opened at)
    prev_allow, file_allow = set(), set()
    test_attr, test_depth = False, None  # test_depth: the depth a `#[cfg(test)] mod` body opened at
    code_lines, skips, test_lines = [], {}, set()
    component_rules = {r["name"] for r in rules if needs_reference(r)}
    for n, (code, comment) in enumerate(parsed, 1):
        here, reason_given = allowed(comment)
        if here and not reason_given:
            hits[NO_REASON].append((n, lines[n - 1]))
        if here and reason_given:
            reason = ALLOW.search(comment).group(2).strip()
            if weak(reason):
                hits[WEAK_REASON].append((n, lines[n - 1]))
            named = set(re.findall(r"\bui::([A-Za-z_][A-Za-z0-9_]*)", reason))
            if here & component_rules and not named & catalog_items():
                hits[NO_REFERENCE].append((n, lines[n - 1]))
            if LIST_RULE in here and not BOUND.search(reason):
                hits[NO_BOUND].append((n, lines[n - 1]))
            hits.allows.append((n, here, reason, None))
        if n <= HEAD_LINES and reason_given:
            file_allow |= here
        skip = here | prev_allow
        skips[n] = skip
        # A comment-only line passes its allow to the next line.
        prev_allow = here if not code.strip() else set()
        if test_depth is None and test_attr and MOD_OPEN.match(code):
            test_depth = depth + 1
        if test_depth is not None or (test_attr and code.strip()):
            test_lines.add(n)
        if code.strip():
            starts_tests = bool(TEST_ATTR.search(code))
            test_attr = starts_tests or (test_attr and code.strip().startswith("#["))
            if test_depth is None and not starts_tests:
                code_lines.append((n, lines[n - 1]))
        if m := IMPL_OPEN.match(code):
            impl_stack.append((m.group(1), depth + 1))
        inside = {t for t, _ in impl_stack}
        for rule in rules:
            if rule["name"] in skip or rule["scope"] in WHOLE_FILE or in_design_system:
                continue
            if rule["scope"] == "source":
                # Matched on the line as written (strings included): what a URL asks for lives in a string.
                if code.strip() and n not in test_lines:
                    for _ in rule["pattern"].finditer(lines[n - 1]):
                        hits[rule["name"]].append((n, lines[n - 1]))
                continue
            scope = rule["scope"]
            if scope.startswith("impl:") and scope[5:] not in inside:
                continue
            for _ in rule["pattern"].finditer(code):
                hits[rule["name"]].append((n, lines[n - 1]))
        depth += code.count("{") - code.count("}")
        while impl_stack and depth < impl_stack[-1][1]:
            impl_stack.pop()
        if test_depth is not None and depth < test_depth:
            test_depth = None
    hits.code_lines = len(code_lines)
    if not in_design_system:
        scan_elements(hits, lines, parsed, skips, rules)
        scan_performance(hits, lines, parsed, skips, rules, test_lines)
    for rule in rules:
        # Over its limit, every line of code counts, so the baseline holds the file at its size.
        if rule["scope"] == "file" and rule["name"] not in file_allow and len(code_lines) > rule["pattern"]:
            hits[rule["name"]] = code_lines
    return hits


def scan_elements(hits, lines, parsed, skips, rules):
    """The chain and fn rules: whole elements and functions, matched once the file is read."""
    code = "\n".join(c for c, _ in parsed)
    allow_at = {a[0]: i for i, a in enumerate(hits.allows)}
    chain_rules = [r for r in rules if r["scope"] == "chain"]
    for chain in ui_chains.chains(code) if chain_rules else []:
        for rule in chain_rules:
            if not ui_chains.matches(rule["pattern"], chain):
                continue
            if rule["name"] in skips.get(chain.line, set()):
                # Keep what the exempted element is made of, to tell a second copy of it.
                for at in (chain.line, chain.line - 1):
                    if at in allow_at and rule["name"] in hits.allows[allow_at[at]][1]:
                        n, names, reason, _ = hits.allows[allow_at[at]]
                        hits.allows[allow_at[at]] = (n, names, reason, chain.methods)
                        break
                continue
            hits[rule["name"]].append((chain.line, lines[chain.line - 1]))
    fn_rules = [r for r in rules if r["scope"] == "fn"]
    for line, name, body in ui_chains.functions(code) if fn_rules else []:
        uses = ui_chains.ui_calls(body) - NOT_COMPONENTS
        for rule in fn_rules:
            if rule["pattern"].search(name) and not uses and rule["name"] not in skips.get(line, set()):
                hits[rule["name"]].append((line, lines[line - 1]))


def nearest(rule, new_lines):
    """What the design system already has for a new lookalike: ui_find's best matches for the rule and the code."""
    kind = rule.replace("custom-", "").replace("component", "")
    found = {}
    for _, line in new_lines[:3]:
        for score, name, at, first, options in ui_find.search(f"{kind} {line}", 3):
            found.setdefault(name, (at, first, options))
    if not found:
        return ""
    out = "\n  Nearest in the design system (tools/ui_find.py) — use one, or give it the option you need:"
    for name, (at, first, options) in list(found.items())[:4]:
        out += f"\n    {name} ({at}) {first}" + (f" Options: {', '.join('.' + o + '()' for o in options)}." if options else "")
    return out


def new_components(ref):
    """Design-system items added since `ref`. Each needs a description (its catalog line) and a `// Keywords:` line
    (what ui_find searches), and the report lists what already exists near it, in case an option would have done."""
    import subprocess
    problems, notes = [], []
    names_then = set()
    for path in ["src/ui.rs"] + [f"src/ui/{f}" for f in os.listdir(os.path.join(ROOT, "src/ui")) if f.endswith(".rs")]:
        then = git_file(ref, path) or ""
        names_then |= {m.group(2) for l in then.splitlines() if (m := ui_find.ITEM.match(l))}
    for name, at, doc, keys, options in ui_find.items():
        raw = name.split("::", 1)[1]
        if not name.startswith("ui::") or raw in names_then or raw.isupper():
            continue
        if not doc:
            problems.append(f"{name} ({at}) is new and has no description: add the one-sentence comment above it (it's its catalog line).")
        if not keys and raw[:1].islower():
            problems.append(f"{name} ({at}) is new and has no `// Keywords:` line: add the words someone would search for it by (tools/ui_find.py).")
        near = [r for r in ui_find.search(f"{raw} {doc} {keys}", 8) if r[1].split("::", 1)[1] in names_then][:3]
        if near and raw[:1].islower():
            notes.append(f"{name} is new. Close to it: " + "; ".join(f"{n} ({a})" for _, n, a, _, _ in near) + ". If one of these could take an option instead, prefer that.")
    return problems, notes


_CORPUS = None


def corpus():
    """The app's code, strings and comments blanked, for rules that look at how something is used anywhere."""
    global _CORPUS
    if _CORPUS is None:
        _CORPUS = "\n".join(
            c for rel in sources() for c, _ in split_code(open(os.path.join(ROOT, rel), encoding="utf-8").read().splitlines())
        )
    return _CORPUS


def scan_performance(hits, lines, parsed, skips, rules, test_lines):
    """Rules that need to know where code runs (render path, UI thread, a high-rate handler) or how a collection is
    used across the app."""
    code = "\n".join(c for c, _ in parsed)
    found = ui_perf.scoped(code, [r for r in rules if r["scope"] in ("render", "ui-thread")], test_lines)
    for rule in rules:
        if rule["scope"] == "hot-notify":
            found += [(n, rule["name"]) for n in ui_perf.hot_notifies(code, test_lines)]
        if rule["scope"] == "probe":
            found += [(n, rule["name"]) for n in ui_perf.unprobed(code, test_lines)]
        if rule["scope"] == "growth":
            found += [(n, rule["name"]) for n in ui_perf.unbounded(code, corpus(), test_lines)]
    for n, name in found:
        if name not in skips.get(n, set()):
            hits[name].append((n, lines[n - 1]))


def list_stats():
    """How many of the app's scrolling lists are virtualized, for the unvirtualized-list report."""
    virtual = 0
    for rel in sources():
        if not rel.startswith(EXEMPT):
            code = "\n".join(c for c, _ in split_code(open(os.path.join(ROOT, rel), encoding="utf-8").read().splitlines()))
            virtual += len(VIRTUAL.findall(code))
    rule = [r for r in load_rules() if r["name"] == LIST_RULE]
    plain = sum(len(h.get(LIST_RULE, [])) for h in scan_tree(rule).values()) if rule else 0
    return f"App lists: {virtual + plain} scroll, {virtual} virtualized; {plain} build every row."


def sources():
    for base, _, files in os.walk(os.path.join(ROOT, "src")):
        for f in sorted(files):
            if f.endswith(".rs"):
                rel = os.path.relpath(os.path.join(base, f), ROOT).replace(os.sep, "/")
                yield rel


def scan_tree(rules):
    return {rel: scan(rel, open(os.path.join(ROOT, rel), encoding="utf-8").read(), rules) for rel in sorted(sources())}


def load_baseline():
    counts = Counter()
    path = os.path.join(ROOT, BASELINE)
    if os.path.exists(path):
        for line in open(path, encoding="utf-8"):
            if line.startswith("#") or not line.strip():
                continue
            f, r, c = line.rstrip("\n").split("\t")
            counts[(f, r)] = int(c)
    return counts


def over(rel, hits, baseline):
    """Rules this file now breaks more often than its baseline allows: (rule, count, allowed)."""
    return [(r, len(h), baseline[(rel, r)]) for r, h in sorted(hits.items()) if len(h) > baseline[(rel, r)]]


def added_lines(rel, rule_hits):
    """The hits on lines the last commit didn't have, so a report names the new lines, not the old debt."""
    try:
        import subprocess
        before = subprocess.run(["git", "-C", ROOT, "show", f"HEAD:{rel}"], capture_output=True, text=True, encoding="utf-8", errors="replace").stdout.splitlines()
    except OSError:
        before = []
    old = Counter(l.strip() for l in before)
    new = []
    # One entry a line, however many hits it holds.
    for n, line in dict(rule_hits).items():
        if old[line.strip()] > 0:
            old[line.strip()] -= 1
        else:
            new.append((n, line))
    return new or list(dict(rule_hits).items())


def fixes(rules):
    return {r["name"]: r["fix"] for r in rules} | {
        NO_REASON: "Say why after the colon: `lint-allow(rule): <reason>`.",
        WEAK_REASON: "Say what about this case the rule doesn't fit, in a few words someone could act on later.",
        NO_BOUND: "Say how big the list can get (\"a few dozen at most\", \"one per open call\"), so whoever grows it knows to virtualize it.",
        NO_REFERENCE: "Name the design-system item you tried and what it lacks: `lint-allow(custom-button): ui::chip has no cycling value`. Better still, give that item the option and use it.",
    }


def file_report(rel, hits, baseline, rules):
    """(blocking problems, warning) for one file."""
    fix = fixes(rules)
    limits = {r["name"]: r["pattern"] for r in rules if r["scope"] == "file"}
    problems, warnings = [], []
    for rule, count, allowed_count in over(rel, hits, baseline):
        if rule in limits:
            problems.append(f"{rule} in {rel}: {count} lines of code, allowed {allowed_count or limits[rule]}. {fix[rule]}")
            continue
        lines = "\n".join(f"  {rel}:{n}: {l.strip()}" for n, l in added_lines(rel, hits[rule])[:20])
        problems.append(f"{rule} in {rel}: {count}, allowed {allowed_count}. {fix[rule]}")
        if rule == LIST_RULE:
            problems[-1] += " " + list_stats()
        if any(r["name"] == rule and needs_reference(r) for r in rules):
            problems[-1] += nearest(rule, added_lines(rel, hits[rule]))
        problems[-1] += f"\n{lines}"
    debt = ", ".join(f"{baseline[(rel, r)]} {r}" for r in fix if baseline[(rel, r)] and r not in limits)
    if debt:
        warnings.append(
            f"Careful: {rel} still has {debt} from before the rules. Don't add to them; migrate the lines you touch "
            f"(src/ui/CATALOG.md has the replacements). A line that can't comply takes `// lint-allow(<rule>): <reason>`."
        )
    for rule, limit in limits.items():
        if hits.code_lines >= limit * WARN_AT:
            cap = baseline[(rel, rule)] or limit
            warnings.append(f"Careful: {rel} has {hits.code_lines} lines of code; it may not pass {cap}. {fix[rule]}")
    return problems, " ".join(warnings)


def summary(tree, rules):
    totals, by_file = Counter(), Counter()
    limits = {r["name"] for r in rules if r["scope"] == "file"}
    for rel, hits in tree.items():
        for r, h in hits.items():
            # A file-wide rule counts files over the limit, not their lines.
            totals[r] += 1 if r in limits else len(h)
            by_file[rel] += 0 if r in limits else len(h)
    fix = fixes(rules)
    out = ["Hand-written UI values and app-wide redraws left (warnings; each count may only fall):"]
    out += [f"  {totals[r]:>5}  {r}: {fix[r]}" for r in fix if totals[r]]
    out.append(list_stats())
    out.append("Most in:")
    out += [f"  {c:>5}  {f}" for f, c in by_file.most_common(12)]
    return "\n".join(out)


# The catalog: each public item of the design system with the first sentence of the comment above it.
ITEM = re.compile(r"^pub (?:fn|const|struct|enum|trait|static) ([A-Za-z_][A-Za-z0-9_]*)")
REEXPORT = re.compile(r"^pub use [a-z_]+::\{([^}]*)\}")


def catalog():
    ui = open(os.path.join(ROOT, "src/ui.rs"), encoding="utf-8").read().splitlines()
    exported = {n.strip().split(" ")[-1] for l in ui if (m := REEXPORT.match(l)) for n in m.group(1).split(",") if n.strip()}
    subs = sorted(f"src/ui/{f}" for f in os.listdir(os.path.join(ROOT, "src/ui")) if f.endswith(".rs") and f != "prelude.rs")
    modules = [("ui", "src/ui.rs", None)] + [("ui", p, exported) for p in subs] + [("palette", "src/palette.rs", None), ("motion", "src/motion.rs", None)]
    out = [
        "<!-- Generated by tools/ui_lint.py from the design system's source; regenerate with `python3 tools/ui_lint.py --update` -->",
        "# Design system catalog",
        "",
        "Everything here exists. Use it before writing UI by hand; extend it (in src/ui) when it lacks something.",
    ]
    for prefix, path, only in modules:
        lines = open(os.path.join(ROOT, path), encoding="utf-8").read().splitlines()
        section = []
        for i, line in enumerate(lines):
            m = ITEM.match(line)
            if not m or (only is not None and m.group(1) not in only):
                continue
            above = []
            for prev in reversed(lines[:i]):
                t = prev.strip()
                if t.startswith("#["):
                    continue
                if not t.startswith("//"):
                    break
                above.append(t.lstrip("/").strip())
            text = " ".join(reversed(above))
            first = (text.split(". ")[0] + ("." if ". " in text else "")).strip()[:140]
            section.append(f"- `{prefix}::{m.group(1)}`" + (f" — {first}" if first else ""))
        if section:
            out += ["", f"## {path}", ""] + section
    return "\n".join(out) + "\n"


def one_offs(tree, rules):
    """src/ui/ONE_OFFS.md: every allowed exception by rule, with its reason, so the next one-off of a kind finds
    the earlier ones (and grows the design system instead of a third copy)."""
    by_rule = defaultdict(list)
    for rel, hits in tree.items():
        for _, names, reason, _ in hits.allows:
            for name in sorted(names):
                by_rule[name].append(f"- `{rel}` — {reason}")
    fix = fixes(rules)
    out = [
        "<!-- Generated by tools/ui_lint.py from the lint-allow comments in src/; regenerate with `python3 tools/ui_lint.py --update` -->",
        "# One-offs",
        "",
        "Every place app code was let off a rule, and why. Before writing an exception, look here for the same kind of",
        "thing: a second copy of a one-off is a missing design-system component or variant, and belongs in src/ui/.",
    ]
    for name in [r["name"] for r in rules if by_rule[r["name"]]]:
        out += ["", f"## {name}", "", f"_{fix[name]}_", ""] + sorted(by_rule[name])
    return "\n".join(out) + "\n"


def duplicates(tree, rules):
    """Allowed one-offs of one component rule that are made of nearly the same methods: one component built twice."""
    # Only the component rules: two lists or margins alike aren't a component built twice.
    component = {r["name"] for r in rules if r["scope"] == "chain" and needs_reference(r)}
    seen, problems = defaultdict(list), []
    for rel, hits in sorted(tree.items()):
        for n, names, reason, methods in hits.allows:
            if methods is None:
                continue
            for name in names & component:
                for other_rel, other_n, other in seen[name]:
                    if ui_chains.similarity(methods, other) >= DUPLICATE_AT:
                        problems.append(
                            f"{name}: {rel}:{n} is the same element as {other_rel}:{other_n} (allowed twice). Two copies make a"
                            " component: move it into src/ui/ (or give the closest ui:: item the option) and use it in both."
                        )
                seen[name].append((rel, n, methods))
    return problems


def check(rules):
    tree, baseline = scan_tree(rules), load_baseline()
    problems = [p for rel, hits in tree.items() for p in file_report(rel, hits, baseline, rules)[0]]
    problems += duplicates(tree, rules)
    if not os.path.exists(os.path.join(ROOT, ONE_OFFS)) or open(os.path.join(ROOT, ONE_OFFS), encoding="utf-8").read() != one_offs(tree, rules):
        problems.append(f"{ONE_OFFS} is stale: run `python3 tools/ui_lint.py --update`.")
    counts = Counter({(rel, r): len(h) for rel, hits in tree.items() for r, h in hits.items()})
    fell = [f"{f}:{r}" for (f, r), c in baseline.items() if counts[(f, r)] < c]
    if fell:
        problems.append(f"Counts fell (good): lock them in with `python3 tools/ui_lint.py --update`: {', '.join(fell)}")
    if open(os.path.join(ROOT, CATALOG), encoding="utf-8").read() != catalog():
        problems.append("src/ui/CATALOG.md is stale: run `python3 tools/ui_lint.py --update`.")
    return problems


def update(rules):
    tree, baseline = scan_tree(rules), load_baseline()
    counts = Counter({(rel, r): len(h) for rel, hits in tree.items() for r, h in hits.items()})
    # A rule the baseline has never recorded (a new one) records today's counts once; every other count only falls.
    known = {r for _, r in baseline}
    rose = [f"{f}:{r} {c} > {baseline[(f, r)]}" for (f, r), c in counts.items() if c > baseline[(f, r)] and r in known]
    if rose:
        sys.exit("The baseline only falls; these rose (fix them instead):\n  " + "\n  ".join(rose))
    with open(os.path.join(ROOT, BASELINE), "w", encoding="utf-8") as f:
        f.write("# Left from before the rules (tools/ui_lint.py). Only ever lower: `python3 tools/ui_lint.py --update`.\n")
        for (rel, r), c in sorted(counts.items()):
            if c:
                f.write(f"{rel}\t{r}\t{c}\n")
    open(os.path.join(ROOT, CATALOG), "w", encoding="utf-8").write(catalog())
    open(os.path.join(ROOT, ONE_OFFS), "w", encoding="utf-8").write(one_offs(tree, rules))


def git_file(ref, rel):
    import subprocess
    out = subprocess.run(["git", "-C", ROOT, "show", f"{ref}:{rel}"], capture_output=True, text=True, encoding="utf-8")
    return out.stdout if out.returncode == 0 else None


def against(ref):
    """CI: the ratchet held against `ref` (the PR's base). No rule was dropped from src/ui/rules.tsv, and no rule's
    total in src/ui/rules.baseline rose: the baseline is lowered by fixing code, never raised to let code in.
    Totals, not per-file counts, so moving code between files is free."""
    problems = []
    base_rules, base_baseline = git_file(ref, RULES), git_file(ref, BASELINE)
    if base_rules is None:
        return []
    names = lambda text: {l.split("\t")[0] for l in text.splitlines() if l.strip() and not l.startswith("#")}
    dropped = names(base_rules) - names(open(os.path.join(ROOT, RULES), encoding="utf-8").read())
    if dropped:
        problems.append(f"Rules dropped from {RULES}: {', '.join(sorted(dropped))}. Rules are only ever added or tightened.")
    before, now = _sum(base_baseline or ""), _sum(open(os.path.join(ROOT, BASELINE), encoding="utf-8").read())
    for rule, count in sorted(now.items()):
        if rule in before and count > before[rule]:
            problems.append(f"{BASELINE}: {rule} rose from {before[rule]} to {count} against {ref}. Fix the new hits instead.")
    return problems


def _sum(text):
    totals = Counter()
    for line in text.splitlines():
        if line.strip() and not line.startswith("#"):
            _, rule, count = line.split("\t")
            totals[rule] += int(count)
    return totals


def hook(event, rules):
    """Claude Code hook: before an edit, the file's warning; after it, block on anything new."""
    payload = json.load(sys.stdin)
    path = payload.get("tool_input", {}).get("file_path", "")
    rel = os.path.relpath(path, ROOT).replace(os.sep, "/") if path else ""
    if not rel.startswith("src/") or not rel.endswith(".rs") or not os.path.exists(path):
        return 0
    hits = scan(rel, open(path, encoding="utf-8").read(), rules)
    problems, warning = file_report(rel, hits, load_baseline(), rules)
    if event == "pre":
        if warning:
            print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": warning}}))
        return 0
    if problems:
        print(json.dumps({"decision": "block", "reason": "This edit breaks the codebase's rules (tools/ui_lint.py, CLAUDE.md). Fix it now:\n\n" + "\n\n".join(problems)}))
    return 0


def main(args):
    rules = load_rules()
    if args[:1] == ["--hook"]:
        return hook(args[1], rules)
    if args == ["--update"]:
        return update(rules)
    if args[:1] == ["--against"] and len(args) == 2:
        problems = against(args[1])
        new, notes = new_components(args[1])
        problems += new
        for note in notes:
            print(note)
        print("\n\n".join(problems) if problems else f"ui_lint: the ratchet held against {args[1]}.")
        return 1 if problems else 0
    if args == ["--check"]:
        problems = check(rules)
        print("\n\n".join(problems) if problems else "ui_lint: nothing new.")
        return 1 if problems else 0
    if args:
        baseline, failed = load_baseline(), False
        for path in args:
            rel = os.path.relpath(os.path.abspath(path), ROOT).replace(os.sep, "/")
            problems, warning = file_report(rel, scan(rel, open(path, encoding="utf-8").read(), rules), baseline, rules)
            print("\n\n".join(problems + [warning]).strip() or f"{rel}: clean")
            failed |= bool(problems)
        return 1 if failed else 0
    print(summary(scan_tree(rules), rules))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
