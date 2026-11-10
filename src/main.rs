//! WindowsClock: a clock overlay for Windows 11.
//!
//! Build M0: the program only shows its version, then ends.
//! This build proves that the automated build and the download work.

// No console window on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]
// All `unsafe` code must be in the `win` module.
#![deny(unsafe_op_in_unsafe_fn)]

mod buildinfo;
#[cfg(windows)]
mod win;

fn main() {
    let text = buildinfo::summary();

    #[cfg(windows)]
    win::message_box("WindowsClock", &text);

    #[cfg(not(windows))]
    println!("{text}\nThis program only operates on Windows 11.");
}
