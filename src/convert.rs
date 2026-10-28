use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView, ImageFormat};

const MAX_SNAPSHOT_BYTES: u64 = 48 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatKind {
    Jpeg,
    Png,
    WebP,
    Bmp,
    Gif,
    Tiff,
    Pdf,
}

impl FormatKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
            Self::Bmp => "bmp",
            Self::Gif => "gif",
            Self::Tiff => "tiff",
            Self::Pdf => "pdf",
        }
    }

}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Apply {
    Convert(FormatKind),
    Compress,
    Resize(u32),
    Rotate,
}

#[derive(Clone, Debug)]
pub struct Source {
    pub path: PathBuf,
    pub bytes: Option<Vec<u8>>,
}

pub fn format_of_path(path: &Path) -> Option<FormatKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some(FormatKind::Jpeg),
        "png" => Some(FormatKind::Png),
        "webp" => Some(FormatKind::WebP),
        "bmp" => Some(FormatKind::Bmp),
        "gif" => Some(FormatKind::Gif),
        "tif" | "tiff" => Some(FormatKind::Tiff),
        _ => None,
    }
}

pub fn load_source(path: &Path) -> Source {
    let bytes = std::fs::metadata(path).ok().and_then(|meta| {
        if meta.is_file() && meta.len() <= MAX_SNAPSHOT_BYTES {
            std::fs::read(path).ok()
        } else {
            None
        }
    });
    Source {
        path: path.to_path_buf(),
        bytes,
    }
}

pub fn available_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let named = |suffix: Option<u32>| -> PathBuf {
        let file_name = match (suffix, ext.is_empty()) {
            (None, true) => stem.to_string(),
            (None, false) => format!("{stem}.{ext}"),
            (Some(index), true) => format!("{stem}-{index}"),
            (Some(index), false) => format!("{stem}-{index}.{ext}"),
        };
        dir.join(file_name)
    };

    let first = named(None);
    if !first.exists() {
        return first;
    }
    for index in 2..10_000 {
        let path = named(Some(index));
        if !path.exists() {
            return path;
        }
    }
    named(Some(10_000))
}

pub fn apply_all(sources: &[Source], operation: &Apply) -> Result<String, String> {
    let mut written = Vec::new();
    let mut failure = None;
    for source in sources {
        if format_of_path(&source.path).is_none() {
            continue;
        }
        match apply_one(source, operation) {
            Ok(path) => written.push(path),
            Err(error) => failure = Some(error),
        }
    }
    if written.is_empty() {
        return Err(failure.unwrap_or_else(|| "Aucune image à convertir".to_string()));
    }
    if written.len() == 1 {
        let name = written[0]
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "fichier".to_string());
        return Ok(name);
    }
    Ok(format!("{} fichiers enregistrés", written.len()))
}

fn apply_one(source: &Source, operation: &Apply) -> Result<PathBuf, String> {
    let source_format = format_of_path(&source.path).ok_or_else(|| "Pas une image".to_string())?;
    if let Apply::Convert(target) = operation {
        if *target == source_format {
            return Err("Déjà dans ce format".to_string());
        }
    }

    let bytes = match &source.bytes {
        Some(bytes) => bytes.clone(),
        None => std::fs::read(&source.path).map_err(|_| "Fichier introuvable".to_string())?,
    };
    let image = image::load_from_memory(&bytes).map_err(|_| "Impossible de lire l'image".to_string())?;
    let parent = source
        .path
        .parent()
        .ok_or_else(|| "Dossier introuvable".to_string())?;
    let stem = source
        .path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fichier".to_string());

    let (output_format, file_stem, encoded) = match operation {
        Apply::Convert(target) => (*target, stem, encode(&image, *target, 90)?),
        Apply::Compress => (
            FormatKind::Jpeg,
            format!("{stem}-compresse"),
            encode(&image, FormatKind::Jpeg, 70)?,
        ),
        Apply::Resize(max_edge) => {
            let resized = resize_image(image, *max_edge)?;
            (
                source_format,
                format!("{stem}-{max_edge}"),
                encode(&resized, source_format, 90)?,
            )
        }
        Apply::Rotate => {
            let rotated = DynamicImage::ImageRgba8(image::imageops::rotate90(&image.to_rgba8()));
            (
                source_format,
                format!("{stem}-pivote"),
                encode(&rotated, source_format, 90)?,
            )
        }
    };

    let destination = available_path(parent, &file_stem, output_format.extension());
    std::fs::write(&destination, encoded).map_err(|_| "Impossible d'écrire le fichier".to_string())?;
    Ok(destination)
}

