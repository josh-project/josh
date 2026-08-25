<#
.SYNOPSIS
Run the Windows functional tests against built josh binaries.

.DESCRIPTION
`josh compose` needs podman and does not run on Windows, so the .t suites are
out of reach there. These drive the built binaries directly: the CLI against a
local repository, and josh-proxy against a git server hosted by serve-git.ps1.

.PARAMETER BinDir
Directory holding josh-proxy.exe, josh.exe and josh-filter.exe.

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

$bash = @(
  "$env:ProgramFiles\Git\bin\bash.exe",
  "${env:ProgramFiles(x86)}\Git\bin\bash.exe",
  "$env:LOCALAPPDATA\Programs\Git\bin\bash.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $bash) { throw "Git Bash not found; install Git for Windows" }

$BinDir = (Resolve-Path $BinDir).Path
$here = $PSScriptRoot
$port = 8177

# Git Bash passes these to native tools, which want /c/... spellings.
function To-BashPath([string]$p) {
  $p = $p -replace '\\', '/'
  if ($p -match '^([A-Za-z]):(.*)$') { return "/$($Matches[1].ToLower())$($Matches[2])" }
  return $p
}

$results = [ordered]@{}
function Run-Test([string]$name, [string[]]$bashArgs, [hashtable]$extraEnv = @{}) {
  Write-Host "`n=== $name" -ForegroundColor Cyan
  foreach ($k in $extraEnv.Keys) { Set-Item "env:$k" $extraEnv[$k] }
  & $bash @bashArgs
  $results[$name] = ($LASTEXITCODE -eq 0)
}

Run-Test 'cli' @((To-BashPath "$here\cli.sh"), (To-BashPath $BinDir))

$served = Join-Path ([System.IO.Path]::GetTempPath()) "josh-served-$PID"
New-Item -ItemType Directory -Force -Path $served | Out-Null
$server = Start-Process pwsh -PassThru -WindowStyle Hidden -ArgumentList @(
  '-NoProfile', '-File', "$here\serve-git.ps1", '-Root', $served, '-Port', $port)

try {
  $ready = $false
  foreach ($i in 1..100) {
    try { (New-Object Net.Sockets.TcpClient('127.0.0.1', $port)).Close(); $ready = $true; break }
    catch { Start-Sleep -Milliseconds 100 }
  }
  if (-not $ready) { throw "git server did not start on port $port" }

  $proxy = Join-Path $BinDir 'josh-proxy.exe'
  $env:UPSTREAM_URL = "http://127.0.0.1:$port"

  Run-Test 'proxy' @((To-BashPath "$here\proxy.sh"), (To-BashPath $proxy)) `
    @{ UPSTREAM_ROOT = (To-BashPath $served) }

  if ($PathForms) {
    $tmp = $env:TEMP
    foreach ($case in @(
      @{ name = 'proxy: relative cache path'; dir = './josh-rel' },
      @{ name = 'proxy: cache path with spaces'; dir = (To-BashPath (Join-Path $tmp 'josh cache spaces')) }
    )) {
      Run-Test $case.name @((To-BashPath "$here\proxy.sh"), (To-BashPath $proxy), $case.dir) `
        @{ UPSTREAM_ROOT = (To-BashPath $served) }
    }
  }
} finally {
  if ($server -and -not $server.HasExited) { Stop-Process -Id $server.Id -Force }
  Remove-Item -Recurse -Force $served -ErrorAction SilentlyContinue
}

Write-Host "`n=== verdict" -ForegroundColor Cyan
foreach ($k in $results.Keys) {
  if ($results[$k]) { Write-Host "  PASS  $k" -ForegroundColor Green }
  else { Write-Host "  FAIL  $k" -ForegroundColor Red }
}
if ($results.Values -contains $false) { exit 1 }
Write-Host "`nAll Windows functional tests passed." -ForegroundColor Green
