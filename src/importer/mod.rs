// =============================================================================
// Snapshot Dump Downloader & Importer Module
// =============================================================================
pub mod downloader;
pub mod era;
pub mod processor;

pub use downloader::DumpDownloader;
pub use era::SnapshotEra;
pub use processor::DumpProcessor;

use anyhow::Result;
use colored::*;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

pub struct SnapshotImporter;

impl SnapshotImporter {
    /// Execute the complete download and import pipeline for BTC, ETH, or both
    pub async fn run_pipeline<P: AsRef<Path>>(
        chain_arg: &str,
        era: SnapshotEra,
        limit: Option<usize>,
        custom_url: Option<&str>,
        data_dir: P,
        bytes_counter: Option<Arc<AtomicU64>>,
        total_bytes_counter: Option<Arc<AtomicU64>>,
        rows_counter: Option<Arc<AtomicU64>>,
        speed_counter: Option<Arc<AtomicU64>>,
    ) -> Result<usize> {
        let data_dir = data_dir.as_ref();
        let dumps_dir = data_dir.join("dumps");

        let chains_to_process = match chain_arg.to_lowercase().as_str() {
            "both" | "all" => vec!["btc", "eth"],
            "eth" | "ethereum" => vec!["eth"],
            _ => vec!["btc"],
        };

        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());
        println!("{}", "🚀 STARTING BLOCKCHAIN SNAPSHOT IMPORT PIPELINE".cyan().bold());
        println!(" • Target Chains     : {:?}", chains_to_process);
        println!(" • Era Selection     : {}", era.description());
        println!(" • Local Dumps Path  : {}", dumps_dir.display());
        println!(" • Partitions Path   : {}", data_dir.join("active-wallets").display());
        if let Some(lim) = limit {
            println!(" • Chunk / Limit     : {} rows per chain", lim);
        } else {
            println!(" • Chunk / Limit     : UNLIMITED (Entire File)");
        }
        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());

        let mut total_imported = 0usize;

        for chain in chains_to_process {
            // Step 1: Download and keep the whole compressed archive locally with resume
            let archive_path = DumpDownloader::ensure_downloaded(
                chain,
                custom_url,
                &dumps_dir,
                bytes_counter.clone(),
                total_bytes_counter.clone(),
            ).await?;

            // Step 2: Stream, decompress, and insert into DuckDB partitions
            let imported_count = DumpProcessor::process_dump_file(
                &archive_path,
                chain,
                era,
                limit,
                data_dir,
                rows_counter.clone(),
                speed_counter.clone(),
            )?;

            total_imported += imported_count;

            println!(
                "{}",
                format!("🎉 [{}] Snapshot imported successfully ({} rows).", chain.to_uppercase(), imported_count)
                    .green()
                    .bold()
            );
        }

        println!("\n{}", "✨ All requested blockchain dumps have been downloaded and imported into DuckDB!".green().bold());

        Ok(total_imported)
    }
}