fn resize_image(image: DynamicImage, max_edge: u32) -> Result<DynamicImage, String> {
    let (width, height) = image.dimensions();
    let longest = width.max(height);
    if longest <= max_edge {
        return Err("Déjà plus petit".to_string());
    }
    let scale = max_edge as f32 / longest as f32;
    let next_width = ((width as f32) * scale).round().max(1.0) as u32;
    let next_height = ((height as f32) * scale).round().max(1.0) as u32;
    Ok(image.resize(next_width, next_height, image::imageops::FilterType::Lanczos3))
}

fn encode(image: &DynamicImage, format: FormatKind, jpeg_quality: u8) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(Vec::new());
    match format {
        FormatKind::Jpeg => {
            let rgb = image.to_rgb8();
            let mut encoder = JpegEncoder::new_with_quality(&mut cursor, jpeg_quality);
            encoder
                .encode_image(&rgb)
                .map_err(|_| "Impossible d'encoder le JPEG".to_string())?;
        }
        FormatKind::Png => {
            image
                .to_rgba8()
                .write_to(&mut cursor, ImageFormat::Png)
                .map_err(|_| "Impossible d'encoder le PNG".to_string())?;
        }
        FormatKind::WebP => {
            image
                .to_rgba8()
                .write_to(&mut cursor, ImageFormat::WebP)
                .map_err(|_| "Impossible d'encoder le WebP".to_string())?;
        }
        FormatKind::Bmp => {
            image
                .to_rgb8()
                .write_to(&mut cursor, ImageFormat::Bmp)
                .map_err(|_| "Impossible d'encoder le BMP".to_string())?;
        }
        FormatKind::Gif => {
            image
                .to_rgb8()
                .write_to(&mut cursor, ImageFormat::Gif)
                .map_err(|_| "Impossible d'encoder le GIF".to_string())?;
        }
        FormatKind::Tiff => {
            image
                .to_rgba8()
                .write_to(&mut cursor, ImageFormat::Tiff)
                .map_err(|_| "Impossible d'encoder le TIFF".to_string())?;
        }
        FormatKind::Pdf => return pdf_with_jpeg(image),
    }
    Ok(cursor.into_inner())
}

fn pdf_with_jpeg(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut jpeg_cursor = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_cursor, 90);
    encoder
        .encode_image(&rgb)
        .map_err(|_| "Impossible d'encoder le JPEG".to_string())?;
    let jpeg = jpeg_cursor.into_inner();
    let content = format!("q\n{width} 0 0 {height} 0 0 cm\n/Im Do\nQ\n");
    let objects = [
        format!("<< /Type /Catalog /Pages 2 0 R >>").into_bytes(),
        format!("<< /Type /Pages /Count 1 /Kids [3 0 R] >>").into_bytes(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Contents 4 0 R /Resources << /XObject << /Im 5 0 R >> >> >>"
        )
        .into_bytes(),
        pdf_stream(format!("<< /Length {} >>", content.len()).into_bytes(), content.as_bytes()),
        pdf_stream(
            format!(
                "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>",
                jpeg.len()
            )
            .into_bytes(),
            &jpeg,
        ),
    ];

    let mut document = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
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
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes(),
    );
    Ok(document)
}

