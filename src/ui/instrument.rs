//! The instrument panel: edit whatever the selected channel plays.

use iced::widget::{button, column, container, pick_list, row, scrollable, slider, text, text_input};
use iced::{Alignment, Element, Length, Theme};

use crate::app::{FxHost, InstrumentKindChoice, LimeDaw, Message};
use crate::model::{DrumModel, Id, Instrument, SampleSource};
use crate::theme;
use crate::ui;

pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let Some(id) = app.selected_channel_id() else {
        return container(text("No channel selected").size(12))
            .padding(12)
            .center_x(Length::Fill)
            .into();
    };
    let Some(channel) = app.project.channel(id) else {
        return container(text("")).into();
    };

    let kind = match channel.instrument {
        Instrument::Synth(_) => InstrumentKindChoice::Synth,
        Instrument::Sampler(_) => InstrumentKindChoice::Sampler,
        Instrument::SoundFont(_) => InstrumentKindChoice::SoundFont,
    };

    let header = row![
        text_input("Channel name", &channel.name)
            .on_input(move |name| Message::RenameChannel(id, name))
            .size(13)
            .padding([4, 8])
            .width(Length::Fixed(150.0))
            .style(theme::text_field),
        pick_list(InstrumentKindChoice::ALL.to_vec(), Some(kind), move |k| {
            Message::SetInstrumentKind(id, k)
        })
        .text_size(12)
        .padding([4, 8])
        .style(theme::picker),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let common = row![
        ui::knob("Volume", format!("{:.0}%", channel.volume * 100.0), channel.volume, 0.0, 2.0, 40.0, move |v| {
            Message::SetChannelVolume(id, v)
        }),
        ui::knob_bipolar("Pan", super::mixer::format_pan(channel.pan), channel.pan, -1.0, 1.0, 40.0, move |v| {
            Message::SetChannelPan(id, v)
        }),
        ui::knob_bipolar(
            "Transpose",
            format!("{:+.0}", channel.transpose),
            channel.transpose,
            -24.0,
            24.0,
            40.0,
            move |v| Message::SetChannelTranspose(id, v.round())
        ),
        routing(app, id),
    ]
    .spacing(14)
    .align_y(Alignment::Center);

    let body: Element<'_, Message> = match &channel.instrument {
        Instrument::Synth(_) => synth_editor(app, id),
        Instrument::Sampler(patch) => sampler_editor(app, id, patch),
        Instrument::SoundFont(patch) => soundfont_editor(app, id, patch.bank, patch.preset),
    };

    scrollable(
        column![
            header,
            common,
            iced::widget::rule::horizontal(1).style(theme::separator),
            body,
            iced::widget::rule::horizontal(1).style(theme::separator),
            ui::effects::view(app, FxHost::Channel(id)),
        ]
        .spacing(10)
        .padding(10),
    )
    .style(theme::scroll)
    .height(Length::Fill)
    .into()
}

fn routing(app: &LimeDaw, id: Id) -> Element<'_, Message> {
    let current = app.project.channel(id).map(|c| c.mixer_track).unwrap_or(0);
    let options: Vec<String> = app
        .project
        .mixer
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{i}: {}", t.name))
        .collect();
    let selected = options.get(current).cloned();
    ui::field(
        "Mixer insert",
        pick_list(options, selected, move |label: String| {
            let index = label
                .split(':')
                .next()
                .and_then(|n| n.trim().parse::<usize>().ok())
                .unwrap_or(0);
            Message::SetChannelMixerTrack(id, index)
        })
        .text_size(11)
        .padding([4, 8])
        .style(theme::picker),
    )
}

/// Group a set of named parameters into a labelled block of knobs.
fn knob_group<'a>(
    app: &'a LimeDaw,
    id: Id,
    title: &'a str,
    names: &'a [&'static str],
) -> Element<'a, Message> {
    let Some(channel) = app.project.channel(id) else {
        return container(text("")).into();
    };
    let params = channel.instrument.params();
    let knobs = row(names.iter().filter_map(|name| {
        let info = params.iter().find(|p| p.name == *name)?;
        let label: &'static str = info.label;
        let param_name: &'static str = info.name;
        let display = display_value(param_name, info.value);
        Some(ui::knob(
            label,
            display,
            info.value,
            info.min,
            info.max,
            40.0,
            move |v| Message::SetInstrumentParam(id, param_name.to_string(), v),
        ))
    }))
    .spacing(10)
    .wrap();

    container(column![ui::caption(title), knobs].spacing(4))
        .padding(8)
        .style(theme::card)
        .into()
}

