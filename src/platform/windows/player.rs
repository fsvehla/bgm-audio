//! Playback with WinRT's MediaPlayer; title, artist, chapters and artwork read from the
//! file with mp4ameta.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::Core::MediaSource;
use windows::Media::Playback::{MediaPlaybackItem, MediaPlaybackState, MediaPlayer};
use windows::Storage::StorageFile;
use windows::core::HSTRING;

use crate::track::{Chapter, Track};

/// TimeSpan counts 100-nanosecond ticks.
const TICKS_PER_SECOND: f64 = 10_000_000.0;

pub struct Player {
    player: MediaPlayer,
    /// Set when the file plays to its end. MediaPlayer then reports Paused; the app only
    /// notices the end while the player says it is playing, so `is_playing` stays true
    /// until the app reacts (play, pause or seek).
    ended: Arc<AtomicBool>,
    /// Seeks complete asynchronously: until then the session still reports the old
    /// position, and a quick second "next chapter" would start from there.
    seeking_to: Arc<Mutex<Option<f64>>>,
    pub track: Track,
}

impl Player {
    pub fn open(path: &Path) -> Option<Self> {
        if !path.is_file() {
            return None;
        }
        let track = read_track(path)?;
        let player = match media_player(path) {
            Ok(player) => player,
            Err(e) => {
                eprintln!("Ambience: can't play {}: {e}", path.display());
                return None;
            }
        };

        if let Err(e) = super::now_playing::attach(&player) {
            eprintln!("Ambience: media controls: {e}");
        }

        let ended = Arc::new(AtomicBool::new(false));
        let flag = ended.clone();
        player
            .MediaEnded(&TypedEventHandler::new(move |_, _| {
                flag.store(true, Ordering::Relaxed);
                Ok(())
            }))
            .ok()?;
        let seeking_to = Arc::new(Mutex::new(None));
        let target = seeking_to.clone();
        player
            .PlaybackSession()
            .and_then(|session| {
                session.SeekCompleted(&TypedEventHandler::new(move |_, _| {
                    *target.lock().unwrap() = None;
                    Ok(())
                }))
            })
            .ok()?;
        Some(Self {
            player,
            ended,
            seeking_to,
            track,
        })
    }

    pub fn play(&self) {
        self.ended.store(false, Ordering::Relaxed);
        self.player.Play().ok();
    }

    pub fn pause(&self) {
        self.ended.store(false, Ordering::Relaxed);
        self.player.Pause().ok();
    }

    /// True while playing or waiting to play (buffering), and at the end until handled.
    pub fn is_playing(&self) -> bool {
        if self.ended.load(Ordering::Relaxed) {
            return true;
        }
        let state = self
            .player
            .PlaybackSession()
            .and_then(|s| s.PlaybackState());
        matches!(
            state,
            Ok(MediaPlaybackState::Playing | MediaPlaybackState::Buffering)
        )
    }

    pub fn position(&self) -> f64 {
        if self.ended.load(Ordering::Relaxed) {
            return self.track.duration;
        }
        if let Some(target) = *self.seeking_to.lock().unwrap() {
            return target;
        }
        self.player
            .PlaybackSession()
            .and_then(|s| s.Position())
            .map(|t| (t.Duration as f64 / TICKS_PER_SECOND).max(0.0))
            .unwrap_or(0.0)
    }

    pub fn seek(&self, seconds: f64) {
        self.ended.store(false, Ordering::Relaxed);
        let seconds = seconds.clamp(0.0, self.track.duration.max(0.0));
        let position = TimeSpan {
            Duration: (seconds * TICKS_PER_SECOND) as i64,
        };
        if let Ok(session) = self.player.PlaybackSession() {
            *self.seeking_to.lock().unwrap() = Some(seconds);
            if session.SetPosition(position).is_err() {
                *self.seeking_to.lock().unwrap() = None;
            }
        }
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.SetVolume(volume.clamp(0.0, 1.0) as f64).ok();
    }
}

fn media_player(path: &Path) -> windows::core::Result<MediaPlayer> {
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path.as_os_str()))?.join()?;
    let source = MediaSource::CreateFromStorageFile(&file)?;
    let item = MediaPlaybackItem::Create(&source)?;
    let player = MediaPlayer::new()?;
    player.SetAutoPlay(false)?;
    player.SetSource(&item)?;
    Ok(player)
}

fn read_track(path: &Path) -> Option<Track> {
    let tag = match mp4ameta::Tag::read_from_path(path) {
        Ok(tag) => tag,
        Err(e) => {
            eprintln!("Ambience: can't read {}: {e}", path.display());
            return None;
        }
    };
    let title = tag
        .title()
        .map(str::to_string)
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let chapters = tag
        .chapters()
        .iter()
        .map(|c| Chapter {
            title: c.title.clone(),
            start: c.start.as_secs_f64(),
        })
        .collect();
    Some(Track {
        path: path.to_path_buf(),
        title,
        artist: tag.artist().map(str::to_string),
        duration: tag.duration().as_secs_f64(),
        chapters,
        artwork: tag.artwork().map(|img| img.data.to_vec()),
    })
}
