use crate::archive::unpack_bundle_zip;
use crate::layout::build_import_scanner_settings;
use crate::manifest::{BundleEntry, BundleManifest, ReplayConfig};
use crate::replay::fake_home_submit_command;
use crate::settings::write_scanner_settings;
use crate::tokscale::create_current_machine_bundle;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use walkdir::WalkDir;

const TOKEN_ENV: &str = "TOKSCALE_BUNDLE_TOKEN";
const DEFAULT_BIND: &str = "127.0.0.1:4787";
const DEFAULT_UPSTREAM_URL: &str = "https://tokscale.ai";

#[derive(Debug, Clone)]
pub struct UploadOptions {
    pub server: String,
    pub token: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub bind: String,
    pub storage: PathBuf,
    pub token: Option<String>,
    pub upstream_url: String,
    pub submit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentity {
    pub device_id: String,
    pub device_name: String,
}

#[derive(Debug)]
struct StoredBundle {
    device_id: String,
    archive_path: PathBuf,
}

#[derive(Debug)]
struct CombinedHome {
    root: PathBuf,
    fake_home: PathBuf,
    settings_path: PathBuf,
    bundle_count: usize,
    skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSubmitPayload {
    device_id: String,
    device_name: String,
    received_at: String,
    payload: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
struct TokenBreakdown {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    reasoning: i64,
}

#[derive(Debug, Clone)]
struct ClientAccum {
    client: String,
    model_id: String,
    provider_id: Option<String>,
    tokens: TokenBreakdown,
    cost: f64,
    messages: i64,
}

#[derive(Debug, Clone)]
struct DayAccum {
    date: String,
    timestamp_ms: Option<i64>,
    clients: BTreeMap<String, ClientAccum>,
}

pub fn default_bind() -> &'static str {
    DEFAULT_BIND
}

pub fn default_upstream_url() -> &'static str {
    DEFAULT_UPSTREAM_URL
}

fn token_from_env_or_option(token: Option<String>) -> Result<String> {
    token
        .or_else(|| std::env::var(TOKEN_ENV).ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("{TOKEN_ENV} or --token is required"))
}

fn config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home"))?;
    Ok(home.join(".config/tokscale-bundle"))
}

fn device_identity_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("device.json"))
}

fn sanitize_id(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        "device".to_string()
    } else {
        sanitized
    }
}

fn hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn load_or_create_device_identity(
    device_id: Option<String>,
    device_name: Option<String>,
) -> Result<DeviceIdentity> {
    if let Some(device_id) = device_id.filter(|value| !value.trim().is_empty()) {
        return Ok(DeviceIdentity {
            device_id: sanitize_id(device_id.trim()),
            device_name: device_name.unwrap_or_else(hostname),
        });
    }

    let path = device_identity_path()?;
    match fs::read_to_string(&path) {
        Ok(raw) => {
            let mut identity: DeviceIdentity = serde_json::from_str(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))?;
            if let Some(name) = device_name.filter(|value| !value.trim().is_empty()) {
                identity.device_name = name;
                fs::write(&path, serde_json::to_vec_pretty(&identity)?)?;
            }
            Ok(identity)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(config_dir()?)?;
            let identity = DeviceIdentity {
                device_id: format!("{}-{}", sanitize_id(&hostname()), Utc::now().timestamp()),
                device_name: device_name.unwrap_or_else(hostname),
            };
            fs::write(&path, serde_json::to_vec_pretty(&identity)?)?;
            Ok(identity)
        }
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

pub fn upload_current_machine(options: UploadOptions) -> Result<()> {
    let token = token_from_env_or_option(options.token)?;
    let identity = load_or_create_device_identity(options.device_id, options.device_name)?;
    let temp_dir = tempfile::tempdir()?;
    let archive_path = temp_dir.path().join("bundle.zip");
    let manifest = create_current_machine_bundle(&archive_path)?;
    let endpoint = format!("{}/api/bundles", options.server.trim_end_matches('/'));
    let archive = fs::read(&archive_path)?;

    let response = ureq::post(&endpoint)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/zip")
        .set("X-Tokscale-Bundle-Device-Id", &identity.device_id)
        .set("X-Tokscale-Bundle-Device-Name", &identity.device_name)
        .send_bytes(&archive)?;

    println!(
        "uploaded device={} entries={} status={}",
        identity.device_id,
        manifest.entries.len(),
        response.status()
    );
    Ok(())
}

fn latest_bundles(storage: &Path) -> Result<Vec<StoredBundle>> {
    let devices_dir = storage.join("devices");
    if !devices_dir.exists() {
        return Ok(Vec::new());
    }

    let mut bundles = Vec::new();
    for entry in fs::read_dir(&devices_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let device_id = entry.file_name().to_string_lossy().to_string();
        let mut zips = fs::read_dir(entry.path())?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("zip"))
            .collect::<Vec<_>>();
        zips.sort_by_key(|entry| entry.file_name());
        if let Some(latest) = zips.pop() {
            bundles.push(StoredBundle {
                device_id,
                archive_path: latest.path(),
            });
        }
    }
    bundles.sort_by(|left, right| left.device_id.cmp(&right.device_id));
    Ok(bundles)
}

