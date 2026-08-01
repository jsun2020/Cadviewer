# Cadviewer Release Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pushing a tag `v<x.y.z>` publishes a GitHub Release whose zip is proved usable by running the packaged binary, extracted to a fresh directory, against a real DWG.

**Architecture:** The workflow YAML stays thin and calls PowerShell scripts that can be run locally. `scripts/package.ps1` remains the single packaging recipe, so the released artifact and a local build cannot drift. Two new scripts carry the logic that CI adds: `verify-release.ps1` (the tag, `Cargo.toml` and the binary must agree on the version) and `smoke-test.ps1` (extract the zip elsewhere and convert a real DWG with it).

**Tech Stack:** GitHub Actions on `windows-latest`; Windows PowerShell 5.1; `gh` CLI (preinstalled on the runner); Python 3 stdlib `tarfile` (preinstalled) to lift one fixture out of the pinned LibreDWG source archive.

## Global Constraints

- **Never bundle a font.** SHX files are Autodesk/third-party licensed and TTFs are Microsoft's. `package.ps1`'s `Assert-NoFonts` gate, canary included, must keep running in the release path.
- **Customer material never reaches CI.** The reference DWG, the AutoCAD baselines and `prd.md` are gitignored and must stay that way. Test fixtures come from LibreDWG's own GPL test data, never from `C:\Users\sr9rfx\Desktop`.
- **No non-ASCII in shell command string literals.** Chinese is fine inside `.md`, `.rs` and `.ps1` *file* contents; it must never be an argument typed into a shell command.
- **Windows PowerShell 5.1 only** on the runner's default shell. No `&&`/`||` chaining, no ternary, no `??`, no `-AsHashtable`. `[System.Text.Encoding]::Latin1` does not exist — use `GetEncoding(28591)`.
- **Prove every gate fires.** A check that cannot fail is not a check (LL-032, LL-033). Each gate in this plan ships with a control assertion or a documented triggering procedure.
- **Version source of truth is `Cargo.toml`'s `[package] version`.** Currently `0.1.0`; the first tag is therefore `v0.1.0`.
- Run `cargo clippy --all-targets -- -D warnings` and `cargo test` before every commit; the tree stays clippy-clean.

## Measured facts this plan depends on

Established by direct experiment on 2026-08-01, not assumed:

| Fact | Value |
| --- | --- |
| `dxf2dwg.exe` round-tripping a hand-written DXF | Produces a DWG with **zero entities** — unusable as a fixture |
| `libredwg-0.14.tar.xz` contents | 1171 entries, **141 `.dwg`** test files |
| `libredwg-0.14/test/test-data/example_2004.dwg` | 187,890 bytes, sha256 `e72d5e86d5d36d64b08822fb25a46079f592fd895a6157b1b8d9b07775e06108` |
| That DWG through the packaged `Cadconvert.exe` | exit 0, 1 page, 5 drawings, PDF max `/Length` = **660** |
| A drawing with no entities through the same binary | 1 page, 0 drawings, PDF max `/Length` = **64** |
| The win64 zip's `examples\` folder | C sources and exes only, **no DWG** — the fixture must come from the tar.xz |
| Windows' bundled `tar.exe` | **Cannot** read `.xz` ("unable to run program xz -d -qq") — use Python's `tarfile` |

The 64-versus-660 gap is what lets the smoke test tell a drawn page from a blank one without a magic constant: it converts an empty drawing too and requires the real one to be far larger.

---

### Task 1: `--version` for Cadconvert, and the version guard

The guard has to ask the *binary* what version it is, not just read two text files that a mistake can edit consistently. `Cadconvert.exe` currently prints its stamp only on a successful conversion, so it needs a flag.

**Files:**
- Modify: `src/bin/cadconvert.rs`
- Create: `scripts/verify-release.ps1`

