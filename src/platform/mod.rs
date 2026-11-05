//! OS / IO specific helpers.

pub mod logging;
pub mod single_instance;

#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod window;

pub fn native_handle(window: &gpui::Window) -> Option<isize> {
    #[cfg(windows)]
    {
        window::native(window).map(|(hwnd, _)| hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}
