use std::fmt::Write as _;
use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use pdf_writer::{Filter, Finish, Pdf, Rect, Ref};

use crate::plot::{PlotItem, PlotScene, Rgb, StrokeStyle};

const MM_TO_PT: f64 = 72.0 / 25.4;

fn mm(v: f64) -> f32 {
    (v * MM_TO_PT) as f32
}

fn channel(v: u8) -> f32 {
    v as f32 / 255.0
}

/// Format a 0..=1 colour channel with up to 4 decimal places, trimming
/// trailing zeros so pure primaries read as `1 0 0` rather than
/// `1.0000 0.0000 0.0000`.
fn fmt_channel(v: f32) -> String {
    let s = format!("{v:.4}");
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() { "0".to_owned() } else { trimmed.to_owned() }
}

/// Render one scene per page into a single PDF document.
pub fn write_pdf(scenes: &[PlotScene]) -> Vec<u8> {
    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);

    let mut next = 3i32;
    let mut page_ids = Vec::new();
    let mut content_ids = Vec::new();
    for _ in scenes {
        page_ids.push(Ref::new(next));
        content_ids.push(Ref::new(next + 1));
        next += 2;
    }

    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id)
        .kids(page_ids.iter().copied())
        .count(scenes.len() as i32);

    for (i, scene) in scenes.iter().enumerate() {
        let mut page = pdf.page(page_ids[i]);
        page.parent(page_tree_id);
        page.media_box(Rect::new(
            0.0,
            0.0,
            mm(scene.paper.width_mm),
            mm(scene.paper.height_mm),
        ));
        page.contents(content_ids[i]);
        page.finish();

        let stream = build_content(scene);
        let compressed = deflate(stream.as_bytes());
        pdf.stream(content_ids[i], &compressed).filter(Filter::FlateDecode);
    }

    pdf.finish()
}

/// Zlib-compress (RFC 1950) a content stream for `/Filter /FlateDecode`.
///
/// Real drawings expand into a content stream with one `m`/`l`/`S` run per
/// entity (plus per-INSERT block expansion), which is highly repetitive
/// text; Flate routinely shrinks it 5-10x. Without this, exported PDFs are
/// gigabytes for drawings AutoCAD itself exports at a few megabytes.
fn deflate(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
    encoder
        .write_all(bytes)
        .expect("writing to an in-memory Vec cannot fail");
    encoder
        .finish()
        .expect("finishing an in-memory Vec encoder cannot fail")
}

fn build_content(scene: &PlotScene) -> String {
    let mut out = String::new();
    let mut current_stroke: Option<Rgb> = None;
    let mut current_fill: Option<Rgb> = None;
    let mut current_width: Option<f32> = None;
    let mut current_dash: Option<Option<Vec<f32>>> = None;

    // Butt caps and mitre joins match AutoCAD's plotted geometry.
    let _ = writeln!(out, "0 J 0 j 4 M");

    for item in &scene.items {
        match item {
            PlotItem::Path { geom, style } => {
                apply_stroke(&mut out, style, &mut current_stroke, &mut current_width, &mut current_dash);
                emit_path(&mut out, geom);
                let _ = writeln!(out, "S");
            }
            PlotItem::Fill { geom, color } => {
                if current_fill != Some(*color) {
                    let _ = writeln!(
                        out,
                        "{} {} {} rg",
                        fmt_channel(channel(color.r)),
                        fmt_channel(channel(color.g)),
                        fmt_channel(channel(color.b))
                    );
                    current_fill = Some(*color);
                }
                emit_path(&mut out, geom);
                let _ = writeln!(out, "f");
            }
        }
    }

    out
}

fn apply_stroke(
    out: &mut String,
    style: &StrokeStyle,
    current_stroke: &mut Option<Rgb>,
    current_width: &mut Option<f32>,
    current_dash: &mut Option<Option<Vec<f32>>>,
) {
    if *current_stroke != Some(style.color) {
        let _ = writeln!(
            out,
            "{} {} {} RG",
            fmt_channel(channel(style.color.r)),
            fmt_channel(channel(style.color.g)),
            fmt_channel(channel(style.color.b))
        );
        *current_stroke = Some(style.color);
    }

    let width_pt = (style.width_mm as f64 * MM_TO_PT) as f32;
    if *current_width != Some(width_pt) {
        if width_pt <= 0.0 {
            // PDF width 0 is "thinnest renderable line", which is exactly
            // what a CAD hairline means.
            let _ = writeln!(out, "0 w");
        } else {
            let _ = writeln!(out, "{width_pt:.3} w");
        }
        *current_width = Some(width_pt);
    }

    if current_dash.as_ref() != Some(&style.dash_mm) {
        match &style.dash_mm {
            Some(pattern) if !pattern.is_empty() => {
                let parts: Vec<String> = pattern
                    .iter()
                    .map(|v| format!("{:.3}", (*v as f64) * MM_TO_PT))
                    .collect();
                let _ = writeln!(out, "[{}] 0 d", parts.join(" "));
            }
            _ => {
                let _ = writeln!(out, "[] 0 d");
            }
        }
        *current_dash = Some(style.dash_mm.clone());
    }
}

