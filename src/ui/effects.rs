//! The effect rack: a device panel per effect, with every parameter on show.
//!
//! Shared by the mixer and the channel strip, so an effect looks and behaves
//! the same wherever it is inserted.

use iced::widget::{button, column, container, pick_list, row, scrollable, text, tooltip};
use iced::{Alignment, Color, Element, Length, Theme};

use crate::app::{FxHost, LimeDaw, Message};
use crate::model::{EffectCategory, EffectKind, ParamKind, effects_by_category};
use crate::theme;
use crate::ui;

/// Diameter of a rack knob.
const KNOB: f32 = 42.0;

/// The effects dock along the bottom of the window.
///
/// It shows one chain at a time and, above it, the path the signal actually
/// takes: the channel's own effects, then the mixer insert it is routed to,
/// then the master. Clicking a step moves the dock to that chain, which is how
/// LMMS's per-track chain and FX channels fit together.
pub fn dock(app: &LimeDaw) -> Element<'_, Message> {
    container(column![routing(app), view(app, app.fx_focus)].spacing(0))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(theme::panel)
        .into()
}

/// The signal path, as a row of clickable steps.
fn routing(app: &LimeDaw) -> Element<'_, Message> {
    let channel = app
        .selected_channel_id()
        .and_then(|id| app.project.channel(id));
    let focus = app.fx_focus;

    let mut path = row![].spacing(4).align_y(Alignment::Center);

    if let Some(channel) = channel {
        let id = channel.id;
        let insert = channel.mixer_track.min(app.project.mixer.tracks.len() - 1);

        path = path.push(step_button(
            format!("{} · {} fx", channel.name, channel.effects.len()),
            "The channel's own effects, before it reaches the mixer",
            focus == FxHost::Channel(id),
            Message::FocusEffects(FxHost::Channel(id)),
        ));
        path = path.push(arrow());

        // Which insert this channel feeds, changeable here.
        let options: Vec<String> = app
            .project
            .mixer
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{i}: {}", t.name))
            .collect();
        let selected = options.get(insert).cloned();
        path = path.push(
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
        );
        path = path.push(tooltip(
            button(text("+").size(12))
                .padding([4, 8])
                .style(theme::button_plain)
                .on_press(Message::RouteChannelToNewInsert(id)),
            container(text("Route to the next free insert").size(11))
                .padding(6)
                .style(theme::card),
            tooltip::Position::Top,
        ));

        if insert != 0 {
            let name = app.project.mixer.tracks[insert].name.clone();
            let count = app.project.mixer.tracks[insert].effects.len();
            path = path.push(arrow());
            path = path.push(step_button(
                format!("{name} · {count} fx"),
                "The mixer insert this channel is routed to",
                focus == FxHost::MixerTrack(insert),
                Message::FocusEffects(FxHost::MixerTrack(insert)),
            ));
        }
    }

    path = path.push(arrow());
    let master_count = app.project.mixer.tracks[0].effects.len();
    path = path.push(step_button(
        format!("Master · {master_count} fx"),
        "The last thing before the output",
        focus == FxHost::MixerTrack(0),
        Message::FocusEffects(FxHost::MixerTrack(0)),
    ));

    container(
        row![
            ui::caption("Signal path"),
            path,
            iced::widget::space::horizontal(),
            button(text("Hide").size(11))
                .padding([4, 9])
                .style(theme::button_plain)
                .on_press(Message::ToggleEffectsDock),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([5, 8])
    .width(Length::Fill)
    .style(theme::header)
    .into()
}

fn step_button<'a>(
    label: String,
    hint: &'a str,
    active: bool,
    message: Message,
) -> Element<'a, Message> {
    tooltip(
        button(text(label).size(11))
            .padding([4, 10])
            .style(move |theme: &Theme, status| theme::button_tab(theme, status, active))
            .on_press(message),
        container(text(hint).size(11)).padding(6).style(theme::card),
        tooltip::Position::Top,
    )
    .into()
}

fn arrow<'a>() -> Element<'a, Message> {
    text("→")
        .size(12)
        .style(|theme: &Theme| text::Style {
            color: Some(theme::palette(theme).text_dim),
        })
        .into()
}

