[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][string]$ConvertExe,
    # Optional so this script still runs stand-alone (e.g. a developer
    # checking a local build); the release workflow always supplies it.
    [string]$CommitSha
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

# A version-only check cannot catch the exact incident this guard exists to
# prevent: a binary built from a *different* commit at the *same* version
# (e.g. a rebuild that never ran, still reporting the previous, still
# matching, version). Requiring the shape "Cadviewer <version> (<hex>)"
# rejects that by construction along with the two other bad shapes: a
# missing revision ("(unknown)", the normal case for a source-zip build with
# no .git) and a dirty build (build.rs appends "+" to the revision). The
# version must be regex-escaped before it is interpolated -- it is a dotted
# string ("0.1.0"), and an unescaped "." matches any character.
$EscapedVersion = [regex]::Escape($ManifestVersion)
$StampMatch = [regex]::Match($Reported, "^Cadviewer $EscapedVersion \(([0-9a-f]{7,})\)$")
if (-not $StampMatch.Success) {
    if ($Reported -match '\+\)$') {
        # Name the offending file rather than leaving the developer to
        # guess. The most common cause is Cargo.lock left stale after a
        # version bump in Cargo.toml -- `cargo test`/`cargo build`
        # regenerate it, build.rs then stamps "+", and without this the
        # error points nowhere near that.
        $GitStatus = (& git -C $ProjectRoot status --porcelain | Out-String).Trim()
        throw "$ConvertExe was built from a dirty working tree ('$Reported'); a release must correspond to a commit. git status --porcelain:`n$GitStatus"
    }
    throw "$ConvertExe reports '$Reported', which does not match 'Cadviewer $ManifestVersion (<commit>)'. A missing or dirty revision must not pass."
}

# Independent of the two text-file checks above: ties the binary to the
# exact commit this workflow run checked out, not merely to a version
# string that could have been carried over from a stale build.
if ($PSBoundParameters.ContainsKey('CommitSha')) {
    $ReportedRevision = $StampMatch.Groups[1].Value
    if (-not $CommitSha.StartsWith($ReportedRevision, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$ConvertExe reports commit '$ReportedRevision', which is not a prefix of this run's commit '$CommitSha'. The binary was not built from the commit being released."
    }
}

Write-Host "Version agreed: tag $Tag, Cargo.toml $ManifestVersion, binary '$Reported'"
