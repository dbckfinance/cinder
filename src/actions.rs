#![allow(unsafe_op_in_unsafe_fn)]

use std::path::{Path, PathBuf};
use std::process::Command;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Shell::{
    FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
};

use crate::convert::available_path;

unsafe extern "system" {
    fn GlobalFree(hmem: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
}

pub fn copy_text(text: &str) -> Result<(), String> {
    unsafe {
        OpenClipboard(None).map_err(|error| error.to_string())?;
        let result = write_clipboard(text);
        let _ = CloseClipboard();
        result
    }
}

unsafe fn write_clipboard(text: &str) -> Result<(), String> {
    EmptyClipboard().map_err(|error| error.to_string())?;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();
    let memory = GlobalAlloc(GMEM_MOVEABLE, bytes).map_err(|error| error.to_string())?;
    let locked = GlobalLock(memory);
    if locked.is_null() {
        GlobalFree(memory.0);
        return Err("Presse-papiers indisponible".to_string());
    }
    std::ptr::copy_nonoverlapping(wide.as_ptr(), locked.cast(), wide.len());
    let _ = GlobalUnlock(memory);
    match SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0))) {
        Ok(_) => Ok(()),
        Err(error) => {
            GlobalFree(memory.0);
            Err(error.to_string())
        }
    }
}

pub fn reveal(path: &Path) -> Result<(), String> {
    Command::new("explorer.exe")
        .arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn open_terminal(path: &Path) -> Result<(), String> {
    let directory = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };
    let directory = directory.to_string_lossy().to_string();
    if Command::new("wt.exe").args(["-d", &directory]).spawn().is_ok() {
        return Ok(());
    }
    Command::new("cmd.exe")
        .args(["/C", "start", "", "cmd", "/K", &format!("cd /d {directory}")])
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub fn pick_folder(title: &str) -> Result<Option<PathBuf>, String> {
    unsafe {
        let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| error.to_string())?;
        let mut options = dialog.GetOptions().map_err(|error| error.to_string())?;
        options |= FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM;
        dialog.SetOptions(options).map_err(|error| error.to_string())?;
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        dialog
            .SetTitle(PCWSTR(wide.as_ptr()))
            .map_err(|error| error.to_string())?;
        if dialog.Show(None).is_err() {
            return Ok(None);
        }
        let item = dialog.GetResult().map_err(|error| error.to_string())?;
        let name: PWSTR = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(|error| error.to_string())?;
        let text = name.to_string().unwrap_or_default();
        CoTaskMemFree(Some(name.0.cast()));
        if text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(PathBuf::from(text)))
        }
    }
}

pub fn copy_into(paths: &[PathBuf], destination: &Path) -> Result<String, String> {
    let mut copied = 0usize;
    for source in paths {
        if !source.is_file() {
            continue;
        }
        let target = destination_for(source, destination);
        std::fs::copy(source, &target).map_err(|_| format!("Impossible de copier {}", source.display()))?;
        copied += 1;
    }
    if copied == 0 {
        return Err("Aucun fichier à copier".to_string());
    }
    if copied == 1 {
        Ok("Fichier copié".to_string())
    } else {
        Ok(format!("{copied} fichiers copiés"))
    }
}

pub fn move_into(paths: &[PathBuf], destination: &Path) -> Result<String, String> {
    let mut moved = 0usize;
    let mut already_there = true;
    for source in paths {
        if !source.exists() {
            continue;
        }
        if source.parent() == Some(destination) {
            continue;
        }
        already_there = false;
        let target = destination_for(source, destination);
        move_path(source, &target)?;
        moved += 1;
    }
    if moved == 0 {
        if already_there {
            return Err("Déjà dans ce dossier".to_string());
        }
        return Err("Aucun fichier à déplacer".to_string());
    }
    if moved == 1 {
        Ok("Fichier déplacé".to_string())
    } else {
        Ok(format!("{moved} fichiers déplacés"))
    }
}

fn destination_for(source: &Path, destination: &Path) -> PathBuf {
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fichier".to_string());
    let extension = source
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_default();
    available_path(destination, &stem, &extension)
}

#[cfg(test)]
mod tests {
    use super::{copy_into, move_into};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn copy_and_move_leave_a_new_name_when_the_target_exists() {
        let tick = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("cinder-move-{tick}"));
        let source_dir = root.join("source");
        let dest_dir = root.join("dest");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&dest_dir).unwrap();
        let source = source_dir.join("photo.png");
        fs::write(&source, b"png").unwrap();
        fs::write(dest_dir.join("photo.png"), b"old").unwrap();

        assert_eq!(copy_into(std::slice::from_ref(&source), &dest_dir).unwrap(), "Fichier copié");
        assert_eq!(fs::read(dest_dir.join("photo-2.png")).unwrap(), b"png");
        assert!(source.is_file());

        assert_eq!(move_into(std::slice::from_ref(&source), &dest_dir).unwrap(), "Fichier déplacé");
        assert!(!source.exists());
        assert_eq!(fs::read(dest_dir.join("photo-3.png")).unwrap(), b"png");
        let _ = fs::remove_dir_all(root);
    }
}

fn move_path(source: &Path, target: &Path) -> Result<(), String> {
    if std::fs::rename(source, target).is_ok() {
        return Ok(());
    }
    if source.is_dir() {
        return Err("Impossible de déplacer le dossier".to_string());
    }
    std::fs::copy(source, target).map_err(|_| "Impossible de déplacer le fichier".to_string())?;
    std::fs::remove_file(source).map_err(|_| "Le fichier a été copié, pas déplacé".to_string())?;
    Ok(())
}
