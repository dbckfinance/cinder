use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use image::DynamicImage;
use lopdf::{Document, Object};
use windows::Data::Pdf::PdfDocument;
use windows::Storage::StorageFile;
use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::catalog::{category_of, Category, OutputFormat};
use crate::convert::available_path;

pub fn convert(paths: &[PathBuf], format: OutputFormat) -> Result<String, String> {
    run(paths, |path| convert_one(path, format))
}

pub fn compress_pdf(paths: &[PathBuf]) -> Result<String, String> {
    run(paths, compress_one_pdf)
}

pub fn compress_document(paths: &[PathBuf]) -> Result<String, String> {
    run(paths, compress_one_document)
}

pub fn split(paths: &[PathBuf]) -> Result<String, String> {
    run(paths, split_one)
}

pub fn metadata(paths: &[PathBuf]) -> Result<String, String> {
    run(paths, metadata_one)
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
    match format {
        OutputFormat::Png | OutputFormat::Jpeg => {
            return match category_of(path) {
                Some(Category::Pdf) => render_pdf_pages(path, format),
                Some(Category::Slides) => render_slides(path, format),
                _ => Err("Format inattendu".to_string()),
            };
        }
        OutputFormat::Pptx => return write_output(path, &pptx_bytes(&pdf_page_texts(path)?)?, "pptx"),
        OutputFormat::Xlsx => return write_output(path, &xlsx_bytes(&pdf_line_rows(path)?)?, "xlsx"),
        OutputFormat::Ods => {
            if category_of(path) != Some(Category::Spreadsheet) {
                return Err("Format inattendu".to_string());
            }
            let rows = crate::textconv::table_rows(path)?;
            if rows.is_empty() {
                return Err("Feuille vide".to_string());
            }
            return write_output(path, &ods_bytes(&rows)?, "ods");
        }
        OutputFormat::Epub => return write_output(path, &epub_bytes(&paragraphs_of(path)?)?, "epub"),
        OutputFormat::Pdf if category_of(path) == Some(Category::Slides) => {
            return write_output(path, &slide_pdf_bytes(path)?, "pdf");
        }
        _ => {}
    }
    let paragraphs = paragraphs_of(path)?;
    let bytes = match format {
        OutputFormat::Txt | OutputFormat::Md => paragraphs.join("\n\n").into_bytes(),
        OutputFormat::Html => html_page(&paragraphs).into_bytes(),
        OutputFormat::Pdf => text_pdf(&paragraphs),
        OutputFormat::Docx => docx_bytes(&paragraphs)?,
        _ => return Err("Format inattendu".to_string()),
    };
    write_output(path, &bytes, format.extension())
}

fn paragraphs_of(path: &Path) -> Result<Vec<String>, String> {
    match category_of(path) {
        Some(Category::Pdf) => pdf_paragraphs(path),
        Some(Category::Document) => document_paragraphs(path),
        Some(Category::Epub) => epub_paragraphs(path),
        Some(Category::Slides) => slide_paragraphs(path),
        Some(Category::Spreadsheet) => spreadsheet_paragraphs(path),
        Some(Category::Text) => text_paragraphs(path),
        _ => Err("Pas encore de conversion pour ce type".to_string()),
    }
}

fn pdf_paragraphs(path: &Path) -> Result<Vec<String>, String> {
    let text = pdf_extract::extract_text(path).map_err(|_| "Impossible de lire le PDF".to_string())?;
    let paragraphs = split_paragraphs(&text);
    if paragraphs.is_empty() {
        return Err("Aucun texte dans ce PDF".to_string());
    }
    Ok(paragraphs)
}

fn document_paragraphs(path: &Path) -> Result<Vec<String>, String> {
    let ext = extension(path);
    let paragraphs = if ext == "rtf" {
        split_paragraphs(&rtf_text(&read_string(path)?))
    } else if ext == "odt" {
        let xml = zip_text(path, "content.xml")?;
        marked_paragraphs(&xml, "</text:p>", "</text:h>")
    } else {
        let xml = zip_text(path, "word/document.xml")?;
        docx_paragraphs(&xml)
    };
    if paragraphs.is_empty() {
        return Err("Aucun texte dans ce document".to_string());
    }
    Ok(paragraphs)
}

fn compress_one_pdf(path: &Path) -> Result<PathBuf, String> {
    let before = std::fs::metadata(path).map_err(|_| "Fichier introuvable".to_string())?.len();
    let mut document = Document::load(path).map_err(|_| "Impossible de lire le PDF".to_string())?;
    document.compress();
    let destination = available_path(parent_of(path)?, &format!("{}-compresse", file_stem(path)), "pdf");
    document
        .save(&destination)
        .map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    let after = std::fs::metadata(&destination).map(|meta| meta.len()).unwrap_or(before);
    if after >= before {
        let _ = std::fs::remove_file(&destination);
        return Err("Déjà compressé".to_string());
    }
    Ok(destination)
}

fn compress_one_document(path: &Path) -> Result<PathBuf, String> {
    let ext = extension(path);
    if ext == "rtf" {
        return Err("Déjà compressé".to_string());
    }
    let before = std::fs::metadata(path).map_err(|_| "Fichier introuvable".to_string())?.len();
    let file = File::open(path).map_err(|_| "Fichier introuvable".to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|_| "Document illisible".to_string())?;
    let destination = available_path(parent_of(path)?, &format!("{}-compresse", file_stem(path)), &ext);
    let output = File::create(&destination).map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    let mut writer = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| "Document illisible".to_string())?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|_| "Document illisible".to_string())?;
        writer
            .start_file(&name, options)
            .map_err(|_| "Impossible d'écrire le fichier".to_string())?;
        writer
            .write_all(&bytes)
            .map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    }
    writer.finish().map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    let after = std::fs::metadata(&destination).map(|meta| meta.len()).unwrap_or(before);
    if after >= before {
        let _ = std::fs::remove_file(&destination);
        return Err("Déjà compressé".to_string());
    }
    Ok(destination)
}

