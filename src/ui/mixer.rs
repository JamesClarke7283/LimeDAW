//! The mixer: one strip per insert, plus the effect chain of the selected one.

use iced::widget::{button, column, container, row, scrollable, text, text_input, vertical_slider};
use iced::{Alignment, Element, Length, Theme};

use crate::app::{FxHost, LimeDaw, Message};
use crate::theme;
use crate::ui;

const STRIP_WIDTH: f32 = 78.0;

pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let strips = row(app
        .project
        .mixer
        .tracks
        .iter()
        .enumerate()
        .map(|(index, _)| strip(app, index)))
    .spacing(4);

    let selected = app.selected_mixer_track;
    let chain = ui::effects::view(app, FxHost::MixerTrack(selected));

    column![
        container(
            scrollable(container(strips).padding(6))
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::new().width(6).scroller_width(6)
                ))
                .style(theme::scroll)
        )
        .height(Length::Fixed(330.0))
        .style(theme::well),
        container(chain).height(Length::Fill).style(theme::panel),
    ]
    .spacing(6)
    .into()
}

fn strip(app: &LimeDaw, index: usize) -> Element<'_, Message> {
    let Some(track) = app.project.mixer.tracks.get(index) else {
        return container(text("")).into();
    };
    let selected = app.selected_mixer_track == index;
    let is_master = index == 0;
    let (left, right) = app.engine.status().meter(index);

    let name = text_input("", &track.name)
        .on_input(move |value| Message::RenameMixerTrack(index, value))
        .size(11)
        .padding([2, 4])
        .width(Length::Fixed(STRIP_WIDTH - 8.0))
        .style(theme::text_field);

    let pan = ui::knob_bipolar(
        "Pan",
        format_pan(track.pan),
        track.pan,
        -1.0,
        1.0,
        34.0,
        move |v| Message::SetMixerPan(index, v),
    );

    let fader = vertical_slider(0.0..=1.5f32, track.volume, move |v| {
        Message::SetMixerVolume(index, v)
    })
    .step(0.005f32)
    .height(150.0)
    .style(theme::fader);

    let meters = ui::meter(left, right, 12.0, 150.0);

    let mute = button(text("M").size(10))
        .padding([2, 6])
        .style({
            let on = track.muted;
            move |theme: &Theme, status| {
                theme::button_toggle(theme, status, on, theme::palette(theme).danger)
            }
        })
        .on_press(Message::ToggleMixerMute(index));

    let solo: Element<'_, Message> = if is_master {
        container(text("")).width(Length::Fixed(24.0)).into()
    } else {
        button(text("S").size(10))
            .padding([2, 6])
            .style({
                let on = track.solo;
                move |theme: &Theme, status| {
                    theme::button_toggle(theme, status, on, theme::palette(theme).warning)
                }
            })
            .on_press(Message::ToggleMixerSolo(index))
            .into()
    };

    let fx_count = track.effects.len();
    let fx_label = if fx_count == 0 {
        "no fx".to_string()
    } else {
        format!("{fx_count} fx")
    };

    let body = column![
        name,
        pan,
        row![slider_column(fader), meters,]
            .spacing(4)
            .align_y(Alignment::Center),
        text(format!(
            "{:.1} dB",
            crate::audio::dsp::gain_to_db(track.volume)
        ))
        .size(9),
        row![mute, solo].spacing(4),
        ui::caption(fx_label),
    ]
    .spacing(5)
    .align_x(Alignment::Center);

    let inner = button(body)
        .padding(6)
        .width(Length::Fixed(STRIP_WIDTH))
        .style(move |theme: &Theme, status| {
            let p = theme::palette(theme);
            let base = if selected {
                theme::mix(p.surface_high, p.accent, 0.18)
            } else if is_master {
                theme::mix(p.surface_high, p.accent, 0.05)
            } else {
                p.surface_high
            };
            let bg = match status {
                iced::widget::button::Status::Hovered => theme::lighten(base, 0.04),
                _ => base,
            };
            iced::widget::button::Style {
                background: Some(iced::Background::Color(bg)),
                text_color: p.text,
                border: iced::Border {
                    color: if selected { p.accent } else { p.border },
                    width: 1.0,
                    radius: iced::border::radius(6),
                },
                ..Default::default()
            }
        })
        .on_press(Message::SelectMixerTrack(index));

    inner.into()
}

/// Wrap the vertical fader so it keeps its width in the row.
fn slider_column<'a>(
    fader: iced::widget::VerticalSlider<'a, f32, Message>,
) -> Element<'a, Message> {
    container(fader)
        .width(Length::Fixed(24.0))
        .height(Length::Fixed(150.0))
        .center_x(Length::Fixed(24.0))
        .into()
}

pub fn format_pan(pan: f32) -> String {
    if pan.abs() < 0.01 {
        "C".into()
    } else if pan < 0.0 {
        format!("{:.0}L", pan.abs() * 100.0)
    } else {
        format!("{:.0}R", pan * 100.0)
    }
}