fn read_manifest(root: &Path) -> Result<BundleManifest> {
    let raw = fs::read_to_string(root.join("manifest.json"))?;
    Ok(serde_json::from_str(&raw)?)
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn copy_tree(source_root: &Path, destination_root: &Path) -> Result<()> {
    for entry in WalkDir::new(source_root)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        let source = entry.path();
        let relative = source.strip_prefix(source_root)?;
        let destination = destination_root.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&destination)?;
        } else {
            copy_file(source, &destination)?;
        }
    }
    Ok(())
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn merge_extra_roots(
    imported_home: &Path,
    combined_home: &Path,
    device_id: &str,
    manifest: &BundleManifest,
    replay: &mut ReplayConfig,
    skipped: &mut Vec<String>,
) -> Result<()> {
    for (client, roots) in &manifest.replay.extra_scan_roots {
        for root in roots {
            let source = imported_home.join(root);
            if !source.exists() {
                skipped.push(format!("{device_id}: missing {}", root.display()));
                continue;
            }
            let target = PathBuf::from(".tokscale-bundle")
                .join("remote")
                .join(device_id)
                .join("extra")
                .join(client);
            copy_tree(&source, &combined_home.join(&target))?;
            push_unique_path(
                replay.extra_scan_roots.entry(client.clone()).or_default(),
                target,
            );
        }
    }

    for db in &manifest.replay.opencode_db_paths {
        let source = imported_home.join(db);
        if !source.exists() {
            skipped.push(format!("{device_id}: missing {}", db.display()));
            continue;
        }
        let file_name = db
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("opencode.db");
        let target = PathBuf::from(".tokscale-bundle")
            .join("remote")
            .join(device_id)
            .join("opencode")
            .join(file_name);
        copy_file(&source, &combined_home.join(&target))?;
        push_unique_path(&mut replay.opencode_db_paths, target);
    }
    Ok(())
}

fn merge_fixed_paths(
    imported_home: &Path,
    combined_home: &Path,
    device_id: &str,
    skipped: &mut Vec<String>,
) -> Result<()> {
    let fixed_paths = [
        ".local/share/octofriend/sqlite.db",
        ".local/share/kilo/kilo.db",
        ".hermes/state.db",
        ".local/share/crush/projects.json",
    ];

    for relative in fixed_paths {
        let source = imported_home.join(relative);
        if !source.exists() {
            continue;
        }
        let destination = combined_home.join(relative);
        if destination.exists() {
            skipped.push(format!(
                "{device_id}: fixed-path source already present: {relative}"
            ));
            continue;
        }
        copy_file(&source, &destination)?;
    }
    Ok(())
}

