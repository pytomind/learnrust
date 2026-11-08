// =============================================================================
// Rust Web API Server for Namedox Angular Frontend (Axum on Port 3030)
// =============================================================================
use axum::{
    extract::State,
    http::Method,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use colored::Colorize;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};

use crate::crypto::DerivationEngine;
use crate::database::{DuckDbWalletIndex, WalletHit};
use crate::importer::SnapshotEra;
use crate::live::LiveVerifier;
use crate::recovery::{MissingWordSolver, VerticalMatrixScanner};

pub struct AppState {
    pub engine: Arc<DerivationEngine>,
    pub index: Arc<DuckDbWalletIndex>,
    pub verifier: Arc<LiveVerifier>,
    pub data_dir: PathBuf,
    pub start_time: Instant,
    // Daemon state
    pub daemon_running: Arc<AtomicBool>,
    pub daemon_phrases: Arc<AtomicU64>,
    pub daemon_addrs: Arc<AtomicU64>,
    pub daemon_hits: Arc<AtomicU64>,
    pub daemon_speed: Arc<AtomicU64>,
    pub daemon_started_at: Arc<tokio::sync::RwLock<Option<String>>>,
    pub daemon_start_instant: Arc<tokio::sync::RwLock<Option<Instant>>>,
    pub recent_hits: Arc<tokio::sync::RwLock<Vec<WalletHit>>>,
    // Importer state
    pub import_running: Arc<AtomicBool>,
    pub import_bytes: Arc<AtomicU64>,
    pub import_total_bytes: Arc<AtomicU64>,
    pub import_rows: Arc<AtomicU64>,
    pub import_speed: Arc<AtomicU64>,
    pub import_chain: Arc<tokio::sync::RwLock<Option<String>>>,
    pub import_era: Arc<tokio::sync::RwLock<Option<String>>>,
    pub import_error: Arc<tokio::sync::RwLock<Option<String>>>,
}

pub struct ApiServer;