fn split_one(path: &Path) -> Result<PathBuf, String> {
    let document = Document::load(path).map_err(|_| "Impossible de lire le PDF".to_string())?;
    let numbers: Vec<u32> = document.get_pages().keys().copied().collect();
    if numbers.is_empty() {
        return Err("PDF sans page".to_string());
    }
    let directory = available_path(parent_of(path)?, &format!("{}-pages", file_stem(path)), "");
    std::fs::create_dir_all(&directory).map_err(|_| "Impossible de créer le dossier".to_string())?;
    for keep in &numbers {
        let mut copy = document.clone();
        let remove: Vec<u32> = numbers.iter().copied().filter(|number| number != keep).collect();
        if !remove.is_empty() {
            copy.delete_pages(&remove);
            copy.prune_objects();
        }
        copy.save(directory.join(format!("page-{keep}.pdf")))
            .map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    }
    Ok(directory)
}

fn metadata_one(path: &Path) -> Result<PathBuf, String> {
    let text = match category_of(path) {
        Some(Category::Pdf) => pdf_metadata(path)?,
        Some(Category::Document) => document_metadata(path)?,
        _ => return Err("Aucun outil pour ce type".to_string()),
    };
    let destination = available_path(parent_of(path)?, &format!("{}-meta", file_stem(path)), "txt");
    std::fs::write(&destination, text).map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    Ok(destination)
}

fn pdf_metadata(path: &Path) -> Result<String, String> {
    let document = Document::load(path).map_err(|_| "Impossible de lire le PDF".to_string())?;
    let mut lines = vec![format!("Pages : {}", document.get_pages().len())];
    if let Some(info) = info_dictionary(&document) {
        for key in ["Title", "Author", "Subject", "Creator", "Producer"] {
            if let Ok(value) = info.get(key.as_bytes()).and_then(Object::as_string) {
                let text = value.trim();
                if !text.is_empty() {
                    lines.push(format!("{key} : {text}"));
                }
            }
        }
    }
    Ok(lines.join("\n"))
}

fn info_dictionary(document: &Document) -> Option<&lopdf::Dictionary> {
    let object = document.trailer.get(b"Info").ok()?;
    let object = match object {
        Object::Reference(id) => document.get_object(*id).ok()?,
        other => other,
    };
    object.as_dict().ok()
}

fn document_metadata(path: &Path) -> Result<String, String> {
    let ext = extension(path);
    if ext == "rtf" {
        return Ok("Format : RTF".to_string());
    }
    let name = if ext == "odt" { "meta.xml" } else { "docProps/core.xml" };
    let xml = zip_text(path, name).unwrap_or_default();
    let mut lines = Vec::new();
    for (tag, label) in [
        ("dc:title", "Titre"),
        ("dc:creator", "Auteur"),
        ("dc:subject", "Sujet"),
        ("cp:lastModifiedBy", "Modifié par"),
        ("dcterms:created", "Créé"),
    ] {
        if let Some(value) = xml_tag(&xml, tag) {
            if !value.trim().is_empty() {
                lines.push(format!("{label} : {}", unescape_xml(value.trim())));
            }
        }
    }
    if lines.is_empty() {
        lines.push("Aucune métadonnée".to_string());
    }
    Ok(lines.join("\n"))
}

fn xml_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let start = xml.find(&open)?;
    let after = xml[start..].find('>')? + start + 1;
    let close = format!("</{tag}>");
    let end = xml[after..].find(&close)? + after;
    Some(&xml[after..end])
}

fn split_paragraphs(text: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    for block in text.replace("\r\n", "\n").split("\n\n") {
        let paragraph = block.split_whitespace().collect::<Vec<_>>().join(" ");
        if !paragraph.is_empty() {
            paragraphs.push(paragraph);
        }
    }
    if paragraphs.is_empty() {
        for line in text.lines() {
            let line = line.trim();
            if !line.is_empty() {
                paragraphs.push(line.to_string());
            }
        }
    }
    paragraphs
}

fn docx_paragraphs(xml: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    for chunk in xml.split("</w:p>") {
        let mut text = String::new();
        let mut rest = chunk;
        while let Some(start) = rest.find("<w:t") {
            let Some(close) = rest[start..].find('>') else {
                break;
            };
            let content_start = start + close + 1;
            let Some(end) = rest[content_start..].find("</w:t>") else {
                break;
            };
            text.push_str(&unescape_xml(&rest[content_start..content_start + end]));
            rest = &rest[content_start + end + 6..];
        }
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            paragraphs.push(text);
        }
    }
    paragraphs
}

fn marked_paragraphs(xml: &str, end_a: &str, end_b: &str) -> Vec<String> {
    let marked = xml.replace(end_b, end_a);
    marked
        .split(end_a)
        .filter_map(|chunk| {
            let plain = strip_tags(chunk);
            let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
            if plain.is_empty() {
                None
            } else {
                Some(plain)
            }
        })
        .collect()
}

fn rtf_text(rtf: &str) -> String {
    let mut text = String::new();
    let mut chars = rtf.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' {
            let next = chars.peek().copied();
            if next == Some('\'') {
                chars.next();
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    text.push(byte as char);
                }
                continue;
            }
            if next == Some('\\') || next == Some('{') || next == Some('}') {
                text.push(chars.next().unwrap_or(character));
                continue;
            }
            let mut word = String::new();
            while let Some(part) = chars.peek() {
                if part.is_ascii_alphabetic() {
                    word.push(*part);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek().is_some_and(|part| part.is_ascii_digit() || *part == '-') {
                while chars.peek().is_some_and(|part| part.is_ascii_digit() || *part == '-') {
                    chars.next();
                }
            }
            if chars.peek() == Some(&' ') {
                chars.next();
            }
            if word == "par" || word == "line" {
                text.push('\n');
            }
            continue;
        }
        if character != '{' && character != '}' {
            text.push(character);
        }
    }
    text
}

