fn main() {
    install_git_hooks();
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        // ScreenCaptureKit (the `screencapturekit` crate, for Go Live) links Swift's runtime libraries, which
        // macOS ships in /usr/lib/swift; without this rpath the binary fails to launch with "Library not loaded".
        Ok("macos") => println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift"),
        // The exe's icon: Explorer shows it, and gpui loads it for the window and taskbar.
        Ok("windows") => {
            println!("cargo:rerun-if-changed=assets/windows/app.ico");
            println!("cargo:rerun-if-changed=assets/windows/app.rc");
            // rc.exe can't take quoted strings as /D macros reliably, so the version is written into a copy.
            let version = std::env::var("CARGO_PKG_VERSION").expect("cargo sets the version");
            let mut parts = version.split(['.', '-']).map(|p| p.parse::<u16>().unwrap_or(0));
            let tuple = format!("{},{},{},0", parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));
            let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets the manifest dir"));
            let icon = manifest.join("assets/windows/app.ico").display().to_string().replace('\\', "\\\\");
            let rc = std::fs::read_to_string(manifest.join("assets/windows/app.rc"))
                .expect("read app.rc")
                .replace("ICON_PATH", &icon)
                .replace("VERSION_TUPLE", &tuple)
                .replace("VERSION_STRING", &format!("\"{version}\""));
            let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("app.rc");
            std::fs::write(&out, rc).expect("write app.rc");
            embed_resource::compile(&out, embed_resource::NONE).manifest_optional().expect("embed the app icon and version info");
        }
        _ => {}
    }
}

// Points git at .githooks (the UI rules before each commit) the first time a clone builds, so nobody has to
// remember `make hooks`. Leaves a hooksPath someone set themselves alone, and does nothing outside a git checkout.
fn install_git_hooks() {
    let git = |args: &[&str]| std::process::Command::new("git").args(args).output().ok();
    let Some(current) = git(&["config", "--get", "core.hooksPath"]) else { return };
    if !current.stdout.is_empty() || git(&["rev-parse", "--git-dir"]).is_none_or(|o| !o.status.success()) {
        return;
    }
    git(&["config", "core.hooksPath", ".githooks"]);
}
