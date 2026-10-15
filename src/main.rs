mod mic;
mod now_playing;
mod player;
mod state;
mod theme;
mod ui;
mod updater;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use gpui::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, Image, ImageFormat, KeyBinding, Menu,
    MenuItem, PathPromptOptions, SystemMenuType, Task, Window, WindowBounds, WindowOptions,
    actions, px, size,
};
use gpui_component::slider::{SliderEvent, SliderState};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_media_player::MPMediaItemArtwork;

use mic::MicWatcher;
use now_playing::Remote;
use player::Player;
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
        DarkMode
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
const COMPACT_HEIGHT: f32 = 236.;
const MIN_WIDTH: f32 = 360.;

/// Things that happen outside the UI: system remote commands, microphone changes.
pub enum Event {
    Remote(Remote),
    MicChanged,
    /// The accent colour changed in System Settings.
    SystemColorsChanged,
}

pub struct BgmApp {
    state: State,
    player: Option<Player>,
    artwork_np: Option<Retained<MPMediaItemArtwork>>,
    artwork_ui: Option<Arc<Image>>,
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

impl BgmApp {
    fn new(state: State, open_path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Event>();
        now_playing::register(tx.clone());
        theme::watch_accent(tx.clone());

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
        let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        self.record();
        if let Some(old) = self.player.take() {
            old.pause();
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(player) = Player::open(path, mtm) else {
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
        self.artwork_ui = track.artwork.as_ref().and_then(|bytes| {
            Some(Arc::new(Image::from_bytes(
                image_format(bytes)?,
                bytes.clone(),
            )))
        });
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
        let (mode, bounds) = match window.window_bounds() {
            WindowBounds::Windowed(b) => (state::WindowMode::Windowed, b),
            WindowBounds::Maximized(b) => (state::WindowMode::Maximized, b),
            WindowBounds::Fullscreen(b) => (state::WindowMode::Fullscreen, b),
        };
        let saved = state::SavedWindow {
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            mode,
        };
        if self.state.window == Some(saved) {
            return;
        }
        self.state.window = Some(saved);
        self.window_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WINDOW_SAVE_DELAY).await;
            this.update(cx, |this, _| this.state.save()).ok();
        }));
    }

    /// Switches between the full window and the mini player, resizing the window.
    /// View menu: remembered, applied to the whole app (title bar and Sparkle too).
    pub fn set_appearance(&mut self, appearance: state::Appearance, cx: &mut Context<Self>) {
        self.state.appearance = appearance;
        self.state.save();
        cx.set_menus(app_menus(appearance, cx));
        theme::set_app_appearance(appearance);
        theme::apply(cx.window_appearance(), cx);
        cx.notify();
    }

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
        window.resize(size(current.width, px(height)));
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

fn image_format(bytes: &[u8]) -> Option<ImageFormat> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some(ImageFormat::Png),
        [0xFF, 0xD8, ..] => Some(ImageFormat::Jpeg),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(ImageFormat::Webp),
        _ => None,
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

fn restored_window_bounds(state: &State, cx: &App) -> WindowBounds {
    if let Some(w) = state.window {
        let bounds = Bounds::new(
            gpui::point(px(w.x), px(w.y)),
            size(px(w.width), px(w.height)),
        );
        // A point inside the title bar, so the window can always be dragged.
        let grab = gpui::point(bounds.origin.x + px(60.), bounds.origin.y + px(12.));
        if cx.displays().iter().any(|d| d.bounds().contains(&grab)) {
            return match w.mode {
                state::WindowMode::Windowed => WindowBounds::Windowed(bounds),
                state::WindowMode::Maximized => WindowBounds::Maximized(bounds),
                state::WindowMode::Fullscreen => WindowBounds::Fullscreen(bounds),
            };
        }
    }
    let (width, height) = DEFAULT_WINDOW_SIZE;
    WindowBounds::Windowed(Bounds::centered(None, size(px(width), px(height)), cx))
}

/// The menu bar. Rebuilt when the View menu's checked item changes.
fn app_menus(appearance: state::Appearance, cx: &App) -> Vec<Menu> {
    use state::Appearance;
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
    ]
}

fn main() {
    let open_path = file_argument();

    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        theme::apply(cx.window_appearance(), cx);
        // gpui-component resets its theme from its registry once effects flush (and on
        // registry changes); registered after theirs, this puts our palette back on top.
        cx.observe_global::<gpui_component::ThemeRegistry>(|cx| {
            theme::apply(cx.window_appearance(), cx)
        })
        .detach();
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
            KeyBinding::new("cmd-o", Open, None),
            KeyBinding::new("space", TogglePlay, None),
            KeyBinding::new("left", SkipBack, None),
            KeyBinding::new("right", SkipForward, None),
            KeyBinding::new("shift-left", PrevChapter, None),
            KeyBinding::new("shift-right", NextChapter, None),
            KeyBinding::new("up", VolumeUp, None),
            KeyBinding::new("down", VolumeDown, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        // GPUI has no standard app menu; these mirror what Zed wires up by hand.
        cx.on_action(|_: &Hide, cx| cx.hide());
        cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());

        if let Some(updater) = updater::Updater::start() {
            cx.set_global(updater);
        }
        cx.on_action(|_: &CheckForUpdates, cx| {
            if let Some(updater) = cx.try_global::<updater::Updater>() {
                updater.check_for_updates();
            }
        });
        let state = State::load();
        cx.set_menus(app_menus(state.appearance, cx));
        theme::set_app_appearance(state.appearance);
        theme::apply(cx.window_appearance(), cx);
        let window_bounds = restored_window_bounds(&state, cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(window_bounds),
                window_min_size: Some(size(px(MIN_WIDTH), px(COMPACT_HEIGHT))),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(APP_NAME.into()),
                    appears_transparent: true,
                    traffic_light_position: Some(gpui::point(px(12.), px(12.))),
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| {
                    let mut app = BgmApp::new(state, open_path.clone(), cx);
                    let observer = cx.observe_window_bounds(window, |this, window, cx| {
                        this.window_changed(window, cx)
                    });
                    app._subscriptions.push(observer);
                    let appearance = cx.observe_window_appearance(window, |_, window, cx| {
                        theme::apply(window.appearance(), cx);
                        cx.notify();
                    });
                    app._subscriptions.push(appearance);
                    app
                });
                window.focus(&view.read(cx).focus.clone(), cx);
                view
            },
        )
        .unwrap();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
