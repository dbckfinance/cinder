#![allow(unsafe_op_in_unsafe_fn)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
#[cfg(test)]
use std::sync::Mutex;

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{
    CoCreateInstance, CoTaskMemFree, IServiceProvider, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Com::CoInitializeEx;
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, UIA_CONTROLTYPE_ID, UIA_ListItemControlTypeId,
};
use windows::Win32::UI::Shell::{
    IFolderView, IShellBrowser, IShellItemArray, IShellView, IShellWindows, SHGetNameFromIDList, SWC_DESKTOP,
    SWFO_NEEDDISPATCH, SVGIO_ALLVIEW, SVGIO_SELECTION, SIGDN_FILESYSPATH, ShellWindows,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, FindWindowExW, FindWindowW, GetClassNameW, GetCursorPos, GetForegroundWindow, GetSystemMetrics,
    EnumChildWindows, GetWindowRect, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, GA_ROOT, GetAncestor, HHOOK,
    MSLLHOOKSTRUCT,
    SM_CXDRAG, SM_CYDRAG,
    WH_MOUSE_LL, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
};

use crate::overlay::hit_index;

pub const WM_APP_DRAG_START: u32 = WM_APP + 1;
pub const WM_APP_DROP: u32 = WM_APP + 2;
pub const WM_APP_OPEN: u32 = WM_APP + 3;
pub const WM_APP_HOVER: u32 = WM_APP + 4;
pub const WM_APP_CANCEL: u32 = WM_APP + 5;
pub const WM_APP_TOAST: u32 = WM_APP + 6;
pub const WM_APP_ACTIVATE: u32 = WM_APP + 7;
pub const WM_APP_STATS: u32 = WM_APP + 8;
pub const WM_APP_TOGGLE: u32 = WM_APP + 9;

pub static PAUSED: AtomicBool = AtomicBool::new(false);
pub static DRAG_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static WHEEL_OPEN: AtomicBool = AtomicBool::new(false);
pub static KEY_WHEEL: AtomicBool = AtomicBool::new(false);
pub const SHELL_NONE: u8 = 0;
pub const SHELL_DESKTOP: u8 = 1;
pub const SHELL_EXPLORER: u8 = 2;
pub static SHELL_KEY: AtomicU8 = AtomicU8::new(SHELL_NONE);
pub static CURSOR_X: AtomicI32 = AtomicI32::new(0);
pub static CURSOR_Y: AtomicI32 = AtomicI32::new(0);
pub static HOVER: AtomicI32 = AtomicI32::new(-1);
pub static WHEEL_CX: AtomicI32 = AtomicI32::new(0);
pub static WHEEL_CY: AtomicI32 = AtomicI32::new(0);
pub static WHEEL_DEAD: AtomicU32 = AtomicU32::new(0);
pub static WHEEL_OUTER: AtomicU32 = AtomicU32::new(0);
pub static WHEEL_COUNT: AtomicUsize = AtomicUsize::new(0);

static HOST: AtomicIsize = AtomicIsize::new(0);
static MOUSE_HOOK: AtomicIsize = AtomicIsize::new(0);
static HOOK_CALLS: AtomicU64 = AtomicU64::new(0);
static LAST_CALLS: AtomicU64 = AtomicU64::new(0);
static LAST_X: AtomicI32 = AtomicI32::new(0);
static LAST_Y: AtomicI32 = AtomicI32::new(0);
static WATCH_READY: AtomicBool = AtomicBool::new(false);
static HOOK_LOGGED: AtomicBool = AtomicBool::new(false);
static STALE_TICKS: AtomicU32 = AtomicU32::new(0);
static TRACKING: AtomicBool = AtomicBool::new(false);
static ARMED: AtomicBool = AtomicBool::new(false);
static START_X: AtomicI32 = AtomicI32::new(0);
static START_Y: AtomicI32 = AtomicI32::new(0);

pub fn set_host(hwnd: HWND) {
    HOST.store(hwnd.0 as isize, Ordering::SeqCst);
}

pub fn host() -> HWND {
    HWND(HOST.load(Ordering::SeqCst) as *mut core::ffi::c_void)
}

pub fn cancel_gesture() {
    TRACKING.store(false, Ordering::SeqCst);
    ARMED.store(false, Ordering::SeqCst);
    DRAG_ACTIVE.store(false, Ordering::SeqCst);
    clear_wheel_flag();
}

