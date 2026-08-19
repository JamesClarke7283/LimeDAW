//! LimeDAW's light and dark themes.
//!
//! Both are built from one palette definition so the two stay in step, and the
//! app follows the operating system's preference unless told otherwise.

use iced::widget::{
    button, container, pick_list, progress_bar, rule, scrollable, slider, text_input,
};
use iced::{Background, Border, Color, Shadow, Theme, Vector, border, color};
use serde::{Deserialize, Serialize};

/// Which theme the user wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    /// Follow the operating system's light/dark setting.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];

    pub fn label(self) -> &'static str {
        match self {
            ThemeChoice::System => "Follow system",
            ThemeChoice::Light => "Light",
            ThemeChoice::Dark => "Dark",
        }
    }
}

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// The colours the app draws with, beyond what `iced::Theme` provides.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub is_dark: bool,
    /// The window background, behind every panel.
    pub base: Color,
    /// Panel surfaces.
    pub surface: Color,
    /// Raised surfaces: headers, toolbars, selected rows.
    pub surface_high: Color,
    /// Recessed surfaces: grids, track lanes.
    pub surface_low: Color,
    pub border: Color,
    pub border_strong: Color,
    pub text: Color,
    pub text_dim: Color,
    pub accent: Color,
    pub accent_dim: Color,
    pub accent_text: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    /// Piano-roll white and black key lanes.
    pub lane_white: Color,
    pub lane_black: Color,
    pub grid_line: Color,
    pub grid_beat: Color,
    pub grid_bar: Color,
    pub playhead: Color,
    pub meter_low: Color,
    pub meter_high: Color,
    pub meter_clip: Color,
}

pub const LIGHT: Palette = Palette {
    is_dark: false,
    base: color!(0xEFF1EC),
    surface: color!(0xF8FAF5),
    surface_high: color!(0xFFFFFF),
    surface_low: color!(0xE4E8DE),
    border: color!(0xD2D8C8),
    border_strong: color!(0xB4BDA6),
    text: color!(0x1B2016),
    text_dim: color!(0x636C58),
    accent: color!(0x5C9A0F),
    accent_dim: color!(0xBFDE8F),
    accent_text: color!(0xFFFFFF),
    danger: color!(0xC0392B),
    warning: color!(0xC98A17),
    success: color!(0x2E8B57),
    lane_white: color!(0xF6F8F2),
    lane_black: color!(0xE6EADE),
    grid_line: color!(0xD9DED0),
    grid_beat: color!(0xC3CBB4),
    grid_bar: color!(0x9EAA8A),
    playhead: color!(0xE05A2B),
    meter_low: color!(0x6FBF1B),
    meter_high: color!(0xE0B21B),
    meter_clip: color!(0xD84315),
};

pub const DARK: Palette = Palette {
    is_dark: true,
    base: color!(0x14171A),
    surface: color!(0x1C2024),
    surface_high: color!(0x262B30),
    surface_low: color!(0x101316),
    border: color!(0x30363C),
    border_strong: color!(0x454D55),
    text: color!(0xE6EBE0),
    text_dim: color!(0x8D978A),
    accent: color!(0x9BD634),
    accent_dim: color!(0x3F5418),
    accent_text: color!(0x11170A),
    danger: color!(0xE05252),
    warning: color!(0xE8B93A),
    success: color!(0x54C282),
    lane_white: color!(0x22262A),
    lane_black: color!(0x191D21),
    grid_line: color!(0x2B3036),
    grid_beat: color!(0x3A424A),
    grid_bar: color!(0x525C66),
    playhead: color!(0xFF7043),
    meter_low: color!(0x8BD13A),
    meter_high: color!(0xE8C33A),
    meter_clip: color!(0xFF5252),
};

/// The palette matching an `iced::Theme` produced by this module.
pub fn palette(theme: &Theme) -> Palette {
    if theme.extended_palette().is_dark {
        DARK
    } else {
        LIGHT
    }
}

fn to_iced_palette(p: &Palette) -> iced::theme::Palette {
    iced::theme::Palette {
        background: p.base,
        text: p.text,
        primary: p.accent,
        success: p.success,
        warning: p.warning,
        danger: p.danger,
    }
}

pub fn light() -> Theme {
    Theme::custom("LimeDAW Light", to_iced_palette(&LIGHT))
}

pub fn dark() -> Theme {
    Theme::custom("LimeDAW Dark", to_iced_palette(&DARK))
}

/// Ask the platform whether it is in dark mode.
///
/// On the desktop iced reports this through its own subscription; in a browser
/// the answer comes from the `prefers-color-scheme` media query.
pub fn system_prefers_dark() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
            .map(|q| q.matches())
            .unwrap_or(false)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // The desktop build learns this from `iced::system::theme` instead;
        // dark is the safer starting point until that resolves.
        true
    }
}

