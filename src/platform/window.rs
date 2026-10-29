use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongW, GetWindowRect, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
    LWA_ALPHA, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SetLayeredWindowAttributes, SetWindowLongW, SetWindowPos, ShowWindow, WS_EX_LAYERED,
};

fn hwnd(window: &Window) -> Option<HWND> {
    match HasWindowHandle::window_handle(window).ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as _),
        _ => None,
    }
}

pub fn hide(window: &Window) -> bool {
    let Some(hwnd) = hwnd(window) else {
        return false;
    };
    unsafe { ShowWindow(hwnd, SW_HIDE) };
    true
}

pub fn show(window: &Window) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else if IsWindowVisible(hwnd) != 0 || ANIMATING.swap(true, Ordering::AcqRel) {
            ShowWindow(hwnd, SW_SHOW);
        } else {
            animate_show(hwnd);
        }
    }
}

static ANIMATING: AtomicBool = AtomicBool::new(false);

const FRAMES: u32 = 11;
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const START_SCALE: f32 = 0.96;

unsafe fn animate_show(hwnd: HWND) {
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            ShowWindow(hwnd, SW_SHOW);
            ANIMATING.store(false, Ordering::Release);
            return;
        }
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        let zoomed = IsZoomed(hwnd) != 0;

        SetWindowLongW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED as i32);
        SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA);
        if !zoomed {
            set_bounds(hwnd, scaled(rect, START_SCALE));
        }
        ShowWindow(hwnd, SW_SHOW);

        let raw = hwnd as isize;
        std::thread::spawn(move || {
            let hwnd = raw as HWND;
            for frame in 1..=FRAMES {
                std::thread::sleep(FRAME_INTERVAL);
                if IsWindow(hwnd) == 0 || IsWindowVisible(hwnd) == 0 {
                    break;
                }
                let p = frame as f32 / FRAMES as f32;
                let eased = 1.0 - (1.0 - p).powi(3);
                SetLayeredWindowAttributes(hwnd, 0, (255.0 * eased) as u8, LWA_ALPHA);
                if !zoomed {
                    let scale = START_SCALE + (1.0 - START_SCALE) * eased;
                    set_bounds(hwnd, scaled(rect, scale));
                }
            }
            if IsWindow(hwnd) != 0 {
                if !zoomed {
                    set_bounds(hwnd, rect);
                }
                SetWindowLongW(hwnd, GWL_EXSTYLE, style);
            }
            ANIMATING.store(false, Ordering::Release);
        });
    }
}

fn scaled(rect: RECT, scale: f32) -> RECT {
    let width = (rect.right - rect.left) as f32;
    let height = (rect.bottom - rect.top) as f32;
    let dx = (width * (1.0 - scale) / 2.0).round() as i32;
    let dy = (height * (1.0 - scale) / 2.0).round() as i32;
    RECT {
        left: rect.left + dx,
        top: rect.top + dy,
        right: rect.right - dx,
        bottom: rect.bottom - dy,
    }
}

unsafe fn set_bounds(hwnd: HWND, rect: RECT) {
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}
