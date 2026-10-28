use std::path::{Path, PathBuf};
use std::process::Command;

use crate::catalog::OutputFormat;
use crate::convert::available_path;

#[derive(Clone, Copy)]
pub enum MediaOp {
    Convert(OutputFormat),
    CompressVideo,
    Mute,
    Snapshot,
    CompressAudio,
    Normalize,
}

pub fn apply(paths: &[PathBuf], operation: MediaOp) -> Result<String, String> {
    ensure_ffmpeg()?;
    let mut written = Vec::new();
    let mut failure = None;
    for path in paths {
        match apply_one(path, operation) {
            Ok(destination) => written.push(destination),
            Err(error) => failure = Some(error),
        }
    }
    report(&written, failure)
}

fn apply_one(path: &Path, operation: MediaOp) -> Result<PathBuf, String> {
    let parent = path.parent().ok_or_else(|| "Dossier introuvable".to_string())?;
    let stem = file_stem(path);
    match operation {
        MediaOp::Convert(format) => convert_to(path, parent, &stem, format),
        MediaOp::CompressVideo => {
            let destination = available_path(parent, &format!("{stem}-compresse"), "mp4");
            run(
                path,
                &destination,
                &[
                    "-c:v", "libx264", "-preset", "veryfast", "-crf", "28", "-c:a", "aac", "-b:a", "128k",
                ],
            )?;
            Ok(destination)
        }
        MediaOp::Mute => mute(path, parent, &stem),
        MediaOp::Snapshot => {
            let destination = available_path(parent, &format!("{stem}-capture"), "jpg");
            run(path, &destination, &["-frames:v", "1", "-q:v", "3"])?;
            Ok(destination)
        }
        MediaOp::CompressAudio => {
            let destination = available_path(parent, &format!("{stem}-compresse"), "m4a");
            run(path, &destination, &["-vn", "-c:a", "aac", "-b:a", "96k"])?;
            Ok(destination)
        }
        MediaOp::Normalize => {
            let destination = available_path(parent, &format!("{stem}-normalise"), "m4a");
            run(
                path,
                &destination,
                &["-vn", "-af", "loudnorm=I=-16:TP=-1.5:LRA=11", "-c:a", "aac", "-b:a", "160k"],
            )?;
            Ok(destination)
        }
    }
}

fn convert_to(path: &Path, parent: &Path, stem: &str, format: OutputFormat) -> Result<PathBuf, String> {
    let destination = available_path(parent, stem, format.extension());
    let args: &[&str] = match format {
        OutputFormat::Mp4 | OutputFormat::Mkv => &["-c:v", "libx264", "-preset", "veryfast", "-crf", "23", "-c:a", "aac", "-b:a", "160k"],
        OutputFormat::Mp3 => &["-vn", "-c:a", "libmp3lame", "-q:a", "4"],
        OutputFormat::Webm => &["-c:v", "libvpx", "-b:v", "1M", "-c:a", "libvorbis"],
        OutputFormat::Wmv => &["-c:v", "wmv2", "-c:a", "wmav2"],
        OutputFormat::Avi => &["-c:v", "mpeg4", "-q:v", "5", "-c:a", "libmp3lame"],
        OutputFormat::Gif => &["-vf", "fps=10,scale=480:-1:flags=lanczos"],
        OutputFormat::M4a => &["-vn", "-c:a", "aac", "-b:a", "160k"],
        OutputFormat::Wma => &["-vn", "-c:a", "wmav2"],
        OutputFormat::Wav => &["-vn", "-c:a", "pcm_s16le"],
        OutputFormat::Aiff => &["-vn", "-c:a", "pcm_s16be"],
        OutputFormat::Flac => &["-vn", "-c:a", "flac"],
        OutputFormat::Opus => &["-vn", "-c:a", "libopus", "-b:a", "96k"],
        OutputFormat::Ogg => &["-vn", "-c:a", "libvorbis"],
        _ => return Err("Format inattendu".to_string()),
    };
    run(path, &destination, args)?;
    Ok(destination)
}

fn mute(path: &Path, parent: &Path, stem: &str) -> Result<PathBuf, String> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("mp4");
    let destination = available_path(parent, &format!("{stem}-muet"), extension);
    if run(path, &destination, &["-c", "copy", "-an"]).is_ok() {
        return Ok(destination);
    }
    let _ = std::fs::remove_file(&destination);
    let destination = available_path(parent, &format!("{stem}-muet"), "mp4");
    run(path, &destination, &["-an", "-c:v", "libx264", "-preset", "veryfast", "-crf", "23"])?;
    Ok(destination)
}

fn run(source: &Path, destination: &Path, args: &[&str]) -> Result<(), String> {
    if source == destination {
        return Err("Refus d'écraser l'original".to_string());
    }
    let mut command = Command::new("ffmpeg");
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    command.arg("-i").arg(source);
    command.args(args);
    command.arg("-y").arg(destination);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().map_err(|_| "ffmpeg est introuvable".to_string())?;
    if output.status.success() && destination.is_file() {
        return Ok(());
    }
    let _ = std::fs::remove_file(destination);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr.lines().rev().find(|line| !line.trim().is_empty()).unwrap_or("ffmpeg a échoué");
    Err(truncate(line))
}

fn ensure_ffmpeg() -> Result<(), String> {
    let mut command = Command::new("ffmpeg");
    command.arg("-version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    match command.output() {
        Ok(output) if output.status.success() => Ok(()),
        _ => Err("ffmpeg est introuvable".to_string()),
    }
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

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fichier".to_string())
}

fn truncate(text: &str) -> String {
    let mut short = text.trim().to_string();
    if short.chars().count() > 180 {
        short = short.chars().take(180).collect();
    }
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ffmpeg_ready() -> bool {
        ensure_ffmpeg().is_ok()
    }

    #[test]
    fn mp4_converts_to_mp3_when_ffmpeg_is_present() {
        if !ffmpeg_ready() {
            return;
        }
        let tick = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("cinder-media-{tick}"));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("tone.mp4");
        let mut command = Command::new("ffmpeg");
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=16x16:d=0.3",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=0.3",
            "-shortest",
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
            source.to_str().unwrap(),
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let created = command.output().unwrap();
        assert!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));
        let message = apply(&[source.clone()], MediaOp::Convert(OutputFormat::Mp3)).unwrap();
        assert_eq!(message, "tone.mp3");
        assert!(dir.join("tone.mp3").is_file());
        assert!(source.is_file());
        let _ = std::fs::remove_dir_all(dir);
    }
}