fn rebuild_combined_home(storage: &Path) -> Result<CombinedHome> {
    let bundles = latest_bundles(storage)?;
    if bundles.is_empty() {
        bail!("no uploaded bundles found in {}", storage.display());
    }

    let root = storage.join("combined");
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    let fake_home = root.join("home");
    let mut combined_replay = ReplayConfig {
        extra_scan_roots: BTreeMap::new(),
        opencode_db_paths: Vec::new(),
    };
    let mut entries = Vec::<BundleEntry>::new();
    let mut skipped = Vec::new();

    for (index, bundle) in bundles.iter().enumerate() {
        let import_dir = TempDir::new()?;
        unpack_bundle_zip(&bundle.archive_path, import_dir.path())?;
        let manifest = read_manifest(import_dir.path())?;
        let imported_home = import_dir.path().join("home");
        if !imported_home.is_dir() {
            bail!("{} missing home/", bundle.archive_path.display());
        }

        if index == 0 {
            copy_tree(&imported_home, &fake_home)?;
            combined_replay = manifest.replay.clone();
        } else {
            merge_extra_roots(
                &imported_home,
                &fake_home,
                &bundle.device_id,
                &manifest,
                &mut combined_replay,
                &mut skipped,
            )?;
            merge_fixed_paths(&imported_home, &fake_home, &bundle.device_id, &mut skipped)?;
        }

        entries.extend(manifest.entries.into_iter());
    }

    let manifest = BundleManifest {
        format_version: 1,
        created_at: Utc::now(),
        host: hostname(),
        archive_kind: "server-combined".to_string(),
        entries,
        replay: combined_replay.clone(),
    };
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    let scanner_settings = build_import_scanner_settings(&fake_home, &combined_replay);
    let settings_path = write_scanner_settings(&fake_home, &scanner_settings)?;

    Ok(CombinedHome {
        root,
        fake_home,
        settings_path,
        bundle_count: bundles.len(),
        skipped,
    })
}

fn copy_server_credentials(fake_home: &Path) -> Result<bool> {
    let Some(home) = dirs::home_dir() else {
        return Ok(false);
    };
    let source = home.join(".config/tokscale/credentials.json");
    if !source.is_file() {
        return Ok(false);
    }
    let destination = fake_home.join(".config/tokscale/credentials.json");
    copy_file(&source, &destination)?;
    Ok(true)
}

fn run_submit(fake_home: &Path) -> Result<()> {
    copy_server_credentials(fake_home)?;
    let command = fake_home_submit_command(fake_home, &[]);
    let status = Command::new("sh")
        .arg("-lc")
        .arg(&command)
        .status()
        .with_context(|| format!("failed to run {command}"))?;
    if !status.success() {
        bail!("submit command failed with status {status}");
    }
    Ok(())
}

fn bearer_token(request: &Request) -> Option<String> {
    request.headers().iter().find_map(|header| {
        if header.field.equiv("Authorization") {
            header
                .value
                .as_str()
                .strip_prefix("Bearer ")
                .map(|value| value.to_string())
        } else {
            None
        }
    })
}

fn header_value(request: &Request, name: &str) -> Option<String> {
    request.headers().iter().find_map(|header| {
        if header.field.to_string().eq_ignore_ascii_case(name) {
            Some(header.value.as_str().to_string())
        } else {
            None
        }
    })
}

fn json_response(
    status: StatusCode,
    body: serde_json::Value,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_string(body.to_string()).with_status_code(status);
    response
        .add_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap());
    response
}

fn path_without_query(url: &str) -> &str {
    url.split_once('?').map(|(path, _)| path).unwrap_or(url)
}

fn device_id_from_submit_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("/devices/")?;
    let (device_id, suffix) = rest.split_once("/api/submit")?;
    if suffix.is_empty() && !device_id.trim().is_empty() {
        Some(sanitize_id(device_id))
    } else {
        None
    }
}

fn token_hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let hash = hasher.finalize();
    hex::encode(&hash[..16])
}

fn account_payload_dir(storage: &Path, account_hash: &str) -> PathBuf {
    storage.join("payloads").join(account_hash).join("devices")
}

fn payload_path(storage: &Path, account_hash: &str, device_id: &str) -> PathBuf {
    account_payload_dir(storage, account_hash).join(format!("{device_id}.json"))
}

fn value_i64(value: Option<&serde_json::Value>) -> i64 {
    value.and_then(|value| value.as_i64()).unwrap_or(0)
}

fn value_f64(value: Option<&serde_json::Value>) -> f64 {
    value.and_then(|value| value.as_f64()).unwrap_or(0.0)
}