**Interfaces:**
- Consumes: `cadviewer::build_info::stamp() -> String`, already present, returning e.g. `0.1.0 (4cc5395)`.
- Produces: `Cadconvert.exe --version` printing exactly `Cadviewer <stamp>` on stdout with exit code 0; `scripts/verify-release.ps1 -Tag <v0.1.0> -ConvertExe <path>` exiting non-zero on any disagreement.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module at the bottom of `src/bin/cadconvert.rs`:

```rust
    /// `--version` must be recognised before the input/output arguments are
    /// required, or `Cadconvert.exe --version` fails as a usage error.
    #[test]
    fn a_version_request_is_recognised_anywhere_in_the_arguments() {
        assert!(wants_version(&[os("--version")]));
        assert!(wants_version(&[os("-V")]));
        assert!(wants_version(&[os("in.dwg"), os("out.pdf"), os("--version")]));
    }

    #[test]
    fn an_ordinary_conversion_is_not_a_version_request() {
        assert!(!wants_version(&[os("in.dwg"), os("out.pdf")]));
        assert!(!wants_version(&[os("in.dwg"), os("out.pdf"), os("--mono")]));
        assert!(!wants_version(&[]));
    }
```

and this helper inside the same `tests` module:

```rust
    fn os(value: &str) -> std::ffi::OsString {
        std::ffi::OsString::from(value)
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --offline --bin cadconvert`
Expected: FAIL, `cannot find function 'wants_version' in this scope`.

- [ ] **Step 3: Write the minimal implementation**

In `src/bin/cadconvert.rs`, add above `fn main`:

```rust
/// Whether the caller asked for the version rather than a conversion.
///
/// Checked before the input and output arguments are demanded, so
/// `Cadconvert.exe --version` is not a usage error.
fn wants_version(args: &[std::ffi::OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        arg == "--version" || arg == "-V"
    })
}
```

Then replace the first three statements of `main` — the `let mut args = ...skip(1);` line and the two `let Some(...) = args.next() else` blocks — with:

```rust
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if wants_version(&args) {
        println!("Cadviewer {}", cadviewer::build_info::stamp());
        return ExitCode::SUCCESS;
    }
    let Some(input) = args.first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(EXIT_INPUT);
    };
    let Some(output) = args.get(1) else {
        eprintln!("{USAGE}");
        return ExitCode::from(EXIT_INPUT);
    };
```

Immediately below, replace the `let rest: Vec<String> = args.map(...)` line with:

```rust
    let rest: Vec<String> = args[2..]
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
```

The input and output stay `OsString`: they are paths, and the reference drawing's path contains Chinese, so they must not round-trip through a lossy conversion. Only the option tail becomes `String`.

Update `USAGE` to mention the flag:

```rust
const USAGE: &str =
    "用法：Cadconvert.exe <input.dwg|dxf> <output.pdf> [--mono] [--all|--sheet N] [--font-dir <path>]\n      Cadconvert.exe --version";
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --offline --bin cadconvert`
Expected: PASS, all tests in the file.

- [ ] **Step 5: Check the flag works end to end**

Run:
```powershell
cargo build --offline --release --bin cadconvert
.\target\release\cadconvert.exe --version
```
Expected: one line like `Cadviewer 0.1.0 (<hash>)`, exit code 0.

- [ ] **Step 6: Write `scripts/verify-release.ps1`**