/// Resolve a choice plus the detected system mode into a concrete theme.
pub fn resolve(choice: ThemeChoice, system_dark: bool) -> Theme {
    match choice {
        ThemeChoice::Light => light(),
        ThemeChoice::Dark => dark(),
        ThemeChoice::System => {
            if system_dark {
                dark()
            } else {
                light()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Widget styles
// ---------------------------------------------------------------------------

/// The window background.
pub fn root(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.base)),
        text_color: Some(p.text),
        ..container::Style::default()
    }
}

/// A framed panel.
pub fn panel(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.surface)),
        text_color: Some(p.text),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: border::radius(6),
        },
        ..container::Style::default()
    }
}

/// A panel header strip.
pub fn header(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.surface_high)),
        text_color: Some(p.text),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: border::radius(6),
        },
        ..container::Style::default()
    }
}

/// A recessed area: grids, lists, editors.
pub fn well(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.surface_low)),
        text_color: Some(p.text),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: border::radius(4),
        },
        ..container::Style::default()
    }
}

/// The toolbar across the top of the window.
pub fn toolbar(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.surface_high)),
        text_color: Some(p.text),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: border::radius(8),
        },
        shadow: Shadow {
            color: Color {
                a: if p.is_dark { 0.35 } else { 0.12 },
                ..Color::BLACK
            },
            offset: Vector::new(0.0, 1.0),
            blur_radius: 6.0,
        },
        ..container::Style::default()
    }
}

/// A card that stands out from the panel it sits on.
pub fn card(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(p.surface_high)),
        text_color: Some(p.text),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: border::radius(6),
        },
        ..container::Style::default()
    }
}

/// A selected row or card.
pub fn selected(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(mix(p.surface_high, p.accent, 0.18))),
        text_color: Some(p.text),
        border: Border {
            color: p.accent,
            width: 1.0,
            radius: border::radius(6),
        },
        ..container::Style::default()
    }
}

/// A tinted status strip, used for hints and warnings.
pub fn notice(theme: &Theme) -> container::Style {
    let p = palette(theme);
    container::Style {
        background: Some(Background::Color(mix(p.surface, p.warning, 0.16))),
        text_color: Some(p.text),
        border: Border {
            color: mix(p.border, p.warning, 0.4),
            width: 1.0,
            radius: border::radius(6),
        },
        ..container::Style::default()
    }
}

pub fn transparent(_theme: &Theme) -> container::Style {
    container::Style::default()
}

/// The default toolbar button.
pub fn button_plain(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let bg = match status {
        button::Status::Hovered => mix(p.surface_high, p.accent, 0.14),
        button::Status::Pressed => mix(p.surface_high, p.accent, 0.26),
        button::Status::Disabled => p.surface,
        button::Status::Active => p.surface_high,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: if matches!(status, button::Status::Disabled) {
            p.text_dim
        } else {
            p.text
        },
        border: Border {
            color: p.border,
            width: 1.0,
            radius: border::radius(5),
        },
        ..button::Style::default()
    }
}

/// A button that reads as the primary action.
pub fn button_accent(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let bg = match status {
        button::Status::Hovered => lighten(p.accent, 0.1),
        button::Status::Pressed => darken(p.accent, 0.12),
        button::Status::Disabled => mix(p.surface, p.accent, 0.3),
        button::Status::Active => p.accent,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.accent_text,
        border: Border {
            color: darken(p.accent, 0.15),
            width: 1.0,
            radius: border::radius(5),
        },
        ..button::Style::default()
    }
}

/// A destructive action.
pub fn button_danger(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let bg = match status {
        button::Status::Hovered => lighten(p.danger, 0.1),
        button::Status::Pressed => darken(p.danger, 0.12),
        button::Status::Disabled => mix(p.surface, p.danger, 0.3),
        button::Status::Active => p.danger,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: Color::WHITE,
        border: Border {
            color: darken(p.danger, 0.15),
            width: 1.0,
            radius: border::radius(5),
        },
        ..button::Style::default()
    }
}

/// A borderless button, for icons and list rows.
pub fn button_ghost(theme: &Theme, status: button::Status) -> button::Style {
    let p = palette(theme);
    let bg = match status {
        button::Status::Hovered => Color {
            a: 0.10,
            ..p.accent
        },
        button::Status::Pressed => Color {
            a: 0.20,
            ..p.accent
        },
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: if matches!(status, button::Status::Disabled) {
            p.text_dim
        } else {
            p.text
        },
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: border::radius(5),
        },
        ..button::Style::default()
    }
}

/// A tab in the main view switcher.
pub fn button_tab(theme: &Theme, status: button::Status, active: bool) -> button::Style {
    let p = palette(theme);
    if active {
        return button::Style {
            background: Some(Background::Color(p.accent)),
            text_color: p.accent_text,
            border: Border {
                color: p.accent,
                width: 1.0,
                radius: border::radius(5),
            },
            ..button::Style::default()
        };
    }
    let bg = match status {
        button::Status::Hovered => mix(p.surface, p.accent, 0.12),
        button::Status::Pressed => mix(p.surface, p.accent, 0.22),
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.text_dim,
        border: Border {
            color: Color::TRANSPARENT,
            width: 1.0,
            radius: border::radius(5),
        },
        ..button::Style::default()
    }
}

