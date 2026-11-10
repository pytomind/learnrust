// =============================================================================
// LearnRust: High-Speed Multi-Chain DuckDB Wallet Search Engine
// =============================================================================
mod config;
mod crypto;
mod database;
mod importer;
mod live;
mod recovery;
mod scanner;
mod server;

use anyhow::Result;
use clap::Parser;
use colored::*;
use std::fs::File;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crypto::DerivationEngine;
use database::{DuckDbWalletIndex, WalletHit};
use live::LiveVerifier;
use recovery::{MissingWordSolver, VerticalMatrixScanner};

#[derive(Parser, Debug)]
#[command(
    name = "learnrust-wallet-checker",
    author = "Antigravity Pair Programmer",
    version = "0.1.0",
    about = "High-performance BIP-39 Multi-Chain Wallet Searcher against DuckDB partitions & Live Blockchain"
)]
struct CliArgs {
    /// Path to data directory containing active_wallets.duckdb and active-wallets folder
    #[arg(short, long, default_value = "./data")]
    data_dir: PathBuf,

    /// 12 or 24-word seed phrase to search (e.g. "abandon abandon ...")
    #[arg(short, long)]
    phrase: Option<String>,

    /// Phrase with one missing or uncertain word marked as '?' or '*' (e.g. "word1 word2 ? word4 ...")
    #[arg(short, long)]
    missing: Option<String>,

    /// Run Namedox Vertical Matrix substitution on a phrase
    #[arg(short, long)]
    vertical: Option<String>,

    /// Target word position for Vertical Matrix substitution (1 to 12)
    #[arg(long, default_value_t = 1)]
    position: usize,

    /// Path to a text file containing seed phrases (one per line)
    #[arg(short, long)]
    batch: Option<PathBuf>,

    /// Number of BIP-44 account indices to derive per phrase (e.g. 1 derives index 0; 5 derives 0..4)
    #[arg(long, default_value_t = 1)]
    account_depth: u32,

    /// Download and stream import funded blockchain snapshot dump (Blockchair BTC / Ethereum)
    #[arg(long, default_value_t = false)]
    import_dump: bool,

    /// Target chain for snapshot import: 'btc', 'eth', or 'both'
    #[arg(long, default_value = "btc")]
    chain: String,

    /// Historical Journey & Era: 'all', 'satoshi', 'early', 'modern', or 'present'
    #[arg(long, value_enum, default_value_t = importer::SnapshotEra::All)]
    era: importer::SnapshotEra,

    /// Maximum rows to stream from the dump (omit for unlimited / full file)
    #[arg(long)]
    limit: Option<usize>,

    /// Custom snapshot download URL
    #[arg(long)]
    dump_url: Option<String>,

    /// Run 24/7 autonomous continuous BIP-39 phrase scanner daemon (equivalent to run-24-7-phrase-scanner.js)
    #[arg(long, default_value_t = false)]
    daemon: bool,

    /// Batch size for 24/7 daemon (phrases per tick)
    #[arg(long, default_value_t = 500)]
    batch_size: usize,

    /// Interval in milliseconds between batches (0 for maximum native speed)
    #[arg(long, default_value_t = 0)]
    interval: u64,

    /// Mnemonic word count for 24/7 generator (12 or 24)
    #[arg(long, default_value_t = 12)]
    words: usize,

    /// Launch high-speed Web API server on port 3030 for Namedox Angular UI
    #[arg(long, default_value_t = false)]
    server: bool,

    /// Web API Server port
    #[arg(long, default_value_t = 3030)]
    port: u16,

    /// Query live on-chain balances and tx counts via public RPCs (Ethereum + Bitcoin)
    #[arg(long, default_value_t = false)]
    live: bool,

    /// Custom Ethereum JSON-RPC endpoint
    #[arg(long)]
    eth_rpc: Option<String>,

    /// Path to export matched wallet results
    #[arg(short, long, default_value = "found_wallets.json")]
    output: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    print_banner();

    let mut args = CliArgs::parse();

    // If no action flags were specified, launch interactive mode
    if args.phrase.is_none()
        && args.missing.is_none()
        && args.vertical.is_none()
        && args.batch.is_none()
        && !args.daemon
        && !args.import_dump
        && !args.server
    {
        run_interactive_menu(&mut args)?;
    }

