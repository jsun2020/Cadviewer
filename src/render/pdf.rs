use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write as _;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Filter, Finish, Name, Pdf, Rect, Ref, Str};

use crate::plot::{PlotItem, PlotScene, Rgb, StrokeStyle};
use crate::text::ttf::FaceKey;

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

/// A face embedded in this document, with the object ids reserved for it.
struct FontEntry {
    face: crate::render::embed::EmbeddedFace,
    /// Name in the page `/Font` dictionary and in the `Tf` operator.
    resource: String,
    type0_id: Ref,
    cid_id: Ref,
    descriptor_id: Ref,
    file_id: Ref,
    to_unicode_id: Ref,
}

/// Which characters each TrueType face has to be able to show.
///
/// Collected across every page first, so a face used on twenty sheets is
/// read, subset and embedded once rather than twenty times.
fn faces_used(scenes: &[PlotScene]) -> BTreeMap<FaceKey, BTreeSet<char>> {
    let mut wanted: BTreeMap<FaceKey, BTreeSet<char>> = BTreeMap::new();
    for scene in scenes {
        for item in &scene.items {
            let PlotItem::Glyphs(run) = item else { continue };
            for span in &run.text {
                wanted.entry(span.face.as_ref().clone()).or_default().extend(span.text.chars());
            }
        }
    }
    wanted
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

    // Faces that could not be read, parsed or subset simply do not appear
    // here, and every run that wanted one falls back to its outlines.
    let mut fonts: BTreeMap<FaceKey, FontEntry> = BTreeMap::new();
    for (index, (key, chars)) in faces_used(scenes).into_iter().enumerate() {
        let Some(face) = crate::render::embed::embed(&key, &chars) else { continue };
        fonts.insert(
            key,
            FontEntry {
                face,
                resource: format!("F{index}"),
                type0_id: Ref::new(next),
                cid_id: Ref::new(next + 1),
                descriptor_id: Ref::new(next + 2),
                file_id: Ref::new(next + 3),
                to_unicode_id: Ref::new(next + 4),
            },
        );
        next += 5;
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
        // /Resources is a required inheritable page attribute (PDF 1.7
        // 7.7.3.3). Viewers tolerate its absence; strict preflight and
        // PDF/A do not, and nothing in /Pages supplies it.
        let mut resources = page.resources();
        if !fonts.is_empty() {
            let mut dict = resources.fonts();
            for entry in fonts.values() {
                dict.pair(Name(entry.resource.as_bytes()), entry.type0_id);
            }
            dict.finish();
        }
        resources.finish();
        page.finish();

        let stream = build_content(scene, &fonts);
        let compressed = deflate(stream.as_bytes());
        pdf.stream(content_ids[i], &compressed).filter(Filter::FlateDecode);
    }

    for entry in fonts.values() {
        write_font(&mut pdf, entry);
    }

    pdf.finish()
}

/// Identity ordering: the codes in the content stream are glyph ids in the
/// embedded subset, not characters in any character collection. `/ToUnicode`
/// is what carries the meaning back to a reader.
const IDENTITY: SystemInfo<'static> =
    SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 };

