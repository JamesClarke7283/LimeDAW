//! The piano roll: draw, drag, resize and edit notes on a scrolling grid.

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::widget::{button, column, container, pick_list, row, text};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::app::{LimeDaw, Message};
use crate::model::{Id, Note, Pattern};
use crate::theme;
use crate::ui;

/// Width of the keyboard gutter.
const KEY_WIDTH: f32 = 62.0;
/// Height of the bar ruler.
const RULER_HEIGHT: f32 = 22.0;
/// Height of the velocity lane when it is showing.
const VELOCITY_HEIGHT: f32 = 78.0;
/// How close to a note's right edge counts as grabbing the resize handle.
const RESIZE_GRIP: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Draw,
    Select,
    Erase,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Draw, Tool::Select, Tool::Erase];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Draw => "Draw",
            Tool::Select => "Select",
            Tool::Erase => "Erase",
        }
    }
}

/// Persistent view state, owned by the application.
#[derive(Debug, Clone)]
pub struct State {
    pub scroll_beats: f64,
    /// The MIDI key drawn at the top of the grid.
    pub top_key: f32,
    pub pixels_per_beat: f32,
    pub key_height: f32,
    pub selection: Vec<Id>,
    pub show_velocity: bool,
    pub show_ghosts: bool,
    pub tool: Tool,
    /// The note most recently created by dragging, so the drag can size it.
    pub last_created: Option<Id>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            scroll_beats: 0.0,
            top_key: 84.0,
            pixels_per_beat: 96.0,
            key_height: 13.0,
            selection: Vec::new(),
            show_velocity: true,
            show_ghosts: true,
            tool: Tool::Draw,
            last_created: None,
        }
    }
}

/// What the canvas asks the application to do.
#[derive(Debug, Clone)]
pub enum Action {
    Seek(f64),
    Scroll { beats: f64, top_key: f32 },
    Zoom { pixels_per_beat: f32, key_height: f32 },
    Select(Vec<Id>),
    /// Start a drag: take an undo checkpoint and set the selection in one step.
    BeginDrag(Vec<Id>),
    Checkpoint,
    AddNote { key: u8, start: f64, length: f64 },
    DragNewNote { end: f64 },
    SetPositions(Vec<(Id, u8, f64)>),
    SetLength(Vec<(Id, f64)>),
    Delete(Vec<Id>),
    SetVelocity(Vec<(Id, f32)>),
    Preview(u8),
    Release(u8),
    SetTool(Tool),
    ToggleVelocity,
    ToggleGhosts,
}

/// A drag in progress.
#[derive(Debug, Clone, Default)]
pub enum Interaction {
    #[default]
    Idle,
    /// Moving the selection.
    Moving {
        anchor: Point,
        origins: Vec<(Id, u8, f64)>,
        last_key: u8,
    },
    /// Dragging a note's right edge.
    Resizing {
        anchor_x: f32,
        origins: Vec<(Id, f64)>,
    },
    /// Sizing a note that was just drawn.
    Drawing {
        start: f64,
        key: u8,
    },
    /// Rubber-band selection.
    Selecting {
        origin: Point,
        current: Point,
    },
    /// Middle-drag panning.
    Panning {
        origin: Point,
        scroll: f64,
        top_key: f32,
    },
    /// Dragging in the velocity lane.
    Velocity {
        last: Option<Id>,
    },
    /// Scrubbing the ruler.
    Scrubbing,
    /// Erasing by dragging across notes.
    Erasing,
    /// Holding a key in the keyboard gutter.
    Auditioning,
}

/// The canvas program.
pub struct PianoRoll<'a> {
    pattern: &'a Pattern,
    channel: Id,
    channel_color: Color,
    other_colors: Vec<(Id, Color)>,
    state: &'a State,
    position: f64,
    beats_per_bar: f64,
    snap: f64,
    length: f64,
}

impl<'a> PianoRoll<'a> {
    fn x_of_beat(&self, beat: f64) -> f32 {
        KEY_WIDTH + ((beat - self.state.scroll_beats) * self.state.pixels_per_beat as f64) as f32
    }

    fn beat_of_x(&self, x: f32) -> f64 {
        self.state.scroll_beats + ((x - KEY_WIDTH) / self.state.pixels_per_beat.max(1.0)) as f64
    }

    /// Top edge of a key's lane. Lanes run downward from here, so
    /// `key_of_y(y_of_key(k)) == k` and clicks land on the key they look like.
    fn y_of_key(&self, key: f32) -> f32 {
        RULER_HEIGHT + (self.state.top_key - key) * self.state.key_height
    }

