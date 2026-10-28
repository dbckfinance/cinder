#![allow(unsafe_op_in_unsafe_fn)]

use std::sync::OnceLock;

use fontdue::Font;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, MonitorFromPoint, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, HGDIOBJ,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassExW, ShowWindow, UpdateLayeredWindow, SW_HIDE, SW_SHOWNOACTIVATE,
    ULW_ALPHA, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};
use windows::core::w;

const DISK_RADIUS: f32 = 150.0;
const HOLE_RADIUS: f32 = 52.0;
const OUTER_SLACK: f32 = 18.0;

pub struct WheelView {
    pub title: String,
    pub caption: String,
    pub labels: Vec<String>,
    pub hover: Option<usize>,
    pub scale: f32,
}

pub fn geometry(scale: f32) -> (f32, f32, u32) {
    let radius = DISK_RADIUS * scale;
    let dead = HOLE_RADIUS * scale;
    let outer = radius + OUTER_SLACK * scale;
    let window = (outer * 2.0 + 64.0 * scale).ceil() as u32;
    (dead, outer, window.max(1))
}

pub fn hit_index(dx: f32, dy: f32, count: usize, dead: f32, outer: f32) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < dead || distance > outer {
        return None;
    }
    let mut from_top = dx.atan2(-dy);
    if from_top < 0.0 {
        from_top += std::f32::consts::TAU;
    }
    let segment = std::f32::consts::TAU / count as f32;
    Some(((from_top / segment) as usize).min(count - 1))
}

pub fn scale_at(x: i32, y: i32) -> f32 {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).is_ok() && dpi_x >= 96 {
            return dpi_x as f32 / 96.0;
        }
    }
    1.0
}

pub fn render_wheel(view: &WheelView) -> (Vec<u8>, u32, u32) {
    let scale = view.scale.max(1.0);
    let (_dead, _outer, size) = geometry(scale);
    let mut pixmap = Pixmap::new(size, size).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    pixmap.fill(Color::from_rgba8(0, 0, 0, 0));
    let center = size as f32 / 2.0;
    let radius = DISK_RADIUS * scale;
    let hole = HOLE_RADIUS * scale;

    fill_circle(&mut pixmap, center, center + 6.0 * scale, radius + 8.0 * scale, [20, 20, 24, 40]);
    fill_circle(&mut pixmap, center, center, radius, [246, 246, 248, 248]);

    let count = view.labels.len();
    if let Some(hover) = view.hover.filter(|index| *index < count) {
        fill_segment(&mut pixmap, center, radius - 6.0 * scale, count, hover, [255, 92, 36, 255]);
    }
    fill_circle(&mut pixmap, center, center, hole, [255, 255, 255, 255]);
    stroke_circle(&mut pixmap, center, center, radius - 1.0 * scale, 1.0 * scale, [220, 220, 224, 255]);

    if count > 0 {
        let segment = std::f32::consts::TAU / count as f32;
        for (index, label) in view.labels.iter().enumerate() {
            let angle = segment * (index as f32 + 0.5);
            let label_radius = (hole + radius) * 0.5 + 8.0 * scale;
            let x = center + label_radius * angle.sin();
            let y = center - label_radius * angle.cos();
            let color = if view.hover == Some(index) {
                [255, 255, 255]
            } else {
                [42, 42, 46]
            };
            let label_size = if count >= 10 { 11.0 } else { 14.0 } * scale;
            draw_text(&mut pixmap, label, x, y, label_size, color);
        }
    }

    draw_text(&mut pixmap, &view.title, center, center - 2.0 * scale, 16.0 * scale, [28, 28, 32]);
    draw_text(
        &mut pixmap,
        &view.caption,
        center,
        center + radius + 18.0 * scale,
        13.0 * scale,
        [70, 70, 76],
    );

    (pixmap.take(), size, size)
}

