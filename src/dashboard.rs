use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;

use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, EndPaint, GetDC, InvalidateRect,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetSystemMetrics, LoadCursorW, RegisterClassExW,
    SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow, CS_HREDRAW, CS_VREDRAW, GetForegroundWindow,
    GetWindowThreadProcessId, HMENU, HWND_NOTOPMOST, HWND_TOPMOST,
    IDC_ARROW, IDC_HAND, SM_CXSCREEN,
    SM_CYSCREEN, SW_HIDE, SW_SHOWNORMAL, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_PAINT, WM_SETCURSOR, WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW,
    WS_SYSMENU,
};
use windows::Win32::Foundation::HINSTANCE;

use crate::drag::{post, PAUSED, WM_APP_TOGGLE};
use crate::overlay::draw_centered_text;
use crate::presence::mark_pixels;
use crate::savings::{self, format_minutes};

static WINDOW: AtomicIsize = AtomicIsize::new(0);
static HOVER: AtomicBool = AtomicBool::new(false);
static BUTTON: Mutex<(i32, i32, i32, i32)> = Mutex::new((0, 0, 0, 0));

pub fn create(instance: HINSTANCE) -> Result<(), String> {
    unsafe {
        let class_name = w!("CinderDashboard");
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(dashboard_proc),
            hInstance: instance,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: class_name,
            ..WNDCLASSEXW::default()
        };
        if RegisterClassExW(&class) == 0 && WINDOW.load(Ordering::SeqCst) == 0 {
            return Err("Impossible d'ouvrir le tableau de bord".to_string());
        }
        let scale = drag_scale();
        let width = (440.0 * scale) as i32;
        let height = (640.0 * scale) as i32;
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("Cinder"),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            (screen_w - width) / 2,
            (screen_h - height) / 3,
            width,
            height,
            None,
            None::<HMENU>,
            Some(instance),
            None,
        )
        .map_err(|error| error.to_string())?;
        WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
    }
    Ok(())
}

