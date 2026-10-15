//! The window's contents.

use gpui::{
    AnyElement, Context, ExternalPaths, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window, div, img, prelude::*, px,
    relative,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::scroll::ScrollableElement;
use gpui_component::slider::Slider;
use gpui_component::switch::Switch;
use gpui_component::tab::{Tab as TabItem, TabBar};
use gpui_component::{Icon, IconName, Sizable, StyledExt};

use crate::state::{Appearance, Tab};
use crate::theme::{Palette, palette};
use crate::{
    BgmApp, DarkMode, FollowSystem, LightMode, Minimize, NextChapter, Open, PrevChapter,
    SKIP_SECONDS, SkipBack, SkipForward, ToggleMiniPlayer, TogglePlay, VolumeDown, VolumeUp, Zoom,
};

/// Room for the mini player toggle (a ghost icon button) beside the transport buttons.
const TOGGLE_WIDTH: f32 = 32.;
/// Don't pull the progress bar back to the player position this soon after a scrub.
const SCRUB_SETTLE: std::time::Duration = std::time::Duration::from_millis(600);

impl Render for BgmApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette(cx);
        div()
            .id("root")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Open, _, cx| this.prompt_open(cx)))
            .on_action(cx.listener(|this, _: &TogglePlay, _, cx| this.toggle(cx)))
            .on_action(cx.listener(|this, _: &SkipBack, _, cx| this.skip(-SKIP_SECONDS, cx)))
            .on_action(cx.listener(|this, _: &SkipForward, _, cx| this.skip(SKIP_SECONDS, cx)))
            .on_action(cx.listener(|this, _: &PrevChapter, _, cx| this.prev_chapter(cx)))
            .on_action(cx.listener(|this, _: &NextChapter, _, cx| this.next_chapter(cx)))
            .on_action(cx.listener(|this, _: &VolumeUp, window, cx| {
                this.set_volume(this.state.volume + 0.05, window, cx)
            }))
            .on_action(cx.listener(|this, _: &VolumeDown, window, cx| {
                this.set_volume(this.state.volume - 0.05, window, cx)
            }))
            .on_action(cx.listener(|this, _: &FollowSystem, window, cx| {
                this.set_appearance(Appearance::System, window, cx)
            }))
            .on_action(cx.listener(|this, _: &LightMode, window, cx| {
                this.set_appearance(Appearance::Light, window, cx)
            }))
            .on_action(cx.listener(|this, _: &DarkMode, window, cx| {
                this.set_appearance(Appearance::Dark, window, cx)
            }))
            .on_action(cx.listener(|_, _: &Minimize, window, _| window.minimize_window()))
            .on_action(cx.listener(|_, _: &Zoom, window, _| window.zoom_window()))
            .on_action(
                cx.listener(|this, _: &ToggleMiniPlayer, window, cx| {
                    this.toggle_compact(window, cx)
                }),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first() {
                    this.open(&path.clone(), true, cx);
                }
            }))
            .size_full()
            .bg(p.bg)
            .text_color(p.text)
            .text_sm()
            .flex()
            .flex_col()
            // Room for the traffic lights under the transparent macOS title bar.
            .pt(px(if cfg!(target_os = "macos") { 40. } else { 16. }))
            .px_4()
            .pb_4()
            .gap_4()
            .child(self.render_now_playing(window, cx))
            .when_some(self.error.clone(), |el, error| {
                el.child(div().text_xs().text_color(p.warn).child(error))
            })
            .when(!self.state.compact, |el| el.child(self.render_lists(cx)))
    }
}