/// The chain for one host, as a stack of device panels.
pub fn view(app: &LimeDaw, host: FxHost) -> Element<'_, Message> {
    let tint = match host {
        FxHost::Channel(id) => match app.project.channel(id) {
            Some(channel) => channel.color.to_iced(),
            None => return container(text("")).into(),
        },
        FxHost::MixerTrack(t) => {
            if app.project.mixer.tracks.get(t).is_none() {
                return container(text("")).into();
            }
            theme::palette(&app.theme).accent
        }
    };

    let Some(chain) = app.effect_chain(host) else {
        return container(text("")).into();
    };

    let devices: Element<'_, Message> = if chain.is_empty() {
        container(
            column![
                text("No effects yet").size(13),
                ui::caption("Pick one below to start shaping the sound."),
            ]
            .spacing(4)
            .align_x(Alignment::Center),
        )
        .padding(24)
        .width(Length::Fill)
        .style(theme::well)
        .into()
    } else {
        column(
            chain
                .iter()
                .enumerate()
                .map(|(slot, effect)| device(host, slot, effect, chain.len(), tint)),
        )
        .spacing(8)
        .into()
    };

    scrollable(
        column![devices, add_menu(host)]
            .spacing(10)
            .padding([8, 10]),
    )
    .style(theme::scroll)
    .height(Length::Fill)
    .into()
}

/// One effect, drawn as a device with a header and its parameters laid out.
fn device<'a>(
    host: FxHost,
    slot: usize,
    effect: &'a crate::model::Effect,
    chain_len: usize,
    tint: Color,
) -> Element<'a, Message> {
    let enabled = effect.enabled;
    let is_first = slot == 0;
    let is_last = slot + 1 == chain_len;

    let power = tooltip(
        button(text(if enabled { "⏻" } else { "○" }).size(12))
            .padding([3, 8])
            .style(move |theme: &Theme, status| {
                theme::button_toggle(theme, status, enabled, theme::palette(theme).accent)
            })
            .on_press(Message::ToggleEffect(host, slot)),
        container(text(if enabled { "Bypass" } else { "Enable" }).size(11))
            .padding(6)
            .style(theme::card),
        tooltip::Position::Bottom,
    );

    let presets = effect.kind.presets();
    let preset_names: Vec<String> = presets.iter().map(|(n, _)| (*n).to_string()).collect();
    let preset_picker: Element<'_, Message> = if preset_names.is_empty() {
        container(text("")).into()
    } else {
        pick_list(preset_names, None::<String>, move |name: String| {
            Message::ApplyEffectPreset(host, slot, name)
        })
        .placeholder("Preset…")
        .text_size(11)
        .padding([3, 8])
        .style(theme::picker)
        .into()
    };

    let mut move_up = button(text("↑").size(10))
        .padding([3, 7])
        .style(theme::button_plain);
    if !is_first {
        move_up = move_up.on_press(Message::MoveEffect(host, slot, -1));
    }
    let mut move_down = button(text("↓").size(10))
        .padding([3, 7])
        .style(theme::button_plain);
    if !is_last {
        move_down = move_down.on_press(Message::MoveEffect(host, slot, 1));
    }

    let header = row![
        power,
        column![
            text(format!("{}. {}", slot + 1, effect.name())).size(13),
            ui::caption(effect.description()),
        ]
        .spacing(0),
        iced::widget::space::horizontal(),
        preset_picker,
        move_up,
        move_down,
        button(text("✕").size(10))
            .padding([3, 7])
            .style(theme::button_danger)
            .on_press(Message::RemoveEffect(host, slot)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // Parameters, grouped the way the effect describes them.
    let values = effect.kind.params();
    let groups = row(effect.kind.groups().iter().map(|(name, params)| {
        let controls = row(params.iter().filter_map(|param| {
            let (_, value, min, max) = values.iter().copied().find(|(n, _, _, _)| n == param)?;
            Some(control(
                host,
                slot,
                &effect.kind,
                param,
                value,
                min,
                max,
                tint,
                enabled,
            ))
        }))
        .spacing(8)
        .align_y(Alignment::Center)
        .wrap();

        container(column![ui::caption(*name), controls].spacing(3))
            .padding([6, 8])
            .style(theme::well)
            .into()
    }))
    .spacing(8)
    .wrap();

    container(column![header, groups].spacing(8))
        .padding(10)
        .width(Length::Fill)
        .style(if enabled { theme::card } else { theme::panel })
        .into()
}

/// The right control for one parameter.
#[allow(clippy::too_many_arguments)]
fn control<'a>(
    host: FxHost,
    slot: usize,
    kind: &EffectKind,
    param: &'static str,
    value: f32,
    min: f32,
    max: f32,
    tint: Color,
    enabled: bool,
) -> Element<'a, Message> {
    let label = pretty_label(param);
    match kind.param_kind(param) {
        ParamKind::Toggle => {
            let on = value >= 0.5;
            let next = if on { 0.0 } else { 1.0 };
            column![
                button(text(if on { "On" } else { "Off" }).size(11))
                    .padding([5, 12])
                    .style(move |theme: &Theme, status| theme::button_toggle(
                        theme, status, on, tint
                    ))
                    .on_press(Message::SetEffectParam(host, slot, param.to_string(), next)),
                text(label).size(10).style(|theme: &Theme| text::Style {
                    color: Some(theme::palette(theme).text_dim)
                }),
            ]
            .spacing(3)
            .align_x(Alignment::Center)
            .into()
        }
        ParamKind::Choice(options) => {
            let names: Vec<String> = options.iter().map(|o| (*o).to_string()).collect();
            let current = names.get(value.clamp(0.0, 32.0) as usize).cloned();
            let lookup = options;
            column![
                pick_list(names, current, move |chosen: String| {
                    let index = lookup.iter().position(|o| *o == chosen).unwrap_or(0);
                    Message::SetEffectParam(host, slot, param.to_string(), index as f32)
                })
                .text_size(11)
                .padding([4, 8])
                .style(theme::picker),
                text(label).size(10).style(|theme: &Theme| text::Style {
                    color: Some(theme::palette(theme).text_dim)
                }),
            ]
            .spacing(3)
            .align_x(Alignment::Center)
            .into()
        }
        ParamKind::Stepped | ParamKind::Continuous => {
            let stepped = matches!(kind.param_kind(param), ParamKind::Stepped);
            let bipolar = is_bipolar(param, min, max);
            let display = format_param(param, value);
            let colour = if enabled {
                tint
            } else {
                Color { a: 0.4, ..tint }
            };
            let on_change = move |v: f32| {
                let v = if stepped { v.round() } else { v };
                Message::SetEffectParam(host, slot, param.to_string(), v)
            };
            if bipolar {
                ui::knob_bipolar_tinted(label, display, value, min, max, KNOB, colour, on_change)
            } else {
                ui::knob_tinted(label, display, value, min, max, KNOB, colour, on_change)
            }
        }
    }
}

