<#
.SYNOPSIS
Run the Windows functional tests against built josh binaries.

.DESCRIPTION
`josh compose` needs podman and does not run on Windows, so the .t suites are
out of reach there. These drive the built binaries directly: the CLI against a
local repository, and josh-proxy against a git server hosted by serve-git.ps1.

.PARAMETER BinDir
Directory holding the josh-proxy, josh and josh-filter binaries.

.PARAMETER PathForms
Also run the proxy tests with relative, space-laden and junction cache
directories.

.EXAMPLE
tests\windows\run.ps1 target\release
#>
param(
  [Parameter(Mandatory = $true)][string]$BinDir,
  [switch]$PathForms
)

$ErrorActionPreference = 'Stop'
# Suites run as child processes; their exit codes are data, not errors.
$PSNativeCommandUseErrorActionPreference = $false

. "$PSScriptRoot\lib.ps1"

function Start-GitServer([string]$here, [string]$root, [int]$port) {
  $argv = @('-NoProfile', '-File', "$here\serve-git.ps1", '-Root', $root, '-Port', $port)
  # -WindowStyle only exists on Windows.
  if ($IsWindows) { return Start-Process pwsh -PassThru -WindowStyle Hidden -ArgumentList $argv }
  return Start-Process pwsh -PassThru -ArgumentList $argv
}

$results = [ordered]@{}
function Invoke-Test([string]$name, [string]$script, [string[]]$scriptArgs, [hashtable]$extraEnv = @{}) {
  Write-Host "`n=== $name" -ForegroundColor Cyan
  foreach ($k in $extraEnv.Keys) { Set-Item "env:$k" $extraEnv[$k] }
  & pwsh -NoProfile -File $script @scriptArgs
  $results[$name] = ($LASTEXITCODE -eq 0)
}

function Write-Verdict($results) {
  Write-Host "`n=== verdict" -ForegroundColor Cyan
  foreach ($k in $results.Keys) {
    if ($results[$k]) { Write-Host "  PASS  $k" -ForegroundColor Green }
    else { Write-Host "  FAIL  $k" -ForegroundColor Red }
  }
}

$BinDir = (Resolve-Path $BinDir).Path
$here = $PSScriptRoot
$port = 8177

Invoke-Test 'cli' "$here\cli.ps1" @('-BinDir', $BinDir)

$served = Join-Path ([System.IO.Path]::GetTempPath()) "josh-served-$PID"
New-Item -ItemType Directory -Force -Path $served | Out-Null
$server = Start-GitServer $here $served $port

try {
  if (-not (Wait-TcpPort $port 10000)) { throw "git server did not start on port $port" }

  $proxy = Join-Path $BinDir 'josh-proxy.exe'
  if (-not (Test-Path $proxy)) { $proxy = Join-Path $BinDir 'josh-proxy' }
  if (-not (Test-Path $proxy)) { throw "josh-proxy not found in $BinDir" }
  $env:UPSTREAM_URL = "http://127.0.0.1:$port"

  Invoke-Test 'proxy' "$here\proxy.ps1" @('-Proxy', $proxy) @{ UPSTREAM_ROOT = $served }

  if ($PathForms) {
    $tmp = if ($env:TEMP) { $env:TEMP } else { [System.IO.Path]::GetTempPath() }
    foreach ($case in @(
      @{ name = 'proxy: relative cache path'; dir = './josh-rel' },
      @{ name = 'proxy: cache path with spaces'; dir = (Join-Path $tmp 'josh cache spaces') }
    )) {
      Invoke-Test $case.name "$here\proxy.ps1" @('-Proxy', $proxy, '-LocalDir', $case.dir) `
        @{ UPSTREAM_ROOT = $served }
    }
  }
} finally {
  if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
  Remove-Item -Recurse -Force $served -ErrorAction SilentlyContinue
}

Write-Verdict $results
if ($results.Values -contains $false) { exit 1 }
Write-Host "`nAll Windows functional tests passed." -ForegroundColor Green
