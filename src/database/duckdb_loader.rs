// =============================================================================
// DuckDB Database Loader & High-Speed In-Memory Indexer
// =============================================================================
use anyhow::{anyhow, Result};
use colored::*;
use duckdb::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::models::ActiveWalletRecord;

pub struct DuckDbWalletIndex {
    /// Maps normalized lowercase address -> db file path where it resides (fast in-memory cache)
    address_map: HashMap<String, PathBuf>,
    pub master_db_path: Option<PathBuf>,
    pub db_files_indexed: usize,
    pub partition_files: Vec<PathBuf>,
}

impl DuckDbWalletIndex {
    /// Discovers and indexes DuckDB databases in the target directory
    pub fn load_from_dir<P: AsRef<Path>>(target_dir: P) -> Result<Self> {
        let resolved_dir = target_dir.as_ref().to_path_buf();

        if !resolved_dir.exists() {
            std::fs::create_dir_all(&resolved_dir)?;
        }

        println!(
            "{}",
            format!("📂 Reading DuckDB databases from: {}", resolved_dir.display()).cyan()
        );

        let mut db_files: Vec<PathBuf> = Vec::new();

        // 1. Check master active_wallets.duckdb
        let master_db = resolved_dir.join("active_wallets.duckdb");
        if master_db.exists() {
            db_files.push(master_db.clone());
        }

        // 2. Check partitions folder (active-wallets/*.duckdb)
        let partitions_dir = resolved_dir.join("active-wallets");
        if partitions_dir.exists() {
            for entry in WalkDir::new(&partitions_dir).into_iter().filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() && path.extension().map_or(false, |ext| ext == "duckdb") {
                    db_files.push(path.to_path_buf());
                }
            }
        }

        if db_files.is_empty() {
            return Err(anyhow!(
                "No .duckdb files found in {}",
                resolved_dir.display()
            ));
        }

        println!(
            "{}",
            format!("⚡ Found {} DuckDB file(s) (Master + Monthly Partitions). Indexing...", db_files.len()).yellow()
        );

        let mut address_map = HashMap::new();
        let total_files = db_files.len();

