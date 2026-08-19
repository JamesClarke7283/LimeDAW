//! The interface: shared widgets plus one module per editor.

pub mod automation;
pub mod browser;
pub mod channel_rack;
pub mod effects;
pub mod instrument;
pub mod mcp_panel;
pub mod menu;
pub mod mixer;
pub mod piano_roll;
pub mod playlist;
pub mod settings;
pub mod setup;
pub mod shell;
pub mod transport;

use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path, Stroke, Text};
use iced::widget::{Space, column, container, row, text, tooltip};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme, mouse};

use crate::theme::{self, Palette};

/// A comfortable default row gap.
pub const GAP: u16 = 6;

/// Height of the transport bar.
pub const TOOLBAR_HEIGHT: f32 = 56.0;

/// Format a beat position as bars:beats:sixteenths, the way a DAW shows it.
pub fn format_position(beat: f64, beats_per_bar: f64) -> String {
    let bpb = beats_per_bar.max(1.0);
    let bar = (beat / bpb).floor();
    let within = beat - bar * bpb;
    let beat_in_bar = within.floor();
    let sixteenth = ((within - beat_in_bar) * 4.0).floor();
    format!(
        "{}:{}:{}",
        bar as i64 + 1,
        beat_in_bar as i64 + 1,
        sixteenth as i64 + 1
    )
}

/// Format seconds as m:ss.t.
pub fn format_time(seconds: f64) -> String {
    let s = seconds.max(0.0);
    let minutes = (s / 60.0).floor();
    let rest = s - minutes * 60.0;
    format!("{}:{:04.1}", minutes as i64, rest)
}

/// The MIDI note name for a key.
pub fn note_name(key: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = key as i32 / 12 - 1;
    format!("{}{}", NAMES[(key % 12) as usize], octave)
}

/// Whether a MIDI key is a black key.
pub fn is_black_key(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

/// A small caption.
pub fn caption<'a, M: 'a>(content: impl text::IntoFragment<'a>) -> Element<'a, M> {
    text(content)
        .size(11)
        .style(|theme: &Theme| text::Style {
            color: Some(theme::palette(theme).text_dim),
        })
        .into()
}

/// A section heading.
pub fn heading<'a, M: 'a>(content: impl text::IntoFragment<'a>) -> Element<'a, M> {
    text(content).size(14).into()
}

/// Wrap a widget with a hover tooltip.
pub fn hint<'a, M: 'a>(content: impl Into<Element<'a, M>>, label: &'a str) -> Element<'a, M> {
    tooltip(
        content,
        container(text(label).size(11))
            .padding(6)
            .style(theme::card),
        tooltip::Position::Bottom,
    )
    .into()
}

pub fn spacer<'a, M: 'a>(size: u16) -> Element<'a, M> {
    Space::new()
        .width(Length::Fixed(size as f32))
        .height(Length::Fixed(size as f32))
        .into()
}

/// A labelled value pair, stacked.
pub fn field<'a, M: 'a>(label: &'a str, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
    column![caption(label), content.into()].spacing(2).into()
}

// ---------------------------------------------------------------------------
// Knob
// ---------------------------------------------------------------------------

/// Interaction state for a [`Knob`].
#[derive(Debug, Clone, Copy, Default)]
pub struct KnobState {
    dragging: bool,
    /// Where the drag began and what the value was then.
    origin: f32,
    start_value: f32,
}

/// A rotary control. Dragging up or down changes the value; holding shift
/// makes it fine.
pub struct Knob<'a, Message> {
    value: f32,
    min: f32,
    max: f32,
    /// Where the pointer sits when the value is at its default, so bipolar
    /// controls can draw their arc from the centre.
    centre: Option<f32>,
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
    tint: Option<Color>,
}

impl<'a, Message> Knob<'a, Message> {
    pub fn new(value: f32, min: f32, max: f32, on_change: impl Fn(f32) -> Message + 'a) -> Self {
        Self {
            value,
            min,
            max,
            centre: None,
            on_change: Box::new(on_change),
            tint: None,
        }
    }

    /// Draw the value arc outward from a centre point, for pan-style controls.
    pub fn bipolar(mut self) -> Self {
        self.centre = Some((self.min + self.max) * 0.5);
        self
    }

    pub fn tint(mut self, color: Color) -> Self {
        self.tint = Some(color);
        self
    }

