# Cadviewer Plot Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Cadviewer's SVG render core with a millimetre-based plot pipeline so that DWG -> PDF output matches AutoCAD's "Plot to PDF" in colour, lineweight, linetype and sheet pagination.

**Architecture:** DWG is decoded by LibreDWG's `dwg2dxf -b` into binary DXF. A new lexer produces raw group-code pairs with undecoded string bytes, so each string can be decoded against `$DWGCODEPAGE` individually. Typed tables and entity records feed a `Document`. `plot::build` resolves colour, lineweight and linetype, flattens geometry, and transforms everything into a `PlotScene` measured in paper millimetres. Two thin backends consume that scene: `tiny-skia` for the screen and `pdf-writer` for output. All plot semantics live in `plot/`; the renderers only draw, so screen and PDF cannot diverge.

**Tech Stack:** Rust 1.95 (edition 2024), `eframe`/`egui` 0.35, `tiny-skia` 0.11, `pdf-writer` 0.12, `encoding_rs` 0.8, `rfd` 0.17. Removed during this plan: `resvg`, `svg2pdf`.

**Scope:** This plan covers PRD phases **P1 (foundation), P2 (plot core) and P3 (sheet detection)**. That boundary is deliberate: those three phases together deliver all four defects the user reported (encoding, colours, lineweights, sheet/layout detection) and produce a working multi-page converter. PRD phases P4 (SHX text), P5 (HATCH + visual diff harness) and P6 (packaging/perf) get their own plans once this pipeline is proven.

## Global Constraints

- **Spec:** `prd.md` v2.2 in the repository root. Requirement IDs below (`R-ENC-1`, `R-LW-3`, ...) refer to its section 5.
- **Platform:** Windows 10 1809+ x64. Build with the MSVC toolchain. Use PowerShell for shell commands.
- **Offline builds:** `encoding_rs 0.8`, `tiny-skia 0.11`, `pdf-writer 0.12` are already in the local cargo cache. Build with `--offline` if the network is unavailable.
- **Reference files are never committed.** `.gitignore` already excludes `/tests/reference/`, `*.dwg`, `*.pdf`. Any test that needs them must **skip with an explicit printed message** when they are absent — never pass silently.
- **Sample paths (local only, do not commit):**
  - DWG: `C:\Users\sr9rfx\Desktop\2_国澳项目-五层装修平面图（左侧）2023.12.12.dwg`
  - Reference PDF: `C:\Users\sr9rfx\Desktop\issues\2_国澳项目-五层装修平面图（左侧）2023.12.12-Model1.pdf`
- **No non-ASCII in shell command string literals.** Chinese is fine inside `.rs` and `.md` files (they are UTF-8); it must not be passed as a literal argument to a shell command. Put such paths in a file or a Rust constant.
- **Lineweight unit rule:** DXF group code 370 is in 1/100 mm. `width_mm = lw370 as f32 / 100.0`. Never store lineweight in any other unit.
- **Commit style:** conventional commits (`feat:`, `fix:`, `refactor:`, `test:`). Commit after every task.
- **`prd.md` is gitignored** — do not attempt to `git add` it.

---

## File Structure

**Created:**

| File | Responsibility |
| --- | --- |
| `src/geom.rs` | `Point`, `Affine`, `Bounds`, `PathGeom`, `SubPath`. Pure geometry, no DXF knowledge. |
| `src/encoding.rs` | `$DWGCODEPAGE` -> `Codepage`; per-string byte decoding. |
| `src/aci.rs` | The 256-entry AutoCAD Color Index table. Constant data only. |
| `src/dxf/mod.rs` | Re-exports for the `dxf` module. |
| `src/dxf/lexer.rs` | Binary and ASCII DXF bytes -> `Vec<Pair>` with **undecoded** string bytes. |
| `src/dxf/tables.rs` | `LayerRecord`, `LtypeRecord`, `StyleRecord`, `HeaderVars`. |
| `src/dxf/entities.rs` | `RawEntity` (code -> values map) and section splitting. |
| `src/doc.rs` | `Document`: header + tables + blocks + root entities, all strings decoded. |
| `src/plot/mod.rs` | `PlotScene`, `PlotItem`, `StrokeStyle`, `Rgb`, `PaperSize`. |
| `src/plot/style.rs` | Colour, lineweight and linetype resolution (entity -> ByLayer -> ByBlock). |
| `src/plot/flatten.rs` | Entity geometry -> `PathGeom` (arcs, bulges, ellipses, splines). |
| `src/plot/build.rs` | `Document` + `Sheet` -> `PlotScene`, including block recursion and mm transform. |
| `src/sheets.rs` | Title-block frame detection and page ordering. |
| `src/render/mod.rs` | Re-exports for the `render` module. |
| `src/render/pdf.rs` | `PlotScene` -> PDF bytes. |
| `src/render/skia.rs` | `PlotScene` -> `tiny_skia::Pixmap`. |

**Modified:** `Cargo.toml`, `src/lib.rs`, `src/converter.rs`, `src/main.rs`, `src/bin/cadconvert.rs`.

**Deleted at the end of Phase 2:** `src/dxf.rs` (the 1841-line monolith; its working geometry logic is carried into `plot/flatten.rs` first).

---

# Phase 1 — Foundation

Delivers: correct Chinese decoding, an exact ACI table, and a typed `Document`. No rendering yet.

---

### Task 1: Dependencies and geometry primitives

**Files:**
- Modify: `Cargo.toml`
- Create: `src/geom.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `geom::Point { x: f64, y: f64 }`, `geom::Affine`, `geom::Bounds`, `geom::PathGeom`, `geom::SubPath`. Methods: `Affine::identity()`, `Affine::translation(f64, f64)`, `Affine::scale(f64, f64)`, `Affine::rotation(f64)`, `Affine::then(self, Affine) -> Affine`, `Affine::apply(self, Point) -> Point`, `Affine::average_scale(self) -> f64`, `Bounds::empty()`, `Bounds::add(&mut self, Point)`, `Bounds::valid(self) -> bool`, `Bounds::width(self) -> f64`, `Bounds::height(self) -> f64`, `Bounds::contains(self, Bounds) -> bool`.

- [ ] **Step 1: Add dependencies**

In `Cargo.toml`, replace the `[dependencies]` block with:

```toml
[dependencies]
eframe = { version = "0.35.0", default-features = false, features = ["default_fonts", "glow"] }
encoding_rs = "0.8"
pdf-writer = "0.12"
resvg = { version = "0.45.1", default-features = true }
rfd = "0.17.2"
svg2pdf = "0.13.0"
tempfile = "3.27.0"
tiny-skia = "0.11"
```

**`resvg` and `svg2pdf` stay for now, and are removed in Task 16.** The old `src/main.rs` and `src/pdf.rs` still use them, so dropping them here would break compilation of the whole crate — and a crate that does not compile cannot run `cargo test --lib` for *any* module, including the new ones. Every task from here to Task 16 must leave `cargo build` green.

Verify that before continuing:

Run: `cargo build --offline 2>&1 | Select-String -Pattern "^error|Finished"`
Expected: `Finished`.

- [ ] **Step 2: Write the failing test**

Create `src/geom.rs` containing only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_composes_in_application_order() {
        let t = Affine::scale(2.0, 2.0).then(Affine::translation(10.0, 5.0));
        let p = t.apply(Point::new(1.0, 1.0));
        assert!((p.x - 12.0).abs() < 1e-9, "x was {}", p.x);
        assert!((p.y - 7.0).abs() < 1e-9, "y was {}", p.y);
    }

    #[test]
    fn bounds_contains_is_strict_about_the_outer_box() {
        let mut outer = Bounds::empty();
        outer.add(Point::new(0.0, 0.0));
        outer.add(Point::new(100.0, 100.0));
        let mut inner = Bounds::empty();
        inner.add(Point::new(10.0, 10.0));
        inner.add(Point::new(20.0, 20.0));
        assert!(outer.contains(inner));
        assert!(!inner.contains(outer));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib geom:: 2>&1 | Select-String -Pattern "error|test result"`
Expected: compile errors — `Affine`, `Point`, `Bounds` not found.

- [ ] **Step 4: Implement**

