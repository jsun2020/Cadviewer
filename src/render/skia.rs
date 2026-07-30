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

/// Rasterise a scene at the given zoom. `pixels_per_mm` is the only view
/// parameter: panning is done by the caller cropping or offsetting the
/// resulting pixmap.
pub fn render(scene: &PlotScene, pixels_per_mm: f32) -> Pixmap {
    let w = (scene.paper.width_mm as f32 * pixels_per_mm).ceil().max(1.0) as u32;
    let h = (scene.paper.height_mm as f32 * pixels_per_mm).ceil().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    pixmap.fill(Color::WHITE);

    // Scene Y runs up from the bottom-left (PDF convention); pixmaps run
    // down from the top-left, so flip here rather than in plot::build.
    let transform = Transform::from_row(
        pixels_per_mm,
        0.0,
        0.0,
        -pixels_per_mm,
        0.0,
        scene.paper.height_mm as f32 * pixels_per_mm,
    );

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
                pixmap.stroke_path(&path, &paint_for(style.color), &stroke, transform, None);
            }
            PlotItem::Fill { geom, color } => {
                let Some(path) = build_path(geom) else { continue };
                pixmap.fill_path(
                    &path,
                    &paint_for(*color),
                    tiny_skia::FillRule::Winding,
                    transform,
                    None,
                );
            }
        }
    }
    pixmap
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
}