fn zip_text(path: &Path, name: &str) -> Result<String, String> {
    let file = File::open(path).map_err(|_| "Fichier introuvable".to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|_| "Document illisible".to_string())?;
    let mut entry = archive
        .by_name(name)
        .map_err(|_| "Document illisible".to_string())?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|_| "Document illisible".to_string())?;
    Ok(text)
}

fn read_string(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|_| "Fichier introuvable".to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn html_page(paragraphs: &[String]) -> String {
    let body = paragraphs
        .iter()
        .map(|paragraph| format!("<p>{}</p>", escape_html(paragraph)))
        .collect::<Vec<_>>()
        .join("\n");
    format!("<!DOCTYPE html>\n<meta charset=\"utf-8\">\n{body}\n")
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn docx_bytes(paragraphs: &[String]) -> Result<Vec<u8>, String> {
    let mut body = String::new();
    for paragraph in paragraphs {
        body.push_str("<w:p><w:r><w:t xml:space=\"preserve\">");
        body.push_str(&escape_xml(paragraph));
        body.push_str("</w:t></w:r></w:p>");
    }
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr></w:body></w:document>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
</Types>";
    let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer
            .start_file("[Content_Types].xml", options)
            .map_err(|_| "Impossible d'écrire le DOCX".to_string())?;
        writer
            .write_all(content_types.as_bytes())
            .map_err(|_| "Impossible d'écrire le DOCX".to_string())?;
        writer
            .start_file("_rels/.rels", options)
            .map_err(|_| "Impossible d'écrire le DOCX".to_string())?;
        writer
            .write_all(rels.as_bytes())
            .map_err(|_| "Impossible d'écrire le DOCX".to_string())?;
        writer
            .start_file("word/document.xml", options)
            .map_err(|_| "Impossible d'écrire le DOCX".to_string())?;
        writer
            .write_all(document.as_bytes())
            .map_err(|_| "Impossible d'écrire le DOCX".to_string())?;
        writer.finish().map_err(|_| "Impossible d'écrire le DOCX".to_string())?;
    }
    Ok(cursor.into_inner())
}

fn write_output(path: &Path, bytes: &[u8], extension: &str) -> Result<PathBuf, String> {
    let destination = available_path(parent_of(path)?, &file_stem(path), extension);
    std::fs::write(&destination, bytes).map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    Ok(destination)
}

fn text_paragraphs(path: &Path) -> Result<Vec<String>, String> {
    let source = read_string(path)?;
    let plain = if matches!(extension(path).as_str(), "html" | "htm" | "xhtml") {
        strip_tags(&source)
    } else {
        source
    };
    let paragraphs = split_paragraphs(&plain);
    if paragraphs.is_empty() {
        return Err("Aucun texte dans ce document".to_string());
    }
    Ok(paragraphs)
}

fn epub_paragraphs(path: &Path) -> Result<Vec<String>, String> {
    let html = crate::textconv::epub_markup(path)?;
    let paragraphs = split_paragraphs(&strip_tags(&html));
    if paragraphs.is_empty() {
        return Err("Aucun texte dans cet EPUB".to_string());
    }
    Ok(paragraphs)
}

fn spreadsheet_paragraphs(path: &Path) -> Result<Vec<String>, String> {
    let rows = crate::textconv::table_rows(path)?;
    let paragraphs: Vec<String> = rows
        .into_iter()
        .map(|row| row.join("\t"))
        .filter(|line| !line.trim().is_empty())
        .collect();
    if paragraphs.is_empty() {
        return Err("Feuille vide".to_string());
    }
    Ok(paragraphs)
}

fn slide_paragraphs(path: &Path) -> Result<Vec<String>, String> {
    let ext = extension(path);
    let paragraphs = if ext == "pptx" {
        pptx_slides(path)?
    } else if ext == "odp" {
        let xml = zip_text(path, "content.xml").map_err(|_| "Diapositives illisibles".to_string())?;
        marked_paragraphs(&xml, "</text:p>", "</text:h>")
    } else {
        return Err("Diapositives illisibles".to_string());
    };
    if paragraphs.iter().all(|paragraph| paragraph.trim().is_empty()) {
        return Err("Aucun texte dans ces diapositives".to_string());
    }
    Ok(paragraphs)
}

fn pptx_slides(path: &Path) -> Result<Vec<String>, String> {
    let file = File::open(path).map_err(|_| "Fichier introuvable".to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|_| "Diapositives illisibles".to_string())?;
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|_| "Diapositives illisibles".to_string())?
            .name()
            .to_string();
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") && !name.contains("_rels") {
            names.push(name);
        }
    }
    names.sort_by_key(|name| slide_index(name));
    if names.is_empty() {
        return Err("Diapositives illisibles".to_string());
    }
    let mut slides = Vec::new();
    for name in names {
        let mut entry = archive.by_name(&name).map_err(|_| "Diapositives illisibles".to_string())?;
        let mut xml = String::new();
        entry
            .read_to_string(&mut xml)
            .map_err(|_| "Diapositives illisibles".to_string())?;
        slides.push(texts_between(&xml, "<a:t", "</a:t>").join("\n"));
    }
    Ok(slides)
}

fn slide_index(name: &str) -> u32 {
    name.trim_end_matches(".xml")
        .rsplit(|character: char| !character.is_ascii_digit())
        .next()
        .and_then(|number| number.parse().ok())
        .unwrap_or(0)
}

fn texts_between(xml: &str, open: &str, close: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(open) {
        let Some(bracket) = rest[start..].find('>') else {
            break;
        };
        let content_start = start + bracket + 1;
        let Some(end) = rest[content_start..].find(close) else {
            break;
        };
        let text = unescape_xml(&rest[content_start..content_start + end]);
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !text.is_empty() {
            found.push(text);
        }
        rest = &rest[content_start + end + close.len()..];
    }
    found
}

