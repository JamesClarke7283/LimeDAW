//! The menu bar.
//!
//! iced has no native menus, so these are built from a row of title buttons
//! plus a panel pinned into a `stack` over the whole window. The panel has to
//! sit outside the normal layout — anything inside it would push the toolbar
//! down as it opened.

use iced::widget::{Space, button, column, container, mouse_area, pin, row, rule, text};
use iced::{Alignment, Element, Length, Theme};

use crate::app::{LimeDaw, MainView, Message};
use crate::theme;
use crate::ui;

/// Which menu is open, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuId {
    File,
    Edit,
    View,
    Transport,
    Help,
}

impl MenuId {
    pub const ALL: [MenuId; 5] = [
        MenuId::File,
        MenuId::Edit,
        MenuId::View,
        MenuId::Transport,
        MenuId::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MenuId::File => "File",
            MenuId::Edit => "Edit",
            MenuId::View => "View",
            MenuId::Transport => "Transport",
            MenuId::Help => "Help",
        }
    }
}

/// One row in a dropdown.
enum Entry<'a> {
    /// A label, its shortcut, and what it does. `None` means unavailable.
    Item {
        label: &'a str,
        shortcut: &'a str,
        message: Option<Message>,
    },
    /// A label with a tick, for things that are on or off.
    Toggle {
        label: &'a str,
        shortcut: &'a str,
        on: bool,
        message: Message,
    },
    Separator,
}

fn item<'a>(label: &'a str, shortcut: &'a str, message: Message) -> Entry<'a> {
    Entry::Item {
        label,
        shortcut,
        message: Some(message),
    }
}

fn disabled<'a>(label: &'a str, shortcut: &'a str) -> Entry<'a> {
    Entry::Item {
        label,
        shortcut,
        message: None,
    }
}

fn toggle<'a>(label: &'a str, shortcut: &'a str, on: bool, message: Message) -> Entry<'a> {
    Entry::Toggle {
        label,
        shortcut,
        on,
        message,
    }
}

/// Width of a menu title, fixed so the dropdown can be positioned under it
/// without measuring text.
const TITLE_WIDTH: f32 = 96.0;
/// Gap between titles.
const TITLE_GAP: f32 = 1.0;
/// Left padding inside the bar.
const BAR_PADDING: f32 = 4.0;
/// Height of the bar, and so the top of any dropdown.
pub const BAR_HEIGHT: f32 = 28.0;
/// Width of a dropdown panel.
const PANEL_WIDTH: f32 = 256.0;

/// The row of menu titles. The dropdown itself is drawn by [`overlay`].
pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let bar = row(MenuId::ALL.map(|id| {
        let open = app.open_menu == Some(id);
        button(text(id.label()).size(12).center())
            .padding([4, 10])
            .width(Length::Fixed(TITLE_WIDTH))
            .style(move |theme: &Theme, status| theme::button_tab(theme, status, open))
            .on_press(Message::ToggleMenu(id))
            .into()
    }))
    .spacing(TITLE_GAP);

    container(bar)
        .padding([2, BAR_PADDING as u16])
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT))
        .style(theme::header)
        .into()
}

/// The open dropdown, to be stacked over the rest of the window.
///
/// Returns nothing when no menu is open. A transparent catcher sits behind the
/// panel so that clicking anywhere else dismisses it, which is what every other
/// menu in the world does.
pub fn overlay(app: &LimeDaw) -> Option<Element<'_, Message>> {
    let id = app.open_menu?;
    let index = MenuId::ALL.iter().position(|m| *m == id)? as f32;

    let entries = entries(app, id);
    let panel = container(column(entries.iter().map(render)).spacing(0))
        .padding(4)
        .width(Length::Fixed(PANEL_WIDTH))
        .style(theme::menu_panel);

    let catcher = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
        .on_press(Message::CloseMenu);

    let x = BAR_PADDING + index * (TITLE_WIDTH + TITLE_GAP);

    Some(
        iced::widget::stack![catcher, pin(panel).x(x).y(BAR_HEIGHT),]
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    )
}

fn render<'a>(entry: &Entry<'a>) -> Element<'a, Message> {
    match entry {
        Entry::Separator => container(rule::horizontal(1).style(theme::separator))
            .padding([3, 6])
            .into(),
        Entry::Item {
            label,
            shortcut,
            message,
        } => {
            let body = row![
                text(*label).size(12),
                iced::widget::space::horizontal(),
                text(*shortcut).size(11).style(|theme: &Theme| text::Style {
                    color: Some(theme::palette(theme).text_dim)
                }),
            ]
            .spacing(24)
            .align_y(Alignment::Center);

            let mut control = button(body)
                .padding([5, 10])
                .width(Length::Fill)
                .style(theme::button_menu_item);
            if let Some(message) = message.clone() {
                control = control.on_press(message);
            }
            control.into()
        }
        Entry::Toggle {
            label,
            shortcut,
            on,
            message,
        } => {
            let body = row![
                // A fixed-width tick keeps the labels aligned whether or not
                // the item is on.
                container(text(if *on { "✓" } else { "" }).size(11)).width(Length::Fixed(14.0)),
                text(*label).size(12),
                iced::widget::space::horizontal(),
                text(*shortcut).size(11).style(|theme: &Theme| text::Style {
                    color: Some(theme::palette(theme).text_dim)
                }),
            ]
            .spacing(6)
            .align_y(Alignment::Center);

            button(body)
                .padding([5, 10])
                .width(Length::Fill)
                .style(theme::button_menu_item)
                .on_press(message.clone())
                .into()
        }
    }
}

