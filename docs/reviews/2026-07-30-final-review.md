# Final whole-branch review — `plot-pipeline`

Reviewer: final review agent
Range: `2328acf..959fdca` (29 commits, 30 files, +4367/−2442)
Method: read all 4,473 lines of `src/` (the diff lacks context in several
places), plus `prd.md` §5–7, the plan, and the ledger. Ran `cargo test
--offline` (110 tests green, `real_drawing` really ran — 15.9 s, the sample
DWG is present) and `cargo clippy --offline --all-targets` (**red**). Every
behavioural claim below marked "proven" was reproduced against the real crate
from a scratchpad probe binary; no repository file was modified.

---

## Verdict: **CHANGES REQUESTED**

The architecture is right and the headline claims hold up. I specifically
re-verified the four things the brief flagged, and all four are **correct**:

- `plot/style.rs` — `370 = 0` is genuinely kept distinct from "absent". The
  raw `i16` is threaded through rather than an `Option`, the absent-code
  default is `LW_BYLAYER` (−1) not `0`, and `hundredths_to_mm` maps `0 =>
  HAIRLINE_MM` before the `v > 0` arm. Nothing conflates them.
- `plot/build.rs` — lineweight is **not** scaled by the plot transform.
  `resolve_width_mm` does not even receive `scale`; only `resolve_dash_mm`
  does, which is what R-LT-2 requires. `lineweight_gate.rs` proves the
  property across a 1000× extent change.
- `plot/flatten.rs` — `bulge_arc` is **not** mirrored. Hand-verified for the
  semicircle (chord (0,0)→(10,0), bulge 1.0 → apex y = **+5**, i.e. CCW as
  DXF requires) and the quarter-circle (apex 2.071 = r − apothem). The old
  `src/dxf.rs` sign error is genuinely fixed.
- `render/skia.rs` vs `render/pdf.rs` — the Y flip lives in `skia.rs:70-77`
  only. `pdf.rs` contains no flip, correct for PDF's y-up user space.
- `render/pdf.rs` graphics state — no `q`/`Q`, so state is global and the
  three trackers are sound. The first item always emits colour, width and
  dash because all three trackers start `None`, so the initial state is
  always established. I could not find a missed reset.

What blocks the merge is a different set of problems: two silent-failure
defects, a list of inheritance and entity-coverage gaps that the reference
drawing does exercise, and a red linter that the ledger mislabels.

**Findings: 2 Critical, 10 Important, 16 Minor.**

---

## Critical

### C1. Unbounded/infinite loop on an out-of-range ARC or ELLIPSE angle — the app hangs with no error
`src/plot/flatten.rs:30-32` (`arc_points`) and `src/plot/flatten.rs:176-178`
(`ellipse_points`)

```rust
while sweep <= 0.0 {
    sweep += TAU;
}
```

This parses untrusted third-party files. `lex_ascii` builds F64 values with
`parse_ascii(raw).unwrap_or(0.0)`, and Rust's `f64` parser returns
`inf` — not `Err` — for an overflowing literal. Proven:

```
"1e400" -> inf     "inf" -> inf     "1e300" -> 1e300
sweep normalisation with start_deg = 1e300:
  NOT TERMINATED after 50,000,001 iterations
```

**Failure scenario.** A corrupt or hostile DXF containing
`  0\nARC\n 50\n1e400\n` (or any absurd finite angle, or a literal `inf`)
makes `arc_points` spin forever. With `inf` the loop can never terminate at
all; with `1e300` it needs ~1.6×10²⁹⁸ iterations. This fires in
`model_extents` — i.e. **before anything is drawn** — so the GUI hangs on the
loading spinner and `Cadconvert.exe` hangs with no output and no exit code.
`panic = "abort"` in the release profile means there is not even a message.
No test covers it: every fixture uses sane angles.

**Fix.** Reject non-finite inputs and normalise arithmetically instead of by
accumulation:

```rust
if !start_deg.is_finite() || !end_deg.is_finite() { return Vec::new(); }
let mut sweep = end_deg.to_radians() - start.to_radians();
sweep = sweep.rem_euclid(TAU);
if sweep <= 0.0 { sweep = TAU; }
```

