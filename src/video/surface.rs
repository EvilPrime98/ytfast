use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetCursorPos, GetForegroundWindow, HWND_TOP, SWP_NOACTIVATE,
    SetWindowPos, WS_CHILD, WS_CLIPCHILDREN, WS_DISABLED, WS_VISIBLE,
};

pub struct Surface {
    hwnd: HWND,
    parent: HWND,
}

impl Surface {
    pub fn new(parent: isize) -> Result<Self, String> {
        let parent = parent as HWND;
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_DISABLED | WS_CLIPCHILDREN,
                0,
                0,
                1,
                1,
                parent,
                null_mut(),
                null_mut(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return Err("could not create the video window".into());
        }
        Ok(Self { hwnd, parent })
    }

    pub fn id(&self) -> isize {
        self.hwnd as isize
    }

    pub fn place(&self, x: i32, y: i32, width: i32, height: i32) {
        unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOP,
                x,
                y,
                width.max(1),
                height.max(1),
                SWP_NOACTIVATE,
            );
        }
    }

    pub fn cursor(&self) -> Option<(i32, i32)> {
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            if GetForegroundWindow() != self.parent || GetCursorPos(&mut p) == 0 {
                return None;
            }
            (ScreenToClient(self.parent, &mut p) != 0).then_some((p.x, p.y))
        }
    }

    pub fn left_button_down() -> bool {
        unsafe { GetAsyncKeyState(i32::from(VK_LBUTTON)) < 0 }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.hwnd) };
    }
}
