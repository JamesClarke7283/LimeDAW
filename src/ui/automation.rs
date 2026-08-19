//! The automation editor: draw parameter curves over a pattern's timeline.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{button, column, container, pick_list, row, text};
use iced::{Alignment, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::app::{LimeDaw, Message};
use crate::model::{AutomationLane, AutomationTarget, Curve, Id, ParamRef};
use crate::theme;
use crate::ui;

const RULER_HEIGHT: f32 = 16.0;
/// How close a click must be to grab an existing point.
const GRAB_RADIUS: f32 = 8.0;

#[derive(Debug, Clone)]
pub struct State {
    pub visible: bool,
    /// Index of the lane being edited within the pattern.
    pub lane: usize,
    pub height: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            visible: false,
            lane: 0,
            height: 140.0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Toggle,
    SelectLane(usize),
    AddLane(AutomationTarget),
    RemoveLane(usize),
    Checkpoint,
    AddPoint {
        beat: f64,
        value: f64,
    },
    MovePoint {
        index: usize,
        beat: f64,
        value: f64,
    },
    RemovePoint(usize),
    SetCurve {
        index: usize,
        curve: Curve,
    },
    /// Re-sort the lane once a drag ends; sorting mid-drag would move the
    /// point out from under the index the gesture is holding.
    SortLane,
    Seek(f64),
}

#[derive(Debug, Clone, Copy, Default)]
pub enum Interaction {
    #[default]
    Idle,
    Dragging(usize),
    Scrubbing,
}

struct LaneCanvas<'a> {
    lane: Option<&'a AutomationLane>,
    scroll_beats: f64,
    pixels_per_beat: f32,
    beats_per_bar: f64,
    snap: f64,
    length: f64,
    position: f64,
}

impl LaneCanvas<'_> {
    fn x_of_beat(&self, beat: f64) -> f32 {
        ((beat - self.scroll_beats) * self.pixels_per_beat as f64) as f32
    }

    fn beat_of_x(&self, x: f32) -> f64 {
        (self.scroll_beats + (x / self.pixels_per_beat.max(1.0)) as f64).max(0.0)
    }

    fn y_of_value(&self, value: f64, height: f32) -> f32 {
        let usable = (height - RULER_HEIGHT - 8.0).max(1.0);
        RULER_HEIGHT + 4.0 + (1.0 - value.clamp(0.0, 1.0) as f32) * usable
    }

    fn value_of_y(&self, y: f32, height: f32) -> f64 {
        let usable = (height - RULER_HEIGHT - 8.0).max(1.0);
        (1.0 - ((y - RULER_HEIGHT - 4.0) / usable)).clamp(0.0, 1.0) as f64
    }

    fn snap_beat(&self, beat: f64) -> f64 {
        if self.snap <= 0.0 {
            beat.max(0.0)
        } else {
            ((beat / self.snap).round() * self.snap).max(0.0)
        }
    }

    fn point_at(&self, point: Point, height: f32) -> Option<usize> {
        let lane = self.lane?;
        lane.points.iter().position(|p| {
            let x = self.x_of_beat(p.beat);
            let y = self.y_of_value(p.value, height);
            (x - point.x).abs() <= GRAB_RADIUS && (y - point.y).abs() <= GRAB_RADIUS
        })
    }
}