```powershell
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][string]$ConvertExe
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))

# LL-032: PowerShell variable names are case-insensitive, so a local named
# $version would BE the $Version parameter and silently overwrite the
# caller's argument -- the comparison below could then never fail. Every
# local here is deliberately named something the parameters are not.
$TagVersion = $Tag -replace '^v', ''

$ManifestPath = Join-Path $ProjectRoot 'Cargo.toml'
# Anchored at line start so it matches [package] version and not the
# `version = "..."` inside an inline dependency table.
$ManifestMatch = Select-String -Path $ManifestPath -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1
if (-not $ManifestMatch) {
    throw "No [package] version found in $ManifestPath"
}
$ManifestVersion = $ManifestMatch.Matches[0].Groups[1].Value

if ($TagVersion -ne $ManifestVersion) {
    throw "Tag '$Tag' means version '$TagVersion' but Cargo.toml says '$ManifestVersion'. Bump one of them; do not publish a mislabelled build."
}

# The artifact speaking for itself. The two checks above compare text files
# that one careless edit can make consistently wrong; this one cannot be
# faked without actually building the right source.
$Reported = (& $ConvertExe --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "'$ConvertExe --version' exited with $LASTEXITCODE"
}
$Expected = "Cadviewer $ManifestVersion "
if (-not $Reported.StartsWith($Expected)) {
    throw "$ConvertExe reports '$Reported', which does not start with '$Expected'."
}
if ($Reported -match '\+\)$') {
    throw "$ConvertExe was built from a dirty working tree ('$Reported'); a release must correspond to a commit."
}

Write-Host "Version agreed: tag $Tag, Cargo.toml $ManifestVersion, binary '$Reported'"
```

- [ ] **Step 7: Prove the guard passes on a correct tag**

Run:
```powershell
.\scripts\verify-release.ps1 -Tag v0.1.0 -ConvertExe .\target\release\cadconvert.exe
```
Expected: prints `Version agreed: ...` and exits 0.

Note: this will fail on the dirty-tree check if the working tree has uncommitted changes, because the stamp then ends in `+)`. That is correct behaviour. Commit first, rebuild, then re-run.

- [ ] **Step 8: Prove the guard FAILS on a wrong tag — the control assertion**

Run:
```powershell
.\scripts\verify-release.ps1 -Tag v9.9.9 -ConvertExe .\target\release\cadconvert.exe
```
Expected: throws `Tag 'v9.9.9' means version '9.9.9' but Cargo.toml says '0.1.0'` and exits non-zero.

**Do not skip this step.** LL-032 records this exact guard shipping broken because it was only ever observed staying quiet. If this command succeeds, the guard is inert and the parameter is being shadowed.

- [ ] **Step 9: Run clippy and the full suite**

Run: `cargo clippy --offline --all-targets -- -D warnings` then `cargo test --offline`
Expected: clean; all tests pass.

- [ ] **Step 10: Commit**

```bash
git add src/bin/cadconvert.rs scripts/verify-release.ps1
git commit -m "feat(cli): --version, and a release guard the artifact must satisfy"
```

---

### Task 2: The smoke test — prove the zip works after download

**Files:**
- Create: `scripts/smoke-test.ps1`

**Interfaces:**
- Consumes: `dist\Cadviewer-portable-win64.zip` produced by `scripts/package.ps1`; `third_party\libredwg-0.14.tar.xz` fetched by `scripts/prepare-libredwg.ps1`.
- Produces: `scripts/smoke-test.ps1 -Zip <path> -SourceArchive <path>` exiting non-zero unless the extracted binary converts a real DWG into a PDF that actually contains drawing.

- [ ] **Step 1: Write `scripts/smoke-test.ps1`**

