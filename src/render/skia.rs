use tiny_skia::{
    Color, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, StrokeDash, Transform,
};

use crate::plot::{PlotItem, PlotScene, Rgb};

fn paint_for(color: Rgb) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(color.r, color.g, color.b, 255));
    paint.anti_alias = true;
    paint
}

/// Colour of the area around the sheet in the viewer, matching the app's
/// central panel so the sheet reads as paper on a desk.
pub const DESK_RGB: (u8, u8, u8) = (29, 33, 40);

/// Rasterise a scene at the given zoom. `pixels_per_mm` is the only view
/// parameter: panning is done by the caller cropping or offsetting the
/// resulting pixmap.
pub fn render(scene: &PlotScene, pixels_per_mm: f32) -> Pixmap {
    let w = (scene.paper.width_mm as f32 * pixels_per_mm).ceil().max(1.0) as u32;
    let h = (scene.paper.height_mm as f32 * pixels_per_mm).ceil().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    pixmap.fill(Color::WHITE);
    draw_into(&mut pixmap, scene, pixels_per_mm, 0.0, 0.0);
    pixmap
}

/// Rasterise only the `width` x `height` pixel window whose top-left corner
/// sits at `(origin_x, origin_y)` in the pixel space a full-page [`render`]
/// at the same zoom would produce (so y grows downwards).
///
/// The viewer needs this because a full-page raster is not an option at
/// interactive zoom: an A0 sheet at 200x fit would be a quarter of a million
/// pixels wide. Rendering only the visible window keeps the cost tied to the
/// window size, exactly as the previous resvg-based viewport did.
pub fn render_window(
    scene: &PlotScene,
    pixels_per_mm: f32,
    origin_x: f32,
    origin_y: f32,
    width: u32,
    height: u32,
) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(width.max(1), height.max(1))?;
    pixmap.fill(Color::from_rgba8(DESK_RGB.0, DESK_RGB.1, DESK_RGB.2, 255));

    let sheet_w = scene.paper.width_mm as f32 * pixels_per_mm;
    let sheet_h = scene.paper.height_mm as f32 * pixels_per_mm;
    if let Some(sheet) = tiny_skia::Rect::from_xywh(-origin_x, -origin_y, sheet_w, sheet_h) {
        let mut paint = Paint::default();
        paint.set_color(Color::WHITE);
        pixmap.fill_rect(sheet, &paint, Transform::identity(), None);
    }

    draw_into(&mut pixmap, scene, pixels_per_mm, origin_x, origin_y);
    Some(pixmap)
}