fn pdf_page_texts(path: &Path) -> Result<Vec<String>, String> {
    if category_of(path) != Some(Category::Pdf) {
        return Err("Format inattendu".to_string());
    }
    let pages = pdf_extract::extract_text_by_pages(path).map_err(|_| "Impossible de lire le PDF".to_string())?;
    if pages.iter().all(|page| page.trim().is_empty()) {
        return Err("Aucun texte dans ce PDF".to_string());
    }
    Ok(pages)
}

fn pdf_line_rows(path: &Path) -> Result<Vec<Vec<String>>, String> {
    if category_of(path) != Some(Category::Pdf) {
        return Err("Format inattendu".to_string());
    }
    let text = pdf_extract::extract_text(path).map_err(|_| "Impossible de lire le PDF".to_string())?;
    let rows: Vec<Vec<String>> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| vec![line.to_string()])
        .collect();
    if rows.is_empty() {
        return Err("Aucun texte dans ce PDF".to_string());
    }
    Ok(rows)
}

fn epub_bytes(paragraphs: &[String]) -> Result<Vec<u8>, String> {
    let body = paragraphs
        .iter()
        .map(|paragraph| format!("<p>{}</p>", xml_text(paragraph)))
        .collect::<Vec<_>>()
        .join("");
    let chapter = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"fr\"><head><meta charset=\"utf-8\"/><title>Document</title></head><body>{body}</body></html>"
    );
    let container = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\">\
<rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";
    let opf = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\">\
<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"bookid\">cinder</dc:identifier>\
<dc:title>Document</dc:title><dc:language>fr</dc:language></metadata>\
<manifest><item id=\"chap\" href=\"chapter.xhtml\" media-type=\"application/xhtml+xml\"/></manifest>\
<spine><itemref idref=\"chap\"/></spine></package>";
    zip_archive(vec![
        ("mimetype".to_string(), b"application/epub+zip".to_vec(), true),
        ("META-INF/container.xml".to_string(), container.as_bytes().to_vec(), false),
        ("OEBPS/content.opf".to_string(), opf.as_bytes().to_vec(), false),
        ("OEBPS/chapter.xhtml".to_string(), chapter.into_bytes(), false),
    ])
}

fn pptx_bytes(slides: &[String]) -> Result<Vec<u8>, String> {
    if slides.is_empty() {
        return Err("Aucun texte dans ce PDF".to_string());
    }
    let mut overrides = String::from(
        "<Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\
<Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/>\
<Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/>\
<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>",
    );
    let mut slide_ids = String::new();
    let mut presentation_rels = String::new();
    let mut entries = vec![
        ("ppt/theme/theme1.xml".to_string(), theme_xml().into_bytes(), false),
        ("ppt/slideMasters/slideMaster1.xml".to_string(), slide_master_xml().into_bytes(), false),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels".to_string(),
            master_rels().into_bytes(),
            false,
        ),
        ("ppt/slideLayouts/slideLayout1.xml".to_string(), slide_layout_xml().into_bytes(), false),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels".to_string(),
            layout_rels().into_bytes(),
            false,
        ),
    ];
    for (index, slide) in slides.iter().enumerate() {
        let number = index + 1;
        overrides.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{number}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
        slide_ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"rId{number}\"/>", 256 + index));
        presentation_rels.push_str(&format!(
            "<Relationship Id=\"rId{number}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide{number}.xml\"/>"
        ));
        entries.push((format!("ppt/slides/slide{number}.xml"), slide_xml(slide).into_bytes(), false));
        entries.push((
            format!("ppt/slides/_rels/slide{number}.xml.rels"),
            slide_rels().into_bytes(),
            false,
        ));
    }
    let master_id = slides.len() + 1;
    presentation_rels.push_str(&format!(
        "<Relationship Id=\"rId{master_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>"
    ));
    let content_types = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>{overrides}</Types>"
    );
    let presentation = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx=\"9144000\" cy=\"6858000\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>"
    );
    let package_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/>\
</Relationships>";
    let presentation_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{presentation_rels}</Relationships>"
    );
    let mut packed = vec![
        ("[Content_Types].xml".to_string(), content_types.into_bytes(), false),
        ("_rels/.rels".to_string(), package_rels.as_bytes().to_vec(), false),
        ("ppt/presentation.xml".to_string(), presentation.into_bytes(), false),
        ("ppt/_rels/presentation.xml.rels".to_string(), presentation_rels.into_bytes(), false),
    ];
    packed.append(&mut entries);
    zip_archive(packed)
}

fn slide_xml(text: &str) -> String {
    let paragraphs = if text.trim().is_empty() {
        "<a:p/>".to_string()
    } else {
        text.lines()
            .map(|line| {
                format!(
                    "<a:p><a:r><a:rPr lang=\"fr-FR\" dirty=\"0\"/><a:t>{}</a:t></a:r></a:p>",
                    xml_text(line)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:cSld><p:spTree>\
<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>\
<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Texte\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr/></p:nvSpPr>\
<p:spPr><a:xfrm><a:off x=\"457200\" y=\"457200\"/><a:ext cx=\"8229600\" cy=\"5943600\"/></a:xfrm></p:spPr>\
<p:txBody><a:bodyPr/><a:lstStyle/>{paragraphs}</p:txBody></p:sp>\
</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"
    )
}

fn slide_rels() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>\
</Relationships>"
        .to_string()
}

fn layout_rels() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster\" Target=\"../slideMasters/slideMaster1.xml\"/>\
</Relationships>"
        .to_string()
}

fn master_rels() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme\" Target=\"../theme/theme1.xml\"/>\
</Relationships>"
        .to_string()
}

