//! The first-run screen: pick a SoundFont before getting started.

use std::path::PathBuf;

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length};

use crate::app::{LimeDaw, Message};
use crate::theme;
use crate::ui;

pub fn view<'a>(
    app: &'a LimeDaw,
    found: &'a [PathBuf],
    error: Option<&'a str>,
) -> Element<'a, Message> {
    let discovered: Element<'_, Message> = if found.is_empty() {
        container(
            ui::caption(format!(
                "No SoundFonts found. Put a .sf2 file in {} and it will appear here.",
                crate::paths::display(&crate::paths::soundfonts_dir())
            ))
        )
        .padding(10)
        .into()
    } else {
        let items = column(found.iter().map(|path| {
            let label = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let subtitle = crate::paths::display(path);
            let path = path.clone();
            button(
                column![text(label).size(13), ui::caption(subtitle)].spacing(1),
            )
            .padding([7, 10])
            .width(Length::Fill)
            .style(theme::button_plain)
            .on_press(Message::ChooseSoundFontPath(path))
            .into()
        }))
        .spacing(4);
        container(scrollable(items).style(theme::scroll).height(Length::Fixed(210.0)))
            .padding(6)
            .style(theme::well)
            .into()
    };

    let error_notice: Element<'_, Message> = match error {
        Some(message) => container(text(message).size(12))
            .padding(10)
            .width(Length::Fill)
            .style(theme::notice)
            .into(),
        None => container(text("")).into(),
    };

    let card = container(
        column![
            text("Welcome to LimeDAW").size(26),
            ui::caption(
                "Choose a SoundFont to play orchestral, band and general MIDI sounds. \
                 You can skip this — the built-in synth and drum kit work straight away, \
                 and you can load one later from Settings."
            ),
            iced::widget::rule::horizontal(1).style(theme::separator),
            text("Found on this system").size(14),
            discovered,
            error_notice,
            row![
                button(text("Browse…").size(13))
                    .padding([8, 16])
                    .style(theme::button_accent)
                    .on_press(Message::ChooseSoundFontDialog),
                button(text("Skip for now").size(13))
                    .padding([8, 16])
                    .style(theme::button_plain)
                    .on_press(Message::SkipSoundFont),
                iced::widget::space::horizontal(),
                ui::caption(format!(
                    "Projects will be saved to {}",
                    crate::paths::display(&crate::paths::projects_dir())
                )),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        ]
        .spacing(14)
        .max_width(620),
    )
    .padding(26)
    .style(theme::panel);

    let _ = app;

    container(card)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(24)
        .into()
}
