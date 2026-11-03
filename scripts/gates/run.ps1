# The quality gates (docs/TESTING.md, section 6) for Windows PowerShell 5.1: the same as scripts/gates/run.
#
#   scripts/gates/run.ps1              every offline gate (crates/parley-core/tests/gates.rs, gate_*)
#   scripts/gates/run.ps1 --live <G>   one live gate, such as G1: sets PARLEY_LIVE=1 and runs live_<g>.
#                                      Live gates spend money and are the maintainer's to run.
#
# Thresholds live in scripts/gates/thresholds.toml.
#
# PowerShell 5.1 has no && operator, so the exit code is read from $LASTEXITCODE.

$ErrorActionPreference = 'Continue'
Set-Location (Join-Path $PSScriptRoot '..\..')

function Fail-Usage($message) {
    [Console]::Error.WriteLine("error: $message Run scripts/gates/run.ps1 --help.")
    exit 2
}

function Run-Cargo($filter) {
    & cargo test --release -p parley-core --test gates -- --ignored --show-output $filter
    $code = $LASTEXITCODE
    if ($null -eq $code) { $code = 1 }
    exit $code
}

$cli = @($args)

if ($cli.Count -eq 0) {
    Write-Host '== offline gates'
    Run-Cargo 'gate_'
}

switch ($cli[0]) {
    { $_ -in '--help', '-h' } {
        Get-Content $PSCommandPath -TotalCount 7 | ForEach-Object { $_ -replace '^# ?', '' }
        exit 0
    }
    '--live' {
        if ($cli.Count -lt 2) { Fail-Usage '--live needs a gate, such as G1.' }
        if ($cli.Count -gt 2) { Fail-Usage "unknown argument $($cli[2])." }
        $gate = ([string]$cli[1]).ToLowerInvariant()
        Write-Host "== live gate $($cli[1])"
        Write-Host 'No live gate exists yet, so this runs no test.'
        $env:PARLEY_LIVE = '1'
        Run-Cargo "live_$gate"
    }
    default {
        Fail-Usage "unknown argument $($cli[0])."
    }
}
