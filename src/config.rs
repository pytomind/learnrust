// =============================================================================
// Configuration & Constants for Wallet Searcher
// =============================================================================
use std::path::PathBuf;

pub struct AppConfig {
    pub default_data_dir: PathBuf,
    pub default_eth_rpc: &'static str,
    pub default_mempool_api: &'static str,
}

pub const CONFIG: AppConfig = AppConfig {
    // Default location pointing to namedox DuckDB datasets
    default_data_dir: PathBuf::new(), // Evaluated dynamically or via fallback
    default_eth_rpc: "https://ethereum.publicnode.com",
    default_mempool_api: "https://mempool.space/api",
};

// Standard Derivation Paths
pub const ETH_DERIVATION_PATH: &str = "m/44'/60'/0'/0/0";
pub const BTC_SEGWIT_PATH: &str = "m/84'/0'/0'/0/0";
pub const BTC_LEGACY_PATH: &str = "m/44'/0'/0'/0/0";
pub const BTC_TAPROOT_PATH: &str = "m/86'/0'/0'/0/0";

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}


