//! The system media controls (media flyout, lock screen, media keys) through the
//! player's SystemMediaTransportControls. Each opened file gets a new MediaPlayer, so
//! `attach` takes over the controls of every new player.

use std::cell::RefCell;
use std::sync::Mutex;

use futures::channel::mpsc::UnboundedSender;
use windows::Foundation::{TimeSpan, TypedEventHandler};
use windows::Media::Playback::MediaPlayer;
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::{
    DataWriter, InMemoryRandomAccessStream, RandomAccessStreamReference,
};
use windows::core::{HSTRING, Ref};

use crate::Event;
use crate::platform::Remote;
use crate::track::Track;

/// TimeSpan counts 100-nanosecond ticks.
const TICKS_PER_SECOND: f64 = 10_000_000.0;

/// Button events arrive on other threads.
static TX: Mutex<Option<UnboundedSender<Event>>> = Mutex::new(None);

thread_local! {
    /// The controls of the current player; updated from the main thread.
    static CONTROLS: RefCell<Option<SystemMediaTransportControls>> = const { RefCell::new(None) };
}

/// Cover art in the form the media controls want; built once per track.
pub struct Artwork(RandomAccessStreamReference);

pub fn register(tx: UnboundedSender<Event>) {
    *TX.lock().unwrap() = Some(tx);
}

fn send(remote: Remote) {
    if let Some(tx) = TX.lock().unwrap().as_ref() {
        tx.unbounded_send(Event::Remote(remote)).ok();
    }
}

/// Takes over the media controls of a newly opened player. Main thread.
pub(super) fn attach(player: &MediaPlayer) -> windows::core::Result<()> {
    // Buttons go through the app (mic auto-pause, saved state), not straight to the player.
    player.CommandManager()?.SetIsEnabled(false)?;
    let controls = player.SystemMediaTransportControls()?;
    controls.SetIsEnabled(true)?;
    controls.SetIsPlayEnabled(true)?;
    controls.SetIsPauseEnabled(true)?;
    controls.ButtonPressed(&TypedEventHandler::new(
        |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
            let remote = match args.ok()?.Button()? {
                SystemMediaTransportControlsButton::Play => Remote::Play,
                SystemMediaTransportControlsButton::Pause => Remote::Pause,
                SystemMediaTransportControlsButton::Next => Remote::Next,
                SystemMediaTransportControlsButton::Previous => Remote::Previous,
                SystemMediaTransportControlsButton::FastForward => Remote::SkipForward,
                SystemMediaTransportControlsButton::Rewind => Remote::SkipBackward,
                _ => return Ok(()),
            };
            send(remote);
            Ok(())
        },
    ))?;
    controls.PlaybackPositionChangeRequested(&TypedEventHandler::new(
        |_, args: Ref<PlaybackPositionChangeRequestedEventArgs>| {
            let position = args.ok()?.RequestedPlaybackPosition()?;
            send(Remote::Seek(position.Duration as f64 / TICKS_PER_SECOND));
            Ok(())
        },
    ))?;
    CONTROLS.with_borrow_mut(|c| *c = Some(controls));
    Ok(())
}

fn with_controls(f: impl FnOnce(&SystemMediaTransportControls) -> windows::core::Result<()>) {
    CONTROLS.with_borrow(|c| {
        if let Some(controls) = c {
            if let Err(e) = f(controls) {
                eprintln!("Ambience: media controls: {e}");
            }
        }
    });
}

/// Next/previous step through chapters; without chapters, fast forward and rewind
/// skip seconds instead.
pub fn use_chapter_buttons(chapters: bool) {
    with_controls(|c| {
        c.SetIsNextEnabled(chapters)?;
        c.SetIsPreviousEnabled(chapters)?;
        c.SetIsFastForwardEnabled(!chapters)?;
        c.SetIsRewindEnabled(!chapters)
    });
}

/// Publishes the current state. `artwork` is built once per track via [`artwork`].
pub fn update(track: &Track, artwork: Option<&Artwork>, position: f64, playing: bool) {
    let (title, album) = track.now_playing_titles(position);
    with_controls(|c| {
        let display = c.DisplayUpdater()?;
        display.SetType(MediaPlaybackType::Music)?;
        let music = display.MusicProperties()?;
        music.SetTitle(&HSTRING::from(title.as_str()))?;
        music.SetAlbumTitle(&HSTRING::from(album.as_deref().unwrap_or_default()))?;
        music.SetArtist(&HSTRING::from(track.artist.as_deref().unwrap_or_default()))?;
        if let Some(art) = artwork {
            display.SetThumbnail(&art.0)?;
        }
        display.Update()?;

        let ticks = |s: f64| TimeSpan {
            Duration: (s * TICKS_PER_SECOND) as i64,
        };
        let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
        timeline.SetStartTime(ticks(0.0))?;
        timeline.SetMinSeekTime(ticks(0.0))?;
        timeline.SetEndTime(ticks(track.duration))?;
        timeline.SetMaxSeekTime(ticks(track.duration))?;
        timeline.SetPosition(ticks(position))?;
        c.UpdateTimelineProperties(&timeline)?;

        c.SetPlaybackStatus(if playing {
            MediaPlaybackStatus::Playing
        } else {
            MediaPlaybackStatus::Paused
        })
    });
}

pub fn clear() {
    with_controls(|c| {
        c.DisplayUpdater()?.ClearAll()?;
        c.SetPlaybackStatus(MediaPlaybackStatus::Closed)
    });
}

pub fn artwork(bytes: &[u8]) -> Option<Artwork> {
    let stream = InMemoryRandomAccessStream::new().ok()?;
    let writer = DataWriter::CreateDataWriter(&stream).ok()?;
    writer.WriteBytes(bytes).ok()?;
    writer.StoreAsync().ok()?.join().ok()?;
    writer.DetachStream().ok()?;
    stream.Seek(0).ok()?;
    RandomAccessStreamReference::CreateFromStream(&stream)
        .ok()
        .map(Artwork)
}
