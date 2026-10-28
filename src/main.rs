#![windows_subsystem = "windows"]
#![allow(unsafe_op_in_unsafe_fn)]

mod actions;
mod archive;
mod catalog;
mod convert;
mod dashboard;
mod documents;
mod drag;
mod keys;
mod media;
mod overlay;
mod presence;
mod savings;
mod textconv;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::Mutex;

use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, MessageBoxW,
    PostQuitMessage, RegisterClassExW, SetTimer, TranslateMessage, HWND_MESSAGE, MB_ICONERROR, MSG, WNDCLASSEXW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY, WM_TIMER,
};

use catalog::{category_of, items_for, Category, Command, OutputFormat, WheelItem, WheelKind};
use convert::{apply_all, format_of_path, load_source, Apply, FormatKind, Source};
use drag::{
    button_down, cancel_gesture, clear_wheel_flag, desktop_paths, ensure_com, ensure_hooks_fresh, explorer_paths,
    explorer_selection, host, log_line, post, remember_wheel, remove_mouse_hook, set_host, CURSOR_X, CURSOR_Y, DRAG_ACTIVE,
    ExplorerHit, SHELL_DESKTOP, SHELL_EXPLORER, SHELL_KEY, SHELL_NONE,
    HOVER, KEY_WHEEL, PAUSED, WM_APP_ACTIVATE, WM_APP_CANCEL, WM_APP_DRAG_START, WM_APP_DROP, WM_APP_HOVER,
    WM_APP_OPEN, WM_APP_STATS, WM_APP_TOAST, WM_APP_TOGGLE,
};
use overlay::WheelView;

struct Model {
    paths: Vec<PathBuf>,
    title: String,
    items: Vec<WheelItem>,
    center: (i32, i32),
    scale: f32,
    open: bool,
    toast: bool,
}

impl Model {
    const fn empty() -> Self {
        Self {
            paths: Vec::new(),
            title: String::new(),
            items: Vec::new(),
            center: (0, 0),
            scale: 1.0,
            open: false,
            toast: false,
        }
    }
}

struct Prefetch {
    paths: Vec<PathBuf>,
    sources: Vec<Source>,
    ready: bool,
}

static MODEL: Mutex<Model> = Mutex::new(Model::empty());
static PREFETCH: Mutex<Prefetch> = Mutex::new(Prefetch {
    paths: Vec::new(),
    sources: Vec::new(),
    ready: false,
});
static TOAST: Mutex<String> = Mutex::new(String::new());
static OVERLAY: AtomicIsize = AtomicIsize::new(0);
static ACTIVATE: AtomicBool = AtomicBool::new(false);
static TOGGLE: AtomicBool = AtomicBool::new(false);
static PROMOTED: AtomicBool = AtomicBool::new(false);
static PROMOTE_TRIES: AtomicU32 = AtomicU32::new(0);
static REBUILD: AtomicBool = AtomicBool::new(false);

struct Shell {
    icon: tray_icon::TrayIcon,
    pause: MenuItem,
    status: MenuItem,
}

fn main() {
    std::panic::set_hook(Box::new(|info| log_line(&format!("panic: {info}"))));
    if let Err(error) = run() {
        log_line(&error);
        unsafe {
            let _ = MessageBoxW(None, w!("Cinder n'a pas pu démarrer."), w!("Cinder"), MB_ICONERROR);
        }
    }
}

