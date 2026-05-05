# Overnight fuzz wrapper — see wrk_journals/2026.05.04 - JRN - overnight-fuzz-soak.md
#
# Loops `cargo +nightly fuzz run sip_message_roundtrip` until the cumulative
# fuzz budget (default 8h) is exhausted. Pauses on crash until a RESUME file
# is touched by the supervisor; wall-clock pause time does NOT count against
# the budget.
#
# stdout grammar (one event per line, all space-separated):
#   START budget=<sec> deadline=<utc-iso>
#   HEARTBEAT run=<n> elapsed=<sec> remaining=<sec>
#   CLEAN    run=<n> ran=<sec>
#   CRASH    run=<n> exit=<code> queue=<dir>
#   RESUME   run=<n> waited=<sec>
#   DONE     total_runs=<n> total_crashes=<n> fuzz_time=<sec>

param(
  [int]$BudgetSeconds = 28800,  # 8h
  [int]$HeartbeatSeconds = 300  # 5 min
)

$ErrorActionPreference = "Stop"
$RepoRoot   = $PSScriptRoot
$FuzzDir    = Join-Path $RepoRoot "crates\rsiprtp"
$LogsDir    = Join-Path $RepoRoot "wrk_journals\fuzz_logs"
$TriageDir  = Join-Path $RepoRoot "wrk_journals\fuzz_triage"
$ArtDir     = Join-Path $FuzzDir "fuzz\artifacts\sip_message_roundtrip"

New-Item -ItemType Directory -Path $LogsDir   -Force | Out-Null
New-Item -ItemType Directory -Path $TriageDir -Force | Out-Null
New-Item -ItemType Directory -Path $ArtDir    -Force | Out-Null

# MSVC ASAN dll on PATH (Windows-MSVC requirement for cargo-fuzz)
$AsanDir = "C:\Program Files\Microsoft Visual Studio\18\Enterprise\VC\Tools\MSVC\14.50.35717\bin\Hostx64\x64"
if ($env:PATH -notlike "*$AsanDir*") {
  $env:PATH = "$AsanDir;" + $env:PATH
}

function EmitLine([string]$line) {
  Write-Host $line
  [Console]::Out.Flush()
}

$StartUtc       = [DateTime]::UtcNow
$DeadlineUtc    = $StartUtc.AddSeconds($BudgetSeconds)
$RunIndex       = 0
$CrashCount     = 0
$FuzzTimeSpent  = 0

EmitLine ("START budget={0} deadline={1:yyyy-MM-ddTHH:mm:ssZ}" -f $BudgetSeconds, $DeadlineUtc)

Set-Location $FuzzDir

while ($true) {
  $remaining = [int][Math]::Floor(($DeadlineUtc - [DateTime]::UtcNow).TotalSeconds)
  if ($remaining -le 60) {
    break
  }

  $RunIndex += 1
  $runId      = "run_{0:D3}" -f $RunIndex
  $logPath    = Join-Path $LogsDir "$runId.log"
  $runStartUtc = [DateTime]::UtcNow

  # Spawn cargo-fuzz as a child so we can heartbeat alongside it.
  # libFuzzer exits non-zero on crash; we treat that as a triage event.
  $args = @(
    "+nightly", "fuzz", "run", "sip_message_roundtrip", "--",
    "-max_total_time=$remaining",
    "-timeout=10",
    "-rss_limit_mb=512"
  )

  # Stream stdout+stderr live to per-run log so the supervisor can tail it.
  # Start-Process flushes line-by-line to the redirect files.
  $errPath = Join-Path $LogsDir "$runId.err"
  Set-Content -Path $logPath -Value "" -NoNewline -Encoding utf8
  Set-Content -Path $errPath -Value "" -NoNewline -Encoding utf8

  $proc = Start-Process -FilePath "cargo" -ArgumentList $args `
    -WorkingDirectory $FuzzDir `
    -RedirectStandardOutput $logPath `
    -RedirectStandardError  $errPath `
    -NoNewWindow -PassThru

  $lastBeat = [DateTime]::UtcNow
  while (-not $proc.HasExited) {
    Start-Sleep -Milliseconds 1000
    $now = [DateTime]::UtcNow
    if (($now - $lastBeat).TotalSeconds -ge $HeartbeatSeconds) {
      $elapsedThisRun = [int]($now - $runStartUtc).TotalSeconds
      $remainingNow   = [int][Math]::Floor(($DeadlineUtc - $now).TotalSeconds)
      # Sample the most recent libfuzzer status line for the heartbeat
      $lastStat = ""
      if (Test-Path $errPath) {
        $tail = Get-Content $errPath -Tail 5 -ErrorAction SilentlyContinue
        $statLine = $tail | Where-Object { $_ -match "^#\d+\s" } | Select-Object -Last 1
        if ($statLine) { $lastStat = ($statLine -replace "\s+", " ").Trim() }
      }
      if ($lastStat) {
        EmitLine ("HEARTBEAT run={0} elapsed={1} remaining={2} stat=[{3}]" -f $RunIndex, $elapsedThisRun, $remainingNow, $lastStat)
      } else {
        EmitLine ("HEARTBEAT run={0} elapsed={1} remaining={2}" -f $RunIndex, $elapsedThisRun, $remainingNow)
      }
      $lastBeat = $now
    }
  }

  # Append err into log so triage has one file to read
  if ((Test-Path $errPath) -and (Get-Item $errPath).Length -gt 0) {
    Add-Content -Path $logPath -Value "`n--- STDERR ---`n" -Encoding utf8
    Get-Content $errPath | Add-Content -Path $logPath -Encoding utf8
  }

  $exitCode      = $proc.ExitCode
  $runEndUtc     = [DateTime]::UtcNow
  $runRanSeconds = [int]($runEndUtc - $runStartUtc).TotalSeconds
  $FuzzTimeSpent += $runRanSeconds

  if ($exitCode -eq 0) {
    EmitLine ("CLEAN run={0} ran={1}" -f $RunIndex, $runRanSeconds)
    continue
  }

  # Non-zero — treat as crash. Move artifacts + log into a triage slot,
  # then wait for supervisor to touch RESUME before continuing.
  $CrashCount += 1
  $slot = "crash_{0:D3}" -f $RunIndex
  $slotPath = Join-Path $TriageDir $slot
  New-Item -ItemType Directory -Path $slotPath -Force | Out-Null

  Copy-Item $logPath (Join-Path $slotPath "fuzz.log") -Force

  # Capture any artifact files written by libFuzzer (may be empty on
  # Windows __fastfail per prior journal — log is the source of truth).
  if (Test-Path $ArtDir) {
    Get-ChildItem $ArtDir -File | ForEach-Object {
      Copy-Item $_.FullName (Join-Path $slotPath $_.Name) -Force
      Remove-Item $_.FullName -Force
    }
  }

  EmitLine ("CRASH run={0} exit={1} queue={2}" -f $RunIndex, $exitCode, $slot)

  # Wait for supervisor to touch RESUME inside the slot.
  $waitStart = [DateTime]::UtcNow
  $resumePath = Join-Path $slotPath "RESUME"
  while (-not (Test-Path $resumePath)) {
    Start-Sleep -Seconds 5
  }
  $waited = [int]([DateTime]::UtcNow - $waitStart).TotalSeconds
  EmitLine ("RESUME run={0} waited={1}" -f $RunIndex, $waited)
}

EmitLine ("DONE total_runs={0} total_crashes={1} fuzz_time={2}" -f $RunIndex, $CrashCount, $FuzzTimeSpent)
