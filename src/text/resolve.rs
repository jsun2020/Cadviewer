use std::collections::HashMap;
use std::path::PathBuf;

use crate::text::search::FontSearch;

/// Which half of a style's dual-font pair a reference fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// Group 3. Draws single-byte characters.
    Primary,
    /// Group 4. Draws double-byte characters.
    Bigfont,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    Shx(PathBuf),
    /// A TrueType file and its face index inside a `.ttc` collection.
    Ttf(PathBuf, u32),
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Substitution {
    pub requested: String,
    pub role: Role,
    pub used: Resolved,
}

/// Stroke fonts tried, in order, when a big font is missing.
///
/// `gbcbig.shx` ships with every AutoCAD, is a genuine stroke font, and
/// therefore sits beside the drawing's Latin SHX text without the visible
/// mismatch a TrueType swap produces. That is why the chain prefers it to
/// any TTF (PRD 5.6.2). `hztxt.shx` is listed after `gbcbig.shx` as a
/// fallback for machines that may have the third-party font.
const BIGFONT_SUBSTITUTES: [&str; 2] = ["gbcbig.shx", "hztxt.shx"];

/// Stroke fonts tried, in order, when a single-byte font is missing.
const PRIMARY_SUBSTITUTES: [&str; 3] = ["simplex.shx", "txt.shx", "isocp.shx"];

/// Last-resort TrueType fonts, CJK-capable first.
///
/// Located on the user's machine, never shipped (R-TXT-5.1). `.ttc`
/// collections carry several faces; index 0 is the regular weight.
const TTF_FALLBACKS: [(&str, u32); 4] =
    [("simsun.ttc", 0), ("simhei.ttf", 0), ("msyh.ttc", 0), ("arial.ttf", 0)];

/// The font AutoCAD draws a style with when its group 3 is empty. This is
/// a default, not a substitution, so it raises no warning.
const UNNAMED_PRIMARY_DEFAULT: &str = "txt.shx";

pub struct Resolver {
    search: FontSearch,
    /// Keyed by (upper-case reference, role) so a repeated lookup is
    /// neither re-searched nor re-warned. The reference drawing asks for
    /// the same broken style 504 times.
    cache: HashMap<(String, Role), Resolved>,
    substitutions: Vec<Substitution>,
}

fn is_truetype(reference: &str) -> bool {
    let lower = reference.to_ascii_lowercase();
    [".ttf", ".ttc", ".otf"].iter().any(|ext| lower.ends_with(ext))
}

impl Resolver {
    pub fn new(search: FontSearch) -> Resolver {
        Resolver { search, cache: HashMap::new(), substitutions: Vec::new() }
    }

    pub fn substitutions(&self) -> &[Substitution] {
        &self.substitutions
    }

    pub fn resolve(&mut self, reference: &str, role: Role) -> Resolved {
        let key = (reference.trim().to_ascii_uppercase(), role);
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let resolved = self.resolve_uncached(reference, role);
        self.cache.insert(key, resolved.clone());
        resolved
    }

    fn resolve_uncached(&mut self, reference: &str, role: Role) -> Resolved {
        let trimmed = reference.trim();

        if trimmed.is_empty() {
            return match role {
                // A style with no big font simply has none.
                Role::Bigfont => Resolved::None,
                // A style with no primary font is drawn with txt.shx. That
                // default is not a substitution and must not warn — but if
                // even the default is absent, the style draws nothing, and
                // silence there is the failure R-TXT-2.3 forbids.
                Role::Primary => match self.locate(UNNAMED_PRIMARY_DEFAULT) {
                    Some(found) => found,
                    None => {
                        self.substitutions.push(Substitution {
                            requested: UNNAMED_PRIMARY_DEFAULT.to_owned(),
                            role,
                            used: Resolved::None,
                        });
                        Resolved::None
                    }
                },
            };
        }

        if let Some(found) = self.locate(trimmed) {
            return found;
        }

        // Missing. Walk the chain, then record exactly what was used —
        // R-TXT-2.3 forbids a silent swap.
        let chain: &[&str] =
            if role == Role::Bigfont { &BIGFONT_SUBSTITUTES } else { &PRIMARY_SUBSTITUTES };
        let mut used = Resolved::None;
        for candidate in chain {
            // Avoid a redundant re-scan of a font already proven to be missing.
            if candidate.eq_ignore_ascii_case(trimmed) {
                continue;
            }
            if let Some(found) = self.locate(candidate) {
                used = found;
                break;
            }
        }
        if used == Resolved::None {
            for (candidate, index) in TTF_FALLBACKS {
                if let Some(path) = self.search.find(candidate) {
                    used = Resolved::Ttf(path, index);
                    break;
                }
            }
        }
        self.substitutions.push(Substitution {
            requested: trimmed.to_owned(),
            role,
            used: used.clone(),
        });
        used
    }

