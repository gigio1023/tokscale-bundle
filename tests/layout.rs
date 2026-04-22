use std::collections::BTreeMap;
use std::path::PathBuf;
use tokscale_bundle::layout::{build_import_scanner_settings, extra_client_replay_root};
use tokscale_bundle::manifest::ReplayConfig;

#[test]
fn extra_client_replay_root_uses_deterministic_bundle_namespace() {
    let bundle_home = PathBuf::from("/tmp/imported/home");
    let root = extra_client_replay_root(&bundle_home, "codex");

    assert_eq!(
        root,
        PathBuf::from("/tmp/imported/home/.tokscale-bundle/extra/codex")
    );
}

#[test]
fn build_import_scanner_settings_maps_extra_roots_and_opencode_dbs() {
    let bundle_home = PathBuf::from("/tmp/imported/home");
    let replay = ReplayConfig {
        extra_scan_roots: BTreeMap::from([
            (
                "codex".to_string(),
                vec![PathBuf::from(".tokscale-bundle/extra/codex")],
            ),
            (
                "claude".to_string(),
                vec![PathBuf::from(".tokscale-bundle/extra/claude")],
            ),
        ]),
        opencode_db_paths: vec![PathBuf::from(".tokscale-bundle/opencode/opencode.db")],
    };

    let settings = build_import_scanner_settings(&bundle_home, &replay);

    assert_eq!(
        settings.extra_scan_paths.get("codex"),
        Some(&vec![PathBuf::from(
            "/tmp/imported/home/.tokscale-bundle/extra/codex"
        )])
    );
    assert_eq!(
        settings.opencode_db_paths,
        vec![PathBuf::from(
            "/tmp/imported/home/.tokscale-bundle/opencode/opencode.db"
        )]
    );
}
