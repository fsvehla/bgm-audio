//! Colours: light or dark following the system appearance, accent colour from
//! System Settings → Appearance. The palette is a GPUI global; `apply` also
//! feeds it into gpui-component's theme so its widgets match our own elements.

use crate::platform;
use gpui::{App, Global, Hsla, WindowAppearance, hsla, rgb};
use gpui_component::{Theme, ThemeMode};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Hsla,
    pub panel: Hsla,
    pub panel_hover: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub accent_hover: Hsla,
    /// Text drawn on an accent background.
    pub on_accent: Hsla,
    pub warn: Hsla,
    /// Chapter marks cut into the progress bar.
    pub marker: Hsla,
}

impl Global for Palette {}

impl Palette {
    fn new(dark: bool, accent: Hsla) -> Self {
        let c = |hex: u32| -> Hsla { rgb(hex).into() };
        let base = if dark {
            Palette {
                bg: c(0x15181b),
                panel: c(0x1c1f25),
                panel_hover: c(0x272b33),
                text: c(0xe8e6e1),
                muted: c(0x8b9099),
                warn: c(0xe5786d),
                marker: hsla(0., 0., 0.08, 0.6),
                ..Self::placeholder()
            }
        } else {
            Palette {
                bg: c(0xf5f5f7),
                panel: c(0xe8e8ec),
                panel_hover: c(0xdcdce2),
                text: c(0x1d1d1f),
                muted: c(0x6e6e73),
                warn: c(0xc9443a),
                marker: hsla(0., 0., 1., 0.7),
                ..Self::placeholder()
            }
        };
        let hover_shift = if dark { 0.07 } else { -0.06 };
        Palette {
            accent,
            accent_hover: Hsla {
                l: (accent.l + hover_shift).clamp(0., 1.),
                ..accent
            },
            on_accent: if accent.l > 0.72 {
                c(0x1d1d1f)
            } else {
                c(0xffffff)
            },
            ..base
        }
    }

    fn placeholder() -> Self {
        let none = Hsla::default();
        Palette {
            bg: none,
            panel: none,
            panel_hover: none,
            text: none,
            muted: none,
            accent: none,
            accent_hover: none,
            on_accent: none,
            warn: none,
            marker: none,
        }
    }
}

pub fn palette(cx: &App) -> Palette {
    *cx.global::<Palette>()
}

/// Rebuilds the palette for `appearance` and the current system accent colour.
pub fn apply(appearance: WindowAppearance, cx: &mut App) {
    let dark = matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    );
    let p = Palette::new(
        dark,
        platform::accent_color().unwrap_or(rgb(0x007aff).into()),
    );
    cx.set_global(p);

    // `change` loads gpui-component's own light/dark theme; `update` then writes our
    // colours over it and keeps its token copy, the Base layer and windows in step.
    let mode = if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    if Theme::global(cx).mode != mode {
        Theme::change(mode, None, cx);
    }
    Theme::update(cx, |theme| {
        theme.shadow = false;
        let t = &mut theme.colors;
        t.background = p.bg;
        t.foreground = p.text;
        t.border = p.panel_hover;
        t.ring = p.accent;
        t.primary = p.accent;
        t.primary_hover = p.accent_hover;
        t.primary_active = p.accent;
        t.primary_foreground = p.on_accent;
        t.secondary = p.panel;
        t.secondary_hover = p.panel_hover;
        t.secondary_active = p.panel_hover;
        t.secondary_foreground = p.text;
        t.button = p.panel;
        t.button_hover = p.panel_hover;
        t.button_active = p.panel_hover;
        t.button_foreground = p.text;
        t.button_primary = p.accent;
        t.button_primary_hover = p.accent_hover;
        t.button_primary_active = p.accent;
        t.button_primary_foreground = p.on_accent;
        t.muted = p.panel;
        t.muted_foreground = p.muted;
        t.accent = p.panel_hover;
        t.accent_foreground = p.text;
        t.danger = p.warn;
        t.switch = p.panel_hover;
        t.switch_thumb = rgb(0xffffff).into();
        t.slider_bar = p.accent;
        t.slider_thumb = rgb(0xffffff).into();
        t.tab_bar = p.panel;
        t.tab_bar_segmented = p.panel;
        t.tab = p.panel;
        t.tab_active = p.bg;
        t.tab_foreground = p.muted;
        t.tab_active_foreground = p.text;
        t.scrollbar = Hsla::transparent_black();
        t.scrollbar_thumb = p.muted.opacity(0.5);
        t.scrollbar_thumb_hover = p.muted;
        t.list_hover = p.panel_hover;
        t.selection = p.accent.opacity(0.3);
    });
}
