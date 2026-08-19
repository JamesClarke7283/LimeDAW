//! The effect chain editor, shared by the mixer and the channel strip.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Theme};

use crate::app::{FxHost, LimeDaw, Message};
use crate::model::EffectKind;
use crate::theme;
use crate::ui;

pub fn view(app: &LimeDaw, host: FxHost) -> Element<'_, Message> {
    let title = match host {
        FxHost::Channel(id) => app
            .project
            .channel(id)
            .map(|c| format!("{} — channel effects", c.name))
            .unwrap_or_else(|| "Channel effects".into()),
        FxHost::MixerTrack(t) => app
            .project
            .mixer
            .tracks
            .get(t)
            .map(|track| format!("{} — insert effects", track.name))
            .unwrap_or_else(|| "Insert effects".into()),
    };

    let Some(chain) = app.effect_chain(host) else {
        return container(text("")).into();
    };

    let add_buttons = row(EffectKind::SLUGS.map(|slug| {
        let label = EffectKind::from_slug(slug).map(|k| k.name()).unwrap_or(slug);
        button(text(label).size(10))
            .padding([3, 7])
            .style(theme::button_plain)
            .on_press(Message::AddEffect(host, slug.to_string()))
            .into()
    }))
    .spacing(3)
    .wrap();

    let slots = column(chain.iter().enumerate().map(|(slot, effect)| {
        let selected = app.selected_effect == Some((host, slot));
        let enabled = effect.enabled;
        let header = row![
            button(text(if enabled { "●" } else { "○" }).size(11))
                .padding([2, 6])
                .style(move |theme: &Theme, status| theme::button_toggle(
                    theme,
                    status,
                    enabled,
                    theme::palette(theme).accent
                ))
                .on_press(Message::ToggleEffect(host, slot)),
            button(text(effect.name()).size(12))
                .padding([2, 6])
                .width(Length::Fixed(110.0))
                .style(theme::button_ghost)
                .on_press(Message::SelectEffect(host, slot)),
            iced::widget::space::horizontal(),
            button(text("↑").size(10))
                .padding([2, 6])
                .style(theme::button_plain)
                .on_press(Message::MoveEffect(host, slot, -1)),
            button(text("↓").size(10))
                .padding([2, 6])
                .style(theme::button_plain)
                .on_press(Message::MoveEffect(host, slot, 1)),
            button(text("✕").size(10))
                .padding([2, 6])
                .style(theme::button_danger)
                .on_press(Message::RemoveEffect(host, slot)),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let knobs: Element<'_, Message> = if selected {
            let controls = row(effect.kind.params().into_iter().map(|(name, value, min, max)| {
                let label: &'static str = name;
                let display = format_param(name, value);
                ui::knob(label, display, value, min, max, 38.0, move |v| {
                    Message::SetEffectParam(host, slot, label.to_string(), v)
                })
            }))
            .spacing(10)
            .wrap();
            container(controls).padding([6, 4]).into()
        } else {
            container(text("")).into()
        };

        container(column![header, knobs].spacing(2))
            .padding(6)
            .width(Length::Fill)
            .style(if selected { theme::selected } else { theme::card })
            .into()
    }))
    .spacing(5);

    column![
        row![text(title).size(13)].align_y(Alignment::Center),
        container(add_buttons).padding([2, 0]),
        scrollable(container(slots).padding([0, 4]))
            .style(theme::scroll)
            .height(Length::Fill),
    ]
    .spacing(6)
    .padding(8)
    .into()
}

/// Present a parameter value in units a musician recognises.
pub fn format_param(name: &str, value: f32) -> String {
    match name {
        "cutoff" | "damping" if value > 40.0 => format!("{:.0} Hz", value),
        "low_freq" | "mid_freq" | "high_freq" | "bass_mono" => format!("{:.0} Hz", value),
        "threshold" | "makeup" | "ceiling" | "knee" | "low_gain" | "mid_gain" | "high_gain" => {
            format!("{:+.1} dB", value)
        }
        "attack" | "release" | "hold" | "predelay" => format!("{:.0} ms", value),
        "ratio" => format!("{:.1}:1", value),
        "rate" => format!("{:.2} Hz", value),
        "time" => format!("{:.3}", value),
        "bits" | "stages" | "voices" | "downsample" | "shape" | "kind" => format!("{:.0}", value),
        "sync" | "ping_pong" => {
            if value >= 0.5 {
                "on".into()
            } else {
                "off".into()
            }
        }
        _ => {
            if value.abs() < 10.0 {
                format!("{:.2}", value)
            } else {
                format!("{:.0}", value)
            }
        }
    }
}
