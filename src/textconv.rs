use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use calamine::{open_workbook_auto, Reader};
use zip::ZipArchive;

use crate::catalog::OutputFormat;
use crate::convert::available_path;

pub fn apply(paths: &[PathBuf], format: OutputFormat) -> Result<String, String> {
    let mut written = Vec::new();
    let mut failure = None;
    for path in paths {
        match convert_one(path, format) {
            Ok(destination) => written.push(destination),
            Err(error) => failure = Some(error),
        }
    }
    report(&written, failure)
}

fn convert_one(path: &Path, format: OutputFormat) -> Result<PathBuf, String> {
    let text = match crate::catalog::category_of(path) {
        Some(crate::catalog::Category::Subtitle) => subtitle_text(path, format)?,
        Some(crate::catalog::Category::Spreadsheet) => table_text(path, format)?,
        Some(crate::catalog::Category::Text) => prose_text(path, format)?,
        Some(crate::catalog::Category::Epub) => epub_text(path, format)?,
        _ => return Err("Pas encore de conversion pour ce type".to_string()),
    };
    let parent = path.parent().ok_or_else(|| "Dossier introuvable".to_string())?;
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fichier".to_string());
    let destination = available_path(parent, &stem, format.extension());
    std::fs::write(&destination, text).map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    Ok(destination)
}

fn subtitle_text(path: &Path, format: OutputFormat) -> Result<String, String> {
    let source = read_text(path)?;
    let cues = cues_from(&source);
    if cues.is_empty() {
        return Err("Sous-titre illisible".to_string());
    }
    Ok(match format {
        OutputFormat::Vtt => write_vtt(&cues),
        OutputFormat::Srt => write_srt(&cues),
        OutputFormat::Txt => cues.iter().map(|cue| cue.text.as_str()).collect::<Vec<_>>().join("\n"),
        _ => return Err("Format inattendu".to_string()),
    })
}

fn table_text(path: &Path, format: OutputFormat) -> Result<String, String> {
    let delimiter = match format {
        OutputFormat::Csv => b',',
        OutputFormat::Tsv => b'\t',
        _ => return Err("Format inattendu".to_string()),
    };
    let rows = read_table(path)?;
    if rows.is_empty() {
        return Err("Feuille vide".to_string());
    }
    write_table(&rows, delimiter)
}

fn prose_text(path: &Path, format: OutputFormat) -> Result<String, String> {
    let source = read_text(path)?;
    let plain = if is_html(path) { strip_tags(&source) } else { source };
    Ok(match format {
        OutputFormat::Txt | OutputFormat::Md => plain,
        OutputFormat::Html => html_page(&plain),
        _ => return Err("Format inattendu".to_string()),
    })
}

fn epub_text(path: &Path, format: OutputFormat) -> Result<String, String> {
    let html = read_epub_html(path)?;
    let plain = strip_tags(&html);
    if plain.trim().is_empty() && !matches!(format, OutputFormat::Html) {
        return Err("Aucun texte dans cet EPUB".to_string());
    }
    Ok(match format {
        OutputFormat::Html => html,
        OutputFormat::Txt | OutputFormat::Md => plain,
        _ => return Err("Format inattendu".to_string()),
    })
}

pub(crate) fn table_rows(path: &Path) -> Result<Vec<Vec<String>>, String> {
    read_table(path)
}

fn read_table(path: &Path) -> Result<Vec<Vec<String>>, String> {
    let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "csv" || ext == "tsv" {
        let delimiter = if ext == "tsv" { b'\t' } else { b',' };
        return read_delimited(path, delimiter);
    }
    let mut book = open_workbook_auto(path).map_err(|_| "Tableur illisible".to_string())?;
    let range = book
        .worksheet_range_at(0)
        .ok_or_else(|| "Feuille vide".to_string())?
        .map_err(|_| "Tableur illisible".to_string())?;
    Ok(range
        .rows()
        .map(|row| row.iter().map(|cell| cell.to_string()).collect())
        .collect())
}

fn read_delimited(path: &Path, delimiter: u8) -> Result<Vec<Vec<String>>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_path(path)
        .map_err(|_| "Tableur illisible".to_string())?;
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|_| "Tableur illisible".to_string())?;
        rows.push(record.iter().map(|cell| cell.to_string()).collect());
    }
    Ok(rows)
}

