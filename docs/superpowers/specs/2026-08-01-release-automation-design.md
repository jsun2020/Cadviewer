# Cadviewer Release Automation — Design

**Date:** 2026-08-01
**Status:** Approved, ready for an implementation plan

**Goal:** A tag push produces a GitHub Release whose zip is usable the moment
it is downloaded — proved by running the packaged binary from a fresh
directory, not asserted.

## Why this exists

The project has a complete local packaging recipe (`scripts/package.ps1`) and
no automation around it. Two failure modes follow from that, and both have
already happened in this codebase or its sibling projects:

1. **Stale artifact mistaken for a broken fix.** The MTEXT fix in `039be23`
   was correct in source, but the package in `dist\` predated it by 66
   minutes. Running it reproduced the old output exactly, and the bug was
   reported again. Diagnosing that cost a full pass over the layer table,
   the sheet detector and the plot builder before a file timestamp gave it
   away. `4cc5395` added a build stamp so the binary names its own commit;
   this design removes the remaining gap by making the published artifact a
   product of the tag rather than of somebody's working directory.
2. **Tagged is not released.** LL-032 records a repository with ten pushed
   tags, zero published artifacts, and a README instructing users to extract
   a zip that existed only on the developer's machine. Nobody clicks their
   own download link.

## Scope

In scope: continuous build/test on push, a tag-driven release, artifact
verification, and a download path in the README.

Out of scope, deliberately:

- **GUI verification.** There is no display automation here. The workflow
  never launches `Cadviewer.exe`. This is stated in the README's known-gaps
  list and stays true.
- **Plot fidelity.** The reference drawing and the AutoCAD-plotted baselines
  are customer material and are never committed, therefore never available
  to CI. The tests that need them print an explicit skip. Fidelity
  regressions still require a human comparison.
- **Non-Windows targets.** The product is Windows-only by design.
- **Signing.** No code-signing certificate exists. SmartScreen will warn on
  first run; that is expected and unchanged by this work.

## Triggers

| Workflow | Trigger | Purpose |
| --- | --- | --- |
| `ci.yml` | push to any branch, pull requests | `cargo clippy --all-targets -- -D warnings`, `cargo test` |
| `release.yml` | push of a tag matching `v*` | verify, package, smoke-test, publish |

Tag-driven, so every published artifact is tied to an exact commit and
nothing can be released that is not in git.

## Version: one source, and a guard that proves it

`Cargo.toml`'s `version` is the single source of truth. The release job
asserts three things agree before anything is published:

1. the tag, with its leading `v` removed;
2. `version` in `Cargo.toml`;
3. the version the built `Cadconvert.exe` prints at runtime, via the
   `build_info::stamp()` added in `4cc5395`.

Point 3 is what makes the guard meaningful: 1 and 2 are both text files a
mistake can edit consistently, while 3 is the artifact speaking for itself.

**Implementation trap, from LL-032:** PowerShell variable names are
case-insensitive, so `param([string]$Version)` followed by a local
`$version = <read from Cargo.toml>` is *one variable*. The assignment
overwrites the caller's argument and the comparison can never fail. The
script must use distinct names (`$TagVersion` / `$ManifestVersion`), and the
guard must be tested by triggering it with a deliberately wrong tag, never
by observing it stay quiet.

## The smoke test — the "usable on download" proof

This is the part that justifies the whole design. It runs against the
**published zip**, expanded into a fresh temporary directory, never against
the build tree:

1. Expand `Cadviewer-portable-win64.zip` to a temp path.
2. Obtain a real `.dwg` fixture (see "DWG fixture" below) and a small ASCII
   DXF fixture with no entities, as the control. No customer data ever
   reaches CI.
3. Run the **extracted** `Cadconvert.exe`, with the working directory moved
   into the extraction sandbox, against both fixtures.
4. Assert exit code 0 for each, that each output begins with `%PDF-` and
   contains a resolvable `/Type /Page` object, and that the DWG's page
   content stream is substantially larger than the empty control's — not
   merely present, since a zero-entity conversion still produces a
   structurally valid, empty PDF page (see "DWG fixture" below).
5. As a control assertion, cripple a *copy* of the zip by deleting its
   `runtime\` entries and require the same conversion to fail against it. A
   smoke test that would pass no matter what the zip contains proves
   nothing; this is what makes the pass in step 4 meaningful.

Step 3's working-directory move is not optional. `locate_converter()`
(`src/converter.rs`) falls back to `current_dir()\runtime\dwg2dxf.exe` when
it finds no `runtime\` beside the exe; left at the caller's directory — the
repository root in CI, where `prepare-libredwg.ps1` has just staged that
same path for the build — that fallback silently resolves to the **build
tree's** runtime, and the test would pass even for a zip shipping no
`runtime\` folder at all. This was demonstrated directly: with the
`runtime\` entries deleted from the real package zip, an earlier version of
this script passed when run from the repository root and only failed when
run from an empty directory. Step 5 exists so that failure mode cannot
recur silently.

**DWG fixture.** The original plan was to convert the ASCII DXF fixture to
`.dwg` with `dxf2dwg.exe` from the LibreDWG bundle. Measured directly: that
round-trip produces a DWG with **zero entities** — LibreDWG's DWG writer is
its least mature component — which renders as a structurally valid but
empty PDF page and would make the smoke test pass against a build that
cannot draw anything. The DWG fixture actually shipped is instead
`libredwg-0.14/test/test-data/example_2004.dwg`, lifted from the source
archive already downloaded and SHA-256-pinned for the GPL corresponding-source
offer — LibreDWG's own GPL test data, so no customer drawing is ever
involved, and it exercises `dwg2dxf.exe`/`libredwg-0.dll` for real (5
drawings, page content stream of 660 bytes, versus 64 for the empty
control). This is better than the originally planned fallback of dropping to
a DXF-only test, which would never load the LibreDWG runtime at all.

Magic bytes and a resolvable page object are checked rather than existence
or size, because a zero-page or HTML-shaped file passes both weaker checks
(LL-005).

## Existing gates, kept

`package.ps1` already refuses to build an archive that violates these, and
each carries a control assertion proving the check can fire:

- **No fonts.** SHX files are Autodesk/third-party licensed and TTFs are
  Microsoft's; every font is located on the user's machine at runtime.
  Verified by planting a `canary.shx` and confirming the gate catches it.
- **Icon embedded.** Verified by searching for the 256×256 image from the
  `.ico` inside the exe, with `dwg2dxf.exe` as a negative control.

CI runs the same script, so these gate the released artifact and not merely
local builds.

## Release contents

| File | Why |
| --- | --- |
| `Cadviewer-portable-win64.zip` | The product: both exes, the LibreDWG runtime, licences, and the corresponding source archives |
| `Cadviewer-portable-win64.zip.sha256` | Lets a user verify the download |

Release notes state the GPL-3.0-or-later licence, that LibreDWG's
corresponding source is included in the zip (a GPL obligation, already
satisfied by `package.ps1`), and that no fonts are bundled.

## README

A 下载 section is added above 开发与构建, linking
`https://github.com/jsun2020/Cadviewer/releases/latest` — never a
version-stamped filename, which is precisely how LL-032's repository came to
document a download nobody could obtain.

## Permissions and cost

The release job needs `permissions: contents: write` and uses the built-in
`github.token`; no secrets are configured or required. `gh` is preinstalled
on GitHub-hosted Windows runners. Windows runners are free for public
repositories and bill at 2× minutes for private ones; a release run is
roughly 5–8 minutes.

## Success criteria

1. Pushing tag `v0.1.0` publishes a release carrying the zip and its
   checksum.
2. A tag whose version disagrees with `Cargo.toml` fails the run and
   publishes nothing — demonstrated by triggering it, not by inspection.
3. The smoke test converts a DWG with the extracted binary and the output is
   a real PDF.
4. An ordinary push runs clippy and the test suite.
5. The README leads a new user from the repository page to a working
   download without building anything.
