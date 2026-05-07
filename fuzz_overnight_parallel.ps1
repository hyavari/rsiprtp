# Parallel overnight fuzz wrapper — fans out fuzz_overnight.ps1 across the
# active profiles (sip / sip-diff / rtp / media / ice — sip-legacy excluded
# by default). Each child wrapper runs serially within its profile but the
# 5 profiles run concurrently. Per HLD §"5-profile parallel campaign"
# decided 2026-05-06.
#
# No target overlaps across the active profiles, so the children safely
# share fuzz/corpus/<target>/, fuzz/artifacts/<target>/, the wrk_journals
# log dir, and the wrk_journals triage dir.
#
# Combined event stream is written line-by-line to
#   wrk_journals/fuzz_logs/parallel_combined.events.log
# with each line prefixed by `[<profile>] `. Per-profile streams are also
# kept under wrk_journals/fuzz_logs/parallel_<profile>.events.log.

param(
  [int]$BudgetSeconds  = 28800,                            # 8h
  [int]$HeartbeatSeconds = 300,
  [int]$SliceSeconds   = 1800,
  [int]$RssLimitMb     = 2048,
  [string[]]$Profiles  = @("sip","sip-diff","rtp","media","ice"),
  [int]$PollMs         = 500
)

$ErrorActionPreference = "Stop"
$RepoRoot = $PSScriptRoot
$LogsDir  = Join-Path $RepoRoot "wrk_journals\fuzz_logs"
New-Item -ItemType Directory -Path $LogsDir -Force | Out-Null

$CombinedLog = Join-Path $LogsDir "parallel_combined.events.log"
"" | Out-File -FilePath $CombinedLog -Encoding utf8

function EmitLine([string]$line) {
  Write-Host $line
  Add-Content -Path $CombinedLog -Value $line -Encoding utf8
  [Console]::Out.Flush()
}

# MSVC ASAN dll on PATH (Windows-MSVC requirement for cargo-fuzz). The
# children also do this, but doing it once here avoids any per-child race.
$AsanDir = "C:\Program Files\Microsoft Visual Studio\18\Enterprise\VC\Tools\MSVC\14.50.35717\bin\Hostx64\x64"
if ($env:PATH -notlike "*$AsanDir*") {
  $env:PATH = "$AsanDir;" + $env:PATH
}

$StartUtc    = [DateTime]::UtcNow
$DeadlineUtc = $StartUtc.AddSeconds($BudgetSeconds)

EmitLine ("[parallel] START_ALL profiles={0} budget={1} deadline={2:yyyy-MM-ddTHH:mm:ssZ}" -f ($Profiles -join ","), $BudgetSeconds, $DeadlineUtc)

# Spawn one child wrapper per profile. Each child writes its events to its
# own per-profile log; we tail those logs and re-emit them tagged.
$Children = @()
foreach ($p in $Profiles) {
  $perProfileLog = Join-Path $LogsDir ("parallel_" + $p + ".events.log")
  $perProfileErr = Join-Path $LogsDir ("parallel_" + $p + ".events.err")
  Set-Content -Path $perProfileLog -Value "" -NoNewline -Encoding utf8
  Set-Content -Path $perProfileErr -Value "" -NoNewline -Encoding utf8

  $childArgs = @(
    "-NoProfile", "-NonInteractive",
    "-File", (Join-Path $RepoRoot "fuzz_overnight.ps1"),
    "-Profile", $p,
    "-BudgetSeconds", $BudgetSeconds,
    "-SliceSeconds",  $SliceSeconds,
    "-HeartbeatSeconds", $HeartbeatSeconds,
    "-RssLimitMb", $RssLimitMb
  )

  $proc = Start-Process -FilePath "pwsh" -ArgumentList $childArgs `
    -WorkingDirectory $RepoRoot `
    -RedirectStandardOutput $perProfileLog `
    -RedirectStandardError  $perProfileErr `
    -NoNewWindow -PassThru

  $Children += [PSCustomObject]@{
    Profile = $p
    Process = $proc
    Log     = $perProfileLog
    LastPos = 0
    Done    = $false
  }
  EmitLine ("[parallel] CHILD_START profile={0} pid={1}" -f $p, $proc.Id)
}

# Tail-and-tag loop: every $PollMs, read the new bytes appended to each
# child's per-profile log and emit them tagged on the combined stream.
# Children run for the full budget; this loop exits when all children
# have HasExited == $true AND we've drained their logs.
while ($true) {
  $allDone = $true
  foreach ($c in $Children) {
    if (-not $c.Done) {
      if (-not $c.Process.HasExited) {
        $allDone = $false
      }
    }

    # Read new content since last tail position.
    $newText = ""
    try {
      $stream = [System.IO.File]::Open($c.Log, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
      $stream.Position = $c.LastPos
      $reader = [System.IO.StreamReader]::new($stream)
      $newText = $reader.ReadToEnd()
      $c.LastPos = $stream.Position
      $reader.Close()
      $stream.Close()
    } catch {
      # File transiently unavailable; try again next tick.
    }

    if ($newText.Length -gt 0) {
      foreach ($line in $newText -split "`r?`n") {
        if ($line.Length -gt 0) {
          EmitLine ("[{0}] {1}" -f $c.Profile, $line)
        }
      }
    }

    if ($c.Process.HasExited -and -not $c.Done) {
      $c.Done = $true
      EmitLine ("[parallel] CHILD_EXIT profile={0} exit={1}" -f $c.Profile, $c.Process.ExitCode)
    }
  }

  if ($allDone) {
    # Final drain of any pending log content.
    Start-Sleep -Milliseconds 500
    foreach ($c in $Children) {
      $newText = ""
      try {
        $stream = [System.IO.File]::Open($c.Log, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
        $stream.Position = $c.LastPos
        $reader = [System.IO.StreamReader]::new($stream)
        $newText = $reader.ReadToEnd()
        $c.LastPos = $stream.Position
        $reader.Close()
        $stream.Close()
      } catch {}
      if ($newText.Length -gt 0) {
        foreach ($line in $newText -split "`r?`n") {
          if ($line.Length -gt 0) {
            EmitLine ("[{0}] {1}" -f $c.Profile, $line)
          }
        }
      }
    }
    break
  }

  Start-Sleep -Milliseconds $PollMs
}

$EndUtc = [DateTime]::UtcNow
$Wall   = [int]($EndUtc - $StartUtc).TotalSeconds
EmitLine ("[parallel] DONE_ALL profiles={0} wall={1}" -f ($Profiles -join ","), $Wall)
