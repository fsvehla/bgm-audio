//! Windows: not implemented yet. Same names as `platform::macos`, doing nothing, so
//! the app builds and its UI runs; each piece gets a real implementation later.

use std::path::{Path, PathBuf};

use futures::channel::mpsc::UnboundedSender;
use gpui::Hsla;

use crate::Event;
use crate::state::Appearance;

pub mod player {
    use std::path::Path;

    use crate::track::Track;

    pub struct Player {
        pub track: Track,
    }

    impl Player {
        pub fn open(_path: &Path) -> Option<Self> {
            None
        }
        pub fn play(&self) {}
        pub fn pause(&self) {}
        pub fn is_playing(&self) -> bool {
            false
        }
        pub fn position(&self) -> f64 {
            0.0
        }
        pub fn seek(&self, _seconds: f64) {}
        pub fn set_volume(&self, _volume: f32) {}
    }
}

pub mod now_playing {
    use futures::channel::mpsc::UnboundedSender;

    use crate::Event;
    use crate::track::Track;

    pub struct Artwork;

    pub fn register(_tx: UnboundedSender<Event>) {}
    pub fn use_chapter_buttons(_chapters: bool) {}
    pub fn update(_track: &Track, _artwork: Option<&Artwork>, _position: f64, _playing: bool) {}
    pub fn clear() {}
    pub fn artwork(_bytes: &[u8]) -> Option<Artwork> {
        None
    }
}

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

pub fn accent_color() -> Option<Hsla> {
    None
}
pub fn watch_accent(_tx: UnboundedSender<Event>) {}
pub fn set_app_appearance(_choice: Appearance) {}

pub fn make_bookmark(_path: &Path) -> Option<String> {
    None
}
pub fn resolve_bookmark(_bookmark: &str) -> Option<PathBuf> {
    None
}
pub fn file_url_path(_url: &str) -> Option<PathBuf> {
    None
}