pub fn post(message: u32, wparam: usize, lparam: isize) {
    let raw = HOST.load(Ordering::SeqCst);
    if raw == 0 {
        return;
    }
    unsafe {
        let hwnd = HWND(raw as *mut core::ffi::c_void);
        let _ = PostMessageW(Some(hwnd), message, WPARAM(wparam), LPARAM(lparam));
    }
}

pub fn button_down() -> bool {
    unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 }
}

pub fn install_mouse_hook() -> Result<HHOOK, String> {
    let hook = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0).map_err(|error| error.to_string())? };
    MOUSE_HOOK.store(hook.0 as isize, Ordering::SeqCst);
    Ok(hook)
}

pub fn remove_mouse_hook() {
    let previous = MOUSE_HOOK.swap(0, Ordering::SeqCst);
    if previous != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(previous as *mut core::ffi::c_void));
        }
    }
}

pub fn note_hook_call() {
    HOOK_CALLS.fetch_add(1, Ordering::Relaxed);
}

pub fn gesture_busy() -> bool {
    TRACKING.load(Ordering::Relaxed) || DRAG_ACTIVE.load(Ordering::Relaxed) || WHEEL_OPEN.load(Ordering::Relaxed)
}

/// Windows retire un crochet bas niveau sans prévenir si le fil tarde.
/// On le repose quand le curseur bouge et que le crochet n’a rien vu.
pub fn ensure_hooks_fresh(reinstall_keyboard: fn()) {
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) }.is_err() {
        return;
    }
    let calls = HOOK_CALLS.load(Ordering::Relaxed);
    if calls > 0 && !HOOK_LOGGED.swap(true, Ordering::Relaxed) {
        log_line("hook ok");
    }
    if !WATCH_READY.swap(true, Ordering::Relaxed) {
        LAST_X.store(point.x, Ordering::Relaxed);
        LAST_Y.store(point.y, Ordering::Relaxed);
        LAST_CALLS.store(calls, Ordering::Relaxed);
        return;
    }
    let moved = point.x != LAST_X.swap(point.x, Ordering::Relaxed) || point.y != LAST_Y.swap(point.y, Ordering::Relaxed);
    let previous = LAST_CALLS.swap(calls, Ordering::Relaxed);
    if moved && previous == calls && !gesture_busy() {
        let stale = STALE_TICKS.fetch_add(1, Ordering::Relaxed) + 1;
        if stale >= 3 {
            STALE_TICKS.store(0, Ordering::Relaxed);
            remove_mouse_hook();
            if install_mouse_hook().is_ok() {
                reinstall_keyboard();
                log_line("crochets réinstallés");
            }
        }
    } else {
        STALE_TICKS.store(0, Ordering::Relaxed);
    }
}

pub fn ensure_com() -> Result<(), String> {
    let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if result.is_err() {
        return Err(result.to_string());
    }
    Ok(())
}

pub fn explorer_selection() -> Vec<PathBuf> {
    unsafe { explorer_selection_inner().unwrap_or_default() }
}

pub enum ExplorerHit {
    Files(Vec<PathBuf>),
    Ignore,
}

pub fn explorer_paths() -> ExplorerHit {
    unsafe { explorer_paths_inner() }
}

unsafe fn explorer_selection_inner() -> Result<Vec<PathBuf>, windows::core::Error> {
    let Some(view) = foreground_shell_view()? else {
        return Ok(Vec::new());
    };
    let items: IShellItemArray = view.GetItemObject(SVGIO_SELECTION)?;
    Ok(paths_from_items(&items))
}

unsafe fn explorer_paths_inner() -> ExplorerHit {
    let mut point = POINT::default();
    if GetCursorPos(&mut point).is_err() {
        return ExplorerHit::Ignore;
    }
    let Ok(Some(view)) = foreground_shell_view() else {
        return ExplorerHit::Ignore;
    };
    let folder: IFolderView = match view.cast() {
        Ok(folder) => folder,
        Err(_) => return ExplorerHit::Ignore,
    };
    match row_at_point(point) {
        RowAt::Item { index, name } => {
            let path = index.and_then(|index| path_at(&folder, index)).or_else(|| path_named(&folder, &name));
            match path {
                Some(path) => ExplorerHit::Files(vec![path]),
                None => ExplorerHit::Files(Vec::new()),
            }
        }
        RowAt::List => ExplorerHit::Files(explorer_selection()),
        RowAt::Outside => ExplorerHit::Ignore,
    }
}

