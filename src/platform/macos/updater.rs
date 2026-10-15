//! Automatic updates with Sparkle.
//!
//! packaging/bundle.sh puts Sparkle.framework into Contents/Frameworks. It is
//! loaded at runtime instead of linked, so `cargo run` (no bundle, no framework)
//! still works and simply has no updater. Feed URL and public key are in Info.plist.

use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSBundle, NSString};

pub struct Updater {
    /// SPUStandardUpdaterController: owns the updater and Sparkle's own windows.
    controller: Retained<AnyObject>,
}

impl gpui::Global for Updater {}

impl Updater {
    /// Loads the bundled framework and starts scheduled update checks.
    pub fn start() -> Option<Self> {
        let frameworks = NSBundle::mainBundle().privateFrameworksPath()?;
        let path =
            frameworks.stringByAppendingPathComponent(&NSString::from_str("Sparkle.framework"));
        let bundle = NSBundle::bundleWithPath(&path)?;
        // Loading runs the framework's initializers; it is our own signed copy.
        if !unsafe { bundle.load() } {
            eprintln!("bgm-audio: could not load {path}");
            return None;
        }
        let class = AnyClass::get(c"SPUStandardUpdaterController")?;
        let none: Option<&AnyObject> = None;
        let controller: Retained<AnyObject> = unsafe {
            let allocated: Allocated<AnyObject> = msg_send![class, alloc];
            msg_send![
                allocated,
                initWithStartingUpdater: true,
                updaterDelegate: none,
                userDriverDelegate: none
            ]
        };
        Some(Self { controller })
    }

    /// "Check for Updates…": Sparkle shows its own progress and result windows.
    pub fn check_for_updates(&self) {
        let none: Option<&AnyObject> = None;
        unsafe {
            let _: () = msg_send![&*self.controller, checkForUpdates: none];
        }
    }
}