pub fn show() {
    let hwnd = window();
    if hwnd.0.is_null() {
        return;
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let foreground = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground, None);
        let current = GetCurrentThreadId();
        let attached = !foreground.0.is_null()
            && foreground_thread != current
            && AttachThreadInput(foreground_thread, current, true).as_bool();
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
        let _ = SetForegroundWindow(hwnd);
        if attached {
            let _ = AttachThreadInput(foreground_thread, current, false);
        }
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub fn redraw() {
    let hwnd = window();
    if hwnd.0.is_null() {
        return;
    }
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

fn window() -> HWND {
    HWND(WINDOW.load(Ordering::SeqCst) as *mut core::ffi::c_void)
}

unsafe extern "system" fn dashboard_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = point(lparam);
            let inside = button_contains(x, y);
            if HOVER.swap(inside, Ordering::Relaxed) != inside {
                redraw();
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = point(lparam);
            if button_contains(x, y) {
                post(WM_APP_TOGGLE, 0, 0);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            if HOVER.load(Ordering::Relaxed) {
                if let Ok(cursor) = unsafe { LoadCursorW(None, IDC_HAND) } {
                    unsafe { SetCursor(Some(cursor)) };
                    return LRESULT(1);
                }
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_CLOSE => {
            unsafe {
                let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            WINDOW.store(0, Ordering::SeqCst);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut paint);
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        let scale = (GetDpiForWindow(hwnd).max(96) as f32) / 96.0;
        let running = !PAUSED.load(Ordering::Relaxed);
        let hover = HOVER.load(Ordering::Relaxed);
        let pixels = render(width, height, scale, running, hover, savings::saved_minutes());
        blit(hdc, &pixels, width, height);
        let _ = EndPaint(hwnd, &paint);
    }
}

fn render(width: u32, height: u32, scale: f32, running: bool, hover: bool, minutes: u64) -> Vec<u8> {
    let mut pixmap = Pixmap::new(width, height).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    pixmap.fill(Color::from_rgba8(246, 241, 234, 255));
    let (bx, by, bw, bh) = button_rect(width, height, scale);
    store_button(bx, by, bw, bh);
    let mark = (68.0 * scale).round().clamp(32.0, 128.0) as u32;
    let icon = mark_pixels(mark, running);
    let mark_x = (width as i32 - mark as i32) / 2;
    let mark_y = (26.0 * scale) as i32;
    blit_rgba(&mut pixmap, &icon, mark, mark_x, mark_y);
    let center = width as f32 / 2.0;
    let title_y = mark_y as f32 + mark as f32 + 28.0 * scale;
    draw_centered_text(&mut pixmap, "Cinder", center, title_y, 22.0 * scale, [32, 24, 20]);
    let number_y = title_y + 92.0 * scale;
    draw_centered_text(
        &mut pixmap,
        &format_minutes(minutes),
        center,
        number_y,
        84.0 * scale,
        [28, 20, 16],
    );
    draw_centered_text(
        &mut pixmap,
        "minutes gagnées",
        center,
        number_y + 42.0 * scale,
        18.0 * scale,
        [92, 78, 68],
    );
    draw_centered_text(
        &mut pixmap,
        "2 minutes par fichier converti",
        center,
        number_y + 70.0 * scale,
        13.0 * scale,
        [140, 124, 112],
    );
    let fill = if running {
        if hover { [255, 122, 72] } else { [255, 86, 34] }
    } else if hover {
        [58, 48, 42]
    } else {
        [42, 32, 28]
    };
    fill_pill(&mut pixmap, bx as f32, by as f32, bw as f32, bh as f32, fill);
    let label = if running { "ON" } else { "OFF" };
    let detail = if running { "Activé  ·  cliquer pour couper" } else { "En pause  ·  cliquer pour activer" };
    draw_centered_text(&mut pixmap, label, center, by as f32 + bh as f32 * 0.38, 28.0 * scale, [255, 247, 242]);
    draw_centered_text(&mut pixmap, detail, center, by as f32 + bh as f32 * 0.72, 13.0 * scale, [255, 224, 210]);
    pixmap.take()
}

fn button_rect(width: u32, height: u32, scale: f32) -> (i32, i32, i32, i32) {
    let margin = (32.0 * scale) as i32;
    let button_h = (104.0 * scale) as i32;
    let bottom = (28.0 * scale) as i32;
    let y = (height as i32 - button_h - bottom).max(margin);
    (margin, y, (width as i32 - margin * 2).max(40), button_h.max(40))
}

fn button_contains(x: i32, y: i32) -> bool {
    let (bx, by, bw, bh) = *BUTTON.lock().unwrap_or_else(|error| error.into_inner());
    x >= bx && y >= by && x < bx + bw && y < by + bh
}

fn store_button(x: i32, y: i32, width: i32, height: i32) {
    *BUTTON.lock().unwrap_or_else(|error| error.into_inner()) = (x, y, width, height);
}

fn point(lparam: LPARAM) -> (i32, i32) {
    let packed = lparam.0 as u32;
    ((packed & 0xffff) as i16 as i32, (packed >> 16) as i16 as i32)
}

fn drag_scale() -> f32 {
    unsafe {
        let dpi = GetDpiForWindow(HWND::default());
        if dpi >= 96 { dpi as f32 / 96.0 } else { 1.0 }
    }
}

fn fill_pill(pixmap: &mut Pixmap, x: f32, y: f32, width: f32, height: f32, color: [u8; 3]) {
    let radius = height / 2.0;
    fill_rect(pixmap, x + radius, y, (width - height).max(0.0), height, color);
    fill_circle(pixmap, x + radius, y + radius, radius, color);
    fill_circle(pixmap, x + width - radius, y + radius, radius, color);
}

fn fill_rect(pixmap: &mut Pixmap, x: f32, y: f32, width: f32, height: f32, color: [u8; 3]) {
    let mut builder = PathBuilder::new();
    builder.move_to(x, y);
    builder.line_to(x + width, y);
    builder.line_to(x + width, y + height);
    builder.line_to(x, y + height);
    builder.close();
    fill_path(pixmap, builder, color);
}

fn fill_circle(pixmap: &mut Pixmap, x: f32, y: f32, radius: f32, color: [u8; 3]) {
    let mut builder = PathBuilder::new();
    builder.push_circle(x, y, radius);
    fill_path(pixmap, builder, color);
}

fn fill_path(pixmap: &mut Pixmap, builder: PathBuilder, color: [u8; 3]) {
    let Some(path) = builder.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn blit_rgba(pixmap: &mut Pixmap, image: &[u8], size: u32, left: i32, top: i32) {
    let width = pixmap.width() as i32;
    let height = pixmap.height() as i32;
    let row = pixmap.width();
    let pixels = pixmap.data_mut();
    for y in 0..size as i32 {
        for x in 0..size as i32 {
            let dx = left + x;
            let dy = top + y;
            if dx < 0 || dy < 0 || dx >= width || dy >= height {
                continue;
            }
            let source = ((y as u32 * size + x as u32) * 4) as usize;
            let alpha = image[source + 3] as u32;
            if alpha == 0 {
                continue;
            }
            let dest = ((dy as u32 * row + dx as u32) * 4) as usize;
            let inverse = 255 - alpha;
            for channel in 0..3 {
                pixels[dest + channel] =
                    ((image[source + channel] as u32 * alpha + pixels[dest + channel] as u32 * inverse) / 255) as u8;
            }
            pixels[dest + 3] = 255;
        }
    }
}

fn blit(hdc: windows::Win32::Graphics::Gdi::HDC, rgba: &[u8], width: u32, height: u32) {
    unsafe {
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..BITMAPINFOHEADER::default()
            },
            ..BITMAPINFO::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(memory);
            ReleaseDC(None, screen);
            return;
        };
        let destination = bits.cast::<u8>();
        if !destination.is_null() {
            for index in 0..(width * height) as usize {
                let offset = index * 4;
                *destination.add(offset) = rgba[offset + 2];
                *destination.add(offset + 1) = rgba[offset + 1];
                *destination.add(offset + 2) = rgba[offset];
                *destination.add(offset + 3) = 255;
            }
        }
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        let _ = BitBlt(hdc, 0, 0, width as i32, height as i32, Some(memory), 0, 0, SRCCOPY);
        SelectObject(memory, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
}

#[cfg(test)]
mod tests {
    use super::button_rect;

    #[test]
    fn the_switch_sits_on_the_lower_part_of_the_board() {
        let (x, y, width, height) = button_rect(440, 640, 1.0);
        assert!(y > 400, "{y}");
        assert!(width > 300, "{width}");
        assert!(height > 80, "{height}");
        assert!(x > 0);
    }
}
