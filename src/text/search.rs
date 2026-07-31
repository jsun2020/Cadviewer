#[cfg(test)]
mod tests {
    use super::*;

    /// The spellings measured in the reference drawing's STYLE table.
    #[test]
    fn extensionless_references_gain_the_shx_extension() {
        assert_eq!(candidate_filenames("SIMPLEX"), vec!["SIMPLEX.shx".to_owned()]);
        assert_eq!(candidate_filenames("txt"), vec!["txt.shx".to_owned()]);
    }

    #[test]
    fn references_that_already_carry_an_extension_are_left_alone() {
        assert_eq!(candidate_filenames("isocp.shx"), vec!["isocp.shx".to_owned()]);
        assert_eq!(candidate_filenames("simhei.ttf"), vec!["simhei.ttf".to_owned()]);
    }

    /// A style with no font at all must produce no candidates, so the
    /// caller can tell "no font named" from "a font that is missing".
    #[test]
    fn an_empty_reference_produces_no_candidates() {
        assert!(candidate_filenames("").is_empty());
        assert!(candidate_filenames("   ").is_empty());
    }

    /// Drawings authored on another machine carry absolute paths. Only the
    /// file name is meaningful here.
    #[test]
    fn a_full_path_reference_is_reduced_to_its_file_name() {
        assert_eq!(candidate_filenames("C:\\Fonts\\hztxt.shx"), vec!["hztxt.shx".to_owned()]);
        assert_eq!(candidate_filenames("../fonts/gbcbig.shx"), vec!["gbcbig.shx".to_owned()]);
    }

    #[test]
    fn the_drawings_own_directory_is_searched() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("weird.shx"), b"x").unwrap();
        let drawing = dir.path().join("plan.dwg");
        let search = FontSearch::for_drawing(Some(&drawing), &[]);
        assert!(search.find("weird").is_some(), "dirs were {:?}", search.dirs);
    }

    /// A style naming `GBCBIG` against a file called `gbcbig.shx` must
    /// resolve regardless of the filesystem's own case rules.
    #[test]
    fn lookups_ignore_case() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("gbcbig.shx"), b"x").unwrap();
        let search = FontSearch { dirs: vec![dir.path().to_path_buf()] };
        assert!(search.find("GBCBIG").is_some());
        assert!(search.find("GbCbIg.shx").is_some());
    }

    #[test]
    fn a_font_that_is_not_anywhere_resolves_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let search = FontSearch { dirs: vec![dir.path().to_path_buf()] };
        assert!(search.find("hztxt.shx").is_none());
    }

    /// `--font-dir` must be searched.
    #[test]
    fn extra_directories_are_included() {
        let extra = tempfile::tempdir().unwrap();
        std::fs::write(extra.path().join("hztxt.shx"), b"x").unwrap();
        let search = FontSearch::for_drawing(None, &[extra.path().to_path_buf()]);
        assert!(search.find("hztxt.shx").is_some());
    }
}

use std::path::{Path, PathBuf};

/// Roots under which installed CAD products keep their `Fonts` directory.
///
/// Chinese drawing offices commonly run Gstarsoft (浩辰) or ZWSOFT (中望)
/// rather than AutoCAD, and those ship the same font files, so all three
/// are worth searching before declaring a style's font missing.
const CAD_ROOTS: [&str; 4] = [
    "C:\\Program Files\\Autodesk",
    "C:\\Program Files\\Gstarsoft",
    "C:\\Program Files\\ZWSOFT",
    "C:\\Program Files (x86)\\Autodesk",
];

/// Where TrueType fallbacks live. Fonts are located, never bundled
/// (R-TXT-5.1): SimSun and SimHei belong to Microsoft.
const WINDOWS_FONTS: &str = "C:\\Windows\\Fonts";

#[derive(Clone, Debug, Default)]
pub struct FontSearch {
    /// Searched in order; the first hit wins, which is what makes the
    /// result reproducible across runs (R-TXT-2.4).
    pub dirs: Vec<PathBuf>,
}

/// File names to try for one STYLE font reference.
///
/// The reference drawing spells fonts as `isocp.shx`, `SIMPLEX`, `txt`,
/// `simhei.ttf` and empty, and a drawing authored elsewhere may carry a
/// full path. Anything without a recognised extension is an SHX name.
pub fn candidate_filenames(reference: &str) -> Vec<String> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    // Only the file name matters: the authoring machine's directory layout
    // is meaningless here.
    let name = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if name.is_empty() {
        return Vec::new();
    }
    let lower = name.to_ascii_lowercase();
    if [".shx", ".ttf", ".ttc", ".otf"].iter().any(|ext| lower.ends_with(ext)) {
        vec![name.to_owned()]
    } else {
        vec![format!("{name}.shx")]
    }
}

fn cad_font_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in CAD_ROOTS {
        let Ok(entries) = std::fs::read_dir(root) else { continue };
        let mut products: Vec<PathBuf> =
            entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        // Sorted so two runs on a machine with several products installed
        // pick the same directory (R-TXT-2.4).
        products.sort();
        for product in products {
            let fonts = product.join("Fonts");
            if fonts.is_dir() {
                out.push(fonts);
            }
        }
    }
    out
}

impl FontSearch {
    /// R-TXT-2.1 search order.
    pub fn for_drawing(drawing: Option<&Path>, extra: &[PathBuf]) -> FontSearch {
        let mut dirs = Vec::new();
        if let Ok(exe) = std::env::current_exe()
            && let Some(parent) = exe.parent()
        {
            dirs.push(parent.join("fonts"));
        }
        if let Some(parent) = drawing.and_then(|d| d.parent()) {
            // Chinese drawing sets are routinely shipped with the SHX
            // files sitting beside the dwg.
            dirs.push(parent.to_path_buf());
        }
        dirs.extend(cad_font_dirs());
        dirs.extend(extra.iter().cloned());
        dirs.push(PathBuf::from(WINDOWS_FONTS));
        dirs.retain(|d| d.is_dir());
        dirs.dedup();
        FontSearch { dirs }
    }

    /// First matching file, comparing names case-insensitively.
    pub fn find(&self, reference: &str) -> Option<PathBuf> {
        let wanted = candidate_filenames(reference);
        if wanted.is_empty() {
            return None;
        }
        for dir in &self.dirs {
            for name in &wanted {
                let direct = dir.join(name);
                if direct.is_file() {
                    return Some(direct);
                }
                // Fall back to a listing so a case difference cannot cost
                // a font on a case-sensitive volume.
                let Ok(entries) = std::fs::read_dir(dir) else { continue };
                for entry in entries.filter_map(|e| e.ok()) {
                    if entry.file_name().to_string_lossy().eq_ignore_ascii_case(name)
                        && entry.path().is_file()
                    {
                        return Some(entry.path());
                    }
                }
            }
        }
        None
    }
}
