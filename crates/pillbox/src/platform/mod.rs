//! Thin wrappers around Win32, Direct2D and DirectComposition. All `unsafe` in the app lives in
//! this module; everything above it is safe Rust.

pub mod audio;
pub mod camera;
pub mod clipboard;
pub mod controls;
pub mod devices;
pub mod diskio;
pub mod dpapi;
pub mod dragdrop;
pub mod firewall;
pub mod foreground;
pub mod fullscreen;
pub mod gfx;
pub mod gpu;
pub mod headphones;
pub mod info;
pub mod internet;
pub mod links;
pub mod media;
pub mod net;
pub mod notifications;
pub mod ocr;
pub mod screen;
pub mod sensors;
pub mod settings_window;
pub mod system;
pub mod thumbs;
pub mod tray;
pub mod widgetwin;
pub mod window;