fn slide_layout_xml() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<p:sldLayout xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" type=\"blank\">\
<p:cSld name=\"Vide\"><p:spTree>\
<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>\
</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"
        .to_string()
}

fn slide_master_xml() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<p:sldMaster xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\
<p:cSld><p:spTree>\
<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>\
<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>\
</p:spTree></p:cSld>\
<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>\
<p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst></p:sldMaster>"
        .to_string()
}

fn theme_xml() -> String {
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Cinder\"><a:themeElements>\
<a:clrScheme name=\"Cinder\">\
<a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1><a:lt1><a:sysClr val=\"window\" lastClr=\"FFFFFF\"/></a:lt1>\
<a:dk2><a:srgbClr val=\"1F497D\"/></a:dk2><a:lt2><a:srgbClr val=\"EEECE1\"/></a:lt2>\
<a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1><a:accent2><a:srgbClr val=\"C0504D\"/></a:accent2>\
<a:accent3><a:srgbClr val=\"9BBB59\"/></a:accent3><a:accent4><a:srgbClr val=\"8064A2\"/></a:accent4>\
<a:accent5><a:srgbClr val=\"4BACC6\"/></a:accent5><a:accent6><a:srgbClr val=\"F79646\"/></a:accent6>\
<a:hlink><a:srgbClr val=\"0000FF\"/></a:hlink><a:folHlink><a:srgbClr val=\"800080\"/></a:folHlink></a:clrScheme>\
<a:fontScheme name=\"Cinder\"><a:majorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont>\
<a:minorFont><a:latin typeface=\"Calibri\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme>\
<a:fmtScheme name=\"Cinder\"><a:fillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:fillStyleLst>\
<a:lnStyleLst><a:ln w=\"6350\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:ln></a:lnStyleLst>\
<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>\
<a:bgFillStyleLst><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme>\
</a:themeElements></a:theme>"
        .to_string()
}

fn xlsx_bytes(rows: &[Vec<String>]) -> Result<Vec<u8>, String> {
    let mut sheet_rows = String::new();
    for (row_index, row) in rows.iter().enumerate() {
        let row_number = row_index + 1;
        sheet_rows.push_str(&format!("<row r=\"{row_number}\">"));
        for (column, cell) in row.iter().enumerate() {
            let reference = format!("{}{row_number}", column_name(column));
            sheet_rows.push_str(&format!(
                "<c r=\"{reference}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
                xml_text(cell)
            ));
        }
        sheet_rows.push_str("</row>");
    }
    let sheet = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>{sheet_rows}</sheetData></worksheet>"
    );
    let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/worksheets/sheet1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>\
</Types>";
    let package_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/>\
</Relationships>";
    let workbook = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
<sheets><sheet name=\"Feuille1\" sheetId=\"1\" r:id=\"rId1\"/></sheets></workbook>";
    let workbook_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet1.xml\"/>\
</Relationships>";
    zip_archive(vec![
        ("[Content_Types].xml".to_string(), content_types.as_bytes().to_vec(), false),
        ("_rels/.rels".to_string(), package_rels.as_bytes().to_vec(), false),
        ("xl/workbook.xml".to_string(), workbook.as_bytes().to_vec(), false),
        ("xl/_rels/workbook.xml.rels".to_string(), workbook_rels.as_bytes().to_vec(), false),
        ("xl/worksheets/sheet1.xml".to_string(), sheet.into_bytes(), false),
    ])
}

fn ods_bytes(rows: &[Vec<String>]) -> Result<Vec<u8>, String> {
    let mut table_rows = String::new();
    for row in rows {
        table_rows.push_str("<table:table-row>");
        for cell in row {
            table_rows.push_str(&format!(
                "<table:table-cell office:value-type=\"string\"><text:p>{}</text:p></table:table-cell>",
                xml_text(cell)
            ));
        }
        table_rows.push_str("</table:table-row>");
    }
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\">\
<office:body><office:spreadsheet><table:table table:name=\"Feuille1\">{table_rows}</table:table></office:spreadsheet></office:body></office:document-content>"
    );
    let manifest = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\">\
<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.spreadsheet\"/>\
<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
</manifest:manifest>";
    zip_archive(vec![
        (
            "mimetype".to_string(),
            b"application/vnd.oasis.opendocument.spreadsheet".to_vec(),
            true,
        ),
        ("META-INF/manifest.xml".to_string(), manifest.as_bytes().to_vec(), false),
        ("content.xml".to_string(), content.into_bytes(), false),
    ])
}

fn column_name(index: usize) -> String {
    let mut number = index + 1;
    let mut name = Vec::new();
    while number > 0 {
        number -= 1;
        name.push(b'A' + (number % 26) as u8);
        number /= 26;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_else(|_| "A".to_string())
}

fn xml_text(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .filter(|character| !matches!(*character, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}'))
        .collect();
    escape_xml(&cleaned)
}

fn zip_archive(entries: Vec<(String, Vec<u8>, bool)>) -> Result<Vec<u8>, String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = ZipWriter::new(&mut cursor);
        for (name, data, stored) in entries {
            let method = if stored { CompressionMethod::Stored } else { CompressionMethod::Deflated };
            writer
                .start_file(name, SimpleFileOptions::default().compression_method(method))
                .map_err(|_| "Impossible d'écrire le fichier".to_string())?;
            writer
                .write_all(&data)
                .map_err(|_| "Impossible d'écrire le fichier".to_string())?;
        }
        writer.finish().map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    }
    Ok(cursor.into_inner())
}

fn render_slides(path: &Path, format: OutputFormat) -> Result<PathBuf, String> {
    let bytes = slide_pdf_bytes(path)?;
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let temp = std::env::temp_dir().join(format!("cinder-slide-{tick}.pdf"));
    std::fs::write(&temp, bytes).map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    let rendered = render_pdf_file(&temp, path, format);
    let _ = std::fs::remove_file(&temp);
    rendered
}