```powershell
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Zip,
    [Parameter(Mandatory = $true)][string]$SourceArchive
)

$ErrorActionPreference = 'Stop'

# The whole point is to test what a user downloads, in a place that has none
# of the build tree around it. A binary run from target\release can resolve a
# DLL that a downloaded copy cannot (LL-033).
$Sandbox = Join-Path ([System.IO.Path]::GetTempPath()) ("cadviewer-smoke-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $Sandbox | Out-Null
try {
    Expand-Archive -LiteralPath $Zip -DestinationPath $Sandbox
    $Convert = Join-Path $Sandbox 'Cadviewer-portable-win64\Cadconvert.exe'
    if (-not (Test-Path -LiteralPath $Convert)) {
        throw "The zip does not contain Cadviewer-portable-win64\Cadconvert.exe"
    }

    # A real DWG, so dwg2dxf.exe and libredwg-0.dll are genuinely exercised.
    # A DXF input would never load either, and would pass with an empty
    # runtime folder. This fixture is LibreDWG's own GPL test data, lifted
    # from the source archive we already ship for the GPL offer -- no
    # customer drawing is ever involved.
    $FixtureName = 'libredwg-0.14/test/test-data/example_2004.dwg'
    $FixtureSha = 'e72d5e86d5d36d64b08822fb25a46079f592fd895a6157b1b8d9b07775e06108'
    $Fixture = Join-Path $Sandbox 'fixture.dwg'
    # Windows' bundled tar.exe cannot read .xz; Python's stdlib can.
    $Extract = @"
import sys, tarfile
with tarfile.open(sys.argv[1]) as archive:
    member = archive.extractfile(sys.argv[2])
    if member is None:
        raise SystemExit('fixture %s not in %s' % (sys.argv[2], sys.argv[1]))
    open(sys.argv[3], 'wb').write(member.read())
"@
    $ExtractScript = Join-Path $Sandbox 'extract.py'
    Set-Content -LiteralPath $ExtractScript -Value $Extract -Encoding ASCII
    & python $ExtractScript $SourceArchive $FixtureName $Fixture
    if ($LASTEXITCODE -ne 0) { throw "extracting the DWG fixture failed" }
    $Actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Fixture).Hash.ToLowerInvariant()
    if ($Actual -ne $FixtureSha) {
        throw "Fixture checksum mismatch: expected $FixtureSha, got $Actual"
    }

    # A drawing with no entities at all, as the control. Its page is
    # structurally a valid PDF and passes every magic-byte check while
    # containing nothing -- which is exactly what a broken build produces.
    $EmptyDxf = Join-Path $Sandbox 'empty.dxf'
    Set-Content -LiteralPath $EmptyDxf -Encoding ASCII -Value @(
        '  0', 'SECTION', '  2', 'ENTITIES', '  0', 'ENDSEC', '  0', 'EOF'
    )

    # NOT named $Input/$Output: $input is a PowerShell automatic variable
    # (the pipeline enumerator), and shadowing it inside a function is the
    # same class of silent breakage as the $Version aliasing in LL-032.
    function Get-PdfInk([string]$InputPath, [string]$OutputPath, [string]$What) {
        & $Convert $InputPath $OutputPath | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "converting $What exited with $LASTEXITCODE" }
        if (-not (Test-Path -LiteralPath $OutputPath)) { throw "no PDF produced for $What" }
        $Bytes = [System.IO.File]::ReadAllBytes($OutputPath)
        # 28591 = ISO-8859-1. Windows PowerShell 5.1 has no ::Latin1, and a
        # null encoding turns every check below into a false negative.
        $Text = [System.Text.Encoding]::GetEncoding(28591).GetString($Bytes)
        if (-not $Text.StartsWith('%PDF-')) { throw "$What did not produce PDF magic bytes" }
        if ($Text.IndexOf('%%EOF') -lt 0) { throw "$What produced a truncated PDF" }
        if ($Text.IndexOf('/Type /Page') -lt 0) { throw "$What produced a PDF with no page object" }
        $Lengths = [regex]::Matches($Text, '/Length\s+(\d+)') |
            ForEach-Object { [int]$_.Groups[1].Value }
        if (-not $Lengths) { throw "$What produced a PDF with no content stream" }
        return ($Lengths | Measure-Object -Maximum).Maximum
    }

    $EmptyInk = Get-PdfInk $EmptyDxf (Join-Path $Sandbox 'empty.pdf') 'the empty control drawing'
    $RealInk = Get-PdfInk $Fixture (Join-Path $Sandbox 'fixture.pdf') 'the DWG fixture'

    Write-Host "content stream: empty control $EmptyInk bytes, real drawing $RealInk bytes"
    # Measured on 2026-08-01: 64 for an empty page, 660 for this fixture.
    # Comparing against the control rather than a constant means the check
    # keeps working when the PDF writer's output changes.
    if ($RealInk -le $EmptyInk * 3) {
        throw "The DWG converted to an essentially blank page ($RealInk vs $EmptyInk); the drawing was not rendered."
    }
    Write-Host "Smoke test passed: the packaged binary converted a real DWG from a fresh directory."
}
finally {
    Remove-Item -Recurse -Force -LiteralPath $Sandbox -ErrorAction SilentlyContinue
}
```

