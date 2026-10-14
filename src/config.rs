//! Where LiteSheet keeps its settings files.

use std::path::PathBuf;

/// A settings file in the user's application-support directory.
pub fn file(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let base = if cfg!(target_os = "macos") {
        PathBuf::from(home).join("Library/Application Support/LiteSheet")
    } else {
        PathBuf::from(home).join(".config/litesheet")
    };
    Some(base.join(name))
}