/// Whether a knob should draw its arc outward from the centre.
fn is_bipolar(param: &str, min: f32, max: f32) -> bool {
    matches!(
        param,
        "low_gain" | "mid_gain" | "high_gain" | "makeup" | "feedback"
    ) && min < 0.0
        && max > 0.0
}

/// Turn a parameter name into something worth printing under a knob.
pub fn pretty_label(param: &str) -> &'static str {
    match param {
        "size" => "Size",
        "damping" => "Damping",
        "width" => "Width",
        "predelay" => "Pre-delay",
        "mix" => "Mix",
        "time" => "Time",
        "sync" => "Sync",
        "feedback" => "Feedback",
        "ping_pong" => "Ping-pong",
        "rate" => "Rate",
        "depth" => "Depth",
        "voices" => "Voices",
        "spread" => "Spread",
        "drive" => "Drive",
        "shape" => "Shape",
        "tone" => "Tone",
        "output" => "Output",
        "kind" => "Type",
        "cutoff" => "Cutoff",
        "resonance" => "Reso",
        "env_follow" => "Follow",
        "low_gain" => "Low",
        "mid_gain" => "Mid",
        "high_gain" => "High",
        "low_freq" => "Low freq",
        "mid_freq" => "Mid freq",
        "high_freq" => "High freq",
        "mid_q" => "Mid Q",
        "threshold" => "Threshold",
        "ratio" => "Ratio",
        "attack" => "Attack",
        "release" => "Release",
        "makeup" => "Makeup",
        "knee" => "Knee",
        "ceiling" => "Ceiling",
        "bits" => "Bits",
        "downsample" => "Rate cut",
        "stages" => "Stages",
        "hold" => "Hold",
        "bass_mono" => "Bass mono",
        _ => "Value",
    }
}

