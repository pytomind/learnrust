// =============================================================================
// Database Record Models for DuckDB & Match Vault
// =============================================================================
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWalletRecord {
    pub address: String,
    pub chain: Option<String>,
    pub balance: Option<String>,
    pub tx_count: Option<i64>,
    pub source: Option<String>,
    pub last_seen: Option<String>,
    pub block_height: Option<i64>,
    pub tx_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletHit {
    pub phrase: String,
    pub matched_address: String,
    pub matched_type: String, // "ETH", "BTC_SEGWIT", "BTC_LEGACY", "BTC_TAPROOT"
    pub derivation_index: u32,
    pub database_source: String, // e.g., "active_wallets.duckdb" or "partition:2020-12.duckdb"
    pub recorded_balance: Option<String>,
    pub recorded_tx_count: Option<i64>,
    pub live_balance: Option<String>,
    pub live_tx_count: Option<u64>,
    pub found_timestamp: String,
}

