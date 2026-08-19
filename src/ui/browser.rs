//! The browser panel: instruments, presets, samples and projects.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length, Theme};

use crate::app::{BrowserTab, LimeDaw, Message, NewChannelKind, SynthPreset};
use crate::model::DrumModel;
use crate::theme;
use crate::ui;

pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let tabs = row(BrowserTab::ALL.map(|tab| {
        let active = app.browser_tab == tab;
        button(text(tab.label()).size(11))
            .padding([4, 7])
            .style(move |theme: &Theme, s| theme::button_tab(theme, s, active))
            .on_press(Message::SetBrowserTab(tab))
            .into()
    }))
    .spacing(2);

    let body = match app.browser_tab {
        BrowserTab::Instruments => instruments(),
        BrowserTab::Presets => presets(app),
        BrowserTab::Samples => samples(app),
        BrowserTab::Projects => projects(app),
    };

    container(
        column![
            container(tabs).padding([4, 4]),
            scrollable(container(body).padding(6))
                .style(theme::scroll)
                .height(Length::Fill),
        ]
        .spacing(2),
    )
    .width(Length::Fixed(216.0))
    .height(Length::Fill)
    .style(theme::panel)
    .into()
}

fn entry<'a>(label: String, message: Message) -> Element<'a, Message> {
    button(text(label).size(12))
        .padding([4, 8])
        .width(Length::Fill)
        .style(theme::button_ghost)
        .on_press(message)
        .into()
}

fn section<'a>(title: &'a str, items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    column![ui::caption(title), column(items).spacing(1)]
        .spacing(3)
        .into()
}

fn instruments<'a>() -> Element<'a, Message> {
    column![
        section(
            "Synths",
            SynthPreset::ALL
                .map(|preset| entry(
                    preset.label().to_string(),
                    Message::AddChannel(NewChannelKind::Synth(preset))
                ))
                .into_iter()
                .collect()
        ),
        section(
            "Drums",
            DrumModel::ALL
                .map(|model| entry(
                    model.label().to_string(),
                    Message::AddChannel(NewChannelKind::Drum(model))
                ))
                .into_iter()
                .collect()
        ),
        section(
            "Other",
            vec![
                entry("Sampler".into(), Message::AddChannel(NewChannelKind::EmptySampler)),
                entry("SoundFont".into(), Message::AddChannel(NewChannelKind::SoundFont)),
            ]
        ),
    ]
    .spacing(12)
    .into()
}

fn presets(app: &LimeDaw) -> Element<'_, Message> {
    if app.soundfont_presets.is_empty() {
        return column![
            ui::caption("No SoundFont loaded"),
            button(text("Choose SoundFont…").size(11))
                .padding([4, 8])
                .style(theme::button_accent)
                .on_press(Message::ChooseSoundFontDialog),
        ]
        .spacing(8)
        .into();
    }
    let items: Vec<Element<'_, Message>> = app
        .soundfont_presets
        .iter()
        .take(300)
        .map(|preset| {
            let (bank, program) = (preset.bank, preset.program);
            let name = preset.name.clone();
            button(text(preset.name.clone()).size(11))
                .padding([3, 6])
                .width(Length::Fill)
                .style(theme::button_ghost)
                .on_press(Message::AddSoundFontChannel { bank, program, name })
                .into()
        })
        .collect();
    section("SoundFont presets", items)
}

fn samples(app: &LimeDaw) -> Element<'_, Message> {
    let built_in: Vec<Element<'_, Message>> = DrumModel::ALL
        .map(|model| entry(model.label().to_string(), Message::AddChannel(NewChannelKind::Drum(model))))
        .into_iter()
        .collect();

    let files = crate::paths::discover_samples();
    let file_items: Vec<Element<'_, Message>> = if files.is_empty() {
        vec![ui::caption(format!(
            "Drop .wav files into {}",
            crate::paths::display(&crate::paths::samples_dir())
        ))]
    } else {
        files
            .iter()
            .take(200)
            .map(|path| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                // A dot marks samples already decoded into the engine's bank.
                let loaded = app.bank.contains(&path.to_string_lossy().to_string());
                let label = if loaded { format!("• {name}") } else { name };
                let path = path.clone();
                button(text(label).size(11))
                    .padding([3, 6])
                    .width(Length::Fill)
                    .style(theme::button_ghost)
                    .on_press(Message::AddSampleChannel(path))
                    .into()
            })
            .collect()
    };

    column![section("Built-in drums", built_in), section("Sample folder", file_items)]
        .spacing(12)
        .into()
}

fn projects(app: &LimeDaw) -> Element<'_, Message> {
    let files = crate::paths::discover_projects();
    let items: Vec<Element<'_, Message>> = if files.is_empty() {
        vec![ui::caption(format!(
            "Projects are saved in {}",
            crate::paths::display(&crate::paths::projects_dir())
        ))]
    } else {
        files
            .iter()
            .map(|path| {
                let label = path
                    .file_stem()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let path = path.clone();
                button(text(label).size(11))
                    .padding([3, 6])
                    .width(Length::Fill)
                    .style(theme::button_ghost)
                    .on_press(Message::OpenProjectPath(path))
                    .into()
            })
            .collect()
    };

    let recent: Vec<Element<'_, Message>> = app
        .settings
        .recent_projects
        .iter()
        .take(8)
        .map(|path| {
            let p = std::path::PathBuf::from(path);
            let label = p
                .file_stem()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            button(text(label).size(11))
                .padding([3, 6])
                .width(Length::Fill)
                .style(theme::button_ghost)
                .on_press(Message::OpenProjectPath(p))
                .into()
        })
        .collect();

    column![
        row![
            button(text("New").size(11))
                .padding([4, 8])
                .style(theme::button_plain)
                .on_press(Message::NewProject),
            button(text("Open…").size(11))
                .padding([4, 8])
                .style(theme::button_plain)
                .on_press(Message::OpenProjectDialog),
            button(text("Save").size(11))
                .padding([4, 8])
                .style(theme::button_accent)
                .on_press(Message::SaveProject),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
        section("LimeProjects", items),
        section("Recent", recent),
    ]
    .spacing(12)
    .into()
}
