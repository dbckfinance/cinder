use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::Builder;
use zip::ZipArchive;

use crate::catalog::OutputFormat;
use crate::convert::available_path;

pub fn convert(paths: &[PathBuf], format: OutputFormat) -> Result<String, String> {
    run(paths, |path| convert_one(path, format))
}

pub fn extract(paths: &[PathBuf]) -> Result<String, String> {
    run(paths, extract_one)
}

fn run(paths: &[PathBuf], operation: impl Fn(&Path) -> Result<PathBuf, String>) -> Result<String, String> {
    let mut written = Vec::new();
    let mut failure = None;
    for path in paths {
        match operation(path) {
            Ok(destination) => written.push(destination),
            Err(error) => failure = Some(error),
        }
    }
    if written.is_empty() {
        return Err(failure.unwrap_or_else(|| "Aucun fichier enregistré".to_string()));
    }
    if written.len() == 1 {
        return Ok(written[0]
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "fichier".to_string()));
    }
    Ok(format!("{} fichiers enregistrés", written.len()))
}

fn convert_one(path: &Path, format: OutputFormat) -> Result<PathBuf, String> {
    let entries = read_entries(path)?;
    if entries.is_empty() {
        return Err("Archive vide".to_string());
    }
    let parent = path.parent().ok_or_else(|| "Dossier introuvable".to_string())?;
    let gzip = match format {
        OutputFormat::Tar => false,
        OutputFormat::Gzip => true,
        _ => return Err("Format inattendu".to_string()),
    };
    let extension = if gzip { "tar.gz" } else { "tar" };
    let destination = available_path(parent, &archive_stem(path), extension);
    write_entries(&destination, &entries, gzip)?;
    Ok(destination)
}

fn extract_one(path: &Path) -> Result<PathBuf, String> {
    let entries = read_entries(path)?;
    if entries.is_empty() {
        return Err("Archive vide".to_string());
    }
    let parent = path.parent().ok_or_else(|| "Dossier introuvable".to_string())?;
    let directory = available_path(parent, &archive_stem(path), "");
    std::fs::create_dir_all(&directory).map_err(|_| "Impossible de créer le dossier".to_string())?;
    for (name, bytes) in entries {
        let Some(destination) = safe_join(&directory, &name) else {
            continue;
        };
        if let Some(folder) = destination.parent() {
            std::fs::create_dir_all(folder).map_err(|_| "Impossible de créer le dossier".to_string())?;
        }
        std::fs::write(&destination, bytes).map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    }
    Ok(directory)
}

fn read_entries(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let name = file_name(path);
    let ext = extension(path);
    if ext == "zip" {
        return zip_entries(path);
    }
    if ext == "tar" {
        let file = File::open(path).map_err(|_| "Fichier introuvable".to_string())?;
        return tar_entries(file);
    }
    if ext == "gz" || ext == "tgz" || name.ends_with(".tar.gz") {
        let bytes = read_gzip(path)?;
        if name.ends_with(".tar.gz") || ext == "tgz" || is_tar(&bytes) {
            return tar_entries(Cursor::new(bytes));
        }
        let inner = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "fichier".to_string());
        return Ok(vec![(inner, bytes)]);
    }
    Err("Pas une archive".to_string())
}

fn zip_entries(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let file = File::open(path).map_err(|_| "Fichier introuvable".to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|_| "Archive illisible".to_string())?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| "Archive illisible".to_string())?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy().replace('\\', "/");
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|_| "Archive illisible".to_string())?;
        entries.push((name, bytes));
    }
    Ok(entries)
}

fn tar_entries<R: Read>(reader: R) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut archive = tar::Archive::new(reader);
    let mut entries = Vec::new();
    for entry in archive.entries().map_err(|_| "Archive illisible".to_string())? {
        let mut entry = entry.map_err(|_| "Archive illisible".to_string())?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let name = entry
            .path()
            .map_err(|_| "Archive illisible".to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if safe_join(Path::new("."), &name).is_none() {
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|_| "Archive illisible".to_string())?;
        entries.push((name, bytes));
    }
    Ok(entries)
}

fn read_gzip(path: &Path) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|_| "Fichier introuvable".to_string())?;
    let mut decoder = GzDecoder::new(file);
    let mut bytes = Vec::new();
    decoder
        .read_to_end(&mut bytes)
        .map_err(|_| "Archive illisible".to_string())?;
    Ok(bytes)
}

fn is_tar(bytes: &[u8]) -> bool {
    bytes.len() > 262 && &bytes[257..262] == b"ustar"
}

fn write_entries(destination: &Path, entries: &[(String, Vec<u8>)], gzip: bool) -> Result<(), String> {
    let file = File::create(destination).map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    if gzip {
        let encoder = GzEncoder::new(file, Compression::default());
        let encoder = write_tar(encoder, entries)?;
        encoder.finish().map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    } else {
        write_tar(file, entries)?;
    }
    Ok(())
}

fn write_tar<W: Write>(writer: W, entries: &[(String, Vec<u8>)]) -> Result<W, String> {
    let mut builder = Builder::new(writer);
    for (name, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, &bytes[..])
            .map_err(|_| "Impossible d'écrire l'archive".to_string())?;
    }
    builder.into_inner().map_err(|_| "Impossible d'écrire l'archive".to_string())
}

fn safe_join(root: &Path, name: &str) -> Option<PathBuf> {
    let relative = Path::new(name);
    if name.is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return None;
    }
    Some(root.join(relative))
}

fn archive_stem(path: &Path) -> String {
    let mut stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".to_string());
    if stem.to_ascii_lowercase().ends_with(".tar") {
        stem.truncate(stem.len() - 4);
    }
    if stem.is_empty() {
        "archive".to_string()
    } else {
        stem
    }
}

fn extension(path: &Path) -> String {
    path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn file_name(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).unwrap_or("").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    #[test]
    fn zip_extracts_beside_the_archive() {
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cinder-zip-{tick}"));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("paquet.zip");
        let file = File::create(&source).unwrap();
        let mut writer = ZipWriter::new(file);
        writer.start_file("hello.txt", SimpleFileOptions::default()).unwrap();
        writer.write_all(b"bonjour").unwrap();
        writer.finish().unwrap();

        let message = extract(&[source.clone()]).unwrap();
        assert_eq!(message, "paquet");
        assert_eq!(std::fs::read(dir.join("paquet").join("hello.txt")).unwrap(), b"bonjour");
        assert!(source.is_file());
        let _ = std::fs::remove_dir_all(dir);
    }
}