fn render_pdf_pages(path: &Path, format: OutputFormat) -> Result<PathBuf, String> {
    render_pdf_file(path, path, format)
}

fn render_pdf_file(pdf: &Path, named: &Path, format: OutputFormat) -> Result<PathBuf, String> {
    let (document, _stream) = open_pdf_document(pdf)?;
    let count = document.PageCount().map_err(|_| "Impossible de lire le PDF".to_string())?;
    if count == 0 {
        return Err("PDF sans page".to_string());
    }
    let jpeg = format == OutputFormat::Jpeg;
    let extension = if jpeg { "jpg" } else { "png" };
    if count == 1 {
        let bytes = encode_page(render_page(&document, 0)?, jpeg)?;
        return write_output(named, &bytes, extension);
    }
    let folder = available_path(parent_of(named)?, &format!("{}-pages", file_stem(named)), "");
    std::fs::create_dir_all(&folder).map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    for index in 0..count {
        let bytes = match encode_page(render_page(&document, index)?, jpeg) {
            Ok(bytes) => bytes,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&folder);
                return Err(error);
            }
        };
        if std::fs::write(folder.join(format!("page-{}.{extension}", index + 1)), bytes).is_err() {
            let _ = std::fs::remove_dir_all(&folder);
            return Err("Impossible d'écrire le fichier".to_string());
        }
    }
    Ok(folder)
}

fn open_pdf_document(path: &Path) -> Result<(PdfDocument, Option<InMemoryRandomAccessStream>), String> {
    ensure_winrt()?;
    let wide = windows::core::HSTRING::from(path.to_string_lossy().as_ref());
    if let Ok(operation) = StorageFile::GetFileFromPathAsync(&wide) {
        if let Ok(file) = operation.join() {
            if let Ok(load) = PdfDocument::LoadFromFileAsync(&file) {
                if let Ok(document) = load.join() {
                    return Ok((document, None));
                }
            }
        }
    }
    let bytes = std::fs::read(path).map_err(|_| "Fichier introuvable".to_string())?;
    let stream = memory_stream(&bytes)?;
    let document = PdfDocument::LoadFromStreamAsync(&stream)
        .map_err(|_| "Impossible de lire le PDF".to_string())?
        .join()
        .map_err(|_| "Impossible de lire le PDF".to_string())?;
    Ok((document, Some(stream)))
}

fn ensure_winrt() -> Result<(), String> {
    match unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        Ok(()) => Ok(()),
        Err(error) if error.code().0 == 0x80010106u32 as i32 || error.code().0 == 0x800401F0u32 as i32 => Ok(()),
        Err(_) => Err("Impossible de rendre la page".to_string()),
    }
}

fn memory_stream(bytes: &[u8]) -> Result<InMemoryRandomAccessStream, String> {
    let stream = InMemoryRandomAccessStream::new().map_err(|_| "Impossible de rendre la page".to_string())?;
    let output = stream.GetOutputStreamAt(0).map_err(|_| "Impossible de rendre la page".to_string())?;
    let writer = DataWriter::CreateDataWriter(&output).map_err(|_| "Impossible de rendre la page".to_string())?;
    writer.WriteBytes(bytes).map_err(|_| "Impossible de rendre la page".to_string())?;
    writer
        .StoreAsync()
        .map_err(|_| "Impossible de rendre la page".to_string())?
        .join()
        .map_err(|_| "Impossible de rendre la page".to_string())?;
    writer.DetachStream().map_err(|_| "Impossible de rendre la page".to_string())?;
    stream.Seek(0).map_err(|_| "Impossible de rendre la page".to_string())?;
    Ok(stream)
}

fn render_page(document: &PdfDocument, index: u32) -> Result<DynamicImage, String> {
    let page = document.GetPage(index).map_err(|_| "Impossible de rendre la page".to_string())?;
    let stream = InMemoryRandomAccessStream::new().map_err(|_| "Impossible de rendre la page".to_string())?;
    page.RenderToStreamAsync(&stream)
        .map_err(|_| "Impossible de rendre la page".to_string())?
        .join()
        .map_err(|_| "Impossible de rendre la page".to_string())?;
    let bytes = read_stream(&stream)?;
    let _ = page.Close();
    image::load_from_memory(&bytes).map_err(|_| "Impossible de rendre la page".to_string())
}

fn read_stream(stream: &InMemoryRandomAccessStream) -> Result<Vec<u8>, String> {
    let size = stream.Size().map_err(|_| "Impossible de rendre la page".to_string())?;
    if size == 0 || size > u32::MAX as u64 {
        return Err("Impossible de rendre la page".to_string());
    }
    stream.Seek(0).map_err(|_| "Impossible de rendre la page".to_string())?;
    let input = stream.GetInputStreamAt(0).map_err(|_| "Impossible de rendre la page".to_string())?;
    let reader = DataReader::CreateDataReader(&input).map_err(|_| "Impossible de rendre la page".to_string())?;
    reader
        .LoadAsync(size as u32)
        .map_err(|_| "Impossible de rendre la page".to_string())?
        .join()
        .map_err(|_| "Impossible de rendre la page".to_string())?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).map_err(|_| "Impossible de rendre la page".to_string())?;
    Ok(bytes)
}

