# How tools/ui_lint.py and tools/ui_find.py style what they print: colour and marks in a terminal or a CI log,
# plain text everywhere else (the Claude edit hooks' JSON, a pipe, a file, NO_COLOR).
import os, shutil, sys


def _wants_color(stream):
    if os.environ.get("NO_COLOR"):
        return False
    if os.environ.get("FORCE_COLOR") or os.environ.get("GITHUB_ACTIONS"):
        return True
    return hasattr(stream, "isatty") and stream.isatty()


class Style:
    def __init__(self, color):
        self.color = color

    def _wrap(self, code, text):
        return f"\033[{code}m{text}\033[0m" if self.color else str(text)

    def bold(self, t):
        return self._wrap("1", t)

    def dim(self, t):
        return self._wrap("2", t)

    def red(self, t):
        return self._wrap("31", t)

    def green(self, t):
        return self._wrap("32", t)

    def yellow(self, t):
        return self._wrap("33", t)

    def blue(self, t):
        return self._wrap("34", t)

    def magenta(self, t):
        return self._wrap("35", t)

    def cyan(self, t):
        return self._wrap("36", t)

    # Marks: Unicode where the stream can take it, ASCII otherwise.
    def mark(self, kind):
        fancy = {"fail": "✗", "ok": "✓", "warn": "!", "arrow": "→", "dot": "·", "bar": "│"}
        plain = {"fail": "x", "ok": "ok", "warn": "!", "arrow": "->", "dot": "-", "bar": "|"}
        return fancy[kind] if _UNICODE else plain[kind]


def _unicode_ok():
    try:
        "✗✓→·│".encode(sys.stdout.encoding or "ascii")
        return True
    except (UnicodeEncodeError, LookupError):
        return False


if os.name == "nt":
    # Turns on the console's ANSI handling (Windows Terminal and conhost both honour it after this).
    os.system("")
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    except (AttributeError, ValueError):
        pass

_UNICODE = _unicode_ok()
# The style for stdout; `plain()` switches it off (the hooks print JSON).
S = Style(_wants_color(sys.stdout))


def plain():
    S.color = False


def width():
    return max(60, min(140, shutil.get_terminal_size((100, 20)).columns))


def wrap(text, indent, style=lambda t: t):
    """`text` word-wrapped to the terminal for a caller already `indent` columns in; later lines indent to match."""
    room = width() - indent
    lines, line = [], ""
    for word in text.split():
        if line and len(line) + 1 + len(word) > room:
            lines.append(line)
            line = word
        else:
            line = f"{line} {word}" if line else word
    lines.append(line)
    return ("\n" + " " * indent).join(style(l) for l in lines)
