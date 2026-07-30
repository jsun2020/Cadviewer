# SDD ledger — plan: docs/superpowers/plans/2026-07-30-plot-pipeline.md

Branch: plot-pipeline
Merge base: 2328acfd7858da2d184b57c5fd4cb0767de988d5
Pre-flight: fixed plan defect (Task 1 dropped resvg/svg2pdf too early, crate
would not compile for Tasks 1-15). Committed as part of branch setup.
Task 1: complete (commits e832038..93e8da2). Controller hand-verified Affine::then
  composition order (all 6 terms) and Bounds::contains argument order -- both correct.
  Reviewer review-task1 dispatched; verdict folded in on arrival.
INFRA: review-task1 subagent went idle twice without relaying its verdict; a
  direct SendMessage request also produced no content. Root cause not diagnosed.
  MITIGATION: from Task 2 onward, reviewers must WRITE their verdict to
  <workspace>/task-N-review.md, which the controller reads. Do not rely on
  subagent return text for review verdicts.
Task 1: review not obtained from subagent. Controller verified by hand instead
  (Affine::then term-by-term, Bounds::contains ordering, test quality). Scope is
  130 lines of pure geometry with complete spec. Recorded as a REDUCED review --
  the final whole-branch review must cover src/geom.rs explicitly.
Task 2: complete (commits 93e8da2..ac350a8, 5 tests pass, build green - controller verified)
Task 3: complete (commits ac350a8..f32d756, 5 tests pass incl. calibration).
  Implementer correctly fixed a plan bug: closure inside const fn is invalid Rust.
  Colour arithmetic unchanged; controller independently derived 10,11,12,13,20
  and they match the real ACI palette.
Task 3: minor (deferred): HUE_STEPS const is dead code, silenced with
  #[allow(dead_code)] rather than removed. Final review should delete it.
DECISION (affects Tasks 4,5,6,8,10,11,12,17): plan says module `dxf`, but
  src/dxf.rs (old SVG parser) still exists and Rust rejects dxf.rs + dxf/mod.rs.
  New module is therefore `dxfnew` until Task 16 deletes src/dxf.rs, then renamed
  back to `dxf`. ALL tasks referencing crate::dxf::* must use crate::dxfnew::*
  until that rename. Task 16 must include the rename.
Task 4: complete (commits f32d756..0a2fd53, 22 lib tests pass, build green). Module = dxfnew per DECISION above.
DECISION: switching to limited parallelism. Dependency graph:
  5 -> 6 -> {8->9->10 (same file, serial), 11} -> 12 -> 14 -> 16 -> 17 -> 18 -> 19 -> 20
  7 (geom only) -> {13, 15}
  Parallel dispatches use isolation:worktree to protect test signal; controller
  merges. Tasks 8/9/10 must NEVER run concurrently (same file plot/style.rs).
Task 5: complete (ac2d5ee), lib suite green.
Task 6: complete (12fe28a), 29 lib tests green. PHASE 1 CODE COMPLETE.
PHASE 1 GATE: FAILED on real drawing (commit 533b899 added tests/real_drawing.rs).
  codepage + Chinese layer names PASS. Title-block INSERT count = 0, expected 26.
  Root cause (controller hypothesis, being verified by impl-task4): binary-DXF
  value-width table wrong for (a) codes 290-299 = 1-byte bool, not 2-byte i16;
  (b) codes 310-319 and 1004 = binary chunk (1 length byte + N raw bytes), not
  null-terminated string. One mis-sized value desyncs the whole stream, which
  matches "BLOCK records fine, all later INSERTs garbage".
  This is a DEFECT IN THE PLAN's kind_of table, not implementer error.
  BLOCKS Phase 3 (sheet detection needs INSERT block names). Fix dispatched.
Task 7: complete (worktree branch worktree-agent-a054e0fc44c31d496 merged clean
  into plot-pipeline; src/plot/mod.rs + lib.rs one line). 4 tests pass.
NOTE ON PARALLELISM COST: the worktree run took 36 minutes wall-clock, almost all
  of it a cold `cargo build` of the eframe/resvg dependency tree in a fresh
  target/ dir. The task's own work was ~5 min. For this repo, worktree isolation
  is a BAD trade unless the task itself is long. Reverting to serial dispatch on
  the main checkout.
