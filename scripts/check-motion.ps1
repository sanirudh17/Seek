# Fails when an animation duration or easing curve is defined outside app/src/motion.rs.
# Usage: powershell -NoProfile -File scripts/check-motion.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$source = Join-Path $root 'app\src'
$allowed = Join-Path $source 'motion.rs'
$rules = @(
    @{ Pattern = 'Animation::new\s*\('; Reason = 'construct animations with crate::motion::{enter,exit,state,settle,reversible}' },
    @{ Pattern = '\.with_easing\s*\('; Reason = 'easing curves belong in crate::motion' },
    @{ Pattern = 'fn\s+ease(_\w+)?\s*\('; Reason = 'easing functions belong in crate::motion' },
    @{ Pattern = 'cubic_bezier|bezier\s*\('; Reason = 'curves belong in crate::motion' },
    @{ Pattern = 'elapsed\(\)\.as_millis\(\)\s*<'; Reason = 'compare motion timing against crate::motion durations' }
)
$failures = @()
Get-ChildItem -Path $source -Recurse -Filter *.rs | Where-Object { $_.FullName -ne $allowed } | ForEach-Object {
    $file = $_
    $number = 0
    foreach ($line in [System.IO.File]::ReadLines($file.FullName)) {
        $number++
        foreach ($rule in $rules) {
            if ($line -match $rule.Pattern) {
                $relative = $file.FullName.Substring($root.Length + 1)
                $failures += "${relative}:${number}: $($rule.Reason)`n    $($line.Trim())"
            }
        }
    }
}
if ($failures.Count -gt 0) {
    Write-Host "Motion check failed: animation timing must come from app/src/motion.rs" -ForegroundColor Red
    $failures | ForEach-Object { Write-Host $_ }
    exit 1
}
Write-Host 'Motion check passed: all animation durations and curves come from app/src/motion.rs.'