fn encode_page(image: DynamicImage, jpeg: bool) -> Result<Vec<u8>, String> {
    let rgba = image.to_rgba8();
    if rgba.width() == 0 || rgba.height() == 0 {
        return Err("Impossible de rendre la page".to_string());
    }
    let mut canvas = image::RgbaImage::from_pixel(rgba.width(), rgba.height(), image::Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut canvas, &rgba, 0, 0);
    let mut bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut bytes);
    let result = if jpeg {
        DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(canvas).to_rgb8()).write_to(&mut cursor, image::ImageFormat::Jpeg)
    } else {
        DynamicImage::ImageRgba8(canvas).write_to(&mut cursor, image::ImageFormat::Png)
    };
    result.map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    Ok(bytes)
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn text_pdf(paragraphs: &[String]) -> Vec<u8> {
    let lines = wrap_lines(&paragraphs.join("\n\n"), 85);
    let pages: Vec<Vec<String>> = if lines.is_empty() {
        vec![Vec::new()]
    } else {
        lines.chunks(46).map(|chunk| chunk.to_vec()).collect()
    };
    pdf_pages(&pages, 595, 842, 11, 14, 50, 790)
}

fn slides_pdf(slides: &[String]) -> Vec<u8> {
    let pages: Vec<Vec<String>> = slides
        .iter()
        .map(|slide| {
            let lines = wrap_lines(slide, 52);
            if lines.is_empty() {
                vec![String::new()]
            } else {
                lines.into_iter().take(14).collect()
            }
        })
        .collect();
    pdf_pages(&pages, 960, 540, 20, 28, 48, 490)
}

fn slide_pdf_bytes(path: &Path) -> Result<Vec<u8>, String> {
    if extension(path) == "pptx" {
        let slides = pptx_slides(path)?;
        if slides.iter().all(|slide| slide.trim().is_empty()) {
            return Err("Aucun texte dans ces diapositives".to_string());
        }
        return Ok(slides_pdf(&slides));
    }
    Ok(text_pdf(&slide_paragraphs(path)?))
}

fn pdf_pages(pages: &[Vec<String>], width: i32, height: i32, font: i32, leading: i32, x: i32, y: i32) -> Vec<u8> {
    let blank = vec![Vec::new()];
    let pages = if pages.is_empty() { blank.as_slice() } else { pages };
    let mut objects: Vec<Vec<u8>> = Vec::new();
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objects.push(Vec::new());
    objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec());
    let mut kids = Vec::new();
    for (index, lines) in pages.iter().enumerate() {
        let page_number = 4 + index * 2;
        let content_number = page_number + 1;
        kids.push(format!("{page_number} 0 R"));
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Contents {content_number} 0 R /Resources << /Font << /F1 3 0 R >> >> >>"
            )
            .into_bytes(),
        );
        objects.push(stream_object(&page_stream(lines, font, leading, x, y)));
    }
    objects[1] = format!("<< /Type /Pages /Count {} /Kids [{}] >>", pages.len(), kids.join(" ")).into_bytes();
    assemble_pdf(&objects)
}

fn page_stream(lines: &[String], font: i32, leading: i32, x: i32, y: i32) -> Vec<u8> {
    let mut stream = format!("BT\n/F1 {font} Tf\n{x} {y} Td\n{leading} TL\n").into_bytes();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            stream.extend_from_slice(b"T*\n");
        }
        stream.extend_from_slice(&pdf_literal(line));
        stream.extend_from_slice(b" Tj\n");
    }
    stream.extend_from_slice(b"ET\n");
    stream
}

fn stream_object(data: &[u8]) -> Vec<u8> {
    let mut object = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
    object.extend_from_slice(data);
    object.extend_from_slice(b"\nendstream");
    object
}

fn assemble_pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut document = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(document.len());
        document.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        document.extend_from_slice(body);
        document.extend_from_slice(b"\nendobj\n");
    }
    let xref = document.len();
    document.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    document.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        document.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    document.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    document
}

fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if line.is_empty() {
                line = word.to_string();
            } else if line.chars().count() + 1 + word.chars().count() > width {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line.push(' ');
                line.push_str(word);
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn pdf_literal(text: &str) -> Vec<u8> {
    let mut literal = vec![b'('];
    for byte in winansi(text) {
        if matches!(byte, b'\\' | b'(' | b')') {
            literal.push(b'\\');
        }
        literal.push(byte);
    }
    literal.push(b')');
    literal
}

fn winansi(text: &str) -> Vec<u8> {
    text.chars()
        .map(|character| {
            let code = character as u32;
            if code == 0x20 || (0x21..=0x7E).contains(&code) || (0xA0..=0xFF).contains(&code) {
                code as u8
            } else if character == '\u{0152}' || character == '\u{0153}' {
                b'?'
            } else {
                b'?'
            }
        })
        .collect()
}

fn unescape_xml(input: &str) -> String {
    let mut output = String::new();
    let mut rest = input;
    while let Some(position) = rest.find('&') {
        output.push_str(&rest[..position]);
        rest = &rest[position..];
        if let Some(end) = rest.find(';') {
            if let Some(character) = decode_entity(&rest[1..end]) {
                output.push(character);
                rest = &rest[end + 1..];
                continue;
            }
        }
        output.push('&');
        rest = &rest[1..];
    }
    output.push_str(rest);
    output
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ if entity.starts_with('#') => {
            let code = if let Some(hex) = entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                entity[1..].parse().ok()?
            };
            char::from_u32(code)
        }
        _ => None,
    }
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
    unescape_xml(&plain)
}

fn parent_of(path: &Path) -> Result<&Path, String> {
    path.parent().ok_or_else(|| "Dossier introuvable".to_string())
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fichier".to_string())
}