unsafe fn foreground_shell_view() -> Result<Option<IShellView>, windows::core::Error> {
    let foreground = GetForegroundWindow();
    let tab = FindWindowExW(Some(foreground), None, w!("ShellTabWindowClass"), PCWSTR::null()).unwrap_or_default();
    let shell_windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
    let count = shell_windows.Count().unwrap_or(0);
    for index in 0..count {
        let item = shell_windows.Item(&variant_i4(index))?;
        let provider: IServiceProvider = item.cast()?;
        let browser: IShellBrowser = provider.QueryService(&IShellBrowser::IID)?;
        let window = browser.GetWindow()?;
        let root = GetAncestor(window, GA_ROOT);
        if window != foreground && window != tab && root != foreground && GetAncestor(foreground, GA_ROOT) != window {
            continue;
        }
        return Ok(Some(browser.QueryActiveShellView()?));
    }
    Ok(None)
}

enum RowAt {
    Item { index: Option<i32>, name: String },
    List,
    Outside,
}

unsafe fn row_at_point(point: POINT) -> RowAt {
    let automation: IUIAutomation = match CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL) {
        Ok(automation) => automation,
        Err(_) => return if pointer_in_explorer(point) { RowAt::List } else { RowAt::Outside },
    };
    let Ok(element) = automation.ElementFromPoint(point) else {
        return if pointer_in_explorer(point) { RowAt::List } else { RowAt::Outside };
    };
    let Ok(walker) = automation.RawViewWalker() else {
        return RowAt::Outside;
    };
    let mut current = Some(element);
    let mut index = None;
    let mut name = String::new();
    let mut seen_item = false;
    for _ in 0..16 {
        let Some(element) = current else {
            break;
        };
        let class = bstr_text(element.CurrentClassName());
        let kind = element.CurrentControlType().unwrap_or(UIA_CONTROLTYPE_ID(0));
        if !seen_item && (class == "UIItem" || kind == UIA_ListItemControlTypeId) {
            seen_item = true;
            let id = bstr_text(element.CurrentAutomationId());
            index = id.parse::<i32>().ok().filter(|value| *value >= 0);
            name = bstr_text(element.CurrentName());
        }
        if class == "UIItemsView" || class == "DUIListView" || class == "SysListView32" || class == "SHELLDLL_DefView" {
            if seen_item {
                return RowAt::Item { index, name };
            }
            return RowAt::List;
        }
        current = walker.GetParentElement(&element).ok();
    }
    RowAt::Outside
}

fn bstr_text(value: windows::core::Result<windows::core::BSTR>) -> String {
    value.ok().map(|text| text.to_string()).unwrap_or_default()
}

unsafe fn path_at(view: &IFolderView, index: i32) -> Option<PathBuf> {
    let pidl = view.Item(index).ok()?;
    let named = SHGetNameFromIDList(pidl, SIGDN_FILESYSPATH);
    CoTaskMemFree(Some(pidl.cast()));
    let name = named.ok()?;
    let text = name.to_string().unwrap_or_default();
    CoTaskMemFree(Some(name.0.cast()));
    if text.is_empty() { None } else { Some(PathBuf::from(text)) }
}

unsafe fn path_named(view: &IFolderView, label: &str) -> Option<PathBuf> {
    if label.is_empty() {
        return None;
    }
    let count = view.ItemCount(SVGIO_ALLVIEW).ok()?.clamp(0, 2000);
    let mut stems = Vec::new();
    for index in 0..count {
        let Some(path) = path_at(view, index) else {
            continue;
        };
        let file_name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if file_name.eq_ignore_ascii_case(label) {
            return Some(path);
        }
        let stem = path.file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if stem.eq_ignore_ascii_case(label) {
            stems.push(path);
        }
    }
    if stems.len() == 1 { stems.pop() } else { None }
}

