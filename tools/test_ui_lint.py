# Checks tools/ui_lint.py's counting: python3 tools/test_ui_lint.py (cargo test runs it).
import os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ui_lint

RULES = ui_lint.load_rules()


def counts(src):
    return {rule: len(hits) for rule, hits in ui_lint.scan("src/x.rs", src, RULES).items()}


def test():
    # Code counts; strings, raw strings and comments don't.
    assert counts('div().gap(px(8.)).child("px(8.)").child(r#"px(9.)"#) // px(4.)') == {"raw-px": 1}
    # A reasoned allow exempts its rule on its own line, or on the next line from a comment-only line.
    assert counts("w(px(3.)) // lint-allow(raw-px): measured from the image") == {}
    assert counts("// lint-allow(raw-px, raw-color): the scrubber's own art\nw(px(3.)).bg(rgb(0x123456))") == {}
    assert counts("w(px(3.))\n// lint-allow(raw-px): only the line after\nw(px(3.))\nw(px(3.))") == {"raw-px": 2}
    # An allow for another rule exempts nothing, and an allow with no reason is reported.
    assert counts("w(px(3.)) // lint-allow(raw-color): wrong rule") == {"raw-px": 1}
    assert counts("w(px(3.)) // lint-allow(raw-px)") == {ui_lint.NO_REASON: 1}
    # Notifies count only inside `impl Waffle`, however its body's braces, chars, lifetimes and strings fall.
    waffle = """
impl Other {
    fn a(&mut self, cx: &mut Context<Self>) { cx.notify(); }
}
impl Waffle {
    fn b<'a>(&mut self, s: &'a str, cx: &mut Context<Self>) {
        let (open, close, text) = ('{', '}', "}}}{");
        if s.is_empty() { cx.notify(); }
    }
    fn c(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }
}
impl Render for Waffle {
    fn render(&mut self, cx: &mut Context<Self>) { cx.notify(); }
}
fn free(cx: &mut App) { cx.notify(); }
"""
    assert counts(waffle) == {"global-notify": 3}, counts(waffle)
    # A string spanning lines keeps its braces out of the scope.
    assert counts('impl Waffle {\n  const T: &str = "{\n{";\n}\nfn f() { cx.notify(); }') == {}


test()
print("test_ui_lint: ok")
