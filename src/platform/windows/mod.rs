//! Windows. Same names as `platform::macos`. Done: `player`, `now_playing`, appearance. The rest does nothing yet,
//! so the app builds and its UI runs; each piece gets a real implementation later.

use std::path::{Path, PathBuf};

pub mod player;

pub mod now_playing;

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

pub mod updater {
    pub struct Updater;

    impl gpui::Global for Updater {}

    impl Updater {
        pub fn start() -> Option<Self> {
            None
        }
        pub fn check_for_updates(&self) {}
    }
}

mod appearance;
pub use appearance::{accent_color, set_app_appearance, style_title_bar, watch_accent};

pub fn make_bookmark(_path: &Path) -> Option<String> {
    None
}
pub fn resolve_bookmark(_bookmark: &str) -> Option<PathBuf> {
    None
}
pub fn file_url_path(_url: &str) -> Option<PathBuf> {
    None
}