fn token_breakdown_from(value: Option<&serde_json::Value>) -> TokenBreakdown {
    let Some(value) = value else {
        return TokenBreakdown::default();
    };
    TokenBreakdown {
        input: value_i64(value.get("input")),
        output: value_i64(value.get("output")),
        cache_read: value_i64(value.get("cacheRead")),
        cache_write: value_i64(value.get("cacheWrite")),
        reasoning: value_i64(value.get("reasoning")),
    }
}

fn add_tokens(left: &mut TokenBreakdown, right: &TokenBreakdown) {
    left.input += right.input;
    left.output += right.output;
    left.cache_read += right.cache_read;
    left.cache_write += right.cache_write;
    left.reasoning += right.reasoning;
}

fn token_total(tokens: &TokenBreakdown) -> i64 {
    tokens.input + tokens.output + tokens.cache_read + tokens.cache_write + tokens.reasoning
}

fn client_key(client: &str, model_id: &str, provider_id: Option<&str>) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}",
        client,
        model_id,
        provider_id.unwrap_or("")
    )
}

fn ingest_payload(
    days: &mut BTreeMap<String, DayAccum>,
    payload: &serde_json::Value,
) -> Result<()> {
    let contributions = payload
        .get("contributions")
        .and_then(|value| value.as_array())
        .ok_or_else(|| anyhow::anyhow!("payload missing contributions array"))?;

    for contribution in contributions {
        let date = contribution
            .get("date")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("contribution missing date"))?
            .to_string();
        let day = days.entry(date.clone()).or_insert_with(|| DayAccum {
            date,
            timestamp_ms: None,
            clients: BTreeMap::new(),
        });

        if let Some(timestamp_ms) = contribution
            .get("timestampMs")
            .and_then(|value| value.as_i64())
        {
            day.timestamp_ms = Some(
                day.timestamp_ms
                    .map_or(timestamp_ms, |old| old.min(timestamp_ms)),
            );
        }

        let clients = contribution
            .get("clients")
            .or_else(|| contribution.get("sources"))
            .and_then(|value| value.as_array())
            .ok_or_else(|| anyhow::anyhow!("contribution missing clients array"))?;

        for client in clients {
            let client_name = client
                .get("client")
                .or_else(|| client.get("source"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("unknown");
            let model_id = client
                .get("modelId")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("unknown");
            let provider_id = client
                .get("providerId")
                .and_then(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string);
            let tokens = token_breakdown_from(client.get("tokens"));
            let key = client_key(client_name, model_id, provider_id.as_deref());
            let entry = day.clients.entry(key).or_insert_with(|| ClientAccum {
                client: client_name.to_string(),
                model_id: model_id.to_string(),
                provider_id: provider_id.clone(),
                tokens: TokenBreakdown::default(),
                cost: 0.0,
                messages: 0,
            });
            add_tokens(&mut entry.tokens, &tokens);
            entry.cost += value_f64(client.get("cost"));
            entry.messages += value_i64(client.get("messages"));
        }
    }

    Ok(())
}

fn intensity(cost: f64, max_cost: f64) -> u8 {
    if cost <= 0.0 || max_cost <= 0.0 {
        return 0;
    }
    let ratio = cost / max_cost;
    if ratio >= 0.75 {
        4
    } else if ratio >= 0.5 {
        3
    } else if ratio >= 0.25 {
        2
    } else {
        1
    }
}

pub fn merge_submit_payloads(payloads: &[serde_json::Value]) -> Result<serde_json::Value> {
    let mut days = BTreeMap::<String, DayAccum>::new();
    let mut version = "unknown".to_string();

    for payload in payloads {
        if let Some(payload_version) = payload
            .pointer("/meta/version")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        {
            version = payload_version.to_string();
        }
        ingest_payload(&mut days, payload)?;
    }

    let mut day_values = Vec::new();
    let mut total_tokens = 0_i64;
    let mut total_cost = 0.0_f64;
    let mut active_days = 0_i64;
    let mut max_cost = 0.0_f64;
    let mut clients_set = BTreeSet::new();
    let mut models_set = BTreeSet::new();

    let day_costs = days
        .values()
        .map(|day| day.clients.values().map(|client| client.cost).sum::<f64>())
        .collect::<Vec<_>>();
    if let Some(maximum) = day_costs.iter().copied().reduce(f64::max) {
        max_cost = maximum;
    }

    for day in days.values() {
        let mut day_tokens = TokenBreakdown::default();
        let mut day_cost = 0.0_f64;
        let mut day_messages = 0_i64;
        let mut client_values = Vec::new();

        for client in day.clients.values() {
            clients_set.insert(client.client.clone());
            models_set.insert(client.model_id.clone());
            add_tokens(&mut day_tokens, &client.tokens);
            day_cost += client.cost;
            day_messages += client.messages;

            let mut client_value = serde_json::json!({
                "client": client.client,
                "modelId": client.model_id,
                "tokens": {
                    "input": client.tokens.input,
                    "output": client.tokens.output,
                    "cacheRead": client.tokens.cache_read,
                    "cacheWrite": client.tokens.cache_write,
                    "reasoning": client.tokens.reasoning,
                },
                "cost": client.cost,
                "messages": client.messages,
            });
            if let Some(provider_id) = &client.provider_id {
                client_value["providerId"] = serde_json::Value::String(provider_id.clone());
            }
            client_values.push(client_value);
        }

        let day_total_tokens = token_total(&day_tokens);
        if day_total_tokens > 0 {
            active_days += 1;
        }
        total_tokens += day_total_tokens;
        total_cost += day_cost;

        let mut day_value = serde_json::json!({
            "date": day.date,
            "totals": {
                "tokens": day_total_tokens,
                "cost": day_cost,
                "messages": day_messages,
            },
            "intensity": intensity(day_cost, max_cost),
            "tokenBreakdown": {
                "input": day_tokens.input,
                "output": day_tokens.output,
                "cacheRead": day_tokens.cache_read,
                "cacheWrite": day_tokens.cache_write,
                "reasoning": day_tokens.reasoning,
            },
            "clients": client_values,
        });
        if let Some(timestamp_ms) = day.timestamp_ms {
            day_value["timestampMs"] = serde_json::Value::Number(timestamp_ms.into());
        }
        day_values.push(day_value);
    }

    let date_start = days.keys().next().cloned().unwrap_or_default();
    let date_end = days.keys().next_back().cloned().unwrap_or_default();
    let mut years = BTreeMap::<String, (i64, f64, String, String)>::new();
    for day in &day_values {
        let Some(date) = day.get("date").and_then(|value| value.as_str()) else {
            continue;
        };
        let year = date.chars().take(4).collect::<String>();
        let tokens = value_i64(day.pointer("/totals/tokens"));
        let cost = value_f64(day.pointer("/totals/cost"));
        years
            .entry(year)
            .and_modify(|entry| {
                entry.0 += tokens;
                entry.1 += cost;
                if date < entry.2.as_str() {
                    entry.2 = date.to_string();
                }
                if date > entry.3.as_str() {
                    entry.3 = date.to_string();
                }
            })
            .or_insert_with(|| (tokens, cost, date.to_string(), date.to_string()));
    }

    let year_values = years
        .into_iter()
        .map(|(year, (tokens, cost, start, end))| {
            serde_json::json!({
                "year": year,
                "totalTokens": tokens,
                "totalCost": cost,
                "range": {
                    "start": start,
                    "end": end,
                },
            })
        })
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "meta": {
            "generatedAt": Utc::now().to_rfc3339(),
            "version": version,
            "dateRange": {
                "start": date_start,
                "end": date_end,
            },
        },
        "summary": {
            "totalTokens": total_tokens,
            "totalCost": total_cost,
            "totalDays": day_values.len() as i64,
            "activeDays": active_days,
            "averagePerDay": if active_days > 0 { total_cost / active_days as f64 } else { 0.0 },
            "maxCostInSingleDay": max_cost,
            "clients": clients_set.into_iter().collect::<Vec<_>>(),
            "models": models_set.into_iter().collect::<Vec<_>>(),
        },
        "years": year_values,
        "contributions": day_values,
    }))
}

