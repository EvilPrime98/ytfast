use std::cell::Cell;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetCursorPos, GetForegroundWindow, HWND_TOP, SWP_NOACTIVATE,
    SetWindowPos, WS_CLIPCHILDREN, WS_DISABLED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
    WS_VISIBLE,
};

pub struct Surface {
    hwnd: HWND,
    parent: HWND,
    placed: Cell<(i32, i32, i32, i32)>,
}

impl Surface {
    pub fn new(parent: isize) -> Result<Self, String> {
        let parent = parent as HWND;
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                std::ptr::null(),
                WS_POPUP | WS_VISIBLE | WS_DISABLED | WS_CLIPCHILDREN,
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
        Ok(Self {
            hwnd,
            parent,
            placed: Cell::new((0, 0, 0, 0)),
        })
    }

    pub fn id(&self) -> isize {
        self.hwnd as isize
    }

    pub fn place(&self, x: i32, y: i32, width: i32, height: i32) {
        let mut origin = POINT { x, y };
        if unsafe { ClientToScreen(self.parent, &mut origin) } == 0 {
            return;
        }
        let rect = (origin.x, origin.y, width.max(1), height.max(1));
        if rect == self.placed.get() {
            return;
        }
        self.placed.set(rect);
        unsafe {
            SetWindowPos(
                self.hwnd,
                HWND_TOP,
                rect.0,
                rect.1,
                rect.2,
                rect.3,
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
