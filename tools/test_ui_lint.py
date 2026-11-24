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
    assert counts("w(px(3.)) // lint-allow(raw-color): the wrong rule here") == {"raw-px": 1}
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


    # A reason too thin to act on is reported too.
    assert counts("w(px(3.)) // lint-allow(raw-px): needed") == {ui_lint.WEAK_REASON: 1}


def elements():
    # A clickable, hovered, rounded div is a button, however its chain is split across lines and closures.
    button = """
fn f() -> Div {
    div()
        .id("x")
        .squircle(RADIUS_SM)
        .when(on, |d| d.hover(|s| s.bg(fill)))
        .child(div().child("label"))
        .on_click(go)
}
"""
    assert counts(button)["custom-button"] == 1
    # Children are their own elements: a plain child doesn't make its parent anything.
    assert "custom-button" not in counts("div().child(div().on_click(go)).hover(|s| s).squircle(R)")
    # Rows and columns: a raw flex div, not ui::row().
    assert counts("div().flex().items_center().gap_2()") == {"raw-row": 1}
    assert counts("div().flex().flex_col().gap_2()") == {"raw-col": 1}
    assert counts("ui::row().gap_2().child(ui::col().gap_1())") == {}
    # Margins: spacing between siblings is the parent's gap. A method that only starts with m isn't one.
    assert counts("ui::row().child(x.mt_2()).child(y.mx_neg_1p5()).child(z.ml_auto())") == {"raw-margin": 3}
    assert counts("let id = self.my_id(); x.map(f).min_w_0()") == {}
    # An allow for a component rule names the ui:: item it considered; one that doesn't is reported.
    card = "\nlet card = div().bg(a).shadow(b);"
    assert counts("// lint-allow(custom-surface): ui::surface has no stage ring option yet" + card) == {}
    assert counts("// lint-allow(custom-surface): the stage needs its own ring colour" + card) == {ui_lint.NO_REFERENCE: 1}
    assert counts("// lint-allow(custom-surface): ui::not_a_thing lacks the stage ring" + card) == {ui_lint.NO_REFERENCE: 1}
    # A list that scrolls but builds every row is reported; one that says how big it can get is let off.
    rows = "\nlet list = div().id(\"l\").overflow_y_scroll().children(items.iter().map(row));"
    assert counts(rows) == {"unvirtualized-list": 1}
    assert counts("// lint-allow(unvirtualized-list): a server's roles, a few dozen at most" + rows) == {}
    assert counts("// lint-allow(unvirtualized-list): rows are cheap to draw here" + rows) == {ui_lint.NO_BOUND: 1}
    # A function named like a component builds it from one; a domain wrapper over a ui:: component is fine.
    assert counts("fn stage_button(cx: &App) -> Stateful<Div> { div().id(\"b\") }") == {"custom-component": 1}
    assert counts("fn stage_button(cx: &App) -> Stateful<Div> { ui::button(\"b\", Tone::Ghost, cx) }") == {}
    assert counts("fn show_toast(&mut self, cx: &mut App) { self.toast = 1; }") == {}


