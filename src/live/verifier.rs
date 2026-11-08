// =============================================================================
// Live On-Chain Verifier (Ethereum PublicNode RPC & Bitcoin Mempool API)
// =============================================================================
use anyhow::Result;
use serde_json::{json, Value};
use std::time::Duration;

pub struct LiveVerifier {
    client: reqwest::Client,
    eth_rpc: String,
    mempool_api: String,
}

impl LiveVerifier {
    pub fn new(eth_rpc: Option<String>, mempool_api: Option<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .user_agent("learnrust-wallet-checker/0.1.0")
                .build()
                .unwrap(),
            eth_rpc: eth_rpc.unwrap_or_else(|| "https://ethereum.publicnode.com".to_string()),
            mempool_api: mempool_api.unwrap_or_else(|| "https://mempool.space/api".to_string()),
        }
    }

    /// Query Ethereum mainnet balance (in ETH) and transaction count
    pub async fn check_ethereum(&self, address: &str) -> Result<(f64, u64)> {
        // 1. Balance
        let bal_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getBalance",
            "params": [address, "latest"]
        });

        let resp: Value = self.client.post(&self.eth_rpc).json(&bal_req).send().await?.json().await?;
        let bal_hex = resp["result"].as_str().unwrap_or("0x0");
        let wei = u128::from_str_radix(bal_hex.trim_start_matches("0x"), 16).unwrap_or(0);
        let balance_eth = (wei as f64) / 1e18;

        // 2. Transaction Count (Nonce)
        let tx_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "eth_getTransactionCount",
            "params": [address, "latest"]
        });

        let tx_resp: Value = self.client.post(&self.eth_rpc).json(&tx_req).send().await?.json().await?;
        let tx_hex = tx_resp["result"].as_str().unwrap_or("0x0");
        let tx_count = u64::from_str_radix(tx_hex.trim_start_matches("0x"), 16).unwrap_or(0);

        Ok((balance_eth, tx_count))
    }

    /// Query Bitcoin balance (in BTC) and transaction count via mempool.space
    pub async fn check_bitcoin(&self, address: &str) -> Result<(f64, u64)> {
        let url = format!("{}/address/{}", self.mempool_api, address);
        let resp: Value = self.client.get(&url).send().await?.json().await?;

        let chain_stats = &resp["chain_stats"];
        let funded = chain_stats["funded_txo_sum"].as_u64().unwrap_or(0);
        let spent = chain_stats["spent_txo_sum"].as_u64().unwrap_or(0);
        let tx_count = chain_stats["tx_count"].as_u64().unwrap_or(0);

        let balance_sats = funded.saturating_sub(spent);
        let balance_btc = (balance_sats as f64) / 1e8;

        Ok((balance_btc, tx_count))
    }
}