fn synth_editor(app: &LimeDaw, id: Id) -> Element<'_, Message> {
    let Some(channel) = app.project.channel(id) else {
        return container(text("")).into();
    };
    let Instrument::Synth(patch) = &channel.instrument else {
        return container(text("")).into();
    };

    let oscillators = row((0..3).map(|i| {
        let names: &'static [&'static str] = match i {
            0 => &["osc1_enabled", "osc1_wave", "osc1_semitones", "osc1_cents", "osc1_level", "osc1_unison", "osc1_spread", "osc1_width"],
            1 => &["osc2_enabled", "osc2_wave", "osc2_semitones", "osc2_cents", "osc2_level", "osc2_unison", "osc2_spread", "osc2_width"],
            _ => &["osc3_enabled", "osc3_wave", "osc3_semitones", "osc3_cents", "osc3_level", "osc3_unison", "osc3_spread", "osc3_width"],
        };
        let title = match i {
            0 => "Oscillator 1",
            1 => "Oscillator 2",
            _ => "Oscillator 3",
        };
        let wave = patch.osc[i].waveform;
        let param = match i {
            0 => "osc1_wave",
            1 => "osc2_wave",
            _ => "osc3_wave",
        };
        column![
            knob_group(app, id, title, names),
            pick_list(crate::model::Waveform::ALL.to_vec(), Some(wave), move |w| {
                Message::SetInstrumentParam(id, param.to_string(), w as u8 as f32)
            })
            .text_size(11)
            .padding([3, 6])
            .style(theme::picker),
        ]
        .spacing(4)
        .into()
    }))
    .spacing(8)
    .wrap();

    let filter_kind = pick_list(
        crate::model::FilterKind::ALL.to_vec(),
        Some(patch.filter),
        move |k| Message::SetInstrumentParam(id, "filter".into(), k as u8 as f32),
    )
    .text_size(11)
    .padding([3, 6])
    .style(theme::picker);

    let lfo_wave = pick_list(
        crate::model::Waveform::ALL.to_vec(),
        Some(patch.lfo.waveform),
        move |w| Message::SetInstrumentParam(id, "lfo_wave".into(), w as u8 as f32),
    )
    .text_size(11)
    .padding([3, 6])
    .style(theme::picker);

    let lfo_target = pick_list(
        crate::model::LfoTarget::ALL.to_vec(),
        Some(patch.lfo.target),
        move |t| Message::SetInstrumentParam(id, "lfo_target".into(), t as u8 as f32),
    )
    .text_size(11)
    .padding([3, 6])
    .style(theme::picker);

    column![
        oscillators,
        row![
            knob_group(app, id, "Amp envelope", &["attack", "decay", "sustain", "release"]),
            column![
                knob_group(
                    app,
                    id,
                    "Filter",
                    &["cutoff", "resonance", "filter_env_amount", "filter_key_track"]
                ),
                filter_kind,
            ]
            .spacing(4),
            knob_group(
                app,
                id,
                "Filter envelope",
                &["filter_attack", "filter_decay", "filter_sustain", "filter_release"]
            ),
        ]
        .spacing(8)
        .wrap(),
        row![
            column![
                knob_group(app, id, "LFO", &["lfo_enabled", "lfo_rate", "lfo_depth", "lfo_delay"]),
                row![lfo_wave, lfo_target].spacing(4),
            ]
            .spacing(4),
            knob_group(app, id, "Voice", &["glide", "mono", "gain", "drive", "velocity_amount"]),
        ]
        .spacing(8)
        .wrap(),
    ]
    .spacing(10)
    .into()
}

fn sampler_editor<'a>(
    app: &'a LimeDaw,
    id: Id,
    patch: &'a crate::model::SamplerPatch,
) -> Element<'a, Message> {
    let sample_key = match &patch.source {
        SampleSource::Model(m) => crate::audio::samples::SampleBank::key_for_model(*m),
        SampleSource::File { path } => path.clone(),
    };
    let peaks = app
        .bank
        .get(&sample_key)
        .map(|s| s.peaks(400))
        .unwrap_or_default();
    let colour = app
        .project
        .channel(id)
        .map(|c| c.color.to_iced())
        .unwrap_or(theme::palette(&app.theme).accent);

    let source_label = match &patch.source {
        SampleSource::Model(m) => format!("Built-in: {}", m.label()),
        SampleSource::File { path } => std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone()),
    };

    let current_model = match &patch.source {
        SampleSource::Model(m) => Some(*m),
        _ => None,
    };

    let model_picker = pick_list(DrumModel::ALL.to_vec(), current_model, move |m| {
        Message::SetDrumModel(id, m)
    })
    .placeholder("Built-in drum…")
    .text_size(11)
    .padding([3, 6])
    .style(theme::picker);

    let load = button(text("Load WAV…").size(11))
        .padding([4, 8])
        .style(theme::button_plain)
        .on_press(Message::LoadSampleDialog(id));

    column![
        row![text(source_label).size(12), iced::widget::space::horizontal(), model_picker, load]
            .spacing(6)
            .align_y(Alignment::Center),
        ui::waveform(peaks, patch.start, patch.end, colour, 90.0),
        row![
            knob_group(app, id, "Playback", &["root_key", "tune", "fine", "start", "end"]),
            knob_group(app, id, "Envelope", &["attack", "decay", "sustain", "release", "gated"]),
            knob_group(app, id, "Tone", &["cutoff", "resonance", "gain", "velocity_amount"]),
            knob_group(app, id, "Mode", &["reverse", "loop"]),
        ]
        .spacing(8)
        .wrap(),
    ]
    .spacing(8)
    .into()
}