    /// Find one reference on disk, classifying it by extension.
    fn locate(&self, reference: &str) -> Option<Resolved> {
        let path = self.search.find(reference)?;
        if is_truetype(reference) { Some(Resolved::Ttf(path, 0)) } else { Some(Resolved::Shx(path)) }
    }
}

impl Substitution {
    /// One line for the warnings area, naming what was asked for and what
    /// was actually used (R-TXT-2.3).
    pub fn describe(&self) -> String {
        let half = match self.role {
            Role::Primary => "主字库",
            Role::Bigfont => "大字体",
        };
        match &self.used {
            Resolved::Shx(path) | Resolved::Ttf(path, _) => format!(
                "缺少{half} {}，已替代为 {}",
                self.requested,
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            ),
            Resolved::None => format!("缺少{half} {}，且找不到任何替代字库", self.requested),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::search::FontSearch;

    /// A directory holding exactly the named (empty) font files.
    fn dir_with(names: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for name in names {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        dir
    }

    fn resolver(dir: &tempfile::TempDir) -> Resolver {
        Resolver::new(FontSearch { dirs: vec![dir.path().to_path_buf()] })
    }

    #[test]
    fn a_font_that_exists_is_used_and_reported_as_no_substitution() {
        let dir = dir_with(&["isocp.shx"]);
        let mut r = resolver(&dir);
        assert!(matches!(r.resolve("isocp.shx", Role::Primary), Resolved::Shx(_)));
        assert!(r.substitutions().is_empty(), "{:?}", r.substitutions());
    }

    /// R-TXT-2.2 rule 1: a missing big font becomes gbcbig, not a TTF.
    /// This is the reference drawing's Standard style.
    #[test]
    fn a_missing_bigfont_substitutes_gbcbig_before_any_ttf() {
        let dir = dir_with(&["gbcbig.shx", "simsun.ttc"]);
        let mut r = resolver(&dir);
        let used = r.resolve("hztxt.shx", Role::Bigfont);
        let Resolved::Shx(path) = &used else { panic!("expected an SHX, got {used:?}") };
        assert!(path.ends_with("gbcbig.shx"), "got {path:?}");
        assert_eq!(r.substitutions().len(), 1);
        assert_eq!(r.substitutions()[0].requested, "hztxt.shx");
        assert_eq!(r.substitutions()[0].used, used);
    }

    /// R-TXT-2.2 rule 2: a missing primary prefers simplex, then txt.
    /// Ordering is pinned by including a TTF that must not be chosen.
    #[test]
    fn a_missing_primary_substitutes_simplex_then_txt() {
        let dir = dir_with(&["simplex.shx", "txt.shx", "simsun.ttc"]);
        let mut r = resolver(&dir);
        let Resolved::Shx(path) = r.resolve("yjkeng.shx", Role::Primary) else { panic!() };
        assert!(path.ends_with("simplex.shx"), "got {path:?}");

        let only_txt = dir_with(&["txt.shx"]);
        let mut r = resolver(&only_txt);
        let Resolved::Shx(path) = r.resolve("yjkeng.shx", Role::Primary) else { panic!() };
        assert!(path.ends_with("txt.shx"), "got {path:?}");
    }

    /// R-TXT-2.2 rule 3: with no SHX anywhere, fall back to a TTF — and
    /// still warn.
    #[test]
    fn with_no_shx_at_all_the_fallback_is_a_ttf_and_is_still_reported() {
        let dir = dir_with(&["simsun.ttc", "arial.ttf"]);
        let mut r = resolver(&dir);
        let used = r.resolve("hztxt.shx", Role::Bigfont);
        assert!(matches!(used, Resolved::Ttf(_, _)), "got {used:?}");
        assert_eq!(r.substitutions().len(), 1, "a silent TTF swap is forbidden");
    }

    /// A style naming a `.ttf` directly is not a substitution — that is
    /// what the drawing asked for. The reference title block uses
    /// simhei.ttf for 73 TEXT entities.
    #[test]
    fn a_style_that_names_a_ttf_directly_is_not_a_substitution() {
        let dir = dir_with(&["simhei.ttf"]);
        let mut r = resolver(&dir);
        assert!(matches!(r.resolve("simhei.ttf", Role::Primary), Resolved::Ttf(_, 0)));
        assert!(r.substitutions().is_empty());
    }

    /// An empty group 3 is "no font named", which AutoCAD draws with
    /// txt.shx: a default, not a substitution, so it must not warn. But if
    /// even txt.shx is absent — the normal state of a machine with no CAD
    /// product installed — the style draws nothing, and that must not happen
    /// in silence.
    #[test]
    fn an_unnamed_primary_warns_only_when_even_the_default_is_missing() {
        let present = dir_with(&["txt.shx"]);
        let mut r = resolver(&present);
        assert!(matches!(r.resolve("", Role::Primary), Resolved::Shx(_)));
        assert!(r.substitutions().is_empty(), "the default must not warn: {:?}", r.substitutions());

        let empty = dir_with(&[]);
        let mut r = resolver(&empty);
        assert_eq!(r.resolve("", Role::Primary), Resolved::None);
        assert_eq!(r.substitutions().len(), 1, "a missing default must not be silent");
        assert!(r.substitutions()[0].describe().contains("txt.shx"));
    }

    /// An empty group 4 means the style simply has no big font. It must
    /// not warn — most Latin styles are like this.
    #[test]
    fn an_unnamed_bigfont_is_absent_not_missing() {
        let dir = dir_with(&["gbcbig.shx"]);
        let mut r = resolver(&dir);
        assert_eq!(r.resolve("", Role::Bigfont), Resolved::None);
        assert!(r.substitutions().is_empty());
    }

    /// R-TXT-2.3: one warning per distinct missing font, not one per
    /// entity. The reference drawing has 504 ATTRIBs on one broken style.
    #[test]
    fn repeated_lookups_of_the_same_missing_font_warn_once() {
        let dir = dir_with(&["gbcbig.shx"]);
        let mut r = resolver(&dir);
        for _ in 0..500 {
            r.resolve("hztxt.shx", Role::Bigfont);
        }
        assert_eq!(r.substitutions().len(), 1);
    }

    /// R-TXT-2.4: the same drawing on the same machine must choose the
    /// same substitute twice running.
    #[test]
    fn substitution_is_deterministic() {
        let dir = dir_with(&["gbcbig.shx", "simplex.shx", "txt.shx"]);
        let first = resolver(&dir).resolve("hztxt.shx", Role::Bigfont);
        let second = resolver(&dir).resolve("hztxt.shx", Role::Bigfont);
        assert_eq!(first, second);
    }

    /// Nothing at all on disk: report it and draw nothing, rather than
    /// pretending a font was found.
    #[test]
    fn an_empty_machine_resolves_to_none_with_a_warning() {
        let dir = dir_with(&[]);
        let mut r = resolver(&dir);
        assert_eq!(r.resolve("hztxt.shx", Role::Bigfont), Resolved::None);
        assert_eq!(r.substitutions().len(), 1);
        assert!(r.substitutions()[0].describe().contains("hztxt.shx"));
    }
}