impl ApiServer {
    pub async fn run(
        port: u16,
        engine: Arc<DerivationEngine>,
        index: Arc<DuckDbWalletIndex>,
        data_dir: PathBuf,
    ) -> anyhow::Result<()> {
        let verifier = Arc::new(LiveVerifier::new(None, None));

        let state = Arc::new(AppState {
            engine,
            index,
            verifier,
            data_dir,
            start_time: Instant::now(),
            daemon_running: Arc::new(AtomicBool::new(false)),
            daemon_phrases: Arc::new(AtomicU64::new(0)),
            daemon_addrs: Arc::new(AtomicU64::new(0)),
            daemon_hits: Arc::new(AtomicU64::new(0)),
            daemon_speed: Arc::new(AtomicU64::new(0)),
            daemon_started_at: Arc::new(tokio::sync::RwLock::new(None)),
            daemon_start_instant: Arc::new(tokio::sync::RwLock::new(None)),
            recent_hits: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            import_running: Arc::new(AtomicBool::new(false)),
            import_bytes: Arc::new(AtomicU64::new(0)),
            import_total_bytes: Arc::new(AtomicU64::new(0)),
            import_rows: Arc::new(AtomicU64::new(0)),
            import_speed: Arc::new(AtomicU64::new(0)),
            import_chain: Arc::new(tokio::sync::RwLock::new(None)),
            import_era: Arc::new(tokio::sync::RwLock::new(None)),
            import_error: Arc::new(tokio::sync::RwLock::new(None)),
        });

        // Configure permissive CORS for Namedox frontend (localhost:4200 / any origin)
        let cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers(Any)
            .allow_origin(Any);

        let app = Router::new()
            .route("/health", get(handle_status))
            .route("/api/status", get(handle_status))
            .route("/api/check-phrase", post(handle_check_phrase))
            .route("/api/solve-missing", post(handle_solve_missing))
            .route("/api/vertical-matrix", post(handle_vertical_matrix))
            .route("/api/daemon/stats", get(handle_daemon_stats))
            .route("/api/daemon/start", post(handle_daemon_start))
            .route("/api/daemon/stop", post(handle_daemon_stop))
            .route("/api/import/start", post(handle_import_start))
            .route("/api/import/status", get(handle_import_status))
            .route("/api/found-wallets", get(handle_found_wallets))
            .layer(cors)
            .with_state(state);

        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        println!("\n{}", "═══════════════════════════════════════════════════════════".dimmed());
        println!(
            "🦀 RUST HIGH-SPEED API SERVER LISTENING ON: http://localhost:{}",
            port
        );
        println!("   Connected to DuckDB Master & 49 Partitions in RAM");
        println!("   CORS Enabled for Namedox Angular Client (http://localhost:4200)");
        println!("{}", "═══════════════════════════════════════════════════════════\n".dimmed());

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Route Handlers
// -----------------------------------------------------------------------------

async fn handle_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    Json(serde_json::json!({
        "online": true,
        "version": "0.1.0",
        "totalAddressesIndexed": state.index.total_addresses(),
        "dbFilesCount": state.index.db_files_indexed,
        "dataDir": state.data_dir.to_string_lossy(),
        "uptimeSeconds": uptime
    }))
}

#[derive(Deserialize)]
struct CheckPhraseReq {
    phrase: String,
    #[serde(default = "default_one")]
    account_depth: u32,
    #[serde(default)]
    live: bool,
}
fn default_one() -> u32 { 1 }

async fn handle_check_phrase(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CheckPhraseReq>,
) -> impl IntoResponse {
    let t0 = Instant::now();
    match state.engine.derive_phrase(&payload.phrase, payload.account_depth) {
        Ok(derived) => {
            let mut matches = Vec::new();
            let mut is_available = false;

            for acc in &derived.additional_accounts {
                // ETH
                let eth_db = state.index.find_and_fetch(&acc.eth_address);
                let (live_bal, live_tx) = if payload.live {
                    state.verifier.check_ethereum(&acc.eth_address).await
                        .map(|(b, t)| (Some(format!("{:.6} ETH", b)), Some(t)))
                        .unwrap_or((None, None))
                } else {
                    (None, None)
                };

                if let Some((origin, record)) = eth_db {
                    is_available = true;
                    let file_name = origin.file_name().unwrap_or_default().to_string_lossy().to_string();

                    matches.push(WalletHit {
                        phrase: payload.phrase.clone(),
                        matched_address: acc.eth_address.clone(),
                        matched_type: "ETH".to_string(),
                        derivation_index: acc.index,
                        database_source: file_name,
                        recorded_balance: record.balance.clone(),
                        recorded_tx_count: record.tx_count,
                        live_balance: live_bal.clone(),
                        live_tx_count: live_tx,
                        found_timestamp: crate::config::now_iso(),
                    });
                } else if live_bal.is_some() {
                    is_available = true;
                    matches.push(WalletHit {
                        phrase: payload.phrase.clone(),
                        matched_address: acc.eth_address.clone(),
                        matched_type: "ETH (Live RPC Verified)".to_string(),
                        derivation_index: acc.index,
                        database_source: "Live Blockchain RPC".to_string(),
                        recorded_balance: None,
                        recorded_tx_count: None,
                        live_balance: live_bal.clone(),
                        live_tx_count: live_tx,
                        found_timestamp: crate::config::now_iso(),
                    });
                }

                // BTC SegWit
                let btc_sw_db = state.index.find_and_fetch(&acc.btc_segwit);
                let (btc_sw_live_bal, btc_sw_live_tx) = if payload.live {
                    state.verifier.check_bitcoin(&acc.btc_segwit).await
                        .map(|(b, t)| (Some(format!("{:.8} BTC", b)), Some(t)))
                        .unwrap_or((None, None))
                } else {
                    (None, None)
                };

                if let Some((origin, record)) = btc_sw_db {
                    is_available = true;
                    let file_name = origin.file_name().unwrap_or_default().to_string_lossy().to_string();

                    matches.push(WalletHit {
                        phrase: payload.phrase.clone(),
                        matched_address: acc.btc_segwit.clone(),
                        matched_type: "BTC_SEGWIT".to_string(),
                        derivation_index: acc.index,
                        database_source: file_name,
                        recorded_balance: record.balance.clone(),
                        recorded_tx_count: record.tx_count,
                        live_balance: btc_sw_live_bal.clone(),
                        live_tx_count: btc_sw_live_tx,
                        found_timestamp: crate::config::now_iso(),
                    });
                } else if btc_sw_live_bal.is_some() {
                    is_available = true;
                    matches.push(WalletHit {
                        phrase: payload.phrase.clone(),
                        matched_address: acc.btc_segwit.clone(),
                        matched_type: "BTC_SEGWIT (Live Mempool Verified)".to_string(),
                        derivation_index: acc.index,
                        database_source: "Live Mempool API".to_string(),
                        recorded_balance: None,
                        recorded_tx_count: None,
                        live_balance: btc_sw_live_bal.clone(),
                        live_tx_count: btc_sw_live_tx,
                        found_timestamp: crate::config::now_iso(),
                    });
                }

                // BTC Legacy
                let btc_leg_db = state.index.find_and_fetch(&acc.btc_legacy);
                if let Some((origin, record)) = btc_leg_db {
                    is_available = true;
                    let file_name = origin.file_name().unwrap_or_default().to_string_lossy().to_string();

                    matches.push(WalletHit {
                        phrase: payload.phrase.clone(),
                        matched_address: acc.btc_legacy.clone(),
                        matched_type: "BTC_LEGACY".to_string(),
                        derivation_index: acc.index,
                        database_source: file_name,
                        recorded_balance: record.balance.clone(),
                        recorded_tx_count: record.tx_count,
                        live_balance: None,
                        live_tx_count: None,
                        found_timestamp: crate::config::now_iso(),
                    });
                }
            }

            Json(serde_json::json!({
                "success": true,
                "phrase": payload.phrase,
                "wallets": {
                    "phrase": derived.phrase,
                    "ethAddress": derived.primary_eth,
                    "btcSegwit": derived.primary_btc_segwit,
                    "btcLegacy": derived.primary_btc_legacy,
                    "btcTaproot": derived.primary_btc_taproot
                },
                "isAvailableInDuckdb": is_available,
                "matches": matches,
                "durationMs": t0.elapsed().as_millis()
            }))
        }
        Err(err) => {
            Json(serde_json::json!({
                "success": false,
                "error": err.to_string(),
                "durationMs": t0.elapsed().as_millis()
            }))
        }
    }
}

#[derive(Deserialize)]
struct SolveMissingReq {
    template: String,
    #[serde(default = "default_one", alias = "accountDepth")]
    account_depth: u32,
    #[serde(default, alias = "targetAddress")]
    target_address: Option<String>,
}

async fn handle_solve_missing(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SolveMissingReq>,
) -> impl IntoResponse {
    let t0 = Instant::now();
    let word_count = payload.template.split_whitespace().filter(|&w| w == "?" || w == "*").count();
    let tested_count = if word_count == 2 { 4194304 } else { 2048 };

    match MissingWordSolver::solve_with_target(
        &payload.template,
        &state.engine,
        &state.index,
        payload.account_depth,
        payload.target_address.as_deref(),
    ) {
        Ok(hits) => {
            Json(serde_json::json!({
                "success": true,
                "templateOrPhrase": payload.template,
                "testedCount": tested_count,
                "durationMs": t0.elapsed().as_millis(),
                "hits": hits
            }))
        }
        Err(err) => {
            Json(serde_json::json!({
                "success": false,
                "error": err.to_string(),
                "durationMs": t0.elapsed().as_millis()
            }))
        }
    }
}

#[derive(Deserialize)]
struct VerticalReq {
    phrase: String,
    #[serde(default = "default_one_usize")]
    position: usize,
    #[serde(default = "default_one")]
    account_depth: u32,
}
fn default_one_usize() -> usize { 1 }

async fn handle_vertical_matrix(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<VerticalReq>,
) -> impl IntoResponse {
    let t0 = Instant::now();
    match VerticalMatrixScanner::scan_position(&payload.phrase, payload.position, &state.engine, &state.index, payload.account_depth) {
        Ok(hits) => {
            Json(serde_json::json!({
                "success": true,
                "templateOrPhrase": payload.phrase,
                "testedCount": 2048,
                "durationMs": t0.elapsed().as_millis(),
                "hits": hits
            }))
        }
        Err(err) => {
            Json(serde_json::json!({
                "success": false,
                "error": err.to_string(),
                "durationMs": t0.elapsed().as_millis()
            }))
        }
    }
}

// -----------------------------------------------------------------------------
// Daemon Handlers
// -----------------------------------------------------------------------------

async fn handle_daemon_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let is_running = state.daemon_running.load(Ordering::SeqCst);
    let phrases = state.daemon_phrases.load(Ordering::SeqCst);
    let addrs = state.daemon_addrs.load(Ordering::SeqCst);
    let hits = state.daemon_hits.load(Ordering::SeqCst);
    let speed = state.daemon_speed.load(Ordering::SeqCst);
    let started_at = state.daemon_started_at.read().await.clone();
    let recent = state.recent_hits.read().await.clone();
    let uptime_seconds = if is_running {
        state.daemon_start_instant.read().await.map(|t| t.elapsed().as_secs()).unwrap_or(0)
    } else {
        0
    };