fn run() -> Result<(), String> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    ensure_com()?;
    if let presence::Startup::Stop(code) = presence::prepare_startup() {
        std::process::exit(code);
    }
    let _instance = match single_instance() {
        Instance::AlreadyRunning => {
            if !presence::wake_running() {
                log_line("déjà lancé");
            }
            return Ok(());
        }
        Instance::Primary(handle) => handle,
    };
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(w!("Cinder.Desktop"));
    }

    let module = unsafe { GetModuleHandleW(PCWSTR::null()) }.map_err(|error| error.to_string())?;
    let instance = HINSTANCE(module.0);
    let host_window = create_host(instance)?;
    set_host(host_window);
    let overlay = overlay::create_overlay(instance)?;
    OVERLAY.store(overlay.0 as isize, Ordering::SeqCst);

    let mouse_hook = drag::install_mouse_hook()?;
    let keyboard_hook = keys::install_keyboard_hook()?;

    let spot = presence::place_tray_icon();
    let mut shell = build_shell(true)?;
    if spot != presence::TraySpot::Waiting {
        PROMOTED.store(true, Ordering::SeqCst);
        if spot == presence::TraySpot::Updated {
            log_line("icône en bas");
        }
    }
    presence::install_shortcuts();

    log_line("prêt");
    unsafe {
        let _ = SetTimer(Some(host_window), 2, 1000, None);
    }
    if let Err(error) = dashboard::create(instance) {
        log_line(&format!("tableau: {error}"));
    }
    on_activate(&shell.pause, &shell.status, &shell.icon);
    let mut message = MSG::default();
    loop {
        let message_status = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if message_status.0 == 0 || message_status.0 == -1 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == "pause" {
                toggle_running(&shell.pause, &shell.status, &shell.icon);
            } else if event.id == "quit" {
                unsafe { PostQuitMessage(0) };
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                on_activate(&shell.pause, &shell.status, &shell.icon);
            }
        }
        if ACTIVATE.swap(false, Ordering::SeqCst) {
            on_activate(&shell.pause, &shell.status, &shell.icon);
        }
        if TOGGLE.swap(false, Ordering::SeqCst) {
            toggle_running(&shell.pause, &shell.status, &shell.icon);
        }
        if REBUILD.swap(false, Ordering::SeqCst) {
            let running = !PAUSED.load(Ordering::SeqCst);
            drop(shell);
            shell = build_shell(running)?;
        }
    }

    let _ = (mouse_hook, keyboard_hook);
    remove_mouse_hook();
    keys::remove_keyboard_hook();
    let _ = _instance;
    Ok(())
}

enum Instance {
    Primary(windows::Win32::Foundation::HANDLE),
    AlreadyRunning,
}

fn single_instance() -> Instance {
    unsafe {
        let handle = match CreateMutexW(None, true, w!("Local\\Cinder.SingleInstance")) {
            Ok(handle) => handle,
            Err(_) => return Instance::AlreadyRunning,
        };
        if GetLastError() == ERROR_ALREADY_EXISTS {
            Instance::AlreadyRunning
        } else {
            Instance::Primary(handle)
        }
    }
}

fn create_host(instance: HINSTANCE) -> Result<HWND, String> {
    unsafe {
        let class_name = w!("CinderHost");
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(host_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..WNDCLASSEXW::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err("Impossible de créer la fenêtre de Cinder".to_string());
        }
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Cinder"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )
        .map_err(|error| error.to_string())
    }
}

