Set-Location -Path $PSScriptRoot

Write-Host "========================================================" -ForegroundColor Cyan
Write-Host "  Starting LearnRust High-Speed Web API Server (Port 3030)" -ForegroundColor Green
Write-Host "  Connects to DuckDB Partitions & Namedox Angular UI" -ForegroundColor Cyan
Write-Host "========================================================" -ForegroundColor Cyan
Write-Host ""

$exePath = Join-Path $PSScriptRoot "target\release\learnrust-wallet-checker.exe"
if (Test-Path $exePath) {
    Write-Host "[INFO] Found optimized release binary. Launching directly..." -ForegroundColor Green
    & $exePath --server
} else {
    Write-Host "[INFO] Release binary not found. Building and running with cargo..." -ForegroundColor Yellow
    cargo run --release -- --server
}

