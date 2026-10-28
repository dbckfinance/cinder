use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelKind {
    Convert = 0,
    Tools = 1,
    Actions = 2,
}

impl WheelKind {
    pub fn from_code(code: usize) -> Option<Self> {
        match code {
            0 => Some(Self::Convert),
            1 => Some(Self::Tools),
            2 => Some(Self::Actions),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Image,
    Video,
    Audio,
    Subtitle,
    Archive,
    Spreadsheet,
    Text,
    Epub,
    Pdf,
    Document,
    Slides,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputFormat {
    Jpeg,
    Png,
    WebP,
    Bmp,
    Gif,
    Tiff,
    Pdf,
    Mp4,
    Mp3,
    Mkv,
    Webm,
    Wmv,
    Avi,
    M4a,
    Wma,
    Wav,
    Aiff,
    Flac,
    Opus,
    Ogg,
    Vtt,
    Srt,
    Txt,
    Md,
    Html,
    Csv,
    Tsv,
    Tar,
    Gzip,
    Docx,
    Epub,
    Pptx,
    Xlsx,
    Ods,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
            Self::Bmp => "bmp",
            Self::Gif => "gif",
            Self::Tiff => "tiff",
            Self::Pdf => "pdf",
            Self::Mp4 => "mp4",
            Self::Mp3 => "mp3",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Wmv => "wmv",
            Self::Avi => "avi",
            Self::M4a => "m4a",
            Self::Wma => "wma",
            Self::Wav => "wav",
            Self::Aiff => "aiff",
            Self::Flac => "flac",
            Self::Opus => "opus",
            Self::Ogg => "ogg",
            Self::Vtt => "vtt",
            Self::Srt => "srt",
            Self::Txt => "txt",
            Self::Md => "md",
            Self::Html => "html",
            Self::Csv => "csv",
            Self::Tsv => "tsv",
            Self::Tar => "tar",
            Self::Gzip => "tar.gz",
            Self::Docx => "docx",
            Self::Epub => "epub",
            Self::Pptx => "pptx",
            Self::Xlsx => "xlsx",
            Self::Ods => "ods",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPG",
            Self::Png => "PNG",
            Self::WebP => "WebP",
            Self::Bmp => "BMP",
            Self::Gif => "GIF",
            Self::Tiff => "TIFF",
            Self::Pdf => "PDF",
            Self::Mp4 => "MP4",
            Self::Mp3 => "MP3",
            Self::Mkv => "MKV",
            Self::Webm => "WEBM",
            Self::Wmv => "WMV",
            Self::Avi => "AVI",
            Self::M4a => "M4A",
            Self::Wma => "WMA",
            Self::Wav => "WAV",
            Self::Aiff => "AIFF",
            Self::Flac => "FLAC",
            Self::Opus => "OPUS",
            Self::Ogg => "OGG",
            Self::Vtt => "VTT",
            Self::Srt => "SRT",
            Self::Txt => "TXT",
            Self::Md => "MD",
            Self::Html => "HTML",
            Self::Csv => "CSV",
            Self::Tsv => "TSV",
            Self::Tar => "TAR",
            Self::Gzip => "GZIP",
            Self::Docx => "DOCX",
            Self::Epub => "EPUB",
            Self::Pptx => "PPTX",
            Self::Xlsx => "XLSX",
            Self::Ods => "ODS",
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Self::Jpeg => "Convertir en JPG",
            Self::Png => "Convertir en PNG",
            Self::WebP => "Convertir en WebP",
            Self::Bmp => "Convertir en BMP",
            Self::Gif => "Convertir en GIF",
            Self::Tiff => "Convertir en TIFF",
            Self::Pdf => "Convertir en PDF",
            Self::Mp4 => "Convertir en MP4",
            Self::Mp3 => "Convertir en MP3",
            Self::Mkv => "Convertir en MKV",
            Self::Webm => "Convertir en WebM",
            Self::Wmv => "Convertir en WMV",
            Self::Avi => "Convertir en AVI",
            Self::M4a => "Convertir en M4A",
            Self::Wma => "Convertir en WMA",
            Self::Wav => "Convertir en WAV",
            Self::Aiff => "Convertir en AIFF",
            Self::Flac => "Convertir en FLAC",
            Self::Opus => "Convertir en Opus",
            Self::Ogg => "Convertir en OGG",
            Self::Vtt => "Convertir en VTT",
            Self::Srt => "Convertir en SRT",
            Self::Txt => "Convertir en TXT",
            Self::Md => "Convertir en Markdown",
            Self::Html => "Convertir en HTML",
            Self::Csv => "Convertir en CSV",
            Self::Tsv => "Convertir en TSV",
            Self::Tar => "Convertir en TAR",
            Self::Gzip => "Convertir en GZIP",
            Self::Docx => "Convertir en DOCX",
            Self::Epub => "Convertir en EPUB",
            Self::Pptx => "Convertir en PPTX",
            Self::Xlsx => "Convertir en XLSX",
            Self::Ods => "Convertir en ODS",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Convert(OutputFormat),
    Compress,
    Resize(u32),
    Rotate,
    Mute,
    Snapshot,
    Normalize,
    Extract,
    Split,
    Metadata,
    CopyPath,
    Reveal,
    Terminal,
    CopyTo,
    MoveTo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WheelItem {
    pub command: Command,
    pub label: &'static str,
    pub detail: &'static str,
}

pub fn category_of(path: &Path) -> Option<Category> {
    let ext = extension(path);
    let name = file_name(path);
    if name.ends_with(".tar.gz") || matches!(ext.as_str(), "zip" | "tar" | "gz" | "tgz") {
        return Some(Category::Archive);
    }
    Some(match ext.as_str() {
        "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" | "tif" | "tiff" => Category::Image,
        "mp4" | "m4v" | "mkv" | "webm" | "avi" | "wmv" | "mov" => Category::Video,
        "m4a" | "mp3" | "wav" | "flac" | "ogg" | "opus" | "aiff" | "aif" | "wma" | "aac" => Category::Audio,
        "vtt" | "srt" => Category::Subtitle,
        "csv" | "tsv" | "xlsx" | "xls" | "ods" => Category::Spreadsheet,
        "txt" | "md" | "html" | "htm" => Category::Text,
        "epub" => Category::Epub,
        "pdf" => Category::Pdf,
        "docx" | "odt" | "rtf" => Category::Document,
        "pptx" | "ppt" | "odp" => Category::Slides,
        _ => return None,
    })
}

pub fn items_for(kind: WheelKind, paths: &[impl AsRef<Path>]) -> (String, Vec<WheelItem>) {
    match kind {
        WheelKind::Convert => convert_items(paths),
        WheelKind::Tools => tool_items(paths),
        WheelKind::Actions => (
            "Actions".to_string(),
            vec![
                item(Command::CopyPath, "Chemin", "Copier le chemin"),
                item(Command::Reveal, "Explorateur", "Afficher dans l'Explorateur"),
                item(Command::Terminal, "Terminal", "Ouvrir le terminal ici"),
                item(Command::CopyTo, "Copier", "Copier vers un dossier"),
                item(Command::MoveTo, "Déplacer", "Déplacer vers un dossier"),
            ],
        ),
    }
}

fn convert_items(paths: &[impl AsRef<Path>]) -> (String, Vec<WheelItem>) {
    let category = paths.first().and_then(|path| category_of(path.as_ref())).unwrap_or(Category::Image);
    if paths.is_empty() {
        return ("Convertir".to_string(), format_items(Category::Image, None));
    }
    if category_of(paths[0].as_ref()).is_none() {
        return ("Pas encore".to_string(), Vec::new());
    }
    let current = paths.first().map(|path| path.as_ref());
    ("Convertir".to_string(), format_items(category, current))
}

fn format_items(category: Category, current: Option<&Path>) -> Vec<WheelItem> {
    targets(category)
        .iter()
        .copied()
        .filter(|format| current.is_none_or(|path| !same_format(path, *format)))
        .map(|format| item(Command::Convert(format), format.label(), format.detail()))
        .collect()
}

fn targets(category: Category) -> &'static [OutputFormat] {
    use OutputFormat::*;
    match category {
        Category::Image => &[Jpeg, Png, WebP, Bmp, Gif, Tiff, Pdf],
        Category::Video => &[Mp4, Mp3, Mkv, Gif, Webm, Wmv, Avi],
        Category::Audio => &[M4a, Wma, Wav, Aiff, Flac, Opus, Ogg],
        Category::Subtitle => &[Vtt, Srt, Txt],
        Category::Archive => &[Tar, Gzip],
        Category::Spreadsheet => &[Ods, Csv, Tsv, Pdf],
        Category::Text => &[Txt, Md, Html, Pdf, Docx, Epub],
        Category::Epub => &[Html, Txt, Md, Pdf, Docx],
        Category::Pdf => &[Txt, Md, Html, Docx, Epub, Png, Jpeg, Pptx, Xlsx],
        Category::Document => &[Txt, Md, Html, Pdf, Docx, Epub],
        Category::Slides => &[Pdf, Jpeg, Png],
    }
}

fn tool_items(paths: &[impl AsRef<Path>]) -> (String, Vec<WheelItem>) {
    let category = if paths.is_empty() {
        Category::Image
    } else if let Some(category) = category_of(paths[0].as_ref()) {
        category
    } else {
        return ("Aucun outil".to_string(), Vec::new());
    };
    let mut items = match category {
        Category::Image => vec![
            item(Command::Compress, "Compresser", "JPEG qualité 70"),
            item(Command::Resize(1920), "1920 px", "Réduire à 1920 px"),
            item(Command::Resize(1280), "1280 px", "Réduire à 1280 px"),
            item(Command::Rotate, "Pivoter", "Pivoter de 90°"),
        ],
        Category::Video => vec![
            item(Command::Compress, "Compresser", "Vidéo plus légère"),
            item(Command::Mute, "Muet", "Retirer le son"),
            item(Command::Snapshot, "Capture", "Enregistrer une image"),
        ],
        Category::Audio => vec![
            item(Command::Compress, "Compresser", "Audio plus léger"),
            item(Command::Normalize, "Normaliser", "Niveau sonore uniforme"),
        ],
        Category::Archive => vec![item(Command::Extract, "Extraire", "Extraire l'archive")],
        Category::Pdf => vec![
            item(Command::Compress, "Compresser", "Réduire le PDF"),
            item(Command::Split, "Séparer", "Une page par fichier"),
            item(Command::Metadata, "Métadonnées", "Écrire les informations"),
        ],
        Category::Document => vec![
            item(Command::Compress, "Compresser", "Réduire le document"),
            item(Command::Metadata, "Métadonnées", "Écrire les informations"),
        ],
        _ => Vec::new(),
    };
    let had_tools = !items.is_empty();
    let current = paths.first().map(|path| path.as_ref());
    for format_item in format_items(category, current) {
        if items.iter().all(|existing| existing.command != format_item.command) {
            items.push(format_item);
        }
    }
    if items.is_empty() {
        ("Aucun outil".to_string(), items)
    } else if had_tools {
        ("Outils".to_string(), items)
    } else {
        ("Convertir".to_string(), items)
    }
}

fn same_format(path: &Path, format: OutputFormat) -> bool {
    let ext = extension(path);
    let name = file_name(path);
    match format {
        OutputFormat::Jpeg => ext == "jpg" || ext == "jpeg",
        OutputFormat::Tiff => ext == "tif" || ext == "tiff",
        OutputFormat::Html => ext == "html" || ext == "htm",
        OutputFormat::Aiff => ext == "aiff" || ext == "aif",
        OutputFormat::Gzip => ext == "gz" || ext == "tgz" || name.ends_with(".tar.gz"),
        OutputFormat::Tar => ext == "tar" || ext == "tgz" || name.ends_with(".tar.gz"),
        _ => ext == format.extension(),
    }
}

fn extension(path: &Path) -> String {
    path.extension().and_then(|ext| ext.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn file_name(path: &Path) -> String {
    path.file_name().and_then(|name| name.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn item(command: Command, label: &'static str, detail: &'static str) -> WheelItem {
    WheelItem { command, label, detail }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn image_tools_also_offer_the_other_formats() {
        let (_title, items) = items_for(WheelKind::Tools, &[PathBuf::from("photo.png")]);
        assert!(items.iter().any(|item| item.command == Command::Compress));
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Jpeg)));
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::WebP)));
        assert!(items.iter().all(|item| item.command != Command::Convert(OutputFormat::Png)));
    }

    #[test]
    fn jpeg_wheel_offers_the_other_formats() {
        let (_title, items) = items_for(WheelKind::Convert, &[PathBuf::from("photo.jpg")]);
        assert!(items.iter().all(|item| item.command != Command::Convert(OutputFormat::Jpeg)));
        assert!(items.iter().all(|item| item.command != Command::Convert(OutputFormat::Mp4)));
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Png)));
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Pdf)));
        assert_eq!(items.len(), 6);
    }

    #[test]
    fn an_empty_selection_still_shows_the_formats() {
        let (title, items) = items_for(WheelKind::Convert, &[] as &[PathBuf]);
        assert_eq!(title, "Convertir");
        assert_eq!(items.len(), 7);
    }

    #[test]
    fn text_files_convert_to_markdown_and_html() {
        let (title, items) = items_for(WheelKind::Convert, &[PathBuf::from("notes.txt")]);
        assert_eq!(title, "Convertir");
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Md)));
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Html)));
        assert!(items.iter().all(|item| item.command != Command::Convert(OutputFormat::Mp4)));
    }

    #[test]
    fn video_wheel_does_not_offer_image_formats() {
        let (_title, items) = items_for(WheelKind::Convert, &[PathBuf::from("film.mp4")]);
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Mp3)));
        assert!(items.iter().all(|item| item.command != Command::Convert(OutputFormat::Png)));
        assert!(items.iter().all(|item| item.command != Command::Convert(OutputFormat::Mp4)));
    }

    #[test]
    fn zip_tools_offer_extract() {
        let (_title, items) = items_for(WheelKind::Tools, &[PathBuf::from("dossier.zip")]);
        assert!(items.iter().any(|item| item.command == Command::Extract));
    }

    #[test]
    fn subtitles_offer_txt() {
        let (_title, items) = items_for(WheelKind::Convert, &[PathBuf::from("dialogue.vtt")]);
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Txt)));
        assert!(items.iter().all(|item| item.command != Command::Convert(OutputFormat::Vtt)));
    }

    #[test]
    fn a_docx_converts_and_a_pdf_has_tools() {
        let (_title, items) = items_for(WheelKind::Convert, &[PathBuf::from("contrat.docx")]);
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Pdf)));
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Txt)));
        assert!(items.iter().all(|item| item.command != Command::Convert(OutputFormat::Docx)));
        let (_title, tools) = items_for(WheelKind::Tools, &[PathBuf::from("contrat.pdf")]);
        assert!(tools.iter().any(|item| item.command == Command::Compress));
        assert!(tools.iter().any(|item| item.command == Command::Split));
        assert!(tools.iter().any(|item| item.command == Command::Convert(OutputFormat::Docx)));
        assert!(tools.iter().any(|item| item.command == Command::Convert(OutputFormat::Png)));
        let (_title, doc_tools) = items_for(WheelKind::Tools, &[PathBuf::from("contrat.docx")]);
        assert!(doc_tools.iter().any(|item| item.command == Command::Metadata));
        assert!(doc_tools.iter().any(|item| item.command == Command::Convert(OutputFormat::Pdf)));
        let (_title, html) = items_for(WheelKind::Convert, &[PathBuf::from("page.html")]);
        assert!(html.iter().any(|item| item.command == Command::Convert(OutputFormat::Pdf)));
        assert!(html.iter().any(|item| item.command == Command::Convert(OutputFormat::Docx)));
        assert!(html.iter().all(|item| item.command != Command::Convert(OutputFormat::Html)));
    }

    #[test]
    fn a_pdf_offers_docx_png_and_epub_but_not_pages() {
        let (_title, items) = items_for(WheelKind::Convert, &[PathBuf::from("rapport.pdf")]);
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Docx)));
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Png)));
        assert!(items.iter().any(|item| item.command == Command::Convert(OutputFormat::Epub)));
        assert!(items.iter().all(|item| item.label != "PAGES"));
        let (_title, slides) = items_for(WheelKind::Convert, &[PathBuf::from("deck.pptx")]);
        assert!(slides.iter().any(|item| item.command == Command::Convert(OutputFormat::Pdf)));
        assert!(slides.iter().any(|item| item.command == Command::Convert(OutputFormat::Jpeg)));
        assert!(slides.iter().any(|item| item.command == Command::Convert(OutputFormat::Png)));
        assert_eq!(slides.len(), 3);
        let (_title, excel) = items_for(WheelKind::Convert, &[PathBuf::from("classeur.xlsx")]);
        assert!(excel.iter().any(|item| item.command == Command::Convert(OutputFormat::Ods)));
        assert!(excel.iter().any(|item| item.command == Command::Convert(OutputFormat::Csv)));
        assert!(excel.iter().any(|item| item.command == Command::Convert(OutputFormat::Pdf)));
        assert!(excel.iter().all(|item| item.command != Command::Convert(OutputFormat::Xlsx)));
        let (_title, sheet) = items_for(WheelKind::Convert, &[PathBuf::from("table.csv")]);
        assert!(sheet.iter().any(|item| item.command == Command::Convert(OutputFormat::Ods)));
        assert!(sheet.iter().any(|item| item.command == Command::Convert(OutputFormat::Pdf)));
    }

    #[test]
    fn actions_exist_for_any_file() {
        let (_title, items) = items_for(WheelKind::Actions, &[PathBuf::from("notes.txt")]);
        assert_eq!(items.len(), 5);
    }
}
