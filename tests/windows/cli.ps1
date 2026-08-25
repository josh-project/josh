<#
.SYNOPSIS
Functional test for the josh CLI on a platform where `josh compose` cannot run.

.DESCRIPTION
Exercises filtering, cloning, pulling and pushing against a local bare
repository: no server, no network, nothing but git.

  tests/windows/cli.ps1 -BinDir <dir-containing-josh-and-josh-filter>

The clone deliberately targets a relative directory, which is what turns a
path into a remote URL internally — the case that was broken on Windows.

.PARAMETER BinDir
Directory holding the josh and josh-filter binaries.
#>
param([Parameter(Mandatory = $true)][string]$BinDir)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

. "$PSScriptRoot\lib.ps1"
# Write test files with explicit LF newlines ([IO.File], not Set-Content):
# bash's echo produced LF everywhere; Set-Content would emit CRLF on Windows.

$BinDir = (Resolve-Path $BinDir).Path
$exe = if (Test-Path (Join-Path $BinDir 'josh.exe')) { '.exe' } else { '' }
$Josh = Join-Path $BinDir "josh$exe"
$JoshFilter = Join-Path $BinDir "josh-filter$exe"
if (-not (Test-Path $Josh)) { Fail "$Josh not found" }
if (-not (Test-Path $JoshFilter)) { Fail "$JoshFilter not found" }

$Branch = 'main'
$Work = $null
$C3 = $null

function Initialize-Upstream() {
  Write-Host '== setup: local upstream'
  $script:Work = New-TestTempDir 'josh-cli'
  $upstream = "$script:Work/upstream.git"
  git init -q --bare -b $Branch $upstream
  $seed = "$script:Work/seed"
  git init -q -b $Branch $seed
  git -C $seed config user.email 't@t'
  git -C $seed config user.name t
  [IO.File]::WriteAllText("$seed/README.md", "hello`n")
  git -C $seed add .
  git -C $seed commit -qm 'c1: readme'
  New-Item -ItemType Directory -Force -Path "$seed/src" | Out-Null
  [IO.File]::WriteAllText("$seed/src/lib.txt", "lib`n")
  git -C $seed add .
  git -C $seed commit -qm 'c2: lib'
  git -C $seed push -q $upstream $Branch
}

function Test-Filter() {
  Write-Host '== josh-filter'
  $plain = "$script:Work/plain"
  git clone -q "$script:Work/upstream.git" $plain
  Push-Location $plain
  try {
    try { & $JoshFilter ':prefix=lib' $Branch } catch { Fail 'josh-filter' }
  } finally {
    Pop-Location
  }
  $tree = git -C $plain ls-tree --name-only -r FILTERED_HEAD
  if (-not ($tree | Where-Object { $_ -ceq 'lib/README.md' })) {
    Fail 'josh-filter: prefix missing from FILTERED_HEAD'
  }
  if ((git -C $plain rev-list --count FILTERED_HEAD) -ne '2') {
    Fail 'josh-filter: expected 2 commits'
  }
}

function Test-Clone() {
  Write-Host '== josh clone, into a relative directory'
  $cliDir = "$script:Work/cli"
  New-Item -ItemType Directory -Force -Path $cliDir | Out-Null
  Push-Location $cliDir
  try {
    try { & $Josh clone "$script:Work/upstream.git" ':prefix=lib' './clone' } catch { Fail 'josh clone' }
  } finally {
    Pop-Location
  }
  if (-not (Test-Path "$cliDir/clone/lib/README.md")) { Fail 'josh clone: prefix missing' }
  if ((git -C "$cliDir/clone" rev-list --count HEAD) -ne '2') {
    Fail 'josh clone: expected 2 commits'
  }
}

function Test-ChangesPull() {
  Write-Host '== josh changes pull'
  [IO.File]::AppendAllText("$script:Work/seed/src/lib.txt", "more`n")
  git -C "$script:Work/seed" commit -qam 'c3: more lib'
  $script:C3 = git -C "$script:Work/seed" rev-parse HEAD
  git -C "$script:Work/seed" push -q "$script:Work/upstream.git" $Branch
  Push-Location "$script:Work/cli/clone"
  try {
    try { & $Josh changes pull } catch { Fail 'josh changes pull' }
  } finally {
    Pop-Location
  }
  if (-not (Select-String -Path "$script:Work/cli/clone/lib/src/lib.txt" -SimpleMatch 'more' -Quiet)) {
    Fail 'josh changes pull: upstream change did not arrive through the filter'
  }
}

function Test-Push() {
  Write-Host '== josh push'
  $clone = "$script:Work/cli/clone"
  Push-Location $clone
  try {
    git config user.email 't@t'
    git config user.name t
    [IO.File]::AppendAllText("$clone/lib/src/lib.txt", "change`n")
    git commit -qam 'c4: change through the filter'
    try { & $Josh push origin 'HEAD:refs/heads/roundtrip' --base $Branch } catch { Fail 'josh push' }
  } finally {
    Pop-Location
  }
  $upstream = "$script:Work/upstream.git"
  try { $rt = git -C $upstream rev-parse refs/heads/roundtrip } catch { Fail 'josh push: branch missing upstream' }
  $content = git -C $upstream show "${rt}:src/lib.txt" | Out-String
  if (-not $content.Contains('change')) {
    Fail 'josh push: change not reverse-filtered to src/lib.txt'
  }
  if ((git -C $upstream rev-parse "$rt^") -ne $script:C3) {
    Fail 'josh push: pushed commit is not rooted on the upstream tip'
  }
}

try {
  Initialize-Upstream
  Test-Filter
  Test-Clone
  Test-ChangesPull
  Test-Push
  Write-Host 'PASS'
} finally {
  if ($Work -and (Test-Path $Work)) { Remove-Item -Recurse -Force $Work }
}
