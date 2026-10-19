// Windows release builds are GUI programs (no console window); debug builds keep the
// console for logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod assets;
mod platform;
mod state;
mod theme;
mod track;
mod ui;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use gpui::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, KeyBinding, Menu, MenuItem,
    PathPromptOptions, SystemMenuType, Task, Window, WindowBounds, WindowOptions, actions, px,
    size,
};
use gpui_component::slider::{SliderEvent, SliderState};

use platform::mic::MicWatcher;
use platform::player::Player;
use platform::{Remote, now_playing, updater};
use state::State;

actions!(
    bgm,
    [
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        CheckForUpdates,
        Open,
        TogglePlay,
        SkipBack,
        SkipForward,
        PrevChapter,
        NextChapter,
        VolumeUp,
        VolumeDown,
        FollowSystem,
        LightMode,
        DarkMode,
        Minimize,
        Zoom,
        ToggleMiniPlayer
    ]
);

/// Shown in the menu bar, Dock and window title (the crate and bundle ID stay bgm-audio).
const APP_NAME: &str = "Ambience";

const SKIP_SECONDS: f64 = 10.0;
/// On resume, rewind a little so playback doesn't restart mid-phrase.
const RESUME_REWIND: f64 = 3.0;
/// Wait this long after a microphone change before reacting, to ignore blips.
const MIC_DEBOUNCE: Duration = Duration::from_millis(1500);
const SAVE_EVERY_TICKS: u32 = 5;
/// Wait this long after the window stops moving before saving its frame.
const WINDOW_SAVE_DELAY: Duration = Duration::from_millis(500);
const DEFAULT_WINDOW_SIZE: (f32, f32) = (440., 620.);
/// Window height of the mini player (title bar inset, header, progress, controls).
/// The macOS content starts under the title bar; elsewhere the inset is 24 smaller.
const COMPACT_HEIGHT: f32 = if cfg!(target_os = "macos") {
    228.
} else {
    198.
};
/// Fits the controls row: transport buttons (with chapter buttons) plus the mini
/// player toggle on each side's column, gaps and window padding.
const MIN_WIDTH: f32 = 392.;

/// Things that happen outside the UI: system remote commands, microphone changes.
pub enum Event {
    Remote(Remote),
    MicChanged,
    /// The accent colour changed in System Settings.
    SystemColorsChanged,
    /// A file opened from Finder ("Open With", double-click, drop on the Dock icon).
    OpenFile(PathBuf),
    /// The updater installed an update (Windows) and wants the app to quit.
    Quit,
}

pub struct BgmApp {
    state: State,
    player: Option<Player>,
    artwork_np: Option<now_playing::Artwork>,
    /// The track's artwork at full size; ui.rs scales it down for the screen.
    artwork: Option<image::RgbaImage>,
    /// `artwork` scaled for the window's scale factor (the factor it was made for).
    artwork_ui: Option<(f32, Arc<gpui::RenderImage>)>,
    chapter: Option<usize>,
    mic: MicWatcher,
    /// Bundle ID of the app that is currently recording, if any.
    mic_app: Option<String>,
    /// We paused because of the microphone, so we may resume when it stops.
    auto_paused: bool,
    mic_check: Option<Task<()>>,
    window_save: Option<Task<()>>,
    error: Option<String>,
    focus: FocusHandle,
    volume_slider: Entity<SliderState>,
    /// Progress bar as a slider (0..1 of the duration), for drag-to-scrub.
    progress_slider: Entity<SliderState>,
    /// Last scrub; the bar isn't synced to the player right after, so it doesn't jump back.
    scrubbed_at: Option<std::time::Instant>,
    volume_save: Option<Task<()>>,
    ticks: u32,
    /// Position changed since the file was opened (played or seeked). Until then
    /// we keep the saved position, so the resume rewind doesn't pile up.
    moved: bool,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<gpui::Subscription>,
}

/// macOS: the mini player shows the desktop through a blur (ui.rs tints it with a
/// translucent background); the full window stays opaque.
fn set_backdrop(compact: bool, window: &mut gpui::Window) {
    if cfg!(target_os = "macos") {
        window.set_background_appearance(if compact {
            gpui::WindowBackgroundAppearance::Blurred
        } else {
            gpui::WindowBackgroundAppearance::Opaque
        });
    }
}