- [ ] **Step 2: Run it against the package you already have**

Run:
```powershell
.\scripts\package.ps1
.\scripts\smoke-test.ps1 -Zip .\dist\Cadviewer-portable-win64.zip -SourceArchive .\third_party\libredwg-0.14.tar.xz
```
Expected: prints `content stream: empty control 64 bytes, real drawing 660 bytes` (numbers may differ slightly) then `Smoke test passed: ...`, exit 0.

- [ ] **Step 3: Prove the blank-page check fires — the control assertion**

Temporarily change the comparison line to `if ($RealInk -le $EmptyInk * 3000) {` and re-run Step 2.
Expected: throws `The DWG converted to an essentially blank page`.

Then **revert the line to `* 3`** and re-run Step 2 to confirm it passes again.

This proves the assertion can fail. Without it, a smoke test that only ever passes tells you nothing about whether it would notice a regression.

- [ ] **Step 4: Commit**

```bash
git add scripts/smoke-test.ps1
git commit -m "test(release): prove the packaged zip converts a real DWG after extraction"
```

---

### Task 3: Continuous integration on every push

**Files:**
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: a `CI` check on every push and pull request.

- [ ] **Step 1: Write `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: ['**']
  pull_request:

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

jobs:
  build:
    name: clippy + tests
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      # build.rs reads the commit for the version stamp; a shallow checkout
      # still has HEAD, which is all it needs.
      - name: Clippy
        run: cargo clippy --all-targets -- -D warnings

      - name: Tests
        run: cargo test
```

Note there is no `--offline` here: the runner has a network and an empty cargo cache on the first run, so it must be allowed to fetch. `--offline` is a local convenience for this machine only.

- [ ] **Step 2: Commit and push, then watch the run**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: run clippy and the test suite on every push"
git push origin text-shx
```

Open `https://github.com/jsun2020/Cadviewer/actions` and wait for the run to finish.
Expected: green. Tests that need the customer reference files print their skip message and pass.

- [ ] **Step 3: If the run fails, fix it before continuing**

The likely first-run failures and their causes:

- `error: linker 'link.exe' not found` — the toolchain action did not install MSVC. `windows-latest` ships it; re-run the job.
- A test that passes locally but fails on the runner is a genuine finding: it means the test depends on this machine's state (an installed font, a file on the Desktop). Fix the test to skip explicitly with a printed reason, the way the reference-drawing tests already do. Do not delete it.

---

### Task 4: The release workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `scripts/package.ps1`, `scripts/verify-release.ps1` (Task 1), `scripts/smoke-test.ps1` (Task 2).
- Produces: a published GitHub Release carrying `Cadviewer-portable-win64.zip` and `Cadviewer-portable-win64.zip.sha256`.

- [ ] **Step 1: Write `.github/workflows/release.yml`**

