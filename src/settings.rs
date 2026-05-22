use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokscale_core::scanner::ScannerSettings;

pub fn default_settings_path(home_dir: &Path) -> PathBuf {
    home_dir.join(".config/tokscale/settings.json")
}

pub fn macos_settings_path(home_dir: &Path) -> PathBuf {
    home_dir.join("Library/Application Support/tokscale/settings.json")
}

fn settings_path_candidates(home_dir: &Path) -> Vec<PathBuf> {
    let default_path = default_settings_path(home_dir);
    let macos_path = macos_settings_path(home_dir);
    if default_path == macos_path {
        vec![default_path]
    } else {
        vec![default_path, macos_path]
    }
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
    let app_settings = AppSettings {
        scanner: scanner.clone(),
    };
    let content = serde_json::to_vec_pretty(&app_settings)?;
    let mut written_path = None;

    for path in settings_path_candidates(home_dir) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &content)?;
        written_path.get_or_insert(path);
    }

    Ok(written_path.expect("settings_path_candidates is never empty"))
}
