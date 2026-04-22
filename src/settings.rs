use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokscale_core::scanner::ScannerSettings;

pub fn default_settings_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".config/tokscale/settings.json")
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct AppSettings {
    scanner: ScannerSettings,
}

pub fn load_scanner_settings(home_dir: &Path) -> ScannerSettings {
    let path = default_settings_path(home_dir);
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ScannerSettings::default();
    };

    serde_json::from_str::<AppSettings>(&raw)
        .map(|settings| settings.scanner)
        .unwrap_or_default()
}

pub fn write_scanner_settings(home_dir: &Path, scanner: &ScannerSettings) -> Result<PathBuf> {
    let path = default_settings_path(home_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let app_settings = AppSettings {
        scanner: scanner.clone(),
    };
    std::fs::write(&path, serde_json::to_vec_pretty(&app_settings)?)?;
    Ok(path)
}