impl BgmApp {
    fn new(
        state: State,
        open_path: Option<PathBuf>,
        (tx, mut rx): (UnboundedSender<Event>, UnboundedReceiver<Event>),
        cx: &mut Context<Self>,
    ) -> Self {
        now_playing::register(tx.clone());
        platform::watch_accent(tx.clone());

        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = rx.next().await {
                if this.update(cx, |this, cx| this.handle(event, cx)).is_err() {
                    break;
                }
            }
        });
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        });
        let quit = cx.on_app_quit(|this, _| {
            this.record();
            this.state.save();
            now_playing::clear();
            async {}
        });

        let volume_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(1.0)
                .step(0.01)
                .default_value(state.volume)
        });
        let volume_changed = cx.subscribe(&volume_slider, |this, _, event: &SliderEvent, cx| {
            let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
            this.apply_volume(value.end(), cx);
        });

        let progress_slider = cx.new(|_| SliderState::new().min(0.0).max(1.0).step(0.0001));
        let scrubbed = cx.subscribe(&progress_slider, |this, _, event: &SliderEvent, cx| {
            let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
            let Some(duration) = this.player.as_ref().map(|p| p.track.duration) else {
                return;
            };
            this.scrubbed_at = Some(std::time::Instant::now());
            this.seek(value.end() as f64 * duration, cx);
        });

        let mut app = Self {
            state,
            player: None,
            artwork_np: None,
            artwork: None,
            artwork_ui: None,
            chapter: None,
            mic: MicWatcher::new(tx),
            mic_app: None,
            auto_paused: false,
            mic_check: None,
            window_save: None,
            error: None,
            focus: cx.focus_handle(),
            volume_slider,
            progress_slider,
            scrubbed_at: None,
            volume_save: None,
            ticks: 0,
            moved: false,
            _tasks: vec![events, ticker],
            _subscriptions: vec![quit, volume_changed, scrubbed],
        };

        // Restore the last session (paused), unless a file was passed on the command line.
        match open_path {
            Some(path) => app.open(&path, false, cx),
            None => app.restore(cx),
        }
        app.evaluate_mic(cx);
        app
    }

    fn restore(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.state.files.first().cloned() else {
            return;
        };
        if let Some(path) = entry.resolve() {
            if path != entry.path {
                self.state.relocate(0, path.clone());
            }
            self.open(&path, false, cx);
        }
    }

    pub fn open(&mut self, path: &Path, autoplay: bool, cx: &mut Context<Self>) {
        let path = &dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.record();
        if let Some(old) = self.player.take() {
            old.pause();
        }
        let Some(player) = Player::open(path) else {
            self.error = Some(format!("Can't play {}", path.display()));
            cx.notify();
            return;
        };
        self.error = None;
        player.set_volume(self.state.volume);

        let saved = self.state.entry(path).map_or(0.0, |e| e.position);
        let resume = Some(saved)
            .filter(|&p| p < player.track.duration - 5.0)
            .map(|p| (p - RESUME_REWIND).max(0.0))
            .unwrap_or(0.0);
        player.seek(resume);

        let track = &player.track;
        self.artwork_np = track.artwork.as_deref().and_then(now_playing::artwork);
        self.artwork = track
            .artwork
            .as_deref()
            .and_then(|bytes| image::load_from_memory(bytes).ok())
            .map(|image| image.into_rgba8());
        // Outside a window update, so this reaches every window's sprite atlas.
        if let Some((_, old)) = self.artwork_ui.take() {
            cx.drop_image(old, None);
        }
        self.chapter = track.chapter_at(resume);
        now_playing::use_chapter_buttons(!track.chapters.is_empty());
        self.state.touch(path, &track.title, saved, track.duration);
        self.state.save();
        self.moved = false;

        if autoplay {
            player.play();
            self.auto_paused = false;
        }
        self.player = Some(player);
        self.publish(Some(resume), autoplay);
        cx.notify();
    }

    pub fn prompt_open(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                if let Some(path) = paths.into_iter().next() {
                    this.update(cx, |this, cx| this.open(&path, true, cx)).ok();
                }
            }
        })
        .detach();
    }

    pub fn open_recent(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.state.files.get(index).cloned() else {
            return;
        };
        match entry.resolve() {
            Some(path) => {
                if path != entry.path {
                    self.state.relocate(index, path.clone());
                }
                self.open(&path, true, cx);
            }
            None => {
                self.error = Some(format!("Missing: {}", entry.path.display()));
                cx.notify();
            }
        }
    }

    pub fn forget_recent(&mut self, index: usize, cx: &mut Context<Self>) {
        self.state.remove(index);
        self.state.save();
        cx.notify();
    }

    pub fn is_playing(&self) -> bool {
        self.player.as_ref().is_some_and(|p| p.is_playing())
    }

    pub fn play(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        player.play();
        self.moved = true;
        self.auto_paused = false;
        self.publish(None, true);
        cx.notify();
    }

    pub fn pause(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        player.pause();
        self.auto_paused = false;
        self.record();
        self.publish(None, false);
        cx.notify();
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.is_playing() {
            self.pause(cx);
        } else {
            self.play(cx);
        }
    }

    pub fn seek(&mut self, seconds: f64, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        let seconds = seconds.clamp(0.0, player.track.duration);
        player.seek(seconds);
        self.chapter = player.track.chapter_at(seconds);
        let playing = player.is_playing();
        self.moved = true;
        self.record_at(seconds);
        self.publish(Some(seconds), playing);
        cx.notify();
    }

    pub fn skip(&mut self, delta: f64, cx: &mut Context<Self>) {
        if let Some(player) = &self.player {
            self.seek(player.position() + delta, cx);
        }
    }

    pub fn next_chapter(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        let position = player.position();
        if let Some(c) = player
            .track
            .chapters
            .iter()
            .find(|c| c.start > position + 0.5)
        {
            self.seek(c.start, cx);
        }
    }

    pub fn prev_chapter(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        let position = player.position();
        let chapters = &player.track.chapters;
        let Some(i) = player.track.chapter_at(position) else {
            self.seek(0.0, cx);
            return;
        };
        // Like a CD player: first go to the start of this chapter, then to the previous one.
        let target = if position - chapters[i].start > 3.0 || i == 0 {
            chapters[i].start
        } else {
            chapters[i - 1].start
        };
        self.seek(target, cx);
    }

    /// Sets the volume from outside the slider (keys, menu) and moves the slider along.
    pub fn set_volume(&mut self, volume: f32, window: &mut Window, cx: &mut Context<Self>) {
        let volume = volume.clamp(0.0, 1.0);
        self.volume_slider
            .update(cx, |slider, cx| slider.set_value(volume, window, cx));
        self.apply_volume(volume, cx);
    }

    /// Applies a volume change; saved once it stops changing (the slider fires per mouse move).
    fn apply_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        self.state.volume = volume.clamp(0.0, 1.0);
        if let Some(player) = &self.player {
            player.set_volume(self.state.volume);
        }
        self.volume_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            this.update(cx, |this, _| this.state.save()).ok();
        }));
        cx.notify();
    }

    /// Remembers the window frame; saved once the window stops moving.
    fn window_changed(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        // Saved once the window settles. The window learns its new display separately
        // from the move, so the display is read here, not when the move is reported.
        self.window_save = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(WINDOW_SAVE_DELAY).await;
            this.update_in(cx, |this, window, cx| this.save_window(window, cx))
                .ok();
        }));
    }

    fn save_window(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let (mode, bounds) = match window.window_bounds() {
            WindowBounds::Windowed(b) => (state::WindowMode::Windowed, b),
            WindowBounds::Maximized(b) => (state::WindowMode::Maximized, b),
            WindowBounds::Fullscreen(b) => (state::WindowMode::Fullscreen, b),
        };
        let saved = state::SavedWindow {
            display: window
                .display(cx)
                .and_then(|d| d.uuid().ok())
                .map(|uuid| uuid.to_string()),
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            mode,
        };
        if self.state.window.as_ref() != Some(&saved) {
            self.state.window = Some(saved);
            self.state.save();
        }
    }

    /// View menu: remembered, applied to the whole app (title bar and Sparkle too).
    pub fn set_appearance(
        &mut self,
        appearance: state::Appearance,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.state.appearance = appearance;
        self.state.save();
        cx.set_menus(app_menus(&self.state, cx));
        platform::set_app_appearance(appearance);
        cx.set_global(theme::Choice(appearance));
        theme::apply(cx.window_appearance(), cx);
        platform::style_title_bar(window, theme::palette(cx).dark);
        cx.notify();
    }

    /// Windows: the app's menus as a popup from the "⋯" button. macOS has the menu bar.
    pub fn show_window_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(not(target_os = "macos"))]
        window_menu(&self.state, cx).show(position, window, cx);
        #[cfg(target_os = "macos")]
        let _ = (position, window, cx);
    }

    /// Switches between the full window and the mini player, resizing the window.
    pub fn toggle_compact(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        let current = window.viewport_size();
        let height = if self.state.compact {
            self.state.expanded_height.unwrap_or(DEFAULT_WINDOW_SIZE.1)
        } else {
            self.state.expanded_height = Some(current.height.into());
            COMPACT_HEIGHT
        };
        self.state.compact = !self.state.compact;
        self.state.save();
        cx.set_menus(app_menus(&self.state, cx));
        window.resize(size(current.width, px(height)));
        set_backdrop(self.state.compact, window);
        cx.notify();
    }

    pub fn set_tab(&mut self, tab: state::Tab, cx: &mut Context<Self>) {
        self.state.tab = tab;
        self.state.save();
        cx.notify();
    }

    pub fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        self.state.looping = !self.state.looping;
        self.state.save();
        cx.notify();
    }

    pub fn toggle_mic_auto_pause(&mut self, cx: &mut Context<Self>) {
        self.state.mic_auto_pause = !self.state.mic_auto_pause;
        self.state.save();
        if !self.state.mic_auto_pause && self.auto_paused {
            self.play(cx);
        }
        self.evaluate_mic(cx);
        cx.notify();
    }

    fn handle(&mut self, event: Event, cx: &mut Context<Self>) {
        match event {
            Event::Remote(remote) => match remote {
                Remote::Play => self.play(cx),
                Remote::Pause => self.pause(cx),
                Remote::Toggle => self.toggle(cx),
                Remote::Next => self.next_chapter(cx),
                Remote::Previous => self.prev_chapter(cx),
                Remote::SkipForward => self.skip(SKIP_SECONDS, cx),
                Remote::SkipBackward => self.skip(-SKIP_SECONDS, cx),
                Remote::Seek(seconds) => self.seek(seconds, cx),
            },
            Event::Quit => cx.quit(),
            Event::OpenFile(path) => {
                self.open(&path, true, cx);
                cx.activate(true);
            }
            Event::SystemColorsChanged => {
                theme::apply(cx.window_appearance(), cx);
                cx.notify();
            }
            Event::MicChanged => {
                self.mic.sync();
                self.mic_check = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(MIC_DEBOUNCE).await;
                    this.update(cx, |this, cx| this.evaluate_mic(cx)).ok();
                }));
            }
        }
    }

    fn evaluate_mic(&mut self, cx: &mut Context<Self>) {
        self.mic_app = self.mic.recording_app();
        if self.state.mic_auto_pause {
            if self.mic_app.is_some() && self.is_playing() {
                self.pause(cx);
                self.auto_paused = true;
            } else if self.mic_app.is_none() && self.auto_paused {
                self.play(cx);
            }
        }
        cx.notify();
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        if !player.is_playing() {
            return;
        }
        let position = player.position();
        let duration = player.track.duration;

        if position >= duration - 0.3 {
            if self.state.looping {
                self.seek(0.0, cx);
                if let Some(p) = &self.player {
                    p.play();
                }
            } else {
                self.pause(cx);
                self.seek(0.0, cx);
            }
            return;
        }

        let chapter = player.track.chapter_at(position);
        if chapter != self.chapter {
            self.chapter = chapter;
            self.publish(Some(position), true);
        }

        self.ticks += 1;
        if self.ticks % SAVE_EVERY_TICKS == 0 {
            self.record_at(position);
        }
        cx.notify();
    }

    /// Saves the current file's position.
    fn record(&mut self) {
        if !self.moved {
            return;
        }
        if let Some(position) = self.player.as_ref().map(|p| p.position()) {
            self.record_at(position);
        }
    }

    fn record_at(&mut self, position: f64) {
        let Some(player) = &self.player else { return };
        let track = &player.track;
        self.state
            .touch(&track.path, &track.title, position, track.duration);
        self.state.save();
    }

    fn publish(&self, position: Option<f64>, playing: bool) {
        if let Some(player) = &self.player {
            let position = position.unwrap_or_else(|| player.position());
            now_playing::update(&player.track, self.artwork_np.as_ref(), position, playing);
        }
    }
}