Same shape in `ellipse_points`. Consider also clamping `Kind::F64` at the
lexer so a single guard covers every consumer.

### C2. Two coincident frame candidates annihilate each other — pages vanish silently
`src/sheets.rs:180-199` (`reject_containers`)

`Bounds::contains` uses `<=`/`>=`, so two *identical* candidates each contain
the other. The loop then runs twice: pass `i=0` marks `keep[1] = false` (via
the `nearly_coincident` branch), and pass `i=1` marks `keep[0] = false`.
Both are dropped. Proven:

```
B: two identical frame candidates -> 0 kept (expected 1)
```

**Failure scenario.** A frame block inserted twice at the same point —
ordinary copy-paste-in-place, or a frame duplicated on a second layer —
removes **both** copies. The page disappears from the PDF with no warning,
no report entry, and a page count that simply reads one lower. This is
exactly the silent catastrophic class R-SHEET-3 was written to prevent, and
the existing `a_double_line_border_keeps_the_outer_rectangle` test cannot
catch it because its two rectangles are strictly nested, not coincident.

**Fix.** Make the coincident-pair rule asymmetric and idempotent — suppress
only the *strictly smaller* of a coincident pair, and break ties on index so
one of two identical candidates always survives:

```rust
if nearly_coincident(outer.bounds, inner.bounds) {
    let outer_bigger = outer.bounds.width() * outer.bounds.height()
                     > inner.bounds.width() * inner.bounds.height();
    if outer_bigger || (!outer_bigger && i < j) {
        keep[j] = false;
    }
    continue;
}
```

Add a regression test with two byte-identical candidates asserting exactly
one survivor.

---

## Important

### I1. DIMENSION entities are never drawn — 74 of them in the reference drawing
`src/plot/build.rs:98-121`, `src/plot/flatten.rs:103-163`

`flatten` has no `DIMENSION` arm and `emit` expands only `kind == "INSERT"`.
A DIMENSION carries the name of its anonymous geometry block in group code 2
and must be expanded exactly like an INSERT. Measured against the reference
DXF:

```
model space: kind:DIMENSION: 74
```

**Failure scenario.** Every dimension line, extension line, arrowhead and
tick on all 26 pages is absent from the exported PDF. This is not masked by
the text phase: the *linework* is missing too, and it is linework AutoCAD's
reference PDF contains.

This is **not** in the plan's "What this plan deliberately leaves out"
table. That table covers R-TXT, HATCH, R-LW-4, the visual-diff harness, PDF
layers, R-LW-5, perf, packaging and R-SHEET-1 — DIMENSION, LEADER, MLINE and
MINSERT (all named in PRD §5.7 R-ENT) appear nowhere. Either implement the
DIMENSION-block expansion (it is ~6 lines: treat `DIMENSION` like `INSERT`
with an identity local transform) or add these four to the deferred table
before merge so their absence is a recorded decision rather than an
oversight.

### I2. ByBlock lineweight and linetype inherit the INSERT's *raw* group code, not its resolved value
`src/plot/build.rs:107` and `:109-111`

```rust
lineweight: ent.int(370, inherited.lineweight as i32) as i16,
linetype: ent.text(6, cp).filter(|s| !s.eq_ignore_ascii_case("BYLAYER") && ...)
```

Colour is resolved properly one line above (`resolve_color(ent, layer,
inherited.color, req.mode)`), but lineweight and linetype pass the INSERT's
*unresolved* code down. When the INSERT is itself ByLayer (−1), the child's
ByBlock lookup sees −1, fails the `inherited_lw >= 0` test in
`style.rs:132-139`, and falls back to the **child entity's own layer** rather
than the INSERT's layer. Proven:

```
C: ByBlock width = 0.25 mm   (AutoCAD: 1.00 mm, from the INSERT's layer)
```

**Failure scenario.** A title-block or symbol block whose contents are drawn
"ByBlock" — the standard way to author reusable blocks — plots at the
fallback 0.25 mm instead of the width its INSERT's layer specifies. Silent,
and invisible to the current tests because `byblock_takes_the_inherited_
lineweight` passes an already-resolved `50` directly to `resolve_width_mm`,
bypassing the `build.rs` code that computes it.