unsafe extern "system" fn host_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if let Some(result) = handle_message(message, wparam) {
        return result;
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn handle_message(message: u32, wparam: WPARAM) -> Option<LRESULT> {
    match message {
        WM_APP_DRAG_START => {
            on_drag_start();
            Some(LRESULT(0))
        }
        WM_APP_OPEN => {
            if let Some(kind) = WheelKind::from_code(wparam.0) {
                on_open(kind);
            }
            Some(LRESULT(0))
        }
        WM_APP_HOVER => {
            on_hover();
            Some(LRESULT(0))
        }
        WM_APP_DROP => {
            on_drop();
            Some(LRESULT(0))
        }
        WM_APP_CANCEL => {
            on_cancel();
            Some(LRESULT(0))
        }
        WM_APP_TOAST => {
            on_toast_message();
            Some(LRESULT(0))
        }
        WM_APP_ACTIVATE => {
            ACTIVATE.store(true, Ordering::SeqCst);
            Some(LRESULT(0))
        }
        WM_APP_TOGGLE => {
            TOGGLE.store(true, Ordering::SeqCst);
            Some(LRESULT(0))
        }
        WM_APP_STATS => {
            dashboard::redraw();
            Some(LRESULT(0))
        }
        WM_TIMER => {
            if wparam.0 == 2 {
                ensure_hooks_fresh(keys::reinstall_keyboard_hook);
                promote_once();
            } else {
                on_timer();
            }
            Some(LRESULT(0))
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            Some(LRESULT(0))
        }
        _ => None,
    }
}

fn on_drag_start() {
    if !button_down() {
        DRAG_ACTIVE.store(false, Ordering::SeqCst);
        return;
    }
    let paths = explorer_selection();
    if paths.is_empty() || !button_down() {
        DRAG_ACTIVE.store(false, Ordering::SeqCst);
        model().paths.clear();
        log_line("glisser sans fichier");
        return;
    }
    log_line(&format!("glisser {} fichier(s)", paths.len()));
    prefetch_images(&paths);
    model().paths = paths;
    DRAG_ACTIVE.store(true, Ordering::SeqCst);
}

fn on_open(kind: WheelKind) {
    let place = SHELL_KEY.swap(SHELL_NONE, Ordering::SeqCst);
    if !DRAG_ACTIVE.load(Ordering::SeqCst) && place == SHELL_NONE {
        return;
    }
    let mut model = model();
    if place == SHELL_DESKTOP {
        let paths = desktop_paths();
        log_line(&format!("bureau {} fichier(s)", paths.len()));
        prefetch_images(&paths);
        model.paths = paths;
        KEY_WHEEL.store(true, Ordering::SeqCst);
    } else if place == SHELL_EXPLORER {
        match explorer_paths() {
            ExplorerHit::Ignore => return,
            ExplorerHit::Files(paths) => {
                log_line(&format!("explorateur {} fichier(s)", paths.len()));
                prefetch_images(&paths);
                model.paths = paths;
                KEY_WHEEL.store(true, Ordering::SeqCst);
            }
        }
    } else if model.paths.is_empty() {
        return;
    }
    if model.paths.is_empty() {
        close_wheel(&mut model);
        drop(model);
        show_toast("Sélectionne un fichier");
        log_line("Sélectionne un fichier");
        return;
    }
    let (title, items) = items_for(kind, &model.paths);
    if items.is_empty() {
        let message = if model.paths.is_empty() {
            "Sélectionne un fichier"
        } else if kind == WheelKind::Tools {
            "Aucun outil pour ce type"
        } else if model.paths.first().and_then(|path| category_of(path)).is_some() {
            "Rien d'autre à convertir"
        } else {
            "Pas encore de conversion pour ce type"
        };
        close_wheel(&mut model);
        drop(model);
        show_toast(message);
        log_line(message);
        return;
    }
    let x = CURSOR_X.load(Ordering::Relaxed);
    let y = CURSOR_Y.load(Ordering::Relaxed);
    let scale = overlay::scale_at(x, y);
    let (dead, outer, _) = overlay::geometry(scale);
    model.title = title;
    model.items = items;
    model.center = (x, y);
    model.scale = scale;
    model.open = true;
    model.toast = false;
    remember_wheel(x, y, dead, outer, model.items.len());
    unsafe {
        let _ = KillTimer(Some(host()), 1);
    }
    paint_wheel(&model);
    let name = model
        .paths
        .first()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "fichier".to_string());
    let labels = model.items.iter().map(|item| item.label).collect::<Vec<_>>().join(" ");
    log_line(&format!("roue {name} : {labels}"));
}

fn on_hover() {
    let model = model();
    if model.open {
        paint_wheel(&model);
    }
}

fn on_drop() {
    let mut model = model();
    if !model.open {
        clear_wheel_flag();
        return;
    }
    let command = usize::try_from(HOVER.load(Ordering::Relaxed))
        .ok()
        .and_then(|index| model.items.get(index).map(|item| item.command.clone()));
    let paths = model.paths.clone();
    close_wheel(&mut model);
    drop(model);
    if paths.is_empty() {
        if command.is_some() {
            show_toast("Sélectionne un fichier");
        }
        return;
    }
    if let Some(command) = command {
        perform(command, paths);
    }
}

fn on_cancel() {
    let mut model = model();
    close_wheel(&mut model);
}

fn on_toast_message() {
    let text = TOAST.lock().unwrap_or_else(|error| error.into_inner()).clone();
    if !text.is_empty() {
        show_toast(&text);
    }
}

fn on_timer() {
    let mut model = model();
    if model.toast {
        model.toast = false;
        if !model.open {
            overlay::hide(overlay_window());
        }
    }
    unsafe {
        let _ = KillTimer(Some(host()), 1);
    }
}

