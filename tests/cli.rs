use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::{Builder, TempDir};
use tokscale_bundle::archive::write_bundle_zip;
use tokscale_bundle::manifest::{BundleManifest, ReplayConfig};
use tokscale_bundle::settings::{default_settings_path, load_scanner_settings};
use tokscale_core::scanner::scan_all_clients_with_scanner_settings;
use zip::ZipArchive;

fn bundle_manifest(replay: ReplayConfig) -> BundleManifest {
    BundleManifest {
        format_version: 1,
        created_at: chrono::Utc::now(),
        host: "test-host".to_string(),
        archive_kind: "zip".to_string(),
        entries: Vec::new(),
        replay,
    }
}

fn write_file(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn bundle_fixture(replay: ReplayConfig, files: &[(&str, &[u8])]) -> (TempDir, PathBuf) {
    let bundle_dir = tempfile::tempdir().unwrap();
    for (relative, contents) in files {
        write_file(&bundle_dir.path().join(relative), contents);
    }

    let manifest = bundle_manifest(replay);
    fs::write(
        bundle_dir.path().join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let archive_dir = tempfile::tempdir().unwrap();
    let archive_path = archive_dir.path().join("bundle.zip");
    write_bundle_zip(bundle_dir.path(), &archive_path).unwrap();

    (archive_dir, archive_path)
}

fn read_zip_entry(archive_path: &Path, entry_name: &str) -> Vec<u8> {
    let file = fs::File::open(archive_path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name(entry_name).unwrap();
    let mut contents = Vec::new();
    entry.read_to_end(&mut contents).unwrap();
    contents
}

fn zip_entry_names(archive_path: &Path) -> Vec<String> {
    let file = fs::File::open(archive_path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect()
}

fn command_output(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_tokscale-bundle"))
        .args(args)
        .output()
        .unwrap()
}

fn parse_output_path(stdout: &str, prefix: &str) -> PathBuf {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(PathBuf::from)
        .unwrap()
}

#[test]
fn cli_help_no_longer_mentions_submit() {
    let help = command_output(&["--help"]);
    assert!(help.status.success());

    let stdout = String::from_utf8(help.stdout).unwrap();
    assert!(!stdout.contains("submit"));

    let submit = command_output(&["submit", "--help"]);
    assert!(!submit.status.success());
}

#[test]
fn unpack_materializes_settings_and_prints_manual_submit_steps() {
    let replay = ReplayConfig {
        extra_scan_roots: std::collections::BTreeMap::from([(
            "codex".to_string(),
            vec![PathBuf::from(".tokscale-bundle/extra/codex")],
        )]),
        opencode_db_paths: vec![PathBuf::from(
            ".tokscale-bundle/opencode/source/opencode.db",
        )],
    };

    let (_archive_dir, archive_path) = bundle_fixture(
        replay,
        &[
            (
                "home/.tokscale-bundle/extra/codex/source/.codex/sessions/demo.jsonl",
                b"{}\n",
            ),
            (
                "home/.tokscale-bundle/opencode/source/opencode.db",
                b"sqlite",
            ),
        ],
    );

    let output = command_output(&["unpack", archive_path.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("unpack_root="));
    assert!(stdout.contains("fake_home="));
    assert!(stdout.contains("manual submit"));
    assert!(stdout.contains("imported-only"));
    assert!(stdout.contains("mkdir -p"));
    assert!(stdout.contains("cp ~/.config/tokscale/credentials.json"));
    assert!(stdout.contains("HOME=\""));
    assert!(stdout.contains("tokscale submit --no-spinner"));

    let unpack_root = parse_output_path(&stdout, "unpack_root=");
    let fake_home = parse_output_path(&stdout, "fake_home=");
    let settings_path = default_settings_path(&fake_home);
    assert!(unpack_root.join("manifest.json").is_file());
    assert!(fake_home.is_dir());
    assert!(settings_path.is_file());

    let settings = load_scanner_settings(&fake_home);
    assert_eq!(
        settings.extra_scan_paths.get("codex"),
        Some(&vec![fake_home.join(".tokscale-bundle/extra/codex")])
    );
    assert_eq!(
        settings.opencode_db_paths,
        vec![fake_home.join(".tokscale-bundle/opencode/source/opencode.db")]
    );

    let scan_result = scan_all_clients_with_scanner_settings(
        fake_home.to_str().unwrap(),
        &["codex".to_string(), "opencode".to_string()],
        false,
        &settings,
    );
    assert_eq!(scan_result.get(tokscale_core::ClientId::Codex).len(), 1);
    assert_eq!(scan_result.opencode_dbs, settings.opencode_db_paths);

    fs::remove_dir_all(unpack_root).unwrap();
}

#[test]
fn cleanup_removes_valid_unpack_root_and_rejects_invalid_paths() {
    let valid_dir = Builder::new().prefix("tokscale-bundle-").tempdir().unwrap();
    write_file(&valid_dir.path().join("manifest.json"), b"{}");
    fs::create_dir_all(valid_dir.path().join("home")).unwrap();

    let valid_path = valid_dir.path().to_path_buf();
    let home_path = valid_path.join("home");
    let output = command_output(&["cleanup", home_path.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(valid_path.exists());

    let output = command_output(&["cleanup", valid_path.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    assert!(!valid_path.exists());

    let invalid_dir = tempfile::tempdir().unwrap();
    let invalid_path = invalid_dir.path().to_path_buf();
    let output = command_output(&["cleanup", invalid_path.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(invalid_path.exists());
}

#[test]
fn export_archive_contains_special_case_replay_paths() {
    let source_home = tempfile::tempdir().unwrap();
    let archive_dir = tempfile::tempdir().unwrap();
    let archive_path = archive_dir.path().join("bundle.zip");

    write_file(
        &source_home.path().join(".codex/sessions/session.jsonl"),
        b"{}\n",
    );
    write_file(
        &source_home.path().join(".local/share/opencode/opencode.db"),
        b"sqlite",
    );
    write_file(
        &source_home.path().join(".local/share/octofriend/sqlite.db"),
        b"sqlite",
    );
    write_file(
        &source_home.path().join(".local/share/kilo/kilo.db"),
        b"sqlite",
    );
    write_file(&source_home.path().join(".hermes/state.db"), b"sqlite");

    let crush_project = source_home
        .path()
        .join("workspace/project-a/.crush/crush.db");
    write_file(&crush_project, b"sqlite");
    write_file(
        &source_home.path().join(".local/share/crush/projects.json"),
        format!(
            "{{\"projects\":[{{\"path\":\"{}\",\"data_dir\":\".crush\"}}]}}",
            source_home.path().join("workspace/project-a").display()
        )
        .as_bytes(),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_tokscale-bundle"))
        .args(["export", "--output", archive_path.to_str().unwrap()])
        .env("HOME", source_home.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let entry_names = zip_entry_names(&archive_path);
    assert!(
        entry_names
            .iter()
            .any(|name| name == "home/.local/share/octofriend/sqlite.db")
    );
    assert!(
        entry_names
            .iter()
            .any(|name| name == "home/.local/share/kilo/kilo.db")
    );
    assert!(
        entry_names
            .iter()
            .any(|name| name == "home/.hermes/state.db")
    );
    assert!(
        entry_names
            .iter()
            .any(|name| name == "home/.local/share/crush/projects.json")
    );
    assert!(
        entry_names
            .iter()
            .any(|name| name.contains("home/.tokscale-bundle/crush"))
    );
    assert!(
        entry_names
            .iter()
            .any(|name| name.contains("home/.tokscale-bundle/opencode"))
    );
    assert!(
        entry_names
            .iter()
            .any(|name| name.contains("home/.tokscale-bundle/extra/codex"))
    );

    let manifest_bytes = read_zip_entry(&archive_path, "manifest.json");
    let manifest: BundleManifest = serde_json::from_slice(&manifest_bytes).unwrap();
    assert!(manifest.replay.extra_scan_roots.contains_key("codex"));
    assert!(!manifest.replay.opencode_db_paths.is_empty());
}