    fn key_of_y(&self, y: f32) -> f32 {
        self.state.top_key - (y - RULER_HEIGHT) / self.state.key_height.max(1.0)
    }

    fn snap_beat(&self, beat: f64) -> f64 {
        if self.snap <= 0.0 {
            beat.max(0.0)
        } else {
            ((beat / self.snap).round() * self.snap).max(0.0)
        }
    }

    fn grid_height(&self, bounds: Size) -> f32 {
        let lane = if self.state.show_velocity { VELOCITY_HEIGHT } else { 0.0 };
        (bounds.height - lane).max(RULER_HEIGHT + 10.0)
    }

    /// The note under a point, preferring the topmost drawn one.
    fn note_at(&self, point: Point, bounds: Size) -> Option<&Note> {
        if point.x < KEY_WIDTH || point.y < RULER_HEIGHT || point.y > self.grid_height(bounds) {
            return None;
        }
        let key = self.key_of_y(point.y).floor().clamp(0.0, 127.0) as i32;
        let beat = self.beat_of_x(point.x);
        self.pattern
            .notes
            .iter()
            .rev()
            .find(|n| n.channel == self.channel && n.key as i32 == key && n.contains(beat))
    }

    fn is_over_resize(&self, note: &Note, point: Point) -> bool {
        let right = self.x_of_beat(note.end());
        (point.x - right).abs() <= RESIZE_GRIP && point.x <= right + 2.0
    }

    fn notes_in_rect(&self, a: Point, b: Point) -> Vec<Id> {
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        let beat0 = self.beat_of_x(x0);
        let beat1 = self.beat_of_x(x1);
        let key_hi = self.key_of_y(y0).floor();
        let key_lo = self.key_of_y(y1).floor();
        self.pattern
            .notes
            .iter()
            .filter(|n| {
                n.channel == self.channel
                    && n.end() > beat0
                    && n.start < beat1
                    && (n.key as f32) <= key_hi
                    && (n.key as f32) >= key_lo
            })
            .map(|n| n.id)
            .collect()
    }

    /// The velocity lane's vertical extent.
    fn velocity_rect(&self, bounds: Size) -> Rectangle {
        let top = self.grid_height(bounds);
        Rectangle {
            x: KEY_WIDTH,
            y: top,
            width: bounds.width - KEY_WIDTH,
            height: (bounds.height - top).max(0.0),
        }
    }
}