fn extension(path: &Path) -> String {
    path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cinder-doc-{tick}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_docx(path: &Path, paragraph: &str) {
        let paragraphs = vec![paragraph.to_string()];
        std::fs::write(path, docx_bytes(&paragraphs).unwrap()).unwrap();
    }

    #[test]
    fn docx_becomes_txt() {
        let dir = scratch();
        let source = dir.join("note.docx");
        write_docx(&source, "Bonjour le contrat");
        let message = convert(&[source], OutputFormat::Txt).unwrap();
        assert_eq!(message, "note.txt");
        assert!(std::fs::read_to_string(dir.join("note.txt")).unwrap().contains("Bonjour le contrat"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pptx_becomes_pdf_and_png_and_xlsx_becomes_a_sheet() {
        let dir = scratch();
        let deck = dir.join("deck.pptx");
        std::fs::write(&deck, pptx_bytes(&["Bonjour les diapositives".to_string()]).unwrap()).unwrap();
        let pdf = convert(std::slice::from_ref(&deck), OutputFormat::Pdf).unwrap();
        let slide_text = pdf_extract::extract_text(dir.join(&pdf)).unwrap();
        assert!(slide_text.contains("Bonjour les diapositives"), "{slide_text}");
        let png = convert(&[deck], OutputFormat::Png).unwrap();
        let bytes = std::fs::read(dir.join(&png)).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"), "{png}");
        let sheet = dir.join("classeur.xlsx");
        std::fs::write(&sheet, xlsx_bytes(&[vec!["Cinder".to_string(), "Paris".to_string()]]).unwrap()).unwrap();
        let csv = crate::textconv::apply(std::slice::from_ref(&sheet), OutputFormat::Csv).unwrap();
        let csv_text = std::fs::read_to_string(dir.join(&csv)).unwrap();
        assert!(csv_text.contains("Cinder"), "{csv_text}");
        let ods = convert(std::slice::from_ref(&sheet), OutputFormat::Ods).unwrap();
        assert!(sheet_contains(&dir.join(&ods), "Cinder"));
        let table_pdf = convert(&[sheet], OutputFormat::Pdf).unwrap();
        let table_text = pdf_extract::extract_text(dir.join(table_pdf)).unwrap();
        assert!(table_text.contains("Cinder"), "{table_text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn html_becomes_docx_and_pdf() {
        let dir = scratch();
        let source = dir.join("page.html");
        std::fs::write(&source, "<p>Bonjour la page</p>").unwrap();
        let docx = convert(std::slice::from_ref(&source), OutputFormat::Docx).unwrap();
        assert!(zip_contains(&dir.join(&docx), "Bonjour la page"));
        let pdf = convert(&[source], OutputFormat::Pdf).unwrap();
        let text = pdf_extract::extract_text(dir.join(pdf)).unwrap();
        assert!(text.contains("Bonjour la page"), "{text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pdf_text_roundtrip_and_split() {
        let dir = scratch();
        let source = dir.join("page.pdf");
        std::fs::write(&source, text_pdf(&["Page une".to_string(), "Page deux assez longue".to_string()])).unwrap();
        let loaded = Document::load(&source).expect("pdf lisible");
        assert_eq!(loaded.get_pages().len(), 1);
        let text = pdf_extract::extract_text(&source).unwrap();
        assert!(text.contains("Page une"), "{text}");
        let message = split(&[source]).unwrap();
        assert_eq!(message, "page-pages");
        assert!(dir.join("page-pages").join("page-1.pdf").is_file());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn pdf_becomes_docx_epub_slides_sheet_and_png() {
        let dir = scratch();
        let source = dir.join("page.pdf");
        std::fs::write(&source, text_pdf(&["Page une du contrat".to_string()])).unwrap();
        let docx = convert(std::slice::from_ref(&source), OutputFormat::Docx).unwrap();
        assert!(zip_contains(&dir.join(&docx), "Page une du contrat"));
        let epub = convert(std::slice::from_ref(&source), OutputFormat::Epub).unwrap();
        assert!(zip_contains(&dir.join(&epub), "Page une du contrat"));
        let pptx = convert(std::slice::from_ref(&source), OutputFormat::Pptx).unwrap();
        assert!(zip_contains(&dir.join(&pptx), "Page une du contrat"));
        let xlsx = convert(std::slice::from_ref(&source), OutputFormat::Xlsx).unwrap();
        assert!(sheet_contains(&dir.join(&xlsx), "Page une du contrat"));
        let png = convert(std::slice::from_ref(&source), OutputFormat::Png).unwrap();
        let bytes = std::fs::read(dir.join(&png)).unwrap();
        assert!(bytes.starts_with(b"\x89PNG"), "rendu: {png}");
        let back = convert(&[dir.join(&epub)], OutputFormat::Pdf).unwrap();
        let text = pdf_extract::extract_text(dir.join(back)).unwrap();
        assert!(text.contains("Page une du contrat"), "{text}");
        let deck = dir.join("deck.pptx");
        std::fs::write(&deck, pptx_bytes(&["Bonjour les diapositives".to_string()]).unwrap()).unwrap();
        let slides = convert(&[deck], OutputFormat::Pdf).unwrap();
        let slide_text = pdf_extract::extract_text(dir.join(slides)).unwrap();
        assert!(slide_text.contains("Bonjour les diapositives"), "{slide_text}");
        std::fs::write(dir.join("table.csv"), "nom,ville\nCinder,Paris\n").unwrap();
        let ods = convert(&[dir.join("table.csv")], OutputFormat::Ods).unwrap();
        assert!(sheet_contains(&dir.join(&ods), "Cinder"));
        let table_pdf = convert(&[dir.join("table.csv")], OutputFormat::Pdf).unwrap();
        let table_text = pdf_extract::extract_text(dir.join(table_pdf)).unwrap();
        assert!(table_text.contains("Cinder"), "{table_text}");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn zip_contains(path: &Path, needle: &str) -> bool {
        let file = File::open(path).unwrap();
        let mut archive = ZipArchive::new(file).unwrap();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            let mut text = String::new();
            if entry.read_to_string(&mut text).is_ok() && text.contains(needle) {
                return true;
            }
        }
        false
    }

    fn sheet_contains(path: &Path, needle: &str) -> bool {
        use calamine::{open_workbook_auto, Reader};
        let mut book = open_workbook_auto(path).unwrap();
        let range = book.worksheet_range_at(0).unwrap().unwrap();
        range.rows().any(|row| row.iter().any(|cell| cell.to_string().contains(needle)))
    }
}
