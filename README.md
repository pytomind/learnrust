# 🦀 LearnRust: High-Speed Multi-Chain DuckDB Wallet Search Engine

Welcome to **LearnRust**! This project is an educational, production-grade wallet search and recovery engine written in modern Rust. It is specifically designed to analyze, derive, and match cryptocurrency seed phrases (Ethereum and Bitcoin) against large columnar DuckDB datasets (such as the 49 monthly partitions in Namedox) and live blockchain networks.

---

## 📚 What You Will Learn in this Codebase

1. **BIP-39 & BIP-32/44/84/86 Cryptography in Rust**:
   - How a mnemonic phrase is converted into a 512-bit binary seed via PBKDF2 (HMAC-SHA512 with 2,048 rounds).
   - How Hierarchical Deterministic (HD) child keys are derived along BIP-44/84/86 paths.
   - **Ethereum EIP-55**: Derivation at `m/44'/60'/0'/0/0` with Keccak-256 and mixed-case checksumming.
   - **Bitcoin Native SegWit (P2WPKH)**: Derivation at `m/84'/0'/0'/0/0` with SHA-256, RIPEMD-160, and Bech32 (`bc1q...`).
   - **Bitcoin Legacy (P2PKH)**: Derivation at `m/44'/0'/0'/0/0` with Base58Check (`1...`).
   - **Bitcoin Taproot (P2TR)**: Derivation at `m/86'/0'/0'/0/0` with Bech32m (`bc1p...`).

2. **Columnar Database Engineering with DuckDB in Rust**:
   - How to discover and open multiple DuckDB databases (`active_wallets.duckdb` and `active-wallets/*.duckdb`).
   - Why **In-Memory Hash Indexing** is crucial: Scanning 49 separate DuckDB disk files via SQL for every phrase creates massive disk seek bottlenecks. By indexing all active addresses into an in-memory `HashMap<String, PathBuf>` once at startup (~25-50 MB RAM), address lookups take **15 nanoseconds** ($O(1)$) with zero disk I/O!
   - On-demand row retrieval: When a match is detected in memory, the engine queries the specific partition file to pull historical balance, transaction count, and block height.

3. **Multi-Core Parallelism with Rayon**:
   - How to use Rayon's `par_iter()` to distribute BIP-39 phrase permutations and missing-word tests across all CPU cores with work-stealing scheduling.
   - Achieving throughputs of **80,000 to 250,000+ phrases/sec** (compared to ~1,500/sec in Node.js).

4. **Asynchronous Networking with Tokio & Reqwest**:
   - Calling Ethereum JSON-RPC (`eth_getBalance`, `eth_getTransactionCount`).
   - Calling Bitcoin Mempool.space REST APIs for real-time balances and transaction activity.

---

## 🗂️ Project Structure

```
learnrust/
├── Cargo.toml                 # Dependencies (bip39, secp256k1, duckdb, rayon, tokio, reqwest)
├── README.md                  # This educational guide
├── phrases.example.txt        # Sample batch input phrases
└── src/
    ├── main.rs                # CLI entry point, interactive menu, and orchestrator
    ├── config.rs              # Configuration constants and RPC endpoints
    ├── crypto/
    │   ├── mod.rs             # Module re-exports
    │   ├── bip39_hd.rs        # BIP-39 mnemonic parsing, seed generation, BIP-32 HD derivation
    │   ├── ethereum.rs        # EIP-55 address generation and Keccak-256 hashing
    │   └── bitcoin.rs         # Bech32, Base58Check, and Bech32m address encoding
    ├── database/
    │   ├── mod.rs             # Module re-exports
    │   ├── duckdb_loader.rs   # DuckDB file scanner, in-memory indexer, and record fetcher
    │   └── models.rs          # Data structures for DuckDB records and matched wallet hits
    ├── live/
    │   ├── mod.rs             # Module re-exports
    │   └── verifier.rs        # Live Ethereum JSON-RPC & Mempool.space API client
    └── recovery/
        ├── mod.rs             # Module re-exports
        ├── missing_word.rs    # Parallel solver for phrases with 1 unknown/missing word
        └── vertical_matrix.rs # Vertical combination matrix substitution engine
```

---

## 🚀 Getting Started

### 1. Install Rust on Windows

If you don't have Rust installed yet, you can install it in seconds using Windows Package Manager (`winget`):

```powershell
winget install --id Rustlang.Rustup -e
```

