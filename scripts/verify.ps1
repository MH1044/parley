# The one full check (docs/TESTING.md §2) for Windows PowerShell 5.1: the same steps, in the same
# order, as scripts/verify. Stops at the first failure.
#
# PowerShell 5.1 has no && operator, so every step reads $LASTEXITCODE; native stderr is left
# alone (piping it through 2>&1 would make a success look like a failure).

$ErrorActionPreference = 'Continue'
Set-Location (Join-Path $PSScriptRoot '..')

function Step($name, $command, $arguments) {
    Write-Host ""
    Write-Host "== $name"
    & $command @arguments
    if ($LASTEXITCODE -ne 0) {
        Write-Host ""
        Write-Host "verify: '$name' failed with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }
}

if (-not (Test-Path 'app/node_modules')) {
    Step 'npm ci' 'npm' @('ci', '--prefix', 'app')
}
Step 'front-end build' 'npm' @('--prefix', 'app', 'run', 'build')
Step 'cargo fmt --check' 'cargo' @('fmt', '--all', '--', '--check')
Step 'lint' 'npm' @('--prefix', 'app', 'run', 'lint')
Step 'typecheck' 'npm' @('--prefix', 'app', 'run', 'typecheck')
Step 'clippy' 'cargo' @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
Step 'licence gate: cargo deny' 'cargo' @('deny', 'check', 'licenses')
Step 'licence gate: npm' 'node' @('scripts/check-npm-licences.mjs', 'app/package-lock.json')
Step 'cargo test' 'cargo' @('test', '--workspace')
Step 'vitest' 'npm' @('--prefix', 'app', 'test', '--', '--run')

Write-Host ""
Write-Host "verify: all green"