fn soundfont_editor(app: &LimeDaw, id: Id, bank: u16, program: u16) -> Element<'_, Message> {
    if app.soundfont.is_none() {
        return container(
            column![
                text("No SoundFont loaded").size(13),
                text("Load one to use SoundFont channels.").size(11),
                button(text("Choose SoundFont…").size(12))
                    .padding([5, 10])
                    .style(theme::button_accent)
                    .on_press(Message::ChooseSoundFontDialog),
            ]
            .spacing(8)
            .align_x(Alignment::Center),
        )
        .padding(16)
        .center_x(Length::Fill)
        .style(theme::notice)
        .into();
    }

    let filter = app.sf_filter.to_lowercase();
    let matches: Vec<&crate::app::SfPreset> = app
        .soundfont_presets
        .iter()
        .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter))
        .take(400)
        .collect();

    let list = column(matches.iter().map(|preset| {
        let active = preset.bank == bank && preset.program == program;
        let (b, pgm) = (preset.bank, preset.program);
        button(
            row![
                text(format!("{:>3}:{:<3}", preset.bank, preset.program)).size(10),
                text(preset.name.clone()).size(12),
            ]
            .spacing(8),
        )
        .padding([3, 6])
        .width(Length::Fill)
        .style(move |theme: &Theme, status| theme::button_tab(theme, status, active))
        .on_press(Message::SetSoundFontPreset(id, b, pgm))
        .into()
    }))
    .spacing(1);

    column![
        row![
            text_input("Search presets", &app.sf_filter)
                .on_input(Message::SoundFontFilter)
                .size(12)
                .padding([4, 8])
                .style(theme::text_field),
            ui::caption(format!("{} presets", app.soundfont_presets.len())),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        container(scrollable(list).style(theme::scroll).height(Length::Fixed(260.0)))
            .style(theme::well)
            .padding(4),
    ]
    .spacing(8)
    .into()
}

/// Format an instrument parameter for its readout.
fn display_value(name: &str, value: f32) -> String {
    match name {
        "cutoff" => {
            if value >= 1000.0 {
                format!("{:.1}k", value / 1000.0)
            } else {
                format!("{:.0}", value)
            }
        }
        n if n.ends_with("_wave") => crate::model::Waveform::ALL
            [(value.clamp(0.0, 6.0) as usize).min(6)]
        .label()
        .to_string(),
        "filter" => crate::model::FilterKind::ALL[(value.clamp(0.0, 4.0) as usize).min(4)]
            .label()
            .to_string(),
        "lfo_target" => crate::model::LfoTarget::ALL[(value.clamp(0.0, 4.0) as usize).min(4)]
            .label()
            .to_string(),
        "root_key" => ui::note_name(value.clamp(0.0, 127.0) as u8),
        n if n.ends_with("_enabled") || n == "mono" || n == "gated" || n == "reverse" || n == "loop" || n == "percussion" => {
            if value >= 0.5 { "on".into() } else { "off".into() }
        }
        "attack" | "decay" | "release" | "filter_attack" | "filter_decay" | "filter_release"
        | "glide" | "lfo_delay" => {
            if value < 1.0 {
                format!("{:.0} ms", value * 1000.0)
            } else {
                format!("{:.2} s", value)
            }
        }
        "lfo_rate" => format!("{:.2} Hz", value),
        "semitones" | "tune" => format!("{:+.0}", value),
        n if n.ends_with("_semitones") => format!("{:+.0}", value),
        n if n.ends_with("_cents") || n == "fine" => format!("{:+.0}c", value),
        n if n.ends_with("_unison") => format!("{:.0}", value),
        _ => {
            if value.abs() >= 100.0 {
                format!("{:.0}", value)
            } else {
                format!("{:.2}", value)
            }
        }
    }
}

/// A compact strip used when the panel is docked narrow.
pub fn quick_controls(app: &LimeDaw) -> Element<'_, Message> {
    let Some(id) = app.selected_channel_id() else {
        return container(text("")).into();
    };
    let Some(channel) = app.project.channel(id) else {
        return container(text("")).into();
    };
    row![
        text(channel.name.clone()).size(12),
        slider(0.0..=1.5, channel.volume, move |v| Message::SetChannelVolume(id, v))
            .step(0.01f32)
            .width(Length::Fixed(90.0))
            .style(theme::fader),
    ]
    .spacing(6)
    .align_y(Alignment::Center)
    .into()
}
