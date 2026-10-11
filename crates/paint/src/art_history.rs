//! Art History Brush: stylised strokes coloured from a history state.
//!
//! Each place the pointer passes scatters short strokes over the Area around it. A stroke starts
//! at a random point, takes the source (history state) colour there, and runs along the source's
//! edges (perpendicular to its luminance gradient), so the painting follows the image's shapes.
//! The style sets how long the strokes are, how loosely they follow the edges and whether they
//! curl; Tolerance keeps strokes off places that already match the source.
//!
//! Every random choice is a hash of the seed and the stroke index ([`crate::rng`]), so a journaled
//! stroke replays exactly. The work is bounded: at most [`MAX_STROKES`] strokes of at most
//! [`MAX_STEPS`] points each, whatever the params.

use photocraft_geom::Rect;
use photocraft_raster::{Surface, to_rgba};

use crate::rng::rand01;
use crate::{Stroke, StrokePoint};

/// Photoshop's Art History Brush styles, in its menu order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtStyle {
    TightShort,
    TightMedium,
    TightLong,
    LooseMedium,
    LooseLong,
    Dab,
    TightCurl,
    TightCurlLong,
    LooseCurl,
    LooseCurlLong,
}

impl ArtStyle {
    pub const ALL: [Self; 10] = [
        Self::TightShort,
        Self::TightMedium,
        Self::TightLong,
        Self::LooseMedium,
        Self::LooseLong,
        Self::Dab,
        Self::TightCurl,
        Self::TightCurlLong,
        Self::LooseCurl,
        Self::LooseCurlLong,
    ];

    /// The id commands and settings use.
    pub fn id(self) -> &'static str {
        match self {
            Self::TightShort => "tightShort",
            Self::TightMedium => "tightMedium",
            Self::TightLong => "tightLong",
            Self::LooseMedium => "looseMedium",
            Self::LooseLong => "looseLong",
            Self::Dab => "dab",
            Self::TightCurl => "tightCurl",
            Self::TightCurlLong => "tightCurlLong",
            Self::LooseCurl => "looseCurl",
            Self::LooseCurlLong => "looseCurlLong",
        }
    }

    /// The name Photoshop's Style menu shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::TightShort => "Tight Short",
            Self::TightMedium => "Tight Medium",
            Self::TightLong => "Tight Long",
            Self::LooseMedium => "Loose Medium",
            Self::LooseLong => "Loose Long",
            Self::Dab => "Dab",
            Self::TightCurl => "Tight Curl",
            Self::TightCurlLong => "Tight Curl Long",
            Self::LooseCurl => "Loose Curl",
            Self::LooseCurlLong => "Loose Curl Long",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Stroke length in brush diameters.
    fn length(self) -> f64 {
        match self {
            Self::Dab => 0.0,
            Self::TightShort => 1.5,
            Self::TightMedium | Self::LooseMedium | Self::TightCurl | Self::LooseCurl => 3.0,
            Self::TightLong | Self::LooseLong | Self::TightCurlLong | Self::LooseCurlLong => 6.0,
        }
    }

    fn loose(self) -> bool {
        matches!(self, Self::LooseMedium | Self::LooseLong | Self::LooseCurl | Self::LooseCurlLong)
    }

    fn curl(self) -> bool {
        matches!(self, Self::TightCurl | Self::TightCurlLong | Self::LooseCurl | Self::LooseCurlLong)
    }
}

/// The Art History Brush's own options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArtSettings {
    pub style: ArtStyle,
    /// Diameter, in pixels, of the area each pointer position scatters strokes over (0..=500).
    pub area: f64,
    /// 0..1: a stroke is placed only where the layer differs from the source by at least this
    /// much (0 paints anywhere).
    pub tolerance: f32,
}

/// One stroke to paint: its points and its colour (the source's native channels).
#[derive(Clone, Debug, PartialEq)]
pub struct ArtStroke {
    pub points: Vec<StrokePoint>,
    pub color: Vec<f32>,
}

