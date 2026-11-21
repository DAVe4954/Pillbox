//! Gives `pillbox.exe` its icon (Explorer, the taskbar, Alt+Tab): the picture is drawn by
//! `pillbox_core::appicon`, packed into a Windows resource file and handed to the linker.
//! No resource compiler or picture file is needed.

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../pillbox-core/src/appicon.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let Some(out_dir) = env::var_os("OUT_DIR").map(PathBuf::from) else {
        return;
    };
    let res = out_dir.join("pillbox-icon.res");
    if let Err(e) = fs::write(&res, pillbox_core::appicon::res_file()) {
        println!("cargo:warning=the app icon was not written: {e}");
        return;
    }
    println!("cargo:rustc-link-arg-bins={}", res.display());
}