**Fix.** Resolve before descending, exactly as colour does:

```rust
lineweight: resolve_raw_width(ent, layer, inherited.lineweight, doc.header.celweight),
linetype:   resolve_linetype_name(ent, layer, &inherited.linetype, cp),
```

(both need a small `pub(crate)` extraction from `style.rs`). Then add a
build-level test that drives the inheritance through a real INSERT.

### I3. Lineweight −3 ("Default") consults the layer before `$CELWEIGHT`, contradicting R-LW-1
`src/plot/style.rs:140-143`

R-LW-1 states the chain as `370 → ByLayer(−1) 取图层 → ByBlock(−2) 从 INSERT
继承 → 默认(−3) 取 $CELWEIGHT → 兜底 0.25 mm`. The `LW_DEFAULT` arm inserts a
layer lookup that the spec does not have — and that AutoCAD does not have
either (entity lineweight "Default" resolves to the `LWDEFAULT` system
variable, not to the layer). Proven:

```
D: 370=-3 width = 1.00 mm   (PRD R-LW-1: $CELWEIGHT, then 0.25)
```

**Failure scenario.** An entity explicitly set to "Default" sitting on a
1.00 mm layer plots at 1.00 mm instead of 0.25 mm. Note the two existing
tests (`default_falls_through_layer_to_celweight`,
`default_falls_all_the_way_to_the_fallback_width`) both use a layer whose
lineweight is −3, so neither can distinguish the two chains.

**Caveat before changing this.** Because the reference drawing renders
correctly today, it is possible dwg2dxf emits `370 = -3` where AutoCAD means
ByLayer, in which case the current behaviour is accidentally right and the
spec-conformant version would flatten every width to 0.25 mm. Do not fix
blind — dump the 370 distribution from the reference DXF and compare the
resulting width histogram against the 13-bucket reference in PRD 3.9.2 first.
Whichever way it resolves, record the decision: right now code and spec
disagree with nothing explaining why.

### I4. `reject_containers` uses `enclosed >= 1`; R-SHEET-3 says "2 个及以上"
`src/sheets.rs:196`

The PRD is unambiguous: *候选集内若某候选 A 完整包含 **2 个及以上** 其他图框级
候选，则 A 是分组框*, with the sole exception of near-coincident nesting. The
implementation discards A when it encloses **one**. The ledger records this
as plan bug #6 (the plan's prose said `>= 2` while the plan's own test
required `>= 1`) and accepted `>= 1` as "conservative in the dangerous
direction" — but that reasoning is one-sided. Dropping the outer candidate is
equally destructive: you lose a real sheet *and* gain a bogus page from
whatever was inside it.

**Failure scenario.** The L3 geometric fallback (`sheets.rs:120-133`) accepts
any closed 4-vertex rectangle scoring ≥ 0.75 — an A-series aspect ratio and
nothing more. A title-block frame containing a detail-view box or a legend
box of roughly A-series proportions causes the *frame* to be discarded and
the *inner box* to become the page. On a drawing with no block-name or
layer-name signal this silently produces the wrong page set.

The test `a_container_holding_only_one_frame_is_still_a_border`
(`sheets.rs:355-360`) encodes the non-spec rule, so it will need updating
together with the code.

**Fix.** Follow the PRD (`enclosed >= 2`), or — if the `>= 1` behaviour is
genuinely wanted for L1/L2 — scope it: `>= 1` only when both candidates carry
a `BlockName`/`LayerName` signal (frames never nest), `>= 2` on the L3
geometric path. Either way the PRD text or the code has to move; they cannot
both stand.

### I5. Block base points are ignored, so any block not authored at its own origin is offset
`src/dxf/entities.rs:109-113` (`read_blocks` reads only group 2 from the
`BLOCK` record) and `src/plot/build.rs:113-116`

The `BLOCK` record's group 10/20/30 base point defines which point of the
block's coordinate system lands on the INSERT point. The correct transform is
`translate(−base) → scale → rotate → translate(insert)`; the implementation
omits the first term. `sheets.rs:84-86` repeats the same omission for frame
candidates.

