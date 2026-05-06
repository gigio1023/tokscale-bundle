use crate::archive::unpack_bundle_zip;
use crate::layout::build_import_scanner_settings;
use crate::manifest::BundleManifest;
use crate::settings::write_scanner_settings;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

pub struct UnpackedBundle {
    pub unpack_root: PathBuf,
    pub fake_home: PathBuf,
    pub settings_path: PathBuf,
}

fn load_bundle_manifest(unpack_root: &Path) -> Result<BundleManifest> {
    let manifest_path = unpack_root.join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn unpack_bundle_archive(archive_path: &Path) -> Result<UnpackedBundle> {
    let temp_dir = tempfile::Builder::new()
        .prefix("tokscale-bundle-")
        .tempdir()?;
    // Keep the fake home alive after this process exits so the user can run Tokscale manually.
    let unpack_root = temp_dir.keep();

    unpack_bundle_zip(archive_path, &unpack_root)?;
    let manifest = load_bundle_manifest(&unpack_root)?;
    let fake_home = unpack_root.join("home");
    if !fake_home.is_dir() {
        bail!("bundle missing home/ directory");
    }

    let scanner_settings = build_import_scanner_settings(&fake_home, &manifest.replay);
    let settings_path = write_scanner_settings(&fake_home, &scanner_settings)?;

    Ok(UnpackedBundle {
        unpack_root,
        fake_home,
        settings_path,
    })
}

pub fn print_unpack_summary(bundle: &UnpackedBundle) {
    let credentials_path = bundle.fake_home.join(".config/tokscale/credentials.json");

    println!("unpack_root={}", bundle.unpack_root.display());
    println!("fake_home={}", bundle.fake_home.display());
    println!("settings_path={}", bundle.settings_path.display());
    println!("manual submit: imported-only v1");
    println!(
        "mkdir -p \"{}\"",
        credentials_path
            .parent()
            .unwrap_or(&bundle.fake_home)
            .display()
    );
    println!(
        "cp ~/.config/tokscale/credentials.json \"{}\"",
        credentials_path.display()
    );
    println!(
        "HOME=\"{}\" tokscale submit --no-spinner",
        bundle.fake_home.display()
    );
}

fn validate_unpack_root(unpack_root: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(unpack_root)
        .with_context(|| format!("failed to resolve {}", unpack_root.display()))?;
    let Some(name) = canonical.file_name().and_then(|value| value.to_str()) else {
        bail!("cleanup requires an unpack root path");
    };
    if !name.starts_with("tokscale-bundle-") {
        bail!("cleanup only accepts unpack roots created by `tokscale-bundle unpack`");
    }
    // Cleanup is recursive, so require the marker files that unpack always creates.
    if !canonical.join("manifest.json").is_file() || !canonical.join("home").is_dir() {
        bail!("cleanup requires a bundle root containing manifest.json and home/");
    }
    Ok(canonical)
}

pub fn cleanup_unpack_root(unpack_root: &Path) -> Result<PathBuf> {
    let canonical = validate_unpack_root(unpack_root)?;
    fs::remove_dir_all(&canonical)?;
    Ok(canonical)
}
