# Automated Resilient Dump Downloader for BTC & ETH
$ErrorActionPreference = "Continue"

$dumpsDir = "e:\2026\ANTIGRAVITY\learnrust\data\dumps"
if (!(Test-Path $dumpsDir)) {
    New-Item -ItemType Directory -Path $dumpsDir -Force | Out-Null
}

$targets = @(
    [PSCustomObject]@{
        Name = "Bitcoin Dump"
        FileName = "blockchair_bitcoin_addresses_latest.tsv.gz"
        Url = "https://privatekeyfinder.io/assets/downloads/blockchair_bitcoin_addresses_latest.tsv.gz"
        ExpectedSize = 1783247010
    },
    [PSCustomObject]@{
        Name = "Ethereum Dump"
        FileName = "ethereum.tsv.gz"
        Url = "https://privatekeyfinder.io/assets/downloads/ethereum.tsv.gz"
        ExpectedSize = 5899077310
    }
)

foreach ($target in $targets) {
    $filePath = Join-Path $dumpsDir $target.FileName
    Write-Host "==========================================================="
    Write-Host "CHECKING & DOWNLOADING: $($target.Name)"
    Write-Host "   URL: $($target.Url)"
    Write-Host "   Destination: $filePath"
    Write-Host "   Expected Size: $($target.ExpectedSize) bytes"
    Write-Host "==========================================================="

    while ($true) {
        $currentSize = 0
        if (Test-Path $filePath) {
            $currentSize = (Get-Item $filePath).Length
        }

        if ($currentSize -ge $target.ExpectedSize) {
            Write-Host "COMPLETED: $($target.Name) is 100% complete ($currentSize bytes)! Ready for ingestion."
            break
        }

        $mbDownloaded = [math]::Round($currentSize / 1048576, 2)
        $mbTotal = [math]::Round($target.ExpectedSize / 1048576, 2)
        $pct = [math]::Round(($currentSize / $target.ExpectedSize) * 100, 1)

        if ($currentSize -gt 0) {
            Write-Host "Resuming download at $mbDownloaded MB / $mbTotal MB ($pct percent)..."
            curl.exe -L -C $currentSize --retry 20 --retry-delay 3 --keepalive-time 30 -o $filePath $target.Url
        } else {
            Write-Host "Starting fresh download (0 MB / $mbTotal MB)..."
            curl.exe -L --retry 20 --retry-delay 3 --keepalive-time 30 -o $filePath $target.Url
        }

        Start-Sleep -Seconds 2
    }
}

Write-Host "ALL BLOCKCHAIN DUMP ARCHIVES DOWNLOADED SUCCESSFULLY!"