    // Mode: Snapshot Dump Downloader & Importer (BTC / ETH / Both - Blockchair dumps)
    if args.import_dump {
        importer::SnapshotImporter::run_pipeline(
            &args.chain,
            args.era,
            args.limit,
            args.dump_url.as_deref(),
            &args.data_dir,
            None,
            None,
            None,
            None,
        ).await?;
        return Ok(());
    }

    // 1. Load & Index DuckDB Data Partitions into RAM
    let t_index = Instant::now();
    let index = match DuckDbWalletIndex::load_from_dir(&args.data_dir) {
        Ok(idx) => idx,
        Err(err) => {
            eprintln!("{}", format!("❌ Failed to index DuckDB databases: {}", err).red().bold());
            eprintln!("{}", "Tip: specify the path using --data-dir \"path/to/data\"".yellow());
            return Ok(());
        }
    };
    let index_duration = t_index.elapsed();
    println!(
        "{}",
        format!("⚡ Indexed {} active addresses in {:.2?}\n", index.total_addresses(), index_duration).green()
    );

    let engine = DerivationEngine::new();

    // Mode: Web API Server for Namedox Angular UI
    if args.server {
        server::ApiServer::run(
            args.port,
            Arc::new(engine),
            Arc::new(index),
            args.data_dir.clone(),
        ).await?;
        return Ok(());
    }

    // Mode: 24/7 Autonomous Scanner Daemon (equivalent to run-24-7-phrase-scanner.js)
    if args.daemon {
        let daemon_cfg = scanner::ScannerDaemonConfig {
            batch_size: args.batch_size,
            interval_ms: args.interval,
            word_count: args.words,
            output_json: args.output.clone(),
        };

        scanner::ScannerDaemon::run(
            daemon_cfg,
            Arc::new(engine),
            Arc::new(index),
        ).await?;

        return Ok(());
    }

    let verifier = LiveVerifier::new(args.eth_rpc.clone(), None);

    let mut collected_hits: Vec<WalletHit> = Vec::new();

    // Mode A: Single Phrase Check
    if let Some(ref phrase) = args.phrase {
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());
        println!("{}", "🔍 MODE: SINGLE SEED PHRASE CHECK".cyan().bold());
        println!("   Phrase: \"{}\"", phrase.bold());
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());

        check_single_phrase(phrase, args.account_depth, &engine, &index, &verifier, args.live, &mut collected_hits).await?;
    }

    // Mode B: Missing Word Solver
    if let Some(ref template) = args.missing {
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());
        println!("{}", "🧩 MODE: PARALLEL MISSING WORD RECOVERY".cyan().bold());
        println!("   Template: \"{}\"", template.bold());
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());

        let hits = MissingWordSolver::solve(template, &engine, &index, args.account_depth)?;
        enrich_and_report_hits(hits, &index, &verifier, args.live, &mut collected_hits).await?;
    }

    // Mode C: Vertical Matrix Substitution
    if let Some(ref phrase) = args.vertical {
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());
        println!("{}", "🧬 MODE: VERTICAL COMBINATION MATRIX".cyan().bold());
        println!("   Base Phrase: \"{}\"", phrase.bold());
        println!("   Target Position: #{}", args.position);
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());

        let hits = VerticalMatrixScanner::scan_position(phrase, args.position, &engine, &index, args.account_depth)?;
        enrich_and_report_hits(hits, &index, &verifier, args.live, &mut collected_hits).await?;
    }

    // Mode D: Batch File Checker
    if let Some(ref batch_path) = args.batch {
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());
        println!("{}", "📁 MODE: BATCH PHRASE FILE CHECKER".cyan().bold());
        println!("   File: {}", batch_path.display());
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());

        check_batch_file(batch_path, args.account_depth, &engine, &index, &verifier, args.live, &mut collected_hits).await?;
    }

    // 3. Save Hits to File if any found
    if !collected_hits.is_empty() {
        println!("\n{}", format!("💾 Saving {} match(es) to: {}", collected_hits.len(), args.output.display()).green().bold());
        let file = File::create(&args.output)?;
        serde_json::to_writer_pretty(file, &collected_hits)?;
        println!("{}", "✅ Match report successfully saved.".green());
    } else {
        println!("\n{}", "⚪ Execution completed. No matches found in current DuckDB partitions.".yellow());
    }

    Ok(())
}