    Json(serde_json::json!({
        "isRunning": is_running,
        "startedAt": started_at,
        "phrasesScanned": phrases,
        "addressesChecked": addrs,
        "speedAddrsPerSec": speed,
        "hitsFound": hits,
        "uptimeSeconds": uptime_seconds,
        "recentHits": recent
    }))
}

#[derive(Deserialize)]
struct DaemonStartReq {
    #[serde(default = "default_batch")]
    batch_size: Option<usize>,
    #[serde(default)]
    interval_ms: Option<u64>,
    #[serde(default = "default_word_count")]
    word_count: Option<usize>,
}
fn default_batch() -> Option<usize> { Some(500) }
fn default_word_count() -> Option<usize> { Some(12) }

async fn handle_daemon_start(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DaemonStartReq>,
) -> impl IntoResponse {
    if state.daemon_running.load(Ordering::SeqCst) {
        return Json(serde_json::json!({
            "success": false,
            "message": "Daemon is already running."
        }));
    }

    state.daemon_running.store(true, Ordering::SeqCst);
    *state.daemon_started_at.write().await = Some(crate::config::now_iso());
    *state.daemon_start_instant.write().await = Some(Instant::now());

    let b_size = payload.batch_size.unwrap_or(500);
    let interval = payload.interval_ms.unwrap_or(0);
    let w_count = payload.word_count.unwrap_or(12);

    let running_flag = state.daemon_running.clone();
    let phrases_counter = state.daemon_phrases.clone();
    let addrs_counter = state.daemon_addrs.clone();
    let hits_counter = state.daemon_hits.clone();
    let speed_counter = state.daemon_speed.clone();
    let recent_hits = state.recent_hits.clone();
    let engine = state.engine.clone();
    let index = state.index.clone();

    tokio::spawn(async move {
        use rand::Rng;
        use bip39::Mnemonic;
        use rayon::prelude::*;

        let mut last_speed_check = Instant::now();
        let mut last_phrases = 0u64;

        while running_flag.load(Ordering::SeqCst) {
            let phrases: Vec<String> = (0..b_size)
                .into_par_iter()
                .map(|_| {
                    let mut rng = rand::thread_rng();
                    if w_count == 24 {
                        let mut entropy = [0u8; 32];
                        rng.fill(&mut entropy);
                        Mnemonic::from_entropy(&entropy).unwrap().to_string()
                    } else {
                        let mut entropy = [0u8; 16];
                        rng.fill(&mut entropy);
                        Mnemonic::from_entropy(&entropy).unwrap().to_string()
                    }
                })
                .collect();

            let phrases_count = phrases.len() as u64;

            let engine_ref = &engine;
            let index_ref = &index;

            let hits_this_tick: Vec<WalletHit> = phrases
                .par_iter()
                .flat_map(|p| {
                    let mut hits = Vec::new();
                    if let Ok(derived) = engine_ref.derive_phrase(p, 1) {
                        // Check ETH
                        if let Some(orig) = index_ref.find_origin(&derived.primary_eth) {
                            let rec = index_ref.fetch_record_from_path(&orig, &derived.primary_eth);
                            hits.push(WalletHit {
                                phrase: p.clone(),
                                matched_address: derived.primary_eth.clone(),
                                matched_type: "ETH".to_string(),
                                derivation_index: 0,
                                database_source: orig.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                recorded_balance: rec.as_ref().and_then(|r| r.balance.clone()),
                                recorded_tx_count: rec.as_ref().and_then(|r| r.tx_count),
                                live_balance: None,
                                live_tx_count: None,
                                found_timestamp: crate::config::now_iso(),
                            });
                        }
                        // Check BTC SegWit
                        if let Some(orig) = index_ref.find_origin(&derived.primary_btc_segwit) {
                            let rec = index_ref.fetch_record_from_path(&orig, &derived.primary_btc_segwit);
                            hits.push(WalletHit {
                                phrase: p.clone(),
                                matched_address: derived.primary_btc_segwit.clone(),
                                matched_type: "BTC_SEGWIT".to_string(),
                                derivation_index: 0,
                                database_source: orig.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                recorded_balance: rec.as_ref().and_then(|r| r.balance.clone()),
                                recorded_tx_count: rec.as_ref().and_then(|r| r.tx_count),
                                live_balance: None,
                                live_tx_count: None,
                                found_timestamp: crate::config::now_iso(),
                            });
                        }
                        // Check BTC Legacy
                        if let Some(orig) = index_ref.find_origin(&derived.primary_btc_legacy) {
                            let rec = index_ref.fetch_record_from_path(&orig, &derived.primary_btc_legacy);
                            hits.push(WalletHit {
                                phrase: p.clone(),
                                matched_address: derived.primary_btc_legacy.clone(),
                                matched_type: "BTC_LEGACY".to_string(),
                                derivation_index: 0,
                                database_source: orig.file_name().unwrap_or_default().to_string_lossy().to_string(),
                                recorded_balance: rec.as_ref().and_then(|r| r.balance.clone()),
                                recorded_tx_count: rec.as_ref().and_then(|r| r.tx_count),
                                live_balance: None,
                                live_tx_count: None,
                                found_timestamp: crate::config::now_iso(),
                            });
                        }
                    }
                    hits
                })
                .collect();

            if !hits_this_tick.is_empty() {
                hits_counter.fetch_add(hits_this_tick.len() as u64, Ordering::SeqCst);
                let mut guard = recent_hits.write().await;
                for h in &hits_this_tick {
                    guard.insert(0, h.clone());
                    if guard.len() > 50 {
                        guard.pop();
                    }

                    // Save to master active_wallets.duckdb found_duckdb_wallets table
                    let _ = index.save_found_wallet(
                        &h.phrase,
                        &h.matched_address,
                        &h.matched_type,
                        h.recorded_balance.as_deref(),
                        h.recorded_tx_count,
                        &h.database_source,
                        Some(&h.matched_address),
                        None,
                        None,
                    );

                    // Dispatch instant Telegram "Goat found" alert
                    crate::scanner::daemon::send_telegram_goat_alert(
                        &h.matched_address,
                        &h.matched_type,
                        h.recorded_balance.as_deref(),
                        &h.database_source,
                        "24/7 Autonomous Daemon",
                    ).await;
                }
            }

            phrases_counter.fetch_add(phrases_count, Ordering::SeqCst);
            addrs_counter.fetch_add(phrases_count * 3, Ordering::SeqCst);

            let elapsed_sec = last_speed_check.elapsed().as_secs_f64();
            if elapsed_sec >= 1.0 {
                let current_p = phrases_counter.load(Ordering::SeqCst);
                let diff = current_p - last_phrases;
                let sp = ((diff * 3) as f64 / elapsed_sec).round() as u64;
                speed_counter.store(sp, Ordering::SeqCst);
                last_phrases = current_p;
                last_speed_check = Instant::now();
            }

            if interval > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(interval)).await;
            } else {
                tokio::task::yield_now().await;
            }
        }
    });

    Json(serde_json::json!({
        "success": true,
        "message": "24/7 Autonomous Scanner Daemon started successfully."
    }))
}