```yaml
name: Release

on:
  push:
    tags: ['v*']

permissions:
  contents: write

jobs:
  release:
    name: package and publish
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo
        uses: Swatinem/rust-cache@v2

      # Fails the release rather than shipping an untested build.
      - name: Tests
        run: cargo test

      # Downloads the LibreDWG runtime and its corresponding source, both
      # SHA-256 pinned, then builds, stages, gates and zips. The same script
      # a developer runs locally, so the release cannot drift from it.
      - name: Package
        run: .\scripts\package.ps1

      - name: Verify the version the artifact reports
        run: >
          .\scripts\verify-release.ps1
          -Tag "${{ github.ref_name }}"
          -ConvertExe ".\dist\Cadviewer-portable-win64\Cadconvert.exe"

      - name: Smoke test the packaged zip
        run: >
          .\scripts\smoke-test.ps1
          -Zip ".\dist\Cadviewer-portable-win64.zip"
          -SourceArchive ".\third_party\libredwg-0.14.tar.xz"

      - name: Checksum
        run: |
          $Hash = (Get-FileHash -Algorithm SHA256 .\dist\Cadviewer-portable-win64.zip).Hash.ToLowerInvariant()
          "$Hash  Cadviewer-portable-win64.zip" | Set-Content -Encoding ASCII .\dist\Cadviewer-portable-win64.zip.sha256
          Write-Host $Hash

      - name: Publish
        env:
          GH_TOKEN: ${{ github.token }}
        run: >
          gh release create "${{ github.ref_name }}"
          ".\dist\Cadviewer-portable-win64.zip"
          ".\dist\Cadviewer-portable-win64.zip.sha256"
          --title "Cadviewer ${{ github.ref_name }}"
          --notes-file .github/release-notes.md
          --generate-notes
```

- [ ] **Step 2: Write `.github/release-notes.md`**

```markdown
Windows 便携版，解压即可运行，无需安装。

- `Cadviewer.exe` — 图形界面查看器与导出器
- `Cadconvert.exe` — 命令行转换器：`Cadconvert.exe input.dwg output.pdf [--mono] [--sheet N]`

下载 `Cadviewer-portable-win64.zip`，解压后直接运行。`Cadviewer-portable-win64.zip.sha256`
可用于校验下载完整性。首次运行时 Windows SmartScreen 可能提示未知发布者——本程序未做
代码签名。

**不附带任何字库。** SHX 属 Autodesk 及第三方授权资产，TrueType 属微软，程序一律在运行时
查找机器上已安装的字库；缺字库时会在警告区逐条写明缺哪个、用了哪个、影响多少实体。

本程序按 GPL-3.0-or-later 发布，内含 GNU LibreDWG 0.14（同为 GPL-3.0-or-later）。
压缩包内 `source\` 目录附带 LibreDWG 对应源码与本程序源码，以履行 GPL 的源码提供义务。
```

- [ ] **Step 3: Verify the workflow file parses before tagging**

A YAML mistake is only discovered when the tag fires, and a tag is awkward to retract. Check it first:

```powershell
python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml', encoding='utf-8')); print('release.yml parses')"
python -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml', encoding='utf-8')); print('ci.yml parses')"
```
Expected: both print that they parse. If `yaml` is not installed, run `pip install pyyaml` first, or skip this step and accept that a syntax error costs one deleted tag.

- [ ] **Step 4: Commit and push**

```bash
git add .github/workflows/release.yml .github/release-notes.md
git commit -m "ci: publish a verified portable package when a version tag is pushed"
git push origin text-shx
```

- [ ] **Step 5: Cut the first release**

```bash
git tag v0.1.0
git push origin v0.1.0
```

Watch `https://github.com/jsun2020/Cadviewer/actions`.
Expected: the Release job runs the tests, packages, passes both gates, and publishes.

- [ ] **Step 6: Verify the published artifact by downloading it**

Do not trust the green tick — LL-005 and LL-032 are both about artifacts that looked fine and were not:

