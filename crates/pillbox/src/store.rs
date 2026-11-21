//! Reading and writing `%APPDATA%\Pillbox\settings.json`.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use pillbox_core::settings::Settings;

/// `%APPDATA%\Pillbox`, created if missing. `PILLBOX_DATA_DIR` overrides it (used by tests so
/// they never touch the real settings).
pub fn dir() -> Result<PathBuf> {
    let dir = match std::env::var_os("PILLBOX_DATA_DIR") {
        Some(custom) => PathBuf::from(custom),
        None => {
            let base = std::env::var_os("APPDATA").ok_or_else(|| anyhow!("APPDATA is not set"))?;
            PathBuf::from(base).join("Pillbox")
        }
    };
    let fresh = !dir.exists();
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    if fresh && std::env::var_os("PILLBOX_DATA_DIR").is_none() {
        migrate_legacy(&dir);
    }
    Ok(dir)
}

/// Pillbox was called Island before and kept its files in `%APPDATA%\Island`. The first start
/// under the new name copies them over (settings, clipboard history, reminders, play time...).
/// It is a copy: the old folder is left alone.
fn migrate_legacy(new: &std::path::Path) {
    let Some(old) = new.parent().map(|p| p.join("Island")) else {
        return;
    };
    if !old.is_dir() {
        return;
    }
    match copy_tree(&old, new) {
        Ok(n) => tracing::info!("copied {n} files from the old Island folder"),
        Err(e) => tracing::warn!("could not copy the old Island files: {e:#}"),
    }
}

/// Copy every file under `from` into `to`, keeping the folder layout; returns how many.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> Result<usize> {
    let mut copied = 0;
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copied += copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
            copied += 1;
        }
    }
    Ok(copied)
}

pub fn path() -> Result<PathBuf> {
    Ok(dir()?.join("settings.json"))
}

/// Load the settings. A missing file is created with the defaults; an unreadable or broken
/// file yields `None` so the caller keeps whatever it already has.
pub fn load() -> Option<Settings> {
    let path = match path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("no settings folder: {e:#}");
            return None;
        }
    };
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let defaults = Settings::default();
            if let Err(e) = save(&defaults) {
                tracing::warn!("could not create the settings file: {e:#}");
            }
            return Some(defaults);
        }
        Err(e) => {
            tracing::warn!("could not read {}: {e}", path.display());
            return None;
        }
    };
    match Settings::from_json(&text) {
        Ok((settings, warnings)) => {
            for w in warnings {
                tracing::warn!("settings: {w}");
            }
            Some(settings)
        }
        Err(e) => {
            // Usually a half-written or hand-edited file with a syntax error.
            tracing::warn!("settings file is not valid JSON ({e}); keeping the current settings");
            None
        }
    }
}

/// Write the settings atomically: a reader never sees a half-written file.
pub fn save(settings: &Settings) -> Result<()> {
    write_atomic(&path()?, settings.to_json().as_bytes())
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(())
}

/// `%APPDATA%\Pillbox\data`: clipboard history, shelf, reminders, notes. A subfolder, so
/// writing these never wakes the settings watcher (which watches only the top folder).
pub fn data_dir() -> Result<PathBuf> {
    let d = dir()?.join("data");
    fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
    Ok(d)
}

/// Replace `data\<name>` atomically (write a temporary file, then rename over).
pub fn save_bytes(name: &str, bytes: &[u8]) -> Result<()> {
    write_atomic(&data_dir()?.join(name), bytes)
}

/// `data\<name>`; `None` if it does not exist or cannot be read.
pub fn load_bytes(name: &str) -> Option<Vec<u8>> {
    let path = data_dir().ok()?.join(name);
    match fs::read(&path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!("could not read {}: {e}", path.display());
            None
        }
    }
}

/// A JSON data file (reminders, shelf, notes). Missing or broken files give the default; a
/// broken one is kept aside as `<name>.bad` so nothing is silently lost.
pub fn load_json<T: serde::de::DeserializeOwned + Default>(name: &str) -> T {
    let Some(bytes) = load_bytes(name) else {
        return T::default();
    };
    match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("{name} is damaged ({e}); starting empty and keeping a copy");
            if let Ok(d) = data_dir() {
                let _ = fs::copy(d.join(name), d.join(format!("{name}.bad")));
            }
            T::default()
        }
    }
}

pub fn save_json<T: serde::Serialize>(name: &str, value: &T) -> Result<()> {
    let text = serde_json::to_vec_pretty(value).context("encoding JSON")?;
    save_bytes(name, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_copied_with_its_subfolders() {
        let root = std::env::temp_dir().join(format!("pillbox-copy-test-{}", std::process::id()));
        let (from, to) = (root.join("from"), root.join("to"));
        fs::create_dir_all(from.join("data")).unwrap();
        fs::write(from.join("settings.json"), "{}").unwrap();
        fs::write(from.join("data").join("playtime.json"), "{}").unwrap();
        assert_eq!(copy_tree(&from, &to).unwrap(), 2);
        assert!(to.join("settings.json").is_file());
        assert!(to.join("data").join("playtime.json").is_file());
        let _ = fs::remove_dir_all(&root);
    }
}