**Failure scenario.** Every entity of a block whose base point is not (0,0)
is displaced by exactly that vector. For a title-block frame this shifts the
detected page window, so the exported page is cropped off-centre. The
reference drawing evidently uses origin-based blocks (the visual check
passed), so this is latent here and will surface on the next file.

**Fix.** Store the base point in `read_blocks` (return
`HashMap<String, (Point, Vec<RawEntity>)>` or a small `BlockRecord`), and
prepend `Affine::translation(-base.x, -base.y)` in both `build.rs` and
`sheets.rs`.

### I6. `MAX_BLOCK_DEPTH` bounds recursion depth but not work — a 260-byte file expands to 2²⁴ items
`src/plot/build.rs:12-13`, `:93-95`

The depth cap is 24 and the branching factor is unbounded. Measured growth
with a controlled chain (each block holding a LINE plus two INSERTs of the
next):

```
depth  8:  1278-byte input ->     255 items in 2.2 ms
depth 14:  2220-byte input ->  16,383 items in 79 ms
depth 18:  2852-byte input -> 262,143 items in 2.0 s
```

A self-referential block with two self-INSERTs reaches 2²⁴ = **16,777,216**
items — roughly 64× the depth-18 case, i.e. minutes of CPU and, when the
geometry lands inside the paper, well over a gigabyte of `PlotItem`s, from an
input smaller than this paragraph. Self-referential blocks do occur in
damaged files; the comment at `build.rs:12` shows the author anticipated
them, but the guard chosen does not bound the blast radius.

**Fix.** Add an item/expansion budget alongside the depth cap — e.g. carry a
`&mut usize` counter, bail out past a few million, and record the abort in
`BuildReport` so the user is told the drawing was truncated rather than
silently getting a partial page. The bounds-cull fix in I7 also removes most
of the exposure.

### I7. INSERT recursion is never bounds-culled — the 5m45s diagnosis is confirmed
`src/converter.rs:163-180`, `src/plot/build.rs:98-121` vs `:128-138`

**Confirmed as diagnosed in the ledger, and I can add the mechanism
precisely.** `scenes_for` calls `build(doc, &req)` once per sheet
(`converter.rs:178`, inside `for sheet in &sheets`). Inside `build`, `emit`
descends into every INSERT unconditionally — the only cull
(`build.rs:130-138`) runs *after* `flatten`, on leaf geometry. So each of the
26 pages walks the entire document and materialises the full ~3.8 M-item
expansion before discarding ~96 % of it. Cost is O(sheets × full expansion)
where it should be ~O(entities). The reference drawing has 1,512 model-space
INSERTs over 1,884 more inside block bodies, so the multiplier is real.

Per the brief I have not fixed this. The fix that matters: compute the
INSERT's transformed block extents (`sheets::block_extents` already does the
untransformed half) and return early when they miss the paper, *before*
recursing. That is also the cheapest mitigation for I6.

### I8. No clip to the printable area — neighbouring sheets bleed into the margin
`src/render/pdf.rs:85-120`, `src/plot/build.rs:130-138`

The content stream contains no clip path (`re W n`), and the cull drops only
entities lying *entirely* off the paper. An entity that straddles the frame
boundary is emitted in full and drawn across the whole media box.

**Failure scenario.** On a tiled model space — which is exactly this
project's primary drawing — geometry belonging to the sheet next door is
drawn into the 10 mm margin of the current page wherever an entity crosses
the frame edge. AutoCAD clips at the plot window. The controller's visual
check ("ONE sheet correctly framed and clipped") is consistent with this:
entities *wholly* inside the neighbour are culled, so only straddlers leak.

**Fix.** Emit a clip rectangle for the printable area once at the top of
`build_content`:

```
q  <x> <y> <w> <h> re  W  n   ... content ...  Q
```

Mirror it in `skia.rs` with a clip mask so the preview and the PDF agree.

### I9. Layers that AutoCAD would not plot are plotted
`src/plot/style.rs:79-82`, `src/dxf/tables.rs:142-156`

