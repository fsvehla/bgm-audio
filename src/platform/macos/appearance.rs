//! Light/dark and the accent colour: NSApp.appearance and NSColor.controlAccentColor.

use std::ptr::NonNull;

use block2::RcBlock;
use futures::channel::mpsc::UnboundedSender;
use gpui::Hsla;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSColor,
    NSColorSpace, NSSystemColorsDidChangeNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};

use crate::Event;
use crate::state::Appearance;

/// Sets `NSApp.appearance` (nil follows the system).
/// Windows then report the new appearance, which `apply` turns into the palette.
pub fn set_app_appearance(choice: Appearance) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let appearance = unsafe {
        match choice {
            Appearance::System => None,
            Appearance::Light => NSAppearance::appearanceNamed(NSAppearanceNameAqua),
            Appearance::Dark => NSAppearance::appearanceNamed(NSAppearanceNameDarkAqua),
        }
    };
    app.setAppearance(appearance.as_deref());
}

/// `NSColor.controlAccentColor` in sRGB: the accent colour picked in System Settings
/// (blue for "Multicolor").
pub fn accent_color() -> Option<Hsla> {
    let color = NSColor::controlAccentColor();
    let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let rgba = gpui::Rgba {
        r: srgb.redComponent() as f32,
        g: srgb.greenComponent() as f32,
        b: srgb.blueComponent() as f32,
        a: 1.0,
    };
    Some(rgba.into())
}

/// Sends `Event::SystemColorsChanged` when the accent colour changes in System Settings.
pub fn watch_accent(tx: UnboundedSender<Event>) {
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        tx.unbounded_send(Event::SystemColorsChanged).ok();
    });
    let center = NSNotificationCenter::defaultCenter();
    // The observer token lives for the whole app run, like the notification itself.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSSystemColorsDidChangeNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &block,
        )
    };
    std::mem::forget(token);
}

/// macOS: the title bar follows NSApp.appearance (see `set_app_appearance`).
pub fn style_title_bar(_window: &gpui::Window, _dark: bool) {}