fn draw_into(
    pixmap: &mut Pixmap,
    scene: &PlotScene,
    pixels_per_mm: f32,
    origin_x: f32,
    origin_y: f32,
) {
    // Scene Y runs up from the bottom-left (PDF convention); pixmaps run
    // down from the top-left, so flip here rather than in plot::build.
    let transform = Transform::from_row(
        pixels_per_mm,
        0.0,
        0.0,
        -pixels_per_mm,
        -origin_x,
        scene.paper.height_mm as f32 * pixels_per_mm - origin_y,
    );

    // Mirror the PDF backend's clip to the printable area, or the preview
    // shows ink in the margin that the exported page does not have.
    let clip = clip_mask(pixmap.width(), pixmap.height(), scene, transform);

    for item in &scene.items {
        match item {
            PlotItem::Path { geom, style } => {
                let Some(path) = build_path(geom) else { continue };
                let mut stroke = Stroke {
                    width: (style.width_mm * pixels_per_mm).max(1.0) / pixels_per_mm,
                    line_cap: LineCap::Butt,
                    line_join: LineJoin::Miter,
                    miter_limit: 4.0,
                    ..Stroke::default()
                };
                if let Some(dash) = &style.dash_mm {
                    let usable: Vec<f32> = dash.iter().map(|v| v.max(0.01)).collect();
                    if usable.len() >= 2 {
                        stroke.dash = StrokeDash::new(usable, 0.0);
                    }
                }
                pixmap.stroke_path(
                    &path,
                    &paint_for(style.color),
                    &stroke,
                    transform,
                    clip.as_ref(),
                );
            }
            PlotItem::Fill { geom, color } => {
                let Some(path) = build_path(geom) else { continue };
                pixmap.fill_path(
                    &path,
                    &paint_for(*color),
                    tiny_skia::FillRule::Winding,
                    transform,
                    clip.as_ref(),
                );
            }
            PlotItem::Glyphs(run) => {
                let Some(path) = build_path(&run.geom) else { continue };
                if run.fill {
                    // TrueType contours are closed areas. Non-zero winding
                    // is what leaves the counters inside 'o' and 'B' open.
                    pixmap.fill_path(
                        &path,
                        &paint_for(run.style.color),
                        tiny_skia::FillRule::Winding,
                        transform,
                        clip.as_ref(),
                    );
                } else {
                    // Round caps and joins, unlike the mitre joins geometry
                    // uses: SHX is a pen-plotter font whose strokes meet at
                    // sharp angles, and mitres spike at them.
                    let stroke = Stroke {
                        width: (run.style.width_mm * pixels_per_mm).max(1.0) / pixels_per_mm,
                        line_cap: LineCap::Round,
                        line_join: LineJoin::Round,
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(
                        &path,
                        &paint_for(run.style.color),
                        &stroke,
                        transform,
                        clip.as_ref(),
                    );
                }
            }
        }
    }
}

/// A mask covering the scene's printable area, in the pixmap's pixel space.
fn clip_mask(
    width: u32,
    height: u32,
    scene: &PlotScene,
    transform: Transform,
) -> Option<tiny_skia::Mask> {
    let area = scene.clip?;
    if !area.valid() || area.width() <= 0.0 || area.height() <= 0.0 {
        return None;
    }
    let rect = tiny_skia::Rect::from_xywh(
        area.min_x as f32,
        area.min_y as f32,
        area.width() as f32,
        area.height() as f32,
    )?;
    let path = PathBuilder::from_rect(rect);
    let mut mask = tiny_skia::Mask::new(width, height)?;
    mask.fill_path(&path, tiny_skia::FillRule::Winding, true, transform);
    Some(mask)
}

fn build_path(geom: &crate::geom::PathGeom) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for sp in &geom.subpaths {
        let mut iter = sp.points.iter();
        let Some(first) = iter.next() else { continue };
        pb.move_to(first.x as f32, first.y as f32);
        for p in iter {
            pb.line_to(p.x as f32, p.y as f32);
        }
        if sp.closed {
            pb.close();
        }
    }
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{PathGeom, Point, SubPath};
    use crate::plot::{PaperSize, PlotItem, PlotScene, Rgb, StrokeStyle};

    fn scene_with(width_mm: f32) -> PlotScene {
        let mut s = PlotScene::new(PaperSize { width_mm: 100.0, height_mm: 100.0 });
        s.items.push(PlotItem::Path {
            geom: PathGeom {
                subpaths: vec![SubPath {
                    points: vec![Point::new(10.0, 50.0), Point::new(90.0, 50.0)],
                    closed: false,
                }],
            },
            style: StrokeStyle { color: Rgb::BLACK, width_mm, dash_mm: None },
        });
        s
    }

    fn non_white_pixels(pm: &tiny_skia::Pixmap) -> usize {
        pm.pixels().iter().filter(|p| p.red() < 250 || p.green() < 250).count()
    }

    #[test]
    fn pixmap_size_follows_paper_and_zoom() {
        let pm = render(&scene_with(0.5), 2.0);
        assert_eq!(pm.width(), 200);
        assert_eq!(pm.height(), 200);
    }

    #[test]
    fn draws_the_geometry() {
        let pm = render(&scene_with(0.5), 4.0);
        assert!(non_white_pixels(&pm) > 100, "expected a visible line");
    }

    #[test]
    fn hairlines_stay_visible_at_low_zoom() {
        // A 0.0 mm width would round to nothing without the clamp.
        let pm = render(&scene_with(0.0), 1.0);
        assert!(non_white_pixels(&pm) > 10, "hairline disappeared entirely");
    }

    #[test]
    fn heavier_lineweights_cover_more_pixels() {
        let thin = non_white_pixels(&render(&scene_with(0.15), 8.0));
        let thick = non_white_pixels(&render(&scene_with(1.00), 8.0));
        assert!(thick > thin * 2, "thin {thin}, thick {thick}");
    }

    #[test]
    fn y_is_flipped_so_the_scene_origin_is_at_the_bottom() {
        let mut s = PlotScene::new(PaperSize { width_mm: 100.0, height_mm: 100.0 });
        s.items.push(PlotItem::Path {
            geom: PathGeom {
                subpaths: vec![SubPath {
                    points: vec![Point::new(10.0, 5.0), Point::new(90.0, 5.0)],
                    closed: false,
                }],
            },
            style: StrokeStyle { color: Rgb::BLACK, width_mm: 1.0, dash_mm: None },
        });
        let pm = render(&s, 2.0);
        let h = pm.height();

        let dark_rows: Vec<u32> = (0..h)
            .filter(|y| {
                (0..pm.width())
                    .any(|x| pm.pixel(x, *y).map(|p| p.red() < 250).unwrap_or(false))
            })
            .collect();

        assert!(!dark_rows.is_empty(), "nothing was drawn at all");

        // A scene Y of 5mm on a 100mm sheet is 5% up from the bottom, so every
        // marked row must sit in the bottom tenth of the image. Without the
        // flip they would land in the TOP tenth instead.
        let topmost = *dark_rows.iter().min().unwrap();
        assert!(
            topmost >= h * 9 / 10,
            "expected all marks in the bottom tenth (row >= {}), topmost was {topmost}; \
             rows: {dark_rows:?}",
            h * 9 / 10
        );
    }

    /// First non-white row down the middle column. Scanning one column rather
    /// than the whole width keeps the desk border out of the answer.
    fn marked_row(pm: &tiny_skia::Pixmap) -> Option<u32> {
        let x = pm.width() / 2;
        (0..pm.height()).find(|y| pm.pixel(x, *y).map(|p| p.red() < 250).unwrap_or(false))
    }

    fn assert_near(actual: Option<u32>, expected: u32) {
        let actual = actual.expect("nothing was drawn in the middle column");
        // A one-pixel stroke centred on a pixel boundary marks the row above
        // it too, so allow a pixel of slack.
        assert!(
            actual.abs_diff(expected) <= 1,
            "expected the mark near row {expected}, found it at {actual}"
        );
    }

    /// I8, screen side: the preview must clip where the PDF clips, or the
    /// two disagree about what is on the page.
    #[test]
    fn the_clip_keeps_ink_out_of_the_margin() {
        let mut s = scene_with(1.0);
        let unclipped = non_white_pixels(&render(&s, 4.0));

        let mut area = crate::geom::Bounds::empty();
        area.add(crate::geom::Point::new(0.0, 0.0));
        area.add(crate::geom::Point::new(50.0, 100.0));
        s.clip = Some(area);
        let clipped = non_white_pixels(&render(&s, 4.0));

        assert!(clipped > 0, "the clip erased everything");
        assert!(
            (clipped as f64) < (unclipped as f64) * 0.6,
            "half the line lies outside the clip: {clipped} vs {unclipped}"
        );
    }

    #[test]
    fn a_window_has_exactly_the_requested_pixel_size() {
        let pm = render_window(&scene_with(0.5), 4.0, 120.0, 60.0, 300, 180).unwrap();
        assert_eq!((pm.width(), pm.height()), (300, 180));
    }

    #[test]
    fn an_unshifted_window_matches_the_full_page_render() {
        // The line sits 50 mm up a 100 mm sheet, so at 2 px/mm it lands on
        // pixel row 100 in both.
        let scene = scene_with(0.5);
        assert_near(marked_row(&render(&scene, 2.0)), 100);
        assert_near(
            marked_row(&render_window(&scene, 2.0, 0.0, 0.0, 200, 200).unwrap()),
            100,
        );
    }

    #[test]
    fn shifting_the_origin_pans_the_content() {
        // Moving the window 40 px down the sheet moves the mark 40 px up in
        // the window. Without the origin offset it would stay at row 100.
        let pm = render_window(&scene_with(0.5), 2.0, 0.0, 40.0, 200, 200).unwrap();
        assert_near(marked_row(&pm), 60);
    }

    #[test]
    fn shifting_the_origin_pans_horizontally_too() {
        // The line spans x = 10..90 mm, so a window starting 30 px in sees it
        // begin at column 20 - 30 = -10, i.e. from the very first column.
        let marked_col = |pm: &tiny_skia::Pixmap, y: u32| -> Option<u32> {
            (0..pm.width()).find(|x| pm.pixel(*x, y).map(|p| p.red() < 250).unwrap_or(false))
        };
        let scene = scene_with(0.5);
        let unshifted = render_window(&scene, 2.0, 0.0, 0.0, 200, 200).unwrap();
        let shifted = render_window(&scene, 2.0, 30.0, 0.0, 200, 200).unwrap();
        assert_eq!(marked_col(&unshifted, 100), Some(20));
        assert_eq!(marked_col(&shifted, 100), Some(0));
    }

    fn glyph_scene(fill: bool) -> PlotScene {
        let mut s = PlotScene::new(PaperSize { width_mm: 100.0, height_mm: 100.0 });
        s.items.push(PlotItem::Glyphs(crate::plot::GlyphRun {
            geom: PathGeom {
                subpaths: vec![SubPath {
                    points: vec![
                        Point::new(20.0, 20.0),
                        Point::new(80.0, 20.0),
                        Point::new(80.0, 80.0),
                        Point::new(20.0, 80.0),
                    ],
                    closed: fill,
                }],
            },
            style: StrokeStyle { color: Rgb::BLACK, width_mm: 0.5, dash_mm: None },
            fill,
        }));
        s
    }

    #[test]
    fn stroked_glyph_runs_are_drawn() {
        let pm = render(&glyph_scene(false), 4.0);
        assert!(non_white_pixels(&pm) > 100, "SHX text was not drawn");
    }

    /// A filled run covers its interior; a stroked one only its outline.
    #[test]
    fn filled_glyph_runs_cover_more_than_stroked_ones() {
        let stroked = non_white_pixels(&render(&glyph_scene(false), 4.0));
        let filled = non_white_pixels(&render(&glyph_scene(true), 4.0));
        assert!(filled > stroked * 4, "stroked {stroked}, filled {filled}");
    }

    #[test]
    fn the_area_outside_the_sheet_is_desk_not_paper() {
        // A 100 mm sheet at 2 px/mm covers 200 px; ask for 260 and the last
        // 60 columns must be desk-coloured.
        let pm = render_window(&scene_with(0.5), 2.0, 0.0, 0.0, 260, 200).unwrap();
        let inside = pm.pixel(10, 10).unwrap();
        let outside = pm.pixel(250, 10).unwrap();
        assert_eq!((inside.red(), inside.green(), inside.blue()), (255, 255, 255));
        assert_eq!((outside.red(), outside.green(), outside.blue()), (29, 33, 40));
    }
}
