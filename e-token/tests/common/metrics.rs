//! Record compute-unit usage from `BanksClient::process_transaction_with_metadata` during tests.
//!
//! Set [`ENV_METRICS_JSON_PATH`] to override the output path (default `metrics/current.json`).
//! Metric keys are short `area::step` strings (for example `del_eata::delegate`, `tq_auto::crank_1`).
//! Relative paths are resolved from the **workspace** root (the parent of this crate’s manifest).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use fslock::LockFile;
use serde::{Deserialize, Serialize};
use solana_program_test::{BanksClient, BanksClientError, BanksTransactionResultWithMetadata};

/// Environment variable overriding the metrics JSON path (default `metrics/current.json`).
pub const ENV_METRICS_JSON_PATH: &str = "METRICS_PATH";
const DEFAULT_METRICS_JSON_PATH: &str = "metrics/current.json";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR must have a parent (workspace root)")
        .to_path_buf()
}

fn metrics_json_path() -> PathBuf {
    let raw = std::env::var(ENV_METRICS_JSON_PATH)
        .unwrap_or_else(|_| DEFAULT_METRICS_JSON_PATH.to_string());
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        workspace_root().join(path)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuEntry {
    pub compute_units_consumed: u64,
}

fn write_entries(map: &HashMap<String, CuEntry>) {
    let path = metrics_json_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(map).expect("metrics JSON serialize");
    fs::write(&path, json).expect("metrics JSON write");
}

fn lock_path() -> PathBuf {
    metrics_json_path().with_extension("json.lock")
}

fn with_file_lock<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let path = lock_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("metrics lock dir create");
    }
    let mut lock = LockFile::open(&path).expect("metrics lock file open");
    lock.lock().expect("metrics file lock");
    let out = f();
    lock.unlock().expect("metrics file unlock");
    out
}

/// Merge `compute_units_consumed` under `entry_key` into the metrics file.
pub fn record_compute_units(entry_key: &str, compute_units_consumed: u64) {
    with_file_lock(|| {
        let path = metrics_json_path();
        let mut map: HashMap<String, CuEntry> = if path.exists() {
            let s = fs::read_to_string(&path).expect("metrics JSON read");
            serde_json::from_str(&s).expect("metrics JSON parse")
        } else {
            HashMap::new()
        };
        if let Some(_) = map.insert(
            entry_key.to_string(),
            CuEntry {
                compute_units_consumed,
            },
        ) {
            // If the env var is explicitly set, panic if the entry key already exists.
            if std::env::var(ENV_METRICS_JSON_PATH).is_ok() {
                panic!("entry_key already exists: {entry_key}");
            }
        };
        write_entries(&map);
    });
}

/// Append CU from transaction metadata (if any) under `entry_key`.
pub fn record_compute_units_from_result(entry_key: &str, res: &BanksTransactionResultWithMetadata) {
    let cu = res
        .metadata
        .as_ref()
        .map(|m| m.compute_units_consumed)
        .unwrap_or(0);
    record_compute_units(entry_key, cu);
}

/// Run `process_transaction_with_metadata`, record CU, then return the full result (success or failure).
pub async fn process_transaction_with_metadata_recorded(
    banks_client: &BanksClient,
    tx: impl Into<solana_transaction::versioned::VersionedTransaction>,
    entry_key: &str,
) -> Result<BanksTransactionResultWithMetadata, BanksClientError> {
    let res = banks_client.process_transaction_with_metadata(tx).await?;
    record_compute_units_from_result(entry_key, &res);
    Ok(res)
}

/// Process a transaction, record CU, and propagate transaction failure after recording.
pub async fn process_transaction_record_cu(
    banks_client: &BanksClient,
    tx: impl Into<solana_transaction::versioned::VersionedTransaction>,
    entry_key: &str,
) -> Result<(), BanksClientError> {
    let res = process_transaction_with_metadata_recorded(banks_client, tx, entry_key).await?;
    res.result.map_err(BanksClientError::from)
}