INCIDENT (controller error): impl-task6 had not finished when I dispatched
  impl-gate1 for the Phase 1 gate; task6 then moved on to the gate itself and
  began fixing src/dxfnew/lexer.rs at the same time as impl-task4, which I had
  dispatched to fix the same file. Two agents, one file, one working tree.
  RESOLUTION: impl-task6 told to stand down; lexer.rs owned solely by impl-task4.
  task6's committed work (12fe28a) is accepted and unaffected.
  ROOT CAUSE: I treated "commit appeared" as "agent finished". It is not -- an
  agent can keep working after its commit lands. Wait for the agent's own
  completion message before dispatching anything that touches adjacent files.
PHASE 1 GATE: PASSES (26 inserts, Chinese layer names correct).
  Real root cause was NOT the desync I hypothesised. Lexer Bool/Chunk fixes
  (8fc033e) were correct and kept, but the actual defect was that LibreDWG's
  BINARY writer emits name fields (codes 2, 8) as UTF-16LE -> null-terminated
  read truncates "ASHADE" to "A". Entity counts identical in both formats, which
  is what ruled out desync. FIX: use ASCII DXF. Plan updated (Task 16 must not
  use -b). Diagnostic examples/ removed.
PHASE 1 COMPLETE: Tasks 1-7 + gate. 36 lib tests + 1 integration test green.
Task 8: complete (d08c8df), 43 lib tests green.
Task 9: complete (bc7e11e), 51 lib tests, 13-width acceptance test passes.
Task 10: complete (56873d3), 57 lib tests green. style.rs now has colour+lineweight+linetype.
Task 11: complete (bc7728b), 66 lib tests green.
  PLAN BUG #2 found and fixed by implementer: bulge_arc had the wrong sign
  (center = mid + apothem*perp with sweep +theta). Controller independently
  verified: for chord (0,0)->(10,0) bulge 1.0 that yields peak y=-5; DXF says
  positive bulge is CCW so it must be +5. Correct form is center = mid -
  apothem*perp with sweep -theta. Verified against a quarter-circle too
  (peak 2.071 = r - apothem). NOTE: plan's version was ported from the existing
  src/dxf.rs, so the CURRENT shipped renderer may mirror all bulged arcs.
  Flag for the final review / a follow-up bug report to the user.
Task 12: complete (b53e402), 73 lib tests green, scale-independence test passes.
Task 13: complete (e7d58a3), 80 lib tests green. PLAN BUG #3: colour format string {:.4} never matched expected '1 0 0 RG'; implementer added fmt_channel trimming.
Task 14: complete (e1327e7). PHASE 2 GATE PASSES: 13 widths exact + scale-independent. Full suite 83 green (real_drawing runs for real, 11.6s).
Task 15: PLAN BUG #4 (in a TEST, not impl): y-flip test probed row h-12 but the
  stroke occupies rows 189-190 on a 200px image (scene y=5mm * 2px/mm = 10px from
  bottom). Implementer correctly refused to edit the test unilaterally and asked.
  Controller verified the arithmetic and replaced the exact-row probe with an
  intent-level assertion (all marked rows must be in the bottom tenth), which is
  robust to AA/rounding and still fails if the flip is absent or inverted.
Task 15: complete (85249dc), 85 lib tests green.
Task 16: complete (19af44b wiring + ca4981a module rename back to dxf).
  resvg/svg2pdf removed, src/dxf.rs and src/pdf.rs deleted, release build green,
  full suite 89 green (86 lib + 2 gate + 1 real_drawing).
  END-TO-END WORKS: Cadconvert.exe sample.dwg out.pdf -> exit 0, "已导出 1 页".
PLAN GAP #5 (controller finding, not implementer error): the produced PDF is
  1,015,874,771 bytes (1.0 GB). Content stream is emitted RAW; the plan never
  specified compression. AutoCAD's reference for ONE sheet is 3.67MB on disk from
  a ~28MB stream (~7.6x flate). Two compounding causes: (a) no FlateDecode,
  (b) all 26 sheets' geometry on one page pending Phase 3 pagination.
  FIX DISPATCHED to impl-task13: add flate2 compression + a test that the
  operator assertions still hold (decompress before asserting), plus a diagnostic
  PlotItem count to rule out runaway block-expansion duplication.
PLAN GAP #5 FIXED (08d986f): flate2 compression, 1.0GB -> 65MB (15.5x).
  Operator-assertion tests preserved by decompressing before asserting.
  PlotItem count for whole model space = 3,845,011 (~130k unique entities x ~29
  block expansion). DEFERRED to Phase 3: once paginated, per-page item count and
  file size become directly comparable to AutoCAD's 3.67MB single-sheet reference.
  If a single page is still >>4MB after Phase 3, investigate block duplication.
