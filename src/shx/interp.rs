use std::collections::HashMap;

use crate::geom::Point;
use crate::plot::flatten::bulge_arc;

/// Directions are 22.5-degree steps counter-clockwise from east.
const DIRECTION_STEP_DEG: f64 = 22.5;

/// Total bytecode instructions one glyph may execute, across every
/// subshape it calls.
///
/// A depth cap bounds nesting but not work: two glyphs that each call the
/// other twice reach 2^depth executions from a handful of bytes. Real
/// glyphs are tiny — the largest in `gbcbig.shx` is 143 bytes — so a
/// four-figure budget is orders of magnitude above anything honest.
const MAX_INSTRUCTIONS: u32 = 20_000;

/// Deepest subshape nesting. Measured usage is one level (every CJK glyph
/// calls 0x8E and 0x8F); eight leaves room for composed forms.
const MAX_DEPTH: u8 = 8;

#[derive(Clone, Debug, Default)]
pub struct Outline {
    /// Polylines in raw font units, y up.
    pub strokes: Vec<Vec<Point>>,
    /// The pen's final x, which is the advance width. Glyphs end with a
    /// pen-up move to the next character's origin, so this is larger than
    /// the inked bounding box: measured `simplex` 'A' ends at 22 with a
    /// bbox 16 wide.
    pub advance: f64,
}

pub struct Interp<'a> {
    pub glyphs: &'a HashMap<u16, Vec<u8>>,
    /// Unifont encodes a subshape number in two big-endian bytes; bigfont
    /// and shape files use one.
    pub wide_subshape: bool,
}

/// Everything a glyph and its subshapes share. A subshape is a subroutine,
/// not a nested glyph: `gbcbig`'s 0x8E sets the scale that its *caller's*
/// vectors use, and 0x8F restores it and performs the advance move.
struct Pen {
    x: f64,
    y: f64,
    scale: f64,
    down: bool,
    stack: Vec<(f64, f64)>,
    current: Vec<Point>,
    strokes: Vec<Vec<Point>>,
    fuel: u32,
}

impl Pen {
    fn lift(&mut self) {
        if self.current.len() > 1 {
            self.strokes.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }

    fn go(&mut self, dx: f64, dy: f64) {
        let (nx, ny) = (self.x + dx, self.y + dy);
        if self.down {
            if self.current.is_empty() {
                self.current.push(Point::new(self.x, self.y));
            }
            self.current.push(Point::new(nx, ny));
        } else {
            self.lift();
        }
        self.x = nx;
        self.y = ny;
    }
}

impl<'a> Interp<'a> {
    pub fn run(&self, code: u16) -> Outline {
        let mut pen = Pen {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            down: false,
            stack: Vec::new(),
            current: Vec::new(),
            strokes: Vec::new(),
            fuel: MAX_INSTRUCTIONS,
        };
        self.exec(code, &mut pen, 0);
        pen.lift();
        Outline { strokes: pen.strokes, advance: pen.x }
    }