fn emit_path(out: &mut String, geom: &crate::geom::PathGeom) {
    for sp in &geom.subpaths {
        let mut iter = sp.points.iter();
        let Some(first) = iter.next() else { continue };
        let _ = writeln!(out, "{:.3} {:.3} m", mm(first.x), mm(first.y));
        for p in iter {
            let _ = writeln!(out, "{:.3} {:.3} l", mm(p.x), mm(p.y));
        }
        if sp.closed {
            let _ = writeln!(out, "h");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use flate2::read::ZlibDecoder;

    use super::*;
    use crate::geom::{PathGeom, Point, SubPath};
    use crate::plot::{PaperSize, PlotItem, PlotScene, Rgb, StrokeStyle};

    fn line_scene(width_mm: f32) -> PlotScene {
        let mut s = PlotScene::new(PaperSize::a4_landscape());
        s.items.push(PlotItem::Path {
            geom: PathGeom {
                subpaths: vec![SubPath {
                    points: vec![Point::new(10.0, 10.0), Point::new(100.0, 50.0)],
                    closed: false,
                }],
            },
            style: StrokeStyle { color: Rgb::new(255, 0, 0), width_mm, dash_mm: None },
        });
        s
    }

    fn content(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    /// Locate the (single, in these single-scene tests) content stream
    /// inside a full PDF document and inflate it back to the operator text
    /// the state machine produced. Operators are the whole point of this
    /// module -- a missed graphics-state reset silently applies one
    /// entity's width/colour/dash to the next -- so tests must keep
    /// asserting on them directly rather than trusting the compressed
    /// bytes "look about right".
    fn decompressed_content(out: &[u8]) -> String {
        let start_marker = b"stream\n";
        let end_marker = b"\nendstream";
        let start = find_bytes(out, start_marker).expect("no stream marker") + start_marker.len();
        let end = find_bytes(&out[start..], end_marker).expect("no endstream marker") + start;
        let mut decoder = ZlibDecoder::new(&out[start..end]);
        let mut text = String::new();
        decoder
            .read_to_string(&mut text)
            .expect("failed to inflate the FlateDecode content stream");
        text
    }

    #[test]
    fn writes_a_valid_pdf_header_and_trailer() {
        let out = write_pdf(&[line_scene(0.35)]);
        assert!(out.starts_with(b"%PDF-"), "missing PDF header");
        assert!(content(&out).contains("%%EOF"), "missing EOF marker");
    }

    #[test]
    fn one_page_per_scene() {
        let out = write_pdf(&[line_scene(0.35), line_scene(0.15), line_scene(0.25)]);
        let text = content(&out);
        assert_eq!(text.matches("/Type /Page\n").count().max(text.matches("/Type/Page").count()), 3);
    }

    #[test]
    fn page_media_box_is_the_paper_size_in_points() {
        let out = write_pdf(&[line_scene(0.35)]);
        let text = content(&out);
        // A4 landscape: 297 x 210 mm = 841.89 x 595.28 pt.
        assert!(text.contains("841.8"), "MediaBox width missing from {text:.400}");
        assert!(text.contains("595.2"), "MediaBox height missing");
    }

    #[test]
    fn stroke_width_is_emitted_in_points() {
        // 0.35 mm = 0.9921 pt.
        let out = write_pdf(&[line_scene(0.35)]);
        assert!(
            decompressed_content(&out).contains("0.992"),
            "expected 0.992 w in the content stream"
        );
    }

    #[test]
    fn hairline_is_emitted_as_zero_width() {
        let out = write_pdf(&[line_scene(0.0)]);
        assert!(decompressed_content(&out).contains("0 w"), "hairline must be '0 w'");
    }

    #[test]
    fn colour_is_emitted_as_a_normalised_rgb_stroke() {
        let out = write_pdf(&[line_scene(0.35)]);
        assert!(
            decompressed_content(&out).contains("1 0 0 RG"),
            "expected red stroke colour"
        );
    }

    #[test]
    fn dash_patterns_reach_the_content_stream() {
        let mut s = line_scene(0.35);
        if let PlotItem::Path { style, .. } = &mut s.items[0] {
            style.dash_mm = Some(vec![2.0, 1.0]);
        }
        let out = write_pdf(&[s]);
        assert!(decompressed_content(&out).contains(" d\n"), "expected a dash operator");
    }

    #[test]
    fn content_stream_declares_flate_decode() {
        let out = write_pdf(&[line_scene(0.35)]);
        assert!(
            content(&out).contains("/FlateDecode"),
            "expected the content stream to declare /Filter /FlateDecode"
        );
    }

    #[test]
    fn flate_compression_shrinks_repetitive_content() {
        // Real drawings are enormously repetitive (one m/l/S run per
        // entity, plus per-INSERT block expansion), so this is exactly the
        // shape of content Flate is expected to shrink drastically -- and
        // it is what made the uncompressed backend produce a 1 GB PDF from
        // a 6.5 MB drawing. Assert a substantial ratio, not an exact byte
        // count, so the test does not become brittle.
        let mut s = PlotScene::new(PaperSize::a4_landscape());
        for _ in 0..200 {
            s.items.push(PlotItem::Path {
                geom: PathGeom {
                    subpaths: vec![SubPath {
                        points: vec![Point::new(10.0, 10.0), Point::new(100.0, 50.0)],
                        closed: false,
                    }],
                },
                style: StrokeStyle { color: Rgb::new(255, 0, 0), width_mm: 0.35, dash_mm: None },
            });
        }

        let raw = build_content(&s);
        let compressed = deflate(raw.as_bytes());
        assert!(
            compressed.len() * 4 < raw.len(),
            "expected Flate to shrink 200 identical paths by more than 4x: raw {} bytes, compressed {} bytes",
            raw.len(),
            compressed.len()
        );
    }
}
