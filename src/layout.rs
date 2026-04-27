use crate::manifest::ReplayConfig;
use std::path::{Path, PathBuf};
use tokscale_core::scanner::ScannerSettings;

pub fn extra_client_replay_root(bundle_home: &Path, client: &str) -> PathBuf {
    bundle_home
        .join(".tokscale-bundle")
        .join("extra")
        .join(client)
}

pub fn build_import_scanner_settings(bundle_home: &Path, replay: &ReplayConfig) -> ScannerSettings {
    ScannerSettings {
        opencode_db_paths: replay
            .opencode_db_paths
            .iter()
            .map(|path| {
                if path.is_absolute() {
                    path.clone()
                } else {
                    bundle_home.join(path)
                }
            })
            .collect(),
        extra_scan_paths: replay
            .extra_scan_roots
            .iter()
            .map(|(client, roots)| {
                let resolved = roots
                    .iter()
                    .map(|root| {
                        if root.is_absolute() {
                            root.clone()
                        } else {
                            bundle_home.join(root)
                        }
                    })
                    .collect::<Vec<_>>();
                (client.clone(), resolved)
            })
            .collect(),
    }
}
