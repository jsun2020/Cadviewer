use std::path::Path;
use std::process::Command;

/// Local-only sample. Reference drawings are never committed, so this test
/// skips loudly rather than failing when the file is absent.
fn sample_dwg() -> String {
    format!(
        "C:\\Users\\sr9rfx\\Desktop\\2_{}2023.12.12.dwg",
        "\u{56fd}\u{6fb3}\u{9879}\u{76ee}-\u{4e94}\u{5c42}\u{88c5}\u{4fee}\
         \u{5e73}\u{9762}\u{56fe}\u{ff08}\u{5de6}\u{4fa7}\u{ff09}"
    )
}

fn to_dxf(dwg: &str, out: &Path) -> bool {
    let exe = Path::new("runtime").join("dwg2dxf.exe");
    if !exe.exists() {
        return false;
    }
    Command::new(exe)
        .args(["-y", "-o"])
        .arg(out)
        .arg(dwg)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn parses_the_reference_drawing() {
    let dwg = sample_dwg();
    if !Path::new(&dwg).exists() {
        eprintln!("SKIPPED: sample DWG not present at {dwg}");
        return;
    }
    let tmp = std::env::temp_dir().join("cadviewer_phase1.dxf");
    if !to_dxf(&dwg, &tmp) {
        eprintln!("SKIPPED: runtime/dwg2dxf.exe unavailable");
        return;
    }
    let bytes = std::fs::read(&tmp).expect("read intermediate DXF");
    let doc = cadviewer::doc::Document::parse(&bytes).expect("parse should succeed");

    assert_eq!(doc.header.codepage, cadviewer::encoding::Codepage::Gbk);

    // R-ENC verification against the AutoCAD reference (PRD 3.9 / 3.10).
    // These layer names render correctly in AutoCAD's own PDF, so they must
    // render correctly here.
    for expected in ["\u{56fe}\u{6846}", "\u{5c3a}\u{5bf8}\u{6807}\u{6ce8}"] {
        assert!(
            doc.layers.contains_key(expected),
            "layer {expected:?} missing; decoded layers include {:?}",
            doc.layers.keys().take(20).collect::<Vec<_>>()
        );
    }

    // The title-block frame block that drives sheet detection in Phase 3.
    let frames = doc
        .entities
        .iter()
        .filter(|e| e.kind == "INSERT")
        .filter(|e| {
            e.text(2, doc.header.codepage)
                .is_some_and(|n| n.contains("\u{56fe}\u{6846}"))
        })
        .count();
    assert_eq!(frames, 26, "expected 26 title-block inserts (PRD 3.10.1)");
}

#[test]
fn detects_twenty_six_sheets_in_reading_order() {
    let dwg = sample_dwg();
    if !Path::new(&dwg).exists() {
        eprintln!("SKIPPED: sample DWG not present at {dwg}");
        return;
    }
    let tmp = std::env::temp_dir().join("cadviewer_sheets.dxf");
    if !to_dxf(&dwg, &tmp) {
        eprintln!("SKIPPED: runtime/dwg2dxf.exe unavailable");
        return;
    }
    let bytes = std::fs::read(&tmp).expect("read intermediate DXF");
    let doc = cadviewer::doc::Document::parse(&bytes).expect("parse");
    let sheets = cadviewer::sheets::detect(&doc);

    // PRD 3.10.1: 26 title-block inserts in rows of 2/2/1/1/6/6/4/3/1.
    assert_eq!(sheets.len(), 26, "expected 26 sheets, got {}", sheets.len());

    // Group the detected sheets back into rows and check the shape.
    let mut rows: Vec<usize> = Vec::new();
    let mut current_y = f64::NAN;
    for s in &sheets {
        if current_y.is_nan() || (current_y - s.bounds.min_y).abs() > s.bounds.height() / 2.0 {
            rows.push(0);
            current_y = s.bounds.min_y;
        }
        *rows.last_mut().unwrap() += 1;
    }
    assert_eq!(rows, vec![2, 2, 1, 1, 6, 6, 4, 3, 1], "row shape mismatch");
}

#[test]
fn group_annotation_boxes_never_become_pages() {
    // PRD 3.10.2: three rectangles on a layer named for plotting enclose
    // groups of frames. If any survives, pages collapse silently.
    let dwg = sample_dwg();
    if !Path::new(&dwg).exists() {
        eprintln!("SKIPPED: sample DWG not present at {dwg}");
        return;
    }
    let tmp = std::env::temp_dir().join("cadviewer_groups.dxf");
    if !to_dxf(&dwg, &tmp) {
        eprintln!("SKIPPED: runtime/dwg2dxf.exe unavailable");
        return;
    }
    let bytes = std::fs::read(&tmp).expect("read intermediate DXF");
    let doc = cadviewer::doc::Document::parse(&bytes).expect("parse");
    for s in cadviewer::sheets::detect(&doc) {
        assert!(
            s.bounds.width() < 400_000.0,
            "sheet {} spans {} units; that is a group box, not a frame",
            s.index,
            s.bounds.width()
        );
    }
}
