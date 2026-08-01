//! Which build is this?
//!
//! A fix that is committed but not repackaged looks exactly like a fix that
//! does not work: the user opens the portable app, sees the old behaviour,
//! and reports the bug again. That happened once already, and it cost a full
//! diagnosis before the binary's timestamp gave it away. The stamp makes the
//! question answerable from the window title instead — package version plus
//! the commit the binary was built from, marked `+` when the working tree
//! had uncommitted changes.

/// Version and source revision of this binary, e.g. `0.1.0 (039be23)`.
///
/// The revision degrades to `unknown` when the build had no git available —
/// which is the normal case for a user building from the shipped source zip,
/// so it must never be treated as an error.
pub fn stamp() -> String {
    format!("{} ({})", env!("CARGO_PKG_VERSION"), revision())
}

/// Just the source revision.
pub fn revision() -> &'static str {
    option_env!("CADVIEWER_BUILD").unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stamp is only useful if it actually names the version, so pin
    /// that rather than "is not empty" — a stamp of `" ()"` would pass the
    /// weaker assertion while telling nobody anything.
    #[test]
    fn the_stamp_carries_the_package_version() {
        let stamp = stamp();
        assert!(
            stamp.starts_with(env!("CARGO_PKG_VERSION")),
            "expected the version at the front, got {stamp:?}"
        );
        assert!(stamp.contains(revision()), "got {stamp:?}");
    }

    /// A repository build must produce a real revision. This is the control
    /// assertion: without it the whole feature could silently degrade to
    /// `unknown` everywhere and still pass its tests, which is the exact
    /// class of vacuous gate LL-032 and LL-033 record.
    #[test]
    fn a_build_from_the_repository_knows_its_revision() {
        if !std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".git").exists() {
            println!("SKIPPED: not a git checkout, so no revision is expected");
            return;
        }
        assert_ne!(
            revision(),
            "unknown",
            "build.rs failed to read the revision in a git checkout"
        );
    }
}
