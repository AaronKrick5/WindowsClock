//! Safe wrappers for Windows calls.
//!
//! This is the only module that is permitted to contain `unsafe` code.
//! Only documented Windows interfaces are used. No code is injected into
//! other programs and the taskbar is not changed.

use crate::geometry::{self, Rect};
use crate::monitors::{DisplayTarget, RawMonitor};
use crate::render;
use core::ffi::c_void;
use std::mem::size_of;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes,
    QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::{
    COLORREF, ERROR_SUCCESS, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Globalization::{GetTimeFormatEx, TIME_NOSECONDS};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
    BLENDFUNCTION, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW,
    DEFAULT_CHARSET, DIB_RGB_COLORS, DeleteDC, DeleteObject, EnumDisplayMonitors, FW_SEMIBOLD,
    GdiFlush, GetMonitorInfoW, GetTextExtentPoint32W, HDC, HGDIOBJ, HMONITOR, MONITORINFO,
    MONITORINFOEXW, OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
    TextOutW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, FindWindowW, GetCursorPos, GetMessageW, HWND_TOPMOST, IDC_ARROW, IDYES,
    KillTimer, LoadCursorW, MA_NOACTIVATE, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK,
    MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    MONITORINFOF_PRIMARY, MSG, MessageBoxW, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND,
    PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetForegroundWindow, SetTimer,
    SetWindowPos, ShowWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WM_APP, WM_DESTROY, WM_DISPLAYCHANGE,
    WM_DPICHANGED, WM_MOUSEACTIVATE, WM_NULL, WM_POWERBROADCAST, WM_RBUTTONUP, WM_SETTINGCHANGE,
    WM_TIMECHANGE, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

// ---------------------------------------------------------------------------
// Events for the application
// ---------------------------------------------------------------------------

/// What the application must do. The window procedures translate Windows
/// messages into these events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// The displayed time can be out of date. Draw again and set the next timer.
    Tick,
    /// Monitors or settings of Windows changed. Read the monitor list again.
    Rebuild,
    /// The user did a right-click on a clock.
    Menu,
    /// The user started the program a second time. Ask for the monitor selection.
    Select,
}

type Handler = fn(Event);

static HANDLER: OnceLock<Handler> = OnceLock::new();
static CONTROLLER: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

const CONTROLLER_CLASS: PCWSTR = w!("WindowsClock.Controller");
const CLOCK_CLASS: PCWSTR = w!("WindowsClock.Clock");

const TIMER_TICK: usize = 1;
const TIMER_REBUILD: usize = 2;
/// Display changes come in groups. Wait this long, then read the monitor list one time.
const REBUILD_DELAY_MS: u32 = 300;

const WM_APP_MENU: u32 = WM_APP + 1;
const WM_APP_SELECT: u32 = WM_APP + 2;
const WM_APP_REBUILD: u32 = WM_APP + 3;

fn hwnd_from(value: isize) -> HWND {
    HWND(value as *mut c_void)
}

fn controller() -> Option<HWND> {
    let v = CONTROLLER.load(Ordering::Relaxed);
    if v == 0 { None } else { Some(hwnd_from(v)) }
}

fn dispatch(event: Event) {
    if let Some(handler) = HANDLER.get() {
        handler(event);
    }
}

// ---------------------------------------------------------------------------
// Start, message loop, stop
// ---------------------------------------------------------------------------

/// If the program already runs for this user, tell it to ask for the monitor
/// selection. Returns true if a running program was found.
pub fn notify_existing_instance() -> bool {
    // SAFETY: the class name is a valid static string. The window name is null.
    let found = unsafe { FindWindowW(CONTROLLER_CLASS, PCWSTR::null()) };
    match found {
        Ok(hwnd) if !hwnd.0.is_null() => {
            // SAFETY: posting a message with no pointers to a window handle is safe.
            let _ = unsafe { PostMessageW(Some(hwnd), WM_APP_SELECT, WPARAM(0), LPARAM(0)) };
            true
        }
        _ => false,
    }
}