fn perform(command: Command, paths: Vec<PathBuf>) {
    match command {
        Command::Convert(format) => {
            let (paths, skipped) = same_category(&paths);
            convert_paths(paths, skipped, format);
        }
        Command::Compress => {
            let (paths, skipped) = same_category(&paths);
            compress_paths(paths, skipped);
        }
        Command::Resize(edge) => {
            let (paths, skipped) = same_category(&paths);
            spawn_image(paths, skipped, Apply::Resize(edge));
        }
        Command::Rotate => {
            let (paths, skipped) = same_category(&paths);
            spawn_image(paths, skipped, Apply::Rotate);
        }
        Command::Mute => {
            let (paths, skipped) = same_category(&paths);
            spawn_job(paths.len(), skipped, move || media::apply(&paths, media::MediaOp::Mute));
        }
        Command::Snapshot => {
            let (paths, skipped) = same_category(&paths);
            spawn_job(paths.len(), skipped, move || media::apply(&paths, media::MediaOp::Snapshot));
        }
        Command::Normalize => {
            let (paths, skipped) = same_category(&paths);
            spawn_job(paths.len(), skipped, move || media::apply(&paths, media::MediaOp::Normalize));
        }
        Command::Extract => {
            let (paths, skipped) = same_category(&paths);
            spawn_job(paths.len(), skipped, move || archive::extract(&paths));
        }
        Command::Split => {
            let (paths, skipped) = same_category(&paths);
            spawn_job(paths.len(), skipped, move || documents::split(&paths));
        }
        Command::Metadata => {
            let (paths, skipped) = same_category(&paths);
            spawn_job(paths.len(), skipped, move || documents::metadata(&paths));
        }
        Command::CopyPath => {
            let text = paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join("\r\n");
            show_result(actions::copy_text(&text).map(|_| "Chemin copié".to_string()));
        }
        Command::Reveal => {
            let Some(path) = paths.first() else {
                return;
            };
            show_result(actions::reveal(path).map(|_| "Affiché dans l'Explorateur".to_string()));
        }
        Command::Terminal => {
            let Some(path) = paths.first() else {
                return;
            };
            show_result(actions::open_terminal(path).map(|_| "Terminal ouvert".to_string()));
        }
        Command::CopyTo => match actions::pick_folder("Copier vers") {
            Ok(Some(directory)) => show_result(actions::copy_into(&paths, &directory)),
            Ok(None) => {}
            Err(error) => show_result(Err(error)),
        },
        Command::MoveTo => match actions::pick_folder("Déplacer vers") {
            Ok(Some(directory)) => show_result(actions::move_into(&paths, &directory)),
            Ok(None) => {}
            Err(error) => show_result(Err(error)),
        },
    }
}

fn convert_paths(paths: Vec<PathBuf>, skipped: usize, format: OutputFormat) {
    let Some(category) = paths.first().and_then(|path| category_of(path)) else {
        show_toast("Pas encore de conversion pour ce type");
        return;
    };
    match category {
        Category::Image => {
            let Some(kind) = image_kind(format) else {
                show_toast("Pas encore de conversion pour ce type");
                return;
            };
            spawn_image(paths, skipped, Apply::Convert(kind));
        }
        Category::Video | Category::Audio => {
            spawn_job(paths.len(), skipped, move || media::apply(&paths, media::MediaOp::Convert(format)));
        }
        Category::Subtitle => spawn_job(paths.len(), skipped, move || textconv::apply(&paths, format)),
        Category::Text => match format {
            OutputFormat::Pdf | OutputFormat::Docx | OutputFormat::Epub => {
                spawn_job(paths.len(), skipped, move || documents::convert(&paths, format));
            }
            _ => spawn_job(paths.len(), skipped, move || textconv::apply(&paths, format)),
        },
        Category::Spreadsheet => match format {
            OutputFormat::Ods | OutputFormat::Pdf => {
                spawn_job(paths.len(), skipped, move || documents::convert(&paths, format))
            }
            _ => spawn_job(paths.len(), skipped, move || textconv::apply(&paths, format)),
        },
        Category::Epub => match format {
            OutputFormat::Pdf | OutputFormat::Docx => {
                spawn_job(paths.len(), skipped, move || documents::convert(&paths, format))
            }
            _ => spawn_job(paths.len(), skipped, move || textconv::apply(&paths, format)),
        },
        Category::Pdf | Category::Document | Category::Slides => {
            spawn_job(paths.len(), skipped, move || documents::convert(&paths, format))
        }
        Category::Archive => spawn_job(paths.len(), skipped, move || archive::convert(&paths, format)),
    }
}

fn compress_paths(paths: Vec<PathBuf>, skipped: usize) {
    let Some(category) = paths.first().and_then(|path| category_of(path)) else {
        show_toast("Aucun outil pour ce type");
        return;
    };
    match category {
        Category::Image => spawn_image(paths, skipped, Apply::Compress),
        Category::Video => spawn_job(paths.len(), skipped, move || media::apply(&paths, media::MediaOp::CompressVideo)),
        Category::Audio => spawn_job(paths.len(), skipped, move || media::apply(&paths, media::MediaOp::CompressAudio)),
        Category::Pdf => spawn_job(paths.len(), skipped, move || documents::compress_pdf(&paths)),
        Category::Document => spawn_job(paths.len(), skipped, move || documents::compress_document(&paths)),
        _ => show_toast("Aucun outil pour ce type"),
    }
}

