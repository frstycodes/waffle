# Finds the design-system component for what you're about to build, before you build it.
#
#   py -3 tools/ui_find.py round button over video
#   py -3 tools/ui_find.py "avatar with a dot" --all      every match, not just the best few
#
# Searches every public item of the design system (src/ui.rs, src/ui/, palette, motion): its name, its whole doc
# comment, its `// Keywords:` line and its builder methods (`.small()`, `.count(n)`), so asking for an option finds
# the component that has it. Then the one-offs app code was let off a rule for (src/ui/ONE_OFFS.md), so a second
# copy of a one-off is found before it's written. tools/ui_lint.py runs the same search on every new component
# and puts the nearest matches in its block message.
import os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ITEM = re.compile(r"^pub (fn|const|struct|enum|trait|static) ([A-Za-z_][A-Za-z0-9_]*)")
IMPL = re.compile(r"^impl(?:<[^>]*>)? ([A-Z][A-Za-z0-9_]*)\b[^{]*\{")
METHOD = re.compile(r"^    pub fn ([a-z_][a-z0-9_]*)")
KEYWORDS = re.compile(r"^Keywords:\s*(.*)$", re.I)
# Words people reach for that the design system names otherwise.
SYNONYMS = {
    "clickable": "button", "btn": "button", "cta": "button", "action": "button",
    "pill": "chip", "token": "chip", "filter": "chip", "toggle": "switch",
    "label": "badge", "counter": "count", "number": "count", "notification": "count", "dot": "mark", "indicator": "mark",
    "face": "avatar", "picture": "avatar", "pfp": "avatar", "icon": "tile", "server": "tile", "guild": "tile",
    "presence": "status", "online": "status",
    "modal": "dialog", "sheet": "dialog", "popover": "surface", "popup": "surface", "menu": "surface", "card": "surface",
    "panel": "surface", "overlay": "floating", "flyout": "floating",
    "separator": "divider", "rule": "divider", "line": "divider",
    "input": "field", "textbox": "field", "textfield": "field", "search": "search",
    "empty": "empty", "placeholder": "skeleton", "loading": "skeleton", "spinner": "progress", "bar": "progress",
    "tooltip": "tip", "hint": "tip", "shortcut": "kbd", "key": "kbd", "keycap": "kbd",
    "list": "row", "item": "row", "horizontal": "row", "vertical": "col", "column": "col",
    "truncate": "line", "ellipsis": "line", "scroll": "scroll", "notice": "callout", "alert": "callout", "banner": "callout",
    "tab": "tabs", "segment": "segmented", "radio": "radio", "checkbox": "check", "tick": "check",
}
STOP = {"a", "an", "the", "with", "for", "of", "on", "in", "to", "and", "or", "that", "is", "it", "its", "as", "at", "by", "from", "this", "one"}


def words(text):
    out = []
    for w in re.findall(r"[a-z0-9]+", re.sub(r"([a-z])([A-Z])", r"\1 \2", text).replace("_", " ").lower()):
        if w in STOP or len(w) < 2:
            continue
        for stem in (w, w[:-1] if w.endswith("s") and len(w) > 3 else None, w[:-3] if w.endswith("ing") and len(w) > 5 else None):
            if stem:
                out.append(stem)
        if w in SYNONYMS:
            out.append(SYNONYMS[w])
    return out


def items():
    """(qualified name, path:line, doc, keywords, methods) for each public design-system item."""
    paths = ["src/ui.rs"] + sorted(f"src/ui/{f}" for f in os.listdir(os.path.join(ROOT, "src/ui")) if f.endswith(".rs") and f != "prelude.rs")
    paths += ["src/palette.rs", "src/motion.rs"]
    found, methods = [], {}
    for path in paths:
        prefix = {"src/palette.rs": "palette", "src/motion.rs": "motion"}.get(path, "ui")
        lines = open(os.path.join(ROOT, path), encoding="utf-8").read().splitlines()
        owner = None
        for i, line in enumerate(lines):
            if m := IMPL.match(line):
                owner = m.group(1)
            elif line.startswith("}"):
                owner = None
            elif owner and (m := METHOD.match(line)):
                methods.setdefault(owner, []).append(m.group(1))
            if not (m := ITEM.match(line)):
                continue
            doc, keys = [], []
            for prev in reversed(lines[:i]):
                t = prev.strip()
                if t.startswith("#["):
                    continue
                if not t.startswith("//"):
                    break
                text = t.lstrip("/").strip()
                if k := KEYWORDS.match(text):
                    keys.append(k.group(1))
                else:
                    doc.append(text)
            found.append([f"{prefix}::{m.group(2)}", f"{path}:{i + 1}", " ".join(reversed(doc)), " ".join(keys), m.group(2)])
    # A builder's options belong to its constructor too: `ui::badged` finds `.count`, `.status`.
    return [(name, at, doc, keys, list(dict.fromkeys(methods.get(raw, []) + (methods.get(snake_to_camel(raw), []) if raw != snake_to_camel(raw) else [])))) for name, at, doc, keys, raw in found]


def snake_to_camel(name):
    return "".join(p[:1].upper() + p[1:] for p in name.split("_"))


def one_offs():
    path = os.path.join(ROOT, "src/ui/ONE_OFFS.md")
    if not os.path.exists(path):
        return []
    out, rule = [], None
    for line in open(path, encoding="utf-8"):
        if line.startswith("## "):
            rule = line[3:].strip()
        elif line.startswith("- ") and rule:
            out.append((rule, line[2:].strip()))
    return out


def score(query, name, doc, keys, methods):
    name_words, key_words, doc_words, method_words = set(words(name)), set(words(keys)), set(words(doc)), set(words(" ".join(methods)))
    total = 0.0
    for w in set(query):
        total += 6 * (w in name_words) + 4 * (w in key_words) + 2 * (w in method_words) + 1.5 * (w in doc_words)
    return total


def search(text, limit=5):
    """The best design-system matches for `text`: [(score, name, path:line, first sentence, options)]."""
    query = words(text)
    ranked = []
    for name, at, doc, keys, methods in items():
        s = score(query, name, doc, keys, methods)
        if s > 0:
            first = (doc.split(". ")[0] + ("." if ". " in doc else "")).strip()[:160]
            ranked.append((s, name, at, first, methods))
    ranked.sort(key=lambda r: -r[0])
    return ranked[:limit]


def search_one_offs(text, limit=3):
    query = set(words(text))
    hits = [(len(query & set(words(entry))), rule, entry) for rule, entry in one_offs()]
    return [(rule, entry) for s, rule, entry in sorted(hits, key=lambda h: -h[0]) if s >= 2][:limit]


def main(args):
    show_all = "--all" in args
    text = " ".join(a for a in args if a != "--all")
    if not text:
        print(__doc__ if False else open(__file__, encoding="utf-8").read().split("\nimport")[0].replace("# ", "").replace("#", ""))
        return 2
    results = search(text, limit=50 if show_all else 6)
    if not results:
        print(f"Nothing in the design system matches \"{text}\". Check src/ui/CATALOG.md by eye; if it's truly missing, add it to src/ui/ (CLAUDE.md, rule 2).")
    for s, name, at, first, methods in results:
        print(f"{name}  ({at})")
        if first:
            print(f"    {first}")
        if methods:
            print(f"    options: {', '.join('.' + m + '()' for m in methods)}")
    offs = search_one_offs(text)
    if offs:
        print("\nOne-offs of this kind already in app code (a second copy belongs in src/ui/):")
        for rule, entry in offs:
            print(f"  [{rule}] {entry}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
