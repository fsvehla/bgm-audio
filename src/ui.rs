//! The window's contents.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, ExternalPaths, FontWeight, Hsla, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, ParentElement, Pixels, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window, canvas, div, img, prelude::*, px, relative, rgb,
    rgba,
};

use gpui_component::slider::Slider;

use crate::state::Tab;
use crate::{
    BgmApp, NextChapter, Open, PrevChapter, SKIP_SECONDS, SkipBack, SkipForward, TogglePlay,
    VolumeDown, VolumeUp,
};

const BG: u32 = 0x15181b;
const PANEL: u32 = 0x1c1f25;
const PANEL_HOVER: u32 = 0x272b33;
const TEXT: u32 = 0xe8e6e1;
const MUTED: u32 = 0x8b9099;
const ACCENT: u32 = 0x8fc0a0;
const WARN: u32 = 0xe5786d;

impl Render for BgmApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first() {
                    this.open(&path.clone(), true, cx);
                }
            }))
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_sm()
            .flex()
            .flex_col()
            .pt(px(40.))
            .px_4()
            .pb_4()
            .gap_4()
            .child(self.render_now_playing(cx))
            .when_some(self.error.clone(), |el, error| {
                el.child(div().text_xs().text_color(rgb(WARN)).child(error))
            })
            .when(!self.state.compact, |el| el.child(self.render_lists(cx)))
    }
}

