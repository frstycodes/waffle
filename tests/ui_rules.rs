// The design system's guard: tools/ui_lint.py counts what app code writes by hand (src/ui/rules.tsv) and fails
// when any file goes over its count in src/ui/rules.baseline, or src/ui/CATALOG.md is stale. Needs python3.
fn python(args: &[&str]) {
    let out = std::process::Command::new("python3").args(args).output().expect("python3 runs");
    assert!(out.status.success(), "{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
}

#[test]
fn app_code_uses_the_design_system() {
    python(&["tools/ui_lint.py", "--check"]);
}

#[test]
fn the_lint_counts_correctly() {
    python(&["tools/test_ui_lint.py"]);
}