PHASE 2 COMPLETE.
Task 17: complete (f2b8c47), 93 lib tests green.
Task 18: complete (c4e43fe). 99 lib + 3 real_drawing green.
  REAL DRAWING: 26 sheets, rows [2,2,1,1,6,6,4,3,1] EXACT match to the user's
  screenshot. No group box survived. The headline Phase 3 feature works.
  PLAN BUG #6: my plan's code said `enclosed >= 2` while my own test
  (a_container_holding_only_one_frame_is_still_a_border) required >= 1. Direct
  contradiction. Implementer chose >= 1. Controller reviewed: >= 1 is strictly
  more conservative in the dangerous direction (errs toward discarding
  containers; the catastrophic mode is KEEPING a group box and losing 26 pages).
  All real-drawing assertions pass. ACCEPTED.
Task 19: work complete but agent went idle WITHOUT COMMITTING; nudged.
  Verified by controller: suite 109 green, release build clean.
  END-TO-END: "已导出 26 页", 128,575,181 bytes.
  SIZE: 4.94 MB/page vs AutoCAD reference 3.67 MB/sheet = 1.35x. Acceptable;
  the earlier 3.8M-item figure was for ALL sheets at once, not per page. The
  block-duplication worry from Task 13 is resolved -- volume is legitimate.
PERF FINDING (defer to Phase 6, but report to user): 26-page export took
  5m45s against a PRD target of <15s for all sheets. Diagnosis: scenes_for
  calls build() once per sheet, and build() expands every block over the whole
  document before culling to the page -- 26 full traversals generating ~3.8M
  items each. O(sheets x entities) instead of ~O(entities). Fix belongs with the
  spatial index already scoped for Phase 6: cull before expanding, or index once
  and query per sheet.
Task 19: COMMITTED 69cfea9. VISUAL VERIFICATION by controller (rendered page 1
  at 100dpi and compared to the AutoCAD reference):
  - Page 1 shows ONE sheet correctly framed and clipped, NOT the whole model.
    impl-task19's correctness worry is DISPROVEN; culling works. Its perf
    diagnosis (INSERT recursion not bounds-culled before expanding) still stands.
  - Geometry, frame, colours (red frame / green equipment / magenta / cyan
    legend / black walls) and lineweight variation all match the reference.
  - NO TEXT anywhere (empty title block, empty legend). Expected: Phase 4.
  - Paper chosen = A2 594x420mm; AutoCAD used A4. Both are geometrically valid
    per R-SHEET-5 (derive from frame size), but this is a deviation from the
    reference worth noting for the visual-diff harness in Phase 5.
Task 20: complete (959fdca). 109 tests green, release build clean.
  PLAN BUG #7: brief passed DRAWING UNITS to PaperSize::fit which expects MM.
  Viewer preview and exported PDF would have picked different paper sizes and
  therefore disagreed on line thickness (lineweights are absolute mm).
  Implementer mirrored converter::scenes_for's aspect normalisation. ACCEPTED.
  Also corrected: egui 0.35 has no SidePanel (Panel::left), trait method is ui()
  not update(), and the brief's warnings format! accumulated duplicates per rebuild.
  Scene rebuilds are edge-triggered only (dirty flag + generation stamp + worker
  thread), so the ~13s/sheet cost does not freeze the UI.
DEFERRED MINORS for final review:
  - clippy fails on lib (pre-existing): src/sheets.rs:7 1.4142 trips
    approx_constant; src/dxf/lexer.rs:194 while_let_loop.
  - aci.rs HUE_STEPS is dead code behind #[allow(dead_code)].
  - Export re-runs dwg2dxf instead of reusing the in-memory Document.
  - GUI never visually verified (no display automation available).
  - PERF: 26-page export 5m45s vs PRD <15s. Diagnosed: INSERT recursion not
    bounds-culled before block expansion. Phase 6 scope.
PHASE 3 COMPLETE. All 20 plan tasks done.
FIX WAVE: complete (a8f84b3, a66ae44, 12ea4bd, 40f4574, +1). All 2 Critical and
  10 Important addressed; I4 spec-corrected, I7 deferred per rulings. 146 tests
  (up from 110), clippy clean, release clean.
RE-REVIEW: dispatched agent failed to deliver 3x. Controller performed the
  new-breakage analysis directly with measured data -> .../re-review.md.
  Verdict ALL ADDRESSED, no new Critical/Important. Notably confirmed the lexer
  clamp does NOT corrupt +/-2M coordinates, and the sheet-3 sparsity anomaly is
  legitimate content, not data loss.
PACKAGED: dist/Cadviewer-portable-win64.zip (21.1MB), smoke-tested from the
  extracted folder (26 pages), leak-checked with control assertions on both the
  outer zip and the bundled source zip.
