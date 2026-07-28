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
Compress-Archive -Force -Path (Join-Path $SourceStage '*') -DestinationPath (Join-Path $PackageDir 'source\Cadviewer-source.zip')
Remove-Item -Recurse -Force -LiteralPath $SourceStage

$ZipPath = Join-Path $DistRoot 'Cadviewer-portable-win64.zip'
if (Test-Path -LiteralPath $ZipPath) {
    Remove-Item -Force -LiteralPath $ZipPath
}
Compress-Archive -Force -Path $PackageDir -DestinationPath $ZipPath
Write-Host "Portable package: $ZipPath"