fn read_stored_payloads(storage: &Path, account_hash: &str) -> Result<Vec<StoredSubmitPayload>> {
    let dir = account_payload_dir(storage, account_hash);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            serde_json::from_str::<StoredSubmitPayload>(&raw)
                .with_context(|| format!("failed to parse {}", path.display()))
        })
        .collect()
}

fn merge_account_payloads(
    storage: &Path,
    account_hash: &str,
) -> Result<(serde_json::Value, usize)> {
    let stored = read_stored_payloads(storage, account_hash)?;
    let payloads = stored
        .iter()
        .map(|stored| stored.payload.clone())
        .collect::<Vec<_>>();
    Ok((merge_submit_payloads(&payloads)?, stored.len()))
}

fn submit_upstream(
    upstream_url: &str,
    token: &str,
    payload: &serde_json::Value,
) -> Result<serde_json::Value> {
    let endpoint = format!("{}/api/submit", upstream_url.trim_end_matches('/'));
    let request = ureq::post(&endpoint)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {token}"));

    let body = serde_json::to_string(payload)?;
    match request.send_string(&body) {
        Ok(response) => {
            let status = response.status();
            let body = response
                .into_string()
                .unwrap_or_else(|_| String::from("{}"));
            let parsed = serde_json::from_str::<serde_json::Value>(&body)
                .unwrap_or_else(|_| serde_json::json!({ "raw": body }));
            if (200..300).contains(&status) {
                Ok(serde_json::json!({ "status": status, "body": parsed }))
            } else {
                bail!("upstream returned {status}: {parsed}");
            }
        }
        Err(ureq::Error::Status(status, response)) => {
            let body = response
                .into_string()
                .unwrap_or_else(|_| String::from("{}"));
            bail!("upstream returned {status}: {body}");
        }
        Err(err) => Err(err).context("upstream submit failed"),
    }
}

