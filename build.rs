//! Stamp the binary with the commit it was built from.
//!
//! Consumed by `src/build_info.rs`. Failure is never fatal: a user building
//! from the shipped source zip has no `.git`, and possibly no `git`, and
//! must still get a working program.

use std::process::Command;

fn main() {
    // Rebuild when the checked-out commit changes, or when work is
    // committed — otherwise a stale stamp is worse than none.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");

    if let Some(revision) = revision() {
        println!("cargo:rustc-env=CADVIEWER_BUILD={revision}");
    }
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
