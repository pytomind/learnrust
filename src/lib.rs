// =============================================================================
// LearnRust Library Interface
// =============================================================================
pub mod config;
pub mod crypto;
pub mod database;
pub mod importer;
pub mod live;
pub mod recovery;
pub mod scanner;
pub mod server;

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}