async fn handle_daemon_stop(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    state.daemon_running.store(false, Ordering::SeqCst);
    *state.daemon_start_instant.write().await = None;
    state.daemon_speed.store(0, Ordering::SeqCst);
    Json(serde_json::json!({
        "success": true,
        "message": "Daemon stop signaled."
    }))
}

// -----------------------------------------------------------------------------
// Importer Handlers
// -----------------------------------------------------------------------------

async fn handle_import_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let running = state.import_running.load(Ordering::SeqCst);
    let bytes = state.import_bytes.load(Ordering::SeqCst);
    let total = state.import_total_bytes.load(Ordering::SeqCst);
    let rows = state.import_rows.load(Ordering::SeqCst);
    let speed = state.import_speed.load(Ordering::SeqCst);
    let chain = state.import_chain.read().await.clone();
    let era = state.import_era.read().await.clone();
    let error = state.import_error.read().await.clone();

    let percent = if total > 0 {
        ((bytes as f64 / total as f64) * 100.0).min(100.0).round() as u8
    } else {
        0
    };

    Json(serde_json::json!({
        "isRunning": running,
        "chain": chain,
        "era": era,
        "bytesDownloaded": bytes,
        "totalBytes": total,
        "percent": percent,
        "rowsImported": rows,
        "speedRowsPerSec": speed,
        "error": error
    }))
}

