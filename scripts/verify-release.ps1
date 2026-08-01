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
