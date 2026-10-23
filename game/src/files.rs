//! Native open and save dialogs (old: the browser's download link and file input), for
//! exporting and importing saves. `rfd` shows the system's own dialogs: AppKit panels on
//! macOS and the common item dialogs on Windows. They are modal and run on the window
//! thread, which they block until the player answers (the match is paused under the
//! game menu; multiplayer matches have no saves).

use std::path::PathBuf;

/// What the save dialogs filter for.
const FILTER: (&str, &[&str]) = ("files.save-filter", &[crate::saves::EXPORT_EXTENSION]);

/// Asks where to export a save; `None` when cancelled.
pub fn save_as(file_name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr!("files.export-save"))
        .add_filter(crate::text::tr(FILTER.0), FILTER.1)
        .set_file_name(file_name)
        .save_file()
}

/// Asks for a save file to import; `None` when cancelled.
pub fn open() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr!("files.import-save"))
        .add_filter(crate::text::tr(FILTER.0), FILTER.1)
        .pick_file()
}

/// Shows `folder` in the system's file manager (Finder, Explorer).
pub fn reveal(folder: &std::path::Path) -> anyhow::Result<()> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(folder).spawn()?;
    Ok(())
}
