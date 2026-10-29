use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsIconic, SW_HIDE, SW_RESTORE, SW_SHOW, ShowWindow,
};

fn hwnd(window: &Window) -> Option<windows_sys::Win32::Foundation::HWND> {
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
        let cmd = if IsIconic(hwnd) != 0 {
            SW_RESTORE
        } else {
            SW_SHOW
        };
        ShowWindow(hwnd, cmd);
    }
}
