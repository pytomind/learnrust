@echo off
title LearnRust Wallet Engine Server (Port 3030)
cd /d "%~dp0"

echo ========================================================
echo   Starting LearnRust High-Speed Web API Server (Port 3030)
echo   Connects to DuckDB Partitions ^& Namedox Angular UI
echo ========================================================
echo.

if exist "target\release\learnrust-wallet-checker.exe" (
    echo [INFO] Found optimized release binary. Launching directly...
    "target\release\learnrust-wallet-checker.exe" --server
) else (
    echo [INFO] Release binary not found. Building and running with cargo...
    cargo run --release -- --server
)

pause

