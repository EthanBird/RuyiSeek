//! Desktop integration primitives that can be tested without a display server.

pub mod hotkey;

#[cfg(all(target_os = "linux", feature = "x11"))]
pub mod x11_clipboard;
#[cfg(all(target_os = "linux", feature = "x11"))]
pub mod x11_hotkey;
