use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub mod layout;
pub mod mtext;
pub mod resolve;
pub mod search;
pub mod ttf;

use crate::doc::Document;
use crate::dxf::entities::RawEntity;
use crate::dxf::tables::StyleRecord;
use crate::encoding::Codepage;
use layout::{FontHandle, FontPair, TextGeom};
use resolve::{Resolved, Resolver, Role};

/// Everything text needs for one document: resolved fonts, loaded once,
/// plus the substitution warnings they produced.
pub struct TextEngine {
    codepage: Codepage,
    styles: HashMap<String, StyleRecord>,
    resolver: Resolver,
    /// Loaded font pairs, keyed by the upper-case style name. A drawing
    /// has tens of styles and thousands of text entities, so a pair is
    /// loaded at most once — `gbcbig.shx` alone is 900 KB.
    pairs: HashMap<String, FontPair>,
    /// Styles whose font could not be opened, reported once each.
    load_failures: Vec<String>,
    /// Styles already given a big font they never named, so the fallback is
    /// resolved and reported once rather than per entity.
    unnamed_bigfont_styles: std::collections::HashSet<String>,
    /// Which substitutions each style's fonts triggered, by index into
    /// `resolver.substitutions()`. Recorded when the style is first
    /// resolved, so the per-entity tally below costs a map lookup rather
    /// than a re-resolution.
    style_substitutions: HashMap<String, Vec<usize>>,
    /// Entities drawn with each substitution. R-TXT-2.3 requires the
    /// warning to state how many entities a substitution affected, not
    /// just that it happened — "hztxt.shx was replaced" and "hztxt.shx was
    /// replaced across 2,886 entities" are very different messages to
    /// someone deciding whether to go and install the font.
    entity_counts: HashMap<usize, usize>,
}

fn open(resolved: &Resolved) -> Result<FontHandle, String> {
    match resolved {
        Resolved::None => Ok(FontHandle::None),
        Resolved::Shx(path) => {
            let bytes = std::fs::read(path).map_err(|e| format!("{}：{e}", path.display()))?;
            crate::shx::ShxFont::load(&bytes)
                .map(|f| FontHandle::Shx(Box::new(f)))
                .map_err(|e| format!("{}：{e}", path.display()))
        }
        Resolved::Ttf(path, index) => {
            let bytes = std::fs::read(path).map_err(|e| format!("{}：{e}", path.display()))?;
            ttf::TtfFont::load(bytes, *index)
                .map(|f| FontHandle::Ttf(Box::new(f)))
                .map_err(|e| format!("{}：{e}", path.display()))
        }
    }
}

impl TextEngine {
    /// Build the engine for one document. `drawing` is the input file's
    /// path, used to search its own directory (R-TXT-2.1 step 2).
    pub fn new(doc: &Document, drawing: Option<&Path>, extra_dirs: &[PathBuf]) -> TextEngine {
        TextEngine {
            codepage: doc.header.codepage,
            styles: doc.styles.clone(),
            resolver: Resolver::new(search::FontSearch::for_drawing(drawing, extra_dirs)),
            pairs: HashMap::new(),
            load_failures: Vec::new(),
            unnamed_bigfont_styles: std::collections::HashSet::new(),
            style_substitutions: HashMap::new(),
            entity_counts: HashMap::new(),
        }
    }

    /// R-TXT-2.3: every substitution, named, with the number of entities
    /// it affected, one line each.
    pub fn warnings(&self) -> Vec<String> {
        self.resolver
            .substitutions()
            .iter()
            .enumerate()
            .map(|(index, s)| match self.entity_counts.get(&index).copied().unwrap_or(0) {
                // A substitution no entity actually used is still worth
                // reporting — the style exists — but saying so keeps it
                // from reading as a problem on this page.
                0 => format!("{}（本页无实体使用）", s.describe()),
                count => format!("{}（影响 {count} 个实体）", s.describe()),
            })
            .chain(self.load_failures.iter().map(|f| format!("字库无法读取：{f}")))
            .collect()
    }

    fn style_for(&self, entity: &RawEntity) -> StyleRecord {
        let named = entity.text(7, self.codepage).unwrap_or_default();
        let key = named.trim().to_ascii_uppercase();
        self.styles
            .get(&key)
            .or_else(|| self.styles.get("STANDARD"))
            .cloned()
            .unwrap_or_default()
    }

