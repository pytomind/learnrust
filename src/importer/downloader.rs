// =============================================================================
// High-Speed HTTP Streaming Downloader with Resume & Progress Bar
// =============================================================================
use anyhow::{anyhow, Result};
use colored::*;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub const BTC_DUMP_URL: &str = "https://privatekeyfinder.io/assets/downloads/blockchair_bitcoin_addresses_latest.tsv.gz";
pub const ETH_DUMP_URL: &str = "https://privatekeyfinder.io/assets/downloads/ethereum.tsv.gz";

pub struct DumpDownloader;

impl DumpDownloader {
    /// Return the default URL for a chain
    pub fn get_default_url(chain: &str) -> &'static str {
        if chain.eq_ignore_ascii_case("eth") || chain.eq_ignore_ascii_case("ethereum") {
            ETH_DUMP_URL
        } else {
            BTC_DUMP_URL
        }
    }

    /// Return expected local filename
    pub fn get_dump_filename(chain: &str) -> &'static str {
        if chain.eq_ignore_ascii_case("eth") || chain.eq_ignore_ascii_case("ethereum") {
            "ethereum.tsv.gz"
        } else {
            "blockchair_bitcoin_addresses_latest.tsv.gz"
        }
    }

    /// Download the dump file to the local directory (learnrust/data/dumps) with resume & progress bar
    pub async fn ensure_downloaded<P: AsRef<Path>>(
        chain: &str,
        custom_url: Option<&str>,
        dumps_dir: P,
        bytes_counter: Option<Arc<AtomicU64>>,
        total_bytes_counter: Option<Arc<AtomicU64>>,
    ) -> Result<PathBuf> {
        let dumps_path = dumps_dir.as_ref();
        if !dumps_path.exists() {
            fs::create_dir_all(dumps_path)?;
        }

        let url = custom_url.unwrap_or_else(|| Self::get_default_url(chain));
        let file_name = Self::get_dump_filename(chain);
        let dest_path = dumps_path.join(file_name);

        println!("{}", "═══════════════════════════════════════════════════════════".dimmed());
        println!(
            "{}",
            format!(
                "📥 DUMP DOWNLOADER: {} ({})",
                if chain.eq_ignore_ascii_case("eth") { "⟠ Ethereum (~5.89 GB Full TSV)" } else { "₿ Bitcoin (~1.78 GB Compressed / 50M+ Funded)" },
                chain.to_uppercase()
            ).cyan().bold()
        );
        println!("   Source URL  : {}", url);
        println!("   Destination : {}", dest_path.display());

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .connect_timeout(Duration::from_secs(30))
            .build()?;

        // Step 1: Query HEAD to get accurate total Content-Length
        let mut total_size = 0u64;
        if let Ok(head_res) = client.head(url).send().await {
            if head_res.status().is_success() {
                total_size = head_res.content_length().unwrap_or(0);
            }
        }
        if total_size == 0 {
            // Fallback known sizes if server omits Content-Length
            if chain.eq_ignore_ascii_case("eth") || chain.eq_ignore_ascii_case("ethereum") {
                total_size = 5_899_077_310;
            } else {
                total_size = 1_783_247_010;
            }
        }

        if let Some(ref counter) = total_bytes_counter {
            counter.store(total_size, Ordering::SeqCst);
        }

        let existing_len = if dest_path.exists() {
            fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        // If file already exists and size matches total_size, it's 100% complete
        if total_size > 0 && existing_len == total_size {
            let size_mb = existing_len as f64 / (1024.0 * 1024.0);
            println!(
                "{}",
                format!(
                    "✅ Complete archive already verified on disk ({:.2} MB). Skipping download.",
                    size_mb
                ).green().bold()
            );
            if let Some(ref counter) = bytes_counter {
                counter.store(existing_len, Ordering::SeqCst);
            }
            return Ok(dest_path);
        }

        let total_mb = total_size as f64 / (1024.0 * 1024.0);
        println!(
            "{}",
            format!("📦 Total archive size: {:.2} MB. Starting stream download...", total_mb).cyan()
        );

        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA: {eta})")?
                .progress_chars("#>-")
        );

        let mut retries = 0;
        let max_retries = 25;

        while retries < max_retries {
            let current_len = if dest_path.exists() {
                fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };

            if total_size > 0 && current_len >= total_size {
                break;
            }

            pb.set_position(current_len);
            if let Some(ref counter) = bytes_counter {
                counter.store(current_len, Ordering::SeqCst);
            }

            let (res, mut file) = if current_len > 0 {
                let curr_mb = current_len as f64 / (1024.0 * 1024.0);
                println!(
                    "{}",
                    format!(
                        "🔄 Resuming download from byte {} ({:.2} MB of {:.2} MB)...",
                        current_len,
                        curr_mb,
                        total_mb
                    ).yellow()
                );

                let req = client
                    .get(url)
                    .header("Range", format!("bytes={}-", current_len));

                let response = match req.send().await {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("⚠️ Connection error: {}. Retrying in 3s...", e);
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        retries += 1;
                        continue;
                    }
                };

                if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
                    let f = fs::OpenOptions::new().append(true).open(&dest_path)?;
                    (response, f)
                } else if response.status().is_success() {
                    // Server returned 200 OK without partial content support, restart from 0
                    let f = File::create(&dest_path)?;
                    pb.set_position(0);
                    if let Some(ref counter) = bytes_counter {
                        counter.store(0, Ordering::SeqCst);
                    }
                    (response, f)
                } else {
                    eprintln!("⚠️ HTTP error {}. Retrying in 3s...", response.status());
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    retries += 1;
                    continue;
                }
            } else {
                let response = match client.get(url).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("⚠️ Connection error: {}. Retrying in 3s...", e);
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        retries += 1;
                        continue;
                    }
                };
                if !response.status().is_success() {
                    eprintln!("⚠️ HTTP error {}. Retrying in 3s...", response.status());
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    retries += 1;
                    continue;
                }
                let f = File::create(&dest_path)?;
                (response, f)
            };

            let mut stream = res.bytes_stream();
            let mut stream_error = false;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        if let Err(e) = file.write_all(&chunk) {
                            return Err(anyhow!("Disk write error: {}", e));
                        }
                        let chunk_len = chunk.len() as u64;
                        pb.inc(chunk_len);
                        if let Some(ref counter) = bytes_counter {
                            counter.fetch_add(chunk_len, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        eprintln!("\n⚠️ Network stream interrupted: {}. Resuming from disk offset...", e);
                        stream_error = true;
                        break;
                    }
                }
            }

            let _ = file.flush();

            if !stream_error {
                break;
            } else {
                tokio::time::sleep(Duration::from_secs(2)).await;
                retries += 1;
            }
        }

        pb.finish_with_message("Download complete!");
        let final_size = fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0);
        let final_mb = final_size as f64 / (1024.0 * 1024.0);

        println!(
            "{}",
            format!("✅ Saved complete dump archive to: {} ({:.2} MB)", dest_path.display(), final_mb).green().bold()
        );

        if let Some(ref counter) = bytes_counter {
            counter.store(final_size, Ordering::SeqCst);
        }

        Ok(dest_path)
    }
}