fn pdf_stream(dictionary: Vec<u8>, data: &[u8]) -> Vec<u8> {
    let mut object = dictionary;
    object.extend_from_slice(b"\nstream\n");
    object.extend_from_slice(data);
    object.extend_from_slice(b"\nendstream");
    object
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;
    use image::GenericImageView;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch_dir() -> PathBuf {
        let tick = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("cinder-{tick}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_skip_existing_files() {
        let dir = scratch_dir();
        std::fs::write(dir.join("photo.jpg"), b"x").unwrap();
        std::fs::write(dir.join("photo-2.jpg"), b"x").unwrap();
        let path = available_path(&dir, "photo", "jpg");
        assert_eq!(path.file_name().unwrap(), "photo-3.jpg");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn png_converts_to_jpeg_beside_original() {
        let dir = scratch_dir();
        let source_path = dir.join("photo.png");
        RgbImage::from_pixel(2, 2, image::Rgb([12, 40, 80]))
            .save(&source_path)
            .unwrap();
        let source = load_source(&source_path);
        let message = apply_all(&[source], &Apply::Convert(FormatKind::Jpeg)).unwrap();
        assert_eq!(message, "photo.jpg");
        let output = dir.join("photo.jpg");
        assert!(output.is_file());
        assert_ne!(std::fs::read(&source_path).unwrap(), std::fs::read(&output).unwrap());
        let decoded = image::open(&output).unwrap();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert!(std::fs::read(&output).unwrap().starts_with(&[0xFF, 0xD8]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn second_jpeg_does_not_overwrite_the_first() {
        let dir = scratch_dir();
        let source_path = dir.join("photo.png");
        RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3]))
            .save(&source_path)
            .unwrap();
        std::fs::write(dir.join("photo.jpg"), b"already").unwrap();
        let source = load_source(&source_path);
        let message = apply_all(&[source], &Apply::Convert(FormatKind::Jpeg)).unwrap();
        assert_eq!(message, "photo-2.jpg");
        assert_eq!(std::fs::read(dir.join("photo.jpg")).unwrap(), b"already");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rotate_keeps_png_and_swaps_the_sides() {
        let dir = scratch_dir();
        let source_path = dir.join("photo.png");
        RgbImage::from_pixel(2, 3, image::Rgb([9, 9, 9]))
            .save(&source_path)
            .unwrap();
        let message = apply_all(&[load_source(&source_path)], &Apply::Rotate).unwrap();
        assert_eq!(message, "photo-pivote.png");
        assert_eq!(image::open(dir.join("photo-pivote.png")).unwrap().dimensions(), (3, 2));
        assert!(source_path.is_file());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn compress_writes_a_jpeg_copy() {
        let dir = scratch_dir();
        let source_path = dir.join("photo.png");
        RgbImage::from_pixel(2, 2, image::Rgb([200, 10, 10]))
            .save(&source_path)
            .unwrap();
        let message = apply_all(&[load_source(&source_path)], &Apply::Compress).unwrap();
        assert_eq!(message, "photo-compresse.jpg");
        assert!(std::fs::read(dir.join("photo-compresse.jpg")).unwrap().starts_with(&[0xFF, 0xD8]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn png_converts_to_a_one_page_pdf() {
        let dir = scratch_dir();
        let source_path = dir.join("photo.png");
        RgbImage::from_pixel(4, 2, image::Rgb([20, 30, 40]))
            .save(&source_path)
            .unwrap();
        let message = apply_all(&[load_source(&source_path)], &Apply::Convert(FormatKind::Pdf)).unwrap();
        assert_eq!(message, "photo.pdf");
        let pdf = std::fs::read(dir.join("photo.pdf")).unwrap();
        assert!(pdf.starts_with(b"%PDF-"));
        assert!(pdf.windows(4).any(|bytes| bytes == [0xFF, 0xD8, 0xFF, 0xE0] || bytes == [0xFF, 0xD8, 0xFF, 0xDB]));
        assert!(source_path.is_file());
        let _ = std::fs::remove_dir_all(dir);
    }
}
