//! The clock application: state and reactions to events.
//! This module contains no `unsafe` code.

use crate::buildinfo;
use crate::config::{self, Config, Loaded};
use crate::logging;
use crate::monitors::{self, Monitor};
use crate::win::{self, ClockWindow, Event, MenuItem};
use std::cell::RefCell;
use std::path::PathBuf;

const TITLE: &str = "WindowsClock";
const MENU_EXIT: usize = 1;
const MENU_SELECT: usize = 2;
const MENU_HEADER: usize = 3;
const MENU_MONITOR_BASE: usize = 100;
/// Number of extra checks (one each second) when Windows gives no hardware
/// data for a monitor.
const MAX_RETRIES: u32 = 30;

struct App {
    config: Config,
    config_path: PathBuf,
    monitors: Vec<Monitor>,
    /// Clock windows, with the runtime key of the monitor of each window.
    windows: Vec<(String, ClockWindow)>,
    /// Number of extra checks made because Windows gave no hardware data.
    retries: u32,
    /// Last monitor list that was written to the log.
    logged_layout: String,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn log(text: &str) {
    logging::log(&win::timestamp(), text);
}

/// Run a function with the application state.
///
/// The state is removed from its place during the call. If Windows calls us
/// again during that time, the second call finds no state. It then asks for a
/// later rebuild and does nothing else.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    let taken = APP.with(|cell| cell.borrow_mut().take());
    match taken {
        Some(mut app) => {
            let result = f(&mut app);
            APP.with(|cell| *cell.borrow_mut() = Some(app));
            Some(result)
        }
        None => {
            win::schedule_rebuild();
            None
        }
    }
}

/// Start the program. Returns when the program stops.
pub fn run() {
    if win::notify_existing_instance() {
        // The program already runs. It now asks for the monitor selection.
        return;
    }

    logging::init(config::data_dir().join("windowsclock.log"));
    log(&format!("start: {}", buildinfo::summary()));

    if let Err(error) = win::init(handle_event) {
        log(&format!("start failed: {error}"));
        win::message_box(TITLE, &format!("WindowsClock cannot start.\n\n{error}"));
        return;
    }

    let config_path = config::settings_path();
    let (config, first_start) = match Config::load(&config_path) {
        Loaded::Ok(config) => (config, false),
        Loaded::Missing => {
            log("no settings file: first start");
            (Config::default(), true)
        }
        Loaded::Damaged(reason) => {
            log(&format!("settings file not used: {reason}"));
            (Config::default(), true)
        }
    };

    APP.with(|cell| {
        *cell.borrow_mut() = Some(App {
            config,
            config_path,
            monitors: Vec::new(),
            windows: Vec::new(),
            retries: 0,
            logged_layout: String::new(),
        });
    });

    with_app(|app| {
        app.rebuild();
        app.arm_timer();
    });
    if first_start {
        win::post_select_request();
    }

    win::run_loop();
    // Destroy the clock windows before the program ends.
    APP.with(|cell| cell.borrow_mut().take());
    log("stop");
}

fn handle_event(event: Event) {
    match event {
        Event::Tick => {
            with_app(|app| {
                app.draw_all();
                app.arm_timer();
            });
        }
        Event::Rebuild => {
            with_app(|app| {
                app.rebuild();
                app.arm_timer();
            });
        }
        Event::Menu => show_menu(),
        Event::Select => ask_selection(),
    }
}

fn show_menu() {
    let Some((items, monitors)) = with_app(|app| {
        app.refresh_monitors();
        (app.menu_items(), app.monitors.clone())
    }) else {
        return;
    };
    // The menu is open here. The application state is free during this time.
    let Some(choice) = win::show_menu(&items) else {
        return;
    };
    match choice {
        MENU_EXIT => {
            log("exit selected in menu");
            win::quit();
        }
        MENU_SELECT => ask_selection(),
        id if id >= MENU_MONITOR_BASE => {
            if let Some(monitor) = monitors.get(id - MENU_MONITOR_BASE) {
                with_app(|app| {
                    let enabled = !app.is_selected(monitor);
                    app.set_selection(&[(monitor.clone(), enabled)]);
                    app.rebuild();
                });
            }
        }
        _ => {}
    }
}

/// Ask for each monitor if it gets a clock.
fn ask_selection() {
    let Some(monitors) = with_app(|app| {
        app.refresh_monitors();
        app.monitors.clone()
    }) else {
        return;
    };

    let mut answers = Vec::new();
    for monitor in monitors.iter().filter(|m| m.stable) {
        let question = format!(
            "Show the clock on this monitor?\n\n{}\n\nYes = show the clock\nNo = no clock",
            monitor.label()
        );
        let yes = win::ask_yes_no(TITLE, &question);
        answers.push((monitor.clone(), yes));
    }
    let any = answers.iter().any(|(_, yes)| *yes);

    with_app(|app| {
        app.set_selection(&answers);
        app.rebuild();
    });

    if !any {
        win::message_box(
            TITLE,
            "No monitor is selected, so no clock is shown.\n\n\
             WindowsClock continues to run. To select a monitor, start WindowsClock again.",
        );
    }
}

