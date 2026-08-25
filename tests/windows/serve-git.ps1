<#
.SYNOPSIS
Serve bare git repositories over smart HTTP, for tests.

.DESCRIPTION
josh-proxy only accepts an http(s) or ssh upstream, so testing it needs a git
server. Rather than add a dependency, this hosts git's own http-backend as CGI
behind System.Net.HttpListener, which ships with Windows.

Prints the URL it is serving on, then runs until stopped.

.PARAMETER Root
Directory holding bare repositories (GIT_PROJECT_ROOT).

.PARAMETER Port
Port to listen on. 127.0.0.1 only.
#>
param(
  [Parameter(Mandatory = $true)][string]$Root,
  [int]$Port = 8177
)

$ErrorActionPreference = 'Stop'

$git = (Get-Command git).Source
$root = (Resolve-Path $Root).Path

# Configure one git http-backend invocation as a CGI child for $request.
function New-BackendProcess($git, $root, $request) {
  $psi = [System.Diagnostics.ProcessStartInfo]::new($git, 'http-backend')
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true

  $psi.Environment['GIT_PROJECT_ROOT'] = $root
  $psi.Environment['GIT_HTTP_EXPORT_ALL'] = '1'
  $psi.Environment['REQUEST_METHOD'] = $request.HttpMethod
  $psi.Environment['PATH_INFO'] = [Uri]::UnescapeDataString($request.Url.AbsolutePath)
  $psi.Environment['QUERY_STRING'] = $request.Url.Query.TrimStart('?')
  $psi.Environment['REMOTE_ADDR'] = $request.RemoteEndPoint.Address.ToString()
  $psi.Environment['REMOTE_USER'] = 'test'
  if ($request.ContentType) { $psi.Environment['CONTENT_TYPE'] = $request.ContentType }
  if ($request.ContentLength64 -ge 0) { $psi.Environment['CONTENT_LENGTH'] = "$($request.ContentLength64)" }
  # http-backend inflates the body itself when told the encoding, and serves
  # protocol v2 only when the client's version is passed through.
  if ($request.Headers['Content-Encoding']) {
    $psi.Environment['HTTP_CONTENT_ENCODING'] = $request.Headers['Content-Encoding']
  }
  if ($request.Headers['Git-Protocol']) {
    $psi.Environment['GIT_PROTOCOL'] = $request.Headers['Git-Protocol']
  }
  return $psi
}

# Run the CGI child: stream the request body in, buffer the whole reply out.
# git speaks binary over HTTP: every copy is bytes, and stderr is drained on
# its own so a chatty backend cannot fill the pipe and block.
function Invoke-Backend($psi, $request) {
  $process = [System.Diagnostics.Process]::Start($psi)
  try {
    $stderrTask = $process.StandardError.ReadToEndAsync()
    if ($request.HasEntityBody) {
      $request.InputStream.CopyTo($process.StandardInput.BaseStream)
    }
    $process.StandardInput.Close()

    $captured = New-Object System.IO.MemoryStream
    $process.StandardOutput.BaseStream.CopyTo($captured)
    $process.WaitForExit()
    return @{ Bytes = $captured.ToArray(); Stderr = $stderrTask.Result }
  } finally {
    $process.Dispose()
  }
}

# CGI replies with headers, a blank line, then the body. Buffering the whole
# reply keeps the body's bytes intact and lets the response carry a real
# Content-Length, which the git client needs to know where it ends.
function Split-CgiReply([byte[]]$bytes) {
  $split = -1
  for ($i = 0; $i -lt $bytes.Length - 1; $i++) {
    if ($bytes[$i] -eq 10 -and $bytes[$i + 1] -eq 10) { $split = $i + 2; break }
    if ($i -lt $bytes.Length - 3 -and $bytes[$i] -eq 13 -and $bytes[$i + 1] -eq 10 `
        -and $bytes[$i + 2] -eq 13 -and $bytes[$i + 3] -eq 10) { $split = $i + 4; break }
  }
  if ($split -lt 0) { return $null }

  $body = New-Object byte[] ($bytes.Length - $split)
  [Array]::Copy($bytes, $split, $body, 0, $body.Length)
  return @{
    HeaderText = [System.Text.Encoding]::ASCII.GetString($bytes, 0, $split)
    Body       = $body
  }
}

# Map the CGI headers onto the HTTP response and send the buffered body.
function Send-Response($response, $headerText, [byte[]]$body) {
  $response.StatusCode = 200
  foreach ($line in ($headerText -split "`r?`n")) {
    if (-not $line) { continue }
    $name, $value = $line -split ':\s*', 2
    switch ($name) {
      'Status'         { $response.StatusCode = [int]($value -split ' ')[0] }
      'Content-Type'   { $response.ContentType = $value }
      'Content-Length' { }  # taken from the body below
      default          { try { $response.Headers[$name] = $value } catch { } }
    }
  }

  $response.SendChunked = $false
  $response.KeepAlive = $false
  $response.ContentLength64 = $body.Length
  if ($body.Length -gt 0) { $response.OutputStream.Write($body, 0, $body.Length) }
  $response.OutputStream.Close()
}

$listener = [System.Net.HttpListener]::new()
$listener.Prefixes.Add("http://127.0.0.1:$Port/")
$listener.Start()
Write-Host "serving $root on http://127.0.0.1:$Port/"

try {
  while ($listener.IsListening) {
    $context = $listener.GetContext()
    $request = $context.Request
    $response = $context.Response

    $result = Invoke-Backend (New-BackendProcess $git $root $request) $request
    $reply = Split-CgiReply $result.Bytes
    if ($null -eq $reply) {
      $response.StatusCode = 500
      $message = [System.Text.Encoding]::UTF8.GetBytes("no CGI reply from git http-backend`n$($result.Stderr)")
      $response.ContentLength64 = $message.Length
      $response.OutputStream.Write($message, 0, $message.Length)
      $response.OutputStream.Close()
      continue
    }

    Send-Response $response $reply.HeaderText $reply.Body
  }
} finally {
  $listener.Stop()
}