/// Prepare the process and create the hidden controller window.
pub fn init(handler: Handler) -> Result<(), String> {
    let _ = HANDLER.set(handler);

    // SAFETY: all calls below receive valid arguments. Pointers are to local
    // structures that live until each call returns.
    unsafe {
        // Each monitor has its own scaling value. Without this call Windows
        // would scale the window and the text would not be sharp.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        TASKBAR_CREATED.store(
            RegisterWindowMessageW(w!("TaskbarCreated")),
            Ordering::Relaxed,
        );

        let module = GetModuleHandleW(PCWSTR::null()).map_err(|e| e.to_string())?;
        let instance: HINSTANCE = module.into();
        let cursor = LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?;

        let controller_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(controller_proc),
            hInstance: instance,
            lpszClassName: CONTROLLER_CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&controller_class) == 0 {
            return Err("cannot register controller window class".into());
        }

        let clock_class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(clock_proc),
            hInstance: instance,
            hCursor: cursor,
            lpszClassName: CLOCK_CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&clock_class) == 0 {
            return Err("cannot register clock window class".into());
        }

        // A hidden top-level window. It receives the Windows notifications
        // for display, time, and power changes. It is never shown.
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            CONTROLLER_CLASS,
            w!("WindowsClock"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|e| e.to_string())?;
        CONTROLLER.store(hwnd.0 as isize, Ordering::Relaxed);
    }
    Ok(())
}

