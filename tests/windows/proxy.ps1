<#
.SYNOPSIS
Functional test for josh-proxy on a platform where `josh compose` cannot run.

.DESCRIPTION
Drives a real proxy against a real upstream: filtered clone, pinned-SHA fetch,
reverse-filter push, and reuse of the --local cache across a restart.

  $env:UPSTREAM_URL = 'http://127.0.0.1:8177'
  tests/windows/proxy.ps1 -Proxy <josh-proxy> [-LocalDir <cache-dir>]

UPSTREAM_URL is the base URL of a git server exporting the repositories in
UPSTREAM_ROOT (tests/windows/serve-git.ps1 provides one). The optional cache
directory lets a caller exercise unusual path forms.

.PARAMETER Proxy
Path to the josh-proxy binary.

.PARAMETER LocalDir
Cache directory for --local; defaults to a directory under the temp work dir.
#>
param(
  [Parameter(Mandatory = $true)][string]$Proxy,
  [string]$LocalDir
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

. "$PSScriptRoot\lib.ps1"
# Write test files with explicit LF newlines ([IO.File], not Set-Content):
# bash's echo produced LF everywhere; Set-Content would emit CRLF on Windows.

if (-not $env:UPSTREAM_ROOT) { throw 'set UPSTREAM_ROOT to the served directory' }
if (-not $env:UPSTREAM_URL) { throw 'set UPSTREAM_URL to the serving base URL' }
$UpstreamRoot = $env:UPSTREAM_ROOT
$UpstreamUrl = $env:UPSTREAM_URL
$Port = if ($env:JOSH_PORT) { [int]$env:JOSH_PORT } else { 42190 }
$Branch = 'main'

$Work = New-TestTempDir 'josh-proxy'
if (-not $LocalDir) { $LocalDir = Join-Path $Work 'local' }
$UpstreamGit = Join-Path $UpstreamRoot 'upstream.git'
$OutLog = Join-Path $Work 'josh.out.log'
$ErrLog = Join-Path $Work 'josh.err.log'
$C1 = $null
$C2 = $null
$ProxyProc = $null

# Like lib's Fail, but dumps the proxy logs first.
function Fail([string]$msg) {
  [Console]::Error.WriteLine("FAIL: $msg")
  foreach ($log in @($OutLog, $ErrLog)) {
    if ((Test-Path $log) -and (Get-Item $log).Length -gt 0) {
      [Console]::Error.WriteLine("--- josh-proxy log ($log):")
      Get-Content $log | ForEach-Object { [Console]::Error.WriteLine($_) }
    }
  }
  exit 1
}

function Start-Proxy() {
  # Start-Process joins -ArgumentList without quoting: quote whitespace ourselves.
  $argv = @('--local', $LocalDir, '--remote', $UpstreamUrl, "--port=$Port", '--no-background') |
    ForEach-Object { if ($_ -match '\s') { '"{0}"' -f $_ } else { $_ } }
  $script:ProxyProc = Start-Process -PassThru -FilePath $Proxy -ArgumentList $argv `
    -RedirectStandardOutput $OutLog -RedirectStandardError $ErrLog
  if (Wait-TcpPort $Port 10000) { return }
  Fail 'josh-proxy did not start'
}

function Stop-Proxy() {
  if (-not $script:ProxyProc) { return }
  Stop-Process -Id $script:ProxyProc.Id -ErrorAction SilentlyContinue
  if (-not $script:ProxyProc.WaitForExit(2000)) { Fail 'josh-proxy did not exit when terminated' }
  $script:ProxyProc = $null
}

function Initialize-Upstream() {
  Write-Host '== setup: upstream repository'
  if (Test-Path $UpstreamGit) { Remove-Item -Recurse -Force $UpstreamGit }
  git init -q --bare -b $Branch $UpstreamGit
  git -C $UpstreamGit config http.receivepack true
  $seed = "$Work/seed"
  git init -q -b $Branch $seed
  git -C $seed config user.email 't@t'
  git -C $seed config user.name t
  [IO.File]::WriteAllText("$seed/README.md", "hello`n")
  git -C $seed add .
  git -C $seed commit -qm 'c1: readme'
  $script:C1 = git -C $seed rev-parse HEAD
  New-Item -ItemType Directory -Force -Path "$seed/src" | Out-Null
  [IO.File]::WriteAllText("$seed/src/lib.txt", "lib`n")
  git -C $seed add .
  git -C $seed commit -qm 'c2: lib'
  $script:C2 = git -C $seed rev-parse HEAD
  git -C $seed push -q $UpstreamGit $Branch
}

function Test-FilteredClone() {
  Write-Host '== filtered clone'
  $filtered = "http://127.0.0.1:${Port}/upstream.git:prefix=lib.git"
  try { git clone -q $filtered "$Work/clone" } catch { Fail 'filtered clone' }
  if (-not (Test-Path "$Work/clone/lib/README.md")) { Fail 'prefix missing from the clone' }
  if ((git -C "$Work/clone" rev-list --count HEAD) -ne '2') { Fail 'expected 2 commits' }
}

function Test-PinnedFetch() {
  Write-Host '== pinned-SHA fetch'
  # The filter separator is also exercised percent-encoded, as clients send it.
  $url = "http://127.0.0.1:${Port}/upstream.git@$C1%3Aprefix=lib.git"
  try { git -C "$Work/clone" fetch -q $url HEAD } catch { Fail 'pinned fetch' }
  $tree = git -C "$Work/clone" ls-tree --name-only -r FETCH_HEAD
  if (-not ($tree | Where-Object { $_ -ceq 'lib/README.md' })) { Fail 'pinned fetch: README missing' }
  if ($tree | Select-String -SimpleMatch 'lib/src' -Quiet) {
    Fail 'pinned fetch resolved past the pinned commit'
  }
}

function Test-ReversePush() {
  Write-Host '== reverse push'
  $clone = "$Work/clone"
  git -C $clone config user.email 't@t'
  git -C $clone config user.name t
  [IO.File]::AppendAllText("$clone/lib/src/lib.txt", "change`n")
  git -C $clone commit -qam 'c3: change through the filter'
  try {
    git -C $clone push -q -o "base=refs/heads/$Branch" origin 'HEAD:refs/heads/roundtrip'
  } catch { Fail 'reverse push' }
  try { $rt = git -C $UpstreamGit rev-parse refs/heads/roundtrip } catch {
    Fail 'reverse push: branch missing upstream'
  }
  $content = git -C $UpstreamGit show "${rt}:src/lib.txt" | Out-String
  if (-not $content.Contains('change')) {
    Fail 'reverse push: change not reverse-filtered to src/lib.txt'
  }
  if ((git -C $UpstreamGit rev-parse "$rt^") -ne $script:C2) {
    Fail 'reverse push: pushed commit is not rooted on the upstream tip'
  }
}

function Test-CacheReuse() {
  Write-Host '== cache reuse across a restart'
  # Consumers run one proxy per operation rather than a daemon, so the --local
  # cache has to survive a clean stop and serve the next instance.
  Stop-Proxy
  Start-Proxy
  try { git -C "$Work/clone" fetch -q origin } catch { Fail 'fetch against the reused cache' }
}

try {
  Initialize-Upstream
  Write-Host '== boot'
  Start-Proxy
  Test-FilteredClone
  Test-PinnedFetch
  Test-ReversePush
  Test-CacheReuse
  Write-Host 'PASS'
} finally {
  Stop-Proxy
  if (Test-Path $Work) { Remove-Item -Recurse -Force $Work }
}