/// One entry in the "add effect" menu: shown by family and name, and carrying
/// the slug the message needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectChoice {
    pub slug: &'static str,
    label: String,
}

impl std::fmt::Display for EffectChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Every effect the rack can add, in menu order.
pub fn effect_choices() -> Vec<EffectChoice> {
    effects_by_category()
        .into_iter()
        .flat_map(|(category, slugs)| {
            slugs.into_iter().map(move |slug| {
                let name = EffectKind::from_slug(slug)
                    .map(|k| k.name())
                    .unwrap_or(slug);
                EffectChoice {
                    slug,
                    label: format!("{} · {}", category.label(), name),
                }
            })
        })
        .collect()
}

/// The "add effect" control: a single dropdown, with each effect's purpose
/// spelled out beside it.
fn add_menu<'a>(host: FxHost) -> Element<'a, Message> {
    let choices = effect_choices();
    let picker = pick_list(
        choices,
        None::<EffectChoice>,
        move |choice: EffectChoice| Message::AddEffect(host, choice.slug.to_string()),
    )
    .placeholder("Add an effect…")
    .text_size(12)
    .padding([6, 10])
    .width(Length::Fixed(260.0))
    .style(theme::picker);

    let reference = column(EffectCategory::ALL.iter().map(|category| {
        let names: Vec<&str> = EffectKind::SLUGS
            .iter()
            .filter(|slug| {
                EffectKind::from_slug(slug)
                    .map(|k| k.category() == *category)
                    .unwrap_or(false)
            })
            .filter_map(|slug| EffectKind::from_slug(slug).map(|k| k.name()))
            .collect();
        row![
            container(ui::caption(category.label())).width(Length::Fixed(88.0)),
            ui::caption(names.join(", ")),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }))
    .spacing(2);

    container(
        column![
            row![
                picker,
                iced::widget::space::horizontal(),
                tooltip(
                    ui::caption("What are these?"),
                    container(
                        column(EffectKind::SLUGS.iter().filter_map(|slug| {
                            let kind = EffectKind::from_slug(slug)?;
                            Some(
                                row![
                                    container(text(kind.name()).size(11))
                                        .width(Length::Fixed(90.0)),
                                    text(kind.description()).size(11),
                                ]
                                .spacing(6)
                                .into(),
                            )
                        }))
                        .spacing(3)
                    )
                    .padding(10)
                    .max_width(520)
                    .style(theme::card),
                    tooltip::Position::Top,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            reference,
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Length::Fill)
    .style(theme::panel)
    .into()
}

/// Present a parameter value in units a musician recognises.
pub fn format_param(name: &str, value: f32) -> String {
    match name {
        "cutoff" | "low_freq" | "mid_freq" | "high_freq" => {
            if value >= 1000.0 {
                format!("{:.2} kHz", value / 1000.0)
            } else {
                format!("{value:.0} Hz")
            }
        }
        "damping" if value > 40.0 => {
            if value >= 1000.0 {
                format!("{:.1} kHz", value / 1000.0)
            } else {
                format!("{value:.0} Hz")
            }
        }
        "bass_mono" => {
            if value < 20.0 {
                "off".into()
            } else {
                format!("{value:.0} Hz")
            }
        }
        "threshold" | "makeup" | "ceiling" | "knee" | "low_gain" | "mid_gain" | "high_gain" => {
            format!("{value:+.1} dB")
        }
        "attack" | "release" | "hold" | "predelay" => {
            if value >= 1000.0 {
                format!("{:.2} s", value / 1000.0)
            } else {
                format!("{value:.1} ms")
            }
        }
        "ratio" => format!("{value:.1}:1"),
        "rate" => format!("{value:.2} Hz"),
        "time" => format!("{value:.3}"),
        "bits" => format!("{value:.0} bit"),
        "downsample" => format!("÷{value:.0}"),
        "stages" | "voices" => format!("{value:.0}"),
        "width" if value > 1.001 => format!("{:.0}%", value * 100.0),
        "mix" | "depth" | "size" | "drive" | "tone" | "spread" | "resonance" | "env_follow" => {
            format!("{:.0}%", value * 100.0)
        }
        "feedback" => format!("{:.0}%", value * 100.0),
        _ => {
            if value.abs() < 10.0 {
                format!("{value:.2}")
            } else {
                format!("{value:.0}")
            }
        }
    }
}
