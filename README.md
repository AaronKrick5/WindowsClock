# WindowsClock

A small clock overlay for Windows 11. It shows a clock that is always visible on the
monitors you select, and on no other monitor.

Status: early development. Build M2 shows a clock on the monitors you select, has a tray icon with a menu, and can hide the clock for full-screen applications. Appearance and format settings come in later builds.

## Why

Windows 11 applies taskbar auto-hide to all monitors. If you hide the taskbar to protect an
OLED monitor from burn-in, the clock on your other monitors is hidden too. WindowsClock gives
a clock on the monitors you select, and it never draws on the others.

## Technology

- Rust, with Microsoft's `windows` crate. Only documented Windows interfaces.
- Targets: Windows 11, x64 and ARM64. ARM64 builds are not tested yet.
- All `unsafe` code is in the `src/win.rs` module.

## Development builds

Each change is built by GitHub Actions. Development builds are on the Actions page as
artifacts. They are not signed, so Windows shows a confirmation before the first start.

## Build from source

```
cargo build --release --target x86_64-pc-windows-msvc
```

The compiler version is fixed in `rust-toolchain.toml`.

## Component checks

The automated build checks all components for security advisories and for licenses
(`deny.toml`). Dependabot proposes component updates each week.

## License

MIT. See `LICENSE`.
