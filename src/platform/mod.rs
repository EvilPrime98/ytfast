//! OS / IO specific helpers.

pub mod logging;
pub mod single_instance;

#[cfg(windows)]
pub mod tray;
#[cfg(windows)]
pub mod window;
