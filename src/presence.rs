use std::path::{Path, PathBuf};

use tray_icon::Icon;
use windows::core::{Interface, PCWSTR, PWSTR, HSTRING};
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, IPersistFile, CLSCTX_ALL};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_QUERY_VALUE, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_SZ,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHChangeNotify, SHGetKnownFolderPath, FOLDERID_Desktop, FOLDERID_Programs, KNOWN_FOLDER_FLAG,
    SHCNE_ASSOCCHANGED, SHCNE_CREATE, SHCNE_DELETE, SHCNE_UPDATEITEM, SHCNF_FLUSH, SHCNF_FLUSHNOWAIT, SHCNF_IDLIST,
    SHCNF_PATHW, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowExW, FindWindowW, PostMessageW, ASFW_ANY, HWND_MESSAGE,
};

use crate::drag::{log_line, WM_APP_ACTIVATE};

const TILE: [u8; 3] = [247, 243, 236];
const RING: [u8; 3] = [42, 32, 28];
const EMBER_HOT: [u8; 3] = [255, 210, 164];
const EMBER_DEEP: [u8; 3] = [255, 86, 34];
const QUIET: [u8; 3] = [176, 168, 160];

pub fn mark_icon(active: bool) -> Icon {
    let size = 32;
    let pixels = mark_pixels(size, active);
    Icon::from_rgba(pixels, size, size).unwrap_or_else(|_| Icon::from_rgba(vec![224, 122, 61, 255], 1, 1).unwrap())
}

pub fn install_shortcuts() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(dir) = data_dir() else {
        return;
    };
    let icon = dir.join("cinder-mark.ico");
    if let Err(error) = write_icon_file(&icon) {
        log_line(&format!("icône: {error}"));
        return;
    }
    let Some(working) = exe.parent().map(Path::to_path_buf) else {
        return;
    };
    for folder in [FOLDERID_Desktop, FOLDERID_Programs] {
        match write_shortcut(&folder, &exe, &working, &icon) {
            Ok(path) => log_line(&format!("raccourci {}", path.display())),
            Err(error) => log_line(&format!("raccourci: {error}")),
        }
    }
    refresh_desktop_icons();
}

fn refresh_desktop_icons() {
    unsafe {
        let progman = FindWindowW(windows::core::w!("Progman"), PCWSTR::null()).unwrap_or_default();
        redraw_desktop(progman);
        let mut worker = HWND::default();
        loop {
            worker = FindWindowExW(None, Some(worker), windows::core::w!("WorkerW"), PCWSTR::null()).unwrap_or_default();
            if worker.0.is_null() {
                break;
            }
            redraw_desktop(worker);
        }
    }
}

fn redraw_desktop(parent: HWND) {
    unsafe {
        if parent.0.is_null() {
            return;
        }
        let Ok(defview) = FindWindowExW(Some(parent), None, windows::core::w!("SHELLDLL_DefView"), PCWSTR::null()) else {
            return;
        };
        let Ok(list) = FindWindowExW(Some(defview), None, windows::core::w!("SysListView32"), PCWSTR::null()) else {
            return;
        };
        let _ = InvalidateRect(Some(list), None, true);
        let _ = UpdateWindow(list);
    }
}

pub const TRAY_GUID: u128 = 0x4e8c_1d27_6a5b_4f90_b3c2_91d7_e0a4_c8f6;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TraySpot {
    Waiting,
    Already,
    Updated,
}

pub fn place_tray_icon() -> TraySpot {
    unsafe { place_inner() }
}

pub fn wake_running() -> bool {
    unsafe {
        let hwnd = FindWindowExW(Some(HWND_MESSAGE), None, windows::core::w!("CinderHost"), PCWSTR::null())
            .unwrap_or_default();
        if hwnd.0.is_null() {
            return false;
        }
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        PostMessageW(Some(hwnd), WM_APP_ACTIVATE, WPARAM(0), LPARAM(0)).is_ok()
    }
}

fn data_dir() -> Option<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    let dir = PathBuf::from(base).join("Cinder");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

pub enum Startup {
    Run,
    Stop(i32),
}

