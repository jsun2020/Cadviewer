use cadviewer::converter::{ConvertOptions, scenes_for};
use cadviewer::doc::Document;
use cadviewer::plot::style::ColorMode;

/// Four title-block frames in a 2x2 grid, plus a group box around them.
fn multi_sheet_source() -> Vec<u8> {
    let frame = "\u{56fe}\u{6846}";
    let mut src = format!(
        "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nL\n 62\n7\n370\n25\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n\
  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\n{frame}\n\
  0\nLWPOLYLINE\n  8\nL\n 70\n1\n 10\n0.0\n 20\n0.0\n 10\n420.0\n 20\n0.0\n 10\n420.0\n 20\n297.0\n 10\n0.0\n 20\n297.0\n\
  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n"
    );
    for (x, y) in [(0.0, 400.0), (500.0, 400.0), (0.0, 0.0), (500.0, 0.0)] {
        src.push_str(&format!("  0\nINSERT\n  8\nL\n  2\n{frame}\n 10\n{x}\n 20\n{y}\n"));
    }
    // Group box enclosing all four.
    src.push_str("  0\nLWPOLYLINE\n  8\nPLOT\n 70\n1\n 10\n-50.0\n 20\n-50.0\n 10\n1000.0\n 20\n-50.0\n 10\n1000.0\n 20\n800.0\n 10\n-50.0\n 20\n800.0\n");
    src.push_str("  0\nENDSEC\n  0\nEOF\n");
    src.into_bytes()
}

#[test]
fn emits_one_scene_per_detected_frame() {
    let doc = Document::parse(&multi_sheet_source()).unwrap();
    let opts = ConvertOptions { mode: ColorMode::Color, sheet: None };
    let scenes = scenes_for(&doc, &opts).expect("should produce scenes");
    assert_eq!(scenes.len(), 4, "the group box must not add a fifth page");
}

#[test]
fn each_page_is_sized_for_its_frame_not_the_whole_drawing() {
    let doc = Document::parse(&multi_sheet_source()).unwrap();
    let opts = ConvertOptions { mode: ColorMode::Color, sheet: None };
    let scenes = scenes_for(&doc, &opts).unwrap();
    for s in &scenes {
        // A 420x297 frame is A3 landscape.
        assert!((s.paper.width_mm - 420.0).abs() < 1.0, "got {}", s.paper.width_mm);
        assert!((s.paper.height_mm - 297.0).abs() < 1.0, "got {}", s.paper.height_mm);
    }
}

#[test]
fn a_single_sheet_can_be_selected() {
    let doc = Document::parse(&multi_sheet_source()).unwrap();
    let opts = ConvertOptions { mode: ColorMode::Color, sheet: Some(2) };
    assert_eq!(scenes_for(&doc, &opts).unwrap().len(), 1);
}

#[test]
fn selecting_a_nonexistent_sheet_is_an_error_not_an_empty_pdf() {
    let doc = Document::parse(&multi_sheet_source()).unwrap();
    let opts = ConvertOptions { mode: ColorMode::Color, sheet: Some(99) };
    assert!(scenes_for(&doc, &opts).is_err());
}

#[test]
fn drawings_without_frames_still_produce_one_page() {
    let src = b"  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\n0\n 10\n0.0\n 20\n0.0\n 11\n100.0\n 21\n100.0\n  0\nENDSEC\n  0\nEOF\n";
    let doc = Document::parse(src).unwrap();
    let opts = ConvertOptions { mode: ColorMode::Color, sheet: None };
    let scenes = scenes_for(&doc, &opts).expect("must never fail on a frameless drawing");
    assert_eq!(scenes.len(), 1);
}
