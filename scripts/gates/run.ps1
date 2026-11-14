# The quality gates (docs/TESTING.md, section 6) for Windows PowerShell 5.1: the same as scripts/gates/run.
#
#   scripts/gates/run.ps1              every offline gate (crates/parley-core/tests/gates.rs and
#                                      crates/parley-audio/tests/gates.rs, gate_*); without local
#                                      Whisper, G4 prints the line that says why it was skipped
#   scripts/gates/run.ps1 --whisper    the same, with the parley-audio gates built with
#                                      --features local-whisper (cmake and libclang needed), so G4
#                                      runs with models/whisper/ggml-base.bin
#   scripts/gates/run.ps1 --live <G>   one live gate, such as G1: sets PARLEY_LIVE=1 and runs live_<g>
#                                      (crates/parley-cli/tests/gates_live.rs). Live gates spend
#                                      money and are the maintainer's to run.
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

function Run-Cargo($crate, $target, $filter) {
    & cargo test --release -p $crate --test $target -- --ignored --show-output $filter
    $code = $LASTEXITCODE
    if ($null -eq $code) { $code = 1 }
    exit $code
}

# Every offline gate, stopping at the first failure; $features is @() or the local-whisper flags.
function Run-Offline($features) {
    Write-Host '== offline gates'
    & cargo test --release -p parley-core --test gates -- --ignored --show-output gate_
    $code = $LASTEXITCODE
    if ($null -eq $code) { $code = 1 }
    if ($code -ne 0) { exit $code }
    Write-Host '== speaking gates'
    & cargo test --release -p parley-audio @features --test gates -- --ignored --show-output gate_
    $code = $LASTEXITCODE
    if ($null -eq $code) { $code = 1 }
    exit $code
}

$cli = @($args)

if ($cli.Count -eq 0) {
    Run-Offline @()
}

switch ($cli[0]) {
    { $_ -in '--help', '-h' } {
        Get-Content $PSCommandPath -TotalCount 13 | ForEach-Object { $_ -replace '^# ?', '' }
        exit 0
    }
    '--whisper' {
        if ($cli.Count -gt 1) { Fail-Usage "unknown argument $($cli[1])." }
        Run-Offline @('--features', 'local-whisper')
    }
    '--live' {
        if ($cli.Count -lt 2) { Fail-Usage '--live needs a gate, such as G1.' }
        if ($cli.Count -gt 2) { Fail-Usage "unknown argument $($cli[2])." }
        $gate = ([string]$cli[1]).ToLowerInvariant()
        Write-Host "== live gate $($cli[1])"
        # Live gates need the real AI client, so they live in the parley-cli crate (gates_live.rs).
        $env:PARLEY_LIVE = '1'
        Run-Cargo 'parley-cli' 'gates_live' "live_$gate"
    }
    default {
        Fail-Usage "unknown argument $($cli[0])."
    }
}