impl BgmApp {
    fn render_now_playing(&mut self, cx: &mut Context<Self>) -> AnyElement {
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
                .border_color(rgb(PANEL_HOVER))
                .text_color(rgb(MUTED))
                .child("Drop an audio file here")
                .child(
                    button("open", "Open…")
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
                .bg(rgb(PANEL))
                .into_any_element(),
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
                    el.child(div().text_xs().text_color(rgb(MUTED)).child(s))
                })
                .when(self.chapter.is_some(), |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(rgb(MUTED))
                            .line_clamp(1)
                            .child(SharedString::from(track.title.clone())),
                    )
                }),
        );

        // Progress bar: click anywhere to seek. A canvas records the bar's bounds.
        let fraction = (position / track.duration).clamp(0.0, 1.0) as f32;
        let bar_bounds = self.bar_bounds.clone();
        let markers: Vec<f32> = track
            .chapters
            .iter()
            .skip(1)
            .map(|c| (c.start / track.duration) as f32)
            .collect();
        let bar = div()
            .id("bar")
            .relative()
            .h(px(14.))
            .w_full()
            .flex()
            .items_center()
            .cursor_pointer()
            .child(
                div()
                    .relative()
                    .h(px(6.))
                    .w_full()
                    .rounded_full()
                    .bg(rgb(PANEL_HOVER))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .w(relative(fraction))
                            .rounded_full()
                            .bg(rgb(ACCENT)),
                    )
                    .children(markers.into_iter().map(|m| {
                        div()
                            .absolute()
                            .top_0()
                            .left(relative(m))
                            .h_full()
                            .w(px(2.))
                            .bg(rgba(0x13151999))
                    })),
            )
            .child(record_bounds(bar_bounds))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let Some(duration) = this.player.as_ref().map(|p| p.track.duration) else {
                        return;
                    };
                    let f = fraction_in(this.bar_bounds.get(), event.position.x);
                    this.seek(f as f64 * duration, cx);
                }),
            );

        let times = div()
            .flex()
            .justify_between()
            .text_xs()
            .text_color(rgb(MUTED))
            .child(format_time(position))
            .child(format!("-{}", format_time(track.duration - position)));

        let has_chapters = !track.chapters.is_empty();
        let compact = self.state.compact;
        let controls = div()
            .relative()
            .flex()
            .gap_2()
            .justify_center()
            .child(
                div()
                    .id("compact")
                    .absolute()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .px_2()
                    .rounded_md()
                    .text_color(rgb(MUTED))
                    .hover(|s| s.text_color(rgb(TEXT)).bg(rgb(PANEL)))
                    .cursor_pointer()
                    .text_lg()
                    .child(if compact { "⌄" } else { "⌃" })
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_compact(window, cx))),
            )
            .when(has_chapters, |el| {
                el.child(
                    button("prev", "⏮")
                        .on_click(cx.listener(|this, _, _, cx| this.prev_chapter(cx))),
                )
            })
            .child(
                button("back", "−10s")
                    .on_click(cx.listener(|this, _, _, cx| this.skip(-SKIP_SECONDS, cx))),
            )
            .child(
                primary_button("play", if playing { "Pause" } else { "Play" })
                    .w(px(88.))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(cx))),
            )
            .child(
                button("fwd", "+10s")
                    .on_click(cx.listener(|this, _, _, cx| this.skip(SKIP_SECONDS, cx))),
            )
            .when(has_chapters, |el| {
                el.child(
                    button("next", "⏭")
                        .on_click(cx.listener(|this, _, _, cx| this.next_chapter(cx))),
                )
            });

        // gpui-component slider: click to jump, drag to slide. Bar colour comes from
        // bg, thumb colour from text_color.
        let volume = div()
            .flex()
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(rgb(MUTED))
            .child("Vol")
            .child(
                div().w(px(90.)).child(
                    Slider::new(&self.volume_slider)
                        .bg(rgb(MUTED))
                        .text_color(rgb(TEXT)),
                ),
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
                    .gap_2()
                    .child(
                        toggle("loop", "Loop", self.state.looping)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_loop(cx))),
                    )
                    .child(
                        toggle("mic", mic_label, self.state.mic_auto_pause)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_mic_auto_pause(cx))),
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
        let chapter_count = self.player.as_ref().map_or(0, |p| p.track.chapters.len());
        // Without chapters there is nothing to show on the Chapters tab.
        let tab = if chapter_count == 0 {
            Tab::Recent
        } else {
            self.state.tab
        };

        let tabs = div()
            .flex()
            .gap_1()
            .p(px(3.))
            .rounded_lg()
            .bg(rgb(PANEL))
            .text_xs()
            .when(chapter_count > 0, |el| {
                el.child(
                    tab_button(
                        "tab-chapters",
                        "Chapters",
                        chapter_count,
                        tab == Tab::Chapters,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Chapters, cx))),
                )
            })
            .child(
                tab_button(
                    "tab-recent",
                    "Recent",
                    self.state.files.len(),
                    tab == Tab::Recent,
                )
                .on_click(cx.listener(|this, _, _, cx| this.set_tab(Tab::Recent, cx))),
            );

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
            .child(list)
            .into_any_element()
    }

    fn render_chapters(&mut self, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let mut list = scroll_list("chapters");
        let Some(player) = &self.player else {
            return list;
        };
        for (i, chapter) in player.track.chapters.iter().enumerate() {
            let current = self.chapter == Some(i);
            let start = chapter.start;
            list = list.child(
                row(("chapter", i), current)
                    .child(
                        div()
                            .w(px(56.))
                            .flex_none()
                            .text_xs()
                            .text_color(rgb(MUTED))
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

    fn render_recent(&mut self, cx: &mut Context<Self>) -> Stateful<gpui::Div> {
        let mut list = scroll_list("recent");
        if self.state.files.is_empty() {
            return list.child(
                div()
                    .px_2()
                    .py_3()
                    .text_xs()
                    .text_color(rgb(MUTED))
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
                row(("recent", i), current)
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
                                    .bg(rgb(PANEL_HOVER))
                                    .child(
                                        div()
                                            .h_full()
                                            .w(relative(fraction))
                                            .rounded_full()
                                            .bg(rgb(if current { ACCENT } else { MUTED })),
                                    ),
                            )
                            .child(div().text_xs().text_color(rgb(MUTED)).child(detail)),
                    )
                    .when(!current, |el| {
                        el.child(
                            div()
                                .id(("forget", i))
                                .px_2()
                                .rounded_sm()
                                .text_color(rgb(MUTED))
                                .hover(|s| s.text_color(rgb(TEXT)).bg(rgb(PANEL)))
                                .child("×")
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

fn button(id: &'static str, label: impl Into<SharedString>) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .px_3()
        .py_1p5()
        .rounded_md()
        .bg(rgb(PANEL))
        .hover(|s| s.bg(rgb(PANEL_HOVER)))
        .cursor_pointer()
        .flex()
        .justify_center()
        .child(label.into())
}

fn primary_button(id: &'static str, label: impl Into<SharedString>) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .px_3()
        .py_1p5()
        .rounded_md()
        .bg(rgb(ACCENT))
        .text_color(rgb(BG))
        .font_weight(FontWeight::SEMIBOLD)
        .hover(|s| s.bg(rgb(0xa8d1b6)))
        .cursor_pointer()
        .flex()
        .justify_center()
        .child(label.into())
}

fn toggle(id: &'static str, label: impl Into<SharedString>, on: bool) -> Stateful<gpui::Div> {
    let dot: Hsla = if on {
        rgb(ACCENT).into()
    } else {
        rgb(PANEL_HOVER).into()
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1p5()
        .px_2()
        .py_1()
        .rounded_md()
        .text_xs()
        .text_color(rgb(if on { TEXT } else { MUTED }))
        .hover(|s| s.bg(rgb(PANEL)))
        .cursor_pointer()
        .child(div().size(px(8.)).rounded_full().bg(dot))
        .child(label.into())
}

fn tab_button(
    id: &'static str,
    label: &'static str,
    count: usize,
    selected: bool,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .gap_1p5()
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .text_color(rgb(if selected { TEXT } else { MUTED }))
        .when(selected, |el| el.bg(rgb(PANEL_HOVER)))
        .when(!selected, |el| el.hover(|s| s.text_color(rgb(TEXT))))
        .child(label)
        .child(div().text_color(rgb(MUTED)).child(count.to_string()))
}

fn scroll_list(id: &'static str) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap_1()
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

fn row(id: impl Into<gpui::ElementId>, current: bool) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1p5()
        .rounded_md()
        .cursor_pointer()
        .when(current, |el| el.bg(rgb(PANEL)).text_color(rgb(ACCENT)))
        .hover(|s| s.bg(rgb(PANEL_HOVER)))
}

/// An invisible element that stores its parent's bounds each frame.
fn record_bounds(target: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| target.set(bounds), |_, _, _, _| {})
        .absolute()
        .size_full()
}

fn fraction_in(bounds: Bounds<Pixels>, x: Pixels) -> f32 {
    if bounds.size.width <= px(0.) {
        return 0.0;
    }
    ((x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0)
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
