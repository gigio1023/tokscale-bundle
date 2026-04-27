use crate::archive::write_bundle_zip;
use crate::layout::build_import_scanner_settings;
use crate::manifest::{BundleEntry, BundleManifest, ReplayConfig};
use crate::settings::{load_scanner_settings, write_scanner_settings};
use anyhow::{Context, Result, bail};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokscale_core::ClientId;
use tokscale_core::scanner::{CrushDbSource, ScanResult, scan_all_clients_with_scanner_settings};

pub struct AddLocalResult {
    pub unpack_root: PathBuf,
    pub fake_home: PathBuf,
    pub settings_path: PathBuf,
    pub added_entries: usize,
    pub skipped_sources: Vec<String>,
}

fn parse_local_clients() -> Vec<String> {
    let mut clients: Vec<String> = ClientId::iter()
        .filter(|client| client.parse_local())
        .map(|client| client.as_str().to_string())
        .collect();
    clients.push("synthetic".to_string());
    clients
}

fn supports_extra_root_replay(client: ClientId) -> bool {
    !matches!(client, ClientId::Kilo | ClientId::Crush | ClientId::Hermes)
}

fn path_without_root_slash(path: &Path) -> PathBuf {
    path.strip_prefix("/").unwrap_or(path).to_path_buf()
}

fn sha256_hex(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

fn stage_file(
    bundle_root: &Path,
    source_path: &Path,
    destination_path: &Path,
    client: &str,
    entries: &mut Vec<BundleEntry>,
) -> Result<()> {
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source_path, destination_path)?;

    entries.push(BundleEntry {
        client: client.to_string(),
        source_path: source_path.to_path_buf(),
        archive_path: destination_path.strip_prefix(bundle_root)?.to_path_buf(),
        sha256: sha256_hex(source_path)?,
    });

    Ok(())
}

fn stage_supported_client_files(
    bundle_root: &Path,
    bundle_home: &Path,
    client: ClientId,
    files: &[PathBuf],
    replay: &mut ReplayConfig,
    entries: &mut Vec<BundleEntry>,
) -> Result<()> {
    let relative_root = PathBuf::from(".tokscale-bundle")
        .join("extra")
        .join(client.as_str());
    stage_supported_client_files_under(
        bundle_root,
        bundle_home,
        client,
        files,
        relative_root,
        replay,
        entries,
    )
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn stage_supported_client_files_under(
    bundle_root: &Path,
    bundle_home: &Path,
    client: ClientId,
    files: &[PathBuf],
    relative_root: PathBuf,
    replay: &mut ReplayConfig,
    entries: &mut Vec<BundleEntry>,
) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }

    let client_name = client.as_str().to_string();
    let destination_root = bundle_home.join(&relative_root);

    push_unique_path(
        replay
            .extra_scan_roots
            .entry(client_name.clone())
            .or_default(),
        relative_root,
    );

    for file in files {
        let destination = destination_root.join(path_without_root_slash(file));
        stage_file(bundle_root, file, &destination, &client_name, entries)?;
    }

    Ok(())
}

fn stage_special_file(
    bundle_root: &Path,
    bundle_home: &Path,
    source_path: &Path,
    relative_destination: &Path,
    client: &str,
    entries: &mut Vec<BundleEntry>,
) -> Result<()> {
    let destination = bundle_home.join(relative_destination);
    stage_file(bundle_root, source_path, &destination, client, entries)
}

fn stage_opencode_dbs(
    bundle_root: &Path,
    bundle_home: &Path,
    scan_result: &ScanResult,
    replay: &mut ReplayConfig,
    entries: &mut Vec<BundleEntry>,
) -> Result<()> {
    stage_opencode_dbs_under(
        bundle_root,
        bundle_home,
        scan_result,
        Path::new(".tokscale-bundle/opencode"),
        replay,
        entries,
    )
}

fn stage_opencode_dbs_under(
    bundle_root: &Path,
    bundle_home: &Path,
    scan_result: &ScanResult,
    relative_root: &Path,
    replay: &mut ReplayConfig,
    entries: &mut Vec<BundleEntry>,
) -> Result<()> {
    for db in &scan_result.opencode_dbs {
        let relative = relative_root.join(path_without_root_slash(db));
        push_unique_path(&mut replay.opencode_db_paths, relative.clone());
        stage_special_file(bundle_root, bundle_home, db, &relative, "opencode", entries)?;
    }
    Ok(())
}

fn stage_special_file_if_missing(
    bundle_root: &Path,
    bundle_home: &Path,
    source_path: &Path,
    relative_destination: &Path,
    client: &str,
    entries: &mut Vec<BundleEntry>,
    skipped_sources: &mut Vec<String>,
) -> Result<()> {
    let destination = bundle_home.join(relative_destination);
    if destination.exists() {
        skipped_sources.push(format!(
            "{client}: {} already exists in fake home",
            relative_destination.display()
        ));
        return Ok(());
    }

    stage_special_file(
        bundle_root,
        bundle_home,
        source_path,
        relative_destination,
        client,
        entries,
    )
}