pub fn prepare_startup() -> Startup {
    let silent = std::env::args().skip(1).any(|arg| is_quiet_switch(&arg));
    let Ok(current) = std::env::current_exe() else {
        return Startup::Run;
    };
    let Some(dir) = data_dir() else {
        return Startup::Run;
    };
    let dest = dir.join("cinder.exe");
    if paths_match(&current, &dest) {
        if silent {
            install_shortcuts();
            return Startup::Stop(0);
        }
        return Startup::Run;
    }
    if !silent && wake_running() {
        return Startup::Stop(0);
    }
    if let Err(error) = std::fs::copy(&current, &dest) {
        log_line(&format!("installation: {error}"));
        if wake_running() {
            return Startup::Stop(0);
        }
        return Startup::Run;
    }
    if silent {
        let code = std::process::Command::new(&dest)
            .arg("/quiet")
            .status()
            .ok()
            .and_then(|status| status.code())
            .unwrap_or(1);
        return Startup::Stop(code);
    }
    match std::process::Command::new(&dest).spawn() {
        Ok(_) => Startup::Stop(0),
        Err(error) => {
            log_line(&format!("installation: {error}"));
            Startup::Run
        }
    }
}

fn is_quiet_switch(arg: &str) -> bool {
    matches!(arg.to_ascii_lowercase().as_str(), "/quiet" | "/s" | "/silent" | "--quiet" | "/qn")
}

fn paths_match(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn write_icon_file(path: &Path) -> Result<(), String> {
    let images: Vec<Vec<u8>> = [16u32, 32, 48, 64, 256].into_iter().map(bmp_icon_image).collect();
    let mut file = Vec::new();
    file.extend_from_slice(&0u16.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + images.len() * 16;
    for image in &images {
        let size = icon_edge(image);
        file.push(if size >= 256 { 0 } else { size as u8 });
        file.push(if size >= 256 { 0 } else { size as u8 });
        file.extend_from_slice(&[0, 0]);
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&32u16.to_le_bytes());
        file.extend_from_slice(&(image.len() as u32).to_le_bytes());
        file.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in &images {
        file.extend_from_slice(image);
    }
    std::fs::write(path, file).map_err(|error| error.to_string())
}

fn icon_edge(image: &[u8]) -> u32 {
    u32::from_le_bytes(image[4..8].try_into().unwrap_or([0, 0, 0, 0]))
}

fn bmp_icon_image(size: u32) -> Vec<u8> {
    let pixels = mark_pixels(size, true);
    let mask_row = ((size + 31) / 32) * 4;
    let mut bytes = Vec::with_capacity(40 + (size * size * 4 + mask_row * size) as usize);
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(size as i32).to_le_bytes());
    bytes.extend_from_slice(&((size as i32) * 2).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&32u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(size * size * 4).to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for y in (0..size).rev() {
        for x in 0..size {
            let index = ((y * size + x) * 4) as usize;
            bytes.push(pixels[index + 2]);
            bytes.push(pixels[index + 1]);
            bytes.push(pixels[index]);
            bytes.push(pixels[index + 3]);
        }
    }
    bytes.resize(bytes.len() + (mask_row * size) as usize, 0);
    bytes
}

fn write_shortcut(folder: &windows::core::GUID, exe: &Path, working: &Path, icon: &Path) -> Result<PathBuf, String> {
    let directory = known_folder(folder).ok_or_else(|| "Dossier introuvable".to_string())?;
    let link_path = directory.join("Cinder.lnk");
    let _ = std::fs::remove_file(&link_path);
    unsafe {
        let mut removed: Vec<u16> = link_path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        SHChangeNotify(SHCNE_DELETE, SHCNF_PATHW | SHCNF_FLUSH, Some(removed.as_mut_ptr().cast()), None);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL).map_err(|error| error.to_string())?;
        let target = HSTRING::from(exe.to_string_lossy().as_ref());
        let work = HSTRING::from(working.to_string_lossy().as_ref());
        let icon_path = HSTRING::from(icon.to_string_lossy().as_ref());
        let description = HSTRING::from("Cinder");
        link.SetPath(&target).map_err(|error| error.to_string())?;
        link.SetWorkingDirectory(&work).map_err(|error| error.to_string())?;
        link.SetDescription(&description).map_err(|error| error.to_string())?;
        link.SetIconLocation(&icon_path, 0).map_err(|error| error.to_string())?;
        let file: IPersistFile = link.cast().map_err(|error| error.to_string())?;
        let stored = HSTRING::from(link_path.to_string_lossy().as_ref());
        file.Save(&stored, true).map_err(|error| error.to_string())?;
        let mut wide: Vec<u16> = link_path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let ptr = wide.as_mut_ptr().cast();
        SHChangeNotify(SHCNE_CREATE, SHCNF_PATHW | SHCNF_FLUSH, Some(ptr), None);
        SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_PATHW | SHCNF_FLUSH, Some(ptr), None);
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST | SHCNF_FLUSHNOWAIT, None, None);
    }
    Ok(link_path)
}

