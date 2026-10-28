use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SECONDS_PER_FILE: u64 = 120;
static LOCK: Mutex<()> = Mutex::new(());

pub fn record_files(count: usize) {
    if count == 0 {
        return;
    }
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    remember(&store_path(), count);
}

pub fn saved_minutes() -> u64 {
    let _guard = LOCK.lock().unwrap_or_else(|error| error.into_inner());
    read_seconds(&store_path()) / 60
}

pub fn format_minutes(minutes: u64) -> String {
    let digits = minutes.to_string();
    let mut grouped = String::new();
    for (index, character) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push('\u{202f}');
        }
        grouped.push(character);
    }
    grouped.chars().rev().collect()
}

fn remember(path: &Path, files: usize) {
    let total = read_seconds(path).saturating_add(SECONDS_PER_FILE.saturating_mul(files as u64));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, total.to_string());
}

fn read_seconds(path: &Path) -> u64 {
    std::fs::read_to_string(path).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0)
}

fn store_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("Cinder").join("minutes.txt")
}

#[cfg(test)]
mod tests {
    use super::{format_minutes, read_seconds, remember};

    #[test]
    fn two_files_add_four_minutes() {
        let path = std::env::temp_dir().join(format!("cinder-minutes-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        remember(&path, 2);
        assert_eq!(read_seconds(&path) / 60, 4);
        remember(&path, 1);
        assert_eq!(read_seconds(&path) / 60, 6);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn minutes_are_grouped_by_thousands() {
        assert_eq!(format_minutes(12), "12");
        assert_eq!(format_minutes(1200), "1\u{202f}200");
    }
}