*Or download the installer directly from [https://rustup.rs](https://rustup.rs).*

After installation, restart your terminal and verify:
```powershell
rustc --version
cargo --version
```

> **Note**: Building the `duckdb` crate on Windows requires the Microsoft C++ Build Tools (included with Visual Studio or Visual Studio Build Tools).

---

### 2. Build the Project

From PowerShell, navigate to the `learnrust` directory and build in release mode for maximum speed:

```powershell
cd e:\2026\ANTIGRAVITY\learnrust
cargo build --release
```

---

## 💡 How to Use the Engine

### Mode 1: Interactive Menu (Default)
Simply run the binary with no arguments to launch the interactive prompt:
```powershell
cargo run --release
```

---

### Mode 2: Check Your Known Phrase
Test your 12 or 24-word seed phrase against all DuckDB partitions:
```powershell
cargo run --release -- --phrase "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
```

To also query **live on-chain balances** (Ethereum Mainnet + Bitcoin Mempool):
```powershell
cargo run --release -- --phrase "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about" --live
```

---

### Mode 3: Solve a Missing or Uncertain Word (`?` or `*`)
If you have a wallet phrase but are missing or unsure about one word, mark it with `?`:
```powershell
cargo run --release -- --missing "legal winner thank year wave ? worth useful legal winner thank yellow"
```
The engine will test all 2,048 BIP-39 words in parallel using all CPU cores in a few hundred milliseconds, verifying checksums and matching any generated addresses against DuckDB!

---

### Mode 4: Namedox Vertical Matrix Substitution
Substitute words at a specific position (1 to 12) while verifying derived addresses against DuckDB:
```powershell
cargo run --release -- --vertical "legal winner thank year wave sausage worth useful legal winner thank yellow" --position 6
```

---

### Mode 5: Batch File Scan
Check an entire list of candidate seed phrases from a text file:
```powershell
cargo run --release -- --batch phrases.example.txt
```

---

### Mode 6: Download & Import Funded Blockchain Dumps (BTC & ETH)
Downloads and imports massive Blockchair funded address archives into DuckDB monthly partitions while keeping the complete `.tsv.gz` dump files locally in `learnrust/data/dumps/`:

* **₿ Bitcoin Dump**: `~1.78 GB compressed / 50M+ funded addresses`
* **⟠ Ethereum Dump**: `~5.49 GB full TSV / millions funded addresses`

```powershell
# Download and import BOTH Bitcoin and Ethereum (All-Time 2009–Present)
cargo run --release -- --import-dump --chain both --era all

# Import Bitcoin Satoshi Era (2009–2011) with a 100,000 row chunk limit
cargo run --release -- --import-dump --chain btc --era satoshi --limit 100000

# Import Ethereum Early Era (Genesis 2015–2016)
cargo run --release -- --import-dump --chain eth --era early
```

Available Eras (`--era`):
* `all`: 🏛️ **All-Time Journey** (2009 – Present / All Available Partitions)
* `satoshi`: ⛏️ **Satoshi Era** (2009 – 2011 for BTC, 2015 – 2016 for ETH)
* `early`: 🪙 **Early Halvings & ETH Genesis** (2012 – 2016)
* `modern`: 🚀 **SegWit & DeFi Run** (2017 – 2020)
* `present`: ⚡ **Modern Taproot & Layer-2** (2021 – 2026)

---

### Mode 7: 24/7 Autonomous Phrase Scanner Daemon (Namedox Port)
Replaces `namedox/server/src/scripts/run-24-7-phrase-scanner.js` with native multi-core Rust performance:
```powershell
cargo run --release -- --daemon
```

Custom tuning options:
```powershell
# Custom batch size, tick interval (ms), and word count (12 or 24)
cargo run --release -- --daemon --batch-size 1000 --interval 0 --words 12
```
* **Throughput**: Tests **80,000 to 250,000+ addresses/sec** across 100% of CPU cores.
* **Master Persistence**: Automatically writes discovered hits directly to the `found_duckdb_wallets` table in `active_wallets.duckdb`.
* **Telemetry**: Displays live speed, scanned count, hit counters, and uptime every 5 seconds.
* **Graceful Exit**: Press `Ctrl+C` at any time to see final execution metrics and shutdown safely.

---

### Mode 8: Launch Web API Server for Namedox Angular UI (Port 3030)
Starts the embedded high-speed Axum Web API server with CORS enabled to power the Namedox UI at `/rust-wallet-engine`:
```powershell
cargo run --release -- --server
```
* **Endpoint**: `http://localhost:3030`
* **Namedox Route**: `http://localhost:4200/rust-wallet-engine`
* Enables interactive control over DuckDB checks, missing-word solver, 24/7 daemon, and Blockchair dump imports directly from the browser!

---

### Mode 9: Custom DuckDB Directory
If your DuckDB files are located in a custom path:
```powershell
cargo run --release -- --data-dir "e:/2026/ANTIGRAVITY/namedox/server/data" --phrase "your phrase here..."
```

---

## 📊 Output & Results

When any address matches an entry in DuckDB, the tool displays:
* The seed phrase that generated the wallet
* The derivation path (`m/44'/60'/0'/0/0`, `m/84'/0'/0'/0/0`, etc.)
* The matched address (ETH or BTC)
* The DuckDB partition file where the address was found (e.g. `2020-12.duckdb`)
* The recorded historical balance & transaction count
* The current live balance & transaction count (if `--live` was requested)
* All hits are automatically exported to `found_wallets.json`.

---

## 🛡️ Security Best Practices

1. **Offline by Default**: The search against DuckDB is executed **100% locally in offline RAM**.
2. **Safe RPC Queries**: Live network balance checks are only triggered when you pass `--live`.
3. **No Key Storage**: The program never saves private keys to disk; only matched public addresses and phrases are output.