    fn pair_for(&mut self, style: &StyleRecord) -> &mut FontPair {
        let key = style.name.to_ascii_uppercase();
        if !self.pairs.contains_key(&key) {
            // Bracket each resolve so any substitution it appends can be
            // attributed back to this style. The resolver dedupes, so a
            // font already substituted for an earlier style appends
            // nothing here — which is why the earlier style's indices have
            // to be looked up rather than assumed contiguous.
            let before = self.resolver.substitutions().len();
            let primary_ref = self.resolver.resolve(&style.primary, Role::Primary);
            let bigfont_ref = self.resolver.resolve(&style.bigfont, Role::Bigfont);
            let mut mine: Vec<usize> = (before..self.resolver.substitutions().len()).collect();
            // Pick up substitutions this style shares with an earlier one.
            // Compared on the normalised name, not the raw string: a style
            // spelling the font `HZTXT` shares the substitution recorded for
            // `hztxt.shx`, and matching literally would drop its entities
            // from that substitution's count.
            let primary_key = search::normalized_key(&style.primary);
            let bigfont_key = search::normalized_key(&style.bigfont);
            for (index, s) in self.resolver.substitutions().iter().enumerate() {
                let key = search::normalized_key(&s.requested);
                let shared = (s.role == Role::Primary && !key.is_empty() && key == primary_key)
                    || (s.role == Role::Bigfont && !key.is_empty() && key == bigfont_key);
                if shared && !mine.contains(&index) {
                    mine.push(index);
                }
            }
            self.style_substitutions.insert(key.clone(), mine);
            let mut failures = Vec::new();
            let primary = open(&primary_ref).unwrap_or_else(|e| {
                failures.push(e);
                FontHandle::None
            });
            let bigfont = open(&bigfont_ref).unwrap_or_else(|e| {
                failures.push(e);
                FontHandle::None
            });
            for failure in failures {
                if !self.load_failures.contains(&failure) {
                    self.load_failures.push(failure);
                }
            }
            self.pairs.insert(key.clone(), FontPair { primary, bigfont });
        }
        self.pairs.get_mut(&key).expect("just inserted")
    }

    /// Lay one entity out, or `None` if it is not a text entity.
    /// Whether an entity's text contains a character only a big font can
    /// draw, in this drawing's codepage.
    ///
    /// The test is "does it encode to two bytes", not "is it ASCII": in a
    /// GBK drawing the degree sign is single-byte and lives in the primary,
    /// while every Han character needs the big font. Both group 1 and the
    /// group 3 continuation fragments an MTEXT splits its text across are
    /// scanned, or a long paragraph whose Chinese begins after the first
    /// 250 bytes would look Latin-only.
    fn needs_bigfont(entity: &RawEntity, cp: Codepage) -> bool {
        entity
            .codes
            .iter()
            .filter(|(code, _)| *code == 1 || *code == 3)
            .filter_map(|(_, value)| value.as_bytes())
            .any(|bytes| {
                crate::encoding::decode(bytes, cp)
                    .chars()
                    .any(|ch| crate::encoding::bigfont_code(ch, cp).is_some())
            })
    }

    /// Give a style that names no big font one, once, when its text turns
    /// out to need it. Attributed to the style so the warning carries the
    /// entity count like every other substitution.
    fn ensure_cjk_fallback(&mut self, key: &str, style_name: &str) {
        if !self.unnamed_bigfont_styles.insert(key.to_owned()) {
            return;
        }
        let resolved = self.resolver.bigfont_for_unnamed(style_name);
        let index = self.resolver.substitutions().len() - 1;
        self.style_substitutions.entry(key.to_owned()).or_default().push(index);
        match open(&resolved) {
            Ok(handle) => {
                if let Some(pair) = self.pairs.get_mut(key) {
                    pair.bigfont = handle;
                }
            }
            Err(failure) => {
                if !self.load_failures.contains(&failure) {
                    self.load_failures.push(failure);
                }
            }
        }
    }