`entity_color` comments that a negative ACI "marks a layer that is turned
off; treat the absolute value as the colour and **let visibility be handled
elsewhere**". There is no elsewhere — nothing in the pipeline tests layer
visibility. `LayerRecord` does not even carry the flags: group 70 (bit 1 =
frozen) and group 290 (plottable) are never read, and a negative group 62 is
only used for its magnitude. The reference drawing has **1 of 191 layers
switched off**; I could not cheaply measure how many entities sit on it.

**Failure scenario.** Every entity on an off, frozen or non-plotting layer is
inked into the PDF. Since matching AutoCAD's plotted output is this project's
entire purpose, extra ink is a direct fidelity failure — and it is invisible
until someone overlays the reference.

**Fix.** Add `off: bool` (from `aci < 0`), `frozen: bool` (70 & 1) and
`plottable: bool` (290, default true) to `LayerRecord`, and skip such
entities in `emit`, counting them in `BuildReport` so the drop is reported.

### I10. Heavy `POLYLINE` (with `VERTEX` sub-entities) can never be drawn
`src/dxf/entities.rs:61-87`, `src/plot/flatten.rs:128-135`

`read_section` splits records on group code 0, so a `POLYLINE`'s vertices
become separate top-level `VERTEX` records. The `POLYLINE` itself therefore
carries no 10/20 pairs, `polyline_points` returns an empty vec, and `flatten`
returns `None`. The `VERTEX` and `SEQEND` records are then skipped in turn.
Proven:

```
E: heavy POLYLINE -> 0 drawn items, skipped = {"POLYLINE": 1, "VERTEX": 2, "SEQEND": 1}
```

`sheets.rs:104` and `:121` match on `"POLYLINE"` for the same reason and are
equally dead branches. PRD §5.7 lists POLYLINE as required, and `flatten`
advertises support for it.

Magnitude on the reference drawing: **zero** — dwg2dxf emitted no heavy
polylines (the 258 + 212 `SEQEND` records terminate `ATTRIB` sequences, of
which there are 288 + 216). So this is latent, but it will fire on 3D
polylines, polyface meshes and older-format files, and it is at least
reported rather than silent.

**Fix.** In `read_blocks`/`read_section`, fold `VERTEX` records into the
preceding `POLYLINE` until `SEQEND` (the same state-machine shape
`read_blocks` already uses for `BLOCK`/`ENDBLK`).

---

## Minor

1. **LWPOLYLINE bulges are positionally mis-assigned when any vertex omits
   group 42.** `src/plot/flatten.rs:193,203` — `all_f64(42)` collects only the
   codes that are *present*, then `bulges.get(i)` indexes by segment number.
   DXF writers emit 42 only for non-zero bulges. Proven with a spec-conformant
   4-vertex polyline whose only bulge is on the last segment: the arc appears
   at **17 %** along the path instead of **83 %**.
   *Why only Minor:* dwg2dxf writes a 42 for every vertex, so the DWG path is
   structurally safe — I measured **76 bulged polylines in the reference and
   0 misaligned**. But `converter::load` accepts `.dxf` directly, and
   AutoCAD's own DXF export omits zero bulges, so the direct-DXF path draws
   arcs on the wrong segments with no error. Fix: pair bulges with vertices
   during a single ordered pass over `codes` rather than with two independent
   filters.
2. **Clippy is red and the ledger mislabels it as pre-existing.**
   `src/sheets.rs:7` (`approx_constant`, deny-level → build failure) and
   `src/dxf/lexer.rs:194` (`while_let_loop`). Both files are *added* by this
   branch (396 and 366 lines, zero deletions), so neither diagnostic can be
   pre-existing. The repo's own Definition of Done bans linter warnings.
   Two-line fix: `std::f64::consts::SQRT_2`, and
   `while let Some(code_line) = lines.next()`.
3. **No `/Resources` on the page dictionary.** `src/render/pdf.rs:50-59`.
   Verified in the emitted file: the `/Page` dict has `/Type /Parent
   /MediaBox /Contents` only, and `/Pages` supplies nothing inheritable.
   `/Resources` is a required inheritable page attribute (PDF 1.7 §7.7.3.3).
   Viewers tolerate it — strict preflight and PDF/A do not. One line:
   `page.resources();`.