fn existing_crush_projects(registry_path: &Path) -> Vec<serde_json::Value> {
    let Ok(raw) = fs::read_to_string(registry_path) else {
        return Vec::new();
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    parsed
        .get("projects")
        .and_then(|projects| projects.as_array())
        .cloned()
        .unwrap_or_default()
}

fn stage_crush_registry(
    bundle_root: &Path,
    bundle_home: &Path,
    crush_dbs: &[CrushDbSource],
    entries: &mut Vec<BundleEntry>,
) -> Result<()> {
    stage_crush_registry_with_mode(bundle_root, bundle_home, crush_dbs, false, entries)
}

fn stage_crush_registry_with_mode(
    bundle_root: &Path,
    bundle_home: &Path,
    crush_dbs: &[CrushDbSource],
    append_existing: bool,
    entries: &mut Vec<BundleEntry>,
) -> Result<()> {
    if crush_dbs.is_empty() {
        return Ok(());
    }

    let registry_path = bundle_home.join(".local/share/crush/projects.json");
    let mut projects = if append_existing {
        existing_crush_projects(&registry_path)
    } else {
        Vec::new()
    };

    for db in crush_dbs {
        let relative = PathBuf::from(".tokscale-bundle")
            .join(if append_existing {
                "local/crush"
            } else {
                "crush"
            })
            .join("crush")
            .join(path_without_root_slash(&db.db_path));
        let destination = bundle_home.join(&relative);
        stage_file(bundle_root, &db.db_path, &destination, "crush", entries)?;

        let data_dir = destination
            .parent()
            .ok_or_else(|| anyhow::anyhow!("crush db destination missing parent"))?;
        let project_root = data_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("crush db destination missing project root"))?;

        projects.push(serde_json::json!({
            "path": project_root.to_string_lossy(),
            "data_dir": data_dir.file_name().unwrap_or_default().to_string_lossy(),
        }));
    }

    if let Some(parent) = registry_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&serde_json::json!({ "projects": projects }))?,
    )?;

    entries.push(BundleEntry {
        client: "crush".to_string(),
        source_path: PathBuf::from("<generated:crush-registry>"),
        archive_path: registry_path.strip_prefix(bundle_root)?.to_path_buf(),
        sha256: sha256_hex(&registry_path)?,
    });

    Ok(())
}

