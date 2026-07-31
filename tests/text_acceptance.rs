use std::path::Path;
use std::process::Command;

use cadviewer::converter::ConvertOptions;
use cadviewer::doc::Document;
use cadviewer::plot::build::{PlotRequest, build_with_text};
use cadviewer::plot::style::ColorMode;
use cadviewer::plot::{PaperSize, PlotItem};
use cadviewer::text::TextEngine;

/// Local-only sample. Reference drawings are never committed, so this
/// skips loudly rather than failing when the file is absent. The Chinese
/// path is built from escapes: it must never reach a shell literal.
fn sample_dwg() -> String {
    format!(
        "C:\\Users\\sr9rfx\\Desktop\\2_{}2023.12.12.dwg",
        "\u{56fd}\u{6fb3}\u{9879}\u{76ee}-\u{4e94}\u{5c42}\u{88c5}\u{4fee}\
         \u{5e73}\u{9762}\u{56fe}\u{ff08}\u{5de6}\u{4fa7}\u{ff09}"
    )
}

fn load_reference(tag: &str) -> Option<Document> {
    let dwg = sample_dwg();
    if !Path::new(&dwg).exists() {
        eprintln!("SKIPPED: sample DWG not present at {dwg}");
        return None;
    }
    let exe = Path::new("runtime").join("dwg2dxf.exe");
    if !exe.exists() {
        eprintln!("SKIPPED: runtime/dwg2dxf.exe unavailable");
        return None;
    }
    let out = std::env::temp_dir().join(format!("cadviewer_{tag}.dxf"));
    let ok = Command::new(exe)
        .args(["-y", "-o"])
        .arg(&out)
        .arg(&dwg)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        eprintln!("SKIPPED: dwg2dxf failed");
        return None;
    }
    let bytes = std::fs::read(&out).ok()?;
    Document::parse(&bytes).ok()
}

/// R-TXT-6.5: after Phase 4, no text entity may end up in the skipped
/// report. Every one of them must produce glyphs or be reported as a
/// named font failure — never silently absent.
#[test]
fn no_text_entity_is_skipped_on_the_reference_drawing() {
    let Some(doc) = load_reference("text_skip") else { return };
    let sheets = cadviewer::sheets::detect(&doc);
    let sheet = sheets.first().expect("the reference drawing has 26 sheets");
    let mut text = TextEngine::new(&doc, Some(Path::new(&sample_dwg())), &[]);
    let req = PlotRequest {
        window: sheet.bounds,
        paper: PaperSize::fit(sheet.bounds.width(), sheet.bounds.height()),
        margin_mm: cadviewer::converter::DEFAULT_MARGIN_MM,
        mode: ColorMode::Color,
    };
    let (scene, report) = build_with_text(&doc, &req, Some(&mut text));

    for kind in ["TEXT", "MTEXT", "ATTRIB"] {
        assert_eq!(
            report.skipped.get(kind),
            None,
            "{kind} is still being skipped: {:?}; font warnings: {:?}",
            report.skipped,
            text.warnings()
        );
    }
    assert_eq!(report.skipped.get("ATTDEF"), None, "ATTDEF must not be reported at all");

    let glyph_runs = scene.items.iter().filter(|i| matches!(i, PlotItem::Glyphs(_))).count();
    assert!(glyph_runs > 100, "only {glyph_runs} glyph runs on a sheet with a full title block");
}

/// R-TXT-2.3 on real data: `hztxt.shx` is missing even on a machine with
/// a full AutoCAD install, so the warning must name it and name the
/// substitute. A silent swap is the defect.
#[test]
fn the_missing_hztxt_font_is_reported_with_its_substitute() {
    let Some(doc) = load_reference("text_warn") else { return };
    let mut engine = TextEngine::new(&doc, Some(Path::new(&sample_dwg())), &[]);
    // Fonts resolve lazily, so every style has to be touched before the
    // warning list means anything.
    for entity in &doc.entities {
        let _ = engine.lay_out(entity);
    }
    let warnings = engine.warnings();
    eprintln!("font warnings: {warnings:#?}");
    match warnings.iter().find(|w| w.contains("hztxt")) {
        Some(line) => {
            assert!(line.contains("gbcbig"), "the CJK substitute should be gbcbig, got: {line}")
        }
        None => eprintln!("SKIPPED: hztxt.shx appears to be installed here"),
    }
}

