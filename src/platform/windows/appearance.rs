//! Accent colour from UISettings; the dark title bar via DWM. Light/dark itself is the
//! palette's job (theme::apply follows the View menu choice).

use std::ffi::c_void;

use futures::channel::mpsc::UnboundedSender;
use gpui::Hsla;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Foundation::TypedEventHandler;
use windows::UI::ViewManagement::{UIColorType, UISettings};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::core::BOOL;

use crate::Event;
use crate::state::Appearance;

/// The accent colour picked in Settings → Personalization → Colors.
pub fn accent_color() -> Option<Hsla> {
    let c = UISettings::new()
        .ok()?
        .GetColorValue(UIColorType::Accent)
        .ok()?;
    let rgba = gpui::Rgba {
        r: c.R as f32 / 255.0,
        g: c.G as f32 / 255.0,
        b: c.B as f32 / 255.0,
        a: 1.0,
    };
    Some(rgba.into())
}

/// Windows has no window colours worth following here; the palette keeps its own.
pub fn system_colors(_dark: bool) -> Option<crate::platform::SystemColors> {
    None
}

/// Sends `Event::SystemColorsChanged` when the accent colour (or light/dark) changes.
pub fn watch_accent(tx: UnboundedSender<Event>) {
    let Ok(settings) = UISettings::new() else {
        return;
    };
    let handler = TypedEventHandler::new(move |_, _| {
        tx.unbounded_send(Event::SystemColorsChanged).ok();
        Ok(())
    });
    if settings.ColorValuesChanged(&handler).is_ok() {
        // The event only fires while this UISettings lives: keep it for the app's run.
        std::mem::forget(settings);
    }
}

/// Nothing app-wide to set on Windows; see `style_title_bar`.
pub fn set_app_appearance(_choice: Appearance) {}

/// GPUI darkens the title bar only for the system's dark mode; this follows the app's.
pub fn style_title_bar(window: &gpui::Window, dark: bool) {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    let value = BOOL::from(dark);
    unsafe {
        let _ = DwmSetWindowAttribute(
            HWND(h.hwnd.get() as *mut c_void),
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &value as *const BOOL as *const c_void,
            size_of::<BOOL>() as u32,
        );
    }
}