/// Most strokes one command paints.
pub const MAX_STROKES: usize = 4096;
/// Most points in one stroke.
pub const MAX_STEPS: usize = 64;
/// Most strokes scattered around one pointer position.
const PER_PLACE: usize = 24;

const S_RADIUS: u64 = 1;
const S_ANGLE: u64 = 2;
const S_DIR: u64 = 3;
const S_SIGN: u64 = 4;
const S_JITTER: u64 = 5;
const S_CURL: u64 = 6;

/// Luminance of the source at `(x, y)` (0 where it is transparent).
fn luma(src: &Surface, x: f64, y: f64) -> f32 {
    let px = src.pixel(x.floor() as i32, y.floor() as i32);
    let c = to_rgba(&src.format(), &px);
    (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) * c[3]
}

/// The edge direction at `(x, y)`: perpendicular to the luminance gradient, measured `r` pixels
/// apart, as a unit vector. `None` where the source is flat.
fn edge_dir(src: &Surface, x: f64, y: f64, r: f64) -> Option<(f64, f64)> {
    let gx = f64::from(luma(src, x + r, y) - luma(src, x - r, y));
    let gy = f64::from(luma(src, x, y + r) - luma(src, x, y - r));
    let len = gx.hypot(gy);
    (len > 1e-3).then(|| (-gy / len, gx / len))
}

/// The pointer positions strokes scatter around: the stroke's points, resampled every `step`
/// pixels along it (the first point always). A drag too long for [`MAX_STROKES`] places at that
/// spacing is resampled more sparsely, so the whole drag is still painted.
fn places(points: &[StrokePoint], step: f64) -> Vec<StrokePoint> {
    let Some(first) = points.first() else { return Vec::new() };
    let total: f64 = points.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).filter(|d| d.is_finite()).sum();
    let step = step.max(total / MAX_STROKES as f64);
    let mut out = vec![*first];
    let mut carry = 0.0;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = (b.x - a.x).hypot(b.y - a.y);
        if !seg.is_finite() || seg <= 0.0 {
            continue;
        }
        let mut t = step - carry;
        while t <= seg {
            let k = t / seg;
            out.push(StrokePoint::new(a.x + (b.x - a.x) * k, a.y + (b.y - a.y) * k, a.pressure + (b.pressure - a.pressure) * k as f32));
            if out.len() >= MAX_STROKES {
                return out;
            }
            t += step;
        }
        carry = (carry + seg) % step;
    }
    out
}