4. **True-colour black is treated as absent.** `src/plot/style.rs:70-72` —
   `if true_color > 0` means an entity explicitly assigned true-colour
   `0x000000` falls through to ByLayer. Track presence of code 420 instead of
   testing its value.
5. **Linetype lookup is case-sensitive.** `src/plot/style.rs:183,217-223` —
   an explicit name is passed through unchanged while `BYLAYER`/`BYBLOCK` are
   upper-cased, so `hidden` misses a `HIDDEN` table key and silently renders
   solid. Normalise both the table keys and the lookup.
6. **Paper-space entities are plotted into the model scene.** No filter on
   group 67 anywhere; `read_section(pairs, "ENTITIES")` returns both spaces.
   Proven (1 model + 1 paper-space entity → 2 items drawn). Zero occurrences
   in the reference. R-SHEET-1 (layouts) is deferred, but excluding `67 == 1`
   from the model plot is a one-line guard worth having now.
7. **`POINT` emits a zero-length stroke.** `src/plot/flatten.rs:136-139` —
   with `0 J` (butt caps) a degenerate segment draws nothing in PDF. 728
   POINTs live inside the reference's block bodies. Either use round caps for
   points or emit a small cross/dot per `$PDMODE`.
8. **ELLIPSE is never closed and is never rejected.**
   `src/plot/flatten.rs:127,166-189` — a full ellipse is emitted as an open
   subpath, and a zero-length major axis yields degenerate geometry instead
   of `None` (contrast CIRCLE/ARC, which check `r <= 0.0`).
9. **`is_frame_name` is narrower than R-SHEET-2.** `src/sheets.rs:28-35` —
   the PRD's `A[0-4]框` alternative is unimplemented (the code requires 图 *and*
   框/幅), L2's plain `TITLE` is implemented only as `TITLEBLOCK`/`TITLE_BLOCK`,
   and `TK` matches only as the entire name rather than as a substring.
10. **Depth-limit drops are unreported.** `src/plot/build.rs:93-95` returns
    silently, unlike every other drop path, which records into
    `BuildReport::skipped`.
11. **`--sheet` with a non-numeric argument silently exports everything.**
    `src/bin/cadconvert.rs:26-31` — `.and_then(|v| v.parse().ok())` turns
    `--sheet abc` into `None`, i.e. all 26 pages instead of an error. Unknown
    flags are likewise ignored.
12. **R-CLI exit codes are collapsed.** `src/bin/cadconvert.rs:39` returns 1
    for every failure; the PRD specifies 2 (decode failure) and 3 (no
    printable content) as distinct.
13. **`$PSLTSCALE` is parsed and never read** (`src/dxf/tables.rs:11,74`), so
    R-LT-3 is unimplemented and undeclared — it is not in the plan's deferred
    table.
14. **Dead code / leftover scaffolding.** `Affine::average_scale`
    (`geom.rs:61`, no callers — scaffolding for the text phase);
    `converter::build_scene` (`converter.rs:99`, no production callers left
    now that `main.rs` uses `sheet_request` + `build`, kept alive only by its
    own two tests); `aci::HUE_STEPS` (`aci.rs:15`, already known).
15. **R-SHEET-7 partially implemented.** The viewer lists sheets
    (`main.rs:629-648`) but shows raw drawing-unit extents rather than the
    inferred 图幅/比例, and there is no outline overlay on the canvas. Not in
    the deferred table.
16. **The spec is not versioned with the code.** `.gitignore` contains
    `prd.md`, and `git ls-files` confirms only `docs/prd-v1.0-archive.md` is
    tracked. Every requirement ID cited in the source comments points at a
    file that does not exist in a fresh clone.

---

## Notes on test quality

