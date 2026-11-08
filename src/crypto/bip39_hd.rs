// =============================================================================
// BIP-39 Mnemonic & BIP-32/44/84/86 Hierarchical Deterministic (HD) Wallet Engine
// =============================================================================
use anyhow::{anyhow, Result};
use bip39::Mnemonic;
use hmac::{Hmac, Mac};
use secp256k1::{PublicKey, Scalar, Secp256k1, SecretKey};
use sha2::Sha512;

use super::bitcoin::{pubkey_to_legacy_p2pkh, pubkey_to_segwit_p2wpkh, pubkey_to_taproot_p2tr};
use super::ethereum::pubkey_to_eth_address;

type HmacSha512 = Hmac<Sha512>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressEntry {
    pub index: u32,
    pub eth_address: String,
    pub btc_segwit: String,
    pub btc_legacy: String,
    pub btc_taproot: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DerivedWallets {
    pub phrase: String,
    pub word_count: usize,
    pub primary_eth: String,
    pub primary_btc_segwit: String,
    pub primary_btc_legacy: String,
    pub primary_btc_taproot: String,
    pub additional_accounts: Vec<AddressEntry>,
}

pub struct DerivationEngine {
    secp: Secp256k1<secp256k1::All>,
}

impl DerivationEngine {
    pub fn new() -> Self {
        Self {
            secp: Secp256k1::new(),
        }
    }

    /// Derives wallet addresses for index 0 and optional subsequent indices (0..account_limit)
    pub fn derive_phrase(&self, phrase: &str, account_limit: u32) -> Result<DerivedWallets> {
        let clean_phrase = phrase.trim();
        let mnemonic = Mnemonic::parse_normalized(clean_phrase)
            .map_err(|e| anyhow!("Invalid BIP-39 mnemonic: {}", e))?;

        let words: Vec<&str> = clean_phrase.split_whitespace().collect();
        let word_count = words.len();

        // 1. Generate 512-bit seed via PBKDF2 (HMAC-SHA512, 2048 iterations)
        let seed = mnemonic.to_seed("");

        // 2. Derive BIP-32 Master Extended Key
        let (master_key, master_chain_code) = Self::bip32_master_key(&seed)?;

        let mut additional_accounts = Vec::new();

        // 3. Derive for each index in 0..account_limit
        for index in 0..account_limit {
            // Ethereum: m/44'/60'/0'/0/index
            let eth_key = self.derive_path(&master_key, &master_chain_code, &[
                44 | 0x80000000,
                60 | 0x80000000,
                0 | 0x80000000,
                0,
                index,
            ])?;
            let eth_sk = SecretKey::from_slice(&eth_key)?;
            let eth_pk = PublicKey::from_secret_key(&self.secp, &eth_sk);
            let eth_address = pubkey_to_eth_address(&eth_pk);

            // Bitcoin Native SegWit: m/84'/0'/0'/0/index
            let btc_sw_key = self.derive_path(&master_key, &master_chain_code, &[
                84 | 0x80000000,
                0 | 0x80000000,
                0 | 0x80000000,
                0,
                index,
            ])?;
            let btc_sw_sk = SecretKey::from_slice(&btc_sw_key)?;
            let btc_sw_pk = PublicKey::from_secret_key(&self.secp, &btc_sw_sk);
            let btc_segwit = pubkey_to_segwit_p2wpkh(&btc_sw_pk)?;

            // Bitcoin Legacy: m/44'/0'/0'/0/index
            let btc_leg_key = self.derive_path(&master_key, &master_chain_code, &[
                44 | 0x80000000,
                0 | 0x80000000,
                0 | 0x80000000,
                0,
                index,
            ])?;
            let btc_leg_sk = SecretKey::from_slice(&btc_leg_key)?;
            let btc_leg_pk = PublicKey::from_secret_key(&self.secp, &btc_leg_sk);
            let btc_legacy = pubkey_to_legacy_p2pkh(&btc_leg_pk);

            // Bitcoin Taproot: m/86'/0'/0'/0/index
            let btc_tr_key = self.derive_path(&master_key, &master_chain_code, &[
                86 | 0x80000000,
                0 | 0x80000000,
                0 | 0x80000000,
                0,
                index,
            ])?;
            let btc_tr_sk = SecretKey::from_slice(&btc_tr_key)?;
            let btc_tr_pk = PublicKey::from_secret_key(&self.secp, &btc_tr_sk);
            let btc_taproot = pubkey_to_taproot_p2tr(&btc_tr_pk)?;

            additional_accounts.push(AddressEntry {
                index,
                eth_address,
                btc_segwit,
                btc_legacy,
                btc_taproot,
            });
        }

        let primary = &additional_accounts[0];

        Ok(DerivedWallets {
            phrase: clean_phrase.to_string(),
            word_count,
            primary_eth: primary.eth_address.clone(),
            primary_btc_segwit: primary.btc_segwit.clone(),
            primary_btc_legacy: primary.btc_legacy.clone(),
            primary_btc_taproot: primary.btc_taproot.clone(),
            additional_accounts,
        })
    }

    /// Master key generation: HMAC-SHA512(Key = "Bitcoin seed", Data = Seed)
    fn bip32_master_key(seed: &[u8]) -> Result<([u8; 32], [u8; 32])> {
        let mut mac = HmacSha512::new_from_slice(b"Bitcoin seed")
            .map_err(|e| anyhow!("HMAC init error: {}", e))?;
        mac.update(seed);
        let result = mac.finalize().into_bytes();

        let mut key = [0u8; 32];
        let mut chain_code = [0u8; 32];
        key.copy_from_slice(&result[0..32]);
        chain_code.copy_from_slice(&result[32..64]);

        Ok((key, chain_code))
    }

    /// Child key derivation according to BIP-32 spec
    fn derive_path(&self, key: &[u8; 32], chain_code: &[u8; 32], path: &[u32]) -> Result<[u8; 32]> {
        let mut current_key = *key;
        let mut current_cc = *chain_code;

        for &index in path {
            let mut mac = HmacSha512::new_from_slice(&current_cc)
                .map_err(|e| anyhow!("HMAC init error: {}", e))?;

            if index & 0x80000000 != 0 {
                // Hardened derivation: 0x00 || key || index
                mac.update(&[0u8]);
                mac.update(&current_key);
            } else {
                // Normal derivation: compressed_pubkey || index
                let sk = SecretKey::from_slice(&current_key)?;
                let pk = PublicKey::from_secret_key(&self.secp, &sk);
                mac.update(&pk.serialize());
            }
            mac.update(&index.to_be_bytes());

            let result = mac.finalize().into_bytes();
            let mut il = [0u8; 32];
            il.copy_from_slice(&result[0..32]);
            current_cc.copy_from_slice(&result[32..64]);

            // Add IL to current private key modulo secp256k1 curve order
            let sk = SecretKey::from_slice(&current_key)?;
            let scalar = Scalar::from_be_bytes(il)?;
            let child_sk = sk.add_tweak(&scalar)?;
            current_key = child_sk.secret_bytes();
        }

        Ok(current_key)
    }
}