fn account_hash_from_request(request: &Request) -> Result<(String, String)> {
    let token = bearer_token(request).ok_or_else(|| anyhow::anyhow!("missing bearer token"))?;
    Ok((token_hash(&token), token))
}

fn handle_submit_payload(
    mut request: Request,
    options: &ServeOptions,
    path_device_id: Option<String>,
) -> Result<()> {
    let (account_hash, token) = match account_hash_from_request(&request) {
        Ok(value) => value,
        Err(_) => {
            request.respond(json_response(
                StatusCode(401),
                serde_json::json!({ "error": "Missing or invalid Authorization header" }),
            ))?;
            return Ok(());
        }
    };

    let device_id = path_device_id
        .or_else(|| header_value(&request, "X-Tokscale-Device").map(|value| sanitize_id(&value)))
        .unwrap_or_else(|| "default".to_string());
    let device_name =
        header_value(&request, "X-Tokscale-Device-Name").unwrap_or_else(|| device_id.clone());

    let payload = match serde_json::from_reader::<_, serde_json::Value>(request.as_reader()) {
        Ok(payload) => payload,
        Err(err) => {
            request.respond(json_response(
                StatusCode(400),
                serde_json::json!({ "error": format!("invalid JSON body: {err}") }),
            ))?;
            return Ok(());
        }
    };

    if let Err(err) = merge_submit_payloads(std::slice::from_ref(&payload)) {
        request.respond(json_response(
            StatusCode(400),
            serde_json::json!({ "error": err.to_string() }),
        ))?;
        return Ok(());
    }

    let stored = StoredSubmitPayload {
        device_id: device_id.clone(),
        device_name: device_name.clone(),
        received_at: Utc::now().to_rfc3339(),
        payload,
    };
    let path = payload_path(&options.storage, &account_hash, &device_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&stored)?)?;

    let (merged, device_count) = match merge_account_payloads(&options.storage, &account_hash) {
        Ok(value) => value,
        Err(err) => {
            request.respond(json_response(
                StatusCode(500),
                serde_json::json!({ "error": err.to_string() }),
            ))?;
            return Ok(());
        }
    };

    let upstream = if options.submit {
        match submit_upstream(&options.upstream_url, &token, &merged) {
            Ok(value) => Some(value),
            Err(err) => {
                request.respond(json_response(
                    StatusCode(502),
                    serde_json::json!({ "error": err.to_string() }),
                ))?;
                return Ok(());
            }
        }
    } else {
        None
    };

    request.respond(json_response(
        StatusCode(200),
        serde_json::json!({
            "success": true,
            "mode": if options.submit { "proxy-submit" } else { "proxy-store" },
            "deviceId": device_id,
            "deviceName": device_name,
            "deviceCount": device_count,
            "metrics": merged.get("summary").cloned().unwrap_or_else(|| serde_json::json!({})),
            "upstream": upstream,
        }),
    ))?;
    Ok(())
}

