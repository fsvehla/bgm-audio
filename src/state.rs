//! Persistent state: volume, settings and per-file positions (= the recent list).
//!
//! Stored as JSON in ~/Library/Application Support/bgm-audio/state.json and
//! written atomically (temp file + rename).
//! The folder keeps the original bgm-audio name so existing state carries over.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::platform;

const VERSION: u32 = 1;
const MAX_RECENT: usize = 30;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub volume: f32,
    pub looping: bool,
    pub mic_auto_pause: bool,
    /// Which list the window shows under the player.
    pub tab: Tab,
    /// Last window frame, restored on launch if it is still on a display.
    pub window: Option<SavedWindow>,
    /// View menu: follow the system appearance or force light/dark.
    pub appearance: Appearance,
    /// Mini player: only artwork, title, progress bar and controls.
    pub compact: bool,
    /// Window height to return to when leaving the mini player.
    pub expanded_height: Option<f32>,
    /// Most recently played first. The first entry is the session to restore.
    pub files: Vec<FileEntry>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: VERSION,
            volume: 0.7,
            looping: true,
            mic_auto_pause: true,
            tab: Tab::Chapters,
            appearance: Appearance::System,
            window: None,
            compact: false,
            expanded_height: None,
            files: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Chapters,
    Recent,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct SavedWindow {
    /// UUID of the display the window was on; x and y are relative to that display.
    #[serde(default)]
    pub display: Option<String>,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub mode: WindowMode,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    /// Platform bookmark (macOS: base64 NSURL bookmark): finds the file after it is
    /// moved or renamed.
    #[serde(default)]
    pub bookmark: Option<String>,
    pub title: String,
    pub position: f64,
    pub duration: f64,
    /// Unix seconds.
    pub last_played: u64,
}

impl FileEntry {
    /// Current location of the file, following the bookmark if it moved.
    pub fn resolve(&self) -> Option<PathBuf> {
        if self.path.is_file() {
            return Some(self.path.clone());
        }
        let bookmark = self.bookmark.as_deref()?;
        platform::resolve_bookmark(bookmark).filter(|p| p.is_file())
    }
}

impl State {
    pub fn load() -> Self {
        std::fs::read(file())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = file();
        let tmp = path.with_extension("json.tmp");
        let result = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
            std::fs::rename(&tmp, &path)
        })();
        if let Err(e) = result {
            eprintln!("bgm-audio: could not save state: {e}");
        }
    }

    /// The file entry for `path`, if we have played it before.
    pub fn entry(&self, path: &Path) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == path)
    }

    /// Records progress for `path` and moves it to the front of the recent list.
    pub fn touch(&mut self, path: &Path, title: &str, position: f64, duration: f64) {
        let existing = self.files.iter().position(|f| f.path == path);
        let mut entry = match existing {
            Some(i) => self.files.remove(i),
            None => FileEntry {
                path: path.to_path_buf(),
                bookmark: platform::make_bookmark(path),
                title: String::new(),
                position: 0.0,
                duration: 0.0,
                last_played: 0,
            },
        };
        entry.title = title.to_string();
        entry.position = position;
        entry.duration = duration;
        entry.last_played = now();
        self.files.insert(0, entry);
        self.files.truncate(MAX_RECENT);
    }

    /// Updates a moved file's stored path so later lookups by path work.
    pub fn relocate(&mut self, index: usize, new_path: PathBuf) {
        if let Some(entry) = self.files.get_mut(index) {
            entry.path = new_path;
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.files.len() {
            self.files.remove(index);
        }
    }
}

fn file() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("bgm-audio")
        .join("state.json")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
