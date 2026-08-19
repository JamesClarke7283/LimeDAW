//! The playlist: arrange patterns into a song.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke};
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::app::{LimeDaw, Message};
use crate::model::{Clip, ClipSource, Id, Playlist as PlaylistModel};
use crate::theme;
use crate::ui;

const HEADER_WIDTH: f32 = 132.0;
const RULER_HEIGHT: f32 = 24.0;
const TRACK_HEIGHT: f32 = 40.0;
const RESIZE_GRIP: f32 = 7.0;

#[derive(Debug, Clone)]
pub struct State {
    pub scroll_beats: f64,
    pub scroll_track: f32,
    pub pixels_per_beat: f32,
    pub selection: Vec<Id>,
    /// Which pattern gets painted when drawing on an empty lane.
    pub paint_pattern: Option<Id>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            scroll_beats: 0.0,
            scroll_track: 0.0,
            pixels_per_beat: 22.0,
            selection: Vec::new(),
            paint_pattern: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Seek(f64),
    Scroll { beats: f64, track: f32 },
    Zoom(f32),
    Checkpoint,
    AddClip { track: usize, start: f64 },
    SetClip { id: Id, track: usize, start: f64 },
    SetClipLength { id: Id, length: f64 },
    Delete(Id),
    Select(Vec<Id>),
    /// Start dragging a clip: checkpoint, select it, and follow its pattern.
    BeginClipDrag { clip: Id, pattern: Option<Id> },
    SelectPattern(Id),
    ToggleTrackMute(usize),
    ToggleTrackSolo(usize),
    SetLoop { start: f64, end: f64 },
    ClearLoop,
}

#[derive(Debug, Clone, Default)]
pub enum Interaction {
    #[default]
    Idle,
    Moving {
        id: Id,
        anchor: Point,
        origin_start: f64,
        origin_track: usize,
    },
    Resizing {
        id: Id,
        anchor_x: f32,
        origin_length: f64,
    },
    Scrubbing,
    Panning {
        origin: Point,
        scroll: f64,
        track: f32,
    },
    LoopDrag {
        origin: f64,
    },
}

struct PlaylistCanvas<'a> {
    playlist: &'a PlaylistModel,
    pattern_names: Vec<(Id, String, Color)>,
    pattern_lengths: Vec<(Id, f64)>,
    state: &'a State,
    position: f64,
    beats_per_bar: f64,
    snap: f64,
}

impl PlaylistCanvas<'_> {
    fn x_of_beat(&self, beat: f64) -> f32 {
        HEADER_WIDTH + ((beat - self.state.scroll_beats) * self.state.pixels_per_beat as f64) as f32
    }

    fn beat_of_x(&self, x: f32) -> f64 {
        self.state.scroll_beats + ((x - HEADER_WIDTH) / self.state.pixels_per_beat.max(0.5)) as f64
    }

    fn y_of_track(&self, index: usize) -> f32 {
        RULER_HEIGHT + (index as f32 - self.state.scroll_track) * TRACK_HEIGHT
    }

    fn track_of_y(&self, y: f32) -> Option<usize> {
        if y < RULER_HEIGHT {
            return None;
        }
        let index = ((y - RULER_HEIGHT) / TRACK_HEIGHT + self.state.scroll_track).floor();
        if index < 0.0 {
            return None;
        }
        let index = index as usize;
        (index < self.playlist.tracks.len()).then_some(index)
    }

    fn snap_beat(&self, beat: f64) -> f64 {
        let snap = if self.snap <= 0.0 { 1.0 } else { self.snap.max(0.25) };
        ((beat / snap).round() * snap).max(0.0)
    }

    fn clip_at(&self, point: Point) -> Option<&Clip> {
        let track = self.track_of_y(point.y)?;
        let beat = self.beat_of_x(point.x);
        self.playlist
            .clips
            .iter()
            .rev()
            .find(|c| c.track == track && c.contains(beat))
    }

    fn pattern_length(&self, id: Id) -> f64 {
        self.pattern_lengths
            .iter()
            .find(|(pid, _)| *pid == id)
            .map(|(_, len)| *len)
            .unwrap_or(4.0)
    }
}