fn print_banner() {
    println!("{}", r#"
  _                                  _   
 | |    ___  __ _ _ __ _ __  _ __ _   _ ___| |_ 
 | |   / _ \/ _` | '__| '_ \| '__| | | / __| __|
 | |__|  __/ (_| | |  | | | | |  | |_| \__ \ |_ 
 |_____\___|\__,_|_|  |_| |_|_|   \__,_|___/\__|
 🦆 High-Speed DuckDB BIP-39 Multi-Chain Wallet Searcher
    "#.bright_blue());
}

async fn check_single_phrase(
    phrase: &str,
    account_depth: u32,
    engine: &DerivationEngine,
    index: &DuckDbWalletIndex,
    verifier: &LiveVerifier,
    live_check: bool,
    collected_hits: &mut Vec<WalletHit>,
) -> Result<()> {
    match engine.derive_phrase(phrase, account_depth) {
        Ok(wallets) => {
            println!("\n📍 Derived Addresses (Account Index 0):");
            println!("   - Ethereum (EIP-55) : {}", wallets.primary_eth.bright_white());
            println!("   - BTC Native SegWit : {}", wallets.primary_btc_segwit.bright_white());
            println!("   - BTC Legacy (1...) : {}", wallets.primary_btc_legacy.bright_white());
            println!("   - BTC Taproot (bc1p): {}", wallets.primary_btc_taproot.bright_white());

            println!("\n🦆 Searching DuckDB Database Partitions...");
            let mut found_any = false;

            for acc in &wallets.additional_accounts {
                // ETH
                if let Some(origin) = index.find_origin(&acc.eth_address) {
                    found_any = true;
                    let file_name = origin.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let record = index.fetch_wallet_record(&acc.eth_address);

                    println!("\n{}", "✨ [MATCH FOUND IN DUCKDB: ETHEREUM]".green().bold());
                    println!("   Address: {}", acc.eth_address.green());
                    println!("   Account: m/44'/60'/0'/0/{}", acc.index);
                    println!("   Source : Partition {}", file_name.yellow());

                    if let Some(r) = &record {
                        println!("   Balance: {}", r.balance.as_deref().unwrap_or("N/A"));
                        println!("   Txs    : {}", r.tx_count.unwrap_or(0));
                    }

                    let (live_bal, live_tx) = if live_check {
                        println!("   🌐 Verifying on Live Ethereum Network...");
                        match verifier.check_ethereum(&acc.eth_address).await {
                            Ok((b, t)) => {
                                println!("   💰 Current Live Balance: {:.6} ETH ({} txs)", b, t);
                                (Some(format!("{:.6} ETH", b)), Some(t))
                            }
                            Err(e) => {
                                println!("   ⚠️ Live check failed: {}", e);
                                (None, None)
                            }
                        }
                    } else {
                        (None, None)
                    };

                    collected_hits.push(WalletHit {
                        phrase: phrase.to_string(),
                        matched_address: acc.eth_address.clone(),
                        matched_type: "ETH".to_string(),
                        derivation_index: acc.index,
                        database_source: file_name,
                        recorded_balance: record.as_ref().and_then(|r| r.balance.clone()),
                        recorded_tx_count: record.as_ref().and_then(|r| r.tx_count),
                        live_balance: live_bal,
                        live_tx_count: live_tx,
                        found_timestamp: format!("{:?}", std::time::SystemTime::now()),
                    });
                }

                // BTC SegWit
                if let Some(origin) = index.find_origin(&acc.btc_segwit) {
                    found_any = true;
                    let file_name = origin.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let record = index.fetch_wallet_record(&acc.btc_segwit);

                    println!("\n{}", "✨ [MATCH FOUND IN DUCKDB: BITCOIN SEGWIT]".green().bold());
                    println!("   Address: {}", acc.btc_segwit.green());
                    println!("   Account: m/84'/0'/0'/0/{}", acc.index);
                    println!("   Source : Partition {}", file_name.yellow());

                    if let Some(r) = &record {
                        println!("   Balance: {}", r.balance.as_deref().unwrap_or("N/A"));
                        println!("   Txs    : {}", r.tx_count.unwrap_or(0));
                    }

                    let (live_bal, live_tx) = if live_check {
                        println!("   🌐 Verifying on Live Bitcoin Mempool...");
                        match verifier.check_bitcoin(&acc.btc_segwit).await {
                            Ok((b, t)) => {
                                println!("   💰 Current Live Balance: {:.8} BTC ({} txs)", b, t);
                                (Some(format!("{:.8} BTC", b)), Some(t))
                            }
                            Err(e) => {
                                println!("   ⚠️ Live check failed: {}", e);
                                (None, None)
                            }
                        }
                    } else {
                        (None, None)
                    };

                    collected_hits.push(WalletHit {
                        phrase: phrase.to_string(),
                        matched_address: acc.btc_segwit.clone(),
                        matched_type: "BTC_SEGWIT".to_string(),
                        derivation_index: acc.index,
                        database_source: file_name,
                        recorded_balance: record.as_ref().and_then(|r| r.balance.clone()),
                        recorded_tx_count: record.as_ref().and_then(|r| r.tx_count),
                        live_balance: live_bal,
                        live_tx_count: live_tx,
                        found_timestamp: format!("{:?}", std::time::SystemTime::now()),
                    });
                }

                // BTC Legacy
                if let Some(origin) = index.find_origin(&acc.btc_legacy) {
                    found_any = true;
                    let file_name = origin.file_name().unwrap_or_default().to_string_lossy().to_string();
                    let record = index.fetch_wallet_record(&acc.btc_legacy);

                    println!("\n{}", "✨ [MATCH FOUND IN DUCKDB: BITCOIN LEGACY]".green().bold());
                    println!("   Address: {}", acc.btc_legacy.green());
                    println!("   Account: m/44'/0'/0'/0/{}", acc.index);
                    println!("   Source : Partition {}", file_name.yellow());

                    if let Some(r) = &record {
                        println!("   Balance: {}", r.balance.as_deref().unwrap_or("N/A"));
                        println!("   Txs    : {}", r.tx_count.unwrap_or(0));
                    }

                    collected_hits.push(WalletHit {
                        phrase: phrase.to_string(),
                        matched_address: acc.btc_legacy.clone(),
                        matched_type: "BTC_LEGACY".to_string(),
                        derivation_index: acc.index,
                        database_source: file_name,
                        recorded_balance: record.as_ref().and_then(|r| r.balance.clone()),
                        recorded_tx_count: record.as_ref().and_then(|r| r.tx_count),
                        live_balance: None,
                        live_tx_count: None,
                        found_timestamp: format!("{:?}", std::time::SystemTime::now()),
                    });
                }
            }

            if !found_any {
                println!("{}", "⚪ No matching address detected in your local DuckDB partitions.".yellow());
            }

            // If user explicitly asked for live verification even without a DuckDB hit:
            if live_check && !found_any {
                println!("\n🌐 Running Live On-Chain Balance Verification anyway (--live):");
                if let Ok((b, t)) = verifier.check_ethereum(&wallets.primary_eth).await {
                    println!("   Ethereum ({}) : {:.6} ETH ({} txs)", wallets.primary_eth, b, t);
                }
                if let Ok((b, t)) = verifier.check_bitcoin(&wallets.primary_btc_segwit).await {
                    println!("   Bitcoin SegWit ({}) : {:.8} BTC ({} txs)", wallets.primary_btc_segwit, b, t);
                }
            }
        }
        Err(e) => {
            eprintln!("{}", format!("❌ Failed to derive wallet: {}", e).red().bold());
        }
    }

    Ok(())
}

async fn enrich_and_report_hits(
    hits: Vec<WalletHit>,
    index: &DuckDbWalletIndex,
    verifier: &LiveVerifier,
    live_check: bool,
    collected_hits: &mut Vec<WalletHit>,
) -> Result<()> {
    if hits.is_empty() {
        println!("{}", "⚪ No DuckDB hits found.".yellow());
        return Ok(());
    }

    println!("\n{}", format!("🔥 DETECTED {} MATCH(ES) IN DUCKDB!", hits.len()).green().bold());

    for mut hit in hits {
        let record = index.fetch_wallet_record(&hit.matched_address);
        if let Some(r) = record {
            hit.recorded_balance = r.balance;
            hit.recorded_tx_count = r.tx_count;
        }

        if live_check {
            if hit.matched_type == "ETH" {
                if let Ok((b, t)) = verifier.check_ethereum(&hit.matched_address).await {
                    hit.live_balance = Some(format!("{:.6} ETH", b));
                    hit.live_tx_count = Some(t);
                }
            } else if hit.matched_type.starts_with("BTC") {
                if let Ok((b, t)) = verifier.check_bitcoin(&hit.matched_address).await {
                    hit.live_balance = Some(format!("{:.8} BTC", b));
                    hit.live_tx_count = Some(t);
                }
            }
        }

        println!("\n✨ Candidate Match:");
        println!("   Phrase   : \"{}\"", hit.phrase.bright_white());
        println!("   Type     : {}", hit.matched_type.yellow());
        println!("   Address  : {}", hit.matched_address.green());
        println!("   Database : {}", hit.database_source.cyan());
        if let Some(ref bal) = hit.recorded_balance {
            println!("   Recorded : {} ({} txs)", bal, hit.recorded_tx_count.unwrap_or(0));
        }
        if let Some(ref l_bal) = hit.live_balance {
            println!("   Live Net : {} ({} txs)", l_bal.bold(), hit.live_tx_count.unwrap_or(0));
        }

        collected_hits.push(hit);
    }

    Ok(())
}

async fn check_batch_file(
    file_path: &PathBuf,
    account_depth: u32,
    engine: &DerivationEngine,
    index: &DuckDbWalletIndex,
    _verifier: &LiveVerifier,
    _live_check: bool,
    collected_hits: &mut Vec<WalletHit>,
) -> Result<()> {
    let file = File::open(file_path)?;
    let reader = io::BufReader::new(file);

    let mut phrases = Vec::new();
    for line in reader.lines().filter_map(|l| l.ok()) {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            phrases.push(trimmed.to_string());
        }
    }

    println!("⚡ Loaded {} phrases from file. Beginning scan...", phrases.len());
    let t0 = Instant::now();

    for (i, p) in phrases.iter().enumerate() {
        print!("\r  Scanning phrase [{}/{}]...", i + 1, phrases.len());
        io::stdout().flush().ok();

        if let Ok(derived) = engine.derive_phrase(p, account_depth) {
            for acc in &derived.additional_accounts {
                if let Some(origin) = index.find_origin(&acc.eth_address) {
                    collected_hits.push(WalletHit {
                        phrase: p.clone(),
                        matched_address: acc.eth_address.clone(),
                        matched_type: "ETH".to_string(),
                        derivation_index: acc.index,
                        database_source: origin.file_name().unwrap_or_default().to_string_lossy().to_string(),
                        recorded_balance: None,
                        recorded_tx_count: None,
                        live_balance: None,
                        live_tx_count: None,
                        found_timestamp: format!("{:?}", std::time::SystemTime::now()),
                    });
                }
                if let Some(origin) = index.find_origin(&acc.btc_segwit) {
                    collected_hits.push(WalletHit {
                        phrase: p.clone(),
                        matched_address: acc.btc_segwit.clone(),
                        matched_type: "BTC_SEGWIT".to_string(),
                        derivation_index: acc.index,
                        database_source: origin.file_name().unwrap_or_default().to_string_lossy().to_string(),
                        recorded_balance: None,
                        recorded_tx_count: None,
                        live_balance: None,
                        live_tx_count: None,
                        found_timestamp: format!("{:?}", std::time::SystemTime::now()),
                    });
                }
            }
        }
    }

    println!("\n✅ Scanned {} phrases in {:.2?}. Hits found: {}", phrases.len(), t0.elapsed(), collected_hits.len());

    Ok(())
}

fn run_interactive_menu(args: &mut CliArgs) -> Result<()> {
    println!("{}", "Interactive Menu — Choose an option to learn & test:".cyan().bold());
    println!("  1. Test a specific BIP-39 phrase against DuckDB & Live RPC");
    println!("  2. Solve missing word (e.g. \"abandon abandon ? ...\")");
    println!("  3. Run Vertical Matrix substitution on a phrase");
    println!("  4. Check batch phrases file (phrases.example.txt)");
    println!("  5. Download & Import Funded Blockchain Dumps (BTC / ETH / Both)");
    println!("  6. Start 24/7 Autonomous Scanner Daemon (like run-24-7-phrase-scanner.js)");
    println!("  7. Launch Web API Server on port 3030 (For Namedox Angular UI)");
    println!("  8. Exit");
    print!("\nEnter choice [1-8]: ");
    io::stdout().flush().ok();

    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let choice = input.trim();

    match choice {
        "1" => {
            print!("\nEnter your 12 or 24-word phrase: ");
            io::stdout().flush().ok();
            let mut phrase_in = String::new();
            io::stdin().read_line(&mut phrase_in).ok();
            let phrase_str = phrase_in.trim().to_string();

            print!("Also check live blockchain balance? (y/n) [n]: ");
            io::stdout().flush().ok();
            let mut live_in = String::new();
            io::stdin().read_line(&mut live_in).ok();
            args.live = live_in.trim().eq_ignore_ascii_case("y");

            args.phrase = Some(phrase_str);
        }
        "2" => {
            print!("\nEnter phrase with missing word marked as '?' (e.g. \"abandon abandon ? ...\"): ");
            io::stdout().flush().ok();
            let mut t_in = String::new();
            io::stdin().read_line(&mut t_in).ok();
            args.missing = Some(t_in.trim().to_string());
        }
        "3" => {
            print!("\nEnter base 12-word phrase: ");
            io::stdout().flush().ok();
            let mut base_in = String::new();
            io::stdin().read_line(&mut base_in).ok();
            args.vertical = Some(base_in.trim().to_string());

            print!("Enter word position to substitute (1-12) [1]: ");
            io::stdout().flush().ok();
            let mut pos_in = String::new();
            io::stdin().read_line(&mut pos_in).ok();
            args.position = pos_in.trim().parse().unwrap_or(1);
        }
        "4" => {
            args.batch = Some(PathBuf::from("phrases.example.txt"));
        }
        "5" => {
            println!("\n{}", "📥 DUMP IMPORT CONFIGURATION:".cyan().bold());
            print!("Select chain (btc / eth / both) [both]: ");
            io::stdout().flush().ok();
            let mut ch_in = String::new();
            io::stdin().read_line(&mut ch_in).ok();
            let ch_str = ch_in.trim();
            args.chain = if ch_str.is_empty() { "both".to_string() } else { ch_str.to_string() };

            println!("\nSelect Historical Journey & Era:");
            println!("  1. 🏛️ All-Time Journey (2009 – Present) [default]");
            println!("  2. ⛏️ Satoshi Era (2009 – 2011)");
            println!("  3. 🪙 Early Halvings & ETH Genesis (2012 – 2016)");
            println!("  4. 🚀 SegWit & DeFi Run (2017 – 2020)");
            println!("  5. ⚡ Modern Taproot & Layer-2 (2021 – 2026)");
            print!("Enter era choice [1-5]: ");
            io::stdout().flush().ok();
            let mut era_in = String::new();
            io::stdin().read_line(&mut era_in).ok();
            args.era = match era_in.trim() {
                "2" => importer::SnapshotEra::Satoshi,
                "3" => importer::SnapshotEra::Early,
                "4" => importer::SnapshotEra::Modern,
                "5" => importer::SnapshotEra::Present,
                _ => importer::SnapshotEra::All,
            };

            print!("\nEnter stream row limit (press Enter for unlimited / entire file): ");
            io::stdout().flush().ok();
            let mut lim_in = String::new();
            io::stdin().read_line(&mut lim_in).ok();
            let lim_str = lim_in.trim();
            args.limit = if lim_str.is_empty() {
                None
            } else {
                lim_str.parse().ok()
            };

            args.import_dump = true;
        }
        "6" => {
            println!("\nStarting 24/7 Autonomous Scanner Daemon...");
            args.daemon = true;
        }
        "7" => {
            println!("\nStarting Web API Server on port 3030...");
            args.server = true;
        }
        _ => {
            println!("Exiting.");
            std::process::exit(0);
        }
    }

    Ok(())
}

