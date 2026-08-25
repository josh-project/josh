<#
.SYNOPSIS
Shared helpers for the Windows functional tests.

.DESCRIPTION
Dot-sourced by run.ps1, cli.ps1 and proxy.ps1; not run directly.
#>

$ErrorActionPreference = 'Stop'

function Fail([string]$msg) {
  [Console]::Error.WriteLine("FAIL: $msg")
  exit 1
}

function New-TestTempDir([string]$prefix) {
  $base = if ($env:TEMP) { $env:TEMP } else { [System.IO.Path]::GetTempPath() }
  $dir = Join-Path $base "$prefix-$([Guid]::NewGuid().ToString('N').Substring(0, 8))"
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  return $dir
}

function Wait-TcpPort([int]$port, [int]$timeoutMs) {
  foreach ($i in 1..([Math]::Max(1, [int]($timeoutMs / 100)))) {
    $client = New-Object Net.Sockets.TcpClient
    try {
      $client.Connect('127.0.0.1', $port)
      return $true
    } catch {
      Start-Sleep -Milliseconds 100
    } finally {
      $client.Close()
    }
  }
  return $false
}