    fn normalised(&self) -> f32 {
        if (self.max - self.min).abs() < f32::EPSILON {
            0.0
        } else {
            ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }
}

impl<Message: Clone> canvas::Program<Message> for Knob<'_, Message> {
    type State = KnobState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let position = cursor.position_in(bounds)?;
                state.dragging = true;
                state.origin = position.y;
                state.start_value = self.value;
                Some(canvas::Action::request_redraw().and_capture())
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
                // 160 pixels of travel covers the whole range.
                let delta = (state.origin - position.y) / 160.0;
                let span = self.max - self.min;
                let value = (state.start_value + delta * span).clamp(self.min, self.max);
                Some(canvas::Action::publish((self.on_change)(value)).and_capture())
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.dragging =>
            {
                state.dragging = false;
                Some(canvas::Action::request_redraw().and_capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let steps = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
                };
                let span = self.max - self.min;
                let value = (self.value + steps * span * 0.02).clamp(self.min, self.max);
                Some(canvas::Action::publish((self.on_change)(value)).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = theme::palette(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        let centre = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let radius = bounds.width.min(bounds.height) / 2.0 - 2.0;
        let accent = self.tint.unwrap_or(p.accent);
        let hovered = cursor.is_over(bounds) || state.dragging;

        // Track: a 270-degree arc with the gap at the bottom.
        const START: f32 = std::f32::consts::PI * 0.75;
        const SWEEP: f32 = std::f32::consts::PI * 1.5;

        let track = Path::new(|b| {
            b.arc(canvas::path::Arc {
                center: centre,
                radius: radius - 2.0,
                start_angle: START.into(),
                end_angle: (START + SWEEP).into(),
            });
        });
        frame.stroke(
            &track,
            Stroke::default()
                .with_color(p.surface_low)
                .with_width(4.0)
                .with_line_cap(canvas::LineCap::Round),
        );

        let t = self.normalised();
        let (arc_from, arc_to) = match self.centre {
            Some(c) => {
                let cn = ((c - self.min) / (self.max - self.min).max(f32::EPSILON)).clamp(0.0, 1.0);
                let a = START + SWEEP * cn.min(t);
                let b = START + SWEEP * cn.max(t);
                (a, b)
            }
            None => (START, START + SWEEP * t),
        };
        if (arc_to - arc_from).abs() > 0.001 {
            let value_arc = Path::new(|b| {
                b.arc(canvas::path::Arc {
                    center: centre,
                    radius: radius - 2.0,
                    start_angle: arc_from.into(),
                    end_angle: arc_to.into(),
                });
            });
            frame.stroke(
                &value_arc,
                Stroke::default()
                    .with_color(accent)
                    .with_width(4.0)
                    .with_line_cap(canvas::LineCap::Round),
            );
        }

        // Body.
        let body = Path::circle(centre, (radius - 6.0).max(1.0));
        frame.fill(
            &body,
            if hovered {
                theme::lighten(p.surface_high, 0.04)
            } else {
                p.surface_high
            },
        );
        frame.stroke(
            &body,
            Stroke::default().with_color(p.border).with_width(1.0),
        );

        // Pointer.
        let angle = START + SWEEP * t;
        let inner = Point::new(
            centre.x + angle.cos() * (radius - 12.0).max(1.0),
            centre.y + angle.sin() * (radius - 12.0).max(1.0),
        );
        let outer = Point::new(
            centre.x + angle.cos() * (radius - 5.0).max(2.0),
            centre.y + angle.sin() * (radius - 5.0).max(2.0),
        );
        let pointer = Path::line(inner, outer);
        frame.stroke(
            &pointer,
            Stroke::default()
                .with_color(accent)
                .with_width(2.5)
                .with_line_cap(canvas::LineCap::Round),
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.dragging {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

/// A knob with a caption and a readout underneath.
pub fn knob<'a, Message: Clone + 'a>(
    label: &'a str,
    display: String,
    value: f32,
    min: f32,
    max: f32,
    size: f32,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        Canvas::new(Knob::new(value, min, max, on_change))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size)),
        text(label).size(10).style(|theme: &Theme| text::Style {
            color: Some(theme::palette(theme).text_dim)
        }),
        text(display).size(10),
    ]
    .align_x(Alignment::Center)
    .spacing(1)
    .into()
}

/// A knob tinted to match its channel or device.
#[allow(clippy::too_many_arguments)]
pub fn knob_tinted<'a, Message: Clone + 'a>(
    label: &'a str,
    display: String,
    value: f32,
    min: f32,
    max: f32,
    size: f32,
    tint: Color,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        Canvas::new(Knob::new(value, min, max, on_change).tint(tint))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size)),
        text(label).size(10).style(|theme: &Theme| text::Style {
            color: Some(theme::palette(theme).text_dim)
        }),
        text(display).size(10),
    ]
    .align_x(Alignment::Center)
    .spacing(1)
    .into()
}

