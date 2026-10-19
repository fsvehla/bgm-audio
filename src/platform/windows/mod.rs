//! Windows. Same names as `platform::macos`. Still doing nothing: `mic` (auto-pause is a
//! future feature on Windows), bookmarks and file URLs (no "Open With" on Windows).

use std::path::{Path, PathBuf};

pub mod now_playing;
pub mod player;
pub mod updater;

pub mod mic {
    use futures::channel::mpsc::UnboundedSender;

    use crate::Event;

    pub struct MicWatcher;

    impl MicWatcher {
        pub fn new(_tx: UnboundedSender<Event>) -> Self {
            Self
        }
        pub fn sync(&mut self) {}
        pub fn recording_app(&self) -> Option<String> {
            None
        }
    }
}

mod appearance;
pub use appearance::{
    accent_color, set_app_appearance, style_title_bar, system_colors, watch_accent,
};

pub fn make_bookmark(_path: &Path) -> Option<String> {
    None
}
pub fn resolve_bookmark(_bookmark: &str) -> Option<PathBuf> {
    None
}
pub fn file_url_path(_url: &str) -> Option<PathBuf> {
    None
}