impl App {
    /// True if this connected monitor must show a clock.
    fn is_selected(&self, monitor: &Monitor) -> bool {
        monitor.stable
            && self.config.is_enabled(
                &monitor.model,
                &monitor.instance,
                monitors::count_model(&self.monitors, &monitor.model),
            )
    }

    fn refresh_monitors(&mut self) {
        self.monitors = monitors::combine(&win::raw_monitors(), &win::display_targets());
        let layout = self
            .monitors
            .iter()
            .map(|m| {
                format!(
                    "[{} | model={} | connector={} | {},{} {}x{} | dpi={} | selected={}]",
                    m.name,
                    if m.stable {
                        m.model.as_str()
                    } else {
                        "(no hardware data)"
                    },
                    m.instance,
                    m.rect.left,
                    m.rect.top,
                    m.rect.width(),
                    m.rect.height(),
                    m.dpi,
                    self.is_selected(m)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        if layout != self.logged_layout {
            log(&format!("monitors: {layout}"));
            self.logged_layout = layout;
        }
    }

    /// Read the monitor list again and make the clock windows agree with it.
    ///
    /// Rule: a window exists only for a monitor that is connected, has a
    /// stable identifier, and is selected in the settings.
    fn rebuild(&mut self) {
        self.refresh_monitors();

        let wanted: Vec<String> = self
            .monitors
            .iter()
            .filter(|m| self.is_selected(m))
            .map(|m| m.runtime_key())
            .collect();

        // Directly after a monitor is connected, Windows sometimes has no
        // hardware data for it. No more notification comes when the data is
        // ready, so check again after a short time.
        if self.monitors.iter().any(|m| !m.stable) {
            if self.retries < MAX_RETRIES {
                self.retries += 1;
                win::schedule_retry();
            } else if self.retries == MAX_RETRIES {
                self.retries += 1;
                log("no hardware data for a monitor after all retries");
            }
        } else {
            self.retries = 0;
        }

        // Remove windows of monitors that are gone or not selected.
        // Dropping a window destroys it.
        self.windows.retain(|(id, _)| wanted.contains(id));

        // Add windows for newly wanted monitors.
        for id in &wanted {
            if !self.windows.iter().any(|(existing, _)| existing == id) {
                match ClockWindow::create() {
                    Ok(window) => self.windows.push((id.clone(), window)),
                    Err(error) => log(&format!("cannot create clock window: {error}")),
                }
            }
        }

        self.draw_all();
    }

    /// Draw the current time on all clock windows at their monitor positions.
    fn draw_all(&mut self) {
        let text = win::time_text();
        for (id, window) in &self.windows {
            match self.monitors.iter().find(|m| m.runtime_key() == *id) {
                Some(monitor) => {
                    if let Err(error) = window.draw(&text, monitor.rect, monitor.dpi) {
                        window.hide();
                        log(&format!("draw failed on {}: {error}", monitor.name));
                    }
                }
                // No position is known for this window. Never show it.
                None => window.hide(),
            }
        }
    }

    /// Set the timer to the start of the next minute.
    fn arm_timer(&self) {
        // A small extra time makes sure that the minute has changed.
        win::set_tick_timer(win::ms_to_next_minute() + 20);
    }

    /// Change the selection and save it immediately.
    fn set_selection(&mut self, changes: &[(Monitor, bool)]) {
        let mut changed = false;
        for (monitor, enabled) in changes {
            let same_model = monitors::count_model(&self.monitors, &monitor.model);
            if self.config.set_enabled(
                &monitor.model,
                &monitor.instance,
                &monitor.name,
                same_model,
                *enabled,
            ) {
                changed = true;
                log(&format!(
                    "selection: {} (model {}) = {}",
                    monitor.name, monitor.model, enabled
                ));
            }
        }
        if changed {
            match self.config.save(&self.config_path) {
                Ok(()) => log("settings saved"),
                Err(error) => {
                    log(&format!("cannot save settings: {error}"));
                    win::message_box(
                        TITLE,
                        &format!("WindowsClock cannot save the settings.\n\n{error}"),
                    );
                }
            }
        }
        // The log line with the monitor list contains the selection.
        self.logged_layout.clear();
    }

    fn menu_items(&self) -> Vec<MenuItem> {
        let mut items = vec![
            MenuItem {
                id: MENU_HEADER,
                label: buildinfo::summary(),
                checked: false,
                enabled: false,
                separator: false,
            },
            separator(),
        ];
        for (index, monitor) in self.monitors.iter().enumerate() {
            items.push(MenuItem {
                id: MENU_MONITOR_BASE + index,
                label: format!("Show clock on {}", monitor.label()),
                checked: self.is_selected(monitor),
                enabled: monitor.stable,
                separator: false,
            });
        }
        items.push(separator());
        items.push(MenuItem {
            id: MENU_SELECT,
            label: "Ask for each monitor...".into(),
            checked: false,
            enabled: true,
            separator: false,
        });
        items.push(MenuItem {
            id: MENU_EXIT,
            label: "Exit".into(),
            checked: false,
            enabled: true,
            separator: false,
        });
        items
    }
}

fn separator() -> MenuItem {
    MenuItem {
        id: 0,
        label: String::new(),
        checked: false,
        enabled: false,
        separator: true,
    }
}
