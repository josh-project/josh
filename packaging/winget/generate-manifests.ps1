<#
.SYNOPSIS
Assemble the winget-pkgs manifest set for the josh CLI.

.DESCRIPTION
Builds the version, installer and defaultLocale manifests from the
per-architecture zips produced by package.ps1, laid out as winget-pkgs
expects: <OutDir>/j/JoshProject/Josh/<version>/. The installer URLs point at
GitHub release assets that only need to exist once the manifest is actually
submitted: winget validate and everything in CI before that work purely
offline against the local zips.

.PARAMETER PackageDir
Directory with the package.ps1 outputs: josh-<version>-*.zip zips and
version.txt.

.PARAMETER OutDir
Root of the manifest tree.

.EXAMPLE
pwsh packaging/winget/generate-manifests.ps1
#>
param(
  [string]$PackageDir = 'winget-package',
  [string]$OutDir = 'manifests'
)

$ErrorActionPreference = 'Stop'

function Get-PackageVersion([string]$packageDir) {
  return (Get-Content "$packageDir/version.txt" -Raw).Trim()
}

# Installer entries for the packaged zips; maps the target-triple arch to
# winget's names ('x86_64' -> 'x64').
function Get-InstallerEntries([string]$packageDir, [string]$version, [string]$releaseUrl) {
  $entries = @()
  foreach ($zip in Get-ChildItem "$packageDir/josh-$version-*.zip") {
    $arch = if ($zip.Name -like '*x86_64*') { 'x64' } else { 'arm64' }
    $hash = (Get-FileHash $zip.FullName -Algorithm SHA256).Hash
    $entries += "- Architecture: $arch`n  InstallerUrl: $releaseUrl/$($zip.Name)`n  InstallerSha256: $hash"
  }
  if ($entries.Count -eq 0) { throw "no josh-$version-*.zip zips found in $packageDir" }
  return $entries -join "`n"
}

function Write-VersionManifest([string]$path, [string]$version) {
  @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.version.1.12.0.schema.json

PackageIdentifier: JoshProject.Josh
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.12.0
"@ | Set-Content $path
}

function Write-InstallerManifest([string]$path, [string]$version, [string]$installers) {
  @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.installer.1.12.0.schema.json

PackageIdentifier: JoshProject.Josh
PackageVersion: $version
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
- RelativeFilePath: josh.exe
ReleaseDate: $(Get-Date -Format yyyy-MM-dd)
# josh shells out to git (ls-remote, patch-id, ...) so it must be on PATH.
Dependencies:
  PackageDependencies:
  - PackageIdentifier: Git.Git
Installers:
$installers
ManifestType: installer
ManifestVersion: 1.12.0
"@ | Set-Content $path
}

function Write-LocaleManifest([string]$path, [string]$version, [string]$tagUrl) {
  @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.defaultLocale.1.12.0.schema.json

PackageIdentifier: JoshProject.Josh
PackageVersion: $version
PackageLocale: en-US
Publisher: Josh Project authors
PublisherUrl: https://github.com/josh-project
PackageName: Josh
PackageUrl: https://github.com/josh-project/josh
License: MIT
LicenseUrl: https://github.com/josh-project/josh/blob/master/LICENSE
Copyright: Copyright (c) 2022-2026 Josh Project
Moniker: josh
ShortDescription: Work with and transform Git repos locally
Tags:
- git
- monorepo
ReleaseNotesUrl: $tagUrl
ManifestType: defaultLocale
ManifestVersion: 1.12.0
"@ | Set-Content $path
}

$version = Get-PackageVersion $PackageDir
$releaseUrl = "https://github.com/josh-project/josh/releases/download/r$version"
$tagUrl = "https://github.com/josh-project/josh/releases/tag/r$version"
$installers = Get-InstallerEntries $PackageDir $version $releaseUrl

$dir = Join-Path $OutDir "j/JoshProject/Josh/$version"
New-Item -ItemType Directory -Force -Path $dir | Out-Null

Write-VersionManifest "$dir/JoshProject.Josh.yaml" $version
Write-InstallerManifest "$dir/JoshProject.Josh.installer.yaml" $version $installers
Write-LocaleManifest "$dir/JoshProject.Josh.locale.en-US.yaml" $version $tagUrl
Get-ChildItem $dir
