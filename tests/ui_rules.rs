// The design system's guard: tools/ui_lint.py counts what app code writes by hand (src/ui/rules.tsv) and fails
// when any file goes over its count in src/ui/rules.baseline, or src/ui/CATALOG.md is stale. Needs Python 3:
// `python3`, or on Windows the `py -3` launcher or `python`.
fn python(args: &[&str]) {
    let launchers: [&[&str]; 3] = [&["python3"], &["py", "-3"], &["python"]];
    for launcher in launchers {
        let Ok(out) = std::process::Command::new(launcher[0]).args(&launcher[1..]).args(args).output() else { continue };
        // The Windows Store stub answers `python3` with exit 9009 and no Python behind it.
        if out.status.code() == Some(9009) {
            continue;
        }
        assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        return;
    }
    panic!("no Python 3 found (tried python3, py -3, python)");
}

#[test]
fn app_code_uses_the_design_system() {
    python(&["tools/ui_lint.py", "--check"]);
}

#[test]
fn the_lint_counts_correctly() {
    python(&["tools/test_ui_lint.py"]);
}
