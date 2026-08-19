//! The channel rack: one row per instrument, with a step grid for quick beats.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Stroke};
use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::app::{FxHost, LimeDaw, Message, NewChannelKind, SynthPreset};
use crate::model::{DrumModel, Id, Pattern};
use crate::theme;
use crate::ui;

/// Height of one channel row, shared by the strip column and the step grid.
pub const ROW_HEIGHT: f32 = 34.0;
/// Width of one step cell.
const STEP_WIDTH: f32 = 26.0;
/// Height of the step ruler above the grid.
const RULER_HEIGHT: f32 = 18.0;

#[derive(Debug, Clone)]
pub enum Action {
    /// Paint a step. `first` marks the start of a drag, which is where the
    /// undo checkpoint belongs — the rest of the drag is one edit.
    Paint { channel: Id, step: usize, on: bool, first: bool },
    Seek(f64),
    SelectChannel(Id),
    Preview(Id),
}

/// Drag state for the step grid.
#[derive(Debug, Clone, Copy, Default)]
pub enum Interaction {
    #[default]
    Idle,
    /// Painting steps: `on` is what the first cell became.
    Painting {
        on: bool,
        last: (usize, usize),
    },
    Scrubbing,
}

struct StepGrid {
    channels: Vec<(Id, Color, bool)>,
    /// Velocity per (row, step), or `None` where there is no hit. Built once
    /// per frame so drawing never rescans the pattern for every cell.
    cells: Vec<Vec<Option<f32>>>,
    steps: usize,
    step_beats: f64,
    beats_per_bar: f64,
    position: f64,
    selected: Option<Id>,
}

impl StepGrid {
    fn step_of_x(&self, x: f32) -> Option<usize> {
        if x < 0.0 {
            return None;
        }
        let step = (x / STEP_WIDTH) as usize;
        (step < self.steps).then_some(step)
    }

    fn row_of_y(&self, y: f32) -> Option<usize> {
        if y < RULER_HEIGHT {
            return None;
        }
        let row = ((y - RULER_HEIGHT) / ROW_HEIGHT) as usize;
        (row < self.channels.len()).then_some(row)
    }

    fn step_on(&self, row: usize, step: usize) -> Option<f32> {
        self.cells.get(row).and_then(|r| r.get(step)).copied().flatten()
    }
}

impl canvas::Program<Action> for StepGrid {
    type State = Interaction;

