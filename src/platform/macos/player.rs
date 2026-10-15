//! Playback with AVPlayer; title, artist, chapters and artwork read with AVAsset.

use std::path::Path;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_av_foundation::{
    AVAsset, AVMetadataCommonKeyArtist, AVMetadataCommonKeyArtwork, AVMetadataCommonKeyTitle,
    AVMetadataItem, AVPlayer, AVPlayerTimeControlStatus,
};
use objc2_core_media::{CMTime, kCMTimeZero};
use objc2_foundation::{NSArray, NSString, NSURL};

use crate::track::{Chapter, Track};

pub struct Player {
    av: Retained<AVPlayer>,
    pub track: Track,
}

impl Player {
    /// Main thread only (AVPlayer).
    pub fn open(path: &Path) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        if !path.is_file() {
            return None;
        }
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let track = unsafe { read_track(path, &url) }?;
        let av = unsafe { AVPlayer::playerWithURL(&url, mtm) };
        Some(Self { av, track })
    }

    pub fn play(&self) {
        unsafe { self.av.play() }
    }

    pub fn pause(&self) {
        unsafe { self.av.pause() }
    }

    /// True while playing or waiting to play (buffering).
    pub fn is_playing(&self) -> bool {
        unsafe { self.av.timeControlStatus() != AVPlayerTimeControlStatus::Paused }
    }

    pub fn position(&self) -> f64 {
        let s = unsafe { self.av.currentTime().seconds() };
        if s.is_finite() { s.max(0.0) } else { 0.0 }
    }

    pub fn seek(&self, seconds: f64) {
        let seconds = seconds.clamp(0.0, self.track.duration.max(0.0));
        unsafe {
            // Exact seeks: chapter starts should land where they say.
            let zero = kCMTimeZero;
            self.av.seekToTime_toleranceBefore_toleranceAfter(
                CMTime::with_seconds(seconds, 600),
                zero,
                zero,
            );
        }
    }

    pub fn set_volume(&self, volume: f32) {
        unsafe { self.av.setVolume(volume.clamp(0.0, 1.0)) }
    }
}

unsafe fn read_track(path: &Path, url: &NSURL) -> Option<Track> {
    unsafe {
        let title_key = AVMetadataCommonKeyTitle?;
        let artist_key = AVMetadataCommonKeyArtist?;
        let artwork_key = AVMetadataCommonKeyArtwork?;

        let asset = AVAsset::assetWithURL(url);
        #[allow(deprecated)]
        let duration = asset.duration().seconds();
        if !duration.is_finite() || duration <= 0.0 {
            return None;
        }

        #[allow(deprecated)]
        let common = asset.commonMetadata();
        let string_for = |key: &NSString| {
            find_item(&common, key)
                .and_then(|i| i.stringValue())
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
        };
        let title = string_for(title_key).unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let artist = string_for(artist_key);
        let artwork = find_item(&common, artwork_key)
            .and_then(|i| i.dataValue())
            .map(|d| d.to_vec());

        // ffmpeg-written chapters are tagged with the "undetermined" locale, which
        // the preferred-languages lookup skips, so ask for the first locale directly.
        #[allow(deprecated)]
        let groups = asset
            .availableChapterLocales()
            .firstObject()
            .map(|locale| {
                asset.chapterMetadataGroupsWithTitleLocale_containingItemsWithCommonKeys(
                    &locale, None,
                )
            })
            .unwrap_or_default();
        let chapters = groups
            .iter()
            .map(|g| {
                let items = g.items();
                let title = find_item(&items, title_key)
                    .and_then(|i| i.stringValue())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                Chapter {
                    title,
                    start: g.timeRange().start.seconds(),
                }
            })
            .collect();

        Some(Track {
            path: path.to_path_buf(),
            title,
            artist,
            duration,
            chapters,
            artwork,
        })
    }
}

unsafe fn find_item(
    items: &NSArray<AVMetadataItem>,
    key: &NSString,
) -> Option<Retained<AVMetadataItem>> {
    items
        .iter()
        .find(|i| unsafe { i.commonKey() }.is_some_and(|k| &*k == key))
}
