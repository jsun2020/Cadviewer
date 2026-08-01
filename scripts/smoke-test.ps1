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

# locate_converter() in src/converter.rs checks CADVIEWER_LIBREDWG before it
# ever looks in runtime\. If that variable happens to be set in this
# session, Cadconvert.exe would silently validate a LibreDWG outside the
# extracted zip, and this script would stop testing what a user downloads.
$SavedLibredwgEnv = $env:CADVIEWER_LIBREDWG
Remove-Item Env:\CADVIEWER_LIBREDWG -ErrorAction SilentlyContinue
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

        # Measure the PAGE's content stream specifically, not the largest
        # /Length in the whole file. This project embeds TrueType font
        # subsets as separate FontFile2 objects with their own /Length,
        # which can run to kilobytes; a document-wide maximum would
        # silently start measuring embedded font data instead of drawn
        # ink the moment a fixture contains text, and a gate that stops
        # discriminating without anyone noticing is exactly the failure
        # mode this project has already shipped twice (see CLAUDE.md
        # Lessons Learned). Object headers are anchored at line start and
        # names need no space before another name (a bare '/' already
        # ends the token), so '/Type/Page' and '/Type /Page' both match.
        $Options = [System.Text.RegularExpressions.RegexOptions]'Singleline, Multiline'
        $PageMatch = [regex]::Match(
            $Text,
            '^\d+\s+0\s+obj\s*<<(?:(?!endobj).)*?/Type\s*/Page(?!s)(?:(?!endobj).)*?/Contents\s+(\d+)\s+0\s+R',
            $Options)
        if (-not $PageMatch.Success) {
            throw "$What has no /Type /Page object with a resolvable /Contents reference"
        }
        $ContentsObj = $PageMatch.Groups[1].Value
        $StreamMatch = [regex]::Match(
            $Text,
            "^$ContentsObj\s+0\s+obj\s*<<(?:(?!endobj).)*?/Length\s+(\d+)",
            $Options)
        if (-not $StreamMatch.Success) {
            throw "$What's page /Contents points at object $ContentsObj, which has no resolvable /Length"
        }
        return [int]$StreamMatch.Groups[1].Value
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
    if ($null -ne $SavedLibredwgEnv) {
        $env:CADVIEWER_LIBREDWG = $SavedLibredwgEnv
    }
    Remove-Item -Recurse -Force -LiteralPath $Sandbox -ErrorAction SilentlyContinue
}