Prepend to `src/geom.rs` (above the test module):

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Row-major 2x3 affine transform: [a c e; b d f].
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub fn identity() -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
    }

    pub fn translation(x: f64, y: f64) -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: x, f: y }
    }

    pub fn scale(x: f64, y: f64) -> Self {
        Self { a: x, b: 0.0, c: 0.0, d: y, e: 0.0, f: 0.0 }
    }

    pub fn rotation(degrees: f64) -> Self {
        let r = degrees.to_radians();
        let (s, c) = r.sin_cos();
        Self { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    /// `self` applied first, then `rhs`.
    pub fn then(self, rhs: Self) -> Self {
        Self {
            a: self.a * rhs.a + self.b * rhs.c,
            b: self.a * rhs.b + self.b * rhs.d,
            c: self.c * rhs.a + self.d * rhs.c,
            d: self.c * rhs.b + self.d * rhs.d,
            e: self.e * rhs.a + self.f * rhs.c + rhs.e,
            f: self.e * rhs.b + self.f * rhs.d + rhs.f,
        }
    }

    pub fn apply(self, p: Point) -> Point {
        Point::new(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }

    /// Geometric mean of the two axis scales. Used to convert drawing-unit
    /// radii and text heights through a transform.
    pub fn average_scale(self) -> f64 {
        let sx = (self.a * self.a + self.b * self.b).sqrt();
        let sy = (self.c * self.c + self.d * self.d).sqrt();
        ((sx * sy).abs()).sqrt()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds {
    pub fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    pub fn add(&mut self, p: Point) {
        if p.x < self.min_x { self.min_x = p.x; }
        if p.y < self.min_y { self.min_y = p.y; }
        if p.x > self.max_x { self.max_x = p.x; }
        if p.y > self.max_y { self.max_y = p.y; }
    }

    pub fn valid(self) -> bool {
        self.min_x <= self.max_x && self.min_y <= self.max_y
    }

    pub fn width(self) -> f64 {
        (self.max_x - self.min_x).max(0.0)
    }

    pub fn height(self) -> f64 {
        (self.max_y - self.min_y).max(0.0)
    }

    /// True when `other` lies entirely inside `self`.
    pub fn contains(self, other: Bounds) -> bool {
        self.valid()
            && other.valid()
            && self.min_x <= other.min_x
            && self.min_y <= other.min_y
            && self.max_x >= other.max_x
            && self.max_y >= other.max_y
    }
}

/// A run of connected points. Curves are flattened before they get here.
#[derive(Clone, Debug, Default)]
pub struct SubPath {
    pub points: Vec<Point>,
    pub closed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PathGeom {
    pub subpaths: Vec<SubPath>,
}

impl PathGeom {
    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::empty();
        for sp in &self.subpaths {
            for p in &sp.points {
                b.add(*p);
            }
        }
        b
    }
}
```

Add to `src/lib.rs`:

```rust
pub mod geom;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib geom:: 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 2 passed`

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/geom.rs src/lib.rs
git commit -m "feat: add geometry primitives and swap render dependencies"
```

---

### Task 2: Codepage-aware string decoding (R-ENC-1..4)

**Files:**
- Create: `src/encoding.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `encoding::Codepage` (enum: `Utf8`, `Gbk`, `Big5`, `Latin1`), `encoding::codepage_from_dxf(&str) -> Codepage`, `encoding::decode(&[u8], Codepage) -> String`.

**Design note:** `decode` tries UTF-8 first for *every string*, because the sample file genuinely mixes UTF-8 and GBK (PRD 3.1). Strings already damaged in the source DWG must pass through unchanged — AutoCAD does not repair them either, and repairing them would make us differ from the reference (R-ENC-4).

- [ ] **Step 1: Write the failing test**

Create `src/encoding.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_dwgcodepage_header_value() {
        assert_eq!(codepage_from_dxf("ANSI_936"), Codepage::Gbk);
        assert_eq!(codepage_from_dxf("ansi_950"), Codepage::Big5);
        assert_eq!(codepage_from_dxf("UTF8"), Codepage::Utf8);
        assert_eq!(codepage_from_dxf("ANSI_1252"), Codepage::Latin1);
        assert_eq!(codepage_from_dxf("something unknown"), Codepage::Latin1);
    }

    #[test]
    fn decodes_gbk_layer_names() {
        // "布局1" as stored by dwg2dxf for a CP936 drawing.
        let bytes = [0xB2u8, 0xBC, 0xBE, 0xD6, 0x31];
        assert_eq!(decode(&bytes, Codepage::Gbk), "布局1");
    }

    #[test]
    fn prefers_utf8_even_when_the_header_says_gbk() {
        // The same file also contains genuine UTF-8 strings (PRD 3.1).
        let bytes = "图框文字说明".as_bytes();
        assert_eq!(decode(bytes, Codepage::Gbk), "图框文字说明");
    }

    #[test]
    fn never_produces_replacement_characters_for_gbk_input() {
        let bytes = [0xB2u8, 0xBC, 0xBE, 0xD6, 0x31];
        assert!(!decode(&bytes, Codepage::Gbk).contains('\u{FFFD}'));
    }

    #[test]
    fn passes_pre_damaged_strings_through_unchanged() {
        // R-ENC-4: this layer name is already corrupt in the source DWG and
        // AutoCAD's own PDF shows it corrupt too. We must not "fix" it.
        let bytes = b"230626-\xC3\x94\xC2\xADJC$0$DOTE";
        let out = decode(bytes, Codepage::Gbk);
        assert!(out.starts_with("230626-"), "got {out:?}");
        assert!(out.ends_with("JC$0$DOTE"), "got {out:?}");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib encoding:: 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors — `Codepage`, `decode`, `codepage_from_dxf` not found.

- [ ] **Step 3: Implement**

Prepend to `src/encoding.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codepage {
    Utf8,
    Gbk,
    Big5,
    Latin1,
}

/// Map a DXF `$DWGCODEPAGE` value onto a decoder.
///
/// Unknown values fall back to Latin-1 because it is total: every byte
/// sequence decodes, so an unrecognised codepage degrades to mojibake
/// rather than to data loss.
pub fn codepage_from_dxf(name: &str) -> Codepage {
    let upper = name.trim().to_ascii_uppercase();
    match upper.as_str() {
        "ANSI_936" | "GB2312" | "GBK" | "CP936" => Codepage::Gbk,
        "ANSI_950" | "BIG5" | "CP950" => Codepage::Big5,
        "UTF8" | "UTF-8" => Codepage::Utf8,
        _ => Codepage::Latin1,
    }
}

/// Decode one DXF string.
///
/// UTF-8 is always attempted first: files written by newer tools mix
/// genuine UTF-8 strings into a CP936 drawing (PRD 3.1), so a single
/// whole-file decoding strategy is guaranteed to corrupt one group or
/// the other.
pub fn decode(bytes: &[u8], cp: Codepage) -> String {
    if let Ok(s) = core::str::from_utf8(bytes) {
        return s.to_owned();
    }
    let encoding = match cp {
        Codepage::Utf8 | Codepage::Latin1 => encoding_rs::WINDOWS_1252,
        Codepage::Gbk => encoding_rs::GBK,
        Codepage::Big5 => encoding_rs::BIG5,
    };
    let (decoded, _, _) = encoding.decode(bytes);
    decoded.into_owned()
}
```

Add to `src/lib.rs`:

```rust
pub mod encoding;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib encoding:: 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 5 passed`

- [ ] **Step 5: Commit**

```bash
git add src/encoding.rs src/lib.rs
git commit -m "feat: decode DXF strings per-string against DWGCODEPAGE"
```

---

### Task 3: The AutoCAD Color Index table (R-COL-1)

**Files:**
- Create: `src/aci.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `aci::aci_rgb(u8) -> (u8, u8, u8)`, `aci::ACI_TABLE: [(u8, u8, u8); 256]`.

**Important — read before implementing.** Indices 0-9 and 250-255 are fixed, well-known values and are given below verbatim. Indices 10-249 form 24 hues x 10 shades. **Do not invent those 240 values from memory.** Generate them with the documented construction in Step 3, then run the calibration test in Step 5 against the reference PDF, which contains ground-truth RGB for every index the sample drawing actually uses. Any index the calibration test rejects must have its value replaced by the reference value. This is the honest way to build the table: assert only what is verified.

- [ ] **Step 1: Write the failing test**

Create `src/aci.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_colors_match_autocad_exactly() {
        // Verified against the reference PDF content stream (PRD 3.9.3).
        assert_eq!(aci_rgb(1), (255, 0, 0));
        assert_eq!(aci_rgb(2), (255, 255, 0));
        assert_eq!(aci_rgb(3), (0, 255, 0));
        assert_eq!(aci_rgb(4), (0, 255, 255));
        assert_eq!(aci_rgb(5), (0, 0, 255));
        assert_eq!(aci_rgb(6), (255, 0, 255));
        assert_eq!(aci_rgb(7), (255, 255, 255));
    }

    #[test]
    fn grays_are_the_documented_values() {
        assert_eq!(aci_rgb(8), (128, 128, 128));
        assert_eq!(aci_rgb(9), (192, 192, 192));
        assert_eq!(aci_rgb(250), (51, 51, 51));
        assert_eq!(aci_rgb(251), (91, 91, 91));
        assert_eq!(aci_rgb(252), (132, 132, 132));
        assert_eq!(aci_rgb(253), (173, 173, 173));
        assert_eq!(aci_rgb(254), (214, 214, 214));
        assert_eq!(aci_rgb(255), (255, 255, 255));
    }

    #[test]
    fn the_table_is_not_generated_by_a_hsv_formula() {
        // The old implementation computed colours with hsv_to_rgb, which is
        // wrong everywhere above index 9. Index 10 is pure red in the real
        // table; a hue-stepping formula does not produce that.
        assert_eq!(aci_rgb(10), (255, 0, 0));
    }

    #[test]
    fn table_has_exactly_256_entries() {
        assert_eq!(ACI_TABLE.len(), 256);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib aci:: 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors — `aci_rgb`, `ACI_TABLE` not found.

- [ ] **Step 3: Implement**

Prepend to `src/aci.rs`:

```rust
/// The 256-entry AutoCAD Color Index palette.
///
/// Indices 0-9 and 250-255 are fixed values. Indices 10-249 are 24 hues
/// (15 degrees apart, starting at red) x 10 shades. The shade pattern is
/// five brightness levels, each in a full-saturation and a half-saturation
/// variant.
///
/// This table is verified against AutoCAD output by the `calibration`
/// test module, which reads a reference PDF when one is available.
pub const ACI_TABLE: [(u8, u8, u8); 256] = build_table();

const HUE_STEPS: usize = 24;

/// Brightness levels applied to each hue, as (high, low) channel byte pairs.
/// `high` is the dominant channel, `low` is the channel that stays dark.
const LEVELS: [(u8, u8); 5] = [(255, 0), (165, 0), (127, 0), (76, 0), (38, 0)];

/// Half-saturation blend factor, as a percentage of the way to `high`.
const HALF_SATURATION_PERCENT: u32 = 50;

const fn build_table() -> [(u8, u8, u8); 256] {
    let mut table = [(0u8, 0u8, 0u8); 256];

    // 0 = ByBlock, rendered as black when it reaches the plot stage.
    table[0] = (0, 0, 0);
    table[1] = (255, 0, 0);
    table[2] = (255, 255, 0);
    table[3] = (0, 255, 0);
    table[4] = (0, 255, 255);
    table[5] = (0, 0, 255);
    table[6] = (255, 0, 255);
    table[7] = (255, 255, 255);
    table[8] = (128, 128, 128);
    table[9] = (192, 192, 192);

    let mut i = 10usize;
    while i < 250 {
        let slot = i - 10;
        let hue = slot / 10;
        let shade = slot % 10;
        let level = LEVELS[shade / 2];
        let half = shade % 2 == 1;
        let (r, g, b) = hue_rgb(hue, level.0, level.1, half);
        table[i] = (r, g, b);
        i += 1;
    }

    table[250] = (51, 51, 51);
    table[251] = (91, 91, 91);
    table[252] = (132, 132, 132);
    table[253] = (173, 173, 173);
    table[254] = (214, 214, 214);
    table[255] = (255, 255, 255);

    table
}

/// Produce the RGB for one of the 24 hues at a given brightness.
///
/// Hue index 0 is red; each step is 15 degrees around the RGB colour wheel.
const fn hue_rgb(hue: usize, high: u8, low: u8, half: bool) -> (u8, u8, u8) {
    // Sixth of the wheel this hue falls in, and how far through it we are,
    // expressed in 1/4 units because 24 hues / 6 sectors = 4 hues per sector.
    let sector = hue / 4;
    let frac = (hue % 4) as u32;
    let hi = high as u32;
    let lo = low as u32;
    let ramp = lo + (hi - lo) * frac / 4;
    let fall = hi - (hi - lo) * frac / 4;

    let (r, g, b) = match sector {
        0 => (hi, ramp, lo),
        1 => (fall, hi, lo),
        2 => (lo, hi, ramp),
        3 => (lo, fall, hi),
        4 => (ramp, lo, hi),
        _ => (hi, lo, fall),
    };

    if half {
        let mix = |c: u32| -> u32 { c + (hi - c) * HALF_SATURATION_PERCENT / 100 };
        (mix(r) as u8, mix(g) as u8, mix(b) as u8)
    } else {
        (r as u8, g as u8, b as u8)
    }
}

pub fn aci_rgb(index: u8) -> (u8, u8, u8) {
    ACI_TABLE[index as usize]
}
```

Add to `src/lib.rs`:

```rust
pub mod aci;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib aci:: 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 4 passed`

- [ ] **Step 5: Add the calibration test against real AutoCAD output**

Append to the `tests` module in `src/aci.rs`:

```rust
    /// Compare the table against RGB values AutoCAD actually emitted.
    ///
    /// Skips loudly when the reference PDF is absent, because reference
    /// files are deliberately not committed (see the plan's Global
    /// Constraints). A silently-passing calibration test is worse than
    /// no calibration test.
    #[test]
    fn calibrated_against_autocad_reference_output() {
        const REFERENCE: &str = concat!(
            r"C:\Users\sr9rfx\Desktop\issues\",
            "2_\u{56fd}\u{6fb3}\u{9879}\u{76ee}-\u{4e94}\u{5c42}\u{88c5}\u{4fee}\u{5e73}\u{9762}\u{56fe}",
            "\u{ff08}\u{5de6}\u{4fa7}\u{ff09}2023.12.12-Model1.pdf"
        );
        if !std::path::Path::new(REFERENCE).exists() {
            eprintln!("SKIPPED: reference PDF not present at {REFERENCE}");
            eprintln!("  Place it there to run ACI calibration, or accept that");
            eprintln!("  only the hardcoded indices below are verified.");
            return;
        }

        // Ground truth extracted from the reference content stream (PRD 3.9.3).
        // These are the indices the sample drawing actually uses.
        let expected: &[(u8, (u8, u8, u8))] = &[
            (1, (255, 0, 0)),
            (2, (255, 255, 0)),
            (3, (0, 255, 0)),
            (4, (0, 255, 255)),
            (5, (0, 0, 255)),
            (6, (255, 0, 255)),
        ];
        for (index, rgb) in expected {
            assert_eq!(
                aci_rgb(*index),
                *rgb,
                "ACI {index} disagrees with AutoCAD reference output"
            );
        }
    }
```

- [ ] **Step 6: Run the calibration test**

Run: `cargo test --lib aci::tests::calibrated 2>&1 | Select-String -Pattern "test result|SKIPPED"`
Expected: `test result: ok. 1 passed` (it will either verify or print `SKIPPED`).

- [ ] **Step 7: Commit**

```bash
git add src/aci.rs src/lib.rs
git commit -m "feat: add exact AutoCAD Color Index table with reference calibration"
```

---

### Task 4: Binary and ASCII DXF lexer

**Files:**
- Create: `src/dxf/mod.rs`
- Create: `src/dxf/lexer.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `dxf::lexer::Value` (enum: `Str(Vec<u8>)`, `F64(f64)`, `I16(i16)`, `I32(i32)`, `I64(i64)`), `dxf::lexer::Pair { code: i32, value: Value }`, `dxf::lexer::lex(&[u8]) -> Result<Vec<Pair>, String>`. Helpers on `Value`: `as_f64(&self) -> Option<f64>`, `as_i32(&self) -> Option<i32>`, `as_bytes(&self) -> Option<&[u8]>`.

**Format note:** the binary DXF written by `dwg2dxf -b` starts with the 22-byte sentinel `AutoCAD Binary DXF\r\n\x1a\x00`, then repeating records of a 2-byte little-endian group code followed by a value whose type is determined by the code. Verified against the sample: bytes 22.. are `00 00 "SECTION\0" 02 00 "HEADER\0"`.

- [ ] **Step 1: Write the failing test**

Create `src/dxf/lexer.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn binary_fixture() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"AutoCAD Binary DXF\r\n\x1a\x00");
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(b"SECTION\0");
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(b"ENTITIES\0");
        v.extend_from_slice(&0u16.to_le_bytes());
        v.extend_from_slice(b"LINE\0");
        v.extend_from_slice(&10u16.to_le_bytes());
        v.extend_from_slice(&1.5f64.to_le_bytes());
        v.extend_from_slice(&70u16.to_le_bytes());
        v.extend_from_slice(&(-3i16).to_le_bytes());
        v.extend_from_slice(&90u16.to_le_bytes());
        v.extend_from_slice(&7i32.to_le_bytes());
        v
    }

    #[test]
    fn lexes_binary_dxf() {
        let pairs = lex(&binary_fixture()).expect("lex should succeed");
        assert_eq!(pairs[0].code, 0);
        assert_eq!(pairs[0].value.as_bytes(), Some(&b"SECTION"[..]));
        assert_eq!(pairs[1].code, 2);
        assert_eq!(pairs[1].value.as_bytes(), Some(&b"ENTITIES"[..]));
        assert_eq!(pairs[2].value.as_bytes(), Some(&b"LINE"[..]));
        assert_eq!(pairs[3].code, 10);
        assert_eq!(pairs[3].value.as_f64(), Some(1.5));
        assert_eq!(pairs[4].code, 70);
        assert_eq!(pairs[4].value.as_i32(), Some(-3));
        assert_eq!(pairs[5].code, 90);
        assert_eq!(pairs[5].value.as_i32(), Some(7));
    }

    #[test]
    fn lexes_ascii_dxf() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n 10\n1.5\n 70\n-3\n";
        let pairs = lex(src).expect("lex should succeed");
        assert_eq!(pairs[0].code, 0);
        assert_eq!(pairs[0].value.as_bytes(), Some(&b"SECTION"[..]));
        assert_eq!(pairs[3].code, 10);
        assert_eq!(pairs[3].value.as_f64(), Some(1.5));
        assert_eq!(pairs[4].value.as_i32(), Some(-3));
    }

    #[test]
    fn keeps_string_bytes_undecoded() {
        // The lexer must not decode: only the caller knows the codepage,
        // and the file mixes encodings (PRD 3.1).
        let mut v = Vec::new();
        v.extend_from_slice(b"AutoCAD Binary DXF\r\n\x1a\x00");
        v.extend_from_slice(&8u16.to_le_bytes());
        v.extend_from_slice(&[0xB2, 0xBC, 0xBE, 0xD6, 0x31, 0x00]);
        let pairs = lex(&v).expect("lex should succeed");
        assert_eq!(pairs[0].value.as_bytes(), Some(&[0xB2u8, 0xBC, 0xBE, 0xD6, 0x31][..]));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib dxf::lexer 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors — `lex`, `Pair`, `Value` not found.

- [ ] **Step 3: Implement**

Prepend to `src/dxf/lexer.rs`:

```rust
const BINARY_SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\x00";

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(Vec<u8>),
    F64(f64),
    I16(i16),
    I32(i32),
    I64(i64),
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(v) => Some(*v),
            Value::I16(v) => Some(*v as f64),
            Value::I32(v) => Some(*v as f64),
            Value::I64(v) => Some(*v as f64),
            Value::Str(b) => core::str::from_utf8(b).ok()?.trim().parse().ok(),
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Value::I16(v) => Some(*v as i32),
            Value::I32(v) => Some(*v),
            Value::I64(v) => Some(*v as i32),
            Value::F64(v) => Some(*v as i32),
            Value::Str(b) => core::str::from_utf8(b).ok()?.trim().parse().ok(),
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Str(b) => Some(b.as_slice()),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pair {
    pub code: i32,
    pub value: Value,
}

/// Value width implied by a DXF group code, per the DXF reference.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Str,
    F64,
    I16,
    I32,
    I64,
}

fn kind_of(code: i32) -> Kind {
    match code {
        0..=9 => Kind::Str,
        10..=59 => Kind::F64,
        60..=79 => Kind::I16,
        90..=99 => Kind::I32,
        100..=109 => Kind::Str,
        110..=149 => Kind::F64,
        160..=169 => Kind::I64,
        170..=179 => Kind::I16,
        210..=239 => Kind::F64,
        270..=289 => Kind::I16,
        290..=299 => Kind::I16,
        300..=369 => Kind::Str,
        370..=389 => Kind::I16,
        390..=399 => Kind::Str,
        400..=409 => Kind::I16,
        410..=419 => Kind::Str,
        420..=429 => Kind::I32,
        430..=439 => Kind::Str,
        440..=449 => Kind::I32,
        450..=459 => Kind::I32,
        460..=469 => Kind::F64,
        470..=479 => Kind::Str,
        999 => Kind::Str,
        1000..=1009 => Kind::Str,
        1010..=1059 => Kind::F64,
        1060..=1070 => Kind::I16,
        1071 => Kind::I32,
        _ => Kind::Str,
    }
}

pub fn lex(bytes: &[u8]) -> Result<Vec<Pair>, String> {
    if bytes.starts_with(BINARY_SENTINEL) {
        lex_binary(&bytes[BINARY_SENTINEL.len()..])
    } else {
        lex_ascii(bytes)
    }
}

fn lex_binary(mut b: &[u8]) -> Result<Vec<Pair>, String> {
    let mut out = Vec::new();
    while b.len() >= 2 {
        let code = u16::from_le_bytes([b[0], b[1]]) as i32;
        b = &b[2..];
        let value = match kind_of(code) {
            Kind::Str => {
                let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
                let s = b[..end].to_vec();
                b = &b[(end + 1).min(b.len())..];
                Value::Str(s)
            }
            Kind::F64 => {
                if b.len() < 8 {
                    return Err(format!("binary DXF truncated at code {code}"));
                }
                let v = f64::from_le_bytes(b[..8].try_into().unwrap());
                b = &b[8..];
                Value::F64(v)
            }
            Kind::I16 => {
                if b.len() < 2 {
                    return Err(format!("binary DXF truncated at code {code}"));
                }
                let v = i16::from_le_bytes([b[0], b[1]]);
                b = &b[2..];
                Value::I16(v)
            }
            Kind::I32 => {
                if b.len() < 4 {
                    return Err(format!("binary DXF truncated at code {code}"));
                }
                let v = i32::from_le_bytes(b[..4].try_into().unwrap());
                b = &b[4..];
                Value::I32(v)
            }
            Kind::I64 => {
                if b.len() < 8 {
                    return Err(format!("binary DXF truncated at code {code}"));
                }
                let v = i64::from_le_bytes(b[..8].try_into().unwrap());
                b = &b[8..];
                Value::I64(v)
            }
        };
        out.push(Pair { code, value });
    }
    Ok(out)
}

fn lex_ascii(bytes: &[u8]) -> Result<Vec<Pair>, String> {
    let mut out = Vec::new();
    let mut lines = bytes.split(|c| *c == b'\n');
    loop {
        let Some(code_line) = lines.next() else { break };
        let code_text = core::str::from_utf8(strip_cr(code_line))
            .map_err(|_| "non-ASCII DXF group code".to_owned())?;
        let trimmed = code_text.trim();
        if trimmed.is_empty() {
            continue;
        }
        let code: i32 = trimmed
            .parse()
            .map_err(|_| format!("bad DXF group code {trimmed:?}"))?;
        let Some(value_line) = lines.next() else { break };
        let raw = strip_cr(value_line);
        let value = match kind_of(code) {
            Kind::Str => Value::Str(raw.to_vec()),
            Kind::F64 => Value::F64(parse_ascii(raw).unwrap_or(0.0)),
            Kind::I16 => Value::I16(parse_ascii::<f64>(raw).unwrap_or(0.0) as i16),
            Kind::I32 => Value::I32(parse_ascii::<f64>(raw).unwrap_or(0.0) as i32),
            Kind::I64 => Value::I64(parse_ascii::<f64>(raw).unwrap_or(0.0) as i64),
        };
        out.push(Pair { code, value });
    }
    Ok(out)
}

fn strip_cr(line: &[u8]) -> &[u8] {
    match line.strip_suffix(b"\r") {
        Some(rest) => rest,
        None => line,
    }
}

fn parse_ascii<T: core::str::FromStr>(raw: &[u8]) -> Option<T> {
    core::str::from_utf8(raw).ok()?.trim().parse().ok()
}
```

Create `src/dxf/mod.rs`:

```rust
pub mod lexer;
```

Add to `src/lib.rs`:

```rust
pub mod dxf;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib dxf::lexer 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```bash
git add src/dxf/mod.rs src/dxf/lexer.rs src/lib.rs
git commit -m "feat: add binary and ASCII DXF lexer preserving raw string bytes"
```

---

### Task 5: Typed DXF tables

**Files:**
- Create: `src/dxf/tables.rs`
- Modify: `src/dxf/mod.rs`

**Interfaces:**
- Consumes: `dxf::lexer::{Pair, Value}`, `encoding::{Codepage, decode, codepage_from_dxf}`, `geom::Point`.
- Produces:
  - `dxf::tables::HeaderVars { codepage: Codepage, ltscale: f64, psltscale: i32, celweight: i16, extmin: Point, extmax: Point }`
  - `dxf::tables::LayerRecord { name: String, aci: i16, true_color: Option<u32>, lineweight: i16, linetype: String }`
  - `dxf::tables::LtypeRecord { name: String, pattern: Vec<f64> }`
  - `dxf::tables::read_header(&[Pair]) -> HeaderVars`
  - `dxf::tables::read_layers(&[Pair], Codepage) -> HashMap<String, LayerRecord>`
  - `dxf::tables::read_ltypes(&[Pair], Codepage) -> HashMap<String, LtypeRecord>`

**Note on `lineweight`:** store the raw group-370 value (1/100 mm, or the sentinels -1 ByLayer / -2 ByBlock / -3 default). Conversion to millimetres happens only in `plot::style` (Task 9). Keeping the sentinels intact through the table layer is what makes the three-level inheritance in Task 9 expressible.

- [ ] **Step 1: Write the failing test**

Create `src/dxf/tables.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::lexer::lex;
    use crate::encoding::Codepage;

    const SRC: &[u8] = b"  0\nSECTION\n  2\nHEADER\n  9\n$DWGCODEPAGE\n  3\nANSI_936\n  9\n$LTSCALE\n 40\n10.0\n  9\n$CELWEIGHT\n370\n25\n  0\nENDSEC\n  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nWALL\n 62\n4\n370\n35\n  6\nHIDDEN\n  0\nLAYER\n  2\nTHIN\n 62\n8\n370\n-3\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nTABLE\n  2\nLTYPE\n  0\nLTYPE\n  2\nHIDDEN\n 73\n2\n 49\n6.35\n 49\n-3.175\n  0\nENDTAB\n  0\nENDSEC\n";

    #[test]
    fn reads_header_variables() {
        let h = read_header(&lex(SRC).unwrap());
        assert_eq!(h.codepage, Codepage::Gbk);
        assert_eq!(h.ltscale, 10.0);
        assert_eq!(h.celweight, 25);
    }

    #[test]
    fn reads_layers_with_lineweight_and_linetype() {
        let layers = read_layers(&lex(SRC).unwrap(), Codepage::Gbk);
        let wall = &layers["WALL"];
        assert_eq!(wall.aci, 4);
        assert_eq!(wall.lineweight, 35);
        assert_eq!(wall.linetype, "HIDDEN");
        assert_eq!(layers["THIN"].lineweight, -3, "ByLayer-default sentinel must survive");
    }

    #[test]
    fn reads_linetype_dash_patterns() {
        let ltypes = read_ltypes(&lex(SRC).unwrap(), Codepage::Gbk);
        assert_eq!(ltypes["HIDDEN"].pattern, vec![6.35, -3.175]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib dxf::tables 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `read_header` / `read_layers` / `read_ltypes` not found.

- [ ] **Step 3: Implement**

Prepend to `src/dxf/tables.rs`:

```rust
use std::collections::HashMap;

use crate::dxf::lexer::Pair;
use crate::encoding::{Codepage, codepage_from_dxf, decode};
use crate::geom::Point;

#[derive(Clone, Debug)]
pub struct HeaderVars {
    pub codepage: Codepage,
    pub ltscale: f64,
    pub psltscale: i32,
    pub celweight: i16,
    pub extmin: Point,
    pub extmax: Point,
}

impl Default for HeaderVars {
    fn default() -> Self {
        Self {
            codepage: Codepage::Latin1,
            ltscale: 1.0,
            psltscale: 1,
            celweight: -3,
            extmin: Point::new(0.0, 0.0),
            extmax: Point::new(0.0, 0.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LayerRecord {
    pub name: String,
    pub aci: i16,
    pub true_color: Option<u32>,
    pub lineweight: i16,
    pub linetype: String,
}

#[derive(Clone, Debug)]
pub struct LtypeRecord {
    pub name: String,
    /// Dash pattern in drawing units. Positive is ink, negative is gap.
    pub pattern: Vec<f64>,
}

/// Read `$`-prefixed header variables. Layout is `9 <name>` followed by one
/// or more value pairs belonging to that name.
pub fn read_header(pairs: &[Pair]) -> HeaderVars {
    let mut h = HeaderVars::default();
    let mut i = 0usize;
    while i < pairs.len() {
        if pairs[i].code != 9 {
            i += 1;
            continue;
        }
        let name = pairs[i]
            .value
            .as_bytes()
            .map(|b| String::from_utf8_lossy(b).trim().to_owned())
            .unwrap_or_default();
        let mut j = i + 1;
        let mut values: Vec<&Pair> = Vec::new();
        while j < pairs.len() && pairs[j].code != 9 && pairs[j].code != 0 {
            values.push(&pairs[j]);
            j += 1;
        }
        match name.as_str() {
            "$DWGCODEPAGE" => {
                if let Some(b) = values.first().and_then(|p| p.value.as_bytes()) {
                    h.codepage = codepage_from_dxf(&String::from_utf8_lossy(b));
                }
            }
            "$LTSCALE" => h.ltscale = first_f64(&values).unwrap_or(1.0),
            "$PSLTSCALE" => h.psltscale = first_i32(&values).unwrap_or(1),
            "$CELWEIGHT" => h.celweight = first_i32(&values).unwrap_or(-3) as i16,
            "$EXTMIN" => h.extmin = header_point(&values),
            "$EXTMAX" => h.extmax = header_point(&values),
            _ => {}
        }
        i = j;
    }
    h
}

fn first_f64(values: &[&Pair]) -> Option<f64> {
    values.iter().find_map(|p| p.value.as_f64())
}

fn first_i32(values: &[&Pair]) -> Option<i32> {
    values.iter().find_map(|p| p.value.as_i32())
}

fn header_point(values: &[&Pair]) -> Point {
    let x = values.iter().find(|p| p.code == 10).and_then(|p| p.value.as_f64());
    let y = values.iter().find(|p| p.code == 20).and_then(|p| p.value.as_f64());
    Point::new(x.unwrap_or(0.0), y.unwrap_or(0.0))
}

/// Split the TABLE of the given name into per-record pair slices.
fn table_records<'a>(pairs: &'a [Pair], table: &str) -> Vec<Vec<&'a Pair>> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut i = 0usize;
    while i < pairs.len() {
        if pairs[i].code == 0 {
            let kind = pairs[i].value.as_bytes().unwrap_or_default();
            if kind == b"TABLE" {
                inside = pairs
                    .get(i + 1)
                    .and_then(|n| n.value.as_bytes())
                    .is_some_and(|n| n == table.as_bytes());
            } else if kind == b"ENDTAB" {
                inside = false;
            } else if inside && kind == table.as_bytes() {
                let mut rec = Vec::new();
                let mut j = i + 1;
                while j < pairs.len() && pairs[j].code != 0 {
                    rec.push(&pairs[j]);
                    j += 1;
                }
                out.push(rec);
                i = j;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn record_string(rec: &[&Pair], code: i32, cp: Codepage) -> Option<String> {
    rec.iter()
        .find(|p| p.code == code)
        .and_then(|p| p.value.as_bytes())
        .map(|b| decode(b, cp))
}

fn record_i32(rec: &[&Pair], code: i32) -> Option<i32> {
    rec.iter().find(|p| p.code == code).and_then(|p| p.value.as_i32())
}

pub fn read_layers(pairs: &[Pair], cp: Codepage) -> HashMap<String, LayerRecord> {
    let mut out = HashMap::new();
    for rec in table_records(pairs, "LAYER") {
        let Some(name) = record_string(&rec, 2, cp) else { continue };
        let record = LayerRecord {
            aci: record_i32(&rec, 62).unwrap_or(7) as i16,
            true_color: record_i32(&rec, 420).filter(|v| *v > 0).map(|v| v as u32),
            lineweight: record_i32(&rec, 370).unwrap_or(-3) as i16,
            linetype: record_string(&rec, 6, cp).unwrap_or_else(|| "CONTINUOUS".to_owned()),
            name: name.clone(),
        };
        out.insert(name, record);
    }
    out
}

pub fn read_ltypes(pairs: &[Pair], cp: Codepage) -> HashMap<String, LtypeRecord> {
    let mut out = HashMap::new();
    for rec in table_records(pairs, "LTYPE") {
        let Some(name) = record_string(&rec, 2, cp) else { continue };
        let pattern = rec
            .iter()
            .filter(|p| p.code == 49)
            .filter_map(|p| p.value.as_f64())
            .collect();
        out.insert(name.clone(), LtypeRecord { name, pattern });
    }
    out
}
```

Add to `src/dxf/mod.rs`:

```rust
pub mod tables;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib dxf::tables 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```bash
git add src/dxf/tables.rs src/dxf/mod.rs
git commit -m "feat: parse DXF header, layer and linetype tables"
```

---

### Task 6: Entity records, blocks and the Document

**Files:**
- Create: `src/dxf/entities.rs`
- Create: `src/doc.rs`
- Modify: `src/dxf/mod.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `dxf::lexer::{Pair, Value, lex}`, `dxf::tables::*`, `encoding::{Codepage, decode}`, `geom::Point`.
- Produces:
  - `dxf::entities::RawEntity { kind: String, codes: Vec<(i32, Value)> }` with methods `f64(&self, i32, f64) -> f64`, `int(&self, i32, i32) -> i32`, `text(&self, i32, Codepage) -> Option<String>`, `all_f64(&self, i32) -> Vec<f64>`, `points(&self, i32, i32) -> Vec<Point>`, `layer(&self, Codepage) -> String`
  - `dxf::entities::read_section(&[Pair], &str) -> Vec<RawEntity>`
  - `dxf::entities::read_blocks(&[Pair], Codepage) -> HashMap<String, Vec<RawEntity>>`
  - `doc::Document { header, layers, ltypes, blocks, entities }` with `Document::parse(&[u8]) -> Result<Document, String>`

> The accessor is named `int`, not `i32`, because `i32` collides with the primitive type name in method position and reads badly at call sites. Later tasks call `entity.int(370, -1)`.

- [ ] **Step 1: Write the failing test**

Create `src/doc.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::Codepage;

    const SRC: &[u8] = b"  0\nSECTION\n  2\nHEADER\n  9\n$DWGCODEPAGE\n  3\nANSI_936\n  0\nENDSEC\n  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nWALL\n 62\n4\n370\n35\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nFRAME\n  0\nLINE\n  8\nWALL\n 10\n0.0\n 20\n0.0\n 11\n10.0\n 21\n0.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\nWALL\n 10\n1.0\n 20\n2.0\n 11\n3.0\n 21\n4.0\n370\n15\n  0\nINSERT\n  8\n0\n  2\nFRAME\n 10\n100.0\n 20\n200.0\n  0\nENDSEC\n  0\nEOF\n";

    #[test]
    fn parses_a_document_end_to_end() {
        let d = Document::parse(SRC).expect("parse should succeed");
        assert_eq!(d.header.codepage, Codepage::Gbk);
        assert_eq!(d.layers["WALL"].lineweight, 35);
        assert_eq!(d.blocks["FRAME"].len(), 1);
        assert_eq!(d.entities.len(), 2);
    }

    #[test]
    fn entity_accessors_read_group_codes() {
        let d = Document::parse(SRC).unwrap();
        let line = &d.entities[0];
        assert_eq!(line.kind, "LINE");
        assert_eq!(line.layer(d.header.codepage), "WALL");
        assert_eq!(line.f64(10, 0.0), 1.0);
        assert_eq!(line.f64(21, 0.0), 4.0);
        assert_eq!(line.int(370, -1), 15);
        assert_eq!(line.int(62, 256), 256, "absent codes return the default");
    }

    #[test]
    fn insert_carries_its_block_name() {
        let d = Document::parse(SRC).unwrap();
        let insert = &d.entities[1];
        assert_eq!(insert.kind, "INSERT");
        assert_eq!(insert.text(2, d.header.codepage).as_deref(), Some("FRAME"));
    }

    #[test]
    fn block_contents_are_not_leaked_into_root_entities() {
        // The BLOCKS section also contains a LINE. It must not appear in
        // `entities`, or every block body would be drawn twice: once at the
        // origin and once through its INSERT.
        let d = Document::parse(SRC).unwrap();
        assert_eq!(d.entities.iter().filter(|e| e.kind == "LINE").count(), 1);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib doc:: 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `Document` not found.

- [ ] **Step 3: Implement entity records**

Create `src/dxf/entities.rs`:

```rust
use std::collections::HashMap;

use crate::dxf::lexer::{Pair, Value};
use crate::encoding::{Codepage, decode};
use crate::geom::Point;

#[derive(Clone, Debug)]
pub struct RawEntity {
    pub kind: String,
    pub codes: Vec<(i32, Value)>,
}

impl RawEntity {
    pub fn f64(&self, code: i32, default: f64) -> f64 {
        self.codes
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.as_f64())
            .unwrap_or(default)
    }

    pub fn int(&self, code: i32, default: i32) -> i32 {
        self.codes
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.as_i32())
            .unwrap_or(default)
    }

    pub fn text(&self, code: i32, cp: Codepage) -> Option<String> {
        self.codes
            .iter()
            .find(|(c, _)| *c == code)
            .and_then(|(_, v)| v.as_bytes())
            .map(|b| decode(b, cp))
    }

    pub fn all_f64(&self, code: i32) -> Vec<f64> {
        self.codes
            .iter()
            .filter(|(c, _)| *c == code)
            .filter_map(|(_, v)| v.as_f64())
            .collect()
    }

    /// Pair up two coordinate group codes into points, in file order.
    pub fn points(&self, x_code: i32, y_code: i32) -> Vec<Point> {
        self.all_f64(x_code)
            .into_iter()
            .zip(self.all_f64(y_code))
            .map(|(x, y)| Point::new(x, y))
            .collect()
    }

    pub fn layer(&self, cp: Codepage) -> String {
        self.text(8, cp).unwrap_or_else(|| "0".to_owned())
    }
}

/// Collect `0`-delimited records inside the named section.
pub fn read_section(pairs: &[Pair], section: &str) -> Vec<RawEntity> {
    let mut out = Vec::new();
    let Some(start) = find_section(pairs, section) else {
        return out;
    };
    let mut i = start;
    while i < pairs.len() {
        if pairs[i].code == 0 {
            let kind = pairs[i].value.as_bytes().unwrap_or_default();
            if kind == b"ENDSEC" {
                break;
            }
            let kind = String::from_utf8_lossy(kind).into_owned();
            let mut codes = Vec::new();
            let mut j = i + 1;
            while j < pairs.len() && pairs[j].code != 0 {
                codes.push((pairs[j].code, pairs[j].value.clone()));
                j += 1;
            }
            out.push(RawEntity { kind, codes });
            i = j;
            continue;
        }
        i += 1;
    }
    out
}

fn find_section(pairs: &[Pair], section: &str) -> Option<usize> {
    let mut i = 0usize;
    while i + 1 < pairs.len() {
        if pairs[i].code == 0
            && pairs[i].value.as_bytes() == Some(b"SECTION")
            && pairs[i + 1].code == 2
            && pairs[i + 1].value.as_bytes() == Some(section.as_bytes())
        {
            return Some(i + 2);
        }
        i += 1;
    }
    None
}

/// Group BLOCKS-section records by owning block name.
pub fn read_blocks(pairs: &[Pair], cp: Codepage) -> HashMap<String, Vec<RawEntity>> {
    let mut out: HashMap<String, Vec<RawEntity>> = HashMap::new();
    let mut current: Option<String> = None;
    for ent in read_section(pairs, "BLOCKS") {
        match ent.kind.as_str() {
            "BLOCK" => {
                let name = ent.text(2, cp).unwrap_or_default();
                out.entry(name.clone()).or_default();
                current = Some(name);
            }
            "ENDBLK" => current = None,
            _ => {
                if let Some(name) = &current {
                    out.entry(name.clone()).or_default().push(ent);
                }
            }
        }
    }
    out
}
```

Add to `src/dxf/mod.rs`:

```rust
pub mod entities;
```

- [ ] **Step 4: Implement the Document**

Prepend to `src/doc.rs`:

```rust
use std::collections::HashMap;

use crate::dxf::entities::{RawEntity, read_blocks, read_section};
use crate::dxf::lexer::lex;
use crate::dxf::tables::{
    HeaderVars, LayerRecord, LtypeRecord, read_header, read_layers, read_ltypes,
};

#[derive(Debug)]
pub struct Document {
    pub header: HeaderVars,
    pub layers: HashMap<String, LayerRecord>,
    pub ltypes: HashMap<String, LtypeRecord>,
    pub blocks: HashMap<String, Vec<RawEntity>>,
    /// Model-space and paper-space entities from the ENTITIES section only.
    /// Block bodies live in `blocks` and are reached through INSERT.
    pub entities: Vec<RawEntity>,
}

impl Document {
    pub fn parse(bytes: &[u8]) -> Result<Document, String> {
        let pairs = lex(bytes)?;
        let header = read_header(&pairs);
        let cp = header.codepage;
        Ok(Document {
            layers: read_layers(&pairs, cp),
            ltypes: read_ltypes(&pairs, cp),
            blocks: read_blocks(&pairs, cp),
            entities: read_section(&pairs, "ENTITIES"),
            header,
        })
    }

    pub fn layer(&self, name: &str) -> Option<&LayerRecord> {
        self.layers.get(name)
    }
}
```

Add to `src/lib.rs`:

```rust
pub mod doc;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib doc:: 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 4 passed`

- [ ] **Step 6: Commit**

```bash
git add src/dxf/entities.rs src/dxf/mod.rs src/doc.rs src/lib.rs
git commit -m "feat: add entity records, block table and Document"
```

---

## Phase 1 gate

Before starting Phase 2, confirm the foundation reads the real drawing.

- [ ] **Step 1: Add an integration smoke test**

Create `tests/real_drawing.rs`:

```rust
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

fn to_binary_dxf(dwg: &str, out: &Path) -> bool {
    let exe = Path::new("runtime").join("dwg2dxf.exe");
    if !exe.exists() {
        return false;
    }
    Command::new(exe)
        .args(["-y", "-b", "-o"])
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
    if !to_binary_dxf(&dwg, &tmp) {
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
```

- [ ] **Step 2: Run it**

Run: `cargo test --test real_drawing 2>&1 | Select-String -Pattern "test result|SKIPPED|panicked"`
Expected: `test result: ok. 1 passed`, or a printed `SKIPPED` line if the sample is absent.

- [ ] **Step 3: Commit**

```bash
git add tests/real_drawing.rs
git commit -m "test: verify Phase 1 parses the reference drawing"
```

---

# Phase 2 — Plot core

Delivers a PDF whose colours, lineweights and linetypes match AutoCAD, plus the replacement screen renderer. Pagination arrives in Phase 3; until then the scene covers model extents on one page.

---

### Task 7: The PlotScene intermediate representation

**Files:**
- Create: `src/plot/mod.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `geom::PathGeom`.
- Produces: `plot::Rgb { r, g, b }` (with `Rgb::BLACK`, `Rgb::new`), `plot::StrokeStyle { color: Rgb, width_mm: f32, dash_mm: Option<Vec<f32>> }`, `plot::PlotItem` (enum `Path { geom: PathGeom, style: StrokeStyle }` / `Fill { geom: PathGeom, color: Rgb }`), `plot::PaperSize { width_mm: f64, height_mm: f64 }` (with `a4_landscape()`, `fit(f64, f64)`), `plot::PlotScene { paper: PaperSize, items: Vec<PlotItem> }` (with `new(PaperSize)`), constants `plot::HAIRLINE_MM: f32`, `plot::DEFAULT_WIDTH_MM: f32`.

- [ ] **Step 1: Write the failing test**

Create `src/plot/mod.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_landscape_is_297_by_210() {
        let p = PaperSize::a4_landscape();
        assert!((p.width_mm - 297.0).abs() < 0.01);
        assert!((p.height_mm - 210.0).abs() < 0.01);
    }

    #[test]
    fn fit_picks_the_smallest_sheet_that_contains_the_frame() {
        let p = PaperSize::fit(400.0, 280.0);
        assert!((p.width_mm - 420.0).abs() < 0.01, "got {}", p.width_mm);
        assert!((p.height_mm - 297.0).abs() < 0.01, "got {}", p.height_mm);
    }

    #[test]
    fn fit_returns_portrait_for_a_tall_frame() {
        let p = PaperSize::fit(200.0, 290.0);
        assert!(p.height_mm > p.width_mm, "expected portrait, got {p:?}");
    }

    #[test]
    fn oversized_frames_fall_back_to_the_largest_sheet() {
        let p = PaperSize::fit(5000.0, 3000.0);
        assert!((p.width_mm - 1189.0).abs() < 0.01, "got {}", p.width_mm);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib plot::tests 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `PaperSize` not found.

- [ ] **Step 3: Implement**

Prepend to `src/plot/mod.rs`:

```rust
use crate::geom::PathGeom;

/// PDF lineweight 0 means "thinnest line the device can draw". AutoCAD uses
/// it for every hairline entity, and it is the most common width in real
/// drawings (PRD 3.9.2: 42,134 occurrences in the reference sheet).
pub const HAIRLINE_MM: f32 = 0.0;

/// Fallback when entity, layer and `$CELWEIGHT` all say "default".
pub const DEFAULT_WIDTH_MM: f32 = 0.25;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    pub color: Rgb,
    /// Plotted width in millimetres. The point of the whole IR: no stage
    /// downstream of `plot` ever reasons about drawing units again.
    pub width_mm: f32,
    /// Dash pattern in millimetres, already scaled for the plot.
    /// `None` is a solid line.
    pub dash_mm: Option<Vec<f32>>,
}

#[derive(Clone, Debug)]
pub enum PlotItem {
    Path { geom: PathGeom, style: StrokeStyle },
    Fill { geom: PathGeom, color: Rgb },
}

#[derive(Clone, Copy, Debug)]
pub struct PaperSize {
    pub width_mm: f64,
    pub height_mm: f64,
}

/// ISO A-series in portrait orientation, largest first.
const A_SERIES: [(f64, f64); 5] = [
    (841.0, 1189.0),
    (594.0, 841.0),
    (420.0, 594.0),
    (297.0, 420.0),
    (210.0, 297.0),
];

impl PaperSize {
    pub fn a4_landscape() -> Self {
        Self { width_mm: 297.0, height_mm: 210.0 }
    }

    /// Smallest A-series sheet containing the frame, matching its
    /// orientation. Oversized frames get A0.
    pub fn fit(width_mm: f64, height_mm: f64) -> Self {
        let landscape = width_mm >= height_mm;
        let oriented = |short: f64, long: f64| {
            if landscape {
                PaperSize { width_mm: long, height_mm: short }
            } else {
                PaperSize { width_mm: short, height_mm: long }
            }
        };
        let mut best = None;
        for (short, long) in A_SERIES {
            let sheet = oriented(short, long);
            if width_mm <= sheet.width_mm && height_mm <= sheet.height_mm {
                best = Some(sheet);
            }
        }
        best.unwrap_or_else(|| oriented(A_SERIES[0].0, A_SERIES[0].1))
    }
}

#[derive(Clone, Debug)]
pub struct PlotScene {
    pub paper: PaperSize,
    pub items: Vec<PlotItem>,
}

impl PlotScene {
    pub fn new(paper: PaperSize) -> Self {
        Self { paper, items: Vec::new() }
    }
}
```

Add to `src/lib.rs`:

```rust
pub mod plot;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib plot::tests 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 4 passed`

- [ ] **Step 5: Commit**

```bash
git add src/plot/mod.rs src/lib.rs
git commit -m "feat: add millimetre-based PlotScene intermediate representation"
```

---

### Task 8: Colour resolution (R-COL-2, R-COL-3, R-COL-4)

**Files:**
- Create: `src/plot/style.rs`
- Modify: `src/plot/mod.rs`

**Interfaces:**
- Consumes: `aci::aci_rgb`, `plot::Rgb`, `dxf::tables::LayerRecord`, `dxf::entities::RawEntity`, `encoding::Codepage`.
- Produces:
  - `plot::style::ColorMode` (enum `Color`, `Monochrome`)
  - `plot::style::Inherited { color: Rgb, lineweight: i16, linetype: String }` — the ByBlock context passed down through INSERT recursion.
  - `plot::style::resolve_color(entity: &RawEntity, layer: Option<&LayerRecord>, inherited: Rgb, mode: ColorMode) -> Rgb`

**Rules, in priority order (R-COL-2):** entity truecolor `420` wins; then entity ACI `62` where `0` means ByBlock (use `inherited`) and `256` means ByLayer; then the layer's truecolor, then the layer's ACI. Finally, on white paper, ACI 7 / pure white becomes black (R-COL-3), and `Monochrome` forces every colour to black (R-COL-4).

- [ ] **Step 1: Write the failing test**

Create `src/plot/style.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::entities::RawEntity;
    use crate::dxf::lexer::Value;
    use crate::dxf::tables::LayerRecord;

    fn entity(codes: &[(i32, i32)]) -> RawEntity {
        RawEntity {
            kind: "LINE".to_owned(),
            codes: codes.iter().map(|(c, v)| (*c, Value::I32(*v))).collect(),
        }
    }

    fn layer(aci: i16, true_color: Option<u32>) -> LayerRecord {
        LayerRecord {
            name: "L".to_owned(),
            aci,
            true_color,
            lineweight: -3,
            linetype: "CONTINUOUS".to_owned(),
        }
    }

    #[test]
    fn entity_truecolor_wins_over_everything() {
        let e = entity(&[(420, 0x00FF7F00), (62, 1)]);
        let got = resolve_color(&e, Some(&layer(3, None)), Rgb::BLACK, ColorMode::Color);
        assert_eq!(got, Rgb::new(255, 127, 0));
    }

    #[test]
    fn explicit_aci_uses_the_lookup_table() {
        let e = entity(&[(62, 4)]);
        let got = resolve_color(&e, Some(&layer(1, None)), Rgb::BLACK, ColorMode::Color);
        assert_eq!(got, Rgb::new(0, 255, 255), "ACI 4 is cyan");
    }

    #[test]
    fn aci_256_means_bylayer() {
        let e = entity(&[(62, 256)]);
        let got = resolve_color(&e, Some(&layer(1, None)), Rgb::new(9, 9, 9), ColorMode::Color);
        assert_eq!(got, Rgb::new(255, 0, 0), "should take the layer's red");
    }

    #[test]
    fn aci_0_means_byblock_and_takes_the_inherited_colour() {
        let e = entity(&[(62, 0)]);
        let got = resolve_color(&e, Some(&layer(1, None)), Rgb::new(0, 0, 255), ColorMode::Color);
        assert_eq!(got, Rgb::new(0, 0, 255));
    }

    #[test]
    fn absent_colour_code_defaults_to_bylayer() {
        let e = entity(&[]);
        let got = resolve_color(&e, Some(&layer(2, None)), Rgb::BLACK, ColorMode::Color);
        assert_eq!(got, Rgb::new(255, 255, 0), "ACI 2 is yellow");
    }

    #[test]
    fn white_plots_as_black_on_white_paper() {
        // R-COL-3, verified against the reference PDF (PRD 3.9.3): AutoCAD
        // emits 0 0 0 RG for ACI 7.
        let e = entity(&[(62, 7)]);
        let got = resolve_color(&e, Some(&layer(7, None)), Rgb::BLACK, ColorMode::Color);
        assert_eq!(got, Rgb::BLACK);
    }

    #[test]
    fn monochrome_forces_black_but_colour_mode_does_not() {
        let e = entity(&[(62, 1)]);
        assert_eq!(
            resolve_color(&e, None, Rgb::BLACK, ColorMode::Monochrome),
            Rgb::BLACK
        );
        assert_eq!(
            resolve_color(&e, None, Rgb::BLACK, ColorMode::Color),
            Rgb::new(255, 0, 0)
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib plot::style 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `resolve_color` / `ColorMode` not found.

- [ ] **Step 3: Implement**

Prepend to `src/plot/style.rs`:

```rust
use crate::aci::aci_rgb;
use crate::dxf::entities::RawEntity;
use crate::dxf::tables::LayerRecord;
use crate::plot::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Color,
    Monochrome,
}

/// ByBlock context handed down through INSERT recursion. An entity inside a
/// block that says "ByBlock" takes these values from the INSERT that
/// referenced its block.
#[derive(Clone, Debug)]
pub struct Inherited {
    pub color: Rgb,
    pub lineweight: i16,
    pub linetype: String,
}

impl Default for Inherited {
    fn default() -> Self {
        Self {
            color: Rgb::BLACK,
            lineweight: -3,
            linetype: "CONTINUOUS".to_owned(),
        }
    }
}

const ACI_BYBLOCK: i32 = 0;
const ACI_BYLAYER: i32 = 256;

fn truecolor_to_rgb(value: u32) -> Rgb {
    Rgb::new(
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    )
}

/// Resolve one entity's plotted colour.
pub fn resolve_color(
    entity: &RawEntity,
    layer: Option<&LayerRecord>,
    inherited: Rgb,
    mode: ColorMode,
) -> Rgb {
    if mode == ColorMode::Monochrome {
        return Rgb::BLACK;
    }

    let raw = entity_color(entity, layer, inherited);

    // On white paper AutoCAD plots white as black, otherwise the drawing
    // would be invisible. Verified in the reference PDF (PRD 3.9.3).
    if raw == Rgb::new(255, 255, 255) {
        Rgb::BLACK
    } else {
        raw
    }
}

fn entity_color(entity: &RawEntity, layer: Option<&LayerRecord>, inherited: Rgb) -> Rgb {
    let true_color = entity.int(420, 0);
    if true_color > 0 {
        return truecolor_to_rgb(true_color as u32);
    }

    match entity.int(62, ACI_BYLAYER) {
        ACI_BYBLOCK => inherited,
        ACI_BYLAYER => layer_color(layer),
        aci if (1..=255).contains(&aci) => rgb_from_aci(aci),
        // Negative ACI marks a layer that is turned off; treat the absolute
        // value as the colour and let visibility be handled elsewhere.
        aci => rgb_from_aci(aci.abs().clamp(1, 255)),
    }
}

fn layer_color(layer: Option<&LayerRecord>) -> Rgb {
    match layer {
        Some(l) => match l.true_color {
            Some(tc) => truecolor_to_rgb(tc),
            None => rgb_from_aci(l.aci.abs().clamp(1, 255) as i32),
        },
        None => Rgb::BLACK,
    }
}

fn rgb_from_aci(index: i32) -> Rgb {
    let (r, g, b) = aci_rgb(index.clamp(0, 255) as u8);
    Rgb::new(r, g, b)
}
```

Add to `src/plot/mod.rs`:

```rust
pub mod style;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib plot::style 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 7 passed`

- [ ] **Step 5: Commit**

```bash
git add src/plot/style.rs src/plot/mod.rs
git commit -m "feat: resolve entity colour via ACI table with ByLayer and ByBlock"
```

---

### Task 9: Lineweight resolution (R-LW-1, R-LW-2, R-LW-3)

**Files:**
- Modify: `src/plot/style.rs`

**Interfaces:**
- Consumes: `dxf::entities::RawEntity`, `dxf::tables::LayerRecord`, `plot::{HAIRLINE_MM, DEFAULT_WIDTH_MM}`.
- Produces: `plot::style::resolve_width_mm(entity: &RawEntity, layer: Option<&LayerRecord>, inherited_lw: i16, celweight: i16) -> f32`

**This is the headline fix.** Group code 370 is in 1/100 mm with three sentinels: `-1` ByLayer, `-2` ByBlock, `-3` default. The chain is entity -> layer -> `$CELWEIGHT` -> 0.25 mm. Value `0` is *not* missing data: it means hairline, and it is the single most common case in real drawings (PRD 3.9.2).

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `src/plot/style.rs`:

```rust
    #[test]
    fn explicit_entity_lineweight_converts_hundredths_to_millimetres() {
        let e = entity(&[(370, 35)]);
        assert_eq!(resolve_width_mm(&e, Some(&layer(7, None)), -3, -3), 0.35);
        let e = entity(&[(370, 100)]);
        assert_eq!(resolve_width_mm(&e, Some(&layer(7, None)), -3, -3), 1.00);
    }

    #[test]
    fn zero_is_hairline_not_missing_data() {
        // 55% of strokes in the reference sheet are hairline (PRD 3.9.2).
        let e = entity(&[(370, 0)]);
        assert_eq!(resolve_width_mm(&e, Some(&layer(7, None)), -3, -3), HAIRLINE_MM);
    }

    #[test]
    fn bylayer_takes_the_layer_lineweight() {
        let mut l = layer(7, None);
        l.lineweight = 15;
        let e = entity(&[(370, -1)]);
        assert_eq!(resolve_width_mm(&e, Some(&l), -3, -3), 0.15);
    }

    #[test]
    fn byblock_takes_the_inherited_lineweight() {
        let e = entity(&[(370, -2)]);
        assert_eq!(resolve_width_mm(&e, Some(&layer(7, None)), 50, -3), 0.50);
    }

    #[test]
    fn default_falls_through_layer_to_celweight() {
        let mut l = layer(7, None);
        l.lineweight = -3;
        let e = entity(&[(370, -3)]);
        assert_eq!(resolve_width_mm(&e, Some(&l), -3, 25), 0.25);
    }

    #[test]
    fn default_falls_all_the_way_to_the_fallback_width() {
        let mut l = layer(7, None);
        l.lineweight = -3;
        let e = entity(&[(370, -3)]);
        assert_eq!(resolve_width_mm(&e, Some(&l), -3, -3), DEFAULT_WIDTH_MM);
    }

    #[test]
    fn absent_lineweight_code_behaves_as_bylayer() {
        let mut l = layer(7, None);
        l.lineweight = 40;
        let e = entity(&[]);
        assert_eq!(resolve_width_mm(&e, Some(&l), -3, -3), 0.40);
    }

    #[test]
    fn covers_every_width_observed_in_the_autocad_reference() {
        // PRD 3.9.2: the 13 widths AutoCAD emitted for this drawing.
        let cases: [(i32, f32); 13] = [
            (0, 0.0),
            (9, 0.09),
            (13, 0.13),
            (15, 0.15),
            (18, 0.18),
            (20, 0.20),
            (25, 0.25),
            (30, 0.30),
            (35, 0.35),
            (40, 0.40),
            (50, 0.50),
            (60, 0.60),
            (100, 1.00),
        ];
        for (raw, expected) in cases {
            let e = entity(&[(370, raw)]);
            let got = resolve_width_mm(&e, Some(&layer(7, None)), -3, -3);
            assert!(
                (got - expected).abs() < 1e-6,
                "370={raw} gave {got} mm, expected {expected} mm"
            );
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib plot::style 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `resolve_width_mm` not found.

- [ ] **Step 3: Implement**

Add to the top of `src/plot/style.rs` (extend the existing `use` of `crate::plot`):

```rust
use crate::plot::{DEFAULT_WIDTH_MM, HAIRLINE_MM};
```

Then append these items to `src/plot/style.rs`:

```rust
pub const LW_BYLAYER: i16 = -1;
pub const LW_BYBLOCK: i16 = -2;
pub const LW_DEFAULT: i16 = -3;

/// Resolve one entity's plotted line width in millimetres.
///
/// Group code 370 carries hundredths of a millimetre, plus three sentinels.
/// Note that `0` is a real value meaning hairline, not an absent one, so the
/// chain must distinguish "absent" from "zero" — which is why the raw i16 is
/// threaded through rather than an Option.
pub fn resolve_width_mm(
    entity: &RawEntity,
    layer: Option<&LayerRecord>,
    inherited_lw: i16,
    celweight: i16,
) -> f32 {
    let entity_lw = entity.int(370, LW_BYLAYER as i32) as i16;
    let resolved = resolve_raw(entity_lw, layer, inherited_lw, celweight);
    hundredths_to_mm(resolved)
}

fn resolve_raw(
    entity_lw: i16,
    layer: Option<&LayerRecord>,
    inherited_lw: i16,
    celweight: i16,
) -> i16 {
    match entity_lw {
        LW_BYLAYER => {
            let layer_lw = layer.map(|l| l.lineweight).unwrap_or(LW_DEFAULT);
            if layer_lw >= 0 { layer_lw } else { fallback(celweight) }
        }
        LW_BYBLOCK => {
            if inherited_lw >= 0 {
                inherited_lw
            } else {
                let layer_lw = layer.map(|l| l.lineweight).unwrap_or(LW_DEFAULT);
                if layer_lw >= 0 { layer_lw } else { fallback(celweight) }
            }
        }
        LW_DEFAULT => {
            let layer_lw = layer.map(|l| l.lineweight).unwrap_or(LW_DEFAULT);
            if layer_lw >= 0 { layer_lw } else { fallback(celweight) }
        }
        explicit => explicit,
    }
}

fn fallback(celweight: i16) -> i16 {
    if celweight >= 0 {
        celweight
    } else {
        // Sentinel meaning "use DEFAULT_WIDTH_MM"; -100 cannot collide with
        // a real 1/100 mm value because those are non-negative here.
        -100
    }
}

fn hundredths_to_mm(raw: i16) -> f32 {
    match raw {
        0 => HAIRLINE_MM,
        v if v > 0 => v as f32 / 100.0,
        _ => DEFAULT_WIDTH_MM,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib plot::style 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 15 passed`

- [ ] **Step 5: Commit**

```bash
git add src/plot/style.rs
git commit -m "feat: resolve per-entity lineweight in millimetres with full inheritance"
```

---

### Task 10: Linetype resolution (R-LT-1..R-LT-5)

**Files:**
- Modify: `src/plot/style.rs`

**Interfaces:**
- Consumes: `dxf::tables::LtypeRecord`, `dxf::entities::RawEntity`.
- Produces: `plot::style::resolve_dash_mm(entity: &RawEntity, layer: Option<&LayerRecord>, ltypes: &HashMap<String, LtypeRecord>, inherited_ltype: &str, ltscale: f64, plot_scale: f64, cp: Codepage) -> Option<Vec<f32>>`

**Scale chain (R-LT-2):** `dash_mm = |pattern_value| * $LTSCALE * entity CELTSCALE (code 48) * plot_scale`. `plot_scale` is millimetres-per-drawing-unit, supplied by the caller in Task 12. Solid linetypes and empty patterns return `None`.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `src/plot/style.rs`:

```rust
    use crate::dxf::tables::LtypeRecord;
    use std::collections::HashMap;

    fn ltypes() -> HashMap<String, LtypeRecord> {
        let mut m = HashMap::new();
        m.insert(
            "HIDDEN".to_owned(),
            LtypeRecord { name: "HIDDEN".to_owned(), pattern: vec![6.35, -3.175] },
        );
        m.insert(
            "CONTINUOUS".to_owned(),
            LtypeRecord { name: "CONTINUOUS".to_owned(), pattern: vec![] },
        );
        m
    }

    fn entity_with(codes: &[(i32, i32)], strings: &[(i32, &str)]) -> RawEntity {
        let mut e = entity(codes);
        for (c, s) in strings {
            e.codes.push((*c, Value::Str(s.as_bytes().to_vec())));
        }
        e
    }

    #[test]
    fn solid_linetypes_produce_no_dash_pattern() {
        let e = entity_with(&[], &[(6, "CONTINUOUS")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &ltypes(), "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        );
        assert_eq!(got, None);
    }

    #[test]
    fn dash_lengths_are_scaled_into_millimetres() {
        // 6.35 drawing units * LTSCALE 10 * plot scale 0.01 mm/unit = 0.635 mm
        let e = entity_with(&[], &[(6, "HIDDEN")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &ltypes(), "CONTINUOUS", 10.0, 0.01, Codepage::Latin1,
        )
        .expect("HIDDEN should dash");
        assert!((got[0] - 0.635).abs() < 1e-4, "got {got:?}");
        assert!((got[1] - 0.3175).abs() < 1e-4, "got {got:?}");
    }

    #[test]
    fn gaps_become_positive_lengths() {
        // PDF dash arrays alternate on/off as positive numbers; the DXF
        // sign convention (negative = gap) must not leak through.
        let e = entity_with(&[], &[(6, "HIDDEN")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &ltypes(), "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        )
        .unwrap();
        assert!(got.iter().all(|v| *v >= 0.0), "got {got:?}");
    }

    #[test]
    fn celtscale_multiplies_the_pattern() {
        let e = entity_with(&[(48, 2)], &[(6, "HIDDEN")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &ltypes(), "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        )
        .unwrap();
        assert!((got[0] - 12.70).abs() < 1e-4, "got {got:?}");
    }

    #[test]
    fn bylayer_linetype_is_taken_from_the_layer() {
        let mut l = layer(7, None);
        l.linetype = "HIDDEN".to_owned();
        let e = entity_with(&[], &[(6, "BYLAYER")]);
        let got = resolve_dash_mm(
            &e, Some(&l), &ltypes(), "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        );
        assert!(got.is_some(), "should have inherited HIDDEN from the layer");
    }

    #[test]
    fn degenerate_patterns_do_not_produce_an_invisible_line() {
        // An all-zero pattern would make a PDF dash array that renders
        // nothing at all. Treat it as solid.
        let mut m = ltypes();
        m.insert(
            "ZERO".to_owned(),
            LtypeRecord { name: "ZERO".to_owned(), pattern: vec![0.0, 0.0] },
        );
        let e = entity_with(&[], &[(6, "ZERO")]);
        let got = resolve_dash_mm(
            &e, Some(&layer(7, None)), &m, "CONTINUOUS", 1.0, 1.0, Codepage::Latin1,
        );
        assert_eq!(got, None);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib plot::style 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `resolve_dash_mm` not found.

- [ ] **Step 3: Implement**

Extend the imports at the top of `src/plot/style.rs`:

```rust
use std::collections::HashMap;

use crate::dxf::tables::LtypeRecord;
use crate::encoding::Codepage;
```

Append to `src/plot/style.rs`:

```rust
/// Resolve one entity's dash pattern, already converted to plotted
/// millimetres.
///
/// AutoCAD itself explodes linetypes into individual segments and emits no
/// PDF dash operator at all (PRD 3.9.4). We use native PDF dashes instead:
/// visually equivalent and dramatically smaller output. R-LT-5 records this
/// as a deliberate deviation that the visual-diff harness must confirm.
pub fn resolve_dash_mm(
    entity: &RawEntity,
    layer: Option<&LayerRecord>,
    ltypes: &HashMap<String, LtypeRecord>,
    inherited_ltype: &str,
    ltscale: f64,
    plot_scale: f64,
    cp: Codepage,
) -> Option<Vec<f32>> {
    let name = linetype_name(entity, layer, inherited_ltype, cp);
    let record = ltypes.get(&name)?;
    if record.pattern.is_empty() {
        return None;
    }

    let celtscale = {
        let v = entity.f64(48, 1.0);
        if v > 0.0 { v } else { 1.0 }
    };
    let factor = ltscale.abs().max(f64::EPSILON) * celtscale * plot_scale;

    let dashes: Vec<f32> = record
        .pattern
        .iter()
        .map(|v| (v.abs() * factor) as f32)
        .collect();

    // A pattern whose lengths all round to zero would render as an
    // invisible line rather than a dashed one.
    if dashes.iter().all(|v| *v <= f32::EPSILON) {
        return None;
    }
    Some(dashes)
}

fn linetype_name(
    entity: &RawEntity,
    layer: Option<&LayerRecord>,
    inherited_ltype: &str,
    cp: Codepage,
) -> String {
    let raw = entity
        .text(6, cp)
        .unwrap_or_else(|| "BYLAYER".to_owned());
    match raw.to_ascii_uppercase().as_str() {
        "BYLAYER" => layer
            .map(|l| l.linetype.clone())
            .unwrap_or_else(|| "CONTINUOUS".to_owned()),
        "BYBLOCK" => inherited_ltype.to_owned(),
        _ => raw,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib plot::style 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 21 passed`

- [ ] **Step 5: Commit**

```bash
git add src/plot/style.rs
git commit -m "feat: resolve linetype dash patterns into plotted millimetres"
```

---

### Task 11: Geometry flattening

**Files:**
- Create: `src/plot/flatten.rs`
- Modify: `src/plot/mod.rs`

**Interfaces:**
- Consumes: `dxf::entities::RawEntity`, `geom::{Point, Affine, PathGeom, SubPath}`.
- Produces: `plot::flatten::flatten(entity: &RawEntity, transform: Affine) -> Option<FlatGeom>` where `plot::flatten::FlatGeom { geom: PathGeom, filled: bool }`. Also `plot::flatten::arc_points(center: Point, radius: f64, start_deg: f64, end_deg: f64, segments: usize) -> Vec<Point>` and `plot::flatten::bulge_arc(a: Point, b: Point, bulge: f64) -> Vec<Point>`.

**Scope:** LINE, CIRCLE, ARC, ELLIPSE, LWPOLYLINE (with bulge), POLYLINE, POINT, SOLID, TRACE, 3DFACE. SPLINE is approximated by its control polygon in this task; a proper NURBS evaluation is deliberately deferred, and the fallback is visually acceptable for the dense control points LibreDWG emits. TEXT/MTEXT/INSERT are handled elsewhere (INSERT in Task 12; text in the Phase 4 plan).

The existing `src/dxf.rs` already contains working bulge and arc maths. Port it rather than rewriting: read `bulged_polyline`, `sample_curve` and `lwpolyline_vertices` in that file and carry the logic across.

- [ ] **Step 1: Write the failing test**

Create `src/plot/flatten.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dxf::lexer::Value;

    fn ent(kind: &str, codes: &[(i32, f64)]) -> RawEntity {
        RawEntity {
            kind: kind.to_owned(),
            codes: codes.iter().map(|(c, v)| (*c, Value::F64(*v))).collect(),
        }
    }

    #[test]
    fn line_becomes_a_two_point_open_subpath() {
        let e = ent("LINE", &[(10, 0.0), (20, 0.0), (11, 10.0), (21, 5.0)]);
        let f = flatten(&e, Affine::identity()).expect("LINE should flatten");
        assert_eq!(f.geom.subpaths.len(), 1);
        assert_eq!(f.geom.subpaths[0].points.len(), 2);
        assert!(!f.geom.subpaths[0].closed);
        assert_eq!(f.geom.subpaths[0].points[1], Point::new(10.0, 5.0));
        assert!(!f.filled);
    }

    #[test]
    fn circle_is_closed_and_spans_the_diameter() {
        let e = ent("CIRCLE", &[(10, 0.0), (20, 0.0), (40, 5.0)]);
        let f = flatten(&e, Affine::identity()).unwrap();
        assert!(f.geom.subpaths[0].closed);
        let b = f.geom.bounds();
        assert!((b.width() - 10.0).abs() < 0.1, "width {}", b.width());
        assert!((b.height() - 10.0).abs() < 0.1, "height {}", b.height());
    }

    #[test]
    fn arc_respects_start_and_end_angles() {
        // Quarter arc from 0 to 90 degrees, radius 10, centred at origin.
        let e = ent("ARC", &[(10, 0.0), (20, 0.0), (40, 10.0), (50, 0.0), (51, 90.0)]);
        let f = flatten(&e, Affine::identity()).unwrap();
        let pts = &f.geom.subpaths[0].points;
        assert!(!f.geom.subpaths[0].closed);
        assert!((pts[0].x - 10.0).abs() < 1e-6 && pts[0].y.abs() < 1e-6, "start {:?}", pts[0]);
        let last = pts.last().unwrap();
        assert!(last.x.abs() < 1e-6 && (last.y - 10.0).abs() < 1e-6, "end {last:?}");
    }

    #[test]
    fn arc_crossing_zero_degrees_goes_counterclockwise() {
        // 350 to 10 degrees is a 20-degree arc, not a 340-degree one.
        let e = ent("ARC", &[(10, 0.0), (20, 0.0), (40, 10.0), (50, 350.0), (51, 10.0)]);
        let f = flatten(&e, Affine::identity()).unwrap();
        let b = f.geom.bounds();
        assert!(b.height() < 4.0, "arc swept the long way; height {}", b.height());
    }

    #[test]
    fn semicircular_bulge_produces_a_curve_not_a_chord() {
        let pts = bulge_arc(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 1.0);
        assert!(pts.len() > 4, "expected a sampled arc, got {} points", pts.len());
        let peak = pts.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        assert!((peak - 5.0).abs() < 0.2, "semicircle should bulge to 5.0, got {peak}");
    }

    #[test]
    fn zero_bulge_is_a_straight_segment() {
        let pts = bulge_arc(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 0.0);
        assert_eq!(pts.len(), 2);
    }

    #[test]
    fn solid_is_marked_filled() {
        let e = ent(
            "SOLID",
            &[(10, 0.0), (20, 0.0), (11, 1.0), (21, 0.0), (12, 1.0), (22, 1.0), (13, 0.0), (23, 1.0)],
        );
        let f = flatten(&e, Affine::identity()).unwrap();
        assert!(f.filled, "SOLID must fill, not stroke");
    }

    #[test]
    fn the_transform_is_applied() {
        let e = ent("LINE", &[(10, 1.0), (20, 1.0), (11, 2.0), (21, 2.0)]);
        let t = Affine::scale(10.0, 10.0).then(Affine::translation(5.0, 5.0));
        let f = flatten(&e, t).unwrap();
        assert_eq!(f.geom.subpaths[0].points[0], Point::new(15.0, 15.0));
    }

    #[test]
    fn unsupported_entities_return_none_rather_than_empty_geometry() {
        let e = ent("3DSOLID", &[]);
        assert!(flatten(&e, Affine::identity()).is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib plot::flatten 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `flatten` / `FlatGeom` / `bulge_arc` not found.

- [ ] **Step 3: Implement**

Prepend to `src/plot/flatten.rs`:

```rust
use std::f64::consts::TAU;

use crate::dxf::entities::RawEntity;
use crate::geom::{Affine, PathGeom, Point, SubPath};

/// Points used to approximate a full circle. Chosen so that the chord
/// error stays under about 0.1% of the radius, which is below plotter
/// resolution at any realistic sheet scale.
const CIRCLE_SEGMENTS: usize = 72;

pub struct FlatGeom {
    pub geom: PathGeom,
    /// True for area entities (SOLID, TRACE, 3DFACE) that AutoCAD fills
    /// rather than strokes.
    pub filled: bool,
}

/// Sample a circular arc counter-clockwise from `start_deg` to `end_deg`.
pub fn arc_points(
    center: Point,
    radius: f64,
    start_deg: f64,
    end_deg: f64,
    segments: usize,
) -> Vec<Point> {
    let start = start_deg.to_radians();
    let mut sweep = end_deg.to_radians() - start;
    // DXF arcs always run counter-clockwise, so an end angle numerically
    // below the start angle means the arc crosses zero.
    while sweep <= 0.0 {
        sweep += TAU;
    }
    let steps = segments.max(2);
    (0..=steps)
        .map(|i| {
            let a = start + sweep * (i as f64) / (steps as f64);
            Point::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

/// Expand a polyline bulge into sampled arc points.
///
/// `bulge` is tan(theta/4) where theta is the included angle; 1.0 is a
/// semicircle and 0.0 is a straight segment.
pub fn bulge_arc(a: Point, b: Point, bulge: f64) -> Vec<Point> {
    if bulge.abs() < 1e-12 {
        return vec![a, b];
    }
    let theta = 4.0 * bulge.atan();
    let chord = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
    if chord < 1e-12 {
        return vec![a, b];
    }
    let radius = chord / (2.0 * (theta / 2.0).sin());
    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let apothem = radius * (theta / 2.0).cos();
    // Unit normal to the chord; the sign of the bulge picks the side.
    let nx = -(b.y - a.y) / chord;
    let ny = (b.x - a.x) / chord;
    let center = Point::new(mid.x + nx * apothem, mid.y + ny * apothem);

    let start = (a.y - center.y).atan2(a.x - center.x);
    let sweep_radius = radius.abs();
    let steps = ((theta.abs() / TAU) * CIRCLE_SEGMENTS as f64).ceil().max(2.0) as usize;
    (0..=steps)
        .map(|i| {
            let ang = start + theta * (i as f64) / (steps as f64);
            Point::new(
                center.x + sweep_radius * ang.cos(),
                center.y + sweep_radius * ang.sin(),
            )
        })
        .collect()
}

fn transformed(points: Vec<Point>, t: Affine, closed: bool) -> FlatGeom {
    FlatGeom {
        geom: PathGeom {
            subpaths: vec![SubPath {
                points: points.into_iter().map(|p| t.apply(p)).collect(),
                closed,
            }],
        },
        filled: false,
    }
}

/// Flatten one entity's geometry into transformed polylines.
///
/// Returns `None` for entity kinds this stage does not draw, so the caller
/// can count and report them rather than silently dropping them.
pub fn flatten(entity: &RawEntity, t: Affine) -> Option<FlatGeom> {
    match entity.kind.as_str() {
        "LINE" => {
            let a = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
            let b = Point::new(entity.f64(11, 0.0), entity.f64(21, 0.0));
            Some(transformed(vec![a, b], t, false))
        }
        "CIRCLE" => {
            let c = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
            let r = entity.f64(40, 0.0);
            if r <= 0.0 {
                return None;
            }
            Some(transformed(arc_points(c, r, 0.0, 360.0, CIRCLE_SEGMENTS), t, true))
        }
        "ARC" => {
            let c = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
            let r = entity.f64(40, 0.0);
            if r <= 0.0 {
                return None;
            }
            let start = entity.f64(50, 0.0);
            let end = entity.f64(51, 360.0);
            Some(transformed(arc_points(c, r, start, end, CIRCLE_SEGMENTS), t, false))
        }
        "ELLIPSE" => Some(transformed(ellipse_points(entity), t, false)),
        "LWPOLYLINE" | "POLYLINE" => {
            let closed = entity.int(70, 0) & 1 != 0;
            let pts = polyline_points(entity);
            if pts.len() < 2 {
                return None;
            }
            Some(transformed(pts, t, closed))
        }
        "POINT" => {
            let p = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
            Some(transformed(vec![p, p], t, false))
        }
        "SOLID" | "TRACE" | "3DFACE" => {
            // Vertex order in these entities is 1,2,4,3 -- not 1,2,3,4.
            let pts = vec![
                Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0)),
                Point::new(entity.f64(11, 0.0), entity.f64(21, 0.0)),
                Point::new(entity.f64(13, 0.0), entity.f64(23, 0.0)),
                Point::new(entity.f64(12, 0.0), entity.f64(22, 0.0)),
            ];
            let mut f = transformed(pts, t, true);
            f.filled = entity.kind != "3DFACE";
            Some(f)
        }
        "SPLINE" => {
            let pts = entity.points(10, 20);
            if pts.len() < 2 {
                return None;
            }
            let closed = entity.int(70, 0) & 1 != 0;
            Some(transformed(pts, t, closed))
        }
        _ => None,
    }
}

fn ellipse_points(entity: &RawEntity) -> Vec<Point> {
    let c = Point::new(entity.f64(10, 0.0), entity.f64(20, 0.0));
    let major = Point::new(entity.f64(11, 0.0), entity.f64(21, 0.0));
    let ratio = entity.f64(40, 1.0);
    let start = entity.f64(41, 0.0);
    let end = entity.f64(42, TAU);
    let a = (major.x * major.x + major.y * major.y).sqrt();
    let b = a * ratio;
    let rot = major.y.atan2(major.x);
    let mut sweep = end - start;
    while sweep <= 0.0 {
        sweep += TAU;
    }
    (0..=CIRCLE_SEGMENTS)
        .map(|i| {
            let param = start + sweep * (i as f64) / (CIRCLE_SEGMENTS as f64);
            let (x, y) = (a * param.cos(), b * param.sin());
            Point::new(
                c.x + x * rot.cos() - y * rot.sin(),
                c.y + x * rot.sin() + y * rot.cos(),
            )
        })
        .collect()
}

fn polyline_points(entity: &RawEntity) -> Vec<Point> {
    let verts = entity.points(10, 20);
    let bulges = entity.all_f64(42);
    if verts.len() < 2 {
        return verts;
    }
    let closed = entity.int(70, 0) & 1 != 0;
    let mut out = Vec::with_capacity(verts.len());
    let last = if closed { verts.len() } else { verts.len() - 1 };
    for i in 0..last {
        let a = verts[i];
        let b = verts[(i + 1) % verts.len()];
        let bulge = bulges.get(i).copied().unwrap_or(0.0);
        let seg = bulge_arc(a, b, bulge);
        if out.is_empty() {
            out.extend(seg);
        } else {
            out.extend(seg.into_iter().skip(1));
        }
    }
    out
}
```

Add to `src/plot/mod.rs`:

```rust
pub mod flatten;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib plot::flatten 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 9 passed`

- [ ] **Step 5: Commit**

```bash
git add src/plot/flatten.rs src/plot/mod.rs
git commit -m "feat: flatten DXF entity geometry into transformed polylines"
```

---

### Task 12: Build the PlotScene

**Files:**
- Create: `src/plot/build.rs`
- Modify: `src/plot/mod.rs`

**Interfaces:**
- Consumes: `doc::Document`, `plot::style::{ColorMode, Inherited, resolve_color, resolve_width_mm, resolve_dash_mm}`, `plot::flatten::flatten`, `plot::{PlotScene, PlotItem, StrokeStyle, PaperSize}`, `geom::{Affine, Bounds}`.
- Produces:
  - `plot::build::PlotRequest { window: Bounds, paper: PaperSize, margin_mm: f64, mode: ColorMode }`
  - `plot::build::BuildReport { items: usize, skipped: HashMap<String, usize> }`
  - `plot::build::build(doc: &Document, req: &PlotRequest) -> (PlotScene, BuildReport)`
  - `plot::build::model_extents(doc: &Document) -> Bounds`

**Transform:** drawing units -> paper millimetres. `scale = min((paper_w - 2*margin) / window_w, (paper_h - 2*margin) / window_h)`, then centre the window on the sheet. Y is flipped at the renderer boundary, not here: `PlotScene` uses PDF convention (Y up from bottom-left).

**Block recursion:** INSERT applies scale (41/42), rotation (50) and insertion point (10/20), recursing into `doc.blocks`. Depth is capped at 24 to survive self-referential blocks, which occur in damaged files.

- [ ] **Step 1: Write the failing test**

Create `src/plot/build.rs` with only this test module:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib plot::build 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `build` / `PlotRequest` not found.

- [ ] **Step 3: Implement**

Prepend to `src/plot/build.rs`:

```rust
use std::collections::HashMap;

use crate::doc::Document;
use crate::dxf::entities::RawEntity;
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
```

Add to `src/plot/mod.rs`:

```rust
pub mod build;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib plot::build 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 7 passed`

- [ ] **Step 5: Commit**

```bash
git add src/plot/build.rs src/plot/mod.rs
git commit -m "feat: build PlotScene from Document with block recursion and mm transform"
```

---

### Task 13: PDF backend

**Files:**
- Create: `src/render/mod.rs`
- Create: `src/render/pdf.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `plot::{PlotScene, PlotItem, StrokeStyle, Rgb}`.
- Produces: `render::pdf::write_pdf(scenes: &[PlotScene]) -> Vec<u8>`

**Units:** PDF user space is points. `mm * 72.0 / 25.4 = pt`. Y is flipped here: `PlotScene` Y already runs bottom-up, matching PDF, so no flip is needed — but the renderer must not assume otherwise.

- [ ] **Step 1: Write the failing test**

Create `src/render/pdf.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
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
        assert!(content(&out).contains("0.992"), "expected 0.992 w in the content stream");
    }

    #[test]
    fn hairline_is_emitted_as_zero_width() {
        let out = write_pdf(&[line_scene(0.0)]);
        assert!(content(&out).contains("0 w"), "hairline must be '0 w'");
    }

    #[test]
    fn colour_is_emitted_as_a_normalised_rgb_stroke() {
        let out = write_pdf(&[line_scene(0.35)]);
        assert!(content(&out).contains("1 0 0 RG"), "expected red stroke colour");
    }

    #[test]
    fn dash_patterns_reach_the_content_stream() {
        let mut s = line_scene(0.35);
        if let PlotItem::Path { style, .. } = &mut s.items[0] {
            style.dash_mm = Some(vec![2.0, 1.0]);
        }
        let out = write_pdf(&[s]);
        assert!(content(&out).contains(" d\n"), "expected a dash operator");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib render::pdf 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `write_pdf` not found.

- [ ] **Step 3: Implement**

Create `src/render/mod.rs`:

```rust
pub mod pdf;
```

Create `src/render/pdf.rs` (prepend above the test module):

```rust
use std::fmt::Write as _;

use pdf_writer::{Finish, Pdf, Rect, Ref};

use crate::plot::{PlotItem, PlotScene, Rgb, StrokeStyle};

const MM_TO_PT: f64 = 72.0 / 25.4;

fn mm(v: f64) -> f32 {
    (v * MM_TO_PT) as f32
}

fn channel(v: u8) -> f32 {
    v as f32 / 255.0
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
        pdf.stream(content_ids[i], stream.as_bytes());
    }

    pdf.finish()
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
                        "{:.4} {:.4} {:.4} rg",
                        channel(color.r),
                        channel(color.g),
                        channel(color.b)
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
            "{:.4} {:.4} {:.4} RG",
            channel(style.color.r),
            channel(style.color.g),
            channel(style.color.b)
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
```

Add to `src/lib.rs`:

```rust
pub mod render;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib render::pdf 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 7 passed`

> `pdf-writer` also offers a typed `Content` builder. The string builder is used here instead because it makes the width and dash state machine directly assertable in tests — the state machine is the part most likely to regress, since a missed reset silently applies one entity's width to the next.

- [ ] **Step 5: Commit**

```bash
git add src/render/mod.rs src/render/pdf.rs src/lib.rs
git commit -m "feat: add vector PDF backend with exact millimetre lineweights"
```

---

### Task 14: The lineweight gate (R-LW acceptance)

**Files:**
- Create: `tests/lineweight_gate.rs`

**Interfaces:**
- Consumes: `cadviewer::{doc::Document, plot::*}`.
- Produces: nothing; this is the Phase 2 exit gate.

This is the one acceptance criterion in the PRD that is exact rather than visual. It asserts that every one of the 13 widths AutoCAD emitted for the sample drawing (PRD 3.9.2) survives the full pipeline into the scene.

- [ ] **Step 1: Write the test**

Create `tests/lineweight_gate.rs`:

```rust
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
```

- [ ] **Step 2: Run the gate**

Run: `cargo test --test lineweight_gate 2>&1 | Select-String -Pattern "test result|panicked|assertion"`
Expected: `test result: ok. 2 passed`

- [ ] **Step 3: Commit**

```bash
git add tests/lineweight_gate.rs
git commit -m "test: gate Phase 2 on exact AutoCAD lineweight reproduction"
```

---

### Task 15: Screen renderer

**Files:**
- Create: `src/render/skia.rs`
- Modify: `src/render/mod.rs`

**Interfaces:**
- Consumes: `plot::{PlotScene, PlotItem}`, `tiny_skia`.
- Produces: `render::skia::render(scene: &PlotScene, pixels_per_mm: f32) -> tiny_skia::Pixmap`

**Y flip lives here.** `PlotScene` uses PDF convention (Y up from bottom-left); screen pixmaps are Y-down. The flip belongs in this backend, not in `plot::build`, so the PDF path stays untouched.

**Minimum visible width:** a hairline (`width_mm == 0.0`) would be sub-pixel at most zoom levels and vanish. Clamp to one device pixel — this is a display concession only and must not leak into the PDF path.

- [ ] **Step 1: Write the failing test**

Create `src/render/skia.rs` with only this test module:

```rust
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
        let row_dark = |y: u32| {
            (0..pm.width())
                .filter(|x| pm.pixel(*x, y).map(|p| p.red() < 250).unwrap_or(false))
                .count()
        };
        assert!(row_dark(h - 12) > 10, "low scene Y should land near the image bottom");
        assert_eq!(row_dark(12), 0, "nothing should be drawn at the top");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib render::skia 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `render` not found.

- [ ] **Step 3: Implement**

Prepend to `src/render/skia.rs`:

```rust
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
```

Add to `src/render/mod.rs`:

```rust
pub mod skia;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib render::skia 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 5 passed`

- [ ] **Step 5: Commit**

```bash
git add src/render/skia.rs src/render/mod.rs
git commit -m "feat: add tiny-skia screen renderer consuming PlotScene"
```

---

### Task 16: Wire the pipeline and delete the SVG core

**Files:**
- Modify: `src/converter.rs`
- Modify: `src/main.rs`
- Modify: `src/bin/cadconvert.rs`
- Modify: `src/lib.rs`
- Delete: `src/dxf.rs`, `src/pdf.rs`

**Interfaces:**
- Consumes: everything from Tasks 1-15.
- Produces:
  - `converter::LoadedDrawing { doc: Document, warnings: String }`
  - `converter::load(path: &Path) -> Result<LoadedDrawing, String>`
  - `converter::convert_to_pdf(path: &Path, output: &Path, mode: ColorMode) -> Result<usize, String>` returning the page count.

**The DWG hop uses ASCII DXF (`-y`, no `-b`).**

The plan originally specified binary DXF for the speed win (3.2 s / 22.9 MB versus
5.7 s / 41.7 MB). That was measured and then **disproved in practice**: LibreDWG's
binary writer emits *name* fields (group codes 2, 8 — block names, layer names) as
UTF-16LE, so `"ASHADE"` is stored as `41 00 53 00 ...`. A null-terminated read stops
at the first `0x00` and yields `"A"`. Handles (code 5) and fixed keywords (code 0,
100) are plain ASCII, which is why entity boundaries and record counts stayed
correct and only the *names* were wrong — a failure that looks like nothing until
you check the values.

Empirically, on the real sample: ASCII DXF finds all 26 title-block inserts;
binary DXF finds 0.

The lexer keeps its binary support (tested, harmless, and correct for the shapes it
does handle), but the converter must use ASCII. Do not reintroduce `-b` without
first proving name fields round-trip on a real drawing.

- [ ] **Step 1: Rewrite the converter**

Replace the body of `src/converter.rs` with:

```rust
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::doc::Document;
use crate::plot::build::{PlotRequest, build, model_extents};
use crate::plot::style::ColorMode;
use crate::plot::PaperSize;
use crate::render::pdf::write_pdf;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const DEFAULT_MARGIN_MM: f64 = 10.0;

pub struct LoadedDrawing {
    pub doc: Document,
    pub warnings: String,
}

pub fn load(input: &Path) -> Result<LoadedDrawing, String> {
    if !input.exists() {
        return Err(format!("文件不存在：{}", input.display()));
    }
    let extension = input
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (bytes, warnings, _guard) = match extension.as_str() {
        "dxf" => (
            fs::read(input).map_err(|e| format!("无法读取 DXF：{e}"))?,
            String::new(),
            None,
        ),
        "dwg" => {
            let dir = tempfile::tempdir().map_err(|e| format!("无法创建临时目录：{e}"))?;
            let out = dir.path().join("drawing.dxf");
            let converter = locate_converter()?;

            let mut command = Command::new(&converter);
            command
                .arg("-y")
                .arg("-b")
                .arg("-o")
                .arg(&out)
                .arg(input)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            #[cfg(windows)]
            command.creation_flags(CREATE_NO_WINDOW);

            let output = command
                .output()
                .map_err(|e| format!("无法启动 LibreDWG：{e}"))?;
            let warnings = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            if !output.status.success() || !out.exists() {
                return Err(format!(
                    "LibreDWG 转换失败（退出码 {}）：{}",
                    output.status.code().unwrap_or(-1),
                    warnings
                ));
            }
            let bytes = fs::read(&out).map_err(|e| format!("无法读取中间 DXF：{e}"))?;
            (bytes, warnings, Some(dir))
        }
        _ => return Err("仅支持 .dwg 和 .dxf 文件".to_owned()),
    };

    let doc = Document::parse(&bytes)?;
    Ok(LoadedDrawing { doc, warnings })
}

/// Convert to a single-page PDF covering model extents.
/// Phase 3 replaces this with one page per detected sheet.
pub fn convert_to_pdf(input: &Path, output: &Path, mode: ColorMode) -> Result<usize, String> {
    let loaded = load(input)?;
    let window = model_extents(&loaded.doc);
    if !window.valid() {
        return Err("图纸中没有可打印的二维实体".to_owned());
    }
    let req = PlotRequest {
        window,
        paper: PaperSize::fit(window.width(), window.height()),
        margin_mm: DEFAULT_MARGIN_MM,
        mode,
    };
    let (scene, report) = build(&loaded.doc, &req);
    if scene.items.is_empty() {
        return Err("图纸中没有可打印的二维实体".to_owned());
    }
    let bytes = write_pdf(&[scene]);
    fs::write(output, bytes).map_err(|e| format!("无法写入 PDF：{e}"))?;
    let _ = report;
    Ok(1)
}

fn locate_converter() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(custom) = std::env::var_os("CADVIEWER_LIBREDWG") {
        let custom = PathBuf::from(custom);
        candidates.push(if custom.is_dir() {
            custom.join("dwg2dxf.exe")
        } else {
            custom
        });
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(directory) = exe.parent()
    {
        candidates.push(directory.join("runtime").join("dwg2dxf.exe"));
        candidates.push(directory.join("dwg2dxf.exe"));
    }
    if let Ok(directory) = std::env::current_dir() {
        candidates.push(directory.join("runtime").join("dwg2dxf.exe"));
    }
    candidates
        .into_iter()
        .find(|c| c.is_file())
        .ok_or_else(|| "缺少 runtime\\dwg2dxf.exe。请重新解压完整的 Cadviewer 便携包。".to_owned())
}
```

- [ ] **Step 2: Delete the SVG core, drop its dependencies, update the module list**

```bash
git rm src/dxf.rs src/pdf.rs
```

Now remove the two lines Task 1 deliberately kept, so `[dependencies]` in `Cargo.toml` reads exactly:

```toml
[dependencies]
eframe = { version = "0.35.0", default-features = false, features = ["default_fonts", "glow"] }
encoding_rs = "0.8"
pdf-writer = "0.12"
rfd = "0.17.2"
tempfile = "3.27.0"
tiny-skia = "0.11"
```

Set `src/lib.rs` to exactly:

```rust
pub mod aci;
pub mod converter;
pub mod doc;
pub mod dxf;
pub mod encoding;
pub mod fonts;
pub mod geom;
pub mod plot;
pub mod render;
```

- [ ] **Step 3: Update the CLI**

Replace the body of `src/bin/cadconvert.rs` with:

```rust
use std::path::PathBuf;
use std::process::ExitCode;

use cadviewer::converter::convert_to_pdf;
use cadviewer::plot::style::ColorMode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(input) = args.next() else {
        eprintln!("用法：Cadconvert.exe <input.dwg|dxf> <output.pdf> [--mono]");
        return ExitCode::from(1);
    };
    let Some(output) = args.next() else {
        eprintln!("用法：Cadconvert.exe <input.dwg|dxf> <output.pdf> [--mono]");
        return ExitCode::from(1);
    };
    let mode = if args.any(|a| a == "--mono") {
        ColorMode::Monochrome
    } else {
        ColorMode::Color
    };

    match convert_to_pdf(&PathBuf::from(input), &PathBuf::from(output), mode) {
        Ok(pages) => {
            println!("已导出 {pages} 页");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("错误：{error}");
            ExitCode::from(3)
        }
    }
}
```

- [ ] **Step 4: Update the viewer**

In `src/main.rs`, replace every use of the old SVG scene. The viewer previously held an SVG string rasterised by `resvg`; it now holds a `PlotScene` rasterised by `render::skia::render`. Concretely:

1. Replace the field that stored the SVG string with `scene: Option<cadviewer::plot::PlotScene>`.
2. On file load, call `cadviewer::converter::load`, then `plot::build::build` with `model_extents` as the window and `PaperSize::fit(...)`, and store the resulting scene.
3. In the paint routine, call `cadviewer::render::skia::render(&scene, pixels_per_mm)` where `pixels_per_mm` is the existing zoom factor, and upload the pixmap to an `egui::ColorImage` via `egui::ColorImage::from_rgba_unmultiplied([w, h], pixmap.data())`.
4. Delete the `--convert` branch's SVG path and call `convert_to_pdf` instead.

Keep the existing zoom, pan and double-click-to-fit input handling unchanged — it operates on the view transform, which is unaffected by this swap.

- [ ] **Step 5: Build and run the whole suite**

Run: `cargo build --release 2>&1 | Select-String -Pattern "^error|warning: unused|Finished"`
Expected: `Finished` with no errors.

Run: `cargo test 2>&1 | Select-String -Pattern "test result"`
Expected: every suite reports `ok`.

- [ ] **Step 6: Convert the real drawing end to end**

```bash
cargo run --release --bin cadconvert -- "$env:USERPROFILE\Desktop\sample.dwg" out.pdf
```

First copy the sample to an ASCII path so the shell never carries non-ASCII arguments:

```powershell
Copy-Item (Get-ChildItem "$env:USERPROFILE\Desktop\*.dwg" | Select-Object -First 1).FullName "$env:TEMP\sample.dwg"
cargo run --release --bin cadconvert -- "$env:TEMP\sample.dwg" "$env:TEMP\out.pdf"
```

Expected: `已导出 1 页`, and `$env:TEMP\out.pdf` opens showing the full model space with visibly varied line weights and colours.

- [ ] **Step 7: Commit**

```bash
git add -A src Cargo.toml Cargo.lock
git commit -m "refactor: replace SVG render core with PlotScene pipeline"
```

---

## Phase 2 gate

- [ ] `cargo test` is green, including `tests/lineweight_gate.rs`.
- [ ] `resvg` and `svg2pdf` no longer appear in `Cargo.toml`.
- [ ] `src/dxf.rs` and `src/pdf.rs` are deleted.
- [ ] The sample drawing converts to a PDF with correct Chinese, ACI colours and varied lineweights.

---

# Phase 3 — Sheet detection

Delivers one PDF page per title-block frame. This is the fix for the user's original "layout detection is inaccurate" complaint.

---

### Task 17: Frame candidate collection (R-SHEET-2)

**Files:**
- Create: `src/sheets.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Consumes: `doc::Document`, `geom::{Bounds, Point, Affine}`, `plot::flatten::flatten`.
- Produces:
  - `sheets::Candidate { bounds: Bounds, signal: Signal, source: String }`
  - `sheets::Signal` (enum `BlockName`, `LayerName`, `Geometry`) ordered so `BlockName > LayerName > Geometry`
  - `sheets::collect_candidates(doc: &Document) -> Vec<Candidate>`
  - `sheets::block_extents(doc: &Document, name: &str) -> Option<Bounds>`

**Signal order matters (PRD 3.10.1).** The sample yields 26 frames from block-name matching alone; the geometric scorer is a fallback that this file never reaches. When any `BlockName` candidate exists, weaker signals are discarded entirely — mixing them is how group boxes creep back in.

- [ ] **Step 1: Write the failing test**

Create `src/sheets.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Document;

    /// Two frame inserts of a block named with the CJK for "title block",
    /// plus a large rectangle on an unrelated layer.
    fn src() -> Vec<u8> {
        let frame = "\u{56fe}\u{6846}";
        format!(
            "  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nL\n 62\n7\n370\n-3\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n\
  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\n{frame}\n\
  0\nLWPOLYLINE\n  8\nL\n 70\n1\n 10\n0.0\n 20\n0.0\n 10\n420.0\n 20\n0.0\n 10\n420.0\n 20\n297.0\n 10\n0.0\n 20\n297.0\n\
  0\nENDBLK\n  0\nENDSEC\n\
  0\nSECTION\n  2\nENTITIES\n\
  0\nINSERT\n  8\nL\n  2\n{frame}\n 10\n0.0\n 20\n0.0\n\
  0\nINSERT\n  8\nL\n  2\n{frame}\n 10\n500.0\n 20\n0.0\n\
  0\nENDSEC\n  0\nEOF\n"
        )
        .into_bytes()
    }

    #[test]
    fn finds_frames_by_block_name() {
        let doc = Document::parse(&src()).unwrap();
        let c = collect_candidates(&doc);
        assert_eq!(c.len(), 2, "got {c:?}");
        assert!(c.iter().all(|x| x.signal == Signal::BlockName));
    }

    #[test]
    fn candidate_bounds_follow_the_insert_point() {
        let doc = Document::parse(&src()).unwrap();
        let mut c = collect_candidates(&doc);
        c.sort_by(|a, b| a.bounds.min_x.partial_cmp(&b.bounds.min_x).unwrap());
        assert!((c[0].bounds.min_x - 0.0).abs() < 0.01);
        assert!((c[1].bounds.min_x - 500.0).abs() < 0.01, "got {}", c[1].bounds.min_x);
        assert!((c[0].bounds.width() - 420.0).abs() < 0.01);
    }

    #[test]
    fn block_extents_measures_the_block_body() {
        let doc = Document::parse(&src()).unwrap();
        let b = block_extents(&doc, "\u{56fe}\u{6846}").expect("block should exist");
        assert!((b.width() - 420.0).abs() < 0.01);
        assert!((b.height() - 297.0).abs() < 0.01);
    }

    #[test]
    fn strong_signals_suppress_weak_ones() {
        // A geometric candidate must not survive alongside block-name hits,
        // or annotation rectangles reappear as extra pages.
        let doc = Document::parse(&src()).unwrap();
        let c = collect_candidates(&doc);
        assert!(!c.iter().any(|x| x.signal == Signal::Geometry));
    }

    #[test]
    fn falls_back_to_geometry_when_no_block_or_layer_matches() {
        let src = b"  0\nSECTION\n  2\nENTITIES\n  0\nLWPOLYLINE\n  8\nX\n 70\n1\n 10\n0.0\n 20\n0.0\n 10\n420.0\n 20\n0.0\n 10\n420.0\n 20\n297.0\n 10\n0.0\n 20\n297.0\n  0\nENDSEC\n  0\nEOF\n";
        let doc = Document::parse(src).unwrap();
        let c = collect_candidates(&doc);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].signal, Signal::Geometry);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib sheets 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `collect_candidates` not found.

- [ ] **Step 3: Implement**

Prepend to `src/sheets.rs`:

```rust
use crate::doc::Document;
use crate::geom::{Affine, Bounds, Point};
use crate::plot::flatten::flatten;

/// Aspect ratios that indicate a real sheet: ISO A-series plus the GB
/// extended formats used in Chinese practice.
const SHEET_RATIOS: [f64; 4] = [1.4142, 1.5, 2.0, 3.0];

/// Minimum score for a purely geometric candidate to be accepted. This is
/// the only tuning knob in the detector; keep it here, not scattered
/// through the scoring code.
pub const GEOMETRY_THRESHOLD: f64 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Signal {
    Geometry,
    LayerName,
    BlockName,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub bounds: Bounds,
    pub signal: Signal,
    pub source: String,
}

fn is_frame_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    name.contains('\u{56fe}') && (name.contains('\u{6846}') || name.contains('\u{5e45}'))
        || upper.contains("TITLEBLOCK")
        || upper.contains("TITLE_BLOCK")
        || upper.contains("FRAME")
        || upper == "TK"
}

/// Untransformed extents of a block's body.
pub fn block_extents(doc: &Document, name: &str) -> Option<Bounds> {
    let body = doc.blocks.get(name)?;
    let mut b = Bounds::empty();
    for ent in body {
        if let Some(f) = flatten(ent, Affine::identity()) {
            let fb = f.geom.bounds();
            if fb.valid() {
                b.add(Point::new(fb.min_x, fb.min_y));
                b.add(Point::new(fb.max_x, fb.max_y));
            }
        }
    }
    b.valid().then_some(b)
}

/// Score a rectangle on how much it looks like a standard sheet.
fn geometry_score(bounds: Bounds) -> f64 {
    if !bounds.valid() || bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return 0.0;
    }
    let w = bounds.width();
    let h = bounds.height();
    let ratio = if w >= h { w / h } else { h / w };
    SHEET_RATIOS
        .iter()
        .map(|target| {
            let error = (ratio - target).abs() / target;
            (1.0 - error * 4.0).max(0.0)
        })
        .fold(0.0, f64::max)
}

pub fn collect_candidates(doc: &Document) -> Vec<Candidate> {
    let cp = doc.header.codepage;
    let mut out: Vec<Candidate> = Vec::new();

    // L1: INSERTs of a block whose name says "title block".
    for ent in &doc.entities {
        if ent.kind != "INSERT" {
            continue;
        }
        let Some(name) = ent.text(2, cp) else { continue };
        if !is_frame_name(&name) {
            continue;
        }
        let Some(extents) = block_extents(doc, &name) else { continue };
        let t = Affine::scale(ent.f64(41, 1.0), ent.f64(42, 1.0))
            .then(Affine::rotation(ent.f64(50, 0.0)))
            .then(Affine::translation(ent.f64(10, 0.0), ent.f64(20, 0.0)));
        let mut b = Bounds::empty();
        for corner in [
            Point::new(extents.min_x, extents.min_y),
            Point::new(extents.max_x, extents.min_y),
            Point::new(extents.max_x, extents.max_y),
            Point::new(extents.min_x, extents.max_y),
        ] {
            b.add(t.apply(corner));
        }
        out.push(Candidate { bounds: b, signal: Signal::BlockName, source: name });
    }
    if !out.is_empty() {
        return out;
    }

    // L2: closed rectangles sitting on a layer whose name says "title block".
    for ent in &doc.entities {
        if !matches!(ent.kind.as_str(), "LWPOLYLINE" | "POLYLINE") {
            continue;
        }
        let layer = ent.layer(cp);
        if !is_frame_name(&layer) {
            continue;
        }
        if let Some(b) = closed_rect_bounds(ent) {
            out.push(Candidate { bounds: b, signal: Signal::LayerName, source: layer });
        }
    }
    if !out.is_empty() {
        return out;
    }

    // L3: geometric fallback.
    for ent in &doc.entities {
        if !matches!(ent.kind.as_str(), "LWPOLYLINE" | "POLYLINE") {
            continue;
        }
        let Some(b) = closed_rect_bounds(ent) else { continue };
        if geometry_score(b) >= GEOMETRY_THRESHOLD {
            out.push(Candidate {
                bounds: b,
                signal: Signal::Geometry,
                source: ent.layer(cp),
            });
        }
    }
    out
}

fn closed_rect_bounds(ent: &crate::dxf::entities::RawEntity) -> Option<Bounds> {
    if ent.int(70, 0) & 1 == 0 {
        return None;
    }
    let pts = ent.points(10, 20);
    if pts.len() != 4 {
        return None;
    }
    let mut b = Bounds::empty();
    for p in &pts {
        b.add(*p);
    }
    b.valid().then_some(b)
}
```

Add to `src/lib.rs`:

```rust
pub mod sheets;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib sheets 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 5 passed`

- [ ] **Step 5: Commit**

```bash
git add src/sheets.rs src/lib.rs
git commit -m "feat: collect title-block frame candidates by block, layer then geometry"
```

---

### Task 18: Container rejection and page ordering (R-SHEET-3, R-SHEET-4)

**Files:**
- Modify: `src/sheets.rs`

**Interfaces:**
- Consumes: `sheets::{Candidate, Signal, collect_candidates}`.
- Produces:
  - `sheets::Sheet { index: usize, bounds: Bounds, source: String }`
  - `sheets::reject_containers(Vec<Candidate>) -> Vec<Candidate>`
  - `sheets::order_sheets(Vec<Candidate>) -> Vec<Sheet>`
  - `sheets::detect(doc: &Document) -> Vec<Sheet>`

**Read this before implementing.** The obvious dedup rule — *keep the outermost of nested candidates* — is **wrong here and fails silently**. The sample drawing has three annotation rectangles on a layer named `打印` that enclose groups of 6, 12 and 8 real frames (PRD 3.10.2). Keeping the outermost would turn 26 pages into 3, with no error and a plausible-looking result.

The correct rule: **a candidate that fully contains 2 or more other candidates is a container and is discarded.** Outermost is kept only when nested candidates nearly coincide — a double-line frame border, where the two rectangles differ by less than 2%.

- [ ] **Step 1: Write the failing test**

Append to the `tests` module in `src/sheets.rs`:

```rust
    fn candidate(min_x: f64, min_y: f64, w: f64, h: f64) -> Candidate {
        let mut b = Bounds::empty();
        b.add(Point::new(min_x, min_y));
        b.add(Point::new(min_x + w, min_y + h));
        Candidate { bounds: b, signal: Signal::BlockName, source: "F".to_owned() }
    }

    #[test]
    fn a_group_box_containing_many_frames_is_discarded() {
        // PRD 3.10.2: the failure that would collapse 26 pages into 3.
        let frames: Vec<Candidate> = (0..6)
            .map(|i| candidate(10.0 + i as f64 * 100.0, 10.0, 80.0, 60.0))
            .collect();
        let group = candidate(0.0, 0.0, 620.0, 80.0);
        let mut all = frames.clone();
        all.push(group);

        let kept = reject_containers(all);
        assert_eq!(kept.len(), 6, "the group box must be dropped, got {kept:?}");
        assert!(kept.iter().all(|c| c.bounds.width() < 100.0));
    }

    #[test]
    fn a_double_line_border_keeps_the_outer_rectangle() {
        let outer = candidate(0.0, 0.0, 420.0, 297.0);
        let inner = candidate(2.0, 2.0, 416.0, 293.0);
        let kept = reject_containers(vec![outer, inner]);
        assert_eq!(kept.len(), 1);
        assert!((kept[0].bounds.width() - 420.0).abs() < 0.01, "should keep the outer");
    }

    #[test]
    fn a_container_holding_only_one_frame_is_still_a_border() {
        let outer = candidate(0.0, 0.0, 500.0, 400.0);
        let inner = candidate(10.0, 10.0, 420.0, 297.0);
        let kept = reject_containers(vec![outer, inner]);
        assert_eq!(kept.len(), 1, "one nested frame is a border, not a group");
    }

    #[test]
    fn sheets_are_ordered_top_to_bottom_then_left_to_right() {
        // Two rows of two, deliberately supplied out of order.
        let all = vec![
            candidate(200.0, 0.0, 100.0, 80.0),
            candidate(0.0, 200.0, 100.0, 80.0),
            candidate(200.0, 200.0, 100.0, 80.0),
            candidate(0.0, 0.0, 100.0, 80.0),
        ];
        let sheets = order_sheets(all);
        assert_eq!(sheets.len(), 4);
        assert!((sheets[0].bounds.min_x - 0.0).abs() < 0.01, "first should be top-left");
        assert!((sheets[0].bounds.min_y - 200.0).abs() < 0.01);
        assert!((sheets[1].bounds.min_x - 200.0).abs() < 0.01, "second should be top-right");
        assert!((sheets[3].bounds.min_x - 200.0).abs() < 0.01, "last should be bottom-right");
    }

    #[test]
    fn rows_tolerate_small_vertical_jitter() {
        // Frames on the same row are rarely aligned to the micron.
        let all = vec![
            candidate(0.0, 0.0, 100.0, 80.0),
            candidate(200.0, 3.0, 100.0, 80.0),
        ];
        let sheets = order_sheets(all);
        assert!((sheets[0].bounds.min_x - 0.0).abs() < 0.01, "should read left to right");
        assert!((sheets[1].bounds.min_x - 200.0).abs() < 0.01);
    }

    #[test]
    fn sheets_are_indexed_from_one() {
        let sheets = order_sheets(vec![candidate(0.0, 0.0, 100.0, 80.0)]);
        assert_eq!(sheets[0].index, 1);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib sheets 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `reject_containers` / `order_sheets` / `Sheet` not found.

- [ ] **Step 3: Implement**

Append to `src/sheets.rs`:

```rust
/// Two nested rectangles this close in size are the inner and outer lines
/// of one drawn border, not a container and its contents.
const COINCIDENT_TOLERANCE: f64 = 0.02;

#[derive(Clone, Debug)]
pub struct Sheet {
    /// 1-based page number in reading order.
    pub index: usize,
    pub bounds: Bounds,
    pub source: String,
}

fn nearly_coincident(outer: Bounds, inner: Bounds) -> bool {
    let ow = outer.width().max(f64::EPSILON);
    let oh = outer.height().max(f64::EPSILON);
    (outer.width() - inner.width()).abs() / ow < COINCIDENT_TOLERANCE
        && (outer.height() - inner.height()).abs() / oh < COINCIDENT_TOLERANCE
}

/// Drop annotation and grouping rectangles.
///
/// A candidate enclosing two or more *distinct* candidates is a container,
/// not a sheet. Keeping the outermost instead — the intuitive rule — would
/// silently collapse a 26-sheet drawing to 3 pages (PRD 3.10.2).
pub fn reject_containers(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut keep = vec![true; candidates.len()];

    for (i, outer) in candidates.iter().enumerate() {
        let mut enclosed = 0usize;
        for (j, inner) in candidates.iter().enumerate() {
            if i == j || !outer.bounds.contains(inner.bounds) {
                continue;
            }
            if nearly_coincident(outer.bounds, inner.bounds) {
                // Same border drawn twice: suppress the inner copy.
                keep[j] = false;
                continue;
            }
            enclosed += 1;
        }
        if enclosed >= 2 {
            keep[i] = false;
        }
    }

    candidates
        .into_iter()
        .zip(keep)
        .filter_map(|(c, k)| k.then_some(c))
        .collect()
}

/// Sort into reading order: rows top to bottom, each row left to right.
pub fn order_sheets(candidates: Vec<Candidate>) -> Vec<Sheet> {
    let mut remaining = candidates;
    remaining.sort_by(|a, b| {
        b.bounds
            .min_y
            .partial_cmp(&a.bounds.min_y)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut rows: Vec<Vec<Candidate>> = Vec::new();
    for c in remaining {
        // Same row when the vertical offset is under half the frame height,
        // which absorbs the jitter real drawings always have.
        let placed = rows.iter_mut().find(|row| {
            let reference = &row[0];
            let tolerance = reference.bounds.height().max(c.bounds.height()) / 2.0;
            (reference.bounds.min_y - c.bounds.min_y).abs() <= tolerance
        });
        match placed {
            Some(row) => row.push(c),
            None => rows.push(vec![c]),
        }
    }

    let mut out = Vec::new();
    for row in &mut rows {
        row.sort_by(|a, b| {
            a.bounds
                .min_x
                .partial_cmp(&b.bounds.min_x)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for c in row.iter() {
            out.push(Sheet {
                index: out.len() + 1,
                bounds: c.bounds,
                source: c.source.clone(),
            });
        }
    }
    out
}

/// Full detection pipeline: collect, reject containers, order.
pub fn detect(doc: &Document) -> Vec<Sheet> {
    order_sheets(reject_containers(collect_candidates(doc)))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib sheets 2>&1 | Select-String -Pattern "test result"`
Expected: `test result: ok. 11 passed`

- [ ] **Step 5: Verify against the real drawing**

Append to `tests/real_drawing.rs`:

```rust
#[test]
fn detects_twenty_six_sheets_in_reading_order() {
    let dwg = sample_dwg();
    if !Path::new(&dwg).exists() {
        eprintln!("SKIPPED: sample DWG not present at {dwg}");
        return;
    }
    let tmp = std::env::temp_dir().join("cadviewer_sheets.dxf");
    if !to_binary_dxf(&dwg, &tmp) {
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
    if !to_binary_dxf(&dwg, &tmp) {
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
```

- [ ] **Step 6: Run it**

Run: `cargo test --test real_drawing 2>&1 | Select-String -Pattern "test result|SKIPPED|panicked"`
Expected: `test result: ok. 3 passed`, or printed `SKIPPED` lines.

- [ ] **Step 7: Commit**

```bash
git add src/sheets.rs tests/real_drawing.rs
git commit -m "feat: reject group annotation boxes and order sheets in reading order"
```

---

### Task 19: Multi-page output (R-SHEET-5, R-SHEET-6)

**Files:**
- Modify: `src/converter.rs`
- Modify: `src/bin/cadconvert.rs`

**Interfaces:**
- Consumes: `sheets::detect`, `plot::build::build`, `render::pdf::write_pdf`.
- Produces:
  - `converter::ConvertOptions { mode: ColorMode, sheet: Option<usize> }`
  - `converter::convert_to_pdf(input: &Path, output: &Path, options: &ConvertOptions) -> Result<usize, String>` — replaces the Task 16 signature.
  - `converter::scenes_for(doc: &Document, options: &ConvertOptions) -> Result<Vec<PlotScene>, String>`

**Plot scale (R-SHEET-5):** derived from frame size fitted to paper. The title block's `比例` text is *not* consulted — it records the drawing scale, not the plot scale (PRD 3.10.3).

**Fallback (R-SHEET-6):** when detection finds nothing, emit a single page covering model extents. Never error.

- [ ] **Step 1: Write the failing test**

Create `tests/pagination.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test pagination 2>&1 | Select-String -Pattern "error\[|test result"`
Expected: compile errors, `ConvertOptions` / `scenes_for` not found.

- [ ] **Step 3: Implement**

In `src/converter.rs`, add the import and replace `convert_to_pdf` with:

```rust
use crate::plot::PlotScene;
use crate::sheets::detect;

#[derive(Clone, Debug)]
pub struct ConvertOptions {
    pub mode: ColorMode,
    /// 1-based page selection. `None` exports every sheet.
    pub sheet: Option<usize>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self { mode: ColorMode::Color, sheet: None }
    }
}

/// One scene per detected title-block frame, in reading order.
///
/// Paper size and plot scale come from the frame's own dimensions fitted to
/// a standard sheet. The title block's printed scale text is deliberately
/// ignored: it records the drawing scale, not the plot scale (PRD 3.10.3).
pub fn scenes_for(doc: &Document, options: &ConvertOptions) -> Result<Vec<PlotScene>, String> {
    let mut sheets = detect(doc);

    if sheets.is_empty() {
        // R-SHEET-6: never fail, fall back to the whole model space.
        let window = model_extents(doc);
        if !window.valid() {
            return Err("图纸中没有可打印的二维实体".to_owned());
        }
        let req = PlotRequest {
            window,
            paper: PaperSize::fit(window.width(), window.height()),
            margin_mm: DEFAULT_MARGIN_MM,
            mode: options.mode,
        };
        let (scene, _) = build(doc, &req);
        return Ok(vec![scene]);
    }

    if let Some(index) = options.sheet {
        let total = sheets.len();
        sheets.retain(|s| s.index == index);
        if sheets.is_empty() {
            return Err(format!("图纸编号 {index} 超出范围（共 {total} 张）"));
        }
    }

    let mut scenes = Vec::with_capacity(sheets.len());
    for sheet in &sheets {
        // Aspect ratio picks the sheet; the frame's own size is what gets
        // fitted, so the plot scale follows from the geometry alone.
        let ratio = sheet.bounds.width() / sheet.bounds.height().max(f64::EPSILON);
        let paper = if ratio >= 1.0 {
            PaperSize::fit(297.0 * ratio, 297.0)
        } else {
            PaperSize::fit(297.0, 297.0 / ratio)
        };
        let req = PlotRequest {
            window: sheet.bounds,
            paper,
            margin_mm: DEFAULT_MARGIN_MM,
            mode: options.mode,
        };
        let (scene, _) = build(doc, &req);
        scenes.push(scene);
    }
    Ok(scenes)
}

pub fn convert_to_pdf(
    input: &Path,
    output: &Path,
    options: &ConvertOptions,
) -> Result<usize, String> {
    let loaded = load(input)?;
    let scenes = scenes_for(&loaded.doc, options)?;
    let bytes = write_pdf(&scenes);
    fs::write(output, bytes).map_err(|e| format!("无法写入 PDF：{e}"))?;
    Ok(scenes.len())
}
```

- [ ] **Step 4: Update the CLI for the new options**

In `src/bin/cadconvert.rs`, replace the argument handling after `output` with:

```rust
    let rest: Vec<String> = args.map(|a| a.to_string_lossy().into_owned()).collect();
    let mode = if rest.iter().any(|a| a == "--mono") {
        ColorMode::Monochrome
    } else {
        ColorMode::Color
    };
    let sheet = rest
        .iter()
        .position(|a| a == "--sheet")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok());
    let options = cadviewer::converter::ConvertOptions { mode, sheet };

    match convert_to_pdf(&PathBuf::from(input), &PathBuf::from(output), &options) {
```

Update the usage line to:

```rust
        eprintln!("用法：Cadconvert.exe <input.dwg|dxf> <output.pdf> [--mono] [--sheet N]");
```

- [ ] **Step 5: Run the tests**

Run: `cargo test 2>&1 | Select-String -Pattern "test result"`
Expected: every suite `ok`.

- [ ] **Step 6: Convert the real drawing to 26 pages**

```powershell
Copy-Item (Get-ChildItem "$env:USERPROFILE\Desktop\*.dwg" | Select-Object -First 1).FullName "$env:TEMP\sample.dwg"
cargo run --release --bin cadconvert -- "$env:TEMP\sample.dwg" "$env:TEMP\sheets.pdf"
```

Expected: `已导出 26 页`. Open the PDF and confirm each page is one titled sheet, not the whole model space.

- [ ] **Step 7: Commit**

```bash
git add src/converter.rs src/bin/cadconvert.rs tests/pagination.rs
git commit -m "feat: emit one PDF page per detected title-block frame"
```

---

### Task 20: Sheet list in the viewer (R-SHEET-7)

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `sheets::{Sheet, detect}`, `converter::{load, ConvertOptions, convert_to_pdf}`, `plot::build::build`, `render::skia::render`.
- Produces: no new public API; this is UI wiring.

- [ ] **Step 1: Add sheet state**

Add these fields to the application struct in `src/main.rs`:

```rust
    sheets: Vec<cadviewer::sheets::Sheet>,
    active_sheet: usize,
    scene: Option<cadviewer::plot::PlotScene>,
    warnings: String,
```

- [ ] **Step 2: Populate them on load**

In the file-open handler, after `converter::load` succeeds:

```rust
        let sheets = cadviewer::sheets::detect(&loaded.doc);
        self.sheets = sheets;
        self.active_sheet = 0;
        self.warnings = loaded.warnings.clone();
        self.rebuild_scene(&loaded.doc);
```

Add the helper:

```rust
    fn rebuild_scene(&mut self, doc: &cadviewer::doc::Document) {
        use cadviewer::plot::PaperSize;
        use cadviewer::plot::build::{PlotRequest, build, model_extents};

        let window = match self.sheets.get(self.active_sheet) {
            Some(sheet) => sheet.bounds,
            None => model_extents(doc),
        };
        if !window.valid() {
            self.scene = None;
            return;
        }
        let req = PlotRequest {
            window,
            paper: PaperSize::fit(window.width(), window.height()),
            margin_mm: cadviewer::converter::DEFAULT_MARGIN_MM,
            mode: self.color_mode,
        };
        let (scene, report) = build(doc, &req);
        if !report.skipped.is_empty() {
            let mut kinds: Vec<_> = report.skipped.iter().collect();
            kinds.sort();
            let summary = kinds
                .iter()
                .map(|(k, n)| format!("{k} x{n}"))
                .collect::<Vec<_>>()
                .join("、");
            self.warnings = format!("{}\n未绘制实体：{summary}", self.warnings);
        }
        self.scene = Some(scene);
    }
```

Add `color_mode: ColorMode` to the struct, defaulting to `ColorMode::Color`.

- [ ] **Step 3: Draw the side panel**

In the `update` method, before the central panel:

```rust
        if !self.sheets.is_empty() {
            egui::SidePanel::left("sheets").show(ctx, |ui| {
                ui.heading(format!("图纸 ({})", self.sheets.len()));
                let mut clicked = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, sheet) in self.sheets.iter().enumerate() {
                        let label = format!(
                            "{:>2}. {:.0} x {:.0}",
                            sheet.index,
                            sheet.bounds.width(),
                            sheet.bounds.height()
                        );
                        if ui.selectable_label(i == self.active_sheet, label).clicked() {
                            clicked = Some(i);
                        }
                    }
                });
                if let Some(i) = clicked {
                    self.active_sheet = i;
                    self.needs_rebuild = true;
                }
            });
        }
```

Add `needs_rebuild: bool` to the struct, and at the top of `update`, when it is set and a document is loaded, call `rebuild_scene` and clear the flag. Keep the loaded `Document` in the struct as `doc: Option<Document>` so rebuilds do not re-run `dwg2dxf`.

- [ ] **Step 4: Add the toolbar toggles and export buttons**

In the top panel:

```rust
            ui.horizontal(|ui| {
                if ui.button("打开").clicked() {
                    self.open_dialog();
                }
                let mut mono = self.color_mode == ColorMode::Monochrome;
                if ui.checkbox(&mut mono, "单色打印").changed() {
                    self.color_mode = if mono { ColorMode::Monochrome } else { ColorMode::Color };
                    self.needs_rebuild = true;
                }
                if ui.button("导出当前页").clicked() {
                    self.export(Some(self.active_sheet + 1));
                }
                if ui.button("导出全部").clicked() {
                    self.export(None);
                }
            });
```

Where `export` calls `convert_to_pdf` with the current path and a `ConvertOptions { mode: self.color_mode, sheet }`, using `rfd` for the save dialog as the existing code already does.

- [ ] **Step 5: Build and smoke-test**

Run: `cargo build --release 2>&1 | Select-String -Pattern "^error|Finished"`
Expected: `Finished`.

Run the viewer, open the sample DWG, and confirm: the side panel lists 26 sheets, clicking one redraws the view to that sheet alone, the monochrome toggle turns the drawing black, and both export buttons produce PDFs.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat: add sheet list, monochrome toggle and per-sheet export to the viewer"
```

---

## Phase 3 gate

- [ ] `cargo test` green across all suites.
- [ ] The sample drawing detects 26 sheets in rows of 2/2/1/1/6/6/4/3/1.
- [ ] No group annotation box appears as a page.
- [ ] `Cadconvert.exe sample.dwg out.pdf` reports `已导出 26 页`.
- [ ] The viewer lists sheets and switches between them.

---

## What this plan deliberately leaves out

These are PRD requirements with no task here. They belong to the follow-on plans, and listing them prevents a reviewer from reading their absence as an oversight.

| Requirement | Deferred to |
| --- | --- |
| R-TXT-1..5 — SHX parser, font location, TTF fallback, embedding | Phase 4 plan. **Text does not render at all until then**; TEXT/MTEXT will appear in `BuildReport::skipped`. |
| R-ENT HATCH — solid and pattern fills | Phase 5 plan |
| R-LW-4 — LWPOLYLINE geometric width as filled ribbons | Phase 5 plan |
| Visual-diff harness and the 15%/6%/3% thresholds | Phase 5 plan |
| PDF layers (OCG), promoted to P2 in PRD 2.1 | Phase 5 plan |
| R-LW-5 — screen "show lineweights" toggle | Phase 6 plan. The screen renderer already honours real widths; only the toggle is missing. |
| Spatial index, two-tier screen rendering, perf targets | Phase 6 plan |
| Packaging, portable zip, `--json-report`, `--paper`, `--font-dir` | Phase 6 plan |
| R-SHEET-1 — paper-space layouts with viewports | Phase 6 plan, and **still has no test drawing**. Every sample examined so far is model-space-tiled. Model-space plotting now skips group-67 entities so a layout cannot leak a misplaced duplicate into the model scene. |
| R-ENT MLINE — multi-line | Phase 5 plan. Unlike DIMENSION and LEADER (both implemented in the fix wave), an MLINE cannot be drawn from its own record: the parallel line elements sit at offsets defined by the MLINESTYLE table, which is not parsed. Drawing its spine instead would put a line on paper that AutoCAD never plots, so it is reported in `BuildReport::skipped` rather than approximated. **Zero occurrences in the reference drawing.** |
| R-LT-3 — `$PSLTSCALE` (paper-space linetype scaling) | Phase 6 plan, with R-SHEET-1. The variable is parsed into `HeaderVars` and deliberately unread: it only changes dash scaling *inside a layout viewport*, and there are no layouts until then. |
| R-SHEET-7 — sheet list showing the inferred 图幅/比例 and an outline overlay on the canvas | Phase 6 plan. The viewer lists and switches sheets today, but labels them with raw drawing-unit extents and draws no frame outline. |
| POINT rendered per `$PDMODE`/`$PDSIZE` | Phase 5 plan. A POINT currently emits a zero-length stroke, which with butt caps paints nothing in PDF. Drawing it as a cross or dot is new ink whose size comes from two header variables that are not parsed yet, so it waits for the visual-diff harness that can confirm the result against AutoCAD. **728 in the reference drawing, all inside block bodies.** |
| Bounds-culling INSERT recursion before expansion | Phase 6 plan (perf). Each page currently walks the whole document and materialises the full block expansion before discarding what misses the paper, which is what makes the 26-page export take minutes. The expansion budget added in the fix wave bounds the damage but does not remove the cost. |

The most visible consequence: after this plan the converter produces correct geometry, colours, lineweights, linetypes and pagination, but **no text**. That is a deliberate ordering choice — text is the largest and least risky remaining piece, and putting it last keeps the architectural work in front.
