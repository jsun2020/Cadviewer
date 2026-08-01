//! Stamp the binary with the commit it was built from.
//!
//! Consumed by `src/build_info.rs`. Failure is never fatal: a user building
//! from the shipped source zip has no `.git`, and possibly no `git`, and
//! must still get a working program.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    embed_icon();

    // Rebuild when the checked-out commit changes, or when work is
    // committed — otherwise a stale stamp is worse than none.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    if let Some(revision) = revision() {
        println!("cargo:rustc-env=CADVIEWER_BUILD={revision}");
    }
}

/// Embed `assets\app-icon.ico` as the executable's Win32 icon resource.
///
/// This is what Explorer, the taskbar and Alt+Tab read; the window icon set
/// at runtime (`src/icon.rs`) is a separate mechanism and does not cover the
/// file on disk. Resource id 1 because Windows shows the lowest-numbered
/// icon resource as the application icon.
///
/// The `.rc` is generated with an absolute path rather than checked in with
/// a relative one, so it cannot depend on which directory the resource
/// compiler happens to run from.
fn embed_icon() {
    if std::env::var("CARGO_CFG_WINDOWS").is_err() {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let ico = manifest.join("assets").join("app-icon.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    assert!(
        ico.is_file(),
        "missing {} — the exe would ship with no icon and nothing would say so",
        ico.display()
    );

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("set by cargo")).join("icon.rc");
    // Backslashes are escapes inside an .rc string literal.
    let escaped = ico.display().to_string().replace('\\', "\\\\");
    std::fs::write(&out, format!("1 ICON \"{escaped}\"\n")).expect("writing the generated .rc");

    // Loud on failure: an exe that silently loses its icon is the same class
    // of invisible regression as a leak gate that never fires.
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_required()
        .expect("embedding the icon resource failed");
}

fn revision() -> Option<String> {
    let head = git(&["rev-parse", "--short", "HEAD"])?;
    // `--porcelain` prints one line per modified path and nothing at all for
    // a clean tree, so any output means the binary does not correspond to
    // the named commit.
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    Some(if dirty { format!("{head}+") } else { head })
}

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    // Guard against a stamp that would break the `KEY=VALUE` directive.
    if text.contains('\n') || text.contains('\r') {
        return Some(text.lines().next().unwrap_or_default().to_owned());
    }
    Some(text)
}
