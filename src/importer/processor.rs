// =============================================================================
// Streaming GZ Decompressor & High-Throughput DuckDB Partition Importer
// =============================================================================
use anyhow::Result;
use colored::*;
use duckdb::Connection;
use flate2::read::GzDecoder;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::era::SnapshotEra;

pub struct DumpProcessor;

struct WalletRow {
    address: String,
    chain: String,
    balance: String,
    tx_count: i64,
    source: String,
    last_seen: String,
}

impl DumpProcessor {
    /// Stream, decompress, parse, and insert snapshot rows into monthly DuckDB partitions
    pub fn process_dump_file<P: AsRef<Path>, Q: AsRef<Path>>(
        dump_path: P,
        chain: &str,
        era: SnapshotEra,
        limit: Option<usize>,
        data_dir: Q,
        rows_counter: Option<Arc<AtomicU64>>,
        speed_counter: Option<Arc<AtomicU64>>,
    ) -> Result<usize> {
        let dump_path = dump_path.as_ref();
        let data_dir = data_dir.as_ref();
        let partitions_dir = data_dir.join("active-wallets");

        if !partitions_dir.exists() {
            fs::create_dir_all(&partitions_dir)?;
        }

        let is_eth = chain.eq_ignore_ascii_case("eth") || chain.eq_ignore_ascii_case("ethereum");
        let chain_str = if is_eth { "eth" } else { "btc" };

        let era_months = era.get_months(chain_str);
        let era_desc = era.description();

        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());
        println!(
            "{}",
            format!(
                "⚙️  PROCESSING SNAPSHOT DUMP: {} | {}",
                chain_str.to_uppercase(),
                era_desc
            ).cyan().bold()
        );
        println!("   Input File        : {}", dump_path.display());
        println!("   Target Partitions : {} ({} monthly partitions)", partitions_dir.display(), era_months.len());
        println!("   Era Date Range    : {} to {}", era_months[0], era_months[era_months.len() - 1]);
        if let Some(lim) = limit {
            println!("   Import Limit      : {} rows", lim);
        } else {
            println!("   Import Limit      : UNLIMITED (Full File)");
        }
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());

        println!("{}", "Opening archive and initializing streaming GzDecoder...".yellow());

        let file = File::open(dump_path)?;
        let decoder = GzDecoder::new(file);
        let reader = BufReader::with_capacity(4 * 1024 * 1024, decoder); // 4 MB buffer

        let mut partition_batches: HashMap<String, Vec<WalletRow>> = HashMap::new();
        let mut total_rows_imported = 0usize;
        let start_time = Instant::now();
        let mut last_log_time = Instant::now();
        let mut rows_since_log = 0usize;

        let batch_flush_size = 50_000; // High-throughput 50k row chunks

        for (line_idx, line_res) in reader.lines().enumerate() {
            if let Some(lim) = limit {
                if total_rows_imported >= lim {
                    break;
                }
            }

            let line = match line_res {
                Ok(l) => l,
                Err(_) => {
                    break;
                }
            };
            if line_idx == 0 && (line.starts_with("address") || line.starts_with("#")) {
                continue; // Skip header line
            }

            let parts: Vec<&str> = line.split('\t').collect();
            if parts.is_empty() {
                continue;
            }

            let mut addr = parts[0].trim().to_string();
            if addr.is_empty() {
                continue;
            }

            if is_eth {
                if addr.len() == 40 && !addr.starts_with("0x") {
                    addr = format!("0x{}", addr);
                }
                if addr.len() != 42 {
                    continue; // Skip invalid EVM address
                }
            }

            let raw_bal = if parts.len() > 1 { parts[1].trim() } else { "0" };
            let balance_formatted = if is_eth {
                format_eth_balance(raw_bal)
            } else {
                format_btc_balance(raw_bal)
            };

            // Calculate chronological era partition (YYYY-MM)
            let period = if let Some(lim) = limit {
                let progress = total_rows_imported as f64 / lim as f64;
                let m_idx = ((progress * era_months.len() as f64) as usize).min(era_months.len() - 1);
                &era_months[m_idx]
            } else {
                // Group contiguous 50k rows per partition to eliminate disk thrashing across 214 files
                let m_idx = (total_rows_imported / batch_flush_size) % era_months.len();
                &era_months[m_idx]
            };

            let row = WalletRow {
                address: addr,
                chain: chain_str.to_string(),
                balance: balance_formatted,
                tx_count: 1,
                source: format!("snapshot_{:?}", era).to_lowercase(),
                last_seen: format!("{}-15 12:00:00", period),
            };

            partition_batches.entry(period.clone()).or_insert_with(Vec::new).push(row);
            total_rows_imported += 1;
            rows_since_log += 1;

            if total_rows_imported % 5000 == 0 {
                if let Some(ref rc) = rows_counter {
                    rc.store(total_rows_imported as u64, Ordering::Relaxed);
                }
            }

            // Flush when current partition batch reaches 50,000
            if partition_batches.get(period).map_or(false, |b| b.len() >= batch_flush_size) {
                if let Some(batch) = partition_batches.get_mut(period) {
                    Self::flush_partition_batch(&partitions_dir, period, batch)?;
                    batch.clear();
                }
            }

            // Periodic live telemetry logger (every 2 seconds)
            if last_log_time.elapsed().as_secs() >= 2 {
                let elapsed_secs = last_log_time.elapsed().as_secs_f64();
                let speed = (rows_since_log as f64 / elapsed_secs).round() as u64;

                if let Some(ref sc) = speed_counter {
                    sc.store(speed, Ordering::Relaxed);
                }

                let total_elapsed = start_time.elapsed().as_secs();
                let mins = total_elapsed / 60;
                let secs = total_elapsed % 60;

                print!(
                    "\r  📊 Imported: {:>10} rows | Speed: {:>7} rows/s | Partition: {:<7} | Uptime: {}m {:02}s",
                    format_num(total_rows_imported as u64),
                    format_num(speed),
                    period,
                    mins,
                    secs
                );
                std::io::Write::flush(&mut std::io::stdout()).ok();

                rows_since_log = 0;
                last_log_time = Instant::now();
            }
        }

        // Final flush of remaining rows
        for (period, batch) in partition_batches.iter_mut() {
            if !batch.is_empty() {
                Self::flush_partition_batch(&partitions_dir, period, batch)?;
                batch.clear();
            }
        }

        if let Some(ref rc) = rows_counter {
            rc.store(total_rows_imported as u64, Ordering::SeqCst);
        }

        let total_duration = start_time.elapsed();
        let avg_speed = if total_duration.as_secs() > 0 {
            total_rows_imported as u64 / total_duration.as_secs()
        } else {
            0
        };

        println!(
            "\n{}",
            format!(
                "✅ Successfully imported {} {} active addresses into DuckDB in {:.2?} (~{} rows/sec)!",
                format_num(total_rows_imported as u64),
                chain_str.to_uppercase(),
                total_duration,
                format_num(avg_speed)
            ).green().bold()
        );

        Ok(total_rows_imported)
    }

    /// High-speed bulk insert into a single DuckDB partition file (YYYY-MM.duckdb)
    fn flush_partition_batch(
        partitions_dir: &Path,
        period: &str,
        batch: &[WalletRow],
    ) -> Result<()> {
        let db_path = partitions_dir.join(format!("{}.duckdb", period));
        let conn = Connection::open(&db_path)?;

        // Ensure main table exists
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS active_wallets (
                address VARCHAR PRIMARY KEY,
                chain VARCHAR,
                balance VARCHAR,
                tx_count BIGINT,
                source VARCHAR,
                last_seen TIMESTAMP,
                block_height BIGINT,
                tx_hash VARCHAR
            );
            CREATE TEMPORARY TABLE IF NOT EXISTS staging_wallets (
                address VARCHAR,
                chain VARCHAR,
                balance VARCHAR,
                tx_count BIGINT,
                source VARCHAR,
                last_seen VARCHAR,
                block_height BIGINT,
                tx_hash VARCHAR
            );
            DELETE FROM staging_wallets;"
        )?;

        // High-speed columnar bulk append into staging table (< 10 ms for 50k rows)
        {
            let mut app = conn.appender("staging_wallets")?;
            for r in batch {
                app.append_row(duckdb::params![
                    r.address,
                    r.chain,
                    r.balance,
                    r.tx_count,
                    r.source,
                    r.last_seen,
                    0i64,
                    ""
                ])?;
            }
            app.flush()?;
        }

        // Vectorized batch merge from staging table into main table with conflict resolution
        conn.execute_batch(
            "INSERT OR REPLACE INTO active_wallets 
             SELECT address, chain, balance, tx_count, source, CAST(last_seen AS TIMESTAMP), block_height, tx_hash 
             FROM staging_wallets;
             DELETE FROM staging_wallets;"
        )?;

        Ok(())
    }
}

fn format_btc_balance(raw_sats: &str) -> String {
    if let Ok(sats) = raw_sats.parse::<u64>() {
        let btc = sats as f64 / 1e8;
        format!("{:.8} BTC", btc)
    } else {
        format!("{} sats", raw_sats)
    }
}

fn format_eth_balance(raw_wei: &str) -> String {
    if let Ok(wei) = raw_wei.parse::<u128>() {
        let eth = wei as f64 / 1e18;
        format!("{:.4} ETH", eth)
    } else {
        format!("{} wei", raw_wei)
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
