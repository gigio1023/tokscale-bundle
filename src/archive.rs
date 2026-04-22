use anyhow::Result;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub fn write_bundle_zip(_bundle_root: &Path, _output_path: &Path) -> Result<()> {
    let bundle_root = _bundle_root;
    let output_path = _output_path;

    let file = File::create(output_path)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    for entry in WalkDir::new(bundle_root)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        let relative = path.strip_prefix(bundle_root)?;
        if relative.as_os_str().is_empty() {
            continue;
        }

        let name = relative.to_string_lossy().replace('\\', "/");
        if path.is_dir() {
            zip.add_directory(format!("{name}/"), options)?;
            continue;
        }

        zip.start_file(name, options)?;
        let mut source = File::open(path)?;
        io::copy(&mut source, &mut zip)?;
    }

    zip.finish()?;
    Ok(())
}

pub fn unpack_bundle_zip(_archive_path: &Path, _output_dir: &Path) -> Result<()> {
    let archive_path = _archive_path;
    let output_dir = _output_dir;

    fs::create_dir_all(output_dir)?;
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(relative) = entry.enclosed_name().map(|path| path.to_path_buf()) else {
            continue;
        };
        let destination = output_dir.join(relative);

        if entry.name().ends_with('/') {
            fs::create_dir_all(&destination)?;
            continue;
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut output = File::create(&destination)?;
        io::copy(&mut entry, &mut output)?;
        output.flush()?;
    }

    Ok(())
}
