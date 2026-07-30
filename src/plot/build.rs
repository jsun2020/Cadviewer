use std::collections::HashMap;

use crate::doc::Document;
use crate::dxfnew::entities::RawEntity;
use crate::geom::{Affine, Bounds, Point};
use crate::plot::flatten::flatten;
use crate::plot::style::{
    ColorMode, Inherited, resolve_color, resolve_dash_mm, resolve_width_mm,
};
use crate::plot::{PaperSize, PlotItem, PlotScene, StrokeStyle};

/// Guards against self-referential blocks, which do occur in damaged files.
const MAX_BLOCK_DEPTH: usize = 24;

#[derive(Clone, Debug)]
pub struct PlotRequest {
    /// Region of model space to plot, in drawing units.
    pub window: Bounds,
    pub paper: PaperSize,
    pub margin_mm: f64,
    pub mode: ColorMode,
}

#[derive(Clone, Debug, Default)]
pub struct BuildReport {
    pub items: usize,
    /// Entity kinds that were not drawn, with counts. Reported to the user
    /// rather than dropped in silence.
    pub skipped: HashMap<String, usize>,
}

pub fn model_extents(doc: &Document) -> Bounds {
    let mut b = Bounds::empty();
    for ent in &doc.entities {
        if let Some(f) = flatten(ent, Affine::identity()) {
            let fb = f.geom.bounds();
            if fb.valid() {
                b.add(Point::new(fb.min_x, fb.min_y));
                b.add(Point::new(fb.max_x, fb.max_y));
            }
        }
    }
    if !b.valid() {
        b.add(doc.header.extmin);
        b.add(doc.header.extmax);
    }
    b
}

/// Transform from drawing units to paper millimetres, preserving aspect
/// ratio and centring the window on the sheet.
fn plot_transform(req: &PlotRequest) -> (Affine, f64) {
    let avail_w = (req.paper.width_mm - 2.0 * req.margin_mm).max(1.0);
    let avail_h = (req.paper.height_mm - 2.0 * req.margin_mm).max(1.0);
    let win_w = req.window.width().max(f64::EPSILON);
    let win_h = req.window.height().max(f64::EPSILON);
    let scale = (avail_w / win_w).min(avail_h / win_h);

    let offset_x = req.margin_mm + (avail_w - win_w * scale) / 2.0;
    let offset_y = req.margin_mm + (avail_h - win_h * scale) / 2.0;

    let t = Affine::translation(-req.window.min_x, -req.window.min_y)
        .then(Affine::scale(scale, scale))
        .then(Affine::translation(offset_x, offset_y));
    (t, scale)
}

pub fn build(doc: &Document, req: &PlotRequest) -> (PlotScene, BuildReport) {
    let (transform, scale) = plot_transform(req);
    let mut scene = PlotScene::new(req.paper);
    let mut report = BuildReport::default();
    let inherited = Inherited::default();

    for ent in &doc.entities {
        emit(doc, ent, transform, scale, req, &inherited, 0, &mut scene, &mut report);
    }
    report.items = scene.items.len();
    (scene, report)
}