fn entries(app: &LimeDaw, id: MenuId) -> Vec<Entry<'_>> {
    match id {
        MenuId::File => {
            let mut items = vec![
                item("New project", "Ctrl+N", Message::NewProject),
                item("New empty project", "", Message::NewEmptyProject),
                Entry::Separator,
                item("Open…", "Ctrl+O", Message::OpenProjectDialog),
            ];

            // Recent projects, listed inline rather than behind a submenu.
            if app.settings.recent_projects.is_empty() {
                items.push(disabled("No recent projects", ""));
            } else {
                for path in app.settings.recent_projects.iter().take(6) {
                    let name = std::path::Path::new(path)
                        .file_stem()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.clone());
                    items.push(Entry::Item {
                        label: Box::leak(name.into_boxed_str()),
                        shortcut: "",
                        message: Some(Message::OpenProjectPath(std::path::PathBuf::from(path))),
                    });
                }
            }

            items.extend([
                Entry::Separator,
                item("Save", "Ctrl+S", Message::SaveProject),
                item("Save as…", "Ctrl+Shift+S", Message::SaveProjectAs),
                Entry::Separator,
                item("Import MIDI…", "", Message::ImportMidi),
                item("Export MIDI", "", Message::ExportMidi),
                item("Export audio…", "Ctrl+E", Message::ExportWav),
                Entry::Separator,
                item("Open project folder", "", Message::OpenProjectFolder),
                Entry::Separator,
                item("Quit", "Ctrl+Q", Message::Quit),
            ]);
            items
        }

        MenuId::Edit => vec![
            item("Undo", "Ctrl+Z", Message::Undo),
            item("Redo", "Ctrl+Shift+Z", Message::Redo),
            Entry::Separator,
            item("Copy notes", "Ctrl+C", Message::CopyNotes),
            item("Paste notes", "Ctrl+V", Message::PasteNotes),
            item("Duplicate notes", "Ctrl+D", Message::DuplicateNotes),
            item("Delete selection", "Delete", Message::DeleteSelection),
            Entry::Separator,
            item("Quantise", "Ctrl+Q", Message::QuantizeSelection),
            Entry::Separator,
            item("Settings", "", Message::SetView(MainView::Settings)),
        ],

        MenuId::View => vec![
            item("Channel rack", "F5", Message::SetView(MainView::Channels)),
            item("Piano roll", "F6", Message::SetView(MainView::PianoRoll)),
            item("Song editor", "F7", Message::SetView(MainView::Playlist)),
            item("Mixer", "F9", Message::SetView(MainView::Mixer)),
            Entry::Separator,
            toggle(
                "Browser",
                "",
                app.settings.show_browser,
                Message::ToggleBrowser,
            ),
            toggle(
                "Instrument panel",
                "",
                app.settings.show_instrument_panel,
                Message::ToggleInstrumentPanel,
            ),
            toggle(
                "Effects rack",
                "",
                app.settings.show_effects_dock,
                Message::ToggleEffectsDock,
            ),
            toggle(
                "Automation",
                "",
                app.automation.visible,
                Message::Automation(ui::automation::Action::Toggle),
            ),
            Entry::Separator,
            item("MCP server", "", Message::SetView(MainView::Mcp)),
        ],

        MenuId::Transport => vec![
            item(
                if app.engine.status().playing() {
                    "Pause"
                } else {
                    "Play"
                },
                "Space",
                Message::TogglePlay,
            ),
            item("Stop", "", Message::Stop),
            toggle("Record", "Ctrl+R", app.recording, Message::ToggleRecord),
            Entry::Separator,
            toggle(
                "Metronome",
                "",
                app.settings.metronome,
                Message::ToggleMetronome,
            ),
            toggle(
                "Loop region",
                "",
                app.project.playlist.loop_region.is_some(),
                Message::ToggleLoop,
            ),
            Entry::Separator,
            item("Stop all sound", "Esc", Message::Panic),
        ],

        MenuId::Help => vec![
            item(
                "Keyboard shortcuts",
                "",
                Message::SetView(MainView::Settings),
            ),
            item("Where files live", "", Message::SetView(MainView::Settings)),
            Entry::Separator,
            item("MCP setup", "", Message::SetView(MainView::Mcp)),
            Entry::Separator,
            disabled(concat!("LimeDAW ", env!("CARGO_PKG_VERSION")), ""),
        ],
    }
}
