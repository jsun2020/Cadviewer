[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$ProjectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$RuntimeDir = Join-Path $ProjectRoot 'runtime'
$ThirdPartyDir = Join-Path $ProjectRoot 'third_party'
$CacheDir = Join-Path $ThirdPartyDir 'cache'
$Version = '0.14'
$ArchiveName = "libredwg-$Version-win64.zip"
$ArchivePath = Join-Path $CacheDir $ArchiveName
$SourceName = "libredwg-$Version.tar.xz"
$SourcePath = Join-Path $ThirdPartyDir $SourceName
$DownloadBase = "https://github.com/LibreDWG/libredwg/releases/download/$Version"
$ArchiveSha256 = '1ad7e15344d20b3426c3435b078d82fb84b35062815946b2cca9c5fc9810fea8'
$SourceSha256 = '62ebb73b984f865960f20ed26619ea5f8789d5e3fd088fa40a2598384da81275'

New-Item -ItemType Directory -Force -Path $RuntimeDir, $CacheDir | Out-Null

if ((-not (Test-Path -LiteralPath $ArchivePath)) -or
    ((Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash.ToLowerInvariant() -ne $ArchiveSha256)) {
    Invoke-WebRequest -UseBasicParsing -Uri "$DownloadBase/$ArchiveName" -OutFile $ArchivePath
}
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash.ToLowerInvariant() -ne $ArchiveSha256) {
    throw "LibreDWG Windows archive checksum mismatch"
}

$ExtractDir = Join-Path $CacheDir "libredwg-$Version-win64"
if (-not (Test-Path -LiteralPath (Join-Path $ExtractDir 'dwg2dxf.exe'))) {
    if (Test-Path -LiteralPath $ExtractDir) {
        $ResolvedExtract = [System.IO.Path]::GetFullPath($ExtractDir)
        if (-not $ResolvedExtract.StartsWith($CacheDir, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to clear unexpected path: $ResolvedExtract"
        }
        Remove-Item -Recurse -Force -LiteralPath $ResolvedExtract
    }
    Expand-Archive -LiteralPath $ArchivePath -DestinationPath $ExtractDir
}

Copy-Item -Force -LiteralPath (Join-Path $ExtractDir 'dwg2dxf.exe') -Destination $RuntimeDir
Copy-Item -Force -LiteralPath (Join-Path $ExtractDir 'libredwg-0.dll') -Destination $RuntimeDir

if ((-not (Test-Path -LiteralPath $SourcePath)) -or
    ((Get-FileHash -Algorithm SHA256 -LiteralPath $SourcePath).Hash.ToLowerInvariant() -ne $SourceSha256)) {
    Invoke-WebRequest -UseBasicParsing -Uri "$DownloadBase/$SourceName" -OutFile $SourcePath
}
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $SourcePath).Hash.ToLowerInvariant() -ne $SourceSha256) {
    throw "LibreDWG source archive checksum mismatch"
}

Write-Host "LibreDWG $Version runtime is ready in $RuntimeDir"