unsafe fn paths_from_items(items: &IShellItemArray) -> Vec<PathBuf> {
    let count = items.GetCount().unwrap_or(0).min(24);
    let mut paths = Vec::new();
    for index in 0..count {
        let Ok(item) = items.GetItemAt(index) else {
            continue;
        };
        let Ok(name) = item.GetDisplayName(SIGDN_FILESYSPATH) else {
            continue;
        };
        let text = name.to_string().unwrap_or_default();
        CoTaskMemFree(Some(name.0.cast()));
        if text.is_empty() {
            continue;
        }
        let path = PathBuf::from(text);
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

unsafe fn variant_i4(value: i32) -> VARIANT {
    let mut variant = VARIANT::default();
    (*variant.Anonymous.Anonymous).vt = VT_I4;
    (*variant.Anonymous.Anonymous).Anonymous.lVal = value;
    variant
}

pub fn log_line(message: &str) {
    let Ok(base) = std::env::var("LOCALAPPDATA") else {
        return;
    };
    let directory = std::path::PathBuf::from(base).join("Cinder");
    let _ = std::fs::create_dir_all(&directory);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("cinder.log"))
    else {
        return;
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    use std::io::Write;
    let _ = writeln!(file, "{stamp} {message}");
}

fn pointer_in_explorer(point: POINT) -> bool {
    unsafe {
        let foreground = GetForegroundWindow();
        if !is_cabinet(foreground) {
            return false;
        }
        let mut probe = PaneProbe { point, found: false };
        let param = LPARAM(&mut probe as *mut PaneProbe as isize);
        let _ = EnumChildWindows(Some(foreground), Some(enum_file_pane), param);
        probe.found
    }
}

struct PaneProbe {
    point: POINT,
    found: bool,
}

unsafe extern "system" fn enum_file_pane(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
    let probe = &mut *(lparam.0 as *mut PaneProbe);
    let class = class_name(hwnd);
    if matches!(
        class.as_str(),
        "SysListView32" | "DirectUIHWND" | "SHELLDLL_DefView" | "DUIListView" | "UIItemsView" | "Microsoft.UI.Content.DesktopChildSiteBridge"
    ) {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_ok()
            && probe.point.x >= rect.left
            && probe.point.x < rect.right
            && probe.point.y >= rect.top
            && probe.point.y < rect.bottom
        {
            probe.found = true;
            return windows::core::BOOL(0);
        }
    }
    windows::core::BOOL(1)
}

fn is_cabinet(hwnd: HWND) -> bool {
    let class = class_name(hwnd);
    if class == "CabinetWClass" || class == "ExploreWClass" {
        return true;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let root_class = class_name(root);
    root_class == "CabinetWClass" || root_class == "ExploreWClass"
}

fn class_name(hwnd: HWND) -> String {
    if hwnd.0.is_null() {
        return String::new();
    }
    let mut buffer = [0u16; 256];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    if length <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..length as usize])
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    note_hook_call();
    let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    CURSOR_X.store(info.pt.x, Ordering::Relaxed);
    CURSOR_Y.store(info.pt.y, Ordering::Relaxed);
    let message = wparam.0 as u32;

    if !PAUSED.load(Ordering::Relaxed) {
        match message {
            WM_LBUTTONDOWN => {
                if WHEEL_OPEN.load(Ordering::SeqCst) && KEY_WHEEL.load(Ordering::SeqCst) {
                    if HOVER.load(Ordering::Relaxed) >= 0 {
                        post(WM_APP_DROP, 0, 0);
                        return LRESULT(1);
                    }
                    post(WM_APP_CANCEL, 0, 0);
                }
                if pointer_in_explorer(info.pt) {
                    TRACKING.store(true, Ordering::Relaxed);
                    ARMED.store(false, Ordering::Relaxed);
                    START_X.store(info.pt.x, Ordering::Relaxed);
                    START_Y.store(info.pt.y, Ordering::Relaxed);
                } else {
                    TRACKING.store(false, Ordering::Relaxed);
                }
            }
            WM_MOUSEMOVE => {
                if TRACKING.load(Ordering::Relaxed) && !ARMED.load(Ordering::Relaxed) {
                    let dx = (info.pt.x - START_X.load(Ordering::Relaxed)).abs();
                    let dy = (info.pt.y - START_Y.load(Ordering::Relaxed)).abs();
                    let threshold_x = unsafe { GetSystemMetrics(SM_CXDRAG) }.max(4);
                    let threshold_y = unsafe { GetSystemMetrics(SM_CYDRAG) }.max(4);
                    if dx > threshold_x || dy > threshold_y {
                        ARMED.store(true, Ordering::Relaxed);
                        DRAG_ACTIVE.store(true, Ordering::SeqCst);
                        post(WM_APP_DRAG_START, 0, 0);
                    }
                }
                if WHEEL_OPEN.load(Ordering::Relaxed) {
                    publish_hover(info.pt.x, info.pt.y);
                }
            }
            WM_LBUTTONUP => {
                TRACKING.store(false, Ordering::Relaxed);
                ARMED.store(false, Ordering::Relaxed);
                let wheel = WHEEL_OPEN.load(Ordering::SeqCst);
                DRAG_ACTIVE.store(false, Ordering::SeqCst);
                if wheel {
                    post(WM_APP_DROP, 0, 0);
                }
            }
            _ => {}
        }
    }

    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn publish_hover(x: i32, y: i32) {
    let count = WHEEL_COUNT.load(Ordering::Relaxed);
    let dead = f32::from_bits(WHEEL_DEAD.load(Ordering::Relaxed));
    let outer = f32::from_bits(WHEEL_OUTER.load(Ordering::Relaxed));
    let dx = x as f32 - WHEEL_CX.load(Ordering::Relaxed) as f32;
    let dy = y as f32 - WHEEL_CY.load(Ordering::Relaxed) as f32;
    let next = hit_index(dx, dy, count, dead, outer).map(|index| index as i32).unwrap_or(-1);
    let previous = HOVER.swap(next, Ordering::Relaxed);
    if previous != next {
        post(WM_APP_HOVER, 0, 0);
    }
}

pub fn remember_wheel(center_x: i32, center_y: i32, dead: f32, outer: f32, count: usize) {
    WHEEL_CX.store(center_x, Ordering::Relaxed);
    WHEEL_CY.store(center_y, Ordering::Relaxed);
    WHEEL_DEAD.store(dead.to_bits(), Ordering::Relaxed);
    WHEEL_OUTER.store(outer.to_bits(), Ordering::Relaxed);
    WHEEL_COUNT.store(count, Ordering::Relaxed);
    HOVER.store(-1, Ordering::Relaxed);
    WHEEL_OPEN.store(true, Ordering::SeqCst);
}

pub fn clear_wheel_flag() {
    WHEEL_OPEN.store(false, Ordering::SeqCst);
    KEY_WHEEL.store(false, Ordering::SeqCst);
    WHEEL_COUNT.store(0, Ordering::Relaxed);
    HOVER.store(-1, Ordering::Relaxed);
}

pub fn shell_place() -> u8 {
    if desktop_focused() {
        SHELL_DESKTOP
    } else if explorer_focused() {
        SHELL_EXPLORER
    } else {
        SHELL_NONE
    }
}

fn explorer_focused() -> bool {
    unsafe { is_cabinet(GetForegroundWindow()) }
}

pub fn desktop_focused() -> bool {
    unsafe {
        let foreground = GetForegroundWindow();
        if is_desktop_window(foreground) {
            return true;
        }
        is_desktop_window(GetAncestor(foreground, GA_ROOT))
    }
}

fn is_desktop_window(hwnd: HWND) -> bool {
    let class = class_name(hwnd);
    class == "Progman" || class == "WorkerW"
}

pub fn desktop_paths() -> Vec<PathBuf> {
    let mut point = POINT::default();
    if unsafe { GetCursorPos(&mut point) }.is_ok() {
        if let Some(path) = unsafe { desktop_item_at(point) } {
            return vec![path];
        }
    }
    desktop_selection()
}

pub fn desktop_selection() -> Vec<PathBuf> {
    match unsafe { desktop_selection_inner() } {
        Ok(paths) => paths,
        Err(error) => {
            log_line(&format!("sélection bureau: {error}"));
            Vec::new()
        }
    }
}

unsafe fn desktop_selection_inner() -> Result<Vec<PathBuf>, windows::core::Error> {
    let shell_windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
    let mut hwnd = 0i32;
    let location = VARIANT::default();
    let root = VARIANT::default();
    let dispatch = shell_windows.FindWindowSW(&location, &root, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)?;
    let provider: IServiceProvider = dispatch.cast()?;
    let browser: IShellBrowser = provider.QueryService(&IShellBrowser::IID)?;
    let view = browser.QueryActiveShellView()?;
    let items: IShellItemArray = view.GetItemObject(SVGIO_SELECTION)?;
    Ok(paths_from_items(&items))
}

unsafe fn desktop_item_at(screen: POINT) -> Option<PathBuf> {
    let list = desktop_listview()?;
    let shell_windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
    let mut hwnd = 0i32;
    let location = VARIANT::default();
    let root = VARIANT::default();
    let dispatch = shell_windows
        .FindWindowSW(&location, &root, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)
        .ok()?;
    let provider: IServiceProvider = dispatch.cast().ok()?;
    let browser: IShellBrowser = provider.QueryService(&IShellBrowser::IID).ok()?;
    let view = browser.QueryActiveShellView().ok()?;
    let folder: IFolderView = view.cast().ok()?;
    let count = folder.ItemCount(SVGIO_ALLVIEW).ok()?.clamp(0, 800);
    let mut spacing = POINT { x: 90, y: 110 };
    if folder.GetSpacing(&mut spacing).is_err() || spacing.x < 32 || spacing.y < 32 {
        spacing = POINT { x: 90, y: 110 };
    }
    for index in 0..count {
        let Ok(pidl) = folder.Item(index) else {
            continue;
        };
        let mut origin = match folder.GetItemPosition(pidl) {
            Ok(origin) => origin,
            Err(_) => {
                CoTaskMemFree(Some(pidl.cast()));
                continue;
            }
        };
        let _ = ClientToScreen(list, &mut origin);
        let hit = screen.x >= origin.x
            && screen.y >= origin.y
            && screen.x < origin.x + spacing.x
            && screen.y < origin.y + spacing.y;
        if !hit {
            CoTaskMemFree(Some(pidl.cast()));
            continue;
        }
        let named = SHGetNameFromIDList(pidl, SIGDN_FILESYSPATH);
        CoTaskMemFree(Some(pidl.cast()));
        let Ok(name) = named else {
            continue;
        };
        let text = name.to_string().unwrap_or_default();
        CoTaskMemFree(Some(name.0.cast()));
        if !text.is_empty() {
            return Some(PathBuf::from(text));
        }
    }
    None
}

fn desktop_listview() -> Option<HWND> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        let defview = FindWindowExW(Some(progman), None, w!("SHELLDLL_DefView"), PCWSTR::null()).ok()?;
        let list = FindWindowExW(Some(defview), None, w!("SysListView32"), PCWSTR::null()).ok()?;
        if list.0.is_null() { None } else { Some(list) }
    }
}