fn handle_payload_status(request: Request, options: &ServeOptions) -> Result<()> {
    let (account_hash, _) = match account_hash_from_request(&request) {
        Ok(value) => value,
        Err(_) => {
            request.respond(json_response(
                StatusCode(401),
                serde_json::json!({ "error": "Missing or invalid Authorization header" }),
            ))?;
            return Ok(());
        }
    };
    let stored = read_stored_payloads(&options.storage, &account_hash)?;
    let devices = stored
        .iter()
        .map(|payload| {
            serde_json::json!({
                "deviceId": payload.device_id,
                "deviceName": payload.device_name,
                "receivedAt": payload.received_at,
            })
        })
        .collect::<Vec<_>>();
    request.respond(json_response(
        StatusCode(200),
        serde_json::json!({ "ok": true, "deviceCount": devices.len(), "devices": devices }),
    ))?;
    Ok(())
}

fn handle_payload_preview(request: Request, options: &ServeOptions) -> Result<()> {
    let (account_hash, _) = match account_hash_from_request(&request) {
        Ok(value) => value,
        Err(_) => {
            request.respond(json_response(
                StatusCode(401),
                serde_json::json!({ "error": "Missing or invalid Authorization header" }),
            ))?;
            return Ok(());
        }
    };
    let (merged, device_count) = merge_account_payloads(&options.storage, &account_hash)?;
    request.respond(json_response(
        StatusCode(200),
        serde_json::json!({ "ok": true, "deviceCount": device_count, "data": merged }),
    ))?;
    Ok(())
}

fn handle_payload_push(request: Request, options: &ServeOptions) -> Result<()> {
    let (account_hash, token) = match account_hash_from_request(&request) {
        Ok(value) => value,
        Err(_) => {
            request.respond(json_response(
                StatusCode(401),
                serde_json::json!({ "error": "Missing or invalid Authorization header" }),
            ))?;
            return Ok(());
        }
    };
    let (merged, device_count) = merge_account_payloads(&options.storage, &account_hash)?;
    let upstream = match submit_upstream(&options.upstream_url, &token, &merged) {
        Ok(value) => value,
        Err(err) => {
            request.respond(json_response(
                StatusCode(502),
                serde_json::json!({ "error": err.to_string() }),
            ))?;
            return Ok(());
        }
    };
    request.respond(json_response(
        StatusCode(200),
        serde_json::json!({ "success": true, "deviceCount": device_count, "upstream": upstream }),
    ))?;
    Ok(())
}

fn handle_payload_delete_device(
    request: Request,
    options: &ServeOptions,
    device_id: &str,
) -> Result<()> {
    let (account_hash, _) = match account_hash_from_request(&request) {
        Ok(value) => value,
        Err(_) => {
            request.respond(json_response(
                StatusCode(401),
                serde_json::json!({ "error": "Missing or invalid Authorization header" }),
            ))?;
            return Ok(());
        }
    };
    let path = payload_path(&options.storage, &account_hash, &sanitize_id(device_id));
    let deleted = match fs::remove_file(&path) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => false,
        Err(err) => {
            return Err(err).with_context(|| format!("failed to delete {}", path.display()));
        }
    };
    request.respond(json_response(
        StatusCode(200),
        serde_json::json!({ "ok": true, "deleted": deleted }),
    ))?;
    Ok(())
}