def performance():
    # Render-path work counts in element-building functions, but not in their handlers, which run later.
    render = "fn view(&self, cx: &App) -> Div {\n    roles.sort_by_key(|r| r.position);\n    div().on_click(|_, _, cx| list.sort())\n}"
    assert counts(render).get("render-work") == 1, counts(render)
    assert "render-work" not in counts("fn load(&mut self) { roles.sort(); }")
    # Blocking I/O on the UI thread, but not inside work handed to another thread.
    io = "fn open(&mut self, cx: &mut Context<Self>) {\n    let a = std::fs::read(p);\n    cx.background_spawn(async move { std::fs::read(q) });\n}"
    assert counts(io).get("main-thread-io") == 1, counts(io)
    # A timer loop's notify needs a check that something changed first.
    tick = "fn t(cx: &mut App) {\n    loop {\n        timer(ms).await;\n        cx.notify();\n    }\n}"
    assert counts(tick).get("hot-notify") == 1, counts(tick)
    guarded = "fn t(cx: &mut App) {\n    loop {\n        timer(ms).await;\n        if left != before { cx.notify(); }\n    }\n}"
    assert "hot-notify" not in counts(guarded)
    # Memory: a field the app grows and never shrinks, resets or lends out.
    grows = "struct S {\n    seen_ids_xyz: HashMap<String, u32>,\n}\nfn f(s: &mut S) { s.seen_ids_xyz.insert(k, 1); }"
    ui_lint._CORPUS = grows
    assert counts(grows).get("unbounded-growth") == 1, counts(grows)
    ui_lint._CORPUS = grows + "\nfn g(s: &mut S) { s.seen_ids_xyz.retain(|_, v| *v > 0); }"
    assert "unbounded-growth" not in counts(grows)
    ui_lint._CORPUS = None
    # A CDN image asks for the size it's drawn at.
    assert counts('let u = format!("https://cdn.discordapp.com/avatars/{id}/{hash}.png");') == {"unsized-image": 1}
    assert counts('let u = format!("https://cdn.discordapp.com/avatars/{id}/{hash}.png?size=64");') == {}


def duplicate_one_offs():
    rule = "// lint-allow(custom-surface): ui::surface lacks the stage's ring for now\n"
    one = rule + "let a = div().id(\"a\").bg(x).shadow(y).p_2().squircle(R);"
    other = rule + "let b = div().id(\"b\").bg(x).shadow(y).p_2().squircle(R);"
    tree = {"src/a.rs": ui_lint.scan("src/a.rs", one, RULES), "src/b.rs": ui_lint.scan("src/b.rs", other, RULES)}
    assert len(ui_lint.duplicates(tree, RULES)) == 1
    unlike = rule + "let c = div().bg(x).shadow(y).absolute().top_0().w_full().h_10().flex();"
    tree["src/b.rs"] = ui_lint.scan("src/b.rs", unlike, RULES)
    assert ui_lint.duplicates(tree, RULES) == []
    # Every allow is listed for the next reader, under its rule.
    assert "## custom-surface" in ui_lint.one_offs(tree, RULES)


def file_lines():
    limit = next(r["pattern"] for r in RULES if r["name"] == "file-lines")
    body = "fn f() {}\n" * limit
    tests = "#[cfg(test)]\nmod tests {\n    fn g() {\n    }\n}\n"
    comments = "// a comment\n\n/* block */\n" * 50
    # At the limit is fine; blank lines, comments and the test module don't count toward it.
    assert counts(body + comments + tests) == {}
    hits = ui_lint.scan("src/x.rs", body + tests, RULES)
    assert hits.code_lines == limit, hits.code_lines
    # One line over: every line of code counts, so the baseline holds the file at its size.
    assert counts(body + "fn h() {}\n") == {"file-lines": limit + 1}
    # Code after the test module counts again.
    assert counts(body + tests + "fn h() {}\n") == {"file-lines": limit + 1}
    # A reasoned allow at the top of the file waives it; one further down doesn't.
    assert counts("// lint-allow(file-lines): a generated lookup table\n" + body + "fn h() {}\n") == {}
    assert counts(body + "// lint-allow(file-lines): this comes too late\nfn h() {}\n") == {"file-lines": limit + 1}
    # The design system is held to the limit too, though not to the value rules.
    assert {r for r in ui_lint.scan("src/ui/x.rs", body + "fn h() { px(3.) }\n", RULES)} == {"file-lines"}


def titlebar():
    head = "fn head(&self) -> Div {{ ui::row().window_control_area(WindowControlArea::Drag).child(x){} }}"
    # A title-line header that leaves no room for the window's corners is reported; one that does isn't.
    assert counts(head.format("")) == {"titlebar-clearance": 1}
    assert counts(head.format(".child(self.caption_clearance(true))")) == {}
    assert counts(head.format(".pl(px(HEADER_PAD + layout::traffic_lights_overhang(window)))")) == {}


test()
titlebar()
elements()
performance()
duplicate_one_offs()
file_lines()
print("test_ui_lint: ok")