/// The saved window frame if its title bar is still on a connected display,
/// otherwise a centered window of the default size.
/// The file to open from the command line. Skips Cocoa's `-Key value` user-default
/// overrides (e.g. `-AppleInterfaceStyle Dark`), which AppKit reads on its own.
fn file_argument() -> Option<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg.to_string_lossy().starts_with('-') {
            args.next();
        } else {
            return Some(PathBuf::from(arg));
        }
    }
    None
}

/// The saved frame and the display it belongs on, or a centred default window.
/// GPUI window bounds are relative to their display, so both are needed.
fn restored_window_bounds(state: &State, cx: &App) -> (WindowBounds, Option<gpui::DisplayId>) {
    let Some(w) = &state.window else {
        let (width, height) = DEFAULT_WINDOW_SIZE;
        let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
        return (WindowBounds::Windowed(bounds), None);
    };
    // The saved display, or the main one when it is gone (or wasn't recorded yet).
    let display = w
        .display
        .as_ref()
        .and_then(|uuid| {
            cx.displays()
                .into_iter()
                .find(|d| d.uuid().is_ok_and(|u| u.to_string() == *uuid))
        })
        .or_else(|| cx.primary_display());
    let display_id = display.as_ref().map(|d| d.id());
    // Saved by an older version with a smaller minimum width.
    let size = size(px(w.width.max(MIN_WIDTH)), px(w.height));
    let mut bounds = Bounds::new(gpui::point(px(w.x), px(w.y)), size);
    // A point inside the title bar must be on the display, so the window can be dragged.
    let grab = gpui::point(bounds.origin.x + px(60.), bounds.origin.y + px(12.));
    let on_display = display
        .is_some_and(|d| Bounds::new(gpui::point(px(0.), px(0.)), d.bounds().size).contains(&grab));
    if !on_display {
        bounds = Bounds::centered(display_id, size, cx);
    }
    let bounds = match w.mode {
        state::WindowMode::Windowed => WindowBounds::Windowed(bounds),
        state::WindowMode::Maximized => WindowBounds::Maximized(bounds),
        state::WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
    };
    (bounds, display_id)
}

