//! Safe wrappers for Windows calls.
//!
//! This is the only module that is permitted to contain `unsafe` code.
//! Only documented Windows interfaces are used.

use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
use windows::core::HSTRING;

/// Show a simple information window and wait until the user closes it.
pub fn message_box(title: &str, text: &str) {
    let title = HSTRING::from(title);
    let text = HSTRING::from(text);
    // SAFETY: both strings are valid, null-terminated UTF-16 strings that
    // live until the call returns. No owner window is given.
    unsafe {
        let _ = MessageBoxW(None, &text, &title, MB_OK | MB_ICONINFORMATION);
    }
}