fn write_table(rows: &[Vec<String>], delimiter: u8) -> Result<String, String> {
    let mut writer = csv::WriterBuilder::new().delimiter(delimiter).from_writer(Vec::new());
    for row in rows {
        writer.write_record(row).map_err(|_| "Impossible d'écrire le tableau".to_string())?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|_| "Impossible d'écrire le tableau".to_string())?;
    String::from_utf8(bytes).map_err(|_| "Texte illisible".to_string())
}

pub(crate) fn epub_markup(path: &Path) -> Result<String, String> {
    read_epub_html(path)
}

fn read_epub_html(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|_| "Fichier introuvable".to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|_| "EPUB illisible".to_string())?;
    let mut chapters = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| "EPUB illisible".to_string())?;
        let name = entry.name().to_ascii_lowercase();
        if !(name.ends_with(".html") || name.ends_with(".xhtml") || name.ends_with(".htm")) {
            continue;
        }
        let mut text = String::new();
        entry
            .read_to_string(&mut text)
            .map_err(|_| "EPUB illisible".to_string())?;
        chapters.push(text);
    }
    if chapters.is_empty() {
        return Err("Aucun chapitre dans cet EPUB".to_string());
    }
    Ok(chapters.join("\n"))
}

struct Cue {
    start: String,
    end: String,
    text: String,
}

fn cues_from(source: &str) -> Vec<Cue> {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
        let Some(arrow) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let mut parts = lines[arrow].split("-->");
        let start = parts.next().unwrap_or("").trim().to_string();
        let end = parts
            .next()
            .unwrap_or("")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        if start.is_empty() || end.is_empty() {
            continue;
        }
        let text = lines[arrow + 1..].join("\n");
        if text.is_empty() {
            continue;
        }
        cues.push(Cue { start, end, text });
    }
    cues
}

fn write_vtt(cues: &[Cue]) -> String {
    let mut output = String::from("WEBVTT\n\n");
    for cue in cues {
        output.push_str(&dot_ms(&cue.start));
        output.push_str(" --> ");
        output.push_str(&dot_ms(&cue.end));
        output.push('\n');
        output.push_str(&cue.text);
        output.push_str("\n\n");
    }
    output
}

fn write_srt(cues: &[Cue]) -> String {
    let mut output = String::new();
    for (index, cue) in cues.iter().enumerate() {
        output.push_str(&format!("{}\n", index + 1));
        output.push_str(&comma_ms(&cue.start));
        output.push_str(" --> ");
        output.push_str(&comma_ms(&cue.end));
        output.push('\n');
        output.push_str(&cue.text);
        output.push_str("\n\n");
    }
    output
}

fn dot_ms(stamp: &str) -> String {
    swap_ms(stamp, '.')
}

fn comma_ms(stamp: &str) -> String {
    swap_ms(stamp, ',')
}

fn swap_ms(stamp: &str, separator: char) -> String {
    match stamp.rsplit_once(['.', ',']) {
        Some((body, millis)) => format!("{body}{separator}{millis}"),
        None => stamp.to_string(),
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|_| "Fichier introuvable".to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn is_html(path: &Path) -> bool {
    let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase();
    ext == "html" || ext == "htm"
}

fn html_page(text: &str) -> String {
    format!("<!DOCTYPE html>\n<meta charset=\"utf-8\">\n<pre>\n{}\n</pre>\n", escape_html(text))
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn strip_tags(html: &str) -> String {
    let mut plain = String::new();
    let mut inside = false;
    for character in html.chars() {
        match character {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => plain.push(character),
            _ => {}
        }
    }
    plain
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
}

fn report(written: &[PathBuf], failure: Option<String>) -> Result<String, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cinder-text-{tick}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn vtt_becomes_txt() {
        let dir = scratch();
        let source = dir.join("dialogue.vtt");
        std::fs::write(&source, "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nBonjour\n").unwrap();
        let message = apply(&[source], OutputFormat::Txt).unwrap();
        assert_eq!(message, "dialogue.txt");
        assert_eq!(std::fs::read_to_string(dir.join("dialogue.txt")).unwrap(), "Bonjour");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn csv_becomes_tsv() {
        let dir = scratch();
        let source = dir.join("table.csv");
        std::fs::write(&source, "nom,ville\nAda,Paris\n").unwrap();
        let message = apply(&[source], OutputFormat::Tsv).unwrap();
        assert_eq!(message, "table.tsv");
        let text = std::fs::read_to_string(dir.join("table.tsv")).unwrap();
        assert!(text.contains("nom\tville"));
        assert!(text.contains("Ada\tParis"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