/// The strokes an Art History stroke paints. `source` is the history state's surface and
/// `current` the layer before the stroke (both in the same format); strokes start only inside
/// `canvas`. `stroke.brush.size` is the brush diameter.
pub fn art_strokes(source: &Surface, current: &Surface, stroke: &Stroke, settings: &ArtSettings, canvas: Rect, seed: u64) -> Vec<ArtStroke> {
    let size = f64::from(stroke.brush.size).max(1.0);
    let area = if settings.area.is_finite() { settings.area.clamp(0.0, 500.0) } else { 0.0 };
    let tolerance = if settings.tolerance.is_finite() { settings.tolerance.clamp(0.0, 1.0) } else { 0.0 };
    let style = settings.style;
    // Each place covers its Area with strokes about a brush wide.
    let count = if area <= 0.0 { 1 } else { (((area / size).powi(2) * 0.5).round() as usize).clamp(1, PER_PLACE) };
    let length = style.length() * size;
    let step_len = (size / 3.0).max(1.0);
    let steps = ((length / step_len).round() as usize).min(MAX_STEPS - 1);
    let probe = (size / 4.0).max(1.0);
    // Curls turn about one and a half times over their length.
    let turn = if style.curl() && steps > 0 { 3.0 * std::f64::consts::PI / steps as f64 } else { 0.0 };
    let fmt = source.format();
    let alpha = fmt.alpha.then(|| fmt.mode.color_channels());
    let places = places(&stroke.points, (area / 2.0).max(size / 2.0).max(1.0));
    // A long drag scatters fewer strokes per place, so it is painted along its whole length.
    let count = count.min((MAX_STROKES / places.len().max(1)).max(1));
    let mut out = Vec::new();
    let mut index: u64 = 0;
    for place in places {
        for _ in 0..count {
            index += 1;
            if out.len() >= MAX_STROKES {
                return out;
            }
            let r = area / 2.0 * f64::from(rand01(seed, index, S_RADIUS)).sqrt();
            let a = std::f64::consts::TAU * f64::from(rand01(seed, index, S_ANGLE));
            let (x0, y0) = (place.x + r * a.cos(), place.y + r * a.sin());
            let (ix, iy) = (x0.floor(), y0.floor());
            if !ix.is_finite() || !iy.is_finite() || !canvas.contains(ix as i32, iy as i32) {
                continue;
            }
            let color = source.pixel(ix as i32, iy as i32);
            if alpha.and_then(|a| color.get(a)).is_some_and(|v| *v <= 0.0) {
                continue;
            }
            if tolerance > 0.0 {
                let here = current.pixel(ix as i32, iy as i32);
                let diff = color.iter().zip(&here).map(|(s, c)| (s - c).abs()).fold(0.0f32, f32::max);
                if diff < tolerance {
                    continue;
                }
            }
            // Along the edge, either way; a random way where the source is flat.
            let flip = if rand01(seed, index, S_SIGN) < 0.5 { -1.0 } else { 1.0 };
            let free = std::f64::consts::TAU * f64::from(rand01(seed, index, S_DIR));
            let (mut dx, mut dy) = edge_dir(source, x0, y0, probe).map_or((free.cos(), free.sin()), |(x, y)| (x * flip, y * flip));
            let curl = if rand01(seed, index, S_CURL) < 0.5 { -turn } else { turn };
            let mut pts = vec![StrokePoint::new(x0, y0, place.pressure)];
            let (mut x, mut y) = (x0, y0);
            for k in 0..steps as u64 {
                if style.curl() {
                    let (c, s) = (curl.cos(), curl.sin());
                    (dx, dy) = (dx * c - dy * s, dx * s + dy * c);
                } else if let Some((ex, ey)) = edge_dir(source, x, y, probe) {
                    // Follow the edge without reversing; loose strokes follow it less closely.
                    let (ex, ey) = if ex * dx + ey * dy < 0.0 { (-ex, -ey) } else { (ex, ey) };
                    let follow = if style.loose() { 0.35 } else { 0.8 };
                    (dx, dy) = (dx + (ex - dx) * follow, dy + (ey - dy) * follow);
                }
                if style.loose() {
                    let j = 0.5 * f64::from(rand01(seed, index.wrapping_mul(MAX_STEPS as u64).wrapping_add(k), S_JITTER) * 2.0 - 1.0);
                    let (c, s) = (j.cos(), j.sin());
                    (dx, dy) = (dx * c - dy * s, dx * s + dy * c);
                }
                let len = dx.hypot(dy);
                if !len.is_finite() || len <= 1e-9 {
                    break;
                }
                (dx, dy) = (dx / len, dy / len);
                x += dx * step_len;
                y += dy * step_len;
                pts.push(StrokePoint::new(x, y, place.pressure));
            }
            out.push(ArtStroke { points: pts, color });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BrushSettings;
    use photocraft_color::PixelFormat;

    /// A 64×64 RGBA8 surface: left half black, right half white (one vertical edge at x = 32).
    fn edge() -> Surface {
        let mut s = Surface::new(PixelFormat::RGBA8);
        s.fill_rect(Rect::new(0, 0, 32, 64), &[0.0, 0.0, 0.0, 1.0]);
        s.fill_rect(Rect::new(32, 0, 64, 64), &[1.0, 1.0, 1.0, 1.0]);
        s
    }

    fn stroke(points: &[(f64, f64)], size: f32) -> Stroke {
        let brush = BrushSettings { size, ..BrushSettings::default() };
        Stroke { brush, points: points.iter().map(|&(x, y)| StrokePoint::new(x, y, 1.0)).collect() }
    }

    fn settings(style: ArtStyle, area: f64) -> ArtSettings {
        ArtSettings { style, area, tolerance: 0.0 }
    }

    const CANVAS: Rect = Rect::new(0, 0, 64, 64);

    #[test]
    fn styles_round_trip_their_ids() {
        for s in ArtStyle::ALL {
            assert_eq!(ArtStyle::from_id(s.id()), Some(s));
        }
        assert_eq!(ArtStyle::from_id("plaid"), None);
    }

    /// Strokes take the source colour where they start, and Tight strokes run along the edge.
    #[test]
    fn tight_strokes_follow_edges_in_the_source_colour() {
        let src = edge();
        let out = art_strokes(&src, &Surface::new(PixelFormat::RGBA8), &stroke(&[(32.0, 32.0)], 4.0), &settings(ArtStyle::TightLong, 20.0), CANVAS, 7);
        assert!(!out.is_empty());
        for s in &out {
            let p0 = s.points[0];
            let want = if p0.x < 32.0 { 0.0 } else { 1.0 };
            assert_eq!(s.color, vec![want, want, want, 1.0], "colour from where it starts");
            let last = s.points.last().unwrap();
            // Near the edge, strokes run up and down it, not across.
            if (p0.x - 32.0).abs() < 3.0 {
                assert!((last.y - p0.y).abs() > (last.x - p0.x).abs(), "{:?} → {:?}", (p0.x, p0.y), (last.x, last.y));
            }
        }
    }

    #[test]
    fn style_sets_the_length() {
        let src = edge();
        let cur = Surface::new(PixelFormat::RGBA8);
        let len = |style| {
            let out = art_strokes(&src, &cur, &stroke(&[(16.0, 32.0)], 4.0), &settings(style, 10.0), CANVAS, 3);
            out.iter().map(|s| s.points.len()).max().unwrap()
        };
        assert_eq!(len(ArtStyle::Dab), 1);
        assert!(len(ArtStyle::TightShort) < len(ArtStyle::TightMedium));
        assert!(len(ArtStyle::TightMedium) < len(ArtStyle::TightLong));
        // A curl turns back on itself: its end is nearer its start than its length.
        let out = art_strokes(&src, &cur, &stroke(&[(16.0, 32.0)], 4.0), &settings(ArtStyle::TightCurlLong, 10.0), CANVAS, 3);
        for s in &out {
            let (a, b) = (s.points[0], s.points.last().unwrap());
            let walked = (s.points.len() - 1) as f64 * (4.0f64 / 3.0).max(1.0);
            assert!((b.x - a.x).hypot(b.y - a.y) < walked * 0.6);
        }
    }

    /// Area 0 paints one stroke where the pointer is; a larger area scatters more strokes, within it.
    #[test]
    fn area_scatters_strokes() {
        let src = edge();
        let cur = Surface::new(PixelFormat::RGBA8);
        let one = art_strokes(&src, &cur, &stroke(&[(16.0, 16.0)], 6.0), &settings(ArtStyle::Dab, 0.0), CANVAS, 1);
        assert_eq!(one.len(), 1);
        assert_eq!((one[0].points[0].x, one[0].points[0].y), (16.0, 16.0));
        let many = art_strokes(&src, &cur, &stroke(&[(32.0, 32.0)], 4.0), &settings(ArtStyle::Dab, 30.0), CANVAS, 1);
        assert!(many.len() > 5);
        assert!(many.iter().all(|s| (s.points[0].x - 32.0).hypot(s.points[0].y - 32.0) <= 15.0));
    }

    /// The same seed paints the same strokes (journal replay); another seed doesn't.
    #[test]
    fn deterministic_per_seed() {
        let src = edge();
        let cur = Surface::new(PixelFormat::RGBA8);
        let st = stroke(&[(10.0, 10.0), (50.0, 40.0)], 5.0);
        let a = art_strokes(&src, &cur, &st, &settings(ArtStyle::LooseCurl, 25.0), CANVAS, 42);
        assert_eq!(a, art_strokes(&src, &cur, &st, &settings(ArtStyle::LooseCurl, 25.0), CANVAS, 42));
        assert_ne!(a, art_strokes(&src, &cur, &st, &settings(ArtStyle::LooseCurl, 25.0), CANVAS, 43));
    }

    /// Tolerance keeps strokes off places that already match the source.
    #[test]
    fn tolerance_skips_matching_places() {
        let src = edge();
        let st = stroke(&[(32.0, 32.0)], 4.0);
        let s = ArtSettings { style: ArtStyle::Dab, area: 40.0, tolerance: 0.5 };
        // The layer already is the source: nothing to paint.
        assert!(art_strokes(&src, &src, &st, &s, CANVAS, 9).is_empty());
        // Only the right (white) half differs from an all-black layer.
        let mut black = Surface::new(PixelFormat::RGBA8);
        black.fill_rect(CANVAS, &[0.0, 0.0, 0.0, 1.0]);
        let out = art_strokes(&src, &black, &st, &s, CANVAS, 9);
        assert!(!out.is_empty() && out.iter().all(|s| s.points[0].x >= 32.0));
    }

    /// Strokes start only on the canvas and never on transparent source.
    #[test]
    fn nothing_from_outside_or_transparent() {
        let mut src = Surface::new(PixelFormat::RGBA8);
        src.fill_rect(Rect::new(0, 0, 10, 10), &[1.0, 0.0, 0.0, 1.0]);
        let out = art_strokes(&src, &Surface::new(PixelFormat::RGBA8), &stroke(&[(5.0, 5.0)], 4.0), &settings(ArtStyle::Dab, 60.0), CANVAS, 2);
        assert!(!out.is_empty());
        assert!(out.iter().all(|s| (0.0..10.0).contains(&s.points[0].x) && (0.0..10.0).contains(&s.points[0].y)));
    }

    /// A drag too long for the stroke budget is still painted along its whole length, more sparsely.
    #[test]
    fn long_drags_are_painted_end_to_end() {
        let src = edge();
        let cur = Surface::new(PixelFormat::RGBA8);
        let zigzag: Vec<(f64, f64)> = (0..2000).map(|i| (f64::from(i % 2) * 60.0 + 2.0, 2.0 + f64::from(i) * 0.03)).collect();
        let out = art_strokes(&src, &cur, &stroke(&zigzag, 2.0), &settings(ArtStyle::Dab, 40.0), CANVAS, 4);
        assert!(out.len() <= MAX_STROKES && out.len() > MAX_STROKES / 4, "{}", out.len());
        let last_y = out.iter().map(|s| s.points[0].y).fold(0.0, f64::max);
        assert!(last_y > 55.0, "strokes reach the end of the drag: {last_y}");
    }

    /// Hostile input: huge, NaN or empty values stay bounded and never panic.
    #[test]
    fn bounded_on_hostile_input() {
        let src = edge();
        let cur = Surface::new(PixelFormat::RGBA8);
        let long: Vec<(f64, f64)> = (0..10_000).map(|i| (f64::from(i % 64), f64::from(i / 160))).collect();
        let out = art_strokes(&src, &cur, &stroke(&long, 1.0), &settings(ArtStyle::LooseCurlLong, 500.0), CANVAS, 5);
        assert!(out.len() <= MAX_STROKES && out.iter().all(|s| s.points.len() <= MAX_STEPS));
        for area in [f64::NAN, f64::INFINITY, -5.0, 1e300] {
            let s = ArtSettings { style: ArtStyle::TightLong, area, tolerance: f32::NAN };
            let _ = art_strokes(&src, &cur, &stroke(&[(1e300, f64::NAN), (5.0, 5.0)], 5000.0), &s, CANVAS, 0);
        }
        assert!(art_strokes(&src, &cur, &stroke(&[], 4.0), &settings(ArtStyle::Dab, 10.0), CANVAS, 0).is_empty());
    }
}
