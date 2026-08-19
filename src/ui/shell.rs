//! The window layout: transport on top, browser at the left, editor in the middle.

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length};

use crate::app::{LimeDaw, MainView, Message};
use crate::theme;
use crate::ui;

pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let main: Element<'_, Message> = match app.view {
        MainView::Channels => ui::channel_rack::view(app),
        MainView::PianoRoll => piano_roll_area(app),
        MainView::Playlist => ui::playlist::view(app),
        MainView::Mixer => ui::mixer::view(app),
        MainView::Settings => ui::settings::view(app),
        MainView::Mcp => ui::mcp_panel::view(app),
    };

    let centre = container(main)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(4);

    let mut body = row![].spacing(6);
    if app.settings.show_browser {
        body = body.push(ui::browser::view(app));
    }
    body = body.push(centre);
    if app.settings.show_instrument_panel && wants_instrument_panel(app.view) {
        body = body.push(
            container(ui::instrument::view(app))
                .width(Length::Fixed(392.0))
                .height(Length::Fill)
                .style(theme::panel),
        );
    }

    let mut layout = column![
        ui::menu::view(app),
        ui::transport::view(app),
        container(body)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding([0, 6]),
    ]
    .spacing(4);

    // The effects rack lives along the bottom, the way LMMS keeps its chain
    // windows out of the arrangement.
    if app.settings.show_effects_dock {
        layout = layout.push(
            container(ui::effects::dock(app))
                .height(Length::Fixed(app.settings.effects_dock_height))
                .padding([0, 6]),
        );
    }

    let window = layout.push(status_bar(app)).spacing(4).padding([6, 0]);

    // An open menu is stacked over the whole window rather than placed in the
    // layout: anything inside the column would push the toolbar down as it
    // opened, which is what `float` did.
    match ui::menu::overlay(app) {
        Some(menu) => iced::widget::stack![window, menu]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        None => window.into(),
    }
}

fn wants_instrument_panel(view: MainView) -> bool {
    matches!(
        view,
        MainView::Channels | MainView::PianoRoll | MainView::Playlist
    )
}

fn piano_roll_area(app: &LimeDaw) -> Element<'_, Message> {
    if app.automation.visible {
        column![
            container(ui::piano_roll::view(app)).height(Length::FillPortion(3)),
            container(ui::automation::view(app)).height(Length::Shrink),
        ]
        .spacing(4)
        .into()
    } else {
        ui::piano_roll::view(app)
    }
}

fn status_bar(app: &LimeDaw) -> Element<'_, Message> {
    let project = if app.project.name.is_empty() {
        "Untitled".to_string()
    } else {
        app.project.name.clone()
    };
    let saved = match &app.project_path {
        Some(path) => crate::paths::display(path),
        None => "not saved yet".into(),
    };

    let mcp_state = if app.mcp.is_some() {
        "MCP on"
    } else {
        "MCP off"
    };

    container(
        row![
            button(text("Browser").size(10))
                .padding([2, 7])
                .style({
                    let on = app.settings.show_browser;
                    move |theme: &iced::Theme, s| theme::button_tab(theme, s, on)
                })
                .on_press(Message::ToggleBrowser),
            button(text("Instrument").size(10))
                .padding([2, 7])
                .style({
                    let on = app.settings.show_instrument_panel;
                    move |theme: &iced::Theme, s| theme::button_tab(theme, s, on)
                })
                .on_press(Message::ToggleInstrumentPanel),
            button(text("Effects").size(10))
                .padding([2, 7])
                .style({
                    let on = app.settings.show_effects_dock;
                    move |theme: &iced::Theme, s| theme::button_tab(theme, s, on)
                })
                .on_press(Message::ToggleEffectsDock),
            button(text("Automation").size(10))
                .padding([2, 7])
                .style({
                    let on = app.automation.visible;
                    move |theme: &iced::Theme, s| theme::button_tab(theme, s, on)
                })
                .on_press(Message::Automation(ui::automation::Action::Toggle)),
            iced::widget::rule::vertical(1).style(theme::separator),
            ui::caption(format!("{project}{}", if app.dirty { " •" } else { "" })),
            ui::caption(saved),
            iced::widget::space::horizontal(),
            {
                // A render runs on a worker, so say so rather than appearing
                // to have locked up.
                let bar: Element<'_, Message> = if app.render_progress.is_running() {
                    let fraction = app.render_progress.fraction();
                    iced::widget::row![
                        ui::caption(format!("Rendering {:.0}%", fraction * 100.0)),
                        iced::widget::progress_bar(0.0..=1.0, fraction)
                            .length(Length::Fixed(120.0))
                            .girth(Length::Fixed(6.0))
                            .style(theme::progress),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center)
                    .into()
                } else {
                    ui::caption("")
                };
                bar
            },
            ui::caption(app.status.clone()),
            {
                let label: Element<'_, Message> = if app.recording {
                    text("REC")
                        .size(10)
                        .style(|theme: &iced::Theme| iced::widget::text::Style {
                            color: Some(theme::palette(theme).danger),
                        })
                        .into()
                } else {
                    ui::caption("")
                };
                label
            },
            iced::widget::rule::vertical(1).style(theme::separator),
            ui::caption(mcp_state),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .padding([3, 10]),
    )
    .width(Length::Fill)
    .height(Length::Fixed(26.0))
    .style(theme::header)
    .into()
}