fn write_font(pdf: &mut Pdf, entry: &FontEntry) {
    let face = &entry.face;
    let base = Name(face.base_name.as_bytes());

    pdf.type0_font(entry.type0_id)
        .base_font(base)
        // Two-byte codes taken straight as CIDs, which with the identity
        // CID-to-GID map below means the codes are subset glyph ids.
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(entry.cid_id)
        .to_unicode(entry.to_unicode_id);

    let mut cid = pdf.cid_font(entry.cid_id);
    cid.subtype(CidFontType::Type2)
        .base_font(base)
        .system_info(IDENTITY)
        .font_descriptor(entry.descriptor_id)
        .cid_to_gid_map_predefined(Name(b"Identity"))
        .default_width(0.0);
    {
        let mut widths = cid.widths();
        for (gid, width) in &face.widths {
            widths.consecutive(*gid, [*width]);
        }
        widths.finish();
    }
    cid.finish();

    let mut flags = FontFlags::SYMBOLIC;
    if face.is_serif_guess {
        flags |= FontFlags::SERIF;
    }
    if face.italic_angle != 0.0 {
        flags |= FontFlags::ITALIC;
    }
    pdf.font_descriptor(entry.descriptor_id)
        .name(base)
        .flags(flags)
        .bbox(Rect::new(face.bbox[0], face.bbox[1], face.bbox[2], face.bbox[3]))
        .italic_angle(face.italic_angle)
        .ascent(face.ascent)
        .descent(face.descent)
        .cap_height(face.cap_height)
        // Required, and only ever an approximation for a face we did not
        // author; readers use it for synthetic bolding, which this never
        // asks for.
        .stem_v(80.0)
        .font_file2(entry.file_id);

    let compressed = deflate(&face.program);
    pdf.stream(entry.file_id, &compressed).filter(Filter::FlateDecode);

    let mut cmap = UnicodeCmap::new(Name(b"Custom"), IDENTITY);
    // One entry per glyph. Two characters sharing a glyph — a face that maps
    // them to the same shape — can only be reported as one of them, and the
    // first is as good an answer as the reader can get.
    let mut seen = BTreeSet::new();
    for (ch, gid) in &face.glyphs {
        if seen.insert(*gid) {
            cmap.pair(*gid, *ch);
        }
    }
    let bytes = cmap.finish();
    let compressed = deflate(&bytes);
    pdf.cmap(entry.to_unicode_id, &compressed).filter(Filter::FlateDecode);
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

fn build_content(scene: &PlotScene, fonts: &BTreeMap<FaceKey, FontEntry>) -> String {
    let mut out = String::new();
    let mut current_stroke: Option<Rgb> = None;
    let mut current_fill: Option<Rgb> = None;
    let mut current_width: Option<f32> = None;
    let mut current_dash: Option<Option<Vec<f32>>> = None;

    // Butt caps and mitre joins match AutoCAD's plotted geometry.
    let _ = writeln!(out, "0 J 0 j 4 M");

    // Clip to the printable area. AutoCAD clips at the plot window, and the
    // builder's cull can only drop entities lying *entirely* off the paper:
    // one that straddles the frame edge is emitted whole and would
    // otherwise be inked across this page's margin.
    let clipped = scene.clip.filter(|c| c.valid() && c.width() > 0.0 && c.height() > 0.0);
    if let Some(area) = clipped {
        let _ = writeln!(
            out,
            "q {:.3} {:.3} {:.3} {:.3} re W n",
            mm(area.min_x),
            mm(area.min_y),
            mm(area.width()),
            mm(area.height())
        );
    }

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
            PlotItem::Glyphs(run) => {
                // Shown as text when every face it needs was embedded;
                // otherwise the outlines, which look identical and are what
                // this always did.
                let showable = !run.text.is_empty()
                    && run.text.iter().all(|s| fonts.contains_key(s.face.as_ref()));
                if showable {
                    if current_fill != Some(run.style.color) {
                        let _ = writeln!(
                            out,
                            "{} {} {} rg",
                            fmt_channel(channel(run.style.color.r)),
                            fmt_channel(channel(run.style.color.g)),
                            fmt_channel(channel(run.style.color.b))
                        );
                        current_fill = Some(run.style.color);
                    }
                    for span in &run.text {
                        let entry = &fonts[span.face.as_ref()];
                        show_text(&mut out, span, entry);
                    }
                    continue;
                }
                if run.fill {
                    // TrueType contours are closed areas, so they are filled
                    // rather than stroked — outlining them would draw hollow
                    // letters (R-TXT-4.1/4.2).
                    if current_fill != Some(run.style.color) {
                        let _ = writeln!(
                            out,
                            "{} {} {} rg",
                            fmt_channel(channel(run.style.color.r)),
                            fmt_channel(channel(run.style.color.g)),
                            fmt_channel(channel(run.style.color.b))
                        );
                        current_fill = Some(run.style.color);
                    }
                    emit_path(&mut out, &run.geom);
                    let _ = writeln!(out, "f");
                } else {
                    // The graphics-state trackers are shared with the arms
                    // above on purpose: a run that changed the colour without
                    // recording it would leave the next path drawn in the
                    // wrong colour.
                    apply_stroke(
                        &mut out,
                        &run.style,
                        &mut current_stroke,
                        &mut current_width,
                        &mut current_dash,
                    );
                    // Round caps and joins for stroke fonts, bracketed so the
                    // page keeps its butt-cap default for geometry. Colour,
                    // width and dash are set *before* the `q`, so they are
                    // part of the saved state and survive the `Q` unchanged —
                    // the trackers stay accurate. Anything moved inside this
                    // bracket would have to invalidate them.
                    let _ = writeln!(out, "q 1 J 1 j");
                    emit_path(&mut out, &run.geom);
                    let _ = writeln!(out, "S");
                    let _ = writeln!(out, "Q");
                }
            }
        }
    }

    if clipped.is_some() {
        let _ = writeln!(out, "Q");
    }

    out
}

