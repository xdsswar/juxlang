#!/usr/bin/env pwsh
# Jux benchmark runner.
#
# Builds each workload in --release (so the Tier-0 profile applies), runs it, and
# collects the program's self-reported `RESULT <name>=<ms>` line (taking the min
# over a few repeats to cut noise). The `startup` workload is timed differently:
# the runner measures the whole process wall-clock, since that IS the metric.
#
# Usage:
#   pwsh benchmarks/run.ps1              # all benchmarks, 3 repeats
#   pwsh benchmarks/run.ps1 -Repeats 5   # more repeats
#   pwsh benchmarks/run.ps1 -Only graph_walk
#
# Re-run after each optimization (Tier-1 etc.) and compare the table.

param(
    [int]$Repeats = 3,
    [string]$Only = ""
)

$ErrorActionPreference = "Stop"
$benchDir = $PSScriptRoot
$root = Split-Path -Parent $benchDir

# Resolve the juxc binary. Preference order:
#   1. $env:JUX_HOME (the user's installed release build)  -  <home>\juxc.exe or <home>\bin\juxc.exe
#   2. ./juxc.exe at the repo root
#   3. the local cargo builds (release, then debug)
$candidates = @()
if ($env:JUX_HOME) {
    $candidates += (Join-Path $env:JUX_HOME "juxc.exe")
    $candidates += (Join-Path $env:JUX_HOME "bin/juxc.exe")
}
$candidates += (Join-Path $root "juxc.exe")
$candidates += (Join-Path $root "target/release/juxc.exe")
$candidates += (Join-Path $root "target/debug/juxc.exe")
$juxc = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $juxc) { throw "juxc not found - set JUX_HOME or build it (cargo build --release --bin juxc)" }
Write-Host "using juxc: $juxc"

$hot = @("numeric_mandelbrot", "alloc_churn", "dispatch_poly", "graph_walk")
if ($Only) { $hot = $hot | Where-Object { $_ -eq $Only } }

$results = @()

function Build-Bench([string]$name) {
    $src = Join-Path $benchDir "$name.jux"
    Write-Host "building $name ..." -NoNewline
    & $juxc --release --build $src | Out-Null
    Write-Host " ok"
    return Join-Path $benchDir "target/.rust-build/target/release/$name.exe"
}

foreach ($name in $hot) {
    $exe = Build-Bench $name
    $best = [int]::MaxValue
    for ($i = 0; $i -lt $Repeats; $i++) {
        $out = & $exe
        $line = $out | Where-Object { $_ -match "^RESULT $name=(\d+)$" }
        if ($line -and ($line -match "=(\d+)$")) {
            $ms = [int]$Matches[1]
            if ($ms -lt $best) { $best = $ms }
        }
    }
    $results += [pscustomobject]@{
        Benchmark = $name
        Metric    = "work ms (min of $Repeats)"
        Value     = $best
    }
}

if (-not $Only -or $Only -eq "startup") {
    $exe = Build-Bench "startup"
    $best = [double]::MaxValue
    $runs = [math]::Max(5, $Repeats * 3)
    for ($i = 0; $i -lt $runs; $i++) {
        $t = Measure-Command { & $exe | Out-Null }
        if ($t.TotalMilliseconds -lt $best) { $best = $t.TotalMilliseconds }
    }
    $results += [pscustomobject]@{
        Benchmark = "startup"
        Metric    = "process ms (min of $runs)"
        Value     = [math]::Round($best, 2)
    }
}

Write-Host ""
Write-Host "Jux benchmark results (--release, Tier-0 profile):"
$results | Format-Table -AutoSize