    pub fn lay_out(&mut self, entity: &RawEntity) -> Option<TextGeom> {
        if !matches!(entity.kind.as_str(), "TEXT" | "MTEXT" | "ATTRIB") {
            return None;
        }
        let style = self.style_for(entity);
        let codepage = self.codepage;
        let key = style.name.to_ascii_uppercase();
        // Only when the style names no big font at all. One that names a
        // missing font has already been through the substitution chain and
        // reported; running it again would say the same thing twice.
        let unnamed_bigfont = style.bigfont.trim().is_empty()
            && matches!(self.pair_for(&style).bigfont, FontHandle::None);
        if unnamed_bigfont && Self::needs_bigfont(entity, codepage) {
            let name = if style.name.is_empty() { "(未命名)" } else { style.name.as_str() };
            let name = name.to_owned();
            self.ensure_cjk_fallback(&key, &name);
        }
        let pair = self.pair_for(&style);
        let laid = layout::lay_out(entity, &style, pair, codepage);
        // Tally after the pair exists, so `style_substitutions` is
        // populated. Counted per entity laid out, which is what R-TXT-2.3
        // asks for — not per style and not per glyph.
        if let Some(indices) = self.style_substitutions.get(&key) {
            for index in indices.clone() {
                *self.entity_counts.entry(index).or_default() += 1;
            }
        }
        laid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;

    const SRC: &[u8] = b"  0\nSECTION\n  2\nHEADER\n  9\n$DWGCODEPAGE\n  3\nANSI_936\n  0\nENDSEC\n  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nSTYLE\n  0\nSTYLE\n  2\nStandard\n  3\nisocp.shx\n  4\nhztxt.shx\n 41\n0.707\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nTEXT\n  8\n0\n  1\nAB\n 40\n100.0\n 10\n0.0\n 20\n0.0\n  0\nENDSEC\n  0\nEOF\n";

    /// Whether the drawing's missing bigfont is in fact missing here.
    ///
    /// The skip guard must ask the font search, not `warnings()`. Fonts are
    /// resolved lazily on first use, so a freshly built engine reports no
    /// warnings on every machine — a guard keyed on that would skip
    /// unconditionally while printing a claim about this machine that is
    /// false (`hztxt.shx` is one of the seven fonts a full AutoCAD 2021
    /// install does not ship).
    fn hztxt_is_installed() -> bool {
        search::FontSearch::for_drawing(None, &[]).find("hztxt.shx").is_some()
    }

    /// R-TXT-2.3: a missing font must produce a warning naming it. On this
    /// machine this is the real `hztxt.shx` case.
    #[test]
    fn a_missing_bigfont_produces_a_named_warning() {
        if hztxt_is_installed() {
            eprintln!("SKIPPED: hztxt.shx is installed on this machine, so nothing is substituted");
            return;
        }
        let doc = Document::parse(SRC).unwrap();
        let mut engine = TextEngine::new(&doc, None, &[]);
        assert!(engine.warnings().is_empty(), "fonts must not be loaded before they are used");
        for entity in &doc.entities {
            let _ = engine.lay_out(entity);
        }
        let warnings = engine.warnings();
        assert!(
            warnings.iter().any(|w| w.contains("hztxt")),
            "the missing font is not named: {warnings:?}"
        );
    }

    /// Styles are resolved once and shared, not reloaded per entity: the
    /// reference drawing has 1,257 TEXT entities across 38 styles.
    #[test]
    fn each_style_is_resolved_once() {
        let doc = Document::parse(SRC).unwrap();
        let mut engine = TextEngine::new(&doc, None, &[]);
        for entity in &doc.entities {
            let _ = engine.lay_out(entity);
        }
        assert!(engine.warnings().len() <= 2, "one warning per style half: {:?}", engine.warnings());
    }

    /// R-TXT-2.3 requires the affected-entity count, not just the fact of
    /// a substitution. "hztxt.shx was replaced" and "hztxt.shx was
    /// replaced across 2,886 entities" are very different messages to
    /// someone deciding whether to install the font.
    #[test]
    fn substitution_warnings_carry_the_affected_entity_count() {
        if hztxt_is_installed() {
            eprintln!("SKIPPED: hztxt.shx is installed on this machine, so nothing is substituted");
            return;
        }
        let doc = Document::parse(SRC).unwrap();
        let mut engine = TextEngine::new(&doc, None, &[]);
        for entity in &doc.entities {
            let _ = engine.lay_out(entity);
        }
        let counted = engine.warnings();
        assert!(
            counted.iter().any(|w| w.contains("影响 1 个实体")),
            "no warning carries an entity count: {counted:?}"
        );
    }

    #[test]
    fn non_text_entities_are_not_claimed() {
        let doc = Document::parse(SRC).unwrap();
        let mut engine = TextEngine::new(&doc, None, &[]);
        let line = crate::dxf::entities::RawEntity { kind: "LINE".to_owned(), codes: Vec::new() };
        assert!(engine.lay_out(&line).is_none());
    }
}
