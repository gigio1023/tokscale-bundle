use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::Builder;
use tokscale_bundle::hub::merge_submit_payloads;
use tokscale_bundle::manifest::BundleManifest;
use tokscale_bundle::settings::{
    default_settings_path, load_scanner_settings, macos_settings_path,
};
use tokscale_core::ClientId;
use tokscale_core::scanner::scan_all_clients_with_scanner_settings;
use zip::ZipArchive;

fn write_file(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn run_bundle(args: &[&str], home: Option<&Path>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tokscale-bundle"));
    command
        .args(args)
        .env_remove("TOKSCALE_EXTRA_DIRS")
        .env_remove("CODEX_HOME")
        .env_remove("HERMES_HOME")
        .env_remove("XDG_DATA_HOME");

    if let Some(home) = home {
        command
            .env("HOME", home)
            .env("CODEX_HOME", home.join(".codex"))
            .env("HERMES_HOME", home.join(".hermes"))
            .env("XDG_DATA_HOME", home.join(".local/share"));
    }

    command.output().unwrap()
}

fn parse_output_path(stdout: &str, prefix: &str) -> PathBuf {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(PathBuf::from)
        .unwrap()
}

fn zip_entry_names(archive_path: &Path) -> Vec<String> {
    let file = fs::File::open(archive_path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    (0..archive.len())
        .map(|index| archive.by_index(index).unwrap().name().to_string())
        .collect()
}

fn read_zip_entry(archive_path: &Path, entry_name: &str) -> Vec<u8> {
    let file = fs::File::open(archive_path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name(entry_name).unwrap();
    let mut contents = Vec::new();
    entry.read_to_end(&mut contents).unwrap();
    contents
}

fn seed_source_home(home: &Path) {
    write_file(&home.join(".codex/sessions/source.jsonl"), b"{}\n");
    write_file(&home.join(".local/share/opencode/opencode.db"), b"sqlite");
    write_file(&home.join(".local/share/octofriend/sqlite.db"), b"sqlite");
    write_file(&home.join(".local/share/kilo/kilo.db"), b"sqlite");
    write_file(&home.join(".hermes/state.db"), b"sqlite");
    write_file(
        &home.join(".config/tokscale/credentials.json"),
        br#"{"token":"must-not-be-bundled"}"#,
    );

    let project_root = home.join("workspace/project-a");
    write_file(&project_root.join(".crush/crush.db"), b"sqlite");
    write_file(
        &home.join(".local/share/crush/projects.json"),
        format!(
            "{{\"projects\":[{{\"path\":\"{}\",\"data_dir\":\".crush\"}}]}}",
            project_root.display()
        )
        .as_bytes(),
    );
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn submit_payload(contributions: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "meta": {
            "generatedAt": "2026-05-22T00:00:00Z",
            "version": "2.1.3",
            "dateRange": {
                "start": "2026-05-21",
                "end": "2026-05-22"
            }
        },
        "summary": {
            "totalTokens": 0,
            "totalCost": 0.0,
            "totalDays": 0,
            "activeDays": 0,
            "averagePerDay": 0.0,
            "maxCostInSingleDay": 0.0,
            "clients": [],
            "models": []
        },
        "years": [],
        "contributions": contributions
    })
}

#[test]
fn payload_proxy_merge_sums_same_day_client_model_from_multiple_devices() {
    let device_a = submit_payload(serde_json::json!([
        {
            "date": "2026-05-22",
            "totals": { "tokens": 15, "cost": 0.15, "messages": 1 },
            "intensity": 4,
            "tokenBreakdown": {
                "input": 10,
                "output": 5,
                "cacheRead": 0,
                "cacheWrite": 0,
                "reasoning": 0
            },
            "clients": [
                {
                    "client": "codex",
                    "modelId": "gpt-5",
                    "providerId": "openai",
                    "tokens": {
                        "input": 10,
                        "output": 5,
                        "cacheRead": 0,
                        "cacheWrite": 0,
                        "reasoning": 0
                    },
                    "cost": 0.15,
                    "messages": 1
                }
            ]
        }
    ]));
    let device_b = submit_payload(serde_json::json!([
        {
            "date": "2026-05-22",
            "totals": { "tokens": 30, "cost": 0.30, "messages": 2 },
            "intensity": 4,
            "tokenBreakdown": {
                "input": 20,
                "output": 10,
                "cacheRead": 0,
                "cacheWrite": 0,
                "reasoning": 0
            },
            "clients": [
                {
                    "client": "codex",
                    "modelId": "gpt-5",
                    "providerId": "openai",
                    "tokens": {
                        "input": 20,
                        "output": 10,
                        "cacheRead": 0,
                        "cacheWrite": 0,
                        "reasoning": 0
                    },
                    "cost": 0.30,
                    "messages": 2
                }
            ]
        }
    ]));

    let merged = merge_submit_payloads(&[device_a, device_b]).unwrap();
    assert_eq!(merged["summary"]["totalTokens"], 45);
    assert_eq!(merged["summary"]["activeDays"], 1);
    assert_eq!(merged["summary"]["clients"], serde_json::json!(["codex"]));
    assert_eq!(merged["summary"]["models"], serde_json::json!(["gpt-5"]));
    assert_eq!(merged["contributions"].as_array().unwrap().len(), 1);

    let day = &merged["contributions"][0];
    assert_eq!(day["totals"]["tokens"], 45);
    assert_eq!(day["totals"]["messages"], 3);
    assert_eq!(day["clients"].as_array().unwrap().len(), 1);
    assert_eq!(day["clients"][0]["tokens"]["input"], 30);
    assert_eq!(day["clients"][0]["tokens"]["output"], 15);
    assert!((day["clients"][0]["cost"].as_f64().unwrap() - 0.45).abs() < 0.00001);
    assert_eq!(day["clients"][0]["messages"], 3);
}

#[test]
fn cli_flow_builds_a_combined_fake_home_without_credentials() {
    let source_home = tempfile::tempdir().unwrap();
    let archive_dir = tempfile::tempdir().unwrap();
    let archive_path = archive_dir.path().join("bundle.zip");
    seed_source_home(source_home.path());

    let output = run_bundle(
        &["export", "--output", archive_path.to_str().unwrap()],
        Some(source_home.path()),
    );
    assert_success(&output);

    let entries = zip_entry_names(&archive_path);
    assert!(entries.iter().any(|entry| entry == "manifest.json"));
    assert!(
        entries
            .iter()
            .any(|entry| entry.contains("home/.tokscale-bundle/extra/codex"))
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.contains("home/.tokscale-bundle/opencode"))
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry == "home/.local/share/kilo/kilo.db")
    );
    assert!(entries.iter().any(|entry| entry == "home/.hermes/state.db"));
    assert!(
        entries
            .iter()
            .any(|entry| entry == "home/.local/share/crush/projects.json")
    );
    assert!(
        !entries
            .iter()
            .any(|entry| entry.ends_with("credentials.json"))
    );

    let manifest: BundleManifest =
        serde_json::from_slice(&read_zip_entry(&archive_path, "manifest.json")).unwrap();
    assert_eq!(manifest.format_version, 1);
    assert!(manifest.entries.len() >= 6);
    assert!(manifest.replay.extra_scan_roots.contains_key("codex"));
    assert!(!manifest.replay.opencode_db_paths.is_empty());

    let output = run_bundle(&["unpack", archive_path.to_str().unwrap()], None);
    assert_success(&output);

    let stdout = String::from_utf8(output.stdout).unwrap();
    let unpack_root = parse_output_path(&stdout, "unpack_root=");
    let fake_home = parse_output_path(&stdout, "fake_home=");
    assert!(unpack_root.join("manifest.json").is_file());
    assert!(default_settings_path(&fake_home).is_file());
    assert!(macos_settings_path(&fake_home).is_file());
    assert!(!fake_home.join(".config/tokscale/credentials.json").exists());

    let settings = load_scanner_settings(&fake_home);
    let scan_result = scan_all_clients_with_scanner_settings(
        fake_home.to_str().unwrap(),
        &["codex".to_string(), "opencode".to_string()],
        false,
        &settings,
    );
    assert_eq!(scan_result.get(ClientId::Codex).len(), 1);
    assert_eq!(scan_result.opencode_dbs.len(), 1);

    let local_home = tempfile::tempdir().unwrap();
    write_file(
        &local_home.path().join(".codex/sessions/local.jsonl"),
        b"{}\n",
    );

    let output = run_bundle(
        &["add-local", unpack_root.to_str().unwrap()],
        Some(local_home.path()),
    );
    assert_success(&output);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("combined submit: imported + local replay"));
    assert!(stdout.contains("bunx tokscale@latest submit --dry-run"));

    let settings = load_scanner_settings(&fake_home);
    let scan_result = scan_all_clients_with_scanner_settings(
        fake_home.to_str().unwrap(),
        &["codex".to_string()],
        false,
        &settings,
    );
    assert_eq!(scan_result.get(ClientId::Codex).len(), 2);
    assert!(!fake_home.join(".config/tokscale/credentials.json").exists());

    let output = run_bundle(&["cleanup", unpack_root.to_str().unwrap()], None);
    assert_success(&output);
    assert!(!unpack_root.exists());
}

#[test]
fn cleanup_removes_only_valid_unpack_roots() {
    let valid_dir = Builder::new().prefix("tokscale-bundle-").tempdir().unwrap();
    write_file(&valid_dir.path().join("manifest.json"), b"{}");
    fs::create_dir_all(valid_dir.path().join("home")).unwrap();

    let valid_root = valid_dir.path().to_path_buf();
    let nested_home = valid_root.join("home");
    let output = run_bundle(&["cleanup", nested_home.to_str().unwrap()], None);
    assert!(!output.status.success());
    assert!(valid_root.exists());

    let unrelated_dir = tempfile::tempdir().unwrap();
    let output = run_bundle(&["cleanup", unrelated_dir.path().to_str().unwrap()], None);
    assert!(!output.status.success());
    assert!(unrelated_dir.path().exists());

    let output = run_bundle(&["cleanup", valid_root.to_str().unwrap()], None);
    assert_success(&output);
    assert!(!valid_root.exists());
}