impl canvas::Program<Action> for PianoRoll<'_> {
    type State = Interaction;

    fn update(
        &self,
        interaction: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Action>> {
        let size = bounds.size();
        let position = cursor.position_in(bounds);

        match event {
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                position?;
                let (dx, dy) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (*x, *y),
                    mouse::ScrollDelta::Pixels { x, y } => (x / 40.0, y / 40.0),
                };
                // Vertical wheel scrolls the keyboard, shift scrolls in time.
                if dx.abs() > dy.abs() {
                    let beats = (self.state.scroll_beats - dx as f64 * 0.5).max(0.0);
                    return Some(
                        canvas::Action::publish(Action::Scroll {
                            beats,
                            top_key: self.state.top_key,
                        })
                        .and_capture(),
                    );
                }
                let top_key = (self.state.top_key + dy * 2.0).clamp(12.0, 127.0);
                Some(
                    canvas::Action::publish(Action::Scroll {
                        beats: self.state.scroll_beats,
                        top_key,
                    })
                    .and_capture(),
                )
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) => {
                let point = position?;
                *interaction = Interaction::Panning {
                    origin: point,
                    scroll: self.state.scroll_beats,
                    top_key: self.state.top_key,
                };
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let point = position?;
                if let Some(note) = self.note_at(point, size) {
                    let id = note.id;
                    *interaction = Interaction::Erasing;
                    return Some(canvas::Action::publish(Action::Delete(vec![id])).and_capture());
                }
                None
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let point = position?;

                // Ruler: scrub the playhead.
                if point.y < RULER_HEIGHT && point.x >= KEY_WIDTH {
                    *interaction = Interaction::Scrubbing;
                    let beat = self.snap_beat(self.beat_of_x(point.x));
                    return Some(canvas::Action::publish(Action::Seek(beat)).and_capture());
                }

                // Keyboard gutter: audition the key for as long as it is held.
                if point.x < KEY_WIDTH && point.y >= RULER_HEIGHT && point.y < self.grid_height(size) {
                    let key = self.key_of_y(point.y).floor().clamp(0.0, 127.0) as u8;
                    *interaction = Interaction::Auditioning;
                    return Some(canvas::Action::publish(Action::Preview(key)).and_capture());
                }

                // Velocity lane.
                let lane = self.velocity_rect(size);
                if self.state.show_velocity && point.y >= lane.y && point.x >= KEY_WIDTH {
                    *interaction = Interaction::Velocity { last: None };
                    let beat = self.beat_of_x(point.x);
                    let value = 1.0 - ((point.y - lane.y) / lane.height.max(1.0)).clamp(0.0, 1.0);
                    let hits: Vec<(Id, f32)> = self
                        .pattern
                        .notes
                        .iter()
                        .filter(|n| n.channel == self.channel && n.contains(beat))
                        .map(|n| (n.id, value))
                        .collect();
                    if hits.is_empty() {
                        return Some(canvas::Action::capture());
                    }
                    return Some(
                        canvas::Action::publish(Action::SetVelocity(hits)).and_capture(),
                    );
                }

                if point.y < RULER_HEIGHT || point.x < KEY_WIDTH {
                    return None;
                }

                match self.state.tool {
                    Tool::Erase => {
                        *interaction = Interaction::Erasing;
                        if let Some(note) = self.note_at(point, size) {
                            let id = note.id;
                            return Some(
                                canvas::Action::publish(Action::Delete(vec![id])).and_capture(),
                            );
                        }
                        Some(canvas::Action::capture())
                    }
                    Tool::Select => {
                        if let Some(note) = self.note_at(point, size) {
                            return Some(start_move(self, interaction, note, point));
                        }
                        *interaction = Interaction::Selecting { origin: point, current: point };
                        Some(canvas::Action::publish(Action::Select(Vec::new())).and_capture())
                    }
                    Tool::Draw => {
                        if let Some(note) = self.note_at(point, size) {
                            if self.is_over_resize(note, point) {
                                let ids: Vec<Id> = if self.state.selection.contains(&note.id) {
                                    self.state.selection.clone()
                                } else {
                                    vec![note.id]
                                };
                                let origins = self
                                    .pattern
                                    .notes
                                    .iter()
                                    .filter(|n| ids.contains(&n.id))
                                    .map(|n| (n.id, n.length))
                                    .collect();
                                *interaction =
                                    Interaction::Resizing { anchor_x: point.x, origins };
                                return Some(
                                    canvas::Action::publish(Action::Checkpoint).and_capture(),
                                );
                            }
                            return Some(start_move(self, interaction, note, point));
                        }
                        // Empty space: draw a new note and let the drag size it.
                        let key = self.key_of_y(point.y).floor().clamp(0.0, 127.0) as u8;
                        let start = self.snap_beat(self.beat_of_x(point.x));
                        let length = if self.snap > 0.0 { self.snap } else { 0.25 };
                        *interaction = Interaction::Drawing { start, key };
                        Some(
                            canvas::Action::publish(Action::AddNote { key, start, length })
                                .and_capture(),
                        )
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position: cursor_position }) => {
                let point = cursor
                    .position_in(bounds)
                    .unwrap_or(Point::new(cursor_position.x - bounds.x, cursor_position.y - bounds.y));
                match interaction {
                    Interaction::Idle => None,
                    Interaction::Panning { origin, scroll, top_key } => {
                        let beats = (*scroll
                            - ((point.x - origin.x) / self.state.pixels_per_beat.max(1.0)) as f64)
                            .max(0.0);
                        let keys =
                            (*top_key - (point.y - origin.y) / self.state.key_height.max(1.0))
                                .clamp(12.0, 127.0);
                        Some(
                            canvas::Action::publish(Action::Scroll { beats, top_key: keys })
                                .and_capture(),
                        )
                    }
                    Interaction::Scrubbing => {
                        let beat = self.snap_beat(self.beat_of_x(point.x));
                        Some(canvas::Action::publish(Action::Seek(beat)).and_capture())
                    }
                    Interaction::Auditioning => {
                        // Slide along the keyboard to hear neighbouring keys.
                        if point.x < KEY_WIDTH && point.y >= RULER_HEIGHT {
                            let key = self.key_of_y(point.y).floor().clamp(0.0, 127.0) as u8;
                            return Some(
                                canvas::Action::publish(Action::Preview(key)).and_capture(),
                            );
                        }
                        Some(canvas::Action::capture())
                    }
                    Interaction::Erasing => {
                        if let Some(note) = self.note_at(point, size) {
                            let id = note.id;
                            return Some(
                                canvas::Action::publish(Action::Delete(vec![id])).and_capture(),
                            );
                        }
                        Some(canvas::Action::capture())
                    }
                    Interaction::Drawing { start, .. } => {
                        let end = self.snap_beat(self.beat_of_x(point.x)).max(*start + self.snap.max(1.0 / 32.0));
                        Some(canvas::Action::publish(Action::DragNewNote { end }).and_capture())
                    }
                    Interaction::Resizing { anchor_x, origins } => {
                        let delta =
                            ((point.x - *anchor_x) / self.state.pixels_per_beat.max(1.0)) as f64;
                        let min = self.snap.max(1.0 / 32.0);
                        let updates: Vec<(Id, f64)> = origins
                            .iter()
                            .map(|(id, length)| {
                                let raw = length + delta;
                                let snapped = if self.snap > 0.0 {
                                    (raw / self.snap).round() * self.snap
                                } else {
                                    raw
                                };
                                (*id, snapped.max(min))
                            })
                            .collect();
                        Some(canvas::Action::publish(Action::SetLength(updates)).and_capture())
                    }
                    Interaction::Moving { anchor, origins, last_key } => {
                        let beat_delta =
                            ((point.x - anchor.x) / self.state.pixels_per_beat.max(1.0)) as f64;
                        let key_delta =
                            ((anchor.y - point.y) / self.state.key_height.max(1.0)).round() as i32;
                        let mut updates = Vec::with_capacity(origins.len());
                        let mut top_key = *last_key;
                        for (id, key, start) in origins.iter() {
                            let new_key = (*key as i32 + key_delta).clamp(0, 127) as u8;
                            let raw = start + beat_delta;
                            let new_start = if self.snap > 0.0 {
                                (raw / self.snap).round() * self.snap
                            } else {
                                raw
                            };
                            updates.push((*id, new_key, new_start.max(0.0)));
                            top_key = new_key;
                        }
                        *last_key = top_key;
                        // The application auditions the new pitch when it sees
                        // a key change, so the move itself is never dropped.
                        Some(
                            canvas::Action::publish(Action::SetPositions(updates)).and_capture(),
                        )
                    }
                    Interaction::Selecting { origin, current } => {
                        *current = point;
                        let ids = self.notes_in_rect(*origin, point);
                        Some(canvas::Action::publish(Action::Select(ids)).and_capture())
                    }
                    Interaction::Velocity { last } => {
                        let lane = self.velocity_rect(size);
                        let beat = self.beat_of_x(point.x);
                        let value =
                            1.0 - ((point.y - lane.y) / lane.height.max(1.0)).clamp(0.0, 1.0);
                        let hits: Vec<(Id, f32)> = self
                            .pattern
                            .notes
                            .iter()
                            .filter(|n| n.channel == self.channel && n.contains(beat))
                            .map(|n| (n.id, value))
                            .collect();
                        if let Some(first) = hits.first() {
                            *last = Some(first.0);
                            return Some(
                                canvas::Action::publish(Action::SetVelocity(hits)).and_capture(),
                            );
                        }
                        Some(canvas::Action::capture())
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(_)) => {
                let was_active = !matches!(interaction, Interaction::Idle);
                let previewing = matches!(
                    interaction,
                    Interaction::Moving { .. }
                        | Interaction::Drawing { .. }
                        | Interaction::Auditioning
                );
                *interaction = Interaction::Idle;
                if previewing {
                    // Release anything the drag auditioned.
                    return Some(
                        canvas::Action::publish(Action::Release(0)).and_capture(),
                    );
                }
                if was_active {
                    Some(canvas::Action::request_redraw().and_capture())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        interaction: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = theme::palette(theme);
        let size = bounds.size();
        let mut frame = Frame::new(renderer, size);
        let grid_bottom = self.grid_height(size);

        frame.fill_rectangle(Point::ORIGIN, size, p.surface_low);

        // ---- key lanes ----
        let first_key = self.key_of_y(grid_bottom).floor().max(0.0) as i32;
        let last_key = self.key_of_y(RULER_HEIGHT).floor().min(127.0) as i32;
        for key in first_key..=last_key {
            let y = self.y_of_key(key as f32);
            if y > grid_bottom || y + self.state.key_height < RULER_HEIGHT {
                continue;
            }
            let black = ui::is_black_key(key as u8);
            let lane = if black { p.lane_black } else { p.lane_white };
            let top = y.max(RULER_HEIGHT);
            let height = (y + self.state.key_height).min(grid_bottom) - top;
            if height <= 0.0 {
                continue;
            }
            frame.fill_rectangle(
                Point::new(KEY_WIDTH, top),
                Size::new(size.width - KEY_WIDTH, height),
                lane,
            );
            // A brighter line every octave keeps the eye oriented.
            if key % 12 == 0 {
                frame.fill_rectangle(
                    Point::new(KEY_WIDTH, top),
                    Size::new(size.width - KEY_WIDTH, 1.0),
                    p.grid_bar,
                );
            }
        }

        // ---- vertical grid ----
        ui::draw_beat_grid(
            &mut frame,
            &p,
            Size::new(size.width, grid_bottom),
            KEY_WIDTH,
            self.state.pixels_per_beat,
            self.state.scroll_beats,
            self.beats_per_bar,
            self.snap,
        );

        // ---- beyond the pattern's end ----
        let end_x = self.x_of_beat(self.length);
        if end_x < size.width && end_x > KEY_WIDTH {
            frame.fill_rectangle(
                Point::new(end_x, RULER_HEIGHT),
                Size::new(size.width - end_x, grid_bottom - RULER_HEIGHT),
                theme::alpha(p.base, 0.35),
            );
        }

        // ---- ghost notes from other channels ----
        if self.state.show_ghosts {
            for note in &self.pattern.notes {
                if note.channel == self.channel {
                    continue;
                }
                let colour = self
                    .other_colors
                    .iter()
                    .find(|(id, _)| *id == note.channel)
                    .map(|(_, c)| *c)
                    .unwrap_or(p.text_dim);
                draw_note(&mut frame, self, note, theme::alpha(colour, 0.22), false, grid_bottom, &p);
            }
        }

        // ---- this channel's notes ----
        for note in &self.pattern.notes {
            if note.channel != self.channel {
                continue;
            }
            let selected = self.state.selection.contains(&note.id);
            draw_note(&mut frame, self, note, self.channel_color, selected, grid_bottom, &p);
        }

        // ---- rubber band ----
        if let Interaction::Selecting { origin, current } = interaction {
            let x = origin.x.min(current.x);
            let y = origin.y.min(current.y);
            let w = (origin.x - current.x).abs();
            let h = (origin.y - current.y).abs();
            frame.fill_rectangle(Point::new(x, y), Size::new(w, h), theme::alpha(p.accent, 0.15));
            frame.stroke_rectangle(
                Point::new(x, y),
                Size::new(w, h),
                Stroke::default().with_color(p.accent).with_width(1.0),
            );
        }

        // ---- playhead ----
        ui::draw_playhead(
            &mut frame,
            &p,
            Size::new(size.width, grid_bottom),
            KEY_WIDTH,
            self.state.pixels_per_beat,
            self.state.scroll_beats,
            self.position,
        );

        // ---- velocity lane ----
        if self.state.show_velocity {
            let lane = self.velocity_rect(size);
            frame.fill_rectangle(
                Point::new(0.0, lane.y),
                Size::new(size.width, lane.height),
                p.surface,
            );
            frame.fill_rectangle(
                Point::new(0.0, lane.y),
                Size::new(size.width, 1.0),
                p.border,
            );
            frame.fill_text(ui::canvas_text(
                "Velocity",
                Point::new(6.0, lane.y + 4.0),
                p.text_dim,
                10.0,
            ));
            for note in &self.pattern.notes {
                if note.channel != self.channel {
                    continue;
                }
                let x = self.x_of_beat(note.start);
                if x < KEY_WIDTH - 4.0 || x > size.width {
                    continue;
                }
                let h = lane.height * note.velocity.clamp(0.0, 1.0);
                let selected = self.state.selection.contains(&note.id);
                let colour = if selected { p.accent } else { self.channel_color };
                frame.fill_rectangle(
                    Point::new(x, lane.y + lane.height - h),
                    Size::new(3.0, h),
                    colour,
                );
                frame.fill(
                    &Path::circle(Point::new(x + 1.5, lane.y + lane.height - h), 3.0),
                    colour,
                );
            }
        }

        // ---- keyboard gutter ----
        frame.fill_rectangle(
            Point::new(0.0, RULER_HEIGHT),
            Size::new(KEY_WIDTH, grid_bottom - RULER_HEIGHT),
            p.surface,
        );
        for key in first_key..=last_key {
            let y = self.y_of_key(key as f32);
            if y + self.state.key_height < RULER_HEIGHT || y > grid_bottom {
                continue;
            }
            let top = y.max(RULER_HEIGHT);
            let height = (y + self.state.key_height).min(grid_bottom) - top;
            if height <= 0.5 {
                continue;
            }
            let black = ui::is_black_key(key as u8);
            let fill = if black {
                if p.is_dark { theme::darken(p.surface, 0.5) } else { theme::darken(p.surface, 0.75) }
            } else if p.is_dark {
                theme::lighten(p.surface, 0.10)
            } else {
                Color::WHITE
            };
            let width = if black { KEY_WIDTH * 0.62 } else { KEY_WIDTH };
            frame.fill_rectangle(Point::new(0.0, top), Size::new(width, height - 1.0), fill);
            if !black && key % 12 == 0 && height > 8.0 {
                frame.fill_text(ui::canvas_text(
                    ui::note_name(key as u8),
                    Point::new(KEY_WIDTH - 26.0, top + (height - 9.0) * 0.5),
                    p.text_dim,
                    9.0,
                ));
            }
        }
        frame.fill_rectangle(
            Point::new(KEY_WIDTH - 1.0, RULER_HEIGHT),
            Size::new(1.0, grid_bottom - RULER_HEIGHT),
            p.border,
        );

        // ---- ruler ----
        frame.fill_rectangle(Point::ORIGIN, Size::new(size.width, RULER_HEIGHT), p.surface_high);
        frame.fill_rectangle(
            Point::new(0.0, RULER_HEIGHT - 1.0),
            Size::new(size.width, 1.0),
            p.border,
        );
        let visible = ((size.width - KEY_WIDTH) / self.state.pixels_per_beat.max(1.0)) as f64;
        let first_bar = (self.state.scroll_beats / self.beats_per_bar).floor() as i64;
        let last_bar = ((self.state.scroll_beats + visible) / self.beats_per_bar).ceil() as i64;
        for bar in first_bar..=last_bar {
            let beat = bar as f64 * self.beats_per_bar;
            let x = self.x_of_beat(beat);
            if x < KEY_WIDTH || x > size.width {
                continue;
            }
            frame.fill_rectangle(Point::new(x, 0.0), Size::new(1.0, RULER_HEIGHT), p.grid_bar);
            frame.fill_text(ui::canvas_text(
                format!("{}", bar + 1),
                Point::new(x + 3.0, 4.0),
                p.text_dim,
                10.0,
            ));
        }
        let playhead_x = self.x_of_beat(self.position);
        if playhead_x >= KEY_WIDTH && playhead_x <= size.width {
            let marker = Path::new(|b| {
                b.move_to(Point::new(playhead_x - 5.0, 0.0));
                b.line_to(Point::new(playhead_x + 5.0, 0.0));
                b.line_to(Point::new(playhead_x, RULER_HEIGHT - 4.0));
                b.close();
            });
            frame.fill(&marker, p.playhead);
        }

        // ---- hover readout ----
        if let Some(point) = cursor.position_in(bounds) {
            if point.x > KEY_WIDTH && point.y > RULER_HEIGHT && point.y < grid_bottom {
                let key = self.key_of_y(point.y).floor().clamp(0.0, 127.0) as u8;
                let beat = self.beat_of_x(point.x);
                let label = format!("{}  {}", ui::note_name(key), ui::format_position(beat.max(0.0), self.beats_per_bar));
                frame.fill_text(Text {
                    content: label,
                    position: Point::new(size.width - 8.0, RULER_HEIGHT + 4.0),
                    color: p.text_dim,
                    size: 10.0.into(),
                    align_x: iced::alignment::Horizontal::Right.into(),
                    align_y: iced::alignment::Vertical::Top,
                    ..Text::default()
                });
            }
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
            Interaction::Panning { .. } => return mouse::Interaction::Grabbing,
            Interaction::Resizing { .. } => return mouse::Interaction::ResizingHorizontally,
            Interaction::Moving { .. } => return mouse::Interaction::Grabbing,
            _ => {}
        }
        let Some(point) = cursor.position_in(bounds) else {
            return mouse::Interaction::default();
        };
        if point.y < RULER_HEIGHT {
            return mouse::Interaction::ResizingHorizontally;
        }
        if point.x < KEY_WIDTH {
            return mouse::Interaction::Pointer;
        }
        if let Some(note) = self.note_at(point, bounds.size()) {
            if self.is_over_resize(note, point) {
                return mouse::Interaction::ResizingHorizontally;
            }
            return mouse::Interaction::Grab;
        }
        match self.state.tool {
            Tool::Erase => mouse::Interaction::NotAllowed,
            Tool::Draw => mouse::Interaction::Crosshair,
            Tool::Select => mouse::Interaction::Crosshair,
        }
    }
}

fn start_move(
    roll: &PianoRoll<'_>,
    interaction: &mut Interaction,
    note: &Note,
    point: Point,
) -> canvas::Action<Action> {
    let ids: Vec<Id> = if roll.state.selection.contains(&note.id) {
        roll.state.selection.clone()
    } else {
        vec![note.id]
    };
    let origins: Vec<(Id, u8, f64)> = roll
        .pattern
        .notes
        .iter()
        .filter(|n| ids.contains(&n.id))
        .map(|n| (n.id, n.key, n.start))
        .collect();
    *interaction = Interaction::Moving { anchor: point, origins, last_key: note.key };
    canvas::Action::publish(Action::BeginDrag(ids)).and_capture()
}

fn draw_note(
    frame: &mut Frame,
    roll: &PianoRoll<'_>,
    note: &Note,
    colour: Color,
    selected: bool,
    grid_bottom: f32,
    p: &theme::Palette,
) {
    let x = roll.x_of_beat(note.start);
    let width = (note.length * roll.state.pixels_per_beat as f64) as f32;
    let y = roll.y_of_key(note.key as f32);
    if y + roll.state.key_height < RULER_HEIGHT || y > grid_bottom {
        return;
    }
    if x + width < KEY_WIDTH || x > frame.width() {
        return;
    }
    let top = y.max(RULER_HEIGHT);
    let height = (y + roll.state.key_height - 1.0).min(grid_bottom) - top;
    if height <= 0.0 {
        return;
    }
    let left = x.max(KEY_WIDTH);
    let draw_width = (x + width - left).max(2.0);

    // Velocity shades the note, which is how the roll shows dynamics at a glance.
    let shaded = theme::mix(theme::darken(colour, 0.45), colour, note.velocity.clamp(0.0, 1.0));
    frame.fill_rectangle(Point::new(left, top), Size::new(draw_width, height), shaded);
    frame.stroke_rectangle(
        Point::new(left, top),
        Size::new(draw_width, height),
        Stroke::default()
            .with_color(if selected { p.accent } else { theme::darken(colour, 0.35) })
            .with_width(if selected { 2.0 } else { 1.0 }),
    );
    if draw_width > 26.0 && height > 9.0 {
        frame.fill_text(ui::canvas_text(
            ui::note_name(note.key),
            Point::new(left + 3.0, top + (height - 9.0) * 0.5),
            theme::contrasting_text(shaded),
            9.0,
        ));
    }
}

/// Build the piano roll view, toolbar and all.
pub fn view(app: &LimeDaw) -> Element<'_, Message> {
    let Some(pattern_id) = app.selected_pattern_id() else {
        return empty_message("Create a pattern to start writing notes");
    };
    let Some(pattern) = app.project.pattern(pattern_id) else {
        return empty_message("Create a pattern to start writing notes");
    };
    let Some(channel_id) = app.selected_channel_id() else {
        return empty_message("Add a channel to start writing notes");
    };

    let channel_color = app
        .project
        .channel(channel_id)
        .map(|c| c.color.to_iced())
        .unwrap_or(theme::palette(&app.theme).accent);
    let other_colors = app
        .project
        .channels
        .iter()
        .filter(|c| c.id != channel_id)
        .map(|c| (c.id, c.color.to_iced()))
        .collect();

    let program = PianoRoll {
        pattern,
        channel: channel_id,
        channel_color,
        other_colors,
        state: &app.piano_roll,
        position: app.engine.status().position(),
        beats_per_bar: app.project.beats_per_bar(),
        snap: app.settings.grid_snap,
        length: pattern.length_beats(app.project.beats_per_bar()),
    };

    let canvas = Canvas::new(program)
        .width(Length::Fill)
        .height(Length::Fill);

    column![
        toolbar(app, pattern),
        container(Element::from(canvas).map(Message::PianoRoll))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(theme::well),
    ]
    .spacing(4)
    .into()
}

fn empty_message(label: &str) -> Element<'_, Message> {
    container(text(label).size(14))
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

/// A channel as the toolbar's picker sees it: shown by name, matched by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelChoice {
    pub id: Id,
    pub name: String,
}

impl std::fmt::Display for ChannelChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// Snap values offered in the toolbar.
const SNAPS: [(&str, f64); 8] = [
    ("1/1", 4.0),
    ("1/2", 2.0),
    ("1/4", 1.0),
    ("1/8", 0.5),
    ("1/16", 0.25),
    ("1/32", 0.125),
    ("1/3", 4.0 / 3.0),
    ("Off", 0.0),
];

fn toolbar<'a>(app: &'a LimeDaw, pattern: &'a Pattern) -> Element<'a, Message> {
    let tools = row(Tool::ALL.map(|tool| {
        let active = app.piano_roll.tool == tool;
        button(text(tool.label()).size(12))
            .padding([4, 10])
            .style(move |theme: &Theme, status| theme::button_tab(theme, status, active))
            .on_press(Message::PianoRoll(Action::SetTool(tool)))
            .into()
    }))
    .spacing(2);

    let snaps = row(SNAPS.map(|(label, value)| {
        let active = (app.settings.grid_snap - value).abs() < 1e-9;
        button(text(label).size(11))
            .padding([3, 7])
            .style(move |theme: &Theme, status| theme::button_tab(theme, status, active))
            .on_press(Message::SetGridSnap(value))
            .into()
    }))
    .spacing(2);

    // The picker carries the channel id, so two channels sharing a name still
    // select the right one.
    let channels: Vec<ChannelChoice> = app
        .project
        .channels
        .iter()
        .map(|c| ChannelChoice { id: c.id, name: c.name.clone() })
        .collect();
    let current = app
        .selected_channel_id()
        .and_then(|id| channels.iter().find(|c| c.id == id))
        .cloned();

    let channel_picker = pick_list(channels, current, |choice: ChannelChoice| {
        Message::SelectChannel(choice.id)
    })
    .text_size(12)
    .padding([4, 8])
    .style(theme::picker);

    let zoom_out = button(text("−").size(13))
        .padding([2, 8])
        .style(theme::button_plain)
        .on_press(Message::PianoRoll(Action::Zoom {
            pixels_per_beat: (app.piano_roll.pixels_per_beat / 1.3).max(12.0),
            key_height: app.piano_roll.key_height,
        }));
    let zoom_in = button(text("+").size(13))
        .padding([2, 8])
        .style(theme::button_plain)
        .on_press(Message::PianoRoll(Action::Zoom {
            pixels_per_beat: (app.piano_roll.pixels_per_beat * 1.3).min(600.0),
            key_height: app.piano_roll.key_height,
        }));
    let taller = button(text("↕").size(12))
        .padding([2, 8])
        .style(theme::button_plain)
        .on_press(Message::PianoRoll(Action::Zoom {
            pixels_per_beat: app.piano_roll.pixels_per_beat,
            key_height: if app.piano_roll.key_height >= 24.0 { 9.0 } else { app.piano_roll.key_height + 4.0 },
        }));

    let selection_count = app.piano_roll.selection.len();
    let edit_tools = row![
        button(text("Copy").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::CopyNotes),
        button(text("Paste").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::PasteNotes),
        button(text("Duplicate").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::DuplicateNotes),
        button(text("Quantise").size(11))
            .padding([4, 8])
            .style(theme::button_plain)
            .on_press(Message::QuantizeSelection),
        button(text("Delete").size(11))
            .padding([4, 8])
            .style(theme::button_danger)
            .on_press(Message::DeleteSelection),
    ]
    .spacing(3);

    let velocity_toggle = button(text("Velocity").size(11))
        .padding([4, 8])
        .style({
            let on = app.piano_roll.show_velocity;
            move |theme: &Theme, status| theme::button_tab(theme, status, on)
        })
        .on_press(Message::PianoRoll(Action::ToggleVelocity));

    let ghost_toggle = button(text("Ghosts").size(11))
        .padding([4, 8])
        .style({
            let on = app.piano_roll.show_ghosts;
            move |theme: &Theme, status| theme::button_tab(theme, status, on)
        })
        .on_press(Message::PianoRoll(Action::ToggleGhosts));

    container(
        row![
            text(pattern.name.clone()).size(13),
            channel_picker,
            tools,
            ui::caption("Snap"),
            snaps,
            velocity_toggle,
            ghost_toggle,
            edit_tools,
            ui::caption(if selection_count > 0 {
                format!("{selection_count} selected")
            } else {
                String::new()
            }),
            iced::widget::space::horizontal(),
            zoom_out,
            zoom_in,
            taller,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([4, 8])
    .style(theme::header)
    .into()
}