impl BgmApp {
    fn render_now_playing(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let Some(player) = &self.player else {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .h(px(220.))
                .rounded_lg()
                .border_1()
                .border_dashed()
                .border_color(p.panel_hover)
                .text_color(p.muted)
                .child("Drop an audio file here")
                .child(
                    Button::new("open")
                        .label("Open…")
                        .on_click(cx.listener(|this, _, _, cx| this.prompt_open(cx))),
                )
                .into_any_element();
        };

        let track = &player.track;
        let position = player.position();
        let playing = player.is_playing();
        let chapter_title = self
            .chapter
            .and_then(|i| track.chapters.get(i))
            .map(|c| c.short_title())
            .filter(|t| !t.is_empty());
        let subtitle = chapter_title.clone().or_else(|| track.artist.clone());

        let artwork = match &self.artwork_ui {
            Some(image) => img(image.clone())
                .size(px(84.))
                .rounded_md()
                .into_any_element(),
            None => div()
                .size(px(84.))
                .rounded_md()
                .bg(p.panel)
                .into_any_element(),
        };

        // One line; a long name loses its middle ("Lord of the Rings | Mid…, 3 Hours").
        let subtitle_line = || {
            div()
                .text_xs()
                .text_color(p.muted)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
        };
        let header = div().flex().gap_3().items_center().child(artwork).child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_base()
                        .line_clamp(2)
                        .child(SharedString::from(
                            chapter_title.unwrap_or_else(|| track.title.clone()),
                        )),
                )
                .when_some(subtitle.filter(|_| self.chapter.is_none()), |el, s| {
                    el.child(subtitle_line().child(s))
                })
                .when(self.chapter.is_some(), |el| {
                    el.child(subtitle_line().child(SharedString::from(track.title.clone())))
                }),
        );

        // Progress bar: a slider (click to seek, drag to scrub) with chapter marks on top.
        let fraction = (position / track.duration).clamp(0.0, 1.0) as f32;
        let settled = self.scrubbed_at.is_none_or(|t| t.elapsed() > SCRUB_SETTLE);
        if settled && (self.progress_slider.read(cx).value().end() - fraction).abs() > 0.0001 {
            self.progress_slider
                .update(cx, |slider, cx| slider.set_value(fraction, window, cx));
        }
        let markers: Vec<f32> = track
            .chapters
            .iter()
            .skip(1)
            .map(|c| (c.start / track.duration) as f32)
            .collect();
        let bar = div()
            .relative()
            .w_full()
            .child(Slider::new(&self.progress_slider).bg(p.accent))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .children(markers.into_iter().map(move |m| {
                        div()
                            .absolute()
                            .top(px(9.))
                            .left(relative(m))
                            .h(px(6.))
                            .w(px(2.))
                            .bg(p.marker)
                    })),
            );

        let times = div()
            .flex()
            .justify_between()
            .text_xs()
            .text_color(p.muted)
            .child(format_time(position))
            .child(format!("-{}", format_time(track.duration - position)));

        let has_chapters = !track.chapters.is_empty();
        let compact = self.state.compact;
        // Three columns: equal side columns keep the transport buttons centred, and the
        // toggle gets its own space instead of floating over them in a narrow window.
        let toggle = Button::new("compact")
            .ghost()
            .icon(if compact {
                IconName::ChevronDown
            } else {
                IconName::ChevronUp
            })
            .text_color(p.muted)
            .tooltip(if compact {
                "Show everything"
            } else {
                "Mini player"
            })
            .on_click(cx.listener(|this, _, window, cx| this.toggle_compact(window, cx)));
        let side = || div().flex_1().min_w(px(TOGGLE_WIDTH)).flex();
        let transport = div()
            .flex()
            .flex_none()
            .gap_2()
            .when(has_chapters, |el| {
                el.child(
                    Button::new("prev")
                        .small()
                        .h_8()
                        .icon(Icon::new(gpui_kit_assets::IconName::SkipBack))
                        .tooltip("Previous chapter")
                        .on_click(cx.listener(|this, _, _, cx| this.prev_chapter(cx))),
                )
            })
            .child(
                Button::new("back")
                    .small()
                    .h_8()
                    .label("−10s")
                    .on_click(cx.listener(|this, _, _, cx| this.skip(-SKIP_SECONDS, cx))),
            )
            .child(
                Button::new("play")
                    .small()
                    .h_8()
                    .primary()
                    .label(if playing { "Pause" } else { "Play" })
                    // Setting any text style on a Button replaces its colour too; set both.
                    .text_color(p.on_accent)
                    .font_semibold()
                    .w(px(88.))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            )
            .child(
                Button::new("fwd")
                    .small()
                    .h_8()
                    .label("+10s")
                    .on_click(cx.listener(|this, _, _, cx| this.skip(SKIP_SECONDS, cx))),
            )
            .when(has_chapters, |el| {
                el.child(
                    Button::new("next")
                        .small()
                        .h_8()
                        .icon(Icon::new(gpui_kit_assets::IconName::SkipForward))
                        .tooltip("Next chapter")
                        .on_click(cx.listener(|this, _, _, cx| this.next_chapter(cx))),
                )
            });
        let controls = div()
            .flex()
            .items_center()
            .gap_2()
            .child(side().when(!cfg!(target_os = "macos"), |el| {
                el.child(
                    Button::new("menu")
                        .ghost()
                        .icon(IconName::Ellipsis)
                        .text_color(p.muted)
                        .tooltip("Menu")
                        .on_click(cx.listener(|this, event: &gpui::ClickEvent, window, cx| {
                            this.show_window_menu(event.position(), window, cx)
                        })),
                )
            }))
            .child(transport)
            .child(side().justify_end().child(toggle));

        // gpui-component slider: click to jump, drag to slide. Bar colour comes from
        // bg, thumb colour from text_color.
        let volume = div()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(p.muted)
            .child("Vol")
            .child(
                div()
                    .w(px(90.))
                    .child(Slider::new(&self.volume_slider).bg(p.muted)),
            );

        let mic_label: SharedString = match (&self.mic_app, self.auto_paused) {
            (Some(app), true) => format!("Paused: mic in use by {app}").into(),
            (Some(app), false) => format!("Mic in use by {app}").into(),
            _ => "Pause when mic is in use".into(),
        };
        let options = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .flex()
                    .gap_4()
                    .text_xs()
                    .child(
                        Switch::new("loop")
                            .small()
                            .label("Loop")
                            .checked(self.state.looping)
                            .on_click(cx.listener(|this, _: &bool, _, cx| this.toggle_loop(cx))),
                    )
                    .child(
                        Switch::new("mic")
                            .small()
                            .label(mic_label)
                            .checked(self.state.mic_auto_pause)
                            .on_click(
                                cx.listener(|this, _: &bool, _, cx| this.toggle_mic_auto_pause(cx)),
                            ),
                    ),
            )
            .child(volume);

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(header)
            .child(div().flex().flex_col().gap_1().child(bar).child(times))
            .child(controls)
            .when(!compact, |el| el.child(options))
            .into_any_element()
    }

    fn render_lists(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = palette(cx);
        let chapter_count = self.player.as_ref().map_or(0, |p| p.track.chapters.len());
        // Without chapters there is nothing to show on the Chapters tab.
        let tab = if chapter_count == 0 {
            Tab::Recent
        } else {
            self.state.tab
        };

        // Tab indices depend on whether the Chapters tab is shown.
        let tabs_shown: Vec<Tab> = if chapter_count > 0 {
            vec![Tab::Chapters, Tab::Recent]
        } else {
            vec![Tab::Recent]
        };
        let count = |n: usize| div().ml_1p5().text_color(p.muted).child(n.to_string());
        let tabs = TabBar::new("tabs")
            .segmented()
            .small()
            .selected_index(tabs_shown.iter().position(|t| *t == tab).unwrap_or(0))
            .when(chapter_count > 0, |bar| {
                bar.child(TabItem::new().label("Chapters").child(count(chapter_count)))
            })
            .child(
                TabItem::new()
                    .label("Recent")
                    .child(count(self.state.files.len())),
            )
            .on_click(cx.listener(move |this, ix: &usize, _, cx| {
                if let Some(tab) = tabs_shown.get(*ix) {
                    this.set_tab(*tab, cx)
                }
            }));

        let list = match tab {
            Tab::Chapters => self.render_chapters(cx),
            Tab::Recent => self.render_recent(cx),
        };

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().flex().child(tabs))
            .child(list.overflow_y_scrollbar())
            .into_any_element()
    }

    fn render_chapters(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let p = palette(cx);
        let mut list = scroll_list("chapters");
        let Some(player) = &self.player else {
            return list;
        };
        for (i, chapter) in player.track.chapters.iter().enumerate() {
            let current = self.chapter == Some(i);
            let start = chapter.start;
            list = list.child(
                row(("chapter", i), current, &p)
                    .child(
                        div()
                            .w(px(56.))
                            .flex_none()
                            .text_xs()
                            .text_color(p.muted)
                            .child(format_time(start)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(SharedString::from(chapter.short_title())),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.seek(start, cx))),
            );
        }
        list
    }

    fn render_recent(&mut self, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        let p = palette(cx);
        let mut list = scroll_list("recent");
        if self.state.files.is_empty() {
            return list.child(
                div()
                    .px_2()
                    .py_3()
                    .text_xs()
                    .text_color(p.muted)
                    .child("Files you play show up here, with where you left off."),
            );
        }

        let current_path = self.player.as_ref().map(|p| p.track.path.clone());
        let live = self.player.as_ref().map(|p| (p.position(), p.is_playing()));
        let now = now();
        for (i, entry) in self.state.files.clone().into_iter().enumerate() {
            let current = Some(&entry.path) == current_path.as_ref();
            let available = current || entry.path.is_file() || entry.bookmark.is_some();
            let position = match live {
                Some((position, _)) if current => position,
                _ => entry.position,
            };
            let fraction = if entry.duration > 0.0 {
                (position / entry.duration).clamp(0.0, 1.0) as f32
            } else {
                0.0
            };
            let progress = format!(
                "{} / {}",
                format_time(position),
                format_time(entry.duration)
            );
            let detail: SharedString = if current {
                let state = if live.is_some_and(|(_, playing)| playing) {
                    "Playing"
                } else {
                    "Paused"
                };
                format!("{progress} · {state}").into()
            } else if !available {
                "Missing".into()
            } else {
                format!(
                    "{progress} · {}",
                    ago(now.saturating_sub(entry.last_played))
                )
                .into()
            };

            list = list.child(
                row(("recent", i), current, &p)
                    .when(!available, |el| el.opacity(0.5))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .min_w_0()
                            .child(div().truncate().child(SharedString::from(entry.title)))
                            .child(
                                div()
                                    .h(px(3.))
                                    .w_full()
                                    .rounded_full()
                                    .bg(p.panel_hover)
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(fraction))
                                            .rounded_full()
                                            .bg(if current { p.accent } else { p.muted }),
                                    ),
                            )
                            .child(div().text_xs().text_color(p.muted).child(detail)),
                    )
                    .when(!current, |el| {
                        el.child(
                            Button::new(("forget", i))
                                .ghost()
                                .xsmall()
                                .label("×")
                                .text_color(p.muted)
                                .tooltip("Remove from list")
                                // The row opens the entry on click; don't let this click
                                // reach it, or it opens the entry that moves into place.
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.forget_recent(i, cx)
                                })),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.open_recent(i, cx)))
                    }),
            );
        }
        list
    }
}

fn scroll_list(id: &'static str) -> gpui::Stateful<gpui::Div> {
    div().id(id).flex_1().min_h_0().flex().flex_col().gap_1()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago".
fn ago(seconds: u64) -> String {
    match seconds {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86400 => format!("{} h ago", seconds / 3600),
        86400..172800 => "yesterday".into(),
        _ => format!("{} days ago", seconds / 86400),
    }
}

fn row(id: impl Into<gpui::ElementId>, current: bool, p: &Palette) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1p5()
        .rounded_md()
        .cursor_pointer()
        .when(current, |el| el.bg(p.panel).text_color(p.accent))
        .hover(|s| s.bg(p.panel_hover))
}

fn format_time(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
