#![allow(unsafe_op_in_unsafe_fn)]

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_UP, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::drag::{
    self, shell_place, DRAG_ACTIVE, HOVER, KEY_WHEEL, PAUSED, SHELL_NONE, WHEEL_OPEN, WM_APP_CANCEL, WM_APP_DROP,
    WM_APP_OPEN,
};

static HELD: [AtomicBool; 3] = [AtomicBool::new(false), AtomicBool::new(false), AtomicBool::new(false)];
static KEY_HOOK: AtomicIsize = AtomicIsize::new(0);

pub fn release_modifiers() {
    for slot in &HELD {
        slot.store(false, Ordering::Relaxed);
    }
}

pub fn install_keyboard_hook() -> Result<HHOOK, String> {
    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), None, 0).map_err(|error| error.to_string())? };
    KEY_HOOK.store(hook.0 as isize, Ordering::SeqCst);
    Ok(hook)
}

pub fn remove_keyboard_hook() {
    let previous = KEY_HOOK.swap(0, Ordering::SeqCst);
    if previous != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(previous as *mut core::ffi::c_void));
        }
    }
}

pub fn reinstall_keyboard_hook() {
    remove_keyboard_hook();
    let _ = install_keyboard_hook();
}

fn modifier_slot(virtual_key: u32) -> Option<usize> {
    let shift = [VK_SHIFT.0 as u32, VK_LSHIFT.0 as u32, VK_RSHIFT.0 as u32];
    let alt = [VK_MENU.0 as u32, VK_LMENU.0 as u32, VK_RMENU.0 as u32];
    let control = [VK_CONTROL.0 as u32, VK_LCONTROL.0 as u32, VK_RCONTROL.0 as u32];
    if shift.contains(&virtual_key) {
        Some(0)
    } else if alt.contains(&virtual_key) {
        Some(1)
    } else if control.contains(&virtual_key) {
        Some(2)
    } else {
        None
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }
    drag::note_hook_call();
    let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    let released = info.flags.contains(LLKHF_UP);
    let message = wparam.0 as u32;
    let key_event = matches!(message, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP);
    if key_event && !PAUSED.load(Ordering::Relaxed) {
        if info.vkCode == VK_ESCAPE.0 as u32 && WHEEL_OPEN.load(Ordering::Relaxed) {
            if !released {
                drag::post(WM_APP_CANCEL, 0, 0);
            }
            return LRESULT(1);
        }
        if let Some(slot) = modifier_slot(info.vkCode) {
            if DRAG_ACTIVE.load(Ordering::SeqCst) {
                if released {
                    HELD[slot].store(false, Ordering::Relaxed);
                } else if !HELD[slot].swap(true, Ordering::Relaxed) {
                    drag::post(WM_APP_OPEN, slot, 0);
                }
                return LRESULT(1);
            }
            let place = shell_place();
            if place != SHELL_NONE {
                if released {
                    HELD[slot].store(false, Ordering::Relaxed);
                    if KEY_WHEEL.load(Ordering::SeqCst) && WHEEL_OPEN.load(Ordering::SeqCst) && HOVER.load(Ordering::Relaxed) >= 0 {
                        drag::post(WM_APP_DROP, 0, 0);
                    }
                } else if !HELD[slot].swap(true, Ordering::Relaxed) {
                    drag::SHELL_KEY.store(place, Ordering::SeqCst);
                    drag::post(WM_APP_OPEN, slot, 0);
                }
            } else {
                HELD[slot].store(false, Ordering::Relaxed);
            }
        } else if !released && KEY_WHEEL.load(Ordering::SeqCst) && WHEEL_OPEN.load(Ordering::SeqCst) {
            drag::post(WM_APP_CANCEL, 0, 0);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