fn image_kind(format: OutputFormat) -> Option<FormatKind> {
    Some(match format {
        OutputFormat::Jpeg => FormatKind::Jpeg,
        OutputFormat::Png => FormatKind::Png,
        OutputFormat::WebP => FormatKind::WebP,
        OutputFormat::Bmp => FormatKind::Bmp,
        OutputFormat::Gif => FormatKind::Gif,
        OutputFormat::Tiff => FormatKind::Tiff,
        OutputFormat::Pdf => FormatKind::Pdf,
        _ => return None,
    })
}

fn same_category(paths: &[PathBuf]) -> (Vec<PathBuf>, usize) {
    let Some(first) = paths.first() else {
        return (Vec::new(), 0);
    };
    let category = category_of(first);
    let mut same = Vec::new();
    let mut skipped = 0;
    for path in paths {
        if category_of(path) == category {
            same.push(path.clone());
        } else {
            skipped += 1;
        }
    }
    (same, skipped)
}

fn spawn_image(paths: Vec<PathBuf>, skipped: usize, operation: Apply) {
    spawn_apply(paths, operation, skipped);
}

fn spawn_job(files: usize, skipped: usize, job: impl FnOnce() -> Result<String, String> + Send + 'static) {
    std::thread::spawn(move || publish(with_skipped(job(), skipped), files));
}

fn publish(result: Result<String, String>, files: usize) {
    if result.is_ok() {
        savings::record_files(files);
        post(WM_APP_STATS, 0, 0);
    } else if let Err(error) = &result {
        log_line(error);
    }
    let text = result.unwrap_or_else(|error| error);
    *TOAST.lock().unwrap_or_else(|error| error.into_inner()) = text;
    post(WM_APP_TOAST, 0, 0);
}

fn with_skipped(result: Result<String, String>, skipped: usize) -> Result<String, String> {
    match result {
        Ok(text) if skipped == 1 => Ok(format!("{text} · 1 ignoré")),
        Ok(text) if skipped > 1 => Ok(format!("{text} · {skipped} ignorés")),
        other => other,
    }
}

fn prefetch_images(paths: &[PathBuf]) {
    if !paths.is_empty() && paths.iter().all(|path| format_of_path(path).is_some()) {
        begin_prefetch(paths);
    }
}

fn spawn_apply(paths: Vec<PathBuf>, operation: Apply, skipped: usize) {
    let ready = {
        let mut slot = PREFETCH.lock().unwrap_or_else(|error| error.into_inner());
        if slot.ready && slot.paths == paths {
            Some(std::mem::take(&mut slot.sources))
        } else {
            None
        }
    };
    std::thread::spawn(move || {
        let sources = ready.unwrap_or_else(|| paths.iter().map(|path| load_source(path)).collect());
        publish(with_skipped(apply_all(&sources, &operation), skipped), paths.len());
    });
}

fn begin_prefetch(paths: &[PathBuf]) {
    let owned = paths.to_vec();
    {
        let mut slot = PREFETCH.lock().unwrap_or_else(|error| error.into_inner());
        slot.paths = owned.clone();
        slot.sources.clear();
        slot.ready = false;
    }
    std::thread::spawn(move || {
        let sources: Vec<_> = owned.iter().map(|path| load_source(path)).collect();
        let mut slot = PREFETCH.lock().unwrap_or_else(|error| error.into_inner());
        if slot.paths == owned {
            slot.sources = sources;
            slot.ready = true;
        }
    });
}

fn show_result(result: Result<String, String>) {
    let text = match result {
        Ok(text) => text,
        Err(error) => {
            log_line(&error);
            error
        }
    };
    show_toast(&text);
}

fn show_toast(text: &str) {
    let mut model = model();
    if model.open {
        return;
    }
    let x = CURSOR_X.load(Ordering::Relaxed);
    let y = CURSOR_Y.load(Ordering::Relaxed);
    let scale = overlay::scale_at(x, y);
    let shown = truncate_chars(text, 80);
    let (pixels, width, height) = overlay::render_toast(&shown, scale);
    let offset = (72.0 * scale) as i32;
    overlay::present(overlay_window(), (x, y - offset), &pixels, width, height);
    model.toast = true;
    drop(model);
    unsafe {
        SetTimer(Some(host()), 1, 1700, None);
    }
}