    fn update(
        &self,
        interaction: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Action>> {
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(button))
                if matches!(button, mouse::Button::Left | mouse::Button::Right) =>
            {
                let point = cursor.position_in(bounds)?;
                if point.y < RULER_HEIGHT {
                    *interaction = Interaction::Scrubbing;
                    let step = self.step_of_x(point.x).unwrap_or(0);
                    return Some(
                        canvas::Action::publish(Action::Seek(step as f64 * self.step_beats))
                            .and_capture(),
                    );
                }
                let step = self.step_of_x(point.x)?;
                let row = self.row_of_y(point.y)?;
                let (channel, _, _) = self.channels[row];
                let currently_on = self.step_on(row, step).is_some();
                let turning_on = matches!(button, mouse::Button::Left) && !currently_on;
                *interaction = Interaction::Painting { on: turning_on, last: (row, step) };
                Some(
                    canvas::Action::publish(Action::Paint {
                        channel,
                        step,
                        on: turning_on,
                        first: true,
                    })
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let point = cursor.position_in(bounds)?;
                match interaction {
                    Interaction::Painting { on, last } => {
                        let step = self.step_of_x(point.x)?;
                        let row = self.row_of_y(point.y)?;
                        if (row, step) == *last {
                            return Some(canvas::Action::capture());
                        }
                        *last = (row, step);
                        let (channel, _, _) = self.channels[row];
                        Some(
                            canvas::Action::publish(Action::Paint {
                                channel,
                                step,
                                on: *on,
                                first: false,
                            })
                            .and_capture(),
                        )
                    }
                    Interaction::Scrubbing => {
                        let step = self.step_of_x(point.x).unwrap_or(0);
                        Some(
                            canvas::Action::publish(Action::Seek(step as f64 * self.step_beats))
                                .and_capture(),
                        )
                    }
                    Interaction::Idle => None,
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                if matches!(interaction, Interaction::Idle) {
                    return None;
                }
                *interaction = Interaction::Idle;
                Some(canvas::Action::capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _interaction: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = theme::palette(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), p.surface_low);

        let steps_per_bar = (self.beats_per_bar / self.step_beats).round().max(1.0) as usize;
        let hover = cursor.position_in(bounds);

        // Ruler.
        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(bounds.width, RULER_HEIGHT),
            p.surface_high,
        );
        for step in 0..self.steps {
            let x = step as f32 * STEP_WIDTH;
            if step % steps_per_bar == 0 {
                frame.fill_rectangle(
                    Point::new(x, 0.0),
                    Size::new(1.0, RULER_HEIGHT),
                    p.grid_bar,
                );
                frame.fill_text(ui::canvas_text(
                    format!("{}", step / steps_per_bar + 1),
                    Point::new(x + 3.0, 3.0),
                    p.text_dim,
                    10.0,
                ));
            }
        }

        // Cells.
        for (row, (channel, colour, audible)) in self.channels.iter().enumerate() {
            let _ = channel;
            let y = RULER_HEIGHT + row as f32 * ROW_HEIGHT;
            let selected_row = self.selected == Some(*channel);
            if selected_row {
                frame.fill_rectangle(
                    Point::new(0.0, y),
                    Size::new(bounds.width, ROW_HEIGHT),
                    theme::alpha(p.accent, 0.07),
                );
            }
            for step in 0..self.steps {
                let x = step as f32 * STEP_WIDTH;
                let beat_group = (step / (steps_per_bar / 4).max(1)) % 2 == 0;
                let cell = Rectangle {
                    x: x + 2.0,
                    y: y + 3.0,
                    width: STEP_WIDTH - 4.0,
                    height: ROW_HEIGHT - 6.0,
                };
                let base = if beat_group {
                    theme::lighten(p.surface_low, if p.is_dark { 0.05 } else { 0.02 })
                } else {
                    p.surface_low
                };
                let hovered = hover
                    .map(|h| {
                        h.x >= x && h.x < x + STEP_WIDTH && h.y >= y && h.y < y + ROW_HEIGHT
                    })
                    .unwrap_or(false);
                let fill = match self.step_on(row, step) {
                    Some(velocity) => {
                        let c = if *audible { *colour } else { p.text_dim };
                        theme::mix(theme::darken(c, 0.5), c, velocity.clamp(0.0, 1.0))
                    }
                    None if hovered => theme::mix(base, *colour, 0.28),
                    None => base,
                };
                frame.fill_rectangle(
                    Point::new(cell.x, cell.y),
                    Size::new(cell.width, cell.height),
                    fill,
                );
                if step % steps_per_bar == 0 {
                    frame.fill_rectangle(
                        Point::new(x, y),
                        Size::new(1.0, ROW_HEIGHT),
                        p.grid_bar,
                    );
                }
            }
        }

        // Playhead over the grid.
        let step_position = (self.position / self.step_beats) as f32;
        let x = step_position * STEP_WIDTH;
        if x >= 0.0 && x <= bounds.width {
            frame.fill_rectangle(
                Point::new(x - 1.0, 0.0),
                Size::new(2.0, bounds.height),
                p.playhead,
            );
        }

        frame.stroke_rectangle(
            Point::ORIGIN,
            bounds.size(),
            Stroke::default().with_color(p.border).with_width(1.0),
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _interaction: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Build the per-cell velocity grid the canvas draws from.
fn build_cells(pattern: &Pattern, channels: &[(Id, Color, bool)], steps: usize, step_beats: f64) -> Vec<Vec<Option<f32>>> {
    let mut cells = vec![vec![None; steps]; channels.len()];
    for note in &pattern.notes {
        let Some(row) = channels.iter().position(|(id, _, _)| *id == note.channel) else {
            continue;
        };
        // Only notes that sit exactly on a step show up in the rack; anything
        // off-grid belongs to the piano roll.
        let step = note.start / step_beats;
        if (step - step.round()).abs() > 1e-6 {
            continue;
        }
        let step = step.round();
        if step < 0.0 || step as usize >= steps {
            continue;
        }
        cells[row][step as usize] = Some(note.velocity);
    }
    cells
}

/// The whole channel-rack view.
pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let Some(pattern_id) = app.selected_pattern_id() else {
        return container(text("Create a pattern to start"))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
    };
    let Some(pattern) = app.project.pattern(pattern_id) else {
        return container(text("Create a pattern to start"))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into();
    };

    let step_beats = 0.25;
    let length = pattern.length_beats(app.project.beats_per_bar());
    let steps = ((length / step_beats).ceil() as usize).clamp(16, 256);

    let channels: Vec<(Id, Color, bool)> = app
        .project
        .channels
        .iter()
        .map(|c| (c.id, c.color.to_iced(), app.project.channel_audible(c.id)))
        .collect();

    let grid = StepGrid {
        cells: build_cells(pattern, &channels, steps, step_beats),
        channels: channels.clone(),
        steps,
        step_beats,
        beats_per_bar: app.project.beats_per_bar(),
        position: app.engine.status().position(),
        selected: app.selected_channel_id(),
    };

    let grid_canvas = Canvas::new(grid)
        .width(Length::Fixed(steps as f32 * STEP_WIDTH))
        .height(Length::Fixed(RULER_HEIGHT + channels.len() as f32 * ROW_HEIGHT));

    let strips = column(
        std::iter::once(
            container(text(""))
                .height(Length::Fixed(RULER_HEIGHT))
                .into(),
        )
        .chain(app.project.channels.iter().map(|c| channel_strip(app, c.id))),
    )
    .spacing(0)
    .width(Length::Fixed(268.0));

    let grid: Element<'_, Message> = Element::from(grid_canvas).map(Message::Steps);

    let body = scrollable(
        row![
            strips,
            scrollable(grid).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(6).scroller_width(6)
            ))
        ]
        .spacing(6),
    )
    .style(theme::scroll)
    .height(Length::Fill);

    column![rack_toolbar(app), body].spacing(6).into()
}

fn channel_strip(app: &LimeDaw, id: Id) -> Element<'_, Message> {
    let Some(channel) = app.project.channel(id) else {
        return container(text("")).into();
    };
    let selected = app.selected_channel_id() == Some(id);
    let colour = channel.color.to_iced();

    let swatch = container(text(""))
        .width(Length::Fixed(4.0))
        .height(Length::Fixed(ROW_HEIGHT - 8.0))
        .style(move |_: &Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(colour)),
            border: iced::Border { radius: iced::border::radius(2), ..Default::default() },
            ..Default::default()
        });

    let name = button(text(channel.name.clone()).size(12))
        .padding([2, 6])
        .width(Length::Fixed(104.0))
        .style(theme::button_ghost)
        .on_press(Message::SelectChannel(id));

    let mute = button(text("M").size(10))
        .padding([2, 6])
        .style({
            let on = channel.muted;
            move |theme: &Theme, status| {
                theme::button_toggle(theme, status, on, theme::palette(theme).danger)
            }
        })
        .on_press(Message::ToggleChannelMute(id));

    let solo = button(text("S").size(10))
        .padding([2, 6])
        .style({
            let on = channel.solo;
            move |theme: &Theme, status| {
                theme::button_toggle(theme, status, on, theme::palette(theme).warning)
            }
        })
        .on_press(Message::ToggleChannelSolo(id));

    let audition = button(text("▸").size(10))
        .padding([2, 6])
        .style(theme::button_plain)
        .on_press(Message::PreviewNote(id, channel.step_key));

    let volume = iced::widget::slider(0.0..=1.5, channel.volume, move |v| {
        Message::SetChannelVolume(id, v)
    })
    .step(0.01f32)
    .width(Length::Fixed(64.0))
    .style(theme::fader);

    let content = row![swatch, name, audition, mute, solo, volume]
        .spacing(4)
        .align_y(Alignment::Center);

    container(content)
        .height(Length::Fixed(ROW_HEIGHT))
        .padding([0, 4])
        .style(if selected { theme::selected } else { theme::transparent })
        .into()
}