/// A small toggle, lit when engaged.
pub fn button_toggle(
    theme: &Theme,
    status: button::Status,
    on: bool,
    tint: Color,
) -> button::Style {
    let p = palette(theme);
    if on {
        return button::Style {
            background: Some(Background::Color(tint)),
            text_color: if relative_luminance(tint) > 0.5 {
                Color::BLACK
            } else {
                Color::WHITE
            },
            border: Border {
                color: darken(tint, 0.2),
                width: 1.0,
                radius: border::radius(4),
            },
            ..button::Style::default()
        };
    }
    let bg = match status {
        button::Status::Hovered => mix(p.surface_low, tint, 0.25),
        button::Status::Pressed => mix(p.surface_low, tint, 0.4),
        _ => p.surface_low,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: p.text_dim,
        border: Border {
            color: p.border,
            width: 1.0,
            radius: border::radius(4),
        },
        ..button::Style::default()
    }
}

pub fn text_field(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let p = palette(theme);
    let border_color = match status {
        text_input::Status::Focused { .. } => p.accent,
        text_input::Status::Hovered => p.border_strong,
        _ => p.border,
    };
    text_input::Style {
        background: Background::Color(p.surface_low),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: border::radius(5),
        },
        icon: p.text_dim,
        placeholder: p.text_dim,
        value: p.text,
        selection: Color {
            a: 0.35,
            ..p.accent
        },
    }
}

pub fn picker(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let p = palette(theme);
    let border_color = match status {
        pick_list::Status::Opened { .. } | pick_list::Status::Hovered => p.accent,
        _ => p.border,
    };
    pick_list::Style {
        text_color: p.text,
        placeholder_color: p.text_dim,
        handle_color: p.text_dim,
        background: Background::Color(p.surface_low),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: border::radius(5),
        },
    }
}

pub fn menu(theme: &Theme) -> iced::widget::overlay::menu::Style {
    let p = palette(theme);
    iced::widget::overlay::menu::Style {
        background: Background::Color(p.surface_high),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: border::radius(5),
        },
        text_color: p.text,
        selected_background: Background::Color(p.accent),
        selected_text_color: p.accent_text,
        shadow: Shadow {
            color: Color {
                a: if p.is_dark { 0.5 } else { 0.18 },
                ..Color::BLACK
            },
            offset: Vector::new(0.0, 2.0),
            blur_radius: 10.0,
        },
    }
}

pub fn fader(theme: &Theme, status: slider::Status) -> slider::Style {
    let p = palette(theme);
    let handle_color = match status {
        slider::Status::Hovered | slider::Status::Dragged => lighten(p.accent, 0.12),
        _ => p.accent,
    };
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(p.accent),
                Background::Color(p.surface_low),
            ),
            width: 5.0,
            border: Border {
                color: p.border,
                width: 1.0,
                radius: border::radius(3),
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Rectangle {
                width: 10,
                border_radius: border::radius(3),
            },
            background: Background::Color(handle_color),
            border_color: darken(handle_color, 0.25),
            border_width: 1.0,
        },
    }
}

pub fn scroll(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let p = palette(theme);
    let hovered = matches!(
        status,
        scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. }
    );
    let rail = scrollable::Rail {
        background: Some(Background::Color(Color {
            a: 0.0,
            ..p.surface_low
        })),
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(if hovered { p.border_strong } else { p.border }),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: border::radius(4),
            },
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(p.surface_high),
            border: Border {
                color: p.border,
                width: 1.0,
                radius: border::radius(12),
            },
            shadow: Shadow::default(),
            icon: p.text_dim,
        },
    }
}

pub fn separator(theme: &Theme) -> rule::Style {
    let p = palette(theme);
    rule::Style {
        color: p.border,
        radius: border::radius(0),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

pub fn progress(theme: &Theme) -> progress_bar::Style {
    let p = palette(theme);
    progress_bar::Style {
        background: Background::Color(p.surface_low),
        bar: Background::Color(p.accent),
        border: Border {
            color: p.border,
            width: 1.0,
            radius: border::radius(3),
        },
    }
}

// ---------------------------------------------------------------------------
// Colour helpers
// ---------------------------------------------------------------------------

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

pub fn lighten(c: Color, amount: f32) -> Color {
    mix(c, Color::WHITE, amount)
}

pub fn darken(c: Color, amount: f32) -> Color {
    mix(c, Color::BLACK, amount)
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

pub fn relative_luminance(c: Color) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

/// Pick black or white text depending on what will read on `background`.
pub fn contrasting_text(background: Color) -> Color {
    if relative_luminance(background) > 0.55 {
        Color::from_rgb(0.08, 0.09, 0.07)
    } else {
        Color::from_rgb(0.96, 0.98, 0.94)
    }
}
