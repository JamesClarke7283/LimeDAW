//! The MCP panel: server status and a ready-made client configuration to copy.

use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length};

use crate::app::{LimeDaw, Message};
use crate::mcp;
use crate::theme;
use crate::ui;

pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let socket = socket_path();
    let exe = executable_path();
    let config = mcp::client_config_json(&exe, &socket);
    let socat = mcp::client_config_socat(&socket);
    let command = mcp::cli_command(&exe, &socket);

    let running = app.mcp.is_some();
    let status_text = if running {
        format!("Listening on {socket}")
    } else if let Some(error) = &app.mcp_error {
        format!("Not running — {error}")
    } else {
        "Not running".to_string()
    };

    let status = container(
        row![
            container(text("").size(1))
                .width(Length::Fixed(10.0))
                .height(Length::Fixed(10.0))
                .style(move |theme: &iced::Theme| {
                    let p = theme::palette(theme);
                    iced::widget::container::Style {
                        background: Some(iced::Background::Color(if running {
                            p.success
                        } else {
                            p.danger
                        })),
                        border: iced::Border {
                            radius: iced::border::radius(5),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                }),
            text(status_text).size(12),
            iced::widget::space::horizontal(),
            button(
                text(if running {
                    "Stop server"
                } else {
                    "Start server"
                })
                .size(12)
            )
            .padding([5, 10])
            .style(if running {
                theme::button_danger
            } else {
                theme::button_accent
            })
            .on_press(Message::ToggleMcpServer),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(12)
    .width(Length::Fill)
    .style(theme::card);

    let tools = mcp::tool_definitions();
    let tool_list = column(tools.iter().map(|tool| {
        row![
            container(text(tool.name).size(11)).width(Length::Fixed(190.0)),
            text(tool.description)
                .size(11)
                .style(|theme: &iced::Theme| text::Style {
                    color: Some(theme::palette(theme).text_dim)
                }),
        ]
        .spacing(8)
        .into()
    }))
    .spacing(3);

    column![
        text("Model Context Protocol").size(16),
        ui::caption(
            "LimeDAW serves MCP over a unix socket, so an assistant can drive the DAW while it is open."
        ),
        status,
        config_box(
            "Client configuration",
            "Paste this into your MCP client's config (claude_desktop_config.json, .mcp.json, and friends).",
            config,
        ),
        config_box(
            "Alternative: socat bridge",
            "If you would rather not point at the LimeDAW binary, bridge the socket with socat instead.",
            socat,
        ),
        config_box(
            "Claude Code, one line",
            "Adds the server to Claude Code from a terminal.",
            command,
        ),
        container(
            column![
                text(format!("{} tools", tools.len())).size(14),
                ui::caption("Everything the interface can do is reachable from here."),
                container(scrollable(tool_list).style(theme::scroll).height(Length::Fixed(280.0)))
                    .padding(8)
                    .style(theme::well),
            ]
            .spacing(8)
        )
        .padding(12)
        .width(Length::Fill)
        .style(theme::card),
    ]
    .spacing(12)
    .padding(14)
    .max_width(900)
    .into()
}

/// A copyable block of configuration text.
fn config_box<'a>(title: &'a str, description: &'a str, content: String) -> Element<'a, Message> {
    let to_copy = content.clone();
    container(
        column![
            row![
                column![text(title).size(13), ui::caption(description)].spacing(2),
                iced::widget::space::horizontal(),
                button(text("Copy").size(12))
                    .padding([5, 12])
                    .style(theme::button_accent)
                    .on_press(Message::CopyText(to_copy)),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            container(
                scrollable(text(content).size(11).font(iced::Font::MONOSPACE))
                    .direction(iced::widget::scrollable::Direction::Both {
                        vertical: iced::widget::scrollable::Scrollbar::new()
                            .width(5)
                            .scroller_width(5),
                        horizontal: iced::widget::scrollable::Scrollbar::new()
                            .width(5)
                            .scroller_width(5),
                    })
                    .style(theme::scroll)
                    .height(Length::Shrink)
            )
            .padding(10)
            .width(Length::Fill)
            .style(theme::well),
        ]
        .spacing(8),
    )
    .padding(12)
    .width(Length::Fill)
    .style(theme::card)
    .into()
}

fn socket_path() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        mcp::server::socket_path().to_string_lossy().to_string()
    }
    #[cfg(target_arch = "wasm32")]
    {
        crate::paths::mcp_socket().to_string_lossy().to_string()
    }
}

fn executable_path() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "limedaw".into())
    }
    #[cfg(target_arch = "wasm32")]
    {
        "limedaw".into()
    }
}