fn paint_wheel(model: &Model) {
    let hover = usize::try_from(HOVER.load(Ordering::Relaxed))
        .ok()
        .filter(|index| *index < model.items.len());
    let title = hover
        .map(|index| model.items[index].label.to_string())
        .unwrap_or_else(|| model.title.clone());
    let caption = hover
        .map(|index| model.items[index].detail.to_string())
        .unwrap_or_else(|| "Échap ferme".to_string());
    let labels = model.items.iter().map(|item| item.label.to_string()).collect();
    let (pixels, width, height) = overlay::render_wheel(&WheelView {
        title,
        caption,
        labels,
        hover,
        scale: model.scale,
    });
    overlay::present(overlay_window(), model.center, &pixels, width, height);
}

fn close_wheel(model: &mut Model) {
    model.open = false;
    model.items.clear();
    clear_wheel_flag();
    if !model.toast {
        overlay::hide(overlay_window());
    }
}

fn on_activate(pause: &MenuItem, status: &MenuItem, tray: &tray_icon::TrayIcon) {
    if PAUSED.load(Ordering::SeqCst) {
        apply_presence(pause, status, tray, true);
        log_line("reprise");
    }
    log_line("activé");
    dashboard::show();
}

fn toggle_running(pause: &MenuItem, status: &MenuItem, tray: &tray_icon::TrayIcon) {
    let resume = PAUSED.load(Ordering::SeqCst);
    apply_presence(pause, status, tray, resume);
    log_line(if resume { "reprise" } else { "pause" });
}

fn apply_presence(pause: &MenuItem, status: &MenuItem, tray: &tray_icon::TrayIcon, running: bool) {
    PAUSED.store(!running, Ordering::SeqCst);
    if running {
        pause.set_text("Pause");
        status.set_text("Activé");
        let _ = tray.set_tooltip(Some("Cinder activé"));
        let _ = tray.set_icon(Some(presence::mark_icon(true)));
    } else {
        let mut model = model();
        close_wheel(&mut model);
        cancel_gesture();
        keys::release_modifiers();
        pause.set_text("Reprendre");
        status.set_text("En pause");
        let _ = tray.set_tooltip(Some("Cinder en pause"));
        let _ = tray.set_icon(Some(presence::mark_icon(false)));
    }
    dashboard::redraw();
}

fn build_shell(running: bool) -> Result<Shell, String> {
    let status = MenuItem::with_id("status", if running { "Activé" } else { "En pause" }, false, None);
    let pause = MenuItem::with_id("pause", if running { "Pause" } else { "Reprendre" }, true, None);
    let quit = MenuItem::with_id("quit", "Quitter", true, None);
    let menu = Menu::new();
    menu.append(&status).map_err(|error| error.to_string())?;
    menu.append(&pause).map_err(|error| error.to_string())?;
    menu.append(&quit).map_err(|error| error.to_string())?;
    let icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip(if running { "Cinder activé" } else { "Cinder en pause" })
        .with_icon(presence::mark_icon(running))
        .with_guid(presence::TRAY_GUID)
        .build()
        .map_err(|error| error.to_string())?;
    Ok(Shell { icon, pause, status })
}

fn promote_once() {
    if PROMOTED.load(Ordering::Relaxed) || PROMOTE_TRIES.fetch_add(1, Ordering::Relaxed) > 30 {
        PROMOTED.store(true, Ordering::Relaxed);
        return;
    }
    match presence::place_tray_icon() {
        presence::TraySpot::Waiting => {}
        presence::TraySpot::Already => PROMOTED.store(true, Ordering::Relaxed),
        presence::TraySpot::Updated => {
            PROMOTED.store(true, Ordering::Relaxed);
            REBUILD.store(true, Ordering::SeqCst);
            log_line("icône en bas");
        }
    }
}

fn model() -> std::sync::MutexGuard<'static, Model> {
    MODEL.lock().unwrap_or_else(|error| error.into_inner())
}

fn overlay_window() -> HWND {
    HWND(OVERLAY.load(Ordering::SeqCst) as *mut core::ffi::c_void)
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let mut shortened: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    shortened.push('…');
    shortened
}