/// A tinted bipolar knob, for parameters that swing either side of centre.
#[allow(clippy::too_many_arguments)]
pub fn knob_bipolar_tinted<'a, Message: Clone + 'a>(
    label: &'a str,
    display: String,
    value: f32,
    min: f32,
    max: f32,
    size: f32,
    tint: Color,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        Canvas::new(Knob::new(value, min, max, on_change).bipolar().tint(tint))
            .width(Length::Fixed(size))
            .height(Length::Fixed(size)),
        text(label).size(10).style(|theme: &Theme| text::Style {
            color: Some(theme::palette(theme).text_dim)
        }),
        text(display).size(10),
    ]
    .align_x(Alignment::Center)
    .spacing(1)
    .into()
}

/// The same, but drawn as a bipolar control.
pub fn knob_bipolar<'a, Message: Clone + 'a>(
    label: &'a str,
    display: String,
    value: f32,
    min: f32,
    max: f32,
    size: f32,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        Canvas::new(Knob::new(value, min, max, on_change).bipolar())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size)),
        text(label).size(10).style(|theme: &Theme| text::Style {
            color: Some(theme::palette(theme).text_dim)
        }),
        text(display).size(10),
    ]
    .align_x(Alignment::Center)
    .spacing(1)
    .into()
}

// ---------------------------------------------------------------------------
// Level meter
// ---------------------------------------------------------------------------

/// A stereo peak meter.
pub struct LevelMeter {
    pub left: f32,
    pub right: f32,
    pub horizontal: bool,
}

impl<Message> canvas::Program<Message> for LevelMeter {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = theme::palette(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), p.surface_low);

        let draw_bar = |frame: &mut Frame, level: f32, offset: f32, thickness: f32| {
            // A meter scaled in decibels reads far better than a linear one.
            let db = crate::audio::dsp::gain_to_db(level.max(1e-5));
            let t = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
            if t <= 0.001 {
                return;
            }
            let colour = if level >= 0.99 {
                p.meter_clip
            } else if t > 0.86 {
                p.meter_high
            } else {
                p.meter_low
            };
            if self.horizontal {
                frame.fill_rectangle(
                    Point::new(0.0, offset),
                    Size::new(bounds.width * t, thickness),
                    colour,
                );
            } else {
                let h = bounds.height * t;
                frame.fill_rectangle(
                    Point::new(offset, bounds.height - h),
                    Size::new(thickness, h),
                    colour,
                );
            }
        };

        if self.horizontal {
            let thickness = (bounds.height - 3.0) / 2.0;
            draw_bar(&mut frame, self.left, 0.0, thickness);
            draw_bar(&mut frame, self.right, thickness + 3.0, thickness);
        } else {
            let thickness = (bounds.width - 3.0) / 2.0;
            draw_bar(&mut frame, self.left, 0.0, thickness);
            draw_bar(&mut frame, self.right, thickness + 3.0, thickness);
        }

        vec![frame.into_geometry()]
    }
}

pub fn meter<'a, Message: 'a>(
    left: f32,
    right: f32,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    Canvas::new(LevelMeter {
        left,
        right,
        horizontal: width > height,
    })
    .width(Length::Fixed(width))
    .height(Length::Fixed(height))
    .into()
}

// ---------------------------------------------------------------------------
// Waveform display
// ---------------------------------------------------------------------------

/// Draws a sample's peaks, with optional start/end markers.
pub struct Waveform {
    pub peaks: Vec<(f32, f32)>,
    pub start: f32,
    pub end: f32,
    pub tint: Color,
}

impl<Message> canvas::Program<Message> for Waveform {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = theme::palette(theme);
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), p.surface_low);

        if self.peaks.is_empty() {
            frame.fill_text(Text {
                content: "no sample".into(),
                position: Point::new(bounds.width / 2.0, bounds.height / 2.0),
                color: p.text_dim,
                size: 11.0.into(),
                align_x: iced::alignment::Horizontal::Center.into(),
                align_y: iced::alignment::Vertical::Center,
                ..Text::default()
            });
            return vec![frame.into_geometry()];
        }

        let mid = bounds.height / 2.0;
        let step = bounds.width / self.peaks.len() as f32;
        for (i, (lo, hi)) in self.peaks.iter().enumerate() {
            let x = i as f32 * step;
            let top = mid - hi * mid * 0.95;
            let bottom = mid - lo * mid * 0.95;
            frame.fill_rectangle(
                Point::new(x, top),
                Size::new(step.max(1.0), (bottom - top).max(1.0)),
                self.tint,
            );
        }

        // Dim the parts outside the playback window.
        let shade = theme::alpha(p.base, 0.6);
        if self.start > 0.0 {
            frame.fill_rectangle(
                Point::ORIGIN,
                Size::new(bounds.width * self.start, bounds.height),
                shade,
            );
        }
        if self.end < 1.0 {
            frame.fill_rectangle(
                Point::new(bounds.width * self.end, 0.0),
                Size::new(bounds.width * (1.0 - self.end), bounds.height),
                shade,
            );
        }

        let centre = Path::line(Point::new(0.0, mid), Point::new(bounds.width, mid));
        frame.stroke(
            &centre,
            Stroke::default()
                .with_color(theme::alpha(p.text_dim, 0.35))
                .with_width(1.0),
        );

        vec![frame.into_geometry()]
    }
}

