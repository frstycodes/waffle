# The element-level half of tools/ui_lint.py: finds each element's builder chain (`div().id(..).flex()…`) and
# the methods it applies, so a rule can match the shape of a hand-built component (a clickable, hovered,
# rounded div is a button) rather than a single line, and finds functions named like a component.
#
# A chain rule's pattern is `;`-separated clauses, all of which must hold:
#   a|b*     the chain calls one of these methods (`*` ends a prefix: `rounded*` is rounded, rounded_full …)
#   !a|b     it calls none of them
#   root:x   it starts at `x()` (div, ui::row …)
#   =TEXT    the chain's code contains TEXT (a token's name)
#   max:N    it calls at most N methods
import re

ROOTS = re.compile(r"(?<![A-Za-z0-9_:])(div|ui::row|ui::col|ui::line)\(\)")
METHOD = re.compile(r"\.\s*([a-z_][a-z0-9_]*)\s*(?:::<[^>]*>)?\s*\(")
# Methods whose closure argument restyles the same element: what it calls counts as the chain's own.
PASS_THROUGH = {"when", "when_some", "when_none", "when_else", "map"}
CLOSURE = re.compile(r"\|[^|]*\|\s*")


class Chain:
    def __init__(self, line, root, methods, text):
        self.line, self.root, self.methods, self.text = line, root, methods, text


def _arg_end(code, i):
    """Index just past the parenthesised argument list opening at code[i] == '('."""
    depth = 0
    for j in range(i, len(code)):
        if code[j] in "([{":
            depth += 1
        elif code[j] in ")]}":
            depth -= 1
            if depth == 0:
                return j + 1
    return len(code)


def _top_methods(code, start, end, out):
    """The methods called at the top level of code[start:end], following when/map closures inside."""
    i, depth = start, 0
    while i < end:
        c = code[i]
        if depth == 0 and c == ".":
            m = METHOD.match(code, i)
            if m and m.end() <= end:
                name = m.group(1)
                out.append(name)
                close = _arg_end(code, m.end() - 1)
                if name in PASS_THROUGH:
                    args = code[m.end() : close - 1]
                    for closure in CLOSURE.finditer(args):
                        _top_methods(code, m.end() + closure.end(), close - 1, out)
                i = close
                continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                return i
            depth -= 1
        elif depth == 0 and c in ",;":
            return i
        i += 1
    return i


def chains(code):
    """Every element chain in `code` (strings and comments already blanked), with the 1-based line it starts on."""
    out = []
    for m in ROOTS.finditer(code):
        methods = []
        end = _top_methods(code, m.end(), len(code), methods)
        out.append(Chain(code.count("\n", 0, m.start()) + 1, m.group(1), methods, code[m.start() : end]))
    return out


def compile_pattern(text):
    clauses = []
    for clause in (c.strip() for c in text.split(";") if c.strip()):
        if clause.startswith("root:"):
            clauses.append(("root", clause[5:]))
        elif clause.startswith("max:"):
            clauses.append(("max", int(clause[4:])))
        elif clause.startswith("="):
            clauses.append(("text", clause[1:]))
        elif clause.startswith("!"):
            clauses.append(("none", clause[1:].split("|")))
        else:
            clauses.append(("any", clause.split("|")))
    return clauses


def _has(methods, names):
    return any(m.startswith(n[:-1]) if n.endswith("*") else m == n for n in names for m in methods)


def matches(clauses, chain):
    for kind, arg in clauses:
        if kind == "root" and chain.root != arg:
            return False
        if kind == "max" and len(chain.methods) > arg:
            return False
        if kind == "text" and arg not in chain.text:
            return False
        if kind == "any" and not _has(chain.methods, arg):
            return False
        if kind == "none" and _has(chain.methods, arg):
            return False
    return True


FN = re.compile(r"\bfn\s+([a-z_][a-z0-9_]*)\s*(?:<[^>{]*>)?\s*\(")
# What an element-building function returns (maybe in an Option).
ELEMENT = re.compile(r"->\s*(Option<\s*)?(impl\s+IntoElement|impl\s+Element|AnyElement|Div|Stateful<Div>|Img|Svg|Stateful<Img>|(gpui_kit::)?(Div|Stateful<Div>|AnyElement)|AnimationElement<)")
# A function (`ui::menu_row(`) or a component struct's constructor (`ui::Button::new(`, `ui::Badge::count(`).
UI_CALL = re.compile(r"\bui::([a-z_][a-z0-9_]*|[A-Z][A-Za-z0-9]*(?=::[a-z_]+\s*\())\s*(?:::[a-z_]+\s*)?\(")


def functions(code):
    """(line, name, body) for every fn with a body that returns an element."""
    out = []
    for m in FN.finditer(code):
        brace = code.find("{", m.end())
        semi = code.find(";", m.end())
        if brace < 0 or (0 <= semi < brace):
            continue
        if not ELEMENT.search(code[m.end() : brace]):
            continue
        depth, j = 0, brace
        for j in range(brace, len(code)):
            depth += {"{": 1, "}": -1}.get(code[j], 0)
            if depth == 0:
                break
        out.append((code.count("\n", 0, m.start()) + 1, m.group(1), code[brace : j + 1]))
    return out


def ui_calls(body):
    return set(UI_CALL.findall(body))


def similarity(a, b):
    a, b = set(a), set(b)
    return len(a & b) / len(a | b) if a | b else 1.0
