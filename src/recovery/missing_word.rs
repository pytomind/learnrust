// =============================================================================
// Parallel Missing Word Solver (High-Speed Rayon Engine, 1 or 2 Missing Words)
// =============================================================================
use anyhow::{anyhow, Result};
use colored::*;
use rayon::prelude::*;
use std::time::Instant;

use crate::crypto::DerivationEngine;
use crate::database::{DuckDbWalletIndex, WalletHit};

pub struct MissingWordSolver;

impl MissingWordSolver {
    /// Solve a phrase with 1 or 2 missing words marked as '?' or '*'
    pub fn solve(
        template: &str,
        engine: &DerivationEngine,
        index: &DuckDbWalletIndex,
        account_depth: u32,
    ) -> Result<Vec<WalletHit>> {
        Self::solve_with_target(template, engine, index, account_depth, None)
    }

    /// Solve with optional target address match
    pub fn solve_with_target(
        template: &str,
        engine: &DerivationEngine,
        index: &DuckDbWalletIndex,
        account_depth: u32,
        target_address: Option<&str>,
    ) -> Result<Vec<WalletHit>> {
        let words: Vec<&str> = template.split_whitespace().collect();
        let missing_positions: Vec<usize> = words
            .iter()
            .enumerate()
            .filter(|(_, &w)| w == "?" || w == "*")
            .map(|(i, _)| i)
            .collect();

        if missing_positions.is_empty() {
            return Err(anyhow!(
                "No missing word marker ('?' or '*') found in phrase template: \"{}\"",
                template
            ));
        }

        if missing_positions.len() > 2 {
            return Err(anyhow!(
                "Maximum 2 missing words supported for real-time recovery. Found {} markers at positions: {:?}",
                missing_positions.len(),
                missing_positions.iter().map(|p| p + 1).collect::<Vec<_>>()
            ));
        }

        let wordlist = bip39::Language::English.word_list();
        let t0 = Instant::now();
        let norm_target = target_address.map(|t| t.trim().to_lowercase());

        if missing_positions.len() == 1 {
            let pos = missing_positions[0];
            println!(
                "{}",
                format!(
                    "🔍 Testing 1 missing word at Position #{} across all 2,048 BIP-39 words...",
                    pos + 1
                )
                .cyan()
            );

            let matches: Vec<WalletHit> = wordlist
                .par_iter()
                .flat_map(|&candidate| {
                    let mut candidate_words = words.clone();
                    candidate_words[pos] = candidate;
                    let candidate_phrase = candidate_words.join(" ");

                    let mut hits = Vec::new();

                    if let Ok(derived) = engine.derive_phrase(&candidate_phrase, account_depth) {
                        for acc in &derived.additional_accounts {
                            Self::check_account_hit(
                                &candidate_phrase,
                                acc,
                                index,
                                norm_target.as_deref(),
                                &mut hits,
                            );
                        }
                    }
                    hits
                })
                .collect();

            let elapsed = t0.elapsed();
            let speed = (2048.0 / elapsed.as_secs_f64()).round();
            println!(
                "{}",
                format!("⚡ Checked 2,048 variations in {:.2?} ({:.0} phrases/sec)", elapsed, speed).yellow()
            );

            Ok(matches)
        } else {
            // 2 Missing Words: 2,048 x 2,048 = 4,194,304 variations pruned by 4-bit BIP-39 checksum
            let pos1 = missing_positions[0];
            let pos2 = missing_positions[1];
            println!(
                "{}",
                format!(
                    "🔍 Testing 2 missing words at Positions #{} and #{} across 4,194,304 combinations...",
                    pos1 + 1,
                    pos2 + 1
                )
                .cyan()
            );

            let matches: Vec<WalletHit> = (0..2048usize)
                .into_par_iter()
                .flat_map(|i| {
                    let w1 = wordlist[i];
                    let mut local_hits = Vec::new();

                    for j in 0..2048usize {
                        let w2 = wordlist[j];
                        let mut candidate_words = words.clone();
                        candidate_words[pos1] = w1;
                        candidate_words[pos2] = w2;
                        let candidate_phrase = candidate_words.join(" ");

                        // Checksum pruning: BIP-39 checksum rejects ~15 of 16 invalid combinations
                        if bip39::Mnemonic::parse_normalized(&candidate_phrase).is_err() {
                            continue;
                        }

                        if let Ok(derived) = engine.derive_phrase(&candidate_phrase, account_depth) {
                            for acc in &derived.additional_accounts {
                                Self::check_account_hit(
                                    &candidate_phrase,
                                    acc,
                                    index,
                                    norm_target.as_deref(),
                                    &mut local_hits,
                                );
                            }
                        }
                    }
                    local_hits
                })
                .collect();

            let elapsed = t0.elapsed();
            println!(
                "{}",
                format!("⚡ Checked 4,194,304 variations in {:.2?} with Rayon parallel engine", elapsed).yellow()
            );

            Ok(matches)
        }
    }