```powershell
$Temp = Join-Path $env:TEMP 'cadviewer-release-check'
New-Item -ItemType Directory -Force $Temp | Out-Null
Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/jsun2020/Cadviewer/releases/latest/download/Cadviewer-portable-win64.zip' -OutFile "$Temp\downloaded.zip"
$Bytes = [System.IO.File]::ReadAllBytes("$Temp\downloaded.zip")
"first four bytes: {0:X2} {1:X2} {2:X2} {3:X2}" -f $Bytes[0], $Bytes[1], $Bytes[2], $Bytes[3]
.\scripts\smoke-test.ps1 -Zip "$Temp\downloaded.zip" -SourceArchive .\third_party\libredwg-0.14.tar.xz
```

Expected: the first four bytes are `50 4B 03 04` (a real zip, not an HTML error page), and the smoke test passes against the *downloaded* file.

- [ ] **Step 7: If the release job failed, delete the tag before retrying**

```bash
git tag -d v0.1.0
git push origin :refs/tags/v0.1.0
```

Fix the cause, commit, then re-tag. Never leave a tag that has no release — that is the precise state LL-032 records.

---

### Task 5: A download path in the README

Until now the README goes straight from the feature list to build-from-source, so a visitor has no way to obtain the program without a Rust toolchain.

**Files:**
- Modify: `README.md`

**Interfaces:**
- Consumes: the release published in Task 4.
- Produces: nothing other tasks depend on.

- [ ] **Step 1: Add a download section**

Insert immediately above the `## 开发与构建` heading in `README.md`:

```markdown
## 下载

[**下载最新便携版**](https://github.com/jsun2020/Cadviewer/releases/latest)
— 解压 `Cadviewer-portable-win64.zip` 后直接运行 `Cadviewer.exe`，无需安装，
不写注册表。同页的 `.sha256` 可校验下载完整性。

首次运行时 Windows SmartScreen 可能提示未知发布者：本程序未做代码签名。

压缩包内已包含 DWG 解码所需的 LibreDWG 运行时；**不含任何字库**，
文字使用机器上已安装的 SHX / TrueType 字库绘制。
```

The link points at `releases/latest`, never at a version-stamped filename. A
README naming `Cadviewer-portable-win64-v0.1.0.zip` goes stale on the next
release and documents a download that no longer exists — LL-032's opening
symptom.

- [ ] **Step 2: Verify the link resolves**

Run:
```powershell
$Response = Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/jsun2020/Cadviewer/releases/latest' -MaximumRedirection 5
"status: $($Response.StatusCode)"
```
Expected: `status: 200`. A 404 means no release is published yet — finish Task 4 first.

- [ ] **Step 3: Commit and push**

```bash
git add README.md
git commit -m "docs: link the README at the latest release"
git push origin text-shx
```

---

## Self-review notes

Checked against `docs/superpowers/specs/2026-08-01-release-automation-design.md`:

- Triggers (spec §Triggers) — Tasks 3 and 4.
- Version guard with all three sources agreeing, and the case-insensitivity trap (spec §Version) — Task 1, steps 6-8.
- Smoke test against the extracted zip with a real DWG (spec §The smoke test) — Task 2.
- Existing font and icon gates kept (spec §Existing gates) — inherited by calling `package.ps1` unchanged in Task 4, step "Package".
- Release contents and notes (spec §Release contents) — Task 4, steps 1-2.
- README download path (spec §README) — Task 5.
- Permissions and token (spec §Permissions) — Task 4, step 1.

**One deviation from the spec, with evidence.** The spec's smoke test built its
DWG fixture by converting a generated DXF with `dxf2dwg.exe`. Measured on
2026-08-01: that produces a DWG containing **zero entities**, which renders as
a blank page — LibreDWG's DWG writer is its least mature component. The spec
anticipated this and required that a fallback be stated rather than silently
adopted. The fallback taken is *better* than the documented one: instead of
dropping to a DXF-only test that never loads `libredwg-0.dll`, the fixture is
now a real DWG from LibreDWG's own test suite, lifted from the source archive
already downloaded for the GPL offer and pinned by SHA-256. The DWG path is
therefore fully exercised, not unverified. Update the spec's §The smoke test
to match when this plan is executed.
