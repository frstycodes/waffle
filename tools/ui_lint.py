# Counts what app code writes by hand that the design system, or a narrower redraw, already covers
# (src/ui/rules.tsv), and holds every file to its count in src/ui/rules.baseline. Counts only fall.
#
#   python3 tools/ui_lint.py              warnings: what's left, by rule and by file
#   python3 tools/ui_lint.py FILE...      warnings for those files; exit 1 if one went over its count
#   python3 tools/ui_lint.py --check      the whole tree and the catalog; exit 1 on anything new (cargo test runs this)
#   python3 tools/ui_lint.py --update     lower the baseline to today's counts and regenerate src/ui/CATALOG.md
#   python3 tools/ui_lint.py --hook pre|post   for .claude/settings.json: reads the hook's JSON on stdin
#
# A line is exempt from a rule when it, or the line above it, carries `lint-allow(<rule>): <reason>` in a
# comment; several rules go comma-separated. An allow without a reason is itself reported.
import json, os, re, sys
from collections import Counter, defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RULES = "src/ui/rules.tsv"
BASELINE = "src/ui/rules.baseline"
CATALOG = "src/ui/CATALOG.md"
# The design system itself: where the raw values live.
EXEMPT = ("src/ui.rs", "src/ui/", "src/palette.rs", "src/motion.rs", "src/icons.rs")
ALLOW = re.compile(r"lint-allow\(([a-z0-9_, -]+)\)\s*:?\s*(.*)")
NO_REASON = "allow-without-reason"
IMPL_OPEN = re.compile(r"^\s*impl\b[^{]*\b(\w+)\s*\{")
RAW_STRING = re.compile(r'r(#*)"')


def load_rules():
    rules = []
    for line in open(os.path.join(ROOT, RULES), encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        name, pattern, scope, fix = line.rstrip("\n").split("\t")
        rules.append({"name": name, "pattern": re.compile(pattern), "scope": scope, "fix": fix})
    return rules


def split_code(lines):
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


def scan(path, text, rules):
    """{rule: [(line number, line)]} for one file."""
    hits = defaultdict(list)
    lines = text.splitlines()
    parsed = split_code(lines)
    depth, impl_stack = 0, []  # impl_stack: (type, depth its body opened at)
    prev_allow = set()
    for n, (code, comment) in enumerate(parsed, 1):
        here, reason_given = allowed(comment)
        if here and not reason_given:
            hits[NO_REASON].append((n, lines[n - 1]))
        skip = here | prev_allow
        # A comment-only line passes its allow to the next line.
        prev_allow = here if not code.strip() else set()
        if m := IMPL_OPEN.match(code):
            impl_stack.append((m.group(1), depth + 1))
        inside = {t for t, _ in impl_stack}
        for rule in rules:
            if rule["name"] in skip:
                continue
            scope = rule["scope"]
            if scope.startswith("impl:") and scope[5:] not in inside:
                continue
            for _ in rule["pattern"].finditer(code):
                hits[rule["name"]].append((n, lines[n - 1]))
        depth += code.count("{") - code.count("}")
        while impl_stack and depth < impl_stack[-1][1]:
            impl_stack.pop()
    return hits


def sources():
    for base, _, files in os.walk(os.path.join(ROOT, "src")):
        for f in sorted(files):
            if f.endswith(".rs"):
                rel = os.path.relpath(os.path.join(base, f), ROOT).replace(os.sep, "/")
                if not rel.startswith(EXEMPT):
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
        before = subprocess.run(["git", "-C", ROOT, "show", f"HEAD:{rel}"], capture_output=True, text=True).stdout.splitlines()
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
    return {r["name"]: r["fix"] for r in rules} | {NO_REASON: "Say why after the colon: `lint-allow(rule): <reason>`."}


def file_report(rel, hits, baseline, rules):
    """(blocking problems, warning) for one file."""
    fix = fixes(rules)
    problems = []
    for rule, count, allowed_count in over(rel, hits, baseline):
        lines = "\n".join(f"  {rel}:{n}: {l.strip()}" for n, l in added_lines(rel, hits[rule])[:20])
        problems.append(f"{rule} in {rel}: {count}, allowed {allowed_count}. {fix[rule]}\n{lines}")
    debt = ", ".join(f"{baseline[(rel, r)]} {r}" for r in fix if baseline[(rel, r)])
    warning = (
        f"Careful: {rel} still has {debt} from before the rules. Don't add to them; migrate the lines you touch "
        f"(src/ui/CATALOG.md has the replacements). A line that can't comply takes `// lint-allow(<rule>): <reason>`."
        if debt
        else ""
    )
    return problems, warning


def summary(tree, rules):
    totals, by_file = Counter(), Counter()
    for rel, hits in tree.items():
        for r, h in hits.items():
            totals[r] += len(h)
            by_file[rel] += len(h)
    fix = fixes(rules)
    out = ["Hand-written UI values and app-wide redraws left (warnings; each count may only fall):"]
    out += [f"  {totals[r]:>5}  {r}: {fix[r]}" for r in fix if totals[r]]
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


def check(rules):
    tree, baseline = scan_tree(rules), load_baseline()
    problems = [p for rel, hits in tree.items() for p in file_report(rel, hits, baseline, rules)[0]]
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
    first_run = not os.path.exists(os.path.join(ROOT, BASELINE))
    rose = [f"{f}:{r} {c} > {baseline[(f, r)]}" for (f, r), c in counts.items() if c > baseline[(f, r)]]
    if rose and not first_run:
        sys.exit("The baseline only falls; these rose (fix them instead):\n  " + "\n  ".join(rose))
    with open(os.path.join(ROOT, BASELINE), "w", encoding="utf-8") as f:
        f.write("# Left from before the rules (tools/ui_lint.py). Only ever lower: `python3 tools/ui_lint.py --update`.\n")
        for (rel, r), c in sorted(counts.items()):
            if c:
                f.write(f"{rel}\t{r}\t{c}\n")
    open(os.path.join(ROOT, CATALOG), "w", encoding="utf-8").write(catalog())


def hook(event, rules):
    """Claude Code hook: before an edit, the file's warning; after it, block on anything new."""
    payload = json.load(sys.stdin)
    path = payload.get("tool_input", {}).get("file_path", "")
    rel = os.path.relpath(path, ROOT).replace(os.sep, "/") if path else ""
    if not rel.startswith("src/") or not rel.endswith(".rs") or rel.startswith(EXEMPT) or not os.path.exists(path):
        return 0
    hits = scan(rel, open(path, encoding="utf-8").read(), rules)
    problems, warning = file_report(rel, hits, load_baseline(), rules)
    if event == "pre":
        if warning:
            print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": warning}}))
        return 0
    if problems:
        print(json.dumps({"decision": "block", "reason": "This edit adds hand-written UI values or app-wide redraws. Fix them now:\n\n" + "\n\n".join(problems)}))
    return 0


def main(args):
    rules = load_rules()
    if args[:1] == ["--hook"]:
        return hook(args[1], rules)
    if args == ["--update"]:
        return update(rules)
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
