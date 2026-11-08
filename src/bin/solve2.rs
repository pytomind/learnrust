use bip39::{Language, Mnemonic};
use colored::*;
use rayon::prelude::*;
use std::time::Instant;

use learnrust_wallet_checker::crypto::DerivationEngine;

fn main() {
    let target = "0x55a2d84a26343c6ad7ab4713f241c07f70dc025c";
    let prefix = ["issue", "dizzy", "host", "pretty", "ghost"];
    let suffix = ["vital", "differ", "pencil", "pretty", "benefit"];

    println!("{}", "═══════════════════════════════════════════════════════════".cyan());
    println!("{}", "⚡ HIGH-SPEED PARALLEL 2-WORD RECOVERY (RAYON)".yellow().bold());
    println!("   Target ETH Address: {}", target.green());
    println!("   Template: issue dizzy host pretty ghost ? ? vital differ pencil pretty benefit");
    println!("{}", "═══════════════════════════════════════════════════════════".cyan());

    let wordlist = Language::English.word_list();
    let engine = DerivationEngine::new();
    let t0 = Instant::now();

    println!("Checking 4,194,304 word pairs across all CPU threads...");

    // Parallel outer loop with Rayon
    let result = (0..2048usize).into_par_iter().find_map_any(|i| {
        let w1 = wordlist[i];
        for j in 0..2048usize {
            let w2 = wordlist[j];
            let candidate = format!(
                "{} {} {} {} {} {} {} {} {} {} {} {}",
                prefix[0], prefix[1], prefix[2], prefix[3], prefix[4],
                w1, w2,
                suffix[0], suffix[1], suffix[2], suffix[3], suffix[4]
            );

            // Fast checksum pruning: ~1 in 16 pass
            if Mnemonic::parse_normalized(&candidate).is_err() {
                continue;
            }

            // Derive account indices 0, 1, 2
            if let Ok(derived) = engine.derive_phrase(&candidate, 3) {
                for acc in &derived.additional_accounts {
                    if acc.eth_address.eq_ignore_ascii_case(target) {
                        return Some((w1, w2, candidate, acc.index, acc.eth_address.clone()));
                    }
                }
            }
        }
        None
    });

    let elapsed = t0.elapsed();
    println!("Scan finished in {:.2?}", elapsed);

    match result {
        Some((w1, w2, phrase, index, addr)) => {
            println!("{}", "🎉 MATCH FOUND!".green().bold());
            println!("   Missing Word #6 : {}", w1.yellow().bold());
            println!("   Missing Word #7 : {}", w2.yellow().bold());
            println!("   Account Index   : {}", index);
            println!("   Address         : {}", addr.green());
            println!("   Full Seed Phrase: \"{}\"", phrase.bold());
        }
        None => {
            println!("{}", "❌ No match found for standard derivation m/44'/60'/0'/0/index (indices 0..2).".red());
        }
    }
}