/// Text that lays out to nothing is invisible to the skipped report, so
/// the test above cannot see it: for 144 of this drawing's entities —
/// including the sheet's own title, in eight styles that name no font at
/// all — every Chinese character silently produced no geometry while the
/// report stayed clean. Assert on the geometry itself.
///
/// Blocks are walked as well as the root: the title block's 31 labels live
/// in a block definition, and a root-only sweep would have passed while
/// they were specks three units tall.
#[test]
fn every_chinese_entity_produces_geometry() {
    let Some(doc) = load_reference("text_ink") else { return };
    let cp = doc.header.codepage;
    let mut engine = TextEngine::new(&doc, Some(Path::new(&sample_dwg())), &[]);

    let mut entities: Vec<&cadviewer::dxf::entities::RawEntity> = doc.entities.iter().collect();
    for block in doc.blocks.values() {
        entities.extend(block.entities.iter());
    }

    let mut drawn = 0usize;
    let mut empty: Vec<String> = Vec::new();
    for entity in entities {
        if !matches!(entity.kind.as_str(), "TEXT" | "MTEXT" | "ATTRIB") {
            continue;
        }
        let raw = entity.text(1, cp).unwrap_or_default();
        if !raw.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)) {
            continue;
        }
        let strokes = engine
            .lay_out(entity)
            .map(|g| g.stroked.len() + g.filled.len())
            .unwrap_or(0);
        if strokes == 0 {
            if empty.len() < 10 {
                empty.push(format!(
                    "{} style={:?} text={:?}",
                    entity.kind,
                    entity.text(7, cp).unwrap_or_default(),
                    raw.chars().take(8).collect::<String>()
                ));
            }
        } else {
            drawn += 1;
        }
    }
    assert!(drawn > 500, "only {drawn} Chinese entities drew anything");
    assert!(empty.is_empty(), "Chinese text drew nothing, with no warning: {empty:#?}");
}

/// The same defect on the size axis: geometry existed but was 160 times
/// too small, which no stroke count can catch. A title-block label 556
/// units high must produce ink of that order, not of the 3.5 its style
/// fixes.
#[test]
fn title_block_labels_are_drawn_at_the_entitys_own_height() {
    let Some(doc) = load_reference("text_height") else { return };
    let cp = doc.header.codepage;
    let mut engine = TextEngine::new(&doc, Some(Path::new(&sample_dwg())), &[]);

    let mut checked = 0usize;
    for block in doc.blocks.values() {
        for entity in &block.entities {
            if entity.kind != "TEXT" {
                continue;
            }
            let height = entity.f64(40, 0.0);
            if height < 100.0 {
                continue;
            }
            let Some(geom) = engine.lay_out(entity) else { continue };
            let mut bounds = cadviewer::geom::Bounds::empty();
            for contour in geom.stroked.iter().chain(geom.filled.iter()) {
                for point in contour {
                    bounds.add(*point);
                }
            }
            if !bounds.valid() {
                continue;
            }
            checked += 1;
            assert!(
                bounds.height() > height * 0.5,
                "{:?} claims height {height} but inked only {}",
                entity.text(1, cp).unwrap_or_default(),
                bounds.height()
            );
        }
    }
    assert!(checked > 20, "only {checked} block texts were tall enough to check");
}

/// R-TXT-6.5 end to end: the exported PDF gains real content from text.
#[test]
fn exporting_one_sheet_produces_a_pdf_containing_text_geometry() {
    let Some(_doc) = load_reference("text_pdf") else { return };
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("sheet1.pdf");
    let options = ConvertOptions { sheet: Some(1), ..ConvertOptions::default() };
    let (pages, warnings) =
        cadviewer::converter::convert_to_pdf(Path::new(&sample_dwg()), &out, &options)
            .expect("export should succeed");
    assert_eq!(pages, 1);
    eprintln!("font warnings: {warnings:#?}");
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    // A title block full of text is worth tens of kilobytes even
    // compressed; a blank one is not.
    assert!(bytes.len() > 20_000, "the exported page is only {} bytes", bytes.len());
}