pub fn waveform<'a, Message: 'a>(
    peaks: Vec<(f32, f32)>,
    start: f32,
    end: f32,
    tint: Color,
    height: f32,
) -> Element<'a, Message> {
    Canvas::new(Waveform {
        peaks,
        start,
        end,
        tint,
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .into()
}

// ---------------------------------------------------------------------------
// Shared canvas drawing helpers
// ---------------------------------------------------------------------------

/// Draw the vertical grid for a beat-based timeline.
#[allow(clippy::too_many_arguments)]
pub fn draw_beat_grid(
    frame: &mut Frame,
    p: &Palette,
    bounds: Size,
    x_offset: f32,
    pixels_per_beat: f32,
    scroll_beats: f64,
    beats_per_bar: f64,
    snap: f64,
) {
    let visible_beats = ((bounds.width - x_offset) / pixels_per_beat.max(0.01)) as f64;
    let first = scroll_beats.floor();
    let last = scroll_beats + visible_beats + 1.0;

    // Fine grid lines only once they are far enough apart to read.
    let fine = if pixels_per_beat * snap as f32 >= 6.0 {
        snap
    } else {
        1.0
    };
    let mut beat = (first / fine).floor() * fine;
    while beat <= last {
        let x = x_offset + ((beat - scroll_beats) * pixels_per_beat as f64) as f32;
        if x >= x_offset - 1.0 && x <= bounds.width {
            let on_bar = (beat % beats_per_bar).abs() < 1e-6;
            let on_beat = (beat.fract()).abs() < 1e-6;
            let (colour, width) = if on_bar {
                (p.grid_bar, 1.5)
            } else if on_beat {
                (p.grid_beat, 1.0)
            } else {
                (p.grid_line, 1.0)
            };
            frame.fill_rectangle(Point::new(x, 0.0), Size::new(width, bounds.height), colour);
        }
        beat += fine;
    }
}

/// Draw the playhead line.
pub fn draw_playhead(
    frame: &mut Frame,
    p: &Palette,
    bounds: Size,
    x_offset: f32,
    pixels_per_beat: f32,
    scroll_beats: f64,
    position: f64,
) {
    let x = x_offset + ((position - scroll_beats) * pixels_per_beat as f64) as f32;
    if x < x_offset || x > bounds.width {
        return;
    }
    frame.fill_rectangle(
        Point::new(x - 1.0, 0.0),
        Size::new(2.0, bounds.height),
        p.playhead,
    );
}

/// A label drawn into a canvas.
pub fn canvas_text(content: impl Into<String>, position: Point, color: Color, size: f32) -> Text {
    Text {
        content: content.into(),
        position,
        color,
        size: size.into(),
        align_x: iced::alignment::Horizontal::Left.into(),
        align_y: iced::alignment::Vertical::Top,
        ..Text::default()
    }
}

/// Where a horizontal scroll position should sit to keep `beat` visible.
pub fn keep_visible(scroll: f64, beat: f64, visible: f64) -> f64 {
    if beat < scroll {
        beat
    } else if beat > scroll + visible * 0.9 {
        (beat - visible * 0.5).max(0.0)
    } else {
        scroll
    }
}

/// A row of buttons that reads as a segmented control.
pub fn segmented<'a, Message: Clone + 'a, T: Copy + PartialEq>(
    options: &[(T, &'a str)],
    active: T,
    on_select: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    let mut controls = row![].spacing(2);
    for (value, label) in options {
        let is_active = *value == active;
        let value = *value;
        controls = controls.push(
            iced::widget::button(text(*label).size(12))
                .padding([4, 10])
                .style(move |theme: &Theme, status| theme::button_tab(theme, status, is_active))
                .on_press(on_select(value)),
        );
    }
    container(controls).padding(2).style(theme::well).into()
}