/// Process Windows messages until the program stops.
pub fn run_loop() {
    let mut msg = MSG::default();
    // SAFETY: `msg` is a valid structure for the full loop.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Stop the program.
pub fn quit() {
    if let Some(hwnd) = controller() {
        // SAFETY: the handle is the controller window of this thread.
        let _ = unsafe { DestroyWindow(hwnd) };
    }
}

/// Ask for an `Event::Tick` after the given time.
pub fn set_tick_timer(milliseconds: u32) {
    if let Some(hwnd) = controller() {
        // SAFETY: valid window handle, no callback function.
        unsafe { SetTimer(Some(hwnd), TIMER_TICK, milliseconds.max(10), None) };
    }
}

/// Ask for an `Event::Rebuild` after a short delay. Many calls give one event.
pub fn schedule_rebuild() {
    if let Some(hwnd) = controller() {
        // SAFETY: valid window handle, no callback function.
        unsafe { SetTimer(Some(hwnd), TIMER_REBUILD, REBUILD_DELAY_MS, None) };
    }
}

/// Ask for an `Event::Select` from the message loop.
pub fn post_select_request() {
    if let Some(hwnd) = controller() {
        // SAFETY: posting a message with no pointers is safe.
        let _ = unsafe { PostMessageW(Some(hwnd), WM_APP_SELECT, WPARAM(0), LPARAM(0)) };
    }
}

unsafe extern "system" fn controller_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TIMER if wparam.0 == TIMER_TICK => {
            dispatch(Event::Tick);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_REBUILD => {
            // SAFETY: valid window handle and timer identifier.
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_REBUILD) };
            dispatch(Event::Rebuild);
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_APP_REBUILD => {
            schedule_rebuild();
            LRESULT(0)
        }
        WM_TIMECHANGE => {
            dispatch(Event::Tick);
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            let kind = wparam.0 as u32;
            if kind == PBT_APMRESUMEAUTOMATIC || kind == PBT_APMRESUMESUSPEND {
                schedule_rebuild();
                dispatch(Event::Tick);
            }
            LRESULT(1)
        }
        WM_APP_MENU => {
            dispatch(Event::Menu);
            LRESULT(0)
        }
        WM_APP_SELECT => {
            dispatch(Event::Select);
            LRESULT(0)
        }
        WM_DESTROY => {
            CONTROLLER.store(0, Ordering::Relaxed);
            // SAFETY: always safe to call on the thread of the message loop.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => {
            let taskbar_created = TASKBAR_CREATED.load(Ordering::Relaxed);
            if taskbar_created != 0 && msg == taskbar_created {
                // Windows Explorer started again.
                schedule_rebuild();
                return LRESULT(0);
            }
            // SAFETY: the arguments are the unchanged arguments of this call.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

/// Window procedure of the clock windows. It keeps no state.
unsafe extern "system" fn clock_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // A click must not give the keyboard focus to the clock.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_RBUTTONUP => {
            if let Some(ctl) = controller() {
                // SAFETY: posting a message with no pointers is safe.
                let _ = unsafe { PostMessageW(Some(ctl), WM_APP_MENU, WPARAM(0), LPARAM(0)) };
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            // The monitor arrangement changed. Windows can move this window to
            // a different monitor. Hide it immediately. The rebuild shows it
            // again only if its monitor is selected and available.
            // SAFETY: valid window handle.
            let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
            schedule_rebuild();
            LRESULT(0)
        }
        WM_DPICHANGED => {
            schedule_rebuild();
            LRESULT(0)
        }
        // SAFETY: the arguments are the unchanged arguments of this call.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// ---------------------------------------------------------------------------
// Simple dialogs
// ---------------------------------------------------------------------------

/// Show an information window and wait until the user closes it.
pub fn message_box(title: &str, text: &str) {
    let title = HSTRING::from(title);
    let text = HSTRING::from(text);
    // SAFETY: both strings are valid, null-terminated UTF-16 strings that
    // live until the call returns. No owner window is given.
    unsafe {
        let _ = MessageBoxW(
            None,
            &text,
            &title,
            MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

/// Ask a yes/no question. Returns true for yes.
pub fn ask_yes_no(title: &str, text: &str) -> bool {
    let title = HSTRING::from(title);
    let text = HSTRING::from(text);
    // SAFETY: see `message_box`.
    let answer = unsafe {
        MessageBoxW(
            None,
            &text,
            &title,
            MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND | MB_TOPMOST,
        )
    };
    answer == IDYES
}

/// One line of the right-click menu.
pub struct MenuItem {
    /// Identifier that `show_menu` returns. Must not be 0.
    pub id: usize,
    pub label: String,
    pub checked: bool,
    pub enabled: bool,
    pub separator: bool,
}

/// Show the menu at the pointer position. Returns the selected identifier.
pub fn show_menu(items: &[MenuItem]) -> Option<usize> {
    let owner = controller()?;
    // SAFETY: the menu handle is valid from creation to `DestroyMenu`. All
    // strings live until the call that uses them returns.
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for item in items {
            if item.separator {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                continue;
            }
            let mut flags = MF_STRING;
            if item.checked {
                flags |= MF_CHECKED;
            }
            if !item.enabled {
                flags |= MF_GRAYED;
            }
            let label = HSTRING::from(item.label.as_str());
            let _ = AppendMenuW(menu, flags, item.id, &label);
        }
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        // Necessary so that the menu closes when the user clicks outside it.
        let _ = SetForegroundWindow(owner);
        let choice = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            point.x,
            point.y,
            None,
            owner,
            None,
        );
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        if choice.0 > 0 {
            Some(choice.0 as usize)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

/// Current time as text in the format of the Windows regional settings,
/// without seconds.
pub fn time_text() -> String {
    let mut buffer = [0u16; 80];
    // SAFETY: null locale = user default, null time = now, null format = regional format.
    let count = unsafe {
        GetTimeFormatEx(
            PCWSTR::null(),
            TIME_NOSECONDS,
            None,
            PCWSTR::null(),
            Some(&mut buffer),
        )
    };
    if count <= 1 {
        return "--:--".to_string();
    }
    String::from_utf16_lossy(&buffer[..(count as usize - 1)])
}

/// Milliseconds until the next minute starts on the system clock.
pub fn ms_to_next_minute() -> u32 {
    // SAFETY: no arguments.
    let now = unsafe { GetLocalTime() };
    let elapsed = u32::from(now.wSecond) * 1000 + u32::from(now.wMilliseconds);
    60_000u32.saturating_sub(elapsed).max(1)
}

/// Local date and time for the log file.
pub fn timestamp() -> String {
    // SAFETY: no arguments.
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

// ---------------------------------------------------------------------------
// Monitors
// ---------------------------------------------------------------------------

fn wide_to_string(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: `data` is the pointer to the vector that `raw_monitors` passes.
    // The vector lives until `EnumDisplayMonitors` returns.
    let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    list.push(monitor);
    BOOL(1)
}

/// Positions and scaling values of all monitors of the desktop.
pub fn raw_monitors() -> Vec<RawMonitor> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only uses the vector pointer during this call.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(&mut handles as *mut Vec<HMONITOR> as isize),
        );
    }

    let mut list = Vec::new();
    for handle in handles {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: `info` is a MONITORINFOEXW and its size field says so.
        let ok = unsafe {
            GetMonitorInfoW(handle, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO)
        };
        if !ok.as_bool() {
            continue;
        }
        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        // SAFETY: valid monitor handle and pointers to local values.
        let _ = unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let r = info.monitorInfo.rcMonitor;
        list.push(RawMonitor {
            gdi_name: wide_to_string(&info.szDevice),
            rect: Rect {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            },
            dpi: dpi_x,
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    list
}

/// Hardware data of all active display outputs.
pub fn display_targets() -> Vec<DisplayTarget> {
    let mut result = Vec::new();
    // The configuration can change between the two calls. Try a few times.
    for _ in 0..3 {
        let mut path_count = 0u32;
        let mut mode_count = 0u32;
        // SAFETY: pointers to local values.
        let status = unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
        };
        if status != ERROR_SUCCESS {
            return result;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        // SAFETY: the buffers have the sizes given in the count values.
        let status = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                None,
            )
        };
        if status != ERROR_SUCCESS {
            continue;
        }
        paths.truncate(path_count as usize);

        for path in &paths {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = path.sourceInfo.adapterId;
            source.header.id = path.sourceInfo.id;
            // SAFETY: the header gives the correct type and size of the structure.
            if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0 {
                continue;
            }

            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            target.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            target.header.adapterId = path.targetInfo.adapterId;
            target.header.id = path.targetInfo.id;
            // SAFETY: the header gives the correct type and size of the structure.
            if unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } != 0 {
                continue;
            }

            result.push(DisplayTarget {
                gdi_name: wide_to_string(&source.viewGdiDeviceName),
                device_path: wide_to_string(&target.monitorDevicePath),
                friendly_name: wide_to_string(&target.monitorFriendlyDeviceName),
            });
        }
        return result;
    }
    result
}

// ---------------------------------------------------------------------------
// Clock windows
// ---------------------------------------------------------------------------

/// Font size of the clock text in points.
const FONT_POINTS: i32 = 12;
/// Distance between the clock and the monitor edges, in units of 1/96 inch.
const MARGIN: i32 = 10;
/// Space between the text and the edge of the clock window, in units of 1/96 inch.
const PADDING_X: i32 = 10;
const PADDING_Y: i32 = 4;
/// Opacity of the dark background behind the text (0 to 255).
const BACKGROUND_ALPHA: u8 = 170;

/// One clock window. The window is destroyed when this value is dropped.
pub struct ClockWindow {
    hwnd: isize,
}

impl ClockWindow {
    /// Create a hidden clock window. It shows after the first `draw`.
    pub fn create() -> Result<ClockWindow, String> {
        // SAFETY: valid class name and styles. No parent window.
        let hwnd = unsafe {
            let module = GetModuleHandleW(PCWSTR::null()).map_err(|e| e.to_string())?;
            let instance: HINSTANCE = module.into();
            CreateWindowExW(
                // Layered: transparent pixels. Topmost: above normal windows.
                // Tool window: no taskbar button and not in Alt+Tab.
                // No activate: never takes the keyboard focus.
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLOCK_CLASS,
                w!("WindowsClock clock"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )
            .map_err(|e| e.to_string())?
        };
        Ok(ClockWindow {
            hwnd: hwnd.0 as isize,
        })
    }

    /// Hide the window.
    pub fn hide(&self) {
        // SAFETY: valid window handle.
        let _ = unsafe { ShowWindow(hwnd_from(self.hwnd), SW_HIDE) };
    }

    /// Draw the text and put the window in the bottom-right corner of the
    /// given monitor rectangle. Returns the window rectangle.
    pub fn draw(&self, text: &str, monitor: Rect, dpi: u32) -> Result<Rect, String> {
        let hwnd = hwnd_from(self.hwnd);
        let wide: Vec<u16> = text.encode_utf16().collect();
        if wide.is_empty() {
            return Err("empty text".into());
        }
        let font_height = -geometry::points_to_pixels(FONT_POINTS, dpi);
        let pad_x = geometry::scale(PADDING_X, dpi);
        let pad_y = geometry::scale(PADDING_Y, dpi);
        let margin = geometry::scale(MARGIN, dpi);

        // SAFETY: every GDI object that is created here is released before
        // the block ends. The pixel slice is only used while the bitmap exists.
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.0.is_null() {
                return Err("cannot create drawing context".into());
            }
            let font = CreateFontW(
                font_height,
                0,
                0,
                0,
                FW_SEMIBOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                // Grayscale smoothing. Colored (ClearType) smoothing is not
                // correct on a transparent window.
                ANTIALIASED_QUALITY,
                0,
                w!("Segoe UI"),
            );
            let old_font = SelectObject(dc, HGDIOBJ::from(font));

            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
            let width = (size.cx + 2 * pad_x).max(1);
            let height = (size.cy + 2 * pad_y).max(1);

            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    // Negative height: the first pixel row is the top row.
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0);

            let result = match bitmap {
                Ok(bitmap) if !bits.is_null() => {
                    let old_bitmap = SelectObject(dc, HGDIOBJ::from(bitmap));

                    // White text on the black (all zero) bitmap.
                    SetBkMode(dc, TRANSPARENT);
                    SetTextColor(dc, COLORREF(0x00FF_FFFF));
                    let _ = TextOutW(dc, pad_x, pad_y, &wide);
                    let _ = GdiFlush();

                    let pixels = std::slice::from_raw_parts_mut(
                        bits as *mut u32,
                        (width as usize) * (height as usize),
                    );
                    render::compose_white_text(pixels, BACKGROUND_ALPHA);

                    let (x, y) = geometry::bottom_right(monitor, width, height, margin);
                    let position = POINT { x, y };
                    let window_size = SIZE {
                        cx: width,
                        cy: height,
                    };
                    let source = POINT { x: 0, y: 0 };
                    let blend = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 255,
                        AlphaFormat: AC_SRC_ALPHA as u8,
                    };
                    let updated = UpdateLayeredWindow(
                        hwnd,
                        None,
                        Some(&position),
                        Some(&window_size),
                        Some(dc),
                        Some(&source),
                        COLORREF(0),
                        Some(&blend),
                        ULW_ALPHA,
                    );

                    SelectObject(dc, old_bitmap);
                    let _ = DeleteObject(HGDIOBJ::from(bitmap));

                    match updated {
                        Ok(()) => {
                            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                            let _ = SetWindowPos(
                                hwnd,
                                Some(HWND_TOPMOST),
                                0,
                                0,
                                0,
                                0,
                                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                            );
                            Ok(Rect {
                                left: x,
                                top: y,
                                right: x + width,
                                bottom: y + height,
                            })
                        }
                        Err(e) => Err(format!("cannot update window: {e}")),
                    }
                }
                Ok(bitmap) => {
                    let _ = DeleteObject(HGDIOBJ::from(bitmap));
                    Err("bitmap has no pixel memory".into())
                }
                Err(e) => Err(format!("cannot create bitmap: {e}")),
            };

            SelectObject(dc, old_font);
            let _ = DeleteObject(HGDIOBJ::from(font));
            let _ = DeleteDC(dc);
            result
        }
    }
}

impl Drop for ClockWindow {
    fn drop(&mut self) {
        // SAFETY: the handle was created by `create` on this thread.
        let _ = unsafe { DestroyWindow(hwnd_from(self.hwnd)) };
    }
}
