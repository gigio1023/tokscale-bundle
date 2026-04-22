use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleManifest {
    pub format_version: u32,
    pub created_at: DateTime<Utc>,
    pub host: String,
    pub archive_kind: String,
    pub entries: Vec<BundleEntry>,
    pub replay: ReplayConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BundleEntry {
    pub client: String,
    pub source_path: PathBuf,
    pub archive_path: PathBuf,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReplayConfig {
    pub extra_scan_roots: BTreeMap<String, Vec<PathBuf>>,
    pub opencode_db_paths: Vec<PathBuf>,
}