fn known_folder(id: &windows::core::GUID) -> Option<PathBuf> {
    unsafe {
        let wide = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let mut len = 0usize;
        while *wide.0.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(wide.0, len));
        CoTaskMemFree(Some(wide.0.cast()));
        Some(PathBuf::from(text))
    }
}

unsafe fn place_inner() -> TraySpot {
    let mut root = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, windows::core::w!("Control Panel\\NotifyIconSettings"), None, KEY_READ, &mut root)
        .is_err()
    {
        return TraySpot::Waiting;
    }
    let mut spot = TraySpot::Waiting;
    for index in 0..256u32 {
        let mut name = [0u16; 260];
        let mut len = name.len() as u32;
        let status = RegEnumKeyExW(root, index, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None);
        if status == ERROR_NO_MORE_ITEMS || status.is_err() {
            break;
        }
        let mut sub = HKEY::default();
        if RegOpenKeyExW(root, PWSTR(name.as_mut_ptr()), None, KEY_QUERY_VALUE | KEY_SET_VALUE, &mut sub).is_err() {
            continue;
        }
        let guid = reg_sz(sub, windows::core::w!("IconGuid")).unwrap_or_default();
        if same_tray_guid(&guid) {
            spot = if is_promoted(sub) {
                TraySpot::Already
            } else {
                let one = 1u32.to_le_bytes();
                if RegSetValueExW(sub, windows::core::w!("IsPromoted"), None, REG_DWORD, Some(&one)).is_ok() {
                    TraySpot::Updated
                } else {
                    TraySpot::Waiting
                }
            };
            let _ = RegCloseKey(sub);
            break;
        }
        let _ = RegCloseKey(sub);
    }
    let _ = RegCloseKey(root);
    spot
}

unsafe fn reg_sz(key: HKEY, name: windows::core::PCWSTR) -> Option<String> {
    let mut bytes = vec![0u8; 512];
    let mut size = bytes.len() as u32;
    let mut kind = REG_SZ;
    if RegQueryValueExW(key, name, None, Some(&mut kind), Some(bytes.as_mut_ptr()), Some(&mut size)).is_err() {
        return None;
    }
    let chars = (size as usize / 2).saturating_sub(1);
    let wide = std::slice::from_raw_parts(bytes.as_ptr() as *const u16, chars.min(bytes.len() / 2));
    Some(String::from_utf16_lossy(wide).trim_end_matches('\0').to_string())
}

unsafe fn is_promoted(key: HKEY) -> bool {
    let mut data = [0u8; 4];
    let mut size = 4u32;
    let mut kind = REG_DWORD;
    if RegQueryValueExW(
        key,
        windows::core::w!("IsPromoted"),
        None,
        Some(&mut kind),
        Some(data.as_mut_ptr()),
        Some(&mut size),
    )
    .is_err()
    {
        return false;
    }
    u32::from_le_bytes(data) == 1
}

fn same_tray_guid(value: &str) -> bool {
    let expected = format!("{:?}", windows::core::GUID::from_u128(TRAY_GUID));
    value
        .trim()
        .trim_matches(|symbol| symbol == '{' || symbol == '}')
        .eq_ignore_ascii_case(&expected)
}

pub(crate) fn mark_pixels(size: u32, active: bool) -> Vec<u8> {
    let scale = if size <= 48 { 3 } else { 1 };
    let drawn = draw_mark(size * scale, active);
    if scale == 1 {
        drawn
    } else {
        downsample(&drawn, size, scale)
    }
}

fn draw_mark(size: u32, active: bool) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) / 2.0;
    let half = size as f32 * 0.5 - 0.4;
    let corner = size as f32 * 0.27;
    let outer = size as f32 * 0.355;
    let inner = size as f32 * 0.155;
    let start = 0.02_f32;
    let end = 2.45_f32;
    let seam = end + (std::f32::consts::TAU - (end - start)) * 0.5;
    let (hot, deep) = if active { (EMBER_HOT, EMBER_DEEP) } else { (QUIET, QUIET) };
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let offset = ((y * size + x) * 4) as usize;
            let pixel = &mut pixels[offset..offset + 4];
            paint(pixel, TILE, cover(-rounded_box(dx, dy, half, corner)));
            if distance < outer + 1.0 && distance > inner - 1.0 {
                let mut angle = dx.atan2(-dy);
                if angle < 0.0 {
                    angle += std::f32::consts::TAU;
                }
                let ring = cover(outer - distance).min(cover(distance - inner));
                paint(pixel, RING, ring);
                paint(pixel, TILE, ring * spoke(angle, seam, distance));
                let inside = (angle - start).min(end - angle) * distance.max(1.0);
                let heat = ((distance - inner) / (outer - inner)).clamp(0.0, 1.0);
                paint(pixel, mix(hot, deep, heat), ring * cover(inside));
            }
        }
    }
    pixels
}