/// The menu bar. Rebuilt when a checked item (appearance, mini player) changes.
fn app_menus(state: &State, cx: &App) -> Vec<Menu> {
    use state::Appearance;
    let appearance = state.appearance;
    let menu = |name: &str, items| Menu {
        name: name.to_string().into(),
        items,
        disabled: false,
    };
    // Sparkle only exists inside the .app bundle; without it there is no menu item.
    let mut app_menu = Vec::new();
    if cx.has_global::<updater::Updater>() {
        app_menu.push(MenuItem::action("Check for Updates…", CheckForUpdates));
        app_menu.push(MenuItem::separator());
    }
    app_menu.extend([
        MenuItem::os_submenu("Services", SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action(format!("Hide {APP_NAME}"), Hide),
        MenuItem::action("Hide Others", HideOthers),
        MenuItem::action("Show All", ShowAll),
        MenuItem::separator(),
        MenuItem::action(format!("Quit {APP_NAME}"), Quit),
    ]);
    vec![
        menu(APP_NAME, app_menu),
        menu("File", vec![MenuItem::action("Open…", Open)]),
        menu(
            "View",
            vec![
                MenuItem::action("Follow System", FollowSystem)
                    .checked(appearance == Appearance::System),
                MenuItem::action("Light Mode", LightMode).checked(appearance == Appearance::Light),
                MenuItem::action("Dark Mode", DarkMode).checked(appearance == Appearance::Dark),
            ],
        ),
        menu(
            "Playback",
            vec![
                MenuItem::action("Play/Pause", TogglePlay),
                MenuItem::separator(),
                MenuItem::action("Back 10s", SkipBack),
                MenuItem::action("Forward 10s", SkipForward),
                MenuItem::action("Previous Chapter", PrevChapter),
                MenuItem::action("Next Chapter", NextChapter),
                MenuItem::separator(),
                MenuItem::action("Volume Up", VolumeUp),
                MenuItem::action("Volume Down", VolumeDown),
            ],
        ),
        // Named "Window", so macOS adds the window list and "Bring All to Front".
        menu(
            "Window",
            vec![
                MenuItem::action("Minimize", Minimize),
                MenuItem::action(
                    if cfg!(target_os = "macos") {
                        "Zoom"
                    } else {
                        "Maximize"
                    },
                    Zoom,
                ),
                MenuItem::separator(),
                MenuItem::action("Mini Player", ToggleMiniPlayer).checked(state.compact),
            ],
        ),
    ]
}

/// Windows has no menu bar for GPUI menus; the "⋯" button shows this as a native popup
/// menu instead: Open, the View/Playback/Window menus as submenus, updates and Exit.
#[cfg(not(target_os = "macos"))]
pub fn window_menu(state: &State, cx: &App) -> gpui_component::native_menu::NativeMenu {
    use gpui_component::native_menu::NativeMenu;
    let mut menu = NativeMenu::new().menu("Open…", Box::new(Open)).separator();
    // Skip the macOS app menu (Services, Hide, Quit) and File (Open is above).
    for m in app_menus(state, cx).into_iter().skip(2) {
        menu = menu.submenu(m.name.clone(), NativeMenu::from(m));
    }
    menu = menu.separator();
    if cx.has_global::<updater::Updater>() {
        menu = menu.menu("Check for Updates…", Box::new(CheckForUpdates));
    }
    menu.menu("Exit", Box::new(Quit))
}

fn main() {
    let open_path = file_argument();
    // Created before launch: macOS can deliver opened files before the window exists.
    let (tx, rx) = futures::channel::mpsc::unbounded::<Event>();
    let events = Rc::new(RefCell::new(Some((tx.clone(), rx))));
    let updater_tx = tx.clone();

    let app = gpui_platform::application().with_assets(assets::AppAssets);
    app.on_open_urls(move |urls| {
        for path in urls.iter().filter_map(|url| platform::file_url_path(url)) {
            tx.unbounded_send(Event::OpenFile(path)).ok();
        }
    });
    app.run(move |cx: &mut App| {
        gpui_component::init(cx);
        theme::apply(cx.window_appearance(), cx);
        // gpui-component resets its theme from its registry once effects flush (and on
        // registry changes); registered after theirs, this puts our palette back on top.
        cx.observe_global::<gpui_component::ThemeRegistry>(|cx| {
            theme::apply(cx.window_appearance(), cx)
        })
        .detach();
        // "secondary" is ⌘ on macOS and Ctrl elsewhere.
        cx.bind_keys([
            KeyBinding::new("secondary-q", Quit, None),
            KeyBinding::new("secondary-o", Open, None),
            KeyBinding::new("space", TogglePlay, None),
            KeyBinding::new("left", SkipBack, None),
            KeyBinding::new("right", SkipForward, None),
            KeyBinding::new("shift-left", PrevChapter, None),
            KeyBinding::new("shift-right", NextChapter, None),
            KeyBinding::new("up", VolumeUp, None),
            KeyBinding::new("down", VolumeDown, None),
            KeyBinding::new("secondary-shift-m", ToggleMiniPlayer, None),
        ]);
        // macOS conventions without a Windows equivalent.
        #[cfg(target_os = "macos")]
        cx.bind_keys([
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
            KeyBinding::new("cmd-m", Minimize, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        // GPUI has no standard app menu; these mirror what Zed wires up by hand.
        cx.on_action(|_: &Hide, cx| cx.hide());
        cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());

        if let Some(updater) = updater::Updater::start(updater_tx.clone()) {
            cx.set_global(updater);
        }
        cx.on_action(|_: &CheckForUpdates, cx| {
            if let Some(updater) = cx.try_global::<updater::Updater>() {
                updater.check_for_updates();
            }
        });
        let state = State::load();
        cx.set_menus(app_menus(&state, cx));
        platform::set_app_appearance(state.appearance);
        cx.set_global(theme::Choice(state.appearance));
        theme::apply(cx.window_appearance(), cx);
        let (window_bounds, display_id) = restored_window_bounds(&state, cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(window_bounds),
                display_id,
                window_min_size: Some(size(px(MIN_WIDTH), px(COMPACT_HEIGHT))),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(APP_NAME.into()),
                    // macOS: content runs under the title bar, traffic lights inset
                    // (ui.rs leaves room for them). Elsewhere: the system title bar.
                    appears_transparent: cfg!(target_os = "macos"),
                    traffic_light_position: Some(gpui::point(px(12.), px(12.))),
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| {
                    let events = events.take().expect("one window");
                    let mut app = BgmApp::new(state, open_path.clone(), events, cx);
                    let observer = cx.observe_window_bounds(window, |this, window, cx| {
                        this.window_changed(window, cx)
                    });
                    app._subscriptions.push(observer);
                    let appearance = cx.observe_window_appearance(window, |_, window, cx| {
                        theme::apply(window.appearance(), cx);
                        platform::style_title_bar(window, theme::palette(cx).dark);
                        cx.notify();
                    });
                    app._subscriptions.push(appearance);
                    app
                });
                platform::style_title_bar(window, theme::palette(cx).dark);
                set_backdrop(view.read(cx).state.compact, window);
                window.focus(&view.read(cx).focus.clone(), cx);
                view
            },
        )
        .unwrap();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