impl canvas::Program<Action> for LaneCanvas<'_> {
    type State = Interaction;

    fn update(
        &self,
        interaction: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Action>> {
        let height = bounds.height;
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let point = cursor.position_in(bounds)?;
                let index = self.point_at(point, height)?;
                Some(canvas::Action::publish(Action::RemovePoint(index)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let point = cursor.position_in(bounds)?;
                if point.y < RULER_HEIGHT {
                    *interaction = Interaction::Scrubbing;
                    let beat = self.snap_beat(self.beat_of_x(point.x));
                    return Some(canvas::Action::publish(Action::Seek(beat)).and_capture());
                }
                self.lane?;
                if let Some(index) = self.point_at(point, height) {
                    *interaction = Interaction::Dragging(index);
                    return Some(canvas::Action::publish(Action::Checkpoint).and_capture());
                }
                let beat = self.snap_beat(self.beat_of_x(point.x));
                let value = self.value_of_y(point.y, height);
                Some(canvas::Action::publish(Action::AddPoint { beat, value }).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let point = cursor.position_in(bounds)?;
                match interaction {
                    Interaction::Idle => None,
                    Interaction::Scrubbing => {
                        let beat = self.snap_beat(self.beat_of_x(point.x));
                        Some(canvas::Action::publish(Action::Seek(beat)).and_capture())
                    }
                    Interaction::Dragging(index) => {
                        let beat = self.snap_beat(self.beat_of_x(point.x));
                        let value = self.value_of_y(point.y, height);
                        Some(
                            canvas::Action::publish(Action::MovePoint {
                                index: *index,
                                beat,
                                value,
                            })
                            .and_capture(),
                        )
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                let was_dragging = matches!(interaction, Interaction::Dragging(_));
                if matches!(interaction, Interaction::Idle) {
                    return None;
                }
                *interaction = Interaction::Idle;
                if was_dragging {
                    return Some(canvas::Action::publish(Action::SortLane).and_capture());
                }
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
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = theme::palette(theme);
        let size = bounds.size();
        let mut frame = Frame::new(renderer, size);
        frame.fill_rectangle(Point::ORIGIN, size, p.surface_low);

        ui::draw_beat_grid(
            &mut frame,
            &p,
            size,
            0.0,
            self.pixels_per_beat,
            self.scroll_beats,
            self.beats_per_bar,
            self.snap,
        );

        let end_x = self.x_of_beat(self.length);
        if end_x < size.width && end_x > 0.0 {
            frame.fill_rectangle(
                Point::new(end_x, 0.0),
                Size::new(size.width - end_x, size.height),
                theme::alpha(p.base, 0.35),
            );
        }

        // Half-way guide.
        let mid = self.y_of_value(0.5, size.height);
        frame.fill_rectangle(
            Point::new(0.0, mid),
            Size::new(size.width, 1.0),
            theme::alpha(p.text_dim, 0.25),
        );

        if let Some(lane) = self.lane {
            if !lane.points.is_empty() {
                // Sample the lane so curved segments actually look curved.
                let curve = Path::new(|b| {
                    let mut started = false;
                    let steps = (size.width as usize).clamp(2, 1200);
                    for i in 0..=steps {
                        let x = i as f32 * size.width / steps as f32;
                        let beat = self.beat_of_x(x);
                        let value = lane.value_at(beat).unwrap_or(0.0);
                        let y = self.y_of_value(value, size.height);
                        if started {
                            b.line_to(Point::new(x, y));
                        } else {
                            b.move_to(Point::new(x, y));
                            started = true;
                        }
                    }
                });
                frame.stroke(
                    &curve,
                    Stroke::default().with_color(p.accent).with_width(2.0),
                );

                for point in &lane.points {
                    let x = self.x_of_beat(point.beat);
                    if x < -8.0 || x > size.width + 8.0 {
                        continue;
                    }
                    let y = self.y_of_value(point.value, size.height);
                    frame.fill(&Path::circle(Point::new(x, y), 4.0), p.accent);
                    frame.stroke(
                        &Path::circle(Point::new(x, y), 4.0),
                        Stroke::default().with_color(p.surface).with_width(1.0),
                    );
                }
            }
        } else {
            frame.fill_text(ui::canvas_text(
                "Add a lane to start automating",
                Point::new(10.0, size.height / 2.0 - 6.0),
                p.text_dim,
                11.0,
            ));
        }

        ui::draw_playhead(
            &mut frame,
            &p,
            size,
            0.0,
            self.pixels_per_beat,
            self.scroll_beats,
            self.position,
        );

        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(size.width, RULER_HEIGHT),
            theme::alpha(p.surface_high, 0.9),
        );
        frame.fill_rectangle(
            Point::new(0.0, RULER_HEIGHT),
            Size::new(size.width, 1.0),
            p.border,
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _interaction: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let Some(point) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        if self.point_at(point, bounds.height).is_some() {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::Crosshair
        }
    }
}

/// The automation panel shown under the piano roll.
pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let Some(pattern_id) = app.selected_pattern_id() else {
        return container(text("")).into();
    };
    let Some(pattern) = app.project.pattern(pattern_id) else {
        return container(text("")).into();
    };

    let lane = pattern.automation.get(app.automation.lane);
    let beats_per_bar = app.project.beats_per_bar();

    let canvas = Canvas::new(LaneCanvas {
        lane,
        scroll_beats: app.piano_roll.scroll_beats,
        pixels_per_beat: app.piano_roll.pixels_per_beat,
        beats_per_bar,
        snap: app.settings.grid_snap,
        length: pattern.length_beats(beats_per_bar),
        position: app.engine.status().position(),
    })
    .width(Length::Fill)
    .height(Length::Fixed(app.automation.height));

    let lane_buttons = row(pattern.automation.iter().enumerate().map(|(index, lane)| {
        let active = index == app.automation.lane;
        button(text(lane_label(app, lane)).size(11))
            .padding([3, 8])
            .style(move |theme: &Theme, status| theme::button_tab(theme, status, active))
            .on_press(Message::Automation(Action::SelectLane(index)))
            .into()
    }))
    .spacing(3);

    let targets = available_targets(app);
    let target_labels: Vec<String> = targets.iter().map(|(label, _)| label.clone()).collect();
    let add = pick_list(target_labels, None::<String>, {
        let targets = targets.clone();
        move |label: String| {
            let target = targets
                .iter()
                .find(|(l, _)| *l == label)
                .map(|(_, t)| *t)
                .unwrap_or(AutomationTarget::MasterVolume);
            Message::Automation(Action::AddLane(target))
        }
    })
    .placeholder("Add lane…")
    .text_size(11)
    .padding([3, 8])
    .style(theme::picker);

    let remove: Element<'_, Message> = if pattern.automation.is_empty() {
        container(text("")).into()
    } else {
        button(text("Remove lane").size(11))
            .padding([3, 8])
            .style(theme::button_danger)
            .on_press(Message::Automation(Action::RemoveLane(app.automation.lane)))
            .into()
    };

    column![
        container(
            row![
                ui::caption("Automation"),
                lane_buttons,
                iced::widget::space::horizontal(),
                add,
                remove
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        )
        .padding([3, 8])
        .style(theme::header),
        Element::from(canvas).map(Message::Automation),
    ]
    .spacing(2)
    .into()
}

fn lane_label(app: &LimeDaw, lane: &AutomationLane) -> String {
    match lane.target {
        AutomationTarget::Tempo => "Tempo".into(),
        AutomationTarget::MasterVolume => "Master".into(),
        AutomationTarget::ChannelVolume(id) => {
            format!("{} vol", channel_name(app, id))
        }
        AutomationTarget::ChannelPan(id) => format!("{} pan", channel_name(app, id)),
        AutomationTarget::MixerVolume(t) => format!("{} vol", mixer_name(app, t)),
        AutomationTarget::MixerPan(t) => format!("{} pan", mixer_name(app, t)),
        AutomationTarget::MixerEffect { track, slot, param } => {
            format!("{} fx{slot} {param}", mixer_name(app, track))
        }
        AutomationTarget::ChannelEffect {
            channel,
            slot,
            param,
        } => {
            format!("{} fx{slot} {param}", channel_name(app, channel))
        }
        AutomationTarget::Instrument { channel, param } => {
            format!("{} {param}", channel_name(app, channel))
        }
    }
}

fn channel_name(app: &LimeDaw, id: Id) -> String {
    app.project
        .channel(id)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| format!("Ch {id}"))
}

fn mixer_name(app: &LimeDaw, track: usize) -> String {
    app.project
        .mixer
        .tracks
        .get(track)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| format!("Insert {track}"))
}

/// Everything that can be automated in this project, for the "add lane" menu.
pub fn available_targets(app: &LimeDaw) -> Vec<(String, AutomationTarget)> {
    let mut out = vec![
        ("Tempo".to_string(), AutomationTarget::Tempo),
        ("Master volume".to_string(), AutomationTarget::MasterVolume),
    ];
    for channel in &app.project.channels {
        out.push((
            format!("{} volume", channel.name),
            AutomationTarget::ChannelVolume(channel.id),
        ));
        out.push((
            format!("{} pan", channel.name),
            AutomationTarget::ChannelPan(channel.id),
        ));
        for param in channel.instrument.params() {
            out.push((
                format!("{} · {}", channel.name, param.label),
                AutomationTarget::Instrument {
                    channel: channel.id,
                    param: ParamRef::new(param.name),
                },
            ));
        }
        for (slot, effect) in channel.effects.iter().enumerate() {
            for (name, _, _, _) in effect.kind.params() {
                out.push((
                    format!("{} · {} {}", channel.name, effect.name(), name),
                    AutomationTarget::ChannelEffect {
                        channel: channel.id,
                        slot,
                        param: ParamRef::new(name),
                    },
                ));
            }
        }
    }
    for (index, track) in app.project.mixer.tracks.iter().enumerate() {
        out.push((
            format!("{} volume", track.name),
            AutomationTarget::MixerVolume(index),
        ));
        out.push((
            format!("{} pan", track.name),
            AutomationTarget::MixerPan(index),
        ));
        for (slot, effect) in track.effects.iter().enumerate() {
            for (name, _, _, _) in effect.kind.params() {
                out.push((
                    format!("{} · {} {}", track.name, effect.name(), name),
                    AutomationTarget::MixerEffect {
                        track: index,
                        slot,
                        param: ParamRef::new(name),
                    },
                ));
            }
        }
    }
    out
}
