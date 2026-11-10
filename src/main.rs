//! WindowsClock: a clock overlay for Windows 11.
//!
//! Build M1: a clock window on each selected monitor. The selection is saved.

// No console window on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]
// All `unsafe` code must be in the `win` module.
#![deny(unsafe_op_in_unsafe_fn)]
// On other systems only the tests use this code.
#![cfg_attr(not(windows), allow(dead_code))]

mod buildinfo;
mod config;
mod geometry;
mod logging;
mod monitors;
mod render;

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod win;

fn main() {
    #[cfg(windows)]
    app::run();

    #[cfg(not(windows))]
    println!(
        "{}\nThis program only operates on Windows 11.",
        buildinfo::summary()
    );
}