/// Emit one laid-out line as a show-text operator.
///
/// The font is selected at size 1 and the whole scale lives in `Tm`, which
/// is what lets one matrix carry the text height, the rotation, the oblique
/// shear and AutoCAD's width factor together — the same composition the
/// outline path applies to its points, so the two cannot drift apart.
fn show_text(out: &mut String, span: &crate::text::layout::TextSpan, entry: &FontEntry) {
    let mut hex = String::with_capacity(span.text.len() * 4);
    for ch in span.text.chars() {
        // A character with no glyph in the subset would show as .notdef and
        // advance by the wrong width. The subset is built from these very
        // characters, so this is a corruption guard.
        let Some(gid) = entry.face.glyphs.get(&ch) else { return };
        let _ = write!(hex, "{gid:04X}");
    }
    if hex.is_empty() {
        return;
    }
    // Em space is millimetres here; the page is in points.
    let t = span.transform;
    let k = MM_TO_PT;
    let _ = writeln!(out, "BT");
    let _ = writeln!(out, "/{} 1 Tf", entry.resource);
    let _ = writeln!(
        out,
        "{:.6} {:.6} {:.6} {:.6} {:.4} {:.4} Tm",
        t.a * k,
        t.b * k,
        t.c * k,
        t.d * k,
        t.e * k,
        t.f * k
    );
    let _ = writeln!(out, "<{hex}> Tj");
    let _ = writeln!(out, "ET");
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

    /// I8: without a clip, an entity that straddles the frame edge — the
    /// neighbouring sheet on a tiled model space — is emitted whole and
    /// inked across this page's margin. AutoCAD clips at the plot window.
    #[test]
    fn the_printable_area_is_clipped() {
        let mut s = line_scene(0.35);
        let mut area = crate::geom::Bounds::empty();
        area.add(Point::new(10.0, 10.0));
        area.add(Point::new(287.0, 200.0));
        s.clip = Some(area);
        let text = decompressed_content(&write_pdf(&[s]));
        assert!(text.contains(" re W n"), "no clip path in {text}");
        assert!(text.trim_end().ends_with('Q'), "the clip is never closed: {text}");
        // 10 mm = 28.346 pt, and the area is 277 x 190 mm.
        assert!(text.contains("28.346 28.346"), "clip origin missing from {text}");
    }

    #[test]
    fn a_scene_without_a_clip_emits_no_clip_operators() {
        let text = decompressed_content(&write_pdf(&[line_scene(0.35)]));
        assert!(!text.contains(" W n"), "unexpected clip in {text}");
    }

    #[test]
    fn every_page_declares_a_resource_dictionary() {
        let out = write_pdf(&[line_scene(0.35)]);
        assert!(
            content(&out).contains("/Resources"),
            "PDF 1.7 7.7.3.3 makes /Resources a required page attribute"
        );
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

        let raw = build_content(&s, &BTreeMap::new());
        let compressed = deflate(raw.as_bytes());
        assert!(
            compressed.len() * 4 < raw.len(),
            "expected Flate to shrink 200 identical paths by more than 4x: raw {} bytes, compressed {} bytes",
            raw.len(),
            compressed.len()
        );
    }

    fn glyph_scene(fill: bool) -> PlotScene {
        let mut s = PlotScene::new(PaperSize::a4_landscape());
        s.items.push(PlotItem::Glyphs(crate::plot::GlyphRun {
            geom: PathGeom {
                subpaths: vec![SubPath {
                    points: vec![
                        Point::new(10.0, 10.0),
                        Point::new(60.0, 10.0),
                        Point::new(60.0, 40.0),
                    ],
                    closed: fill,
                }],
            },
            style: StrokeStyle { color: Rgb::new(0, 0, 255), width_mm: 0.35, dash_mm: None },
            fill,
            text: Vec::new(),
        }));
        s
    }

    #[test]
    fn a_stroked_glyph_run_emits_a_stroke_operator() {
        let text = decompressed_content(&write_pdf(&[glyph_scene(false)]));
        assert!(text.contains("0 0 1 RG"), "the text colour is missing: {text}");
        assert!(text.lines().any(|l| l.trim() == "S"), "no stroke operator: {text}");
    }

    /// R-TXT-4.1/4.2: SHX text is stroked, TrueType text is filled. A
    /// filled run stroked instead would draw hollow letters.
    #[test]
    fn a_filled_glyph_run_emits_a_fill_operator() {
        let text = decompressed_content(&write_pdf(&[glyph_scene(true)]));
        assert!(text.lines().any(|l| l.trim() == "f"), "no fill operator: {text}");
        assert!(text.contains("0 0 1 rg"), "the fill colour is missing: {text}");
    }

    /// Stroke fonts get round caps and joins, and only they do: a `q`/`Q`
    /// bracket keeps the page's butt-cap default for geometry.
    #[test]
    fn stroked_text_gets_round_caps_without_disturbing_geometry() {
        let text = decompressed_content(&write_pdf(&[glyph_scene(false)]));
        assert!(text.contains("q 1 J 1 j"), "text was drawn with the page's butt caps: {text}");
        let geometry = decompressed_content(&write_pdf(&[line_scene(0.35)]));
        assert!(!geometry.contains("1 J"), "geometry must keep butt caps: {geometry}");
    }

    /// A scene holding one TrueType run: the outlines it would draw plus
    /// the span describing the same glyphs as text.
    fn ttf_text_scene() -> Option<PlotScene> {
        let path = std::path::Path::new("C:\\Windows\\Fonts").join("arial.ttf");
        if !path.is_file() {
            eprintln!("SKIPPED: arial.ttf not present");
            return None;
        }
        let bytes = std::fs::read(&path).ok()?;
        let mut font = crate::text::ttf::TtfFont::load(bytes, 0).ok()?.from(path.clone());
        let mut pair = crate::text::layout::FontPair {
            primary: crate::text::layout::FontHandle::Ttf(Box::new(std::mem::replace(
                &mut font,
                crate::text::ttf::TtfFont::load(std::fs::read(&path).ok()?, 0).ok()?,
            ))),
            bigfont: crate::text::layout::FontHandle::None,
        };
        let entity = crate::dxf::entities::RawEntity {
            kind: "TEXT".to_owned(),
            codes: vec![
                (1, crate::dxf::lexer::Value::Str(b"Plan".to_vec())),
                (40, crate::dxf::lexer::Value::F64(10.0)),
                (10, crate::dxf::lexer::Value::F64(20.0)),
                (20, crate::dxf::lexer::Value::F64(20.0)),
            ],
        };
        let laid = crate::text::layout::lay_out(
            &entity,
            &crate::dxf::tables::StyleRecord::default(),
            &mut pair,
            crate::encoding::Codepage::Gbk,
        )?;
        assert!(!laid.spans.is_empty(), "the layout produced no text span");
        let mut scene = PlotScene::new(PaperSize::a4_landscape());
        scene.items.push(PlotItem::Glyphs(crate::plot::GlyphRun {
            geom: PathGeom {
                subpaths: laid
                    .filled
                    .iter()
                    .map(|points| SubPath { points: points.clone(), closed: true })
                    .collect(),
            },
            style: StrokeStyle { color: Rgb::BLACK, width_mm: 0.35, dash_mm: None },
            fill: true,
            text: laid.spans,
        }));
        Some(scene)
    }

    /// R-TXT-4.2: a TrueType run must reach the page as text, with the face
    /// embedded — matching the reference PDF, which embeds six TrueType
    /// subsets (PRD 3.9.5).
    #[test]
    fn a_truetype_run_is_written_as_embedded_text() {
        let Some(scene) = ttf_text_scene() else { return };
        let out = write_pdf(&[scene]);
        let raw = content(&out);
        assert!(raw.contains("/Type0"), "no composite font was written");
        assert!(raw.contains("/Identity-H"), "no Identity-H encoding");
        assert!(raw.contains("/FontFile2"), "the face was not embedded");
        let text = decompressed_content(&out);
        assert!(text.contains(" Tj"), "no show-text operator: {text}");
        assert!(text.contains("BT") && text.contains("ET"), "no text object");
    }

    /// The embedded subset must decode back to the original characters. A
    /// PDF whose text extracts as the wrong characters is worse than one
    /// that does not extract at all.
    #[test]
    fn the_embedded_subset_round_trips_through_to_unicode() {
        let Some(scene) = ttf_text_scene() else { return };
        let out = write_pdf(&[scene]);
        assert!(content(&out).contains("/ToUnicode"), "no ToUnicode CMap was written");
    }

    /// Showing the text *and* filling the outlines would double-ink every
    /// glyph — visible as a bolder, subtly misregistered page.
    #[test]
    fn a_shown_run_does_not_also_fill_its_outlines() {
        let Some(scene) = ttf_text_scene() else { return };
        let text = decompressed_content(&write_pdf(&[scene]));
        assert!(text.contains(" Tj"), "the run was not shown as text at all");
        assert!(
            !text.lines().any(|l| l.trim() == "f"),
            "the outlines were filled as well as shown: {text}"
        );
    }

    /// A run whose face could not be embedded must still put ink on the
    /// page. Text that silently disappears because a font failed to subset
    /// is the one outcome worse than text that cannot be selected.
    #[test]
    fn a_run_whose_face_is_unavailable_falls_back_to_its_outlines() {
        let Some(mut scene) = ttf_text_scene() else { return };
        let PlotItem::Glyphs(run) = &mut scene.items[0] else { panic!("expected a glyph run") };
        for span in &mut run.text {
            span.face = std::sync::Arc::new(crate::text::ttf::FaceKey {
                path: std::path::PathBuf::from("C:\\nope\\missing.ttf"),
                index: 0,
            });
        }
        let text = decompressed_content(&write_pdf(&[scene]));
        assert!(!text.contains(" Tj"), "a missing face was shown as text anyway");
        assert!(text.lines().any(|l| l.trim() == "f"), "the fallback drew nothing: {text}");
    }

    /// The colour, width and dash a run sets must be emitted *before* its
    /// `q`, so `Q` restores them to the same values and the shared trackers
    /// stay accurate. Moving `apply_stroke` inside the bracket would make
    /// the next path inherit whatever state preceded the text.
    #[test]
    fn text_graphics_state_is_set_outside_the_cap_bracket() {
        let text = decompressed_content(&write_pdf(&[glyph_scene(false)]));
        let colour = text.find("0 0 1 RG").expect("no stroke colour");
        let bracket = text.find("q 1 J 1 j").expect("no cap bracket");
        assert!(colour < bracket, "the colour is inside the q/Q bracket: {text}");
    }
}