pub fn render_toast(text: &str, scale: f32) -> (Vec<u8>, u32, u32) {
    let scale = scale.max(1.0);
    let text_size = 15.0 * scale;
    let width = (measure_text(text, text_size) + 36.0 * scale).clamp(120.0 * scale, 460.0 * scale);
    let height = 46.0 * scale;
    let pixel_width = width.ceil() as u32;
    let pixel_height = height.ceil() as u32;
    let mut pixmap = Pixmap::new(pixel_width.max(1), pixel_height.max(1)).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
    pixmap.fill(Color::from_rgba8(0, 0, 0, 0));
    let radius = height / 2.0;
    fill_circle(&mut pixmap, radius, radius, radius, [24, 18, 14, 235]);
    fill_circle(&mut pixmap, width - radius, radius, radius, [24, 18, 14, 235]);
    fill_rect(&mut pixmap, radius, 0.0, width - height, height, [24, 18, 14, 235]);
    draw_text(&mut pixmap, text, width / 2.0, height / 2.0, text_size, [246, 237, 228]);
    (pixmap.take(), pixel_width, pixel_height)
}

pub fn create_overlay(instance: HINSTANCE) -> Result<HWND, String> {
    unsafe {
        let class_name = w!("CinderWheel");
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_proc),
            hInstance: instance,
            lpszClassName: class_name,
            ..WNDCLASSEXW::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err("Impossible de créer la roue".to_string());
        }
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name,
            w!("Cinder"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|error| error.to_string())
    }
}

