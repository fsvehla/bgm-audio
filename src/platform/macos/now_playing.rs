//! Now Playing (Control Center, media keys, headphone controls).

use std::ptr::NonNull;

use block2::RcBlock;
use futures::channel::mpsc::UnboundedSender;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, Message};
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSData, NSDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumTitle,
    MPMediaItemPropertyArtist, MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration,
    MPMediaItemPropertyTitle, MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyChapterCount,
    MPNowPlayingInfoPropertyChapterNumber, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState, MPRemoteCommand,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};

use crate::Event;
use crate::platform::Remote;
use crate::track::Track;

/// Registers handlers for the system's remote commands. Call once, on the main thread.
pub fn register(tx: UnboundedSender<Event>) {
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        let add = |command: Retained<MPRemoteCommand>,
                   remote: fn(&MPRemoteCommandEvent) -> Remote| {
            let tx = tx.clone();
            let handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
                let _ = tx.unbounded_send(Event::Remote(remote(event.as_ref())));
                MPRemoteCommandHandlerStatus::Success
            });
            command.setEnabled(true);
            command.addTargetWithHandler(&handler);
        };
        add(center.playCommand(), |_| Remote::Play);
        add(center.pauseCommand(), |_| Remote::Pause);
        add(center.togglePlayPauseCommand(), |_| Remote::Toggle);
        add(center.nextTrackCommand(), |_| Remote::Next);
        add(center.previousTrackCommand(), |_| Remote::Previous);
        add(Retained::into_super(center.skipForwardCommand()), |_| {
            Remote::SkipForward
        });
        add(Retained::into_super(center.skipBackwardCommand()), |_| {
            Remote::SkipBackward
        });
        add(
            center.changePlaybackPositionCommand().into_super(),
            |event| {
                let event: &MPChangePlaybackPositionCommandEvent =
                    &*(event as *const MPRemoteCommandEvent).cast();
                Remote::Seek(event.positionTime())
            },
        );
    }
}

/// The widget shows only one pair of skip buttons, and macOS prefers the seconds
/// skip when both are enabled. So: chapter buttons when the file has chapters,
/// otherwise seconds skip.
pub fn use_chapter_buttons(chapters: bool) {
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        center.nextTrackCommand().setEnabled(chapters);
        center.previousTrackCommand().setEnabled(chapters);
        center.skipForwardCommand().setEnabled(!chapters);
        center.skipBackwardCommand().setEnabled(!chapters);
    }
}

/// Publishes the current state. `artwork` is built once per track via [`artwork`].
pub fn update(track: &Track, artwork: Option<&Artwork>, position: f64, playing: bool) {
    let chapter = track.chapter_at(position);
    // With chapters, the chapter is the "song" and the file is the "album".
    let (title, album) = match chapter.map(|i| track.chapters[i].short_title()) {
        Some(t) if !t.is_empty() => (t, Some(track.title.clone())),
        _ => (track.title.clone(), None),
    };

    let mut keys: Vec<&NSString> = Vec::new();
    let mut values: Vec<Retained<AnyObject>> = Vec::new();
    let mut put = |k: &'static NSString, v: Retained<AnyObject>| {
        keys.push(k);
        values.push(v);
    };
    unsafe {
        put(MPMediaItemPropertyTitle, obj(NSString::from_str(&title)));
        if let Some(album) = album {
            put(
                MPMediaItemPropertyAlbumTitle,
                obj(NSString::from_str(&album)),
            );
        }
        if let Some(artist) = &track.artist {
            put(MPMediaItemPropertyArtist, obj(NSString::from_str(artist)));
        }
        put(
            MPMediaItemPropertyPlaybackDuration,
            obj(NSNumber::new_f64(track.duration)),
        );
        put(
            MPNowPlayingInfoPropertyElapsedPlaybackTime,
            obj(NSNumber::new_f64(position)),
        );
        put(
            MPNowPlayingInfoPropertyPlaybackRate,
            obj(NSNumber::new_f64(if playing { 1.0 } else { 0.0 })),
        );
        if let Some(i) = chapter {
            put(
                MPNowPlayingInfoPropertyChapterNumber,
                obj(NSNumber::new_usize(i)),
            );
            put(
                MPNowPlayingInfoPropertyChapterCount,
                obj(NSNumber::new_usize(track.chapters.len())),
            );
        }
        if let Some(art) = artwork {
            put(MPMediaItemPropertyArtwork, obj(art.0.clone()));
        }

        let info = NSDictionary::from_retained_objects(&keys, &values);
        let center = MPNowPlayingInfoCenter::defaultCenter();
        center.setNowPlayingInfo(Some(&info));
        center.setPlaybackState(if playing {
            MPNowPlayingPlaybackState::Playing
        } else {
            MPNowPlayingPlaybackState::Paused
        });
    }
}

pub fn clear() {
    unsafe {
        let center = MPNowPlayingInfoCenter::defaultCenter();
        center.setNowPlayingInfo(None);
        center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
    }
}

/// Cover art in the form Now Playing wants; built once per track.
pub struct Artwork(Retained<MPMediaItemArtwork>);

pub fn artwork(bytes: &[u8]) -> Option<Artwork> {
    unsafe {
        let data = NSData::with_bytes(bytes);
        let image = NSImage::initWithData(NSImage::alloc(), &data)?;
        let size = image.size();
        let handler = RcBlock::new(move |_: CGSize| -> NonNull<NSImage> { NonNull::from(&*image) });
        Some(Artwork(
            MPMediaItemArtwork::initWithBoundsSize_requestHandler(
                MPMediaItemArtwork::alloc(),
                size,
                &handler,
            ),
        ))
    }
}

fn obj<T: Message>(value: Retained<T>) -> Retained<AnyObject> {
    // Every Objective-C object is an AnyObject.
    unsafe { Retained::cast_unchecked(value) }
}