fn rounded_box(dx: f32, dy: f32, half: f32, radius: f32) -> f32 {
    let qx = dx.abs() - half + radius;
    let qy = dy.abs() - half + radius;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    outside + inside - radius
}

fn spoke(angle: f32, seam: f32, distance: f32) -> f32 {
    let mut delta = (angle - seam).abs();
    if delta > std::f32::consts::PI {
        delta = std::f32::consts::TAU - delta;
    }
    cover(1.25 - delta * distance.max(1.0))
}

fn downsample(source: &[u8], size: u32, scale: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let wide = size * scale;
    let samples = scale * scale;
    for y in 0..size {
        for x in 0..size {
            let mut color = [0u32; 3];
            let mut alpha = 0u32;
            for oy in 0..scale {
                for ox in 0..scale {
                    let index = (((y * scale + oy) * wide + x * scale + ox) * 4) as usize;
                    let sample = source[index + 3] as u32;
                    alpha += sample;
                    for channel in 0..3 {
                        color[channel] += source[index + channel] as u32 * sample;
                    }
                }
            }
            let offset = ((y * size + x) * 4) as usize;
            pixels[offset + 3] = (alpha / samples) as u8;
            if alpha > 0 {
                for channel in 0..3 {
                    pixels[offset + channel] = (color[channel] / alpha) as u8;
                }
            }
        }
    }
    pixels
}

fn mix(from: [u8; 3], to: [u8; 3], t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let mut color = [0u8; 3];
    for channel in 0..3 {
        color[channel] = (from[channel] as f32 * (1.0 - t) + to[channel] as f32 * t).round() as u8;
    }
    color
}

fn cover(inside: f32) -> f32 {
    (inside + 0.5).clamp(0.0, 1.0)
}

fn paint(pixel: &mut [u8], color: [u8; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let alpha = alpha.clamp(0.0, 1.0);
    for channel in 0..3 {
        pixel[channel] = (pixel[channel] as f32 * (1.0 - alpha) + color[channel] as f32 * alpha).round() as u8;
    }
    pixel[3] = (pixel[3] as f32 * (1.0 - alpha) + 255.0 * alpha).round() as u8;
}

#[cfg(test)]
mod tests {
    use super::{is_quiet_switch, mark_pixels, same_tray_guid, write_icon_file};

    #[test]
    fn quiet_switches_cover_the_store_installer() {
        assert!(is_quiet_switch("/quiet"));
        assert!(is_quiet_switch("/S"));
        assert!(is_quiet_switch("--quiet"));
        assert!(!is_quiet_switch("/help"));
    }

    #[test]
    fn active_icon_is_the_wheel_with_an_ember_slice() {
        let on = mark_pixels(32, true);
        let off = mark_pixels(32, false);
        assert!(on.chunks(4).any(|pixel| pixel[0] > 240 && pixel[1] < 120 && pixel[2] < 80 && pixel[3] > 200));
        let center = ((16 * 32 + 16) * 4) as usize;
        assert!(on[center] > 220 && on[center + 3] > 200);
        assert!(on.chunks(4).any(|pixel| pixel[0] < 60 && pixel[1] < 50 && pixel[3] > 200));
        assert!(off.chunks(4).all(|pixel| pixel[3] < 80 || pixel[0] < 230 || pixel[1] > 140));
    }

    #[test]
    fn tray_guid_text_matches_the_icon_identity() {
        assert!(same_tray_guid("{4E8C1D27-6A5B-4F90-B3C2-91D7E0A4C8F6}"));
        assert!(!same_tray_guid("{00000000-0000-0000-0000-000000000000}"));
    }

    #[test]
    fn icon_file_is_a_real_ico() {
        let path = std::env::temp_dir().join(format!("cinder-icon-{}.ico", std::process::id()));
        write_icon_file(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() > 100, "{}", bytes.len());
        assert_eq!(&bytes[..4], &[0, 0, 1, 0]);
        let _ = std::fs::remove_file(path);
    }
}
