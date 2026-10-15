//! What the app knows about an open file. Filled in by the platform player.

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Chapter {
    pub title: String,
    pub start: f64,
}

impl Chapter {
    /// Chapter titles often carry credits after a comma or "·"; keep the name.
    pub fn short_title(&self) -> String {
        let title = &self.title;
        let cut = title
            .find(" · ")
            .or_else(|| title.find(", "))
            .unwrap_or(title.len());
        title[..cut].trim().to_string()
    }
}

pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub duration: f64,
    pub chapters: Vec<Chapter>,
    pub artwork: Option<Vec<u8>>,
}

impl Track {
    /// Index of the chapter that contains `position`.
    pub fn chapter_at(&self, position: f64) -> Option<usize> {
        self.chapters
            .iter()
            .rposition(|c| c.start <= position + 0.25)
    }
}
