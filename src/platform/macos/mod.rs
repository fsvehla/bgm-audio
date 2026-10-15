//! macOS: AVFoundation, MediaPlayer, CoreAudio, AppKit and Sparkle.

mod appearance;
mod files;
pub mod mic;
pub mod now_playing;
pub mod player;
pub mod updater;

pub use appearance::{accent_color, set_app_appearance, style_title_bar, watch_accent};
pub use files::{file_url_path, make_bookmark, resolve_bookmark};
