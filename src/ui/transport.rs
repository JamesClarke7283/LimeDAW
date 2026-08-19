//! The transport bar across the top of the window.

use iced::widget::{button, column, container, row, slider, text, text_input};
use iced::{Alignment, Element, Length, Theme};

use crate::app::{LimeDaw, MainView, Message};
use crate::audio::schedule::PlayMode;
use crate::theme;
use crate::ui;

pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let status = app.engine.status();
    let playing = status.playing();
    let position = status.position();
    let beats_per_bar = app.project.beats_per_bar();

    let play = button(text(if playing { "❚❚" } else { "▶" }).size(15))
        .padding([5, 14])
        .style(move |theme: &Theme, s| {
            if playing {
                theme::button_accent(theme, s)
            } else {
                theme::button_plain(theme, s)
            }
        })
        .on_press(Message::TogglePlay);

    let stop = button(text("■").size(13))
        .padding([5, 12])
        .style(theme::button_plain)
        .on_press(Message::Stop);

    let record = button(text("●").size(14))
        .padding([5, 12])
        .style({
            let on = app.recording;
            move |theme: &Theme, s| theme::button_toggle(theme, s, on, theme::palette(theme).danger)
        })
        .on_press(Message::ToggleRecord);

    let mode_buttons = ui::segmented(
        [("Pattern", 0u8), ("Song", 1u8)]
            .map(|(label, value)| (value, label))
            .as_slice(),
        match app.play_mode {
            PlayMode::Pattern(_) => 0u8,
            PlayMode::Song => 1u8,
        },
        move |value| {
            let mode = if value == 0 {
                PlayMode::Pattern(app.selected_pattern.unwrap_or(0))
            } else {
                PlayMode::Song
            };
            Message::SetPlayMode(mode)
        },
    );

    let tempo = column![
        ui::caption("Tempo"),
        text_input("", &app.tempo_text)
            .on_input(Message::TempoInput)
            .on_submit(Message::TempoCommit)
            .size(14)
            .padding([3, 6])
            .width(Length::Fixed(72.0))
            .style(theme::text_field),
    ]
    .spacing(1);

    let time_signature = {
        let ts = app.project.time_signature;
        column![
            ui::caption("Signature"),
            row![
                button(text(format!("{}", ts.numerator)).size(12))
                    .padding([3, 7])
                    .style(theme::button_plain)
                    .on_press(Message::SetTimeSignature(
                        if ts.numerator >= 16 {
                            1
                        } else {
                            ts.numerator + 1
                        },
                        ts.denominator
                    )),
                text("/").size(12),
                button(text(format!("{}", ts.denominator)).size(12))
                    .padding([3, 7])
                    .style(theme::button_plain)
                    .on_press(Message::SetTimeSignature(
                        ts.numerator,
                        match ts.denominator {
                            4 => 8,
                            8 => 16,
                            16 => 2,
                            _ => 4,
                        }
                    )),
            ]
            .spacing(3)
            .align_y(Alignment::Center),
        ]
        .spacing(1)
    };

    let readout = column![
        text(ui::format_position(position, beats_per_bar)).size(18),
        text(ui::format_time(position * app.project.seconds_per_beat()))
            .size(10)
            .style(|theme: &Theme| text::Style {
                color: Some(theme::palette(theme).text_dim)
            }),
    ]
    .align_x(Alignment::Center)
    .spacing(0);

    let metronome = button(text("Metro").size(11))
        .padding([5, 9])
        .style({
            let on = app.settings.metronome;
            move |theme: &Theme, s| theme::button_tab(theme, s, on)
        })
        .on_press(Message::ToggleMetronome);

    let loop_toggle = button(text("Loop").size(11))
        .padding([5, 9])
        .style({
            let on = app.project.playlist.loop_region.is_some();
            move |theme: &Theme, s| theme::button_tab(theme, s, on)
        })
        .on_press(Message::ToggleLoop);

    let swing = column![
        ui::caption(format!("Swing {:.0}%", app.project.swing * 100.0)),
        slider(0.0..=0.9, app.project.swing as f32, |v| Message::SetSwing(
            v as f64
        ))
        .step(0.01f32)
        .width(Length::Fixed(84.0))
        .style(theme::fader),
    ]
    .spacing(1);

    let (master_l, master_r) = status.meter(0);
    let master = column![
        ui::caption("Master"),
        row![
            slider(
                0.0..=1.5,
                app.project.master_volume,
                Message::SetMasterVolume
            )
            .step(0.005f32)
            .width(Length::Fixed(96.0))
            .style(theme::fader),
            ui::meter(master_l, master_r, 44.0, 14.0),
        ]
        .spacing(5)
        .align_y(Alignment::Center),
    ]
    .spacing(1);

    let views = row(MainView::ALL.map(|view| {
        let active = app.view == view;
        button(text(view.label()).size(12))
            .padding([5, 11])
            .style(move |theme: &Theme, s| theme::button_tab(theme, s, active))
            .on_press(Message::SetView(view))
            .into()
    }))
    .spacing(2);

    let octave = column![
        ui::caption("Keys"),
        row![
            button(text("−").size(10))
                .padding([2, 6])
                .style(theme::button_plain)
                .on_press(Message::SetKeyboardOctave(app.keyboard_octave - 1)),
            text(format!("C{}", app.keyboard_octave)).size(12),
            button(text("+").size(10))
                .padding([2, 6])
                .style(theme::button_plain)
                .on_press(Message::SetKeyboardOctave(app.keyboard_octave + 1)),
        ]
        .spacing(3)
        .align_y(Alignment::Center),
    ]
    .spacing(1);

    let engine_info = {
        let voices = status.voices();
        let cpu = status.cpu_load();
        let label = if app.engine.silent {
            "no audio device".to_string()
        } else {
            format!("{voices} voices · {:.0}% dsp", (cpu * 100.0).min(999.0))
        };
        column![
            ui::caption(label),
            ui::caption(format!("{:.0} Hz", app.engine.sample_rate()))
        ]
        .spacing(0)
    };

    let bar = container(
        row![
            row![play, stop, record].spacing(4),
            mode_buttons,
            readout,
            tempo,
            time_signature,
            swing,
            row![metronome, loop_toggle].spacing(4),
            octave,
            iced::widget::space::horizontal(),
            master,
            engine_info,
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding([4, 10]),
    )
    .style(theme::toolbar)
    .width(Length::Fill);

    column![bar, container(views).padding([0, 6])]
        .spacing(4)
        .into()
}