impl canvas::Program<Action> for PlaylistCanvas<'_> {
    type State = Interaction;

    fn update(
        &self,
        interaction: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Action>> {
        match event {
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let (dx, dy) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (*x, *y),
                    mouse::ScrollDelta::Pixels { x, y } => (x / 40.0, y / 40.0),
                };
                if dx.abs() > dy.abs() {
                    let beats = (self.state.scroll_beats - dx as f64 * 2.0).max(0.0);
                    return Some(
                        canvas::Action::publish(Action::Scroll {
                            beats,
                            track: self.state.scroll_track,
                        })
                        .and_capture(),
                    );
                }
                let max_track = self.playlist.tracks.len().saturating_sub(1) as f32;
                let track = (self.state.scroll_track - dy).clamp(0.0, max_track);
                Some(
                    canvas::Action::publish(Action::Scroll {
                        beats: self.state.scroll_beats,
                        track,
                    })
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) => {
                let point = cursor.position_in(bounds)?;
                *interaction = Interaction::Panning {
                    origin: point,
                    scroll: self.state.scroll_beats,
                    track: self.state.scroll_track,
                };
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let point = cursor.position_in(bounds)?;
                if point.y < RULER_HEIGHT {
                    return Some(canvas::Action::publish(Action::ClearLoop).and_capture());
                }
                let clip = self.clip_at(point)?;
                Some(canvas::Action::publish(Action::Delete(clip.id)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let point = cursor.position_in(bounds)?;

                if point.y < RULER_HEIGHT {
                    if point.x < HEADER_WIDTH {
                        return None;
                    }
                    let beat = self.snap_beat(self.beat_of_x(point.x));
                    *interaction = Interaction::LoopDrag { origin: beat };
                    return Some(canvas::Action::publish(Action::Seek(beat)).and_capture());
                }

                if point.x < HEADER_WIDTH {
                    let track = self.track_of_y(point.y)?;
                    // The right half of the header holds mute and solo.
                    if point.x > HEADER_WIDTH - 52.0 {
                        let action = if point.x > HEADER_WIDTH - 26.0 {
                            Action::ToggleTrackSolo(track)
                        } else {
                            Action::ToggleTrackMute(track)
                        };
                        return Some(canvas::Action::publish(action).and_capture());
                    }
                    return None;
                }

                if let Some(clip) = self.clip_at(point) {
                    let right = self.x_of_beat(clip.end());
                    if (point.x - right).abs() <= RESIZE_GRIP {
                        *interaction = Interaction::Resizing {
                            id: clip.id,
                            anchor_x: point.x,
                            origin_length: clip.length,
                        };
                        return Some(canvas::Action::publish(Action::Checkpoint).and_capture());
                    }
                    *interaction = Interaction::Moving {
                        id: clip.id,
                        anchor: point,
                        origin_start: clip.start,
                        origin_track: clip.track,
                    };
                    let id = clip.id;
                    let pattern = match clip.source {
                        ClipSource::Pattern(p) => Some(p),
                        ClipSource::Automation(_) => None,
                    };
                    return Some(
                        canvas::Action::publish(Action::BeginClipDrag { clip: id, pattern })
                            .and_capture(),
                    );
                }

                let track = self.track_of_y(point.y)?;
                let start = self.snap_beat(self.beat_of_x(point.x));
                Some(canvas::Action::publish(Action::AddClip { track, start }).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let point = cursor.position_in(bounds)?;
                match interaction {
                    Interaction::Idle => None,
                    Interaction::Panning { origin, scroll, track } => {
                        let beats = (*scroll
                            - ((point.x - origin.x) / self.state.pixels_per_beat.max(0.5)) as f64)
                            .max(0.0);
                        let max_track = self.playlist.tracks.len().saturating_sub(1) as f32;
                        let t = (*track - (point.y - origin.y) / TRACK_HEIGHT).clamp(0.0, max_track);
                        Some(
                            canvas::Action::publish(Action::Scroll { beats, track: t })
                                .and_capture(),
                        )
                    }
                    Interaction::Scrubbing => {
                        let beat = self.snap_beat(self.beat_of_x(point.x));
                        Some(canvas::Action::publish(Action::Seek(beat)).and_capture())
                    }
                    Interaction::LoopDrag { origin } => {
                        let beat = self.snap_beat(self.beat_of_x(point.x));
                        if (beat - *origin).abs() < 1e-6 {
                            return Some(canvas::Action::capture());
                        }
                        Some(
                            canvas::Action::publish(Action::SetLoop {
                                start: origin.min(beat),
                                end: origin.max(beat),
                            })
                            .and_capture(),
                        )
                    }
                    Interaction::Moving { id, anchor, origin_start, origin_track } => {
                        let beat_delta =
                            ((point.x - anchor.x) / self.state.pixels_per_beat.max(0.5)) as f64;
                        let start = self.snap_beat(*origin_start + beat_delta);
                        let track_delta = ((point.y - anchor.y) / TRACK_HEIGHT).round() as isize;
                        let track = (*origin_track as isize + track_delta)
                            .clamp(0, self.playlist.tracks.len() as isize - 1)
                            as usize;
                        Some(
                            canvas::Action::publish(Action::SetClip { id: *id, track, start })
                                .and_capture(),
                        )
                    }
                    Interaction::Resizing { id, anchor_x, origin_length } => {
                        let delta =
                            ((point.x - *anchor_x) / self.state.pixels_per_beat.max(0.5)) as f64;
                        let snap = if self.snap <= 0.0 { 1.0 } else { self.snap.max(0.25) };
                        let length = (((*origin_length + delta) / snap).round() * snap).max(snap);
                        Some(
                            canvas::Action::publish(Action::SetClipLength { id: *id, length })
                                .and_capture(),
                        )
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                if matches!(interaction, Interaction::Idle) {
                    return None;
                }
                *interaction = Interaction::Idle;
                Some(canvas::Action::request_redraw().and_capture())
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

        // Track lanes.
        let solo = self.playlist.any_solo();
        for (index, track) in self.playlist.tracks.iter().enumerate() {
            let y = self.y_of_track(index);
            if y + TRACK_HEIGHT < RULER_HEIGHT || y > size.height {
                continue;
            }
            let dim = track.muted || (solo && !track.solo);
            let lane = if index % 2 == 0 {
                p.surface_low
            } else {
                theme::lighten(p.surface_low, if p.is_dark { 0.035 } else { 0.02 })
            };
            frame.fill_rectangle(
                Point::new(HEADER_WIDTH, y),
                Size::new(size.width - HEADER_WIDTH, TRACK_HEIGHT),
                if dim { theme::alpha(lane, 0.5) } else { lane },
            );
            frame.fill_rectangle(
                Point::new(HEADER_WIDTH, y + TRACK_HEIGHT - 1.0),
                Size::new(size.width - HEADER_WIDTH, 1.0),
                p.border,
            );
        }

        // Grid.
        ui::draw_beat_grid(
            &mut frame,
            &p,
            size,
            HEADER_WIDTH,
            self.state.pixels_per_beat,
            self.state.scroll_beats,
            self.beats_per_bar,
            self.beats_per_bar,
        );

        // Loop region.
        if let Some((start, end)) = self.playlist.loop_region {
            let x0 = self.x_of_beat(start).max(HEADER_WIDTH);
            let x1 = self.x_of_beat(end).min(size.width);
            if x1 > x0 {
                frame.fill_rectangle(
                    Point::new(x0, 0.0),
                    Size::new(x1 - x0, size.height),
                    theme::alpha(p.accent, 0.07),
                );
                frame.fill_rectangle(
                    Point::new(x0, 0.0),
                    Size::new(x1 - x0, 3.0),
                    theme::alpha(p.accent, 0.8),
                );
            }
        }

        // Clips.
        for clip in &self.playlist.clips {
            let y = self.y_of_track(clip.track);
            if y + TRACK_HEIGHT < RULER_HEIGHT || y > size.height {
                continue;
            }
            let x = self.x_of_beat(clip.start);
            let width = (clip.length * self.state.pixels_per_beat as f64) as f32;
            if x + width < HEADER_WIDTH || x > size.width {
                continue;
            }
            let (name, colour) = match clip.source {
                ClipSource::Pattern(id) => self
                    .pattern_names
                    .iter()
                    .find(|(pid, _, _)| *pid == id)
                    .map(|(_, name, colour)| (name.clone(), *colour))
                    .unwrap_or_else(|| ("?".into(), p.text_dim)),
                ClipSource::Automation(_) => ("Automation".into(), p.warning),
            };
            let selected = self.state.selection.contains(&clip.id);
            let left = x.max(HEADER_WIDTH);
            let draw_width = (x + width - left).max(3.0);
            let fill = if clip.muted { theme::alpha(colour, 0.3) } else { colour };

            frame.fill_rectangle(
                Point::new(left, y + 3.0),
                Size::new(draw_width, TRACK_HEIGHT - 7.0),
                theme::alpha(fill, 0.85),
            );
            frame.stroke_rectangle(
                Point::new(left, y + 3.0),
                Size::new(draw_width, TRACK_HEIGHT - 7.0),
                Stroke::default()
                    .with_color(if selected { p.accent } else { theme::darken(fill, 0.35) })
                    .with_width(if selected { 2.0 } else { 1.0 }),
            );
            // Repeat markers show where the pattern loops inside a long clip.
            if let ClipSource::Pattern(id) = clip.source {
                let pattern_length = self.pattern_length(id);
                if pattern_length > 0.0 {
                    let mut repeat = pattern_length;
                    while repeat < clip.length {
                        let rx = self.x_of_beat(clip.start + repeat);
                        if rx > left && rx < left + draw_width {
                            frame.fill_rectangle(
                                Point::new(rx, y + 3.0),
                                Size::new(1.0, TRACK_HEIGHT - 7.0),
                                theme::alpha(theme::darken(fill, 0.5), 0.8),
                            );
                        }
                        repeat += pattern_length;
                    }
                }
            }
            if draw_width > 30.0 {
                frame.fill_text(ui::canvas_text(
                    name,
                    Point::new(left + 4.0, y + 7.0),
                    theme::contrasting_text(fill),
                    10.0,
                ));
            }
        }

        // Playhead.
        ui::draw_playhead(
            &mut frame,
            &p,
            size,
            HEADER_WIDTH,
            self.state.pixels_per_beat,
            self.state.scroll_beats,
            self.position,
        );

        // Track headers.
        frame.fill_rectangle(
            Point::new(0.0, RULER_HEIGHT),
            Size::new(HEADER_WIDTH, size.height - RULER_HEIGHT),
            p.surface,
        );
        for (index, track) in self.playlist.tracks.iter().enumerate() {
            let y = self.y_of_track(index);
            if y + TRACK_HEIGHT < RULER_HEIGHT || y > size.height {
                continue;
            }
            frame.fill_rectangle(
                Point::new(0.0, y + 4.0),
                Size::new(4.0, TRACK_HEIGHT - 9.0),
                track.color.to_iced(),
            );
            frame.fill_text(ui::canvas_text(
                track.name.clone(),
                Point::new(10.0, y + TRACK_HEIGHT / 2.0 - 6.0),
                p.text,
                10.0,
            ));
            // Mute and solo pads.
            let pad = |frame: &mut Frame, x: f32, on: bool, label: &str, tint: Color| {
                frame.fill_rectangle(
                    Point::new(x, y + 10.0),
                    Size::new(20.0, TRACK_HEIGHT - 21.0),
                    if on { tint } else { p.surface_low },
                );
                frame.fill_text(ui::canvas_text(
                    label,
                    Point::new(x + 6.0, y + 12.0),
                    if on { theme::contrasting_text(tint) } else { p.text_dim },
                    9.0,
                ));
            };
            pad(&mut frame, HEADER_WIDTH - 50.0, track.muted, "M", p.danger);
            pad(&mut frame, HEADER_WIDTH - 26.0, track.solo, "S", p.warning);
            frame.fill_rectangle(
                Point::new(0.0, y + TRACK_HEIGHT - 1.0),
                Size::new(HEADER_WIDTH, 1.0),
                p.border,
            );
        }
        frame.fill_rectangle(
            Point::new(HEADER_WIDTH - 1.0, RULER_HEIGHT),
            Size::new(1.0, size.height - RULER_HEIGHT),
            p.border,
        );

        // Ruler.
        frame.fill_rectangle(Point::ORIGIN, Size::new(size.width, RULER_HEIGHT), p.surface_high);
        let visible = ((size.width - HEADER_WIDTH) / self.state.pixels_per_beat.max(0.5)) as f64;
        let first_bar = (self.state.scroll_beats / self.beats_per_bar).floor() as i64;
        let last_bar = ((self.state.scroll_beats + visible) / self.beats_per_bar).ceil() as i64;
        // Only label bars that have room, so the ruler never turns to mush.
        let bar_pixels = self.beats_per_bar as f32 * self.state.pixels_per_beat;
        let label_every = if bar_pixels > 40.0 {
            1
        } else if bar_pixels > 14.0 {
            4
        } else {
            16
        };
        for bar in first_bar..=last_bar {
            let beat = bar as f64 * self.beats_per_bar;
            let x = self.x_of_beat(beat);
            if x < HEADER_WIDTH || x > size.width {
                continue;
            }
            frame.fill_rectangle(Point::new(x, 0.0), Size::new(1.0, RULER_HEIGHT), p.grid_bar);
            if bar % label_every == 0 {
                frame.fill_text(ui::canvas_text(
                    format!("{}", bar + 1),
                    Point::new(x + 3.0, 5.0),
                    p.text_dim,
                    10.0,
                ));
            }
        }
        frame.fill_rectangle(
            Point::new(0.0, RULER_HEIGHT - 1.0),
            Size::new(size.width, 1.0),
            p.border,
        );
        let playhead_x = self.x_of_beat(self.position);
        if playhead_x >= HEADER_WIDTH && playhead_x <= size.width {
            let marker = Path::new(|b| {
                b.move_to(Point::new(playhead_x - 5.0, 0.0));
                b.line_to(Point::new(playhead_x + 5.0, 0.0));
                b.line_to(Point::new(playhead_x, RULER_HEIGHT - 5.0));
                b.close();
            });
            frame.fill(&marker, p.playhead);
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        interaction: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match interaction {
            Interaction::Panning { .. } | Interaction::Moving { .. } => {
                return mouse::Interaction::Grabbing;
            }
            Interaction::Resizing { .. } => return mouse::Interaction::ResizingHorizontally,
            _ => {}
        }
        let Some(point) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        if point.y < RULER_HEIGHT {
            return mouse::Interaction::ResizingHorizontally;
        }
        if point.x < HEADER_WIDTH {
            return mouse::Interaction::Pointer;
        }
        if let Some(clip) = self.clip_at(point) {
            let right = self.x_of_beat(clip.end());
            if (point.x - right).abs() <= RESIZE_GRIP {
                return mouse::Interaction::ResizingHorizontally;
            }
            return mouse::Interaction::Grab;
        }
        mouse::Interaction::Crosshair
    }
}

pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let beats_per_bar = app.project.beats_per_bar();
    let program = PlaylistCanvas {
        playlist: &app.project.playlist,
        pattern_names: app
            .project
            .patterns
            .iter()
            .map(|p| (p.id, p.name.clone(), p.color.to_iced()))
            .collect(),
        pattern_lengths: app
            .project
            .patterns
            .iter()
            .map(|p| (p.id, p.length_beats(beats_per_bar)))
            .collect(),
        state: &app.playlist,
        position: app.engine.status().position(),
        beats_per_bar,
        snap: app.settings.grid_snap.max(0.25),
    };

    let canvas = Canvas::new(program).width(Length::Fill).height(Length::Fill);

    column![
        toolbar(app),
        container(Element::from(canvas).map(Message::PlaylistEdit))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::well),
    ]
    .spacing(4)
    .into()
}

fn toolbar(app: &LimeDaw) -> Element<'_, Message> {
    let patterns = row(app.project.patterns.iter().map(|pattern| {
        let active = app.playlist.paint_pattern.unwrap_or(app.selected_pattern.unwrap_or(0))
            == pattern.id;
        let id = pattern.id;
        button(text(pattern.name.clone()).size(11))
            .padding([4, 8])
            .style(move |theme: &Theme, status| theme::button_tab(theme, status, active))
            .on_press(Message::PlaylistEdit(Action::SelectPattern(id)))
            .into()
    }))
    .spacing(3);

    let zoom_out = button(text("−").size(13))
        .padding([2, 8])
        .style(theme::button_plain)
        .on_press(Message::PlaylistEdit(Action::Zoom(
            (app.playlist.pixels_per_beat / 1.35).max(3.0),
        )));
    let zoom_in = button(text("+").size(13))
        .padding([2, 8])
        .style(theme::button_plain)
        .on_press(Message::PlaylistEdit(Action::Zoom(
            (app.playlist.pixels_per_beat * 1.35).min(200.0),
        )));

    let loop_button = button(text("Loop region").size(11))
        .padding([4, 8])
        .style({
            let on = app.project.playlist.loop_region.is_some();
            move |theme: &Theme, status| theme::button_tab(theme, status, on)
        })
        .on_press(Message::ToggleLoop);

    container(
        row![
            ui::caption("Paint"),
            iced::widget::scrollable(patterns)
                .direction(iced::widget::scrollable::Direction::Horizontal(
                    iced::widget::scrollable::Scrollbar::new().width(4).scroller_width(4)
                ))
                .style(theme::scroll),
            iced::widget::space::horizontal(),
            loop_button,
            zoom_out,
            zoom_in,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .style(theme::header)
    .into()
}