    fn exec(&self, code: u16, pen: &mut Pen, depth: u8) {
        if depth > MAX_DEPTH {
            return;
        }
        let Some(body) = self.glyphs.get(&code) else { return };
        let mut i = 0usize;
        let mut vertical_only = false;
        while i < body.len() {
            if pen.fuel == 0 {
                return;
            }
            pen.fuel -= 1;
            let op = body[i];
            i += 1;

            if vertical_only {
                // The previous byte was 0x0E. Skip this command *and its
                // operands*: dropping only the opcode shifts every
                // remaining byte of the glyph.
                vertical_only = false;
                i += operand_len(op, body, i, self.wide_subshape);
                continue;
            }

            if op >= 0x10 {
                let length = f64::from(op >> 4) * pen.scale;
                let angle = (f64::from(op & 0x0F) * DIRECTION_STEP_DEG).to_radians();
                pen.go(length * angle.cos(), length * angle.sin());
                continue;
            }

            match op {
                0x00 => return,
                0x01 => pen.down = true,
                0x02 => {
                    pen.lift();
                    pen.down = false;
                }
                0x03 => {
                    let Some(by) = body.get(i).copied() else { return };
                    i += 1;
                    if by != 0 {
                        pen.scale /= f64::from(by);
                    }
                }
                0x04 => {
                    let Some(by) = body.get(i).copied() else { return };
                    i += 1;
                    pen.scale *= f64::from(by);
                }
                0x05 => pen.stack.push((pen.x, pen.y)),
                0x06 => {
                    if let Some((x, y)) = pen.stack.pop() {
                        pen.lift();
                        pen.x = x;
                        pen.y = y;
                    }
                }
                0x07 => {
                    let sub = if self.wide_subshape {
                        let (Some(hi), Some(lo)) = (body.get(i), body.get(i + 1)) else { return };
                        i += 2;
                        u16::from(*hi) << 8 | u16::from(*lo)
                    } else {
                        let Some(by) = body.get(i).copied() else { return };
                        i += 1;
                        u16::from(by)
                    };
                    self.exec(sub, pen, depth + 1);
                }
                0x08 => {
                    let (Some(dx), Some(dy)) = (signed(body, i), signed(body, i + 1)) else {
                        return;
                    };
                    i += 2;
                    pen.go(dx * pen.scale, dy * pen.scale);
                }
                0x09 => loop {
                    if pen.fuel == 0 {
                        return;
                    }
                    pen.fuel -= 1;
                    let (Some(dx), Some(dy)) = (signed(body, i), signed(body, i + 1)) else {
                        return;
                    };
                    i += 2;
                    if dx == 0.0 && dy == 0.0 {
                        break;
                    }
                    pen.go(dx * pen.scale, dy * pen.scale);
                },
                0x0A => {
                    let (Some(radius), Some(spec)) = (body.get(i).copied(), body.get(i + 1).copied())
                    else {
                        return;
                    };
                    i += 2;
                    octant_arc(pen, f64::from(radius), spec);
                }
                0x0B => {
                    let Some(args) = body.get(i..i + 5) else { return };
                    i += 5;
                    fractional_arc(pen, args);
                }
                0x0C => {
                    let (Some(dx), Some(dy), Some(b)) =
                        (signed(body, i), signed(body, i + 1), signed(body, i + 2))
                    else {
                        return;
                    };
                    i += 3;
                    bulge(pen, dx, dy, b);
                }
                0x0D => loop {
                    if pen.fuel == 0 {
                        return;
                    }
                    pen.fuel -= 1;
                    let (Some(dx), Some(dy)) = (signed(body, i), signed(body, i + 1)) else {
                        return;
                    };
                    if dx == 0.0 && dy == 0.0 {
                        i += 2;
                        break;
                    }
                    let Some(b) = signed(body, i + 2) else { return };
                    i += 3;
                    bulge(pen, dx, dy, b);
                },
                0x0E => vertical_only = true,
                _ => {}
            }
        }
    }
}

fn signed(body: &[u8], at: usize) -> Option<f64> {
    body.get(at).map(|b| f64::from(*b as i8))
}

/// How many operand bytes a command consumes, for the 0x0E skip path.
fn operand_len(op: u8, body: &[u8], at: usize, wide_subshape: bool) -> usize {
    match op {
        0x03 | 0x04 => 1,
        0x07 => {
            if wide_subshape {
                2
            } else {
                1
            }
        }
        0x08 | 0x0A => 2,
        0x0B => 5,
        0x0C => 3,
        0x09 => {
            let mut n = 0usize;
            while let Some(pair) = body.get(at + n..at + n + 2) {
                n += 2;
                if pair == [0, 0] {
                    break;
                }
            }
            n
        }
        0x0D => {
            let mut n = 0usize;
            while let Some(pair) = body.get(at + n..at + n + 2) {
                if pair == [0, 0] {
                    n += 2;
                    break;
                }
                n += 3;
            }
            n
        }
        _ => 0,
    }
}

/// Command 0x0C: an arc from the current point through a chord `(dx, dy)`
/// bowed by `bulge`, where 127 is a semicircle. Reuses the flattener the
/// polyline path already trusts, so text arcs and polyline arcs cannot
/// disagree about direction — that sign was wrong for years in the old
/// renderer (docs/reviews/README.md).
fn bulge(pen: &mut Pen, dx: f64, dy: f64, bulge_byte: f64) {
    let start = Point::new(pen.x, pen.y);
    let end = Point::new(pen.x + dx * pen.scale, pen.y + dy * pen.scale);
    let ratio = bulge_byte / 127.0;
    if pen.down {
        for p in bulge_arc(start, end, ratio).into_iter().skip(1) {
            if pen.current.is_empty() {
                pen.current.push(start);
            }
            pen.current.push(p);
        }
    } else {
        pen.lift();
    }
    pen.x = end.x;
    pen.y = end.y;
}

/// Command 0x0A: `radius`, then a byte whose sign bit selects clockwise
/// and whose low nibble counts 45-degree octants (0 meaning a full turn).
fn octant_arc(pen: &mut Pen, radius: f64, spec: u8) {
    let clockwise = spec & 0x80 != 0;
    let start_octant = f64::from((spec >> 4) & 0x07);
    let count = spec & 0x07;
    let octants = if count == 0 { 8.0 } else { f64::from(count) };
    let sweep = octants * 45.0_f64.to_radians() * if clockwise { -1.0 } else { 1.0 };
    let start = start_octant * 45.0_f64.to_radians();
    arc_from_pen(pen, radius * pen.scale, start, sweep);
}

/// Command 0x0B: high/low start offset, high/low end offset, radius —
/// the same arc as 0x0A with fractional start and end angles.
fn fractional_arc(pen: &mut Pen, args: &[u8]) {
    let start_offset = f64::from(args[0]) / 256.0;
    let end_offset = f64::from(args[1]) / 256.0;
    let radius = (f64::from(args[2]) * 256.0 + f64::from(args[3])) * pen.scale;
    let spec = args[4];
    let clockwise = spec & 0x80 != 0;
    let start_octant = f64::from((spec >> 4) & 0x07) + start_offset;
    let count = f64::from(spec & 0x07) - end_offset;
    let octants = if count <= 0.0 { 8.0 } else { count };
    let sweep = octants * 45.0_f64.to_radians() * if clockwise { -1.0 } else { 1.0 };
    arc_from_pen(pen, radius, start_octant * 45.0_f64.to_radians(), sweep);
}

/// Sample an arc that begins at the pen and leaves the pen at its end.
fn arc_from_pen(pen: &mut Pen, radius: f64, start: f64, sweep: f64) {
    if !radius.is_finite() || !start.is_finite() || !sweep.is_finite() || radius <= 0.0 {
        return;
    }
    let centre = Point::new(pen.x - radius * start.cos(), pen.y - radius * start.sin());
    let steps = 16usize;
    let begin = Point::new(pen.x, pen.y);
    for step in 1..=steps {
        let angle = start + sweep * (step as f64 / steps as f64);
        let p = Point::new(centre.x + radius * angle.cos(), centre.y + radius * angle.sin());
        if pen.down {
            if pen.current.is_empty() {
                pen.current.push(begin);
            }
            pen.current.push(p);
        }
        pen.x = p.x;
        pen.y = p.y;
    }
    if !pen.down {
        pen.lift();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn interp(glyphs: &HashMap<u16, Vec<u8>>) -> Interp<'_> {
        Interp { glyphs, wide_subshape: false }
    }

    fn bbox(o: &Outline) -> (f64, f64, f64, f64) {
        let mut b = crate::geom::Bounds::empty();
        for s in &o.strokes {
            for p in s {
                b.add(*p);
            }
        }
        (b.min_x, b.min_y, b.max_x, b.max_y)
    }

    /// A single-byte vector: high nibble length, low nibble direction in
    /// 22.5-degree steps counter-clockwise from east.
    #[test]
    fn a_length_ten_east_vector_draws_ten_units_right() {
        let mut g = HashMap::new();
        g.insert(1u16, vec![0x01, 0xA0, 0x00]); // pen down, 10 east, end
        let o = interp(&g).run(1);
        assert_eq!(o.strokes.len(), 1);
        assert_eq!(o.strokes[0], vec![crate::geom::Point::new(0.0, 0.0), crate::geom::Point::new(10.0, 0.0)]);
        assert!((o.advance - 10.0).abs() < 1e-9);
    }

    #[test]
    fn direction_four_points_north() {
        let mut g = HashMap::new();
        g.insert(1u16, vec![0x01, 0x84, 0x00]); // pen down, 8 north
        let o = interp(&g).run(1);
        let end = *o.strokes[0].last().unwrap();
        assert!(end.x.abs() < 1e-9, "x drifted to {}", end.x);
        assert!((end.y - 8.0).abs() < 1e-9, "y was {}", end.y);
    }

    /// Pen up must break the polyline, not draw a connecting segment.
    #[test]
    fn pen_up_starts_a_new_stroke() {
        let mut g = HashMap::new();
        g.insert(1u16, vec![0x01, 0x10, 0x02, 0x30, 0x01, 0x10, 0x00]);
        let o = interp(&g).run(1);
        assert_eq!(o.strokes.len(), 2, "got {:?}", o.strokes);
    }

    /// Commands 3 and 4 scale every subsequent vector.
    #[test]
    fn scale_commands_apply_to_later_vectors() {
        let mut g = HashMap::new();
        g.insert(1u16, vec![0x04, 0x03, 0x01, 0xA0, 0x00]); // x3, pen down, 10 east
        assert!((interp(&g).run(1).advance - 30.0).abs() < 1e-9);
        g.insert(1u16, vec![0x03, 0x02, 0x01, 0xA0, 0x00]); // /2
        assert!((interp(&g).run(1).advance - 5.0).abs() < 1e-9);
    }

    /// Command 8 is a signed two-byte displacement.
    #[test]
    fn displacement_operands_are_signed() {
        let mut g = HashMap::new();
        g.insert(1u16, vec![0x01, 0x08, 0xF8, 0xEB, 0x00]); // (-8, -21)
        let o = interp(&g).run(1);
        let end = *o.strokes[0].last().unwrap();
        assert!((end.x + 8.0).abs() < 1e-9, "x was {}", end.x);
        assert!((end.y + 21.0).abs() < 1e-9, "y was {}", end.y);
    }

    /// Command 9's list ends at a (0,0) pair, and that pair is consumed:
    /// the byte after it is the next command. Getting this wrong makes the
    /// terminator's own 0x00 read as glyph-End, silently dropping every
    /// stroke after any 9-list.
    #[test]
    fn a_displacement_list_consumes_its_terminator_and_execution_continues() {
        let mut g = HashMap::new();
        // pen down; (5,0); (5,0); terminator; 10-east vector; end
        g.insert(1u16, vec![0x01, 0x09, 0x05, 0x00, 0x05, 0x00, 0x00, 0x00, 0xA0, 0x00]);
        let o = interp(&g).run(1);
        assert_eq!(o.strokes[0].len(), 4, "got {:?}", o.strokes);
        assert!((o.advance - 20.0).abs() < 1e-9, "advance {} — the trailing vector was dropped", o.advance);
    }

    /// The same terminator rule inside the 0x0E vertical-only skip: the
    /// skipped 9-list must swallow its terminator too, or the byte the
    /// interpreter resumes on is a 0x00 that ends the glyph.
    #[test]
    fn skipping_a_vertical_only_displacement_list_swallows_its_terminator() {
        let mut g = HashMap::new();
        // vert-only 9-list {(5,0), terminator}; pen down; 10-east; end
        g.insert(1u16, vec![0x0E, 0x09, 0x05, 0x00, 0x00, 0x00, 0x01, 0xA0, 0x00]);
        let o = interp(&g).run(1);
        assert_eq!(o.strokes.len(), 1, "got {:?}", o.strokes);
        assert!((o.advance - 10.0).abs() < 1e-9, "advance {}", o.advance);
    }

    /// Push and pop restore the pen position without drawing.
    #[test]
    fn pop_returns_to_the_pushed_location() {
        let mut g = HashMap::new();
        g.insert(1u16, vec![0x05, 0x01, 0xA0, 0x06, 0x01, 0x84, 0x00]);
        let o = interp(&g).run(1);
        let second = o.strokes.last().unwrap();
        assert!(second[0].x.abs() < 1e-9, "pop did not restore x: {second:?}");
    }

    /// Command 0x0E marks the NEXT command as vertical-only. In horizontal
    /// text it must be skipped together with all of its operands — a
    /// skipped `08` swallows two bytes, and getting that wrong shifts
    /// every remaining byte of the glyph.
    #[test]
    fn a_vertical_only_command_is_skipped_with_its_operands() {
        let mut g = HashMap::new();
        // vert-only move(100,100), then pen down, 10 east.
        g.insert(1u16, vec![0x0E, 0x08, 0x64, 0x64, 0x01, 0xA0, 0x00]);
        let o = interp(&g).run(1);
        let end = *o.strokes[0].last().unwrap();
        assert!((end.x - 10.0).abs() < 1e-9, "x was {} — operands were not skipped", end.x);
        assert!(end.y.abs() < 1e-9, "y was {}", end.y);
    }

    /// A subshape is a subroutine: it shares the caller's pen position,
    /// pen state, scale and stack. Measured on gbcbig, where subroutine
    /// 0x8E sets the scale its CALLER's vectors then use.
    #[test]
    fn a_subshape_shares_the_callers_pen_state() {
        let mut g = HashMap::new();
        g.insert(0x8E, vec![0x04, 0x03, 0x00]); // x3 then end
        g.insert(1u16, vec![0x07, 0x8E, 0x01, 0xA0, 0x00]);
        let o = interp(&g).run(1);
        assert!(
            (o.advance - 30.0).abs() < 1e-9,
            "the subshape's scale did not reach the caller: advance {}",
            o.advance
        );
    }

    #[test]
    fn a_unifont_subshape_number_is_two_bytes() {
        let mut g = HashMap::new();
        g.insert(0x8E05, vec![0x01, 0xA0, 0x00]);
        g.insert(1u16, vec![0x07, 0x8E, 0x05, 0x00]);
        let o = Interp { glyphs: &g, wide_subshape: true }.run(1);
        assert_eq!(o.strokes.len(), 1, "the wide subshape was not called");
    }

    /// Bulge arcs must produce curvature, not a straight chord.
    #[test]
    fn a_bulge_arc_bows_away_from_its_chord() {
        let mut g = HashMap::new();
        // pen down, bulge arc dx=40 dy=0 bulge=127 (半圆), end
        g.insert(1u16, vec![0x01, 0x0C, 0x28, 0x00, 0x7F, 0x00]);
        let o = interp(&g).run(1);
        let (_, _, _, max_y) = bbox(&o);
        assert!(max_y > 5.0, "the arc is flat: bbox top {max_y}");
        let end = *o.strokes[0].last().unwrap();
        assert!((end.x - 40.0).abs() < 0.5, "the arc did not end on its chord: {end:?}");
    }

    /// R-TXT-1.4: a glyph that calls itself must terminate.
    #[test]
    fn a_self_referential_glyph_terminates() {
        let mut g = HashMap::new();
        g.insert(1u16, vec![0x01, 0xA0, 0x07, 0x01, 0x00]);
        let o = interp(&g).run(1);
        assert!(o.strokes.len() < 10_000, "runaway recursion produced {} strokes", o.strokes.len());
    }

    /// R-TXT-1.4: two glyphs calling each other must terminate too — a
    /// depth cap alone does not bound the work when the branching factor
    /// is above one (the same defect as I6 in docs/reviews/).
    #[test]
    fn mutually_recursive_glyphs_terminate() {
        let mut g = HashMap::new();
        g.insert(1u16, vec![0xA0, 0x07, 0x02, 0x07, 0x02, 0x00]);
        g.insert(2u16, vec![0xA0, 0x07, 0x01, 0x07, 0x01, 0x00]);
        let o = interp(&g).run(1);
        assert!(o.strokes.len() < 10_000, "runaway recursion produced {} strokes", o.strokes.len());
    }

    /// A jump past the end of the bytecode must stop, not read neighbours.
    #[test]
    fn a_truncated_operand_stops_cleanly() {
        let mut g = HashMap::new();
        g.insert(1u16, vec![0x01, 0x08, 0x05]); // command 8 missing its y
        let o = interp(&g).run(1);
        assert!(o.advance.is_finite());
    }

    #[test]
    fn an_unknown_glyph_is_empty_not_a_panic() {
        let g = HashMap::new();
        let o = interp(&g).run(0x4E2D);
        assert!(o.strokes.is_empty());
        assert_eq!(o.advance, 0.0);
    }
}
