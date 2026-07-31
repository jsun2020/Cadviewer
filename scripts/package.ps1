[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$ProjectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$DistRoot = Join-Path $ProjectRoot 'dist'
$PackageDir = Join-Path $DistRoot 'Cadviewer-portable-win64'
$SourceStage = Join-Path $env:TEMP 'Cadviewer-source-stage'

& (Join-Path $PSScriptRoot 'prepare-libredwg.ps1')
if (-not $SkipBuild) {
    Push-Location $ProjectRoot
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

if (Test-Path -LiteralPath $PackageDir) {
    $ResolvedPackage = [System.IO.Path]::GetFullPath($PackageDir)
    if (-not $ResolvedPackage.StartsWith($DistRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clear unexpected path: $ResolvedPackage"
    }
    Remove-Item -Recurse -Force -LiteralPath $ResolvedPackage
}
New-Item -ItemType Directory -Force -Path (Join-Path $PackageDir 'runtime'), (Join-Path $PackageDir 'source') | Out-Null

Copy-Item -LiteralPath (Join-Path $ProjectRoot 'target\release\cadviewer.exe') -Destination (Join-Path $PackageDir 'Cadviewer.exe')
Copy-Item -LiteralPath (Join-Path $ProjectRoot 'target\release\cadconvert.exe') -Destination (Join-Path $PackageDir 'Cadconvert.exe')
Copy-Item -LiteralPath (Join-Path $ProjectRoot 'runtime\dwg2dxf.exe') -Destination (Join-Path $PackageDir 'runtime')
Copy-Item -LiteralPath (Join-Path $ProjectRoot 'runtime\libredwg-0.dll') -Destination (Join-Path $PackageDir 'runtime')
Copy-Item -LiteralPath (Join-Path $ProjectRoot 'README.md') -Destination $PackageDir
Copy-Item -LiteralPath (Join-Path $ProjectRoot 'THIRD_PARTY_NOTICES.md') -Destination $PackageDir
Copy-Item -LiteralPath (Join-Path $ProjectRoot 'LICENSE') -Destination $PackageDir
Copy-Item -LiteralPath (Join-Path $ProjectRoot 'third_party\libredwg-0.14.tar.xz') -Destination (Join-Path $PackageDir 'source')

if (Test-Path -LiteralPath $SourceStage) {
    $ResolvedStage = [System.IO.Path]::GetFullPath($SourceStage)
    $ResolvedTemp = [System.IO.Path]::GetFullPath($env:TEMP)
    if (-not $ResolvedStage.StartsWith($ResolvedTemp, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to clear unexpected path: $ResolvedStage"
    }
    Remove-Item -Recurse -Force -LiteralPath $ResolvedStage
}
New-Item -ItemType Directory -Force -Path $SourceStage | Out-Null
Copy-Item -Recurse -LiteralPath (Join-Path $ProjectRoot 'src') -Destination $SourceStage
Copy-Item -Recurse -LiteralPath (Join-Path $ProjectRoot 'scripts') -Destination $SourceStage
foreach ($File in 'Cargo.toml','Cargo.lock','README.md','THIRD_PARTY_NOTICES.md','LICENSE') {
    Copy-Item -LiteralPath (Join-Path $ProjectRoot $File) -Destination $SourceStage
}
# R-TXT-5.3: SHX fonts are Autodesk/third-party licensed assets and TTFs are
# Microsoft's. Every font this program uses is located on the user's machine
# at runtime; none is ever shipped. Assert that, rather than trusting nobody
# copied one in.
function Get-FontFiles([string]$Root) {
    @(Get-ChildItem -Path $Root -Recurse -File |
        Where-Object { $_.Extension -match '^\.(shx|ttf|ttc|otf)$' })
}

function Assert-NoFonts([string]$Root, [string]$What) {
    $Forbidden = Get-FontFiles $Root
    if ($Forbidden) {
        throw "Refusing to package font files into ${What}: $($Forbidden.FullName -join ', ')"
    }

    # Control assertion: a check that cannot fire is not a check. Plant a file
    # the gate must catch, confirm it does, then remove it. LL-032/LL-033 both
    # shipped leak gates that silently matched nothing and read as "clean".
    $Canary = Join-Path $Root 'canary.shx'
    Set-Content -LiteralPath $Canary -Value 'x'
    $Caught = (Get-FontFiles $Root).Count
    Remove-Item -Force -LiteralPath $Canary
    if ($Caught -eq 0) {
        throw "The font leak gate did not fire on a planted .shx in ${What}; the check is vacuous."
    }
}

# Checked before each archive is written, not after: a gate that throws once
# the zip already exists leaves the leaking artifact sitting on disk.
Assert-NoFonts $SourceStage 'the source zip'
Compress-Archive -Force -Path (Join-Path $SourceStage '*') -DestinationPath (Join-Path $PackageDir 'source\Cadviewer-source.zip')
Remove-Item -Recurse -Force -LiteralPath $SourceStage

$ZipPath = Join-Path $DistRoot 'Cadviewer-portable-win64.zip'
if (Test-Path -LiteralPath $ZipPath) {
    Remove-Item -Force -LiteralPath $ZipPath
}
Assert-NoFonts $PackageDir 'the portable package'
Compress-Archive -Force -Path $PackageDir -DestinationPath $ZipPath
Write-Host "Portable package: $ZipPath"
