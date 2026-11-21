//! Pillbox: a floating Dynamic Island-style pill for Windows 10 and 11.
//!
//! `pillbox.exe` runs Pillbox. `pillbox.exe --settings` runs the Settings window as its own
//! process; the two talk only through the settings file, which Pillbox hot-reloads.
// No console window in release builds; debug builds keep it for log output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod faces;
mod lyrics;
mod phone;
mod platform;
mod scene;
mod script;
mod settings_ui;
mod store;
mod tabs;
mod widgets;

use std::{fs, path::PathBuf, sync::Mutex};

use anyhow::{Context, Result};

/// `%LOCALAPPDATA%\Pillbox\<name>`, rewritten on every start.
fn log_path(name: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Pillbox");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join(name))
}

fn init_logging(file_name: &str) {
    let level = if std::env::var_os("PILLBOX_DEBUG").is_some() {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };
    let builder = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_ansi(false);
    // The last run's log is kept as `<name>.prev`: after a restart or a sign-in start, it is
    // the only record of how the run before this one ended.
    if let Some(path) = log_path(file_name) {
        let _ = fs::rename(&path, path.with_extension("prev.log"));
    }
    match log_path(file_name).and_then(|p| fs::File::create(p).ok()) {
        Some(file) => builder.with_writer(Mutex::new(file)).init(),
        None => builder.init(),
    }
    std::panic::set_hook(Box::new(|info| tracing::error!("panic: {info}")));
}

/// Developer aid: pretend the monitor has this DPI scale (`PILLBOX_SCALE=1.5`).
fn scale_override() -> Option<f32> {
    std::env::var("PILLBOX_SCALE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .filter(|s| (0.5..=5.0).contains(s))
}

fn run_pillbox() -> Result<()> {
    let mut app = app::App::new(scale_override()).context("starting Pillbox")?;
    app.run()
}

fn main() {
    let settings_mode = std::env::args().any(|a| a == "--settings");
    platform::system::enable_dpi_awareness();
    // Checked before logging starts: a second copy must not truncate the running copy's log.
    let _instance = match platform::system::SingleInstance::acquire(settings_mode) {
        Ok(Some(instance)) => Some(instance),
        Ok(None) => return, // already running
        Err(_) => None,     // could not tell; better to run than to refuse
    };
    if settings_mode {
        init_logging("settings.log");
        if let Err(e) = settings_ui::run() {
            tracing::error!("settings window failed: {e:#}");
            std::process::exit(1);
        }
        return;
    }
    init_logging("pillbox.log");
    tracing::info!("Pillbox {} starting", env!("CARGO_PKG_VERSION"));
    // Test runs with their own data folder must not touch the real registry.
    if std::env::var_os("PILLBOX_DATA_DIR").is_none() {
        platform::system::migrate_autostart();
        tracing::info!(
            "start with Windows: {}",
            if platform::system::autostart_enabled() {
                "on"
            } else {
                "off"
            }
        );
    }
    if let Err(e) = run_pillbox() {
        tracing::error!("fatal: {e:#}");
        std::process::exit(1);
    }
    tracing::info!("Pillbox exiting");
}
