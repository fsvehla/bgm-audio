//! Light/dark and the system colours: NSApp.appearance, NSColor.controlAccentColor and
//! the window colours (windowBackgroundColor, labelColor, …).

use std::cell::Cell;
use std::ptr::NonNull;

use block2::{RcBlock, StackBlock};
use futures::channel::mpsc::UnboundedSender;
use gpui::Hsla;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSColor,
    NSColorSpace, NSSystemColorsDidChangeNotification,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};

use crate::Event;
use crate::platform::SystemColors;
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

/// The window colours as AppKit draws them in light (Aqua) or dark (Dark Aqua).
pub fn system_colors(dark: bool) -> Option<SystemColors> {
    let name = unsafe {
        if dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    let appearance = NSAppearance::appearanceNamed(name)?;
    let colors = Cell::new(None);
    // Dynamic colours resolve against the current drawing appearance.
    let resolve = StackBlock::new(|| {
        let srgb = |color: objc2::rc::Retained<NSColor>| {
            let c = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
            Some(gpui::Rgba {
                r: c.redComponent() as f32,
                g: c.greenComponent() as f32,
                b: c.blueComponent() as f32,
                a: c.alphaComponent() as f32,
            })
        };
        colors.set((|| {
            Some(SystemColors {
                background: srgb(NSColor::windowBackgroundColor())?,
                fill: srgb(NSColor::systemFillColor())?,
                label: srgb(NSColor::labelColor())?,
                secondary_label: srgb(NSColor::secondaryLabelColor())?,
                red: srgb(NSColor::systemRedColor())?,
            })
        })());
    });
    appearance.performAsCurrentDrawingAppearance(&resolve);
    colors.get()
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