pub fn present(hwnd: HWND, center: (i32, i32), rgba: &[u8], width: u32, height: u32) {
    if width == 0 || height == 0 || rgba.len() < (width * height * 4) as usize {
        return;
    }
    unsafe {
        let screen = GetDC(None);
        if screen.0.is_null() {
            return;
        }
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
        let bitmap = match CreateDIBSection(Some(memory), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) => bitmap,
            Err(_) => {
                let _ = DeleteDC(memory);
                ReleaseDC(None, screen);
                return;
            }
        };
        swizzle_premultiplied(rgba, bits.cast(), (width * height) as usize);
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        let destination = POINT {
            x: center.0 - width as i32 / 2,
            y: center.1 - height as i32 / 2,
        };
        let size = SIZE {
            cx: width as i32,
            cy: height as i32,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let _ = UpdateLayeredWindow(
            hwnd,
            Some(screen),
            Some(&destination as *const POINT),
            Some(&size as *const SIZE),
            Some(memory),
            Some(&source as *const POINT),
            COLORREF(0),
            Some(&blend as *const BLENDFUNCTION),
            ULW_ALPHA,
        );
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SelectObject(memory, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
    }
}

pub fn hide(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn swizzle_premultiplied(rgba: &[u8], destination: *mut u8, pixels: usize) {
    if destination.is_null() {
        return;
    }
    for index in 0..pixels {
        let offset = index * 4;
        unsafe {
            *destination.add(offset) = rgba[offset + 2];
            *destination.add(offset + 1) = rgba[offset + 1];
            *destination.add(offset + 2) = rgba[offset];
            *destination.add(offset + 3) = rgba[offset + 3];
        }
    }
}

fn fill_circle(pixmap: &mut Pixmap, x: f32, y: f32, radius: f32, color: [u8; 4]) {
    let mut builder = PathBuilder::new();
    builder.push_circle(x, y, radius);
    let Some(path) = builder.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn stroke_circle(pixmap: &mut Pixmap, x: f32, y: f32, radius: f32, width: f32, color: [u8; 4]) {
    let mut builder = PathBuilder::new();
    builder.push_circle(x, y, radius);
    let Some(path) = builder.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

fn fill_rect(pixmap: &mut Pixmap, x: f32, y: f32, width: f32, height: f32, color: [u8; 4]) {
    let mut builder = PathBuilder::new();
    builder.move_to(x, y);
    builder.line_to(x + width, y);
    builder.line_to(x + width, y + height);
    builder.line_to(x, y + height);
    builder.close();
    let Some(path) = builder.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn fill_segment(pixmap: &mut Pixmap, center: f32, radius: f32, count: usize, index: usize, color: [u8; 4]) {
    if count == 0 {
        return;
    }
    let segment = std::f32::consts::TAU / count as f32;
    let start = segment * index as f32 + 0.045;
    let end = segment * (index as f32 + 1.0) - 0.045;
    let mut builder = PathBuilder::new();
    builder.move_to(center, center);
    let steps = 28;
    for step in 0..=steps {
        let angle = start + (end - start) * (step as f32 / steps as f32);
        builder.line_to(center + radius * angle.sin(), center - radius * angle.cos());
    }
    builder.close();
    let Some(path) = builder.finish() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
    paint.anti_alias = true;
    pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

pub(crate) fn draw_centered_text(pixmap: &mut Pixmap, text: &str, center_x: f32, center_y: f32, px: f32, color: [u8; 3]) {
    draw_text(pixmap, text, center_x, center_y, px, color);
}

fn draw_text(pixmap: &mut Pixmap, text: &str, center_x: f32, center_y: f32, px: f32, color: [u8; 3]) {
    let Some(font) = ui_font() else {
        return;
    };
    let mut glyphs = Vec::new();
    let mut width = 0.0f32;
    for character in text.chars() {
        let (metrics, bitmap) = font.rasterize(character, px);
        width += metrics.advance_width;
        glyphs.push((metrics, bitmap));
    }
    let mut pen = center_x - width / 2.0;
    let baseline = center_y + px * 0.32;
    for (metrics, bitmap) in glyphs {
        blit_glyph(pixmap, &metrics, &bitmap, pen, baseline, color);
        pen += metrics.advance_width;
    }
}

fn measure_text(text: &str, px: f32) -> f32 {
    let Some(font) = ui_font() else {
        return text.chars().count() as f32 * px * 0.55;
    };
    text.chars()
        .map(|character| font.rasterize(character, px).0.advance_width)
        .sum()
}

fn blit_glyph(
    pixmap: &mut Pixmap,
    metrics: &fontdue::Metrics,
    bitmap: &[u8],
    pen_x: f32,
    baseline: f32,
    color: [u8; 3],
) {
    let left = pen_x as i32 + metrics.xmin;
    let top = baseline as i32 - metrics.ymin - metrics.height as i32;
    let pixmap_width = pixmap.width() as i32;
    let pixmap_height = pixmap.height() as i32;
    let row_stride = pixmap.width();
    let pixels = pixmap.data_mut();
    for row in 0..metrics.height {
        for column in 0..metrics.width {
            let coverage = bitmap[row * metrics.width + column];
            if coverage == 0 {
                continue;
            }
            let x = left + column as i32;
            let y = top + row as i32;
            if x < 0 || y < 0 || x >= pixmap_width || y >= pixmap_height {
                continue;
            }
            let offset = ((y as u32 * row_stride + x as u32) * 4) as usize;
            let alpha = coverage as u32;
            let inverse = 255 - alpha;
            let source_r = color[0] as u32 * alpha / 255;
            let source_g = color[1] as u32 * alpha / 255;
            let source_b = color[2] as u32 * alpha / 255;
            pixels[offset] = (source_r + pixels[offset] as u32 * inverse / 255) as u8;
            pixels[offset + 1] = (source_g + pixels[offset + 1] as u32 * inverse / 255) as u8;
            pixels[offset + 2] = (source_b + pixels[offset + 2] as u32 * inverse / 255) as u8;
            pixels[offset + 3] = (alpha + pixels[offset + 3] as u32 * inverse / 255) as u8;
        }
    }
}

fn ui_font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| {
        let bytes = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf")
            .or_else(|_| std::fs::read(r"C:\Windows\Fonts\arial.ttf"))
            .ok()?;
        Font::from_bytes(bytes, fontdue::FontSettings::default()).ok()
    })
    .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadrants_select_the_matching_segment() {
        let dead = 40.0;
        let outer = 180.0;
        assert_eq!(hit_index(0.0, -100.0, 4, dead, outer), Some(0));
        assert_eq!(hit_index(100.0, 0.0, 4, dead, outer), Some(1));
        assert_eq!(hit_index(0.0, 100.0, 4, dead, outer), Some(2));
        assert_eq!(hit_index(-100.0, 0.0, 4, dead, outer), Some(3));
        assert_eq!(hit_index(0.0, -10.0, 4, dead, outer), None);
        assert_eq!(hit_index(0.0, -400.0, 4, dead, outer), None);
        assert_eq!(hit_index(0.0, -100.0, 0, dead, outer), None);
    }

    #[test]
    fn wheel_pixels_are_not_empty() {
        let (pixels, width, height) = render_wheel(&WheelView {
            title: "WebP".to_string(),
            caption: "Convertir en WebP".to_string(),
            labels: vec!["PNG".to_string(), "WebP".to_string(), "GIF".to_string()],
            hover: Some(1),
            scale: 1.0,
        });
        assert_eq!(pixels.len(), (width * height * 4) as usize);
        assert!(pixels.chunks(4).any(|pixel| pixel[3] > 0));
    }
}
