//! Text that identifies this build.

/// Version from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Processor type that this build is for (for example `x86_64` or `aarch64`).
pub const ARCH: &str = std::env::consts::ARCH;

/// One short text for the M0 message window.
pub fn summary() -> String {
    format!("WindowsClock {VERSION} ({ARCH})\nBuild M0: build and download test.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_contains_version_and_arch() {
        let s = summary();
        assert!(s.contains(VERSION));
        assert!(s.contains(ARCH));
    }
}