#[derive(Deserialize)]
struct ImportStartReq {
    chain: Option<String>,
    era: Option<String>,
    limit: Option<usize>,
    custom_url: Option<String>,
}

async fn handle_import_start(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ImportStartReq>,
) -> impl IntoResponse {
    if state.import_running.load(Ordering::SeqCst) {
        return Json(serde_json::json!({
            "success": false,
            "message": "An import is already running."
        }));
    }

    let chain_str = payload.chain.unwrap_or_else(|| "both".to_string());
    let era_str = payload.era.unwrap_or_else(|| "all".to_string());
    let era_enum = match era_str.to_lowercase().as_str() {
        "satoshi" => SnapshotEra::Satoshi,
        "early" => SnapshotEra::Early,
        "modern" => SnapshotEra::Modern,
        "present" => SnapshotEra::Present,
        _ => SnapshotEra::All,
    };

    *state.import_chain.write().await = Some(chain_str.clone());
    *state.import_era.write().await = Some(era_str);
    *state.import_error.write().await = None;
    state.import_running.store(true, Ordering::SeqCst);
    state.import_rows.store(0, Ordering::SeqCst);
    state.import_bytes.store(0, Ordering::SeqCst);
    state.import_total_bytes.store(0, Ordering::SeqCst);
    state.import_speed.store(0, Ordering::SeqCst);

    let data_dir = state.data_dir.clone();
    let running_flag = state.import_running.clone();
    let bytes_counter = state.import_bytes.clone();
    let total_bytes_counter = state.import_total_bytes.clone();
    let rows_counter = state.import_rows.clone();
    let speed_counter = state.import_speed.clone();
    let err_holder = state.import_error.clone();

    tokio::spawn(async move {
        match crate::importer::SnapshotImporter::run_pipeline(
            &chain_str,
            era_enum,
            payload.limit,
            payload.custom_url.as_deref(),
            &data_dir,
            Some(bytes_counter),
            Some(total_bytes_counter),
            Some(rows_counter.clone()),
            Some(speed_counter),
        ).await {
            Ok(count) => {
                println!("Snapshot import completed successfully ({} rows).", count);
                rows_counter.store(count as u64, Ordering::SeqCst);
            }
            Err(e) => {
                eprintln!("Snapshot import error: {}", e);
                *err_holder.write().await = Some(e.to_string());
            }
        }
        running_flag.store(false, Ordering::SeqCst);
    });

    Json(serde_json::json!({
        "success": true,
        "message": "Snapshot import initiated."
    }))
}

async fn handle_found_wallets(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let hits = state.recent_hits.read().await.clone();
    Json(hits)
}