fn handle_upload(mut request: Request, options: &ServeOptions, token: Option<&str>) -> Result<()> {
    if token.is_none() || bearer_token(&request).as_deref() != token {
        request.respond(json_response(
            StatusCode(401),
            serde_json::json!({ "error": "unauthorized" }),
        ))?;
        return Ok(());
    }

    let device_id = header_value(&request, "X-Tokscale-Bundle-Device-Id")
        .map(|value| sanitize_id(&value))
        .unwrap_or_else(|| "unknown".to_string());
    let device_name = header_value(&request, "X-Tokscale-Bundle-Device-Name")
        .unwrap_or_else(|| device_id.clone());

    fs::create_dir_all(options.storage.join("devices").join(&device_id))?;
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let destination = options
        .storage
        .join("devices")
        .join(&device_id)
        .join(format!("{timestamp}.zip"));
    let temp_destination = destination.with_extension("zip.tmp");
    let mut body = Vec::new();
    request.as_reader().read_to_end(&mut body)?;
    {
        let mut file = fs::File::create(&temp_destination)?;
        file.write_all(&body)?;
    }

    let validate_dir = TempDir::new()?;
    if let Err(err) = unpack_bundle_zip(&temp_destination, validate_dir.path())
        .with_context(|| "uploaded body is not a valid tokscale-bundle zip")
        .and_then(|_| read_manifest(validate_dir.path()).map(|_| ()))
    {
        let _ = fs::remove_file(&temp_destination);
        request.respond(json_response(
            StatusCode(400),
            serde_json::json!({ "error": err.to_string() }),
        ))?;
        return Ok(());
    }
    fs::rename(&temp_destination, &destination)?;

    let combined = match rebuild_combined_home(&options.storage).and_then(|combined| {
        if options.submit {
            run_submit(&combined.fake_home)?;
        }
        Ok(combined)
    }) {
        Ok(combined) => combined,
        Err(err) => {
            request.respond(json_response(
                StatusCode(500),
                serde_json::json!({ "error": err.to_string() }),
            ))?;
            return Ok(());
        }
    };

    request.respond(json_response(
        StatusCode(200),
        serde_json::json!({
            "ok": true,
            "deviceId": device_id,
            "deviceName": device_name,
            "stored": destination,
            "combinedRoot": combined.root,
            "fakeHome": combined.fake_home,
            "settingsPath": combined.settings_path,
            "bundleCount": combined.bundle_count,
            "submitted": options.submit,
            "skipped": combined.skipped,
        }),
    ))?;
    Ok(())
}

fn handle_request(request: Request, options: &ServeOptions, token: Option<&str>) -> Result<()> {
    let path = path_without_query(request.url()).to_string();
    let submit_device_id = device_id_from_submit_path(&path);
    match (request.method(), path.as_str()) {
        (&Method::Get, "/health") => {
            request.respond(json_response(
                StatusCode(200),
                serde_json::json!({ "ok": true }),
            ))?;
        }
        (&Method::Post, "/api/bundles") => handle_upload(request, options, token)?,
        (&Method::Post, "/api/submit") => handle_submit_payload(request, options, None)?,
        (&Method::Get, "/api/hub/status") => handle_payload_status(request, options)?,
        (&Method::Get, "/api/hub/preview") => handle_payload_preview(request, options)?,
        (&Method::Post, "/api/hub/push") => handle_payload_push(request, options)?,
        (&Method::Delete, path) if path.starts_with("/api/hub/device/") => {
            let device_id = path.trim_start_matches("/api/hub/device/");
            handle_payload_delete_device(request, options, device_id)?
        }
        (&Method::Post, _) if submit_device_id.is_some() => {
            handle_submit_payload(request, options, submit_device_id)?
        }
        _ => {
            request.respond(json_response(
                StatusCode(404),
                serde_json::json!({ "error": "not found" }),
            ))?;
        }
    }
    Ok(())
}

pub fn serve(options: ServeOptions) -> Result<()> {
    let token = options
        .token
        .clone()
        .or_else(|| std::env::var(TOKEN_ENV).ok())
        .filter(|value| !value.trim().is_empty());
    fs::create_dir_all(&options.storage)?;
    let server = Server::http(&options.bind)
        .map_err(|err| anyhow::anyhow!("failed to bind {}: {err}", options.bind))?;
    println!(
        "tokscale-bundle server listening on http://{} storage={} submit={} upstream={} bundles={}",
        options.bind,
        options.storage.display(),
        options.submit,
        options.upstream_url,
        if token.is_some() {
            "enabled"
        } else {
            "disabled"
        }
    );
    for request in server.incoming_requests() {
        if let Err(err) = handle_request(request, &options, token.as_deref()) {
            eprintln!("request failed: {err:#}");
        }
    }
    Ok(())
}