#[cfg(test)]
fn selection_for_tests() -> Vec<PathBuf> {
    let _guard = COM_TEST.lock().unwrap_or_else(|error| error.into_inner());
    let _ = ensure_com();
    let _desktop = desktop_selection();
    explorer_selection()
}

#[cfg(test)]
static COM_TEST: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::selection_for_tests;
    use windows::core::Interface;

    #[test]
    fn reading_the_explorer_selection_does_not_panic() {
        let paths = selection_for_tests();
        let _ = paths.len();
    }

    #[test]
    fn an_icon_cell_resolves_to_its_file() {
        let _guard = super::COM_TEST.lock().unwrap_or_else(|error| error.into_inner());
        let _ = super::ensure_com();
        let Some(list) = super::desktop_listview() else {
            return;
        };
        let found = unsafe {
            (|| {
                let shell_windows: super::IShellWindows =
                    super::CoCreateInstance(&super::ShellWindows, None, super::CLSCTX_ALL).ok()?;
                let mut hwnd = 0i32;
                let dispatch = shell_windows
                    .FindWindowSW(
                        &super::VARIANT::default(),
                        &super::VARIANT::default(),
                        super::SWC_DESKTOP,
                        &mut hwnd,
                        super::SWFO_NEEDDISPATCH,
                    )
                    .ok()?;
                let provider: super::IServiceProvider = dispatch.cast().ok()?;
                let browser: super::IShellBrowser = provider.QueryService(&super::IShellBrowser::IID).ok()?;
                let view = browser.QueryActiveShellView().ok()?;
                let folder: super::IFolderView = view.cast().ok()?;
                let count = folder.ItemCount(super::SVGIO_ALLVIEW).ok()?;
                if count <= 0 {
                    return None;
                }
                let pidl = folder.Item(0).ok()?;
                let mut origin = folder.GetItemPosition(pidl).ok()?;
                let _ = super::ClientToScreen(list, &mut origin);
                super::CoTaskMemFree(Some(pidl.cast()));
                let mut spacing = super::POINT { x: 90, y: 110 };
                if folder.GetSpacing(&mut spacing).is_err() || spacing.x < 32 || spacing.y < 32 {
                    spacing = super::POINT { x: 90, y: 110 };
                }
                let probe = super::POINT {
                    x: origin.x + spacing.x / 2,
                    y: origin.y + spacing.y / 2,
                };
                super::desktop_item_at(probe)
            })()
        };
        let Some(path) = found else {
            panic!("aucune icône trouvée sous sa propre cellule");
        };
        assert!(path.exists(), "{}", path.display());
    }

    #[test]
    fn the_explorer_row_under_the_pointer_is_that_file() {
        let _guard = super::COM_TEST.lock().unwrap_or_else(|error| error.into_inner());
        let _ = super::ensure_com();
        let Some((hwnd, view, index)) = (unsafe { super::explorer_window_with_file(r"cinder\site\index.html") }) else {
            return;
        };
        let Some(center) = (unsafe { super::list_item_center(hwnd, index) }) else {
            return;
        };
        match unsafe { super::row_at_point(center) } {
            super::RowAt::Item { index: found, .. } => {
                assert_eq!(found, Some(index));
                let path = unsafe { super::path_at(&view, index) }.expect("chemin");
                assert!(path.ends_with("index.html"), "{}", path.display());
            }
            super::RowAt::List => panic!("le pointeur est dans la liste, pas sur index.html"),
            super::RowAt::Outside => panic!("le pointeur est hors de la liste de fichiers"),
        }
    }
}

