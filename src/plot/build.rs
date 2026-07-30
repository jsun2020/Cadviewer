use std::collections::HashMap;

use crate::doc::Document;
use crate::dxf::entities::RawEntity;
use crate::geom::{Affine, Bounds, Point};
use crate::plot::flatten::flatten;
use crate::plot::style::{
    ColorMode, Inherited, resolve_color, resolve_dash_mm, resolve_linetype_name, resolve_raw_width,
    resolve_width_mm,
};
use crate::plot::{PaperSize, PlotItem, PlotScene, StrokeStyle};

/// Guards against self-referential blocks, which do occur in damaged files.
const MAX_BLOCK_DEPTH: usize = 24;

/// Total number of entity expansions one page may cost.
///
/// The depth cap alone bounds nesting but not work: the branching factor is
/// unbounded, so a self-referential block holding two references to itself
/// reaches 2^24 = 16.7 million items from an input smaller than this
/// comment — minutes of CPU and gigabytes of `PlotItem`s. The budget is set
/// comfortably above the real reference drawing (about 3.8 million
/// expansions per page) so honest files are unaffected, and an abort is
/// recorded in the report rather than the page silently coming out partial.
const MAX_EXPANDED_ENTITIES: usize = 8_000_000;

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
    /// Entities visited, including every expansion of every block body.
    pub expanded: usize,
    /// Set when the expansion budget ran out, so the caller can say the
    /// page is incomplete instead of presenting a truncated drawing as a
    /// finished one.
    pub truncated: bool,
}

