# The performance and memory half of tools/ui_lint.py: rules that need to know where code runs (on the render
# path, on the UI thread, in a timer loop or a pointer-move handler) or how a collection is used across the app
# (grown but never shrunk). Each takes the file's code with strings and comments blanked (ui_lint.split_code) and
# returns hits as (line number, rule name).
import re

FN = re.compile(r"\bfn\s+([a-z_][a-z0-9_]*)\s*(?:<[^>{]*>)?\s*\(")
ELEMENT = re.compile(r"->\s*(Option<\s*)?(impl\s+IntoElement|impl\s+Element|AnyElement|Div|Stateful<Div>|Img|Svg|Stateful<Img>|(gpui_kit::)?(Div|Stateful<Div>|AnyElement)|AnimationElement<)")
UI_THREAD = re.compile(r"\b(cx|window)\s*:|Context<|&mut\s+App\b|&mut\s+Window\b")
# Calls whose closure runs later, not while the enclosing code runs: event handlers, tooltips, spawned work.
LATER = re.compile(r"\.(on_[a-z_]+|listener|tooltip|hoverable_tooltip|spawn|spawn_in|background_spawn|defer|defer_in|on_next_frame|observe|subscribe|subscribe_in|observe_in|update|update_in)\s*\(|cx\.listener\s*\(")
# Off the UI thread: closures handed to these run elsewhere.
OFF_THREAD = re.compile(r"(background_spawn|thread::spawn|spawn_blocking|background_executor\(\)\.spawn)\s*\(")
HIGH_RATE = re.compile(r"\.(on_mouse_move|on_scroll_wheel|on_drag_move|on_pinch)\s*\(")
TIMER_LOOP = re.compile(r"\bloop\s*\{")
TIMER = re.compile(r"Timer::after|\.timer\(|\btimer\(|Instant::now")
NOTIFY = re.compile(r"\bcx\.notify\(\)")
GUARD = re.compile(r"\bif\b|!=|==|\bchanged\b|\.ne\(|\breturn\b|\bmatch\b")


def line_of(code, i):
    return code.count("\n", 0, i) + 1


def arg_end(code, i):
    """Index just past the parenthesised group opening at code[i] == '('."""
    depth = 0
    for j in range(i, len(code)):
        if code[j] in "([{":
            depth += 1
        elif code[j] in ")]}":
            depth -= 1
            if depth == 0:
                return j + 1
    return len(code)


def blank(code, pattern):
    """`code` with the argument lists of `pattern`'s calls blanked (newlines kept), so their closures don't count."""
    out, last = [], 0
    for m in pattern.finditer(code):
        if m.start() < last:
            continue
        end = arg_end(code, m.end() - 1)
        out.append(code[last : m.end()])
        out.append(re.sub(r"[^\n]", " ", code[m.end() : end - 1]))
        last = end - 1
    out.append(code[last:])
    return "".join(out)


def functions(code):
    """(signature, body start, body end) for every fn with a body."""
    out = []
    for m in FN.finditer(code):
        brace, semi = code.find("{", m.end()), code.find(";", m.end())
        if brace < 0 or 0 <= semi < brace:
            continue
        depth = 0
        for j in range(brace, len(code)):
            depth += {"{": 1, "}": -1}.get(code[j], 0)
            if depth == 0:
                break
        out.append((code[m.start() : brace], brace, j + 1))
    return out


def scoped(code, rules, test_lines):
    """Hits for `render` rules (inside element-building functions, outside their handlers) and `ui-thread` rules
    (inside functions that take cx or window, outside work handed to another thread)."""
    hits = set()
    render = [r for r in rules if r["scope"] == "render"]
    ui_thread = [r for r in rules if r["scope"] == "ui-thread"]
    for sig, start, end in functions(code):
        body = code[start:end]
        if render and ELEMENT.search(sig):
            now = blank(body, LATER)
            for rule in render:
                hits |= {(line_of(code, start + m.start()), rule["name"]) for m in rule["pattern"].finditer(now)}
        if ui_thread and UI_THREAD.search(sig):
            here = blank(body, OFF_THREAD)
            for rule in ui_thread:
                hits |= {(line_of(code, start + m.start()), rule["name"]) for m in rule["pattern"].finditer(here)}
    return sorted((n, r) for n, r in hits if n not in test_lines)


