use std::collections::BTreeSet;

use cadviewer::doc::Document;
use cadviewer::geom::{Bounds, Point};
use cadviewer::plot::build::{PlotRequest, build, model_extents};
use cadviewer::plot::style::ColorMode;
use cadviewer::plot::{PaperSize, PlotItem};

/// Build a synthetic drawing containing exactly the 13 lineweights AutoCAD
/// emitted for the reference sheet. Unlike the real DWG this needs no
/// external file, so the gate runs everywhere.
fn synthetic_drawing() -> Vec<u8> {
    let weights = [0, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 60, 100];
    let mut src = String::from(
        "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nL\n 62\n7\n370\n-3\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n",
    );
    for (i, w) in weights.iter().enumerate() {
        let y = i as f64 * 10.0;
        src.push_str(&format!(
            "  0\nLINE\n  8\nL\n 10\n0.0\n 20\n{y}\n 11\n100.0\n 21\n{y}\n370\n{w}\n"
        ));
    }
    src.push_str("  0\nENDSEC\n  0\nEOF\n");
    src.into_bytes()
}

#[test]
fn every_autocad_lineweight_survives_the_pipeline() {
    let doc = Document::parse(&synthetic_drawing()).expect("parse");
    let window = model_extents(&doc);
    let req = PlotRequest {
        window,
        paper: PaperSize::a4_landscape(),
        margin_mm: 10.0,
        mode: ColorMode::Color,
    };
    let (scene, _) = build(&doc, &req);

    let mut widths = BTreeSet::new();
    for item in &scene.items {
        if let PlotItem::Path { style, .. } = item {
            widths.insert((style.width_mm * 100.0).round() as i32);
        }
    }

    let expected: BTreeSet<i32> =
        [0, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 60, 100].into_iter().collect();
    assert_eq!(
        widths, expected,
        "scene widths (in 1/100 mm) do not match the 13 values AutoCAD emits (PRD 3.9.2)"
    );
}

#[test]
fn lineweight_is_independent_of_drawing_scale() {
    // The defect that motivated this rebuild: the old pipeline expressed
    // stroke width in normalised viewBox units, so the same drawing plotted
    // at a different extent produced different plotted widths.
    let doc = Document::parse(&synthetic_drawing()).expect("parse");
    let widths_for = |scale: f64| -> Vec<i32> {
        let mut b = Bounds::empty();
        b.add(Point::new(0.0, 0.0));
        b.add(Point::new(100.0 * scale, 130.0 * scale));
        let req = PlotRequest {
            window: b,
            paper: PaperSize::a4_landscape(),
            margin_mm: 10.0,
            mode: ColorMode::Color,
        };
        let (scene, _) = build(&doc, &req);
        scene
            .items
            .iter()
            .filter_map(|i| match i {
                PlotItem::Path { style, .. } => Some((style.width_mm * 100.0).round() as i32),
                _ => None,
            })
            .collect()
    };
    assert_eq!(widths_for(1.0), widths_for(1000.0));
}