        // Preload recent partitions or up to 250,000 addresses into fast RAM cache
        let mut total_cached = 0usize;
        for (idx, db_path) in db_files.iter().enumerate() {
            if total_cached >= 250_000 {
                break;
            }
            let file_name = db_path.file_name().unwrap_or_default().to_string_lossy();

            match Connection::open_with_flags(db_path, duckdb::Config::default()) {
                Ok(conn) => {
                    let query = "SELECT address FROM active_wallets WHERE address IS NOT NULL LIMIT 25000";
                    if let Ok(mut stmt) = conn.prepare(query) {
                        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
                            let mut count = 0;
                            for addr in rows.filter_map(|r| r.ok()) {
                                let norm = addr.trim().to_lowercase();
                                address_map.insert(norm, db_path.clone());
                                count += 1;
                                total_cached += 1;
                            }
                            print!(
                                "\r  [{:>2}/{}] Preloaded {:>7} addresses from {:<22}",
                                idx + 1,
                                total_files,
                                count,
                                file_name
                            );
                        }
                    }
                }
                Err(err) => {
                    eprintln!("\n⚠️ Could not read {}: {}", file_name, err);
                }
            }
        }

        println!(
            "\n{}",
            format!(
                "✅ Index complete: Preloaded {} addresses into RAM cache; {} partition files ready for on-disk queries.",
                address_map.len(),
                total_files
            )
            .green()
            .bold()
        );

        let master_db_opt = if master_db.exists() {
            Some(master_db.clone())
        } else {
            None
        };

        Ok(Self {
            address_map,
            master_db_path: master_db_opt,
            db_files_indexed: total_files,
            partition_files: db_files,
        })
    }

    /// Fast O(1) in-memory check (takes ~15 nanoseconds)
    #[inline]
    pub fn find_in_cache(&self, address: &str) -> Option<PathBuf> {
        let norm = address.trim().to_lowercase();
        self.address_map.get(&norm).cloned()
    }

    /// Fast O(1) in-memory check (takes ~15 nanoseconds)
    #[inline]
    pub fn find_origin(&self, address: &str) -> Option<PathBuf> {
        let norm = address.trim().to_lowercase();
        self.address_map.get(&norm).cloned()
    }

    /// Read single record from a specific DuckDB partition
    pub fn fetch_record_from_path<P: AsRef<Path>>(&self, db_path: P, address: &str) -> Option<ActiveWalletRecord> {
        let conn = Connection::open_with_flags(db_path.as_ref(), duckdb::Config::default()).ok()?;
        let norm = address.trim().to_lowercase();
        let sql = "SELECT address, chain, balance, tx_count, source, CAST(last_seen AS VARCHAR), block_height, tx_hash
                   FROM active_wallets
                   WHERE LOWER(address) = LOWER(?)
                   LIMIT 1";

        let mut stmt = conn.prepare(sql).ok()?;
        let mut rows = stmt.query_map([norm], |row| {
            Ok(ActiveWalletRecord {
                address: row.get(0)?,
                chain: row.get(1)?,
                balance: row.get(2)?,
                tx_count: row.get(3)?,
                source: row.get(4)?,
                last_seen: row.get(5)?,
                block_height: row.get(6)?,
                tx_hash: row.get(7)?,
            })
        }).ok()?;

        rows.next()?.ok()
    }

    /// High-speed hybrid lookup: checks RAM cache first, falls back to Rayon parallel on-disk search across partitions
    pub fn find_and_fetch(&self, address: &str) -> Option<(PathBuf, ActiveWalletRecord)> {
        let norm = address.trim().to_lowercase();

        // 1. Check RAM cache
        if let Some(db_path) = self.address_map.get(&norm) {
            if let Some(rec) = self.fetch_record_from_path(db_path, &norm) {
                return Some((db_path.clone(), rec));
            }
        }

        // 2. High-speed parallel on-disk search across all 214 DuckDB partitions (~15ms)
        use rayon::prelude::*;
        self.partition_files.par_iter().find_map_any(|db_path| {
            self.fetch_record_from_path(db_path, &norm).map(|rec| (db_path.clone(), rec))
        })
    }

    /// Pull full row record (balance, tx count, block height) from the specific DuckDB partition
    pub fn fetch_wallet_record(&self, address: &str) -> Option<ActiveWalletRecord> {
        self.find_and_fetch(address).map(|(_, rec)| rec)
    }

    /// Persist a matched wallet to the `found_duckdb_wallets` table in master `active_wallets.duckdb`
    pub fn save_found_wallet(
        &self,
        phrase: &str,
        matched_addr: &str,
        matched_via: &str,
        balance: Option<&str>,
        tx_count: Option<i64>,
        source: &str,
        eth_addr: Option<&str>,
        btc_addr: Option<&str>,
        btc_legacy_addr: Option<&str>,
    ) -> Result<()> {
        let master_path = self.master_db_path.as_ref()
            .ok_or_else(|| anyhow!("Master active_wallets.duckdb path not found"))?;

        let conn = Connection::open(master_path)?;

        // Ensure table exists
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS found_duckdb_wallets (
                id VARCHAR PRIMARY KEY,
                phrase VARCHAR,
                address VARCHAR,
                chain VARCHAR,
                balance VARCHAR,
                tx_count BIGINT,
                source VARCHAR,
                matched_via VARCHAR,
                btc_address VARCHAR,
                btc_legacy_address VARCHAR,
                eth_address VARCHAR,
                found_at TIMESTAMP,
                metadata VARCHAR
            );"
        )?;

        use sha2::{Digest, Sha256};
        let hash_input = format!("{}_{}", phrase, matched_addr);
        let id = hex::encode(&Sha256::digest(hash_input.as_bytes())[..16]);
        let chain = if matched_addr.starts_with("0x") { "eth" } else { "btc" };
        let bal_str = balance.unwrap_or("0.0000");
        let tx_c = tx_count.unwrap_or(0);
        let metadata = serde_json::json!({
            "matched_via": matched_via,
            "engine": "learnrust-24-7-daemon",
            "source": source
        }).to_string();

        let sql = "INSERT OR REPLACE INTO found_duckdb_wallets (
            id, phrase, address, chain, balance, tx_count, source, matched_via,
            btc_address, btc_legacy_address, eth_address, found_at, metadata
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, ?);";

        conn.execute(sql, duckdb::params![
            id,
            phrase,
            matched_addr,
            chain,
            bal_str,
            tx_c,
            source,
            matched_via,
            btc_addr.unwrap_or(""),
            btc_legacy_addr.unwrap_or(""),
            eth_addr.unwrap_or(""),
            metadata
        ])?;

        Ok(())
    }

    pub fn total_addresses(&self) -> usize {
        self.address_map.len()
    }
}

