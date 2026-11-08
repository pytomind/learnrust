// =============================================================================
// Vertical Combination Matrix Scanner (Namedox Matrix Replica)
// =============================================================================
use anyhow::{anyhow, Result};
use colored::*;
use rayon::prelude::*;
use std::time::Instant;

use crate::crypto::DerivationEngine;
use crate::database::{DuckDbWalletIndex, WalletHit};

pub struct VerticalMatrixScanner;

impl VerticalMatrixScanner {
    /// Test vertical substitutions across phrase positions
    pub fn scan_position(
        phrase: &str,
        target_position: usize, // 1 to 12
        engine: &DerivationEngine,
        index: &DuckDbWalletIndex,
        account_depth: u32,
    ) -> Result<Vec<WalletHit>> {
        let words: Vec<&str> = phrase.split_whitespace().collect();
        if words.len() != 12 {
            return Err(anyhow!("Vertical matrix requires a 12-word phrase. Got {} words.", words.len()));
        }

        if target_position < 1 || target_position > 12 {
            return Err(anyhow!("target_position must be between 1 and 12. Got {}.", target_position));
        }

        let pos_idx = target_position - 1;
        println!(
            "{}",
            format!(
                "🧬 Running Vertical Matrix on Position #{} (original word: \"{}\")",
                target_position, words[pos_idx]
            )
            .cyan()
        );

        let wordlist = bip39::Language::English.word_list();
        let t0 = Instant::now();

        let hits: Vec<WalletHit> = wordlist
            .par_iter()
            .flat_map(|&substitute_word| {
                let mut candidate_words = words.clone();
                candidate_words[pos_idx] = substitute_word;

                let mut local_hits = Vec::new();

                // If substituting positions 1..11, test valid checksums for word 12
                if target_position < 12 {
                    // Test candidate
                    let candidate_phrase = candidate_words.join(" ");
                    if let Ok(derived) = engine.derive_phrase(&candidate_phrase, account_depth) {
                        Self::check_derived(&candidate_phrase, &derived, index, &mut local_hits);
                    }
                } else {
                    // Position 12: direct validation
                    let candidate_phrase = candidate_words.join(" ");
                    if let Ok(derived) = engine.derive_phrase(&candidate_phrase, account_depth) {
                        Self::check_derived(&candidate_phrase, &derived, index, &mut local_hits);
                    }
                }

                local_hits
            })
            .collect();

        println!(
            "{}",
            format!(
                "⚡ Vertical Matrix scan completed in {:.2?}. Total matches: {}",
                t0.elapsed(),
                hits.len()
            )
            .yellow()
        );

        Ok(hits)
    }

    fn check_derived(
        phrase: &str,
        derived: &crate::crypto::DerivedWallets,
        index: &DuckDbWalletIndex,
        hits: &mut Vec<WalletHit>,
    ) {
        for acc in &derived.additional_accounts {
            if let Some(origin) = index.find_origin(&acc.eth_address) {
                hits.push(WalletHit {
                    phrase: phrase.to_string(),
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
                hits.push(WalletHit {
                    phrase: phrase.to_string(),
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

            if let Some(origin) = index.find_origin(&acc.btc_legacy) {
                hits.push(WalletHit {
                    phrase: phrase.to_string(),
                    matched_address: acc.btc_legacy.clone(),
                    matched_type: "BTC_LEGACY".to_string(),
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