I looked specifically for vacuous assertions. The suite is better than
average — `decompressed_content` in `render/pdf.rs` inflates the stream and
asserts on real operators rather than trusting compressed bytes, and the
y-flip test asserts an intent-level property ("all marks in the bottom
tenth") that survives anti-aliasing. Three things to note:

- **`lineweight_gate.rs` genuinely tests what it claims.** At the 1000× extent
  the 13 lines compress into the corner of the printable area but remain
  inside it, so all 13 items survive the cull and the comparison is
  meaningful.
- **Skip-when-absent tests pass green.** `tests/real_drawing.rs` (3 tests)
  and `aci.rs::calibrated_against_autocad_reference_output` `eprintln!` and
  return when their local-only inputs are missing. That is a deliberate,
  documented convention and the files legitimately cannot be committed — but
  it means the entire real-drawing signal is worth nothing in CI or on any
  other machine. Consider gating them behind a feature or an env var so their
  absence is loud at the suite level, not just on stderr.
- **`group_annotation_boxes_never_become_pages` asserts `width < 400_000.0`**
  — that only rules out a candidate spanning the whole drawing. It would pass
  with a container half the size of the model. Tighten to the expected frame
  width once it is known.
- **`byblock_takes_the_inherited_lineweight`** (`style.rs:334-337`) passes an
  already-resolved value directly to `resolve_width_mm`, so it cannot see
  I2 — the bug lives in the caller that computes that value.

---

## Deferred-minor triage (from `progress.md`)

| Ledger item | Verdict | Reasoning |
| --- | --- | --- |
| clippy fails on lib — `sheets.rs:7`, `dxf/lexer.rs:194`, "pre-existing" | **Fix before merge** | Not pre-existing: both files are added by this branch. `approx_constant` is deny-level, so `cargo clippy` exits non-zero — the repo's Definition of Done ("no linter/formatter warnings") is currently unmet. Two-line fix. |
| `aci.rs HUE_STEPS` dead code behind `#[allow(dead_code)]` | **Can wait** — but delete it in the same commit as the clippy fix. One line, zero risk. Note there are three more dead symbols (Minor 14); clear them together. |
| Export re-runs `dwg2dxf` instead of reusing the in-memory `Document` | **Can wait** | Perf only, and only on the export path. But flag the structural half now: the preview (`main.rs::sheet_request`) and the export (`converter::scenes_for`) are two independent implementations of the same window/paper choice, kept in sync by a comment. That is a drift hazard, and it should be collapsed into one function *before* Phase 4 adds text metrics and Phase 6 adds `--paper`. |
| GUI never visually verified (no display automation) | **Can wait for merge, must not be called done** | The controller's page-1 render exercised the PDF backend, not `render/skia.rs`; the skia transform is covered by unit tests including pan and the y-flip. Acceptable risk for a branch merge, but the Phase 3 gate item "the viewer lists sheets and switches between them" is unchecked and should be recorded as unverified rather than assumed. |
| PERF: 26-page export 5m45s vs PRD <15s, "INSERT recursion not bounds-culled before block expansion" | **Can wait (Phase 6, as planned)** | **Diagnosis confirmed** — see I7 for the exact call path (`converter.rs:178` inside the per-sheet loop; the only cull is `build.rs:130-138`, after `flatten`, on leaves). Note the same missing cull is the mitigation for I6, so whoever picks up the perf work should be told it is also a robustness fix. |

---

## What I could not verify

- **Whether I3 (the −3 chain) should change.** Deciding it needs the 370-code
  distribution of the reference DXF cross-referenced against the width
  histogram of AutoCAD's reference PDF. The DWG is present on this machine;
  the AutoCAD-plotted PDF is not (the ACI calibration test skipped, which
  confirms it is absent at the path in `aci.rs:148-152`).
- **How many entities sit on the one switched-off layer (I9).** Measuring it
  needs a per-layer entity count including block expansion, which I did not
  run.
- **Whether the 11,192 SPLINEs inside the reference's block bodies look
  acceptable as control polygons.** Task 11's scope asserts the approximation
  is "visually acceptable for the dense control points LibreDWG emits". With
  11 k of them and no reference PDF here, that remains asserted rather than
  shown. Worth an explicit check in the Phase 5 visual-diff harness rather
  than leaving it as a scope note.
- **Whether R-LW-4 (deferred to Phase 5) is bigger than it looks.** Not a
  defect — but the reference drawing has **522 wide LWPOLYLINEs in model
  space plus 12 inside blocks** that currently render as thin strokes rather
  than filled ribbons. Phase 5 should be scoped with that number in hand.
- **The GUI at runtime.** No display automation available; I read `main.rs`
  but did not execute it.