fn read_manifest(unpack_root: &Path) -> Result<BundleManifest> {
    let manifest_path = unpack_root.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

fn write_manifest(bundle_root: &Path, manifest: &BundleManifest) -> Result<()> {
    let manifest_path = bundle_root.join("manifest.json");
    fs::write(manifest_path, serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

fn resolve_unpack_root(unpack_root: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(unpack_root)
        .with_context(|| format!("failed to resolve {}", unpack_root.display()))?;
    let Some(name) = canonical.file_name().and_then(|value| value.to_str()) else {
        bail!("add-local requires an unpack root path");
    };
    if !name.starts_with("tokscale-bundle-") {
        bail!("add-local only accepts unpack roots created by `tokscale-bundle unpack`");
    }
    if !canonical.join("manifest.json").is_file() || !canonical.join("home").is_dir() {
        bail!("add-local requires a bundle root containing manifest.json and home/");
    }
    Ok(canonical)
}

fn replay_namespace() -> PathBuf {
    let host = hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| "local".to_string());
    let sanitized = host
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    PathBuf::from(".tokscale-bundle")
        .join("local")
        .join(if sanitized.is_empty() {
            "local".to_string()
        } else {
            sanitized
        })
}

pub fn add_current_machine_to_unpack_root(unpack_root: &Path) -> Result<AddLocalResult> {
    let unpack_root = resolve_unpack_root(unpack_root)?;
    let fake_home = unpack_root.join("home");
    let local_home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    if fs::canonicalize(&local_home).ok() == fs::canonicalize(&fake_home).ok() {
        bail!("add-local must be run from the real local home, not the fake home");
    }

    let scanner_settings = load_scanner_settings(&local_home);
    let scan_result = scan_all_clients_with_scanner_settings(
        &local_home.to_string_lossy(),
        &parse_local_clients(),
        true,
        &scanner_settings,
    );

    let mut manifest = read_manifest(&unpack_root)?;
    let before_entries = manifest.entries.len();
    let mut skipped_sources = Vec::new();
    let namespace = replay_namespace();

    for client in ClientId::iter() {
        if !supports_extra_root_replay(client) {
            continue;
        }
        let relative_root = namespace.join("extra").join(client.as_str());
        stage_supported_client_files_under(
            &unpack_root,
            &fake_home,
            client,
            scan_result.get(client),
            relative_root,
            &mut manifest.replay,
            &mut manifest.entries,
        )?;
    }

    stage_opencode_dbs_under(
        &unpack_root,
        &fake_home,
        &scan_result,
        &namespace.join("opencode"),
        &mut manifest.replay,
        &mut manifest.entries,
    )?;

    if let Some(synthetic_db) = &scan_result.synthetic_db {
        stage_special_file_if_missing(
            &unpack_root,
            &fake_home,
            synthetic_db,
            Path::new(".local/share/octofriend/sqlite.db"),
            "synthetic",
            &mut manifest.entries,
            &mut skipped_sources,
        )?;
    }

    if let Some(kilo_db) = &scan_result.kilo_db {
        stage_special_file_if_missing(
            &unpack_root,
            &fake_home,
            kilo_db,
            Path::new(".local/share/kilo/kilo.db"),
            "kilo",
            &mut manifest.entries,
            &mut skipped_sources,
        )?;
    }

    if let Some(hermes_db) = &scan_result.hermes_db {
        stage_special_file_if_missing(
            &unpack_root,
            &fake_home,
            hermes_db,
            Path::new(".hermes/state.db"),
            "hermes",
            &mut manifest.entries,
            &mut skipped_sources,
        )?;
    }

    stage_crush_registry_with_mode(
        &unpack_root,
        &fake_home,
        &scan_result.crush_dbs,
        true,
        &mut manifest.entries,
    )?;

    write_manifest(&unpack_root, &manifest)?;
    let scanner_settings = build_import_scanner_settings(&fake_home, &manifest.replay);
    let settings_path = write_scanner_settings(&fake_home, &scanner_settings)?;

    Ok(AddLocalResult {
        unpack_root,
        fake_home,
        settings_path,
        added_entries: manifest.entries.len().saturating_sub(before_entries),
        skipped_sources,
    })
}

pub fn print_add_local_summary(result: &AddLocalResult) {
    println!("unpack_root={}", result.unpack_root.display());
    println!("fake_home={}", result.fake_home.display());
    println!("settings_path={}", result.settings_path.display());
    println!("added local entries={}", result.added_entries);
    for skipped in &result.skipped_sources {
        println!("skipped {skipped}");
    }
    println!("combined submit: imported + local replay");
    println!(
        "HOME=\"{}\" tokscale submit --dry-run --no-spinner",
        result.fake_home.display()
    );
    println!(
        "HOME=\"{}\" tokscale submit --no-spinner",
        result.fake_home.display()
    );
}

pub fn export_current_machine(_output_path: &Path) -> Result<()> {
    let output_path = _output_path;
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    let scanner_settings = load_scanner_settings(&home_dir);
    let scan_result = scan_all_clients_with_scanner_settings(
        &home_dir.to_string_lossy(),
        &parse_local_clients(),
        true,
        &scanner_settings,
    );

    let temp_dir = TempDir::new()?;
    let bundle_root = temp_dir.path();
    let bundle_home = bundle_root.join("home");
    fs::create_dir_all(&bundle_home)?;

    let mut entries: Vec<BundleEntry> = Vec::new();
    let mut replay = ReplayConfig {
        extra_scan_roots: BTreeMap::new(),
        opencode_db_paths: Vec::new(),
    };

    for client in ClientId::iter() {
        if !supports_extra_root_replay(client) {
            continue;
        }
        stage_supported_client_files(
            bundle_root,
            &bundle_home,
            client,
            scan_result.get(client),
            &mut replay,
            &mut entries,
        )?;
    }

    stage_opencode_dbs(
        bundle_root,
        &bundle_home,
        &scan_result,
        &mut replay,
        &mut entries,
    )?;

    if let Some(synthetic_db) = &scan_result.synthetic_db {
        stage_special_file(
            bundle_root,
            &bundle_home,
            synthetic_db,
            Path::new(".local/share/octofriend/sqlite.db"),
            "synthetic",
            &mut entries,
        )?;
    }

    if let Some(kilo_db) = &scan_result.kilo_db {
        stage_special_file(
            bundle_root,
            &bundle_home,
            kilo_db,
            Path::new(".local/share/kilo/kilo.db"),
            "kilo",
            &mut entries,
        )?;
    }

    if let Some(hermes_db) = &scan_result.hermes_db {
        stage_special_file(
            bundle_root,
            &bundle_home,
            hermes_db,
            Path::new(".hermes/state.db"),
            "hermes",
            &mut entries,
        )?;
    }

    stage_crush_registry(
        bundle_root,
        &bundle_home,
        &scan_result.crush_dbs,
        &mut entries,
    )?;

    let manifest = BundleManifest {
        format_version: 1,
        created_at: Utc::now(),
        host: hostname::get()
            .ok()
            .and_then(|value| value.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string()),
        archive_kind: "zip".to_string(),
        entries,
        replay,
    };
    write_manifest(bundle_root, &manifest)?;
    write_bundle_zip(bundle_root, output_path)?;

    println!(
        "Wrote bundle with {} entries to {}",
        manifest.entries.len(),
        output_path.display()
    );
    Ok(())
}
