//! Everything that talks to the operating system. The rest of the app uses only
//! what this module exports; each OS provides the same names:
//!
//! - `player::Player`: open a file (title, artist, chapters, artwork), play, seek.
//! - `now_playing`: system media controls and media keys; sends [`Remote`] events.
//! - `mic::MicWatcher`: which other app, if any, is recording from a microphone.
//! - `updater::Updater`: automatic updates, when the app is installed.
//! - `accent_color`, `system_colors`, `watch_accent`, `set_app_appearance`,
//!   `style_title_bar`: system colours, light/dark.
//! - `make_bookmark`, `resolve_bookmark`: find files again after they move.
//! - `file_url_path`: paths from file URLs the system hands to the app.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

/// The system's own window colours for light or dark, where the OS has them.
/// Fills and labels are translucent, as the system defines them.
#[derive(Clone, Copy, Debug)]
pub struct SystemColors {
    pub background: gpui::Rgba,
    /// Buttons, the tab bar and the selected row.
    pub fill: gpui::Rgba,
    pub label: gpui::Rgba,
    pub secondary_label: gpui::Rgba,
    pub red: gpui::Rgba,
}

/// A command from the system's media controls (media keys, Control Center, headphones).
#[derive(Debug)]
pub enum Remote {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    SkipForward,
    SkipBackward,
    Seek(f64),
}
