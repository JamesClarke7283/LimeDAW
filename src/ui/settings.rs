//! The settings view.

use iced::widget::{button, column, container, pick_list, row, scrollable, slider, text};
use iced::{Alignment, Element, Length};

use crate::app::{LimeDaw, Message};
use crate::config::RendererChoice;
use crate::theme::{self, ThemeChoice};
use crate::ui;

pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let appearance = card(
        "Appearance",
        column![
            ui::field(
                "Theme",
                pick_list(ThemeChoice::ALL.to_vec(), Some(app.settings.theme), Message::SetTheme)
                    .text_size(12)
                    .padding([5, 9])
                    .style(theme::picker),
            ),
            ui::caption(if app.system_dark {
                "The system is currently using a dark appearance."
            } else {
                "The system is currently using a light appearance."
            }),
            labelled(
                format!("Interface scale — {:.0}%", app.settings.ui_scale * 100.0),
                slider(0.6..=2.0f32, app.settings.ui_scale, Message::SetUiScale)
                    .step(0.05f32)
                    .width(Length::Fixed(240.0))
                    .style(theme::fader),
            ),
        ]
        .spacing(12),
    );

    let rendering = card(
        "Rendering",
        column![
            ui::field(
                "Renderer",
                pick_list(
                    RendererChoice::ALL.to_vec(),
                    Some(app.settings.renderer),
                    Message::SetRenderer
                )
                .text_size(12)
                .padding([5, 9])
                .style(theme::picker),
            ),
            ui::caption(app.settings.renderer.description()),
            ui::caption("Takes effect the next time LimeDAW starts."),
        ]
        .spacing(10),
    );

    let audio = card(
        "Audio",
        column![
            row![
                ui::caption("Output"),
                text(if app.engine.silent {
                    "No device — LimeDAW is running silently".to_string()
                } else {
                    format!("{:.0} Hz", app.engine.sample_rate())
                })
                .size(12),
            ]
            .spacing(10),
            labelled(
                format!("Default note velocity — {:.0}%", app.settings.default_velocity * 100.0),
                slider(0.05..=1.0f32, app.settings.default_velocity, Message::SetDefaultVelocity)
                    .step(0.01f32)
                    .width(Length::Fixed(240.0))
                    .style(theme::fader),
            ),
            button(text("Stop all sound").size(12))
                .padding([5, 10])
                .style(theme::button_danger)
                .on_press(Message::Panic),
        ]
        .spacing(12),
    );

    let soundfont = card(
        "SoundFont",
        column![
            text(
                app.soundfont_path
                    .as_ref()
                    .map(|p| crate::paths::display(p))
                    .unwrap_or_else(|| "None loaded".into())
            )
            .size(12),
            ui::caption(format!("{} presets available", app.soundfont_presets.len())),
            button(text("Choose SoundFont…").size(12))
                .padding([5, 10])
                .style(theme::button_plain)
                .on_press(Message::ChooseSoundFontDialog),
        ]
        .spacing(8),
    );

    let folders = card(
        "Folders",
        column![
            path_row("Projects", crate::paths::projects_dir()),
            path_row("Renders", crate::paths::renders_dir()),
            path_row("Samples", crate::paths::samples_dir()),
            path_row("Settings", crate::paths::config_dir()),
            path_row("Data", crate::paths::data_dir()),
        ]
        .spacing(6),
    );

    let about = card(
        "About",
        column![
            text(format!("LimeDAW {}", env!("CARGO_PKG_VERSION"))).size(13),
            ui::caption("A pattern-based DAW: channel rack, piano roll, playlist, mixer and an MCP server."),
            ui::caption("Space plays. Ctrl+S saves. Ctrl+E renders. Escape stops all sound."),
        ]
        .spacing(6),
    );

    scrollable(
        column![appearance, rendering, audio, soundfont, folders, about]
            .spacing(12)
            .padding(14)
            .max_width(760),
    )
    .style(theme::scroll)
    .height(Length::Fill)
    .into()
}

/// A label above a control, where the label is built at call time.
fn labelled<'a>(label: String, content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![ui::caption(label), content.into()].spacing(2).into()
}

fn card<'a>(title: &'a str, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(column![text(title).size(14), body.into()].spacing(10))
        .padding(14)
        .width(Length::Fill)
        .style(theme::card)
        .into()
}

fn path_row<'a>(label: &'a str, path: std::path::PathBuf) -> Element<'a, Message> {
    let display = crate::paths::display(&path);
    row![
        container(ui::caption(label)).width(Length::Fixed(80.0)),
        text(display.clone()).size(11),
        iced::widget::space::horizontal(),
        button(text("Copy").size(10))
            .padding([2, 8])
            .style(theme::button_plain)
            .on_press(Message::CopyText(path.to_string_lossy().to_string())),
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .into()
}
