//! Bookmarks (files found again after a move) and file URLs from the system.

use std::path::{Path, PathBuf};

use objc2::AllocAnyThread;
use objc2::runtime::Bool;
use objc2_foundation::{
    NSData, NSDataBase64DecodingOptions, NSDataBase64EncodingOptions, NSString, NSURL,
    NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
};

/// The local path of a `file://` URL from the system.
pub fn file_url_path(url: &str) -> Option<PathBuf> {
    let url = NSURL::URLWithString(&NSString::from_str(url))?;
    if !url.isFileURL() {
        return None;
    }
    Some(PathBuf::from(url.path()?.to_string()))
}

/// A bookmark for `path` that survives moves and renames, as base64.
pub fn make_bookmark(path: &Path) -> Option<String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let data = url
        .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
            NSURLBookmarkCreationOptions::empty(),
            None,
            None,
        )
        .ok()?;
    Some(
        data.base64EncodedStringWithOptions(NSDataBase64EncodingOptions::empty())
            .to_string(),
    )
}

/// Where the bookmarked file is now.
pub fn resolve_bookmark(base64: &str) -> Option<PathBuf> {
    let data = NSData::initWithBase64EncodedString_options(
        NSData::alloc(),
        &NSString::from_str(base64),
        NSDataBase64DecodingOptions::empty(),
    )?;
    let mut stale = Bool::NO;
    let url = unsafe {
        NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
            &data,
            NSURLBookmarkResolutionOptions::WithoutUI,
            None,
            &mut stale,
        )
    }
    .ok()?;
    url.path().map(|p| PathBuf::from(p.to_string()))
}