    fn check_account_hit(
        candidate_phrase: &str,
        acc: &crate::crypto::AddressEntry,
        index: &DuckDbWalletIndex,
        target: Option<&str>,
        hits: &mut Vec<WalletHit>,
    ) {
        let eth_lower = acc.eth_address.to_lowercase();
        let btc_sw_lower = acc.btc_segwit.to_lowercase();
        let btc_leg_lower = acc.btc_legacy.to_lowercase();

        // 1. Target address match (fast nano-second comparison)
        if let Some(t) = target {
            if eth_lower == t || btc_sw_lower == t || btc_leg_lower == t {
                let m_type = if eth_lower == t {
                    "ETH"
                } else if btc_sw_lower == t {
                    "BTC_SEGWIT"
                } else {
                    "BTC_LEGACY"
                };

                let matched_addr = if eth_lower == t {
                    &acc.eth_address
                } else if btc_sw_lower == t {
                    &acc.btc_segwit
                } else {
                    &acc.btc_legacy
                };

                let (source, bal, tx) = if let Some((path, rec)) = index.find_and_fetch(matched_addr) {
                    (path.file_name().unwrap_or_default().to_string_lossy().to_string(), rec.balance, rec.tx_count)
                } else {
                    ("Target Matched".to_string(), None, None)
                };

                hits.push(WalletHit {
                    phrase: candidate_phrase.to_string(),
                    matched_address: matched_addr.clone(),
                    matched_type: m_type.to_string(),
                    derivation_index: acc.index,
                    database_source: source,
                    recorded_balance: bal,
                    recorded_tx_count: tx,
                    live_balance: None,
                    live_tx_count: None,
                    found_timestamp: chrono_now(),
                });
            }
            return;
        }

        // 2. DuckDB Fast RAM Index Check (when no target specified)
        // ETH
        if let Some(path) = index.find_in_cache(&eth_lower) {
            let rec = index.fetch_record_from_path(&path, &eth_lower);
            hits.push(WalletHit {
                phrase: candidate_phrase.to_string(),
                matched_address: acc.eth_address.clone(),
                matched_type: "ETH".to_string(),
                derivation_index: acc.index,
                database_source: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                recorded_balance: rec.as_ref().and_then(|r| r.balance.clone()),
                recorded_tx_count: rec.as_ref().and_then(|r| r.tx_count),
                live_balance: None,
                live_tx_count: None,
                found_timestamp: chrono_now(),
            });
        }

        // BTC SegWit
        if let Some(path) = index.find_in_cache(&btc_sw_lower) {
            let rec = index.fetch_record_from_path(&path, &btc_sw_lower);
            hits.push(WalletHit {
                phrase: candidate_phrase.to_string(),
                matched_address: acc.btc_segwit.clone(),
                matched_type: "BTC_SEGWIT".to_string(),
                derivation_index: acc.index,
                database_source: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                recorded_balance: rec.as_ref().and_then(|r| r.balance.clone()),
                recorded_tx_count: rec.as_ref().and_then(|r| r.tx_count),
                live_balance: None,
                live_tx_count: None,
                found_timestamp: chrono_now(),
            });
        }

        // BTC Legacy
        if let Some(path) = index.find_in_cache(&btc_leg_lower) {
            let rec = index.fetch_record_from_path(&path, &btc_leg_lower);
            hits.push(WalletHit {
                phrase: candidate_phrase.to_string(),
                matched_address: acc.btc_legacy.clone(),
                matched_type: "BTC_LEGACY".to_string(),
                derivation_index: acc.index,
                database_source: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                recorded_balance: rec.as_ref().and_then(|r| r.balance.clone()),
                recorded_tx_count: rec.as_ref().and_then(|r| r.tx_count),
                live_balance: None,
                live_tx_count: None,
                found_timestamp: chrono_now(),
            });
        }
    }
}

fn chrono_now() -> String {
    format!("{:?}", std::time::SystemTime::now())
}