pub fn model_extents(doc: &Document) -> Bounds {
    let mut b = Bounds::empty();
    for ent in &doc.entities {
        if ent.paper_space() {
            continue;
        }
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
    build_within(doc, req, MAX_EXPANDED_ENTITIES)
}

/// [`build`] with an explicit expansion budget, so the truncation path can
/// be tested in milliseconds instead of by actually spending the real
/// budget.
pub(crate) fn build_within(
    doc: &Document,
    req: &PlotRequest,
    budget: usize,
) -> (PlotScene, BuildReport) {
    let (transform, scale) = plot_transform(req);
    let mut scene = PlotScene::new(req.paper);
    scene.clip = Some(printable_area(req));
    let mut report = BuildReport::default();
    let inherited = Inherited::default();

    for ent in &doc.entities {
        if ent.paper_space() {
            // R-SHEET-1 (layouts) is a separate feature; a paper-space
            // entity drawn into the model scene is a misplaced duplicate.
            *report.skipped.entry("图纸空间实体".to_owned()).or_default() += 1;
            continue;
        }
        emit(doc, ent, transform, scale, req, &inherited, 0, budget, &mut scene, &mut report);
    }
    report.items = scene.items.len();
    (scene, report)
}

/// The area of the sheet that may be inked, in paper millimetres.
fn printable_area(req: &PlotRequest) -> Bounds {
    let mut b = Bounds::empty();
    b.add(Point::new(req.margin_mm, req.margin_mm));
    b.add(Point::new(
        req.paper.width_mm - req.margin_mm,
        req.paper.height_mm - req.margin_mm,
    ));
    b
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
    budget: usize,
    scene: &mut PlotScene,
    report: &mut BuildReport,
) {
    if depth > MAX_BLOCK_DEPTH {
        *report.skipped.entry("超出块嵌套深度".to_owned()).or_default() += 1;
        return;
    }
    if report.truncated {
        return;
    }
    report.expanded += 1;
    if report.expanded > budget {
        report.truncated = true;
        return;
    }
    let cp = doc.header.codepage;

    // A DIMENSION's linework lives in an anonymous block named in group 2,
    // and is drawn by expanding that block exactly as an INSERT is — the
    // difference is only that the block is already positioned in world
    // coordinates, so there is no local placement transform.
    if ent.kind == "INSERT" || ent.kind == "DIMENSION" {
        let layer = doc.layer(&ent.layer(cp));
        if !plotted(layer, report) {
            return;
        }
        let Some(name) = ent.text(2, cp) else { return };
        let Some(block) = doc.blocks.get(&name) else {
            *report
                .skipped
                .entry(format!("{}(missing block)", ent.kind))
                .or_default() += 1;
            return;
        };
        let child = Inherited {
            color: resolve_color(ent, layer, inherited.color, req.mode),
            // Resolve before descending, exactly as colour does. Passing the
            // raw group code down instead makes a child's ByBlock lookup see
            // the INSERT's *sentinel* (-1 "ByLayer"), fail, and fall back to
            // the child's own layer rather than the INSERT's.
            lineweight: resolve_raw_width(ent, layer, inherited.lineweight, doc.header.celweight),
            linetype: resolve_linetype_name(ent, layer, &inherited.linetype, cp),
        };
        let base = Affine::translation(-block.base.x, -block.base.y);
        let placements = if ent.kind == "DIMENSION" {
            vec![base.then(transform)]
        } else {
            insert_placements(ent, base, transform)
        };
        for local in placements {
            for child_ent in &block.entities {
                emit(doc, child_ent, local, scale, req, &child, depth + 1, budget, scene, report);
            }
        }
        return;
    }

    let Some(flat) = flatten(ent, transform) else {
        *report.skipped.entry(ent.kind.clone()).or_default() += 1;
        return;
    };

    // Cull against the printable area. Done after transform so it costs one
    // bounds comparison per entity rather than an inverse transform, and
    // before the layer lookup so the millions of entities a page discards
    // never pay for one. (An INSERT has to look its layer up before
    // descending, which is why that branch does it first.)
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
    if !plotted(layer, report) {
        return;
    }

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

/// Whether AutoCAD would put ink on paper for this entity's layer,
/// counting the drop when it would not.
///
/// A layer that is off, frozen or marked non-plotting produces no ink at
/// all. Since matching AutoCAD's plotted output is the whole purpose,
/// extra ink is a fidelity failure like any other — and an invisible one
/// until somebody overlays the reference.
fn plotted(layer: Option<&crate::dxf::tables::LayerRecord>, report: &mut BuildReport) -> bool {
    match layer {
        Some(record) if !record.plotted() => {
            *report.skipped.entry("不打印图层上的实体".to_owned()).or_default() += 1;
            false
        }
        _ => true,
    }
}

/// Highest column/row count an INSERT array may claim.
///
/// A rectangular array (MINSERT) is written as an ordinary INSERT carrying
/// a column count in 70 and a row count in 71. Real arrays are small; a
/// four-digit count is corruption, and multiplying it by the block body is
/// how a damaged file turns into an out-of-memory abort.
const MAX_ARRAY_COUNT: i32 = 1_000;

/// One transform per copy an INSERT places.
///
/// Normally that is a single placement. An INSERT with a column or row
/// count above one is a rectangular array: the same block repeated on a
/// grid whose spacing (44/45) is measured along the insert's own rotated
/// axes, which is why the offset is applied before the rotation.
fn insert_placements(ent: &RawEntity, base: Affine, transform: Affine) -> Vec<Affine> {
    let scale = Affine::scale(ent.f64(41, 1.0), ent.f64(42, 1.0));
    let rotate = Affine::rotation(ent.f64(50, 0.0));
    let place = Affine::translation(ent.f64(10, 0.0), ent.f64(20, 0.0));

    let columns = ent.int(70, 1).clamp(1, MAX_ARRAY_COUNT);
    let rows = ent.int(71, 1).clamp(1, MAX_ARRAY_COUNT);
    if columns == 1 && rows == 1 {
        return vec![base.then(scale).then(rotate).then(place).then(transform)];
    }

    let column_spacing = ent.f64(44, 0.0);
    let row_spacing = ent.f64(45, 0.0);
    let mut out = Vec::with_capacity((columns * rows) as usize);
    for row in 0..rows {
        for column in 0..columns {
            let offset = Affine::translation(
                column as f64 * column_spacing,
                row as f64 * row_spacing,
            );
            out.push(base.then(scale).then(offset).then(rotate).then(place).then(transform));
        }
    }
    out
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

    /// I1: a DIMENSION's linework lives in an anonymous block named in
    /// group 2. Without expanding it, every dimension line, extension line
    /// and arrowhead in the drawing is missing from the PDF — 74 of them in
    /// the reference drawing — while nothing reports a problem.
    #[test]
    fn dimension_geometry_blocks_are_expanded_like_inserts() {
        let src = b"  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\n*D1\n 10\n0.0\n 20\n0.0\n  0\nLINE\n  8\n0\n 10\n10.0\n 20\n10.0\n 11\n90.0\n 21\n10.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nDIMENSION\n  8\n0\n  2\n*D1\n 10\n50.0\n 20\n50.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let (scene, report) = build(&doc, &request(100.0, 100.0));
        assert_eq!(scene.items.len(), 1, "the dimension's linework must be drawn");
        assert!(!report.skipped.contains_key("DIMENSION"), "{:?}", report.skipped);
    }

    /// The dimension block is already positioned in world coordinates, so
    /// it must not be moved to the DIMENSION's definition point.
    #[test]
    fn a_dimension_block_is_not_displaced_by_its_definition_point() {
        let src = b"  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\n*D1\n 10\n0.0\n 20\n0.0\n  0\nLINE\n  8\n0\n 10\n10.0\n 20\n10.0\n 11\n90.0\n 21\n10.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nDIMENSION\n  8\n0\n  2\n*D1\n 10\n5000.0\n 20\n5000.0\n  0\nLINE\n  8\n0\n 10\n10.0\n 20\n10.0\n 11\n90.0\n 21\n10.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let (scene, _) = build(&doc, &request(100.0, 100.0));
        assert_eq!(scene.items.len(), 2);
        let bounds_of = |item: &PlotItem| match item {
            PlotItem::Path { geom, .. } => geom.bounds(),
            PlotItem::Fill { geom, .. } => geom.bounds(),
        };
        let a = bounds_of(&scene.items[0]);
        let b = bounds_of(&scene.items[1]);
        assert!((a.min_x - b.min_x).abs() < 0.01, "dimension line moved: {a:?} vs {b:?}");
    }

    /// I2: a block authored "ByBlock" — the standard way to make a reusable
    /// symbol — must take the width its INSERT resolves to. Passing the
    /// INSERT's raw group code down instead makes the child see the -1
    /// "ByLayer" sentinel, fail the ByBlock lookup, and fall back to its
    /// own layer. Here that difference is 1.00 mm against 0.25 mm.
    #[test]
    fn byblock_children_inherit_the_inserts_resolved_lineweight() {
        let src = b"  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nLOUD\n 62\n1\n370\n100\n  6\nCONTINUOUS\n  0\nLAYER\n  2\nQUIET\n 62\n2\n370\n25\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nSYM\n 10\n0.0\n 20\n0.0\n  0\nLINE\n  8\nQUIET\n370\n-2\n 10\n10.0\n 20\n10.0\n 11\n90.0\n 21\n90.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  8\nLOUD\n  2\nSYM\n 10\n0.0\n 20\n0.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let (scene, _) = build(&doc, &request(100.0, 100.0));
        let PlotItem::Path { style, .. } = &scene.items[0] else { panic!("expected a stroke") };
        assert_eq!(style.width_mm, 1.00, "ByBlock must follow the INSERT's layer");
    }

    /// The same defect on the linetype channel: a ByBlock child must dash
    /// the way the INSERT's layer dashes.
    #[test]
    fn byblock_children_inherit_the_inserts_resolved_linetype() {
        let src = b"  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nDASHED\n 62\n1\n370\n25\n  6\nHIDDEN\n  0\nLAYER\n  2\nSOLID\n 62\n2\n370\n25\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nTABLE\n  2\nLTYPE\n  0\nLTYPE\n  2\nHIDDEN\n 49\n6.35\n 49\n-3.175\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nSYM\n 10\n0.0\n 20\n0.0\n  0\nLINE\n  8\nSOLID\n  6\nBYBLOCK\n 10\n10.0\n 20\n10.0\n 11\n90.0\n 21\n90.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  8\nDASHED\n  2\nSYM\n 10\n0.0\n 20\n0.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let (scene, _) = build(&doc, &request(100.0, 100.0));
        let PlotItem::Path { style, .. } = &scene.items[0] else { panic!("expected a stroke") };
        assert!(style.dash_mm.is_some(), "ByBlock must follow the INSERT's HIDDEN linetype");
    }

    /// I5: the BLOCK record's base point is the point of the block's own
    /// coordinate system that lands on the insertion point. A block drawn
    /// around (100, 50) and given that base point must plot exactly where
    /// the same block drawn around the origin plots.
    #[test]
    fn a_block_is_placed_relative_to_its_base_point() {
        let at_origin = b"  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nB\n 10\n0.0\n 20\n0.0\n  0\nLINE\n  8\n0\n 10\n0.0\n 20\n0.0\n 11\n10.0\n 21\n0.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  8\n0\n  2\nB\n 10\n40.0\n 20\n40.0\n  0\nENDSEC\n  0\nEOF\n";
        let offset = b"  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nB\n 10\n100.0\n 20\n50.0\n  0\nLINE\n  8\n0\n 10\n100.0\n 20\n50.0\n 11\n110.0\n 21\n50.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  8\n0\n  2\nB\n 10\n40.0\n 20\n40.0\n  0\nENDSEC\n  0\nEOF\n";
        let bounds_of = |src: &[u8]| {
            let doc = Document::parse(src).unwrap();
            let (scene, _) = build(&doc, &request(100.0, 100.0));
            match &scene.items[0] {
                PlotItem::Path { geom, .. } => geom.bounds(),
                _ => panic!("expected a stroke"),
            }
        };
        let a = bounds_of(at_origin);
        let b = bounds_of(offset);
        assert!(
            (a.min_x - b.min_x).abs() < 0.01 && (a.min_y - b.min_y).abs() < 0.01,
            "base point ignored: {a:?} vs {b:?}"
        );
    }

    /// An INSERT carrying a column and row count is a rectangular array.
    #[test]
    fn an_insert_array_places_one_copy_per_cell() {
        let src = b"  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nB\n 10\n0.0\n 20\n0.0\n  0\nLINE\n  8\n0\n 10\n0.0\n 20\n0.0\n 11\n5.0\n 21\n0.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  8\n0\n  2\nB\n 10\n10.0\n 20\n10.0\n 70\n3\n 71\n2\n 44\n20.0\n 45\n20.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let (scene, _) = build(&doc, &request(100.0, 100.0));
        assert_eq!(scene.items.len(), 6, "3 columns x 2 rows");
    }

    /// I6: the depth cap bounds nesting but not work. A block holding two
    /// references to itself expands 2^depth times, so a file smaller than
    /// this comment runs for minutes and allocates gigabytes. The budget
    /// must stop it and say so, not produce a silently partial page.
    #[test]
    fn a_self_referential_block_is_truncated_and_reported() {
        let src = b"  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nBOMB\n 10\n0.0\n 20\n0.0\n  0\nLINE\n  8\n0\n 10\n0.0\n 20\n0.0\n 11\n10.0\n 21\n10.0\n  0\nINSERT\n  8\n0\n  2\nBOMB\n 10\n0.0\n 20\n0.0\n  0\nINSERT\n  8\n0\n  2\nBOMB\n 10\n1.0\n 20\n1.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nINSERT\n  8\n0\n  2\nBOMB\n 10\n0.0\n 20\n0.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let (scene, report) = build_within(&doc, &request(100.0, 100.0), 5_000);
        assert!(report.truncated, "the expansion budget must fire");
        assert!(report.expanded <= 5_001, "budget overrun: {}", report.expanded);
        assert!(scene.items.len() < 5_001, "got {} items", scene.items.len());
    }

    /// An honest drawing must never trip the budget or report truncation.
    #[test]
    fn ordinary_drawings_are_not_reported_as_truncated() {
        let doc = Document::parse(SRC).unwrap();
        let (_, report) = build(&doc, &request(100.0, 100.0));
        assert!(!report.truncated);
        assert_eq!(report.expanded, 3, "one LINE, one INSERT, one block LINE");
    }

    /// I9: AutoCAD puts no ink on paper for a layer that is off, frozen or
    /// marked non-plotting. Drawing them is extra ink the reference PDF
    /// does not have, and it is invisible until someone overlays the two.
    #[test]
    fn entities_on_layers_autocad_would_not_plot_are_skipped() {
        // Layer OFF has a negative colour (switched off), FROZEN has bit 1
        // of group 70, NOPLOT has group 290 = 0, and VISIBLE has none.
        let src = b"  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nOFF\n 62\n-1\n370\n25\n  6\nCONTINUOUS\n  0\nLAYER\n  2\nFROZEN\n 62\n1\n 70\n1\n370\n25\n  6\nCONTINUOUS\n  0\nLAYER\n  2\nNOPLOT\n 62\n1\n290\n0\n370\n25\n  6\nCONTINUOUS\n  0\nLAYER\n  2\nVISIBLE\n 62\n1\n370\n25\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\nOFF\n 10\n0.0\n 20\n0.0\n 11\n90.0\n 21\n90.0\n  0\nLINE\n  8\nFROZEN\n 10\n0.0\n 20\n0.0\n 11\n90.0\n 21\n90.0\n  0\nLINE\n  8\nNOPLOT\n 10\n0.0\n 20\n0.0\n 11\n90.0\n 21\n90.0\n  0\nLINE\n  8\nVISIBLE\n 10\n0.0\n 20\n0.0\n 11\n90.0\n 21\n90.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let (scene, report) = build(&doc, &request(100.0, 100.0));
        assert_eq!(scene.items.len(), 1, "only the visible layer should be inked");
        assert_eq!(report.skipped.values().sum::<usize>(), 3, "{:?}", report.skipped);
    }

    /// Paper-space entities belong to a layout, not to the model scene.
    #[test]
    fn paper_space_entities_are_not_plotted_into_the_model() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\n0\n 10\n0.0\n 20\n0.0\n 11\n90.0\n 21\n90.0\n  0\nLINE\n  8\n0\n 67\n1\n 10\n0.0\n 20\n0.0\n 11\n90.0\n 21\n90.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let (scene, _) = build(&doc, &request(100.0, 100.0));
        assert_eq!(scene.items.len(), 1);
    }

    /// I8: the scene must carry the printable area so the renderers can
    /// clip to it; without it a straddling entity is inked into the margin.
    #[test]
    fn the_scene_carries_the_printable_area() {
        let doc = Document::parse(SRC).unwrap();
        let req = request(100.0, 100.0);
        let (scene, _) = build(&doc, &req);
        let clip = scene.clip.expect("the builder must set a clip");
        assert!((clip.min_x - req.margin_mm).abs() < 0.01);
        assert!((clip.max_x - (req.paper.width_mm - req.margin_mm)).abs() < 0.01);
    }

    #[test]
    fn model_extents_covers_all_root_geometry() {
        let doc = Document::parse(SRC).unwrap();
        let b = model_extents(&doc);
        assert!(b.valid());
        assert!(b.width() >= 100.0, "width {}", b.width());
    }
}