#[cfg(test)]
unsafe fn explorer_window_with_file(suffix: &str) -> Option<(HWND, IFolderView, i32)> {
    let shell_windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
    let count = shell_windows.Count().unwrap_or(0);
    let needle = suffix.to_ascii_lowercase();
    for index in 0..count {
        let Ok(item) = shell_windows.Item(&variant_i4(index)) else {
            continue;
        };
        let Ok(provider) = item.cast::<IServiceProvider>() else {
            continue;
        };
        let Ok(browser) = provider.QueryService::<IShellBrowser>(&IShellBrowser::IID) else {
            continue;
        };
        let Ok(view) = browser.QueryActiveShellView() else {
            continue;
        };
        let Ok(folder) = view.cast::<IFolderView>() else {
            continue;
        };
        let rows = folder.ItemCount(SVGIO_ALLVIEW).unwrap_or(0).clamp(0, 2000);
        for row in 0..rows {
            let Some(path) = path_at(&folder, row) else {
                continue;
            };
            if !path.to_string_lossy().to_ascii_lowercase().ends_with(&needle) {
                continue;
            }
            let Ok(hwnd) = browser.GetWindow() else {
                continue;
            };
            let root = GetAncestor(hwnd, GA_ROOT);
            let window = if root.0.is_null() { hwnd } else { root };
            return Some((window, folder, row));
        }
    }
    None
}

#[cfg(test)]
unsafe fn list_item_center(hwnd: HWND, index: i32) -> Option<POINT> {
    use windows::Win32::UI::Accessibility::TreeScope_Descendants;
    let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL).ok()?;
    let root = automation.ElementFromHandle(hwnd).ok()?;
    let condition = automation.CreateTrueCondition().ok()?;
    let all = root.FindAll(TreeScope_Descendants, &condition).ok()?;
    let length = all.Length().unwrap_or(0).min(800);
    let wanted = index.to_string();
    for item_index in 0..length {
        let Ok(element) = all.GetElement(item_index) else {
            continue;
        };
        if bstr_text(element.CurrentClassName()) != "UIItem" {
            continue;
        }
        if bstr_text(element.CurrentAutomationId()) != wanted {
            continue;
        }
        let rect = element.CurrentBoundingRectangle().ok()?;
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return None;
        }
        return Some(POINT {
            x: rect.left + (rect.right - rect.left) / 2,
            y: rect.top + (rect.bottom - rect.top) / 2,
        });
    }
    None
}
