//! The application icon: a lime, drawn rather than shipped as a binary asset.
//!
//! Generating it in code keeps the repository free of image blobs and means
//! the same drawing produces the window icon, the favicon and any size a
//! desktop environment asks for.

/// The nominal icon size. Window managers scale from this happily.
pub const SIZE: u32 = 128;

/// How many samples per axis the drawing is supersampled at, for smooth edges.
const SUPERSAMPLE: u32 = 3;

/// The number of segments in the fruit.
const SEGMENTS: usize = 9;

type Rgb = [f32; 3];

const BACKGROUND: Rgb = [0.03, 0.04, 0.03];
const RIND_OUTER: Rgb = [0.24, 0.42, 0.09];
const RIND_INNER: Rgb = [0.42, 0.65, 0.13];
const PITH: Rgb = [0.85, 0.93, 0.72];
const FLESH: Rgb = [0.61, 0.84, 0.20];
const FLESH_EDGE: Rgb = [0.44, 0.68, 0.14];
const CORE: Rgb = [0.80, 0.90, 0.62];

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The colour of one point, in a coordinate space where the icon spans -1..1.
fn sample(x: f32, y: f32) -> Rgb {
    let radius = (x * x + y * y).sqrt();

    // Outside the fruit: the black field it sits on.
    if radius > 0.94 {
        return BACKGROUND;
    }
    // Rind: a band that darkens toward the outside.
    if radius > 0.82 {
        let t = (0.94 - radius) / 0.12;
        return mix(RIND_OUTER, RIND_INNER, t);
    }
    // Pith: the pale ring just inside the rind.
    if radius > 0.76 {
        return PITH;
    }

    // Flesh, divided into wedges by pale membranes.
    let angle = y.atan2(x);
    let segment = std::f32::consts::TAU / SEGMENTS as f32;
    // Distance to the nearest membrane, as a fraction of half a segment.
    let within = (angle.rem_euclid(segment) / segment - 0.5).abs() * 2.0;

    // Membranes are thicker toward the rind, the way a real citrus looks.
    let membrane = 0.86 - radius * 0.22;
    if within > membrane {
        let t = (within - membrane) / (1.0 - membrane).max(1e-3);
        return mix(FLESH_EDGE, PITH, t.min(1.0));
    }

    // The core where the membranes meet.
    if radius < 0.1 {
        return mix(CORE, PITH, radius / 0.1);
    }

    // Juice vesicles: subtly brighter toward the middle of each wedge.
    let brightness = 1.0 - within * 0.35 - radius * 0.12;
    mix(FLESH_EDGE, FLESH, brightness)
}

/// Render the icon as RGBA bytes, `size` pixels square.
pub fn rgba(size: u32) -> Vec<u8> {
    let size = size.max(8);
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let samples = SUPERSAMPLE.max(1);
    let step = 1.0 / samples as f32;

    for py in 0..size {
        for px in 0..size {
            let mut acc = [0.0f32; 3];
            for sy in 0..samples {
                for sx in 0..samples {
                    // Map the pixel into -1..1 with a little margin.
                    let fx = (px as f32 + (sx as f32 + 0.5) * step) / size as f32;
                    let fy = (py as f32 + (sy as f32 + 0.5) * step) / size as f32;
                    let x = (fx * 2.0 - 1.0) * 1.06;
                    let y = (fy * 2.0 - 1.0) * 1.06;
                    let c = sample(x, y);
                    acc[0] += c[0];
                    acc[1] += c[1];
                    acc[2] += c[2];
                }
            }
            let n = (samples * samples) as f32;
            for channel in acc {
                out.push(((channel / n).clamp(0.0, 1.0) * 255.0).round() as u8);
            }
            out.push(255);
        }
    }
    out
}

/// The icon for the application window, if it can be built.
pub fn window_icon() -> Option<iced::window::Icon> {
    iced::window::icon::from_rgba(rgba(SIZE), SIZE, SIZE).ok()
}

/// The same artwork as a scalable vector, for desktop entries and the web page.
pub fn svg() -> String {
    let mut wedges = String::new();
    let segment = 360.0 / SEGMENTS as f64;
    for i in 0..SEGMENTS {
        let start = i as f64 * segment + segment * 0.06;
        let end = (i + 1) as f64 * segment - segment * 0.06;
        let (sx, sy) = polar(start, 0.72);
        let (ex, ey) = polar(end, 0.72);
        wedges.push_str(&format!(
            "<path d=\"M 0 0 L {sx:.3} {sy:.3} A 0.72 0.72 0 0 1 {ex:.3} {ey:.3} Z\" fill=\"#9BD634\"/>"
        ));
    }
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-1 -1 2 2" width="128" height="128">
  <rect x="-1" y="-1" width="2" height="2" fill="#080A08"/>
  <circle r="0.94" fill="#3D6A16"/>
  <circle r="0.86" fill="#6BA61F"/>
  <circle r="0.79" fill="#D9EDB8"/>
  <circle r="0.74" fill="#7CB518"/>
  <g>{wedges}</g>
  <circle r="0.08" fill="#D9EDB8"/>
</svg>
"##
    )
}

fn polar(degrees: f64, radius: f64) -> (f64, f64) {
    let r = degrees.to_radians();
    (r.cos() * radius, r.sin() * radius)
}