def hot_notifies(code, test_lines):
    """`cx.notify()` in a pointer-move or scroll handler, or a timer loop, with no check that anything changed first."""
    spans = []
    for m in HIGH_RATE.finditer(code):
        spans.append((m.end(), arg_end(code, m.end() - 1)))
    for m in TIMER_LOOP.finditer(code):
        end = arg_end(code, m.end() - 1)
        if TIMER.search(code, m.end(), end):
            spans.append((m.end(), end))
    hits = set()
    for m in NOTIFY.finditer(code):
        for start, end in spans:
            if start <= m.start() < end and not GUARD.search(code, start, m.start()):
                hits.add(line_of(code, m.start()))
    return sorted(n for n in hits if n not in test_lines)


# Memory: a long-lived collection field that the app grows and never shrinks.
STRUCT = re.compile(r"\bstruct\s+[A-Z]\w*(?:<[^>{]*>)?\s*\{")
FIELD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*(.*)$")
COLLECTION = re.compile(r"\b(HashMap|HashSet|BTreeMap|BTreeSet|Vec|VecDeque|IndexMap|IndexSet)\s*<")


def collection_fields(code):
    """(line, name) of struct fields holding a collection."""
    out = []
    for m in STRUCT.finditer(code):
        end = arg_end(code, m.end() - 1)
        for i, raw in enumerate(code[m.end() : end].split("\n")):
            f = FIELD.match(raw)
            if f and COLLECTION.search(f.group(2)):
                out.append((line_of(code, m.end()) + i, f.group(1)))
    return out




# How each field name is used anywhere in the app, built once: {name: set of method names called on it}, plus the
# names that are ever reassigned or lent out mutably.
FIELD_CALL = re.compile(r"\.([a-z_][a-z0-9_]*)\s*\.\s*([a-z_][a-z0-9_]*)\s*\(")
FIELD_SET = re.compile(r"\.([a-z_][a-z0-9_]*)\s*=[^=]")
FIELD_LENT = re.compile(r"&mut\s+[a-z_][a-z0-9_.]*?\.([a-z_][a-z0-9_]*)\b")
GROWS = {"push", "push_back", "push_front", "insert", "extend", "entry", "append"}
SHRINKS = re.compile(r"^(remove|pop|pop_back|pop_front|truncate|clear|retain|drain|split_off|swap_remove|shift_remove|take|dedup|resize|shrink_to)")
_USES = {}


def uses(all_code):
    key = id(all_code)
    if key not in _USES:
        calls = {}
        for m in FIELD_CALL.finditer(all_code):
            calls.setdefault(m.group(1), set()).add(m.group(2))
        reset = {m.group(1) for m in FIELD_SET.finditer(all_code)} | {m.group(1) for m in FIELD_LENT.finditer(all_code)}
        _USES[key] = (calls, reset)
    return _USES[key]


def unbounded(code, all_code, test_lines):
    """Collection fields that some code grows and no code anywhere shrinks, resets or lends out mutably."""
    calls, reset = uses(all_code)
    hits = []
    for n, name in collection_fields(code):
        methods = calls.get(name, set())
        if n not in test_lines and methods & GROWS and name not in reset and not any(SHRINKS.match(m) for m in methods):
            hits.append(n)
    return hits


RENDER_IMPL = re.compile(r"impl(?:<[^>]*>)?\s+(?:gpui_kit::)?Render\s+for\s+\w+(?:<[^>{]*>)?\s*\{")
PROBE = re.compile(r"perf::probe\s*\(")


def unprobed(code, test_lines):
    """`impl Render` blocks whose render doesn't start by counting itself (perf.rs), so the opt-in counters would
    miss that view."""
    hits = []
    for m in RENDER_IMPL.finditer(code):
        r = code.find("fn render", m.end())
        if r < 0:
            continue
        body = code.find("{", r)
        # A render of a line or two (a drag ghost's empty div) has nothing to count.
        if code[body : arg_end(code, body)].count("\n") <= 3:
            continue
        first = code[body : body + 400].split("\n")[1:3]
        if not any(PROBE.search(l) for l in first) and line_of(code, m.start()) not in test_lines:
            hits.append(line_of(code, m.start()))
    return hits
