<#
.SYNOPSIS
Package a release build of the josh CLI for WinGet.

.DESCRIPTION
Zips josh.exe with a SHA256 sidecar (same lowercase-hex format as the sccache
checksums CI consumes) and a version.txt for downstream manifest generation.
Expects a release build at the exe path: cargo build --release -p josh-cli
--bin josh.

.PARAMETER ExePath
Path to the josh.exe release binary.

.PARAMETER OutDir
Directory receiving the zip, its SHA256 sidecar and version.txt.

.PARAMETER Tag
Release tag to derive the version from (rNN.NN.NN). Defaults to the last
release tag reachable from HEAD, which is what the label-gated CI runs use;
release runs pass github.event.release.tag_name explicitly.

.EXAMPLE
pwsh packaging/winget/package.ps1 -Tag r26.09.15
#>
param(
  [string]$ExePath = 'target/release/josh.exe',
  [string]$OutDir = 'winget-out',
  [string]$Tag = (git describe --tags --abbrev=0)
)

$ErrorActionPreference = 'Stop'

# Turn 'rNN.NN.NN' into the winget package version 'NN.NN.NN'.
function Get-ReleaseVersion([string]$tag) {
  if (-not $tag) { throw 'could not determine release tag' }
  return $tag -replace '^r', ''
}

# Machine arch in target-triple form, as used by the sccache asset names.
function Get-MachineArch {
  $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
  return $arch
}

function Write-ReleasePackage([string]$exePath, [string]$name, [string]$outDir) {
  Compress-Archive -Path $exePath -DestinationPath "$name.zip"
  $hash = (Get-FileHash "$name.zip" -Algorithm SHA256).Hash.ToLower()
  Set-Content -NoNewline -Path "$name.zip.sha256" -Value $hash
  New-Item -ItemType Directory -Force -Path $outDir | Out-Null
  Move-Item "$name.zip", "$name.zip.sha256" -Destination $outDir
}

$version = Get-ReleaseVersion $Tag
$name = "josh-$version-$(Get-MachineArch)-pc-windows-msvc"
Write-ReleasePackage $ExePath $name $OutDir
Set-Content -NoNewline -Path "$OutDir/version.txt" -Value $version