fn rack_toolbar(app: &LimeDaw) -> Element<'_, Message> {
    let pattern_name = app
        .selected_pattern_id()
        .and_then(|id| app.project.pattern(id))
        .map(|p| p.name.clone())
        .unwrap_or_default();

    let rename = text_input("Pattern name", &pattern_name)
        .on_input(move |name| match app.selected_pattern_id() {
            Some(id) => Message::RenamePattern(id, name),
            None => Message::Noop,
        })
        .size(12)
        .padding([4, 8])
        .width(Length::Fixed(150.0))
        .style(theme::text_field);

    let add_menu = row![
        button(text("+ Synth").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::AddChannel(NewChannelKind::Synth(SynthPreset::Default))),
        button(text("+ Drum").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::AddChannel(NewChannelKind::Drum(DrumModel::Kick))),
        button(text("+ Sampler").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::AddChannel(NewChannelKind::EmptySampler)),
        button(text("+ SoundFont").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::AddChannel(NewChannelKind::SoundFont)),
    ]
    .spacing(4);

    let channel_actions = match app.selected_channel_id() {
        Some(id) => row![
            button(text("Clone").size(11))
                .padding([4, 8])
                .style(theme::button_plain)
                .on_press(Message::CloneChannel(id)),
            button(text("↑").size(11))
                .padding([4, 8])
                .style(theme::button_plain)
                .on_press(Message::MoveChannel(id, -1)),
            button(text("↓").size(11))
                .padding([4, 8])
                .style(theme::button_plain)
                .on_press(Message::MoveChannel(id, 1)),
            button(text("Remove").size(11))
                .padding([4, 8])
                .style(theme::button_danger)
                .on_press(Message::RemoveChannel(id)),
        ]
        .spacing(4),
        None => row![].spacing(4),
    };

    let patterns = row(app.project.patterns.iter().map(|pattern| {
        let active = app.selected_pattern_id() == Some(pattern.id);
        let id = pattern.id;
        button(text(pattern.name.clone()).size(11))
            .padding([3, 8])
            .style(move |theme: &Theme, status| theme::button_tab(theme, status, active))
            .on_press(Message::SelectPattern(id))
            .into()
    }))
    .spacing(3);

    let pattern_actions = match app.selected_pattern_id() {
        Some(id) => row![
            button(text("+ Pattern").size(11))
                .padding([4, 8])
                .style(theme::button_plain)
                .on_press(Message::AddPattern),
            button(text("Clone").size(11))
                .padding([4, 8])
                .style(theme::button_plain)
                .on_press(Message::ClonePattern(id)),
            button(text("Clear").size(11))
                .padding([4, 8])
                .style(theme::button_plain)
                .on_press(Message::ClearPattern(id)),
            button(text("Delete").size(11))
                .padding([4, 8])
                .style(theme::button_danger)
                .on_press(Message::RemovePattern(id)),
        ]
        .spacing(4),
        None => row![
            button(text("+ Pattern").size(11))
                .padding([4, 8])
                .style(theme::button_accent)
                .on_press(Message::AddPattern)
        ]
        .spacing(4),
    };

    let midi = row![
        button(text("Import MIDI").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::ImportMidi),
        button(text("Export MIDI").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::ExportMidi),
    ]
    .spacing(4);

    container(
        column![
            row![
                ui::caption("Pattern"),
                rename,
                scrollable(patterns)
                    .direction(scrollable::Direction::Horizontal(
                        scrollable::Scrollbar::new().width(4).scroller_width(4)
                    ))
                    .style(theme::scroll),
                iced::widget::space::horizontal(),
                pattern_actions,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![add_menu, iced::widget::space::horizontal(), midi, channel_actions]
                .spacing(8)
                .align_y(Alignment::Center),
        ]
        .spacing(5),
    )
    .padding([5, 8])
    .style(theme::header)
    .into()
}

/// Effects host for the selected channel, used by the instrument panel.
pub fn channel_fx_host(app: &LimeDaw) -> Option<FxHost> {
    app.selected_channel_id().map(FxHost::Channel)
}