#[allow(clippy::too_many_arguments)]
fn emit(
    doc: &Document,
    ent: &RawEntity,
    transform: Affine,
    scale: f64,
    req: &PlotRequest,
    inherited: &Inherited,
    depth: usize,
    scene: &mut PlotScene,
    report: &mut BuildReport,
) {
    if depth > MAX_BLOCK_DEPTH {
        return;
    }
    let cp = doc.header.codepage;

    if ent.kind == "INSERT" {
        let Some(name) = ent.text(2, cp) else { return };
        let Some(body) = doc.blocks.get(&name) else {
            *report.skipped.entry("INSERT(missing block)".to_owned()).or_default() += 1;
            return;
        };
        let layer = doc.layer(&ent.layer(cp));
        let child = Inherited {
            color: resolve_color(ent, layer, inherited.color, req.mode),
            lineweight: ent.int(370, inherited.lineweight as i32) as i16,
            linetype: ent
                .text(6, cp)
                .filter(|s| !s.eq_ignore_ascii_case("BYLAYER") && !s.eq_ignore_ascii_case("BYBLOCK"))
                .unwrap_or_else(|| inherited.linetype.clone()),
        };
        let local = Affine::scale(ent.f64(41, 1.0), ent.f64(42, 1.0))
            .then(Affine::rotation(ent.f64(50, 0.0)))
            .then(Affine::translation(ent.f64(10, 0.0), ent.f64(20, 0.0)))
            .then(transform);
        for child_ent in body {
            emit(doc, child_ent, local, scale, req, &child, depth + 1, scene, report);
        }
        return;
    }

    let Some(flat) = flatten(ent, transform) else {
        *report.skipped.entry(ent.kind.clone()).or_default() += 1;
        return;
    };

    // Cull against the printable area. Done after transform so it costs one
    // bounds comparison per entity rather than an inverse transform.
    let b = flat.geom.bounds();
    if !b.valid()
        || b.max_x < 0.0
        || b.min_x > req.paper.width_mm
        || b.max_y < 0.0
        || b.min_y > req.paper.height_mm
    {
        return;
    }

    let layer = doc.layer(&ent.layer(cp));
    let color = resolve_color(ent, layer, inherited.color, req.mode);

    if flat.filled {
        scene.items.push(PlotItem::Fill { geom: flat.geom, color });
        return;
    }

    let style = StrokeStyle {
        color,
        width_mm: resolve_width_mm(ent, layer, inherited.lineweight, doc.header.celweight),
        dash_mm: resolve_dash_mm(
            ent,
            layer,
            &doc.ltypes,
            &inherited.linetype,
            doc.header.ltscale,
            scale,
            cp,
        ),
    };
    scene.items.push(PlotItem::Path { geom: flat.geom, style });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;

    const SRC: &[u8] = b"  0\nSECTION\n  2\nHEADER\n  9\n$LTSCALE\n 40\n1.0\n  0\nENDSEC\n  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nWALL\n 62\n1\n370\n35\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nBOX\n  0\nLINE\n  8\nWALL\n 10\n0.0\n 20\n0.0\n 11\n10.0\n 21\n0.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\nWALL\n 10\n0.0\n 20\n0.0\n 11\n100.0\n 21\n100.0\n  0\nINSERT\n  8\nWALL\n  2\nBOX\n 10\n50.0\n 20\n50.0\n 41\n2.0\n 42\n2.0\n  0\nENDSEC\n  0\nEOF\n";

    fn request(w: f64, h: f64) -> PlotRequest {
        let mut b = Bounds::empty();
        b.add(crate::geom::Point::new(0.0, 0.0));
        b.add(crate::geom::Point::new(w, h));
        PlotRequest {
            window: b,
            paper: PaperSize::a4_landscape(),
            margin_mm: 10.0,
            mode: ColorMode::Color,
        }
    }

    #[test]
    fn produces_items_for_root_entities_and_expanded_blocks() {
        let doc = Document::parse(SRC).unwrap();
        let (scene, report) = build(&doc, &request(100.0, 100.0));
        assert_eq!(scene.items.len(), 2, "one root LINE plus one from the block");
        assert_eq!(report.items, 2);
    }

    #[test]
    fn geometry_lands_inside_the_paper_with_margins() {
        let doc = Document::parse(SRC).unwrap();
        let req = request(100.0, 100.0);
        let (scene, _) = build(&doc, &req);
        for item in &scene.items {
            let PlotItem::Path { geom, .. } = item else { continue };
            let b = geom.bounds();
            assert!(b.min_x >= req.margin_mm - 0.01, "min_x {}", b.min_x);
            assert!(b.max_x <= scene.paper.width_mm - req.margin_mm + 0.01, "max_x {}", b.max_x);
            assert!(b.min_y >= req.margin_mm - 0.01, "min_y {}", b.min_y);
            assert!(b.max_y <= scene.paper.height_mm - req.margin_mm + 0.01, "max_y {}", b.max_y);
        }
    }

    #[test]
    fn lineweight_survives_into_the_scene_unscaled_by_the_plot_transform() {
        // A 0.35 mm line is 0.35 mm on paper no matter how the drawing is
        // scaled. This is the property the old SVG pipeline could not hold.
        let doc = Document::parse(SRC).unwrap();
        let (small, _) = build(&doc, &request(100.0, 100.0));
        let (large, _) = build(&doc, &request(100000.0, 100000.0));
        let width_of = |s: &PlotScene| match &s.items[0] {
            PlotItem::Path { style, .. } => style.width_mm,
            _ => panic!("expected a stroked path"),
        };
        assert_eq!(width_of(&small), 0.35);
        assert_eq!(width_of(&large), 0.35);
    }

    #[test]
    fn colour_comes_from_the_layer() {
        let doc = Document::parse(SRC).unwrap();
        let (scene, _) = build(&doc, &request(100.0, 100.0));
        let PlotItem::Path { style, .. } = &scene.items[0] else { panic!() };
        assert_eq!(style.color, crate::plot::Rgb::new(255, 0, 0), "layer WALL is ACI 1");
    }

    #[test]
    fn entities_outside_the_window_are_dropped() {
        let doc = Document::parse(SRC).unwrap();
        let mut tiny = Bounds::empty();
        tiny.add(crate::geom::Point::new(-1000.0, -1000.0));
        tiny.add(crate::geom::Point::new(-900.0, -900.0));
        let req = PlotRequest { window: tiny, ..request(100.0, 100.0) };
        let (scene, _) = build(&doc, &req);
        assert!(scene.items.is_empty(), "got {} items", scene.items.len());
    }

    #[test]
    fn unsupported_entity_kinds_are_counted_not_silently_dropped() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\n3DSOLID\n  8\n0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let (_, report) = build(&doc, &request(100.0, 100.0));
        assert_eq!(report.skipped.get("3DSOLID"), Some(&1));
    }

    #[test]
    fn model_extents_covers_all_root_geometry() {
        let doc = Document::parse(SRC).unwrap();
        let b = model_extents(&doc);
        assert!(b.valid());
        assert!(b.width() >= 100.0, "width {}", b.width());
    }
}
