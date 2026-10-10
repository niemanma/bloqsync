//! Data-driven LED animations ("effects").
//!
//! The model is deliberately small and composable, so both the shipped and the
//! user-built animations use exactly the same schema:
//!
//! * A **pattern** is a *chain of points* on a 2D colour board (Hue × Saturation).
//!   Each point stores a hue/saturation, a brightness and the **vector** that
//!   leads to the next point (its duration and interpolation mode). The chain is
//!   closed, so the last point connects back to the first.
//! * A **movement** decides how that pattern is mapped onto the strip over time:
//!   stationary (the whole strip shows the pattern as it plays over time),
//!   rotating left/right (cyclic) or marching left/right (the pattern travels
//!   across and the area outside it stays dark).
//! * `speed` is a fraction of the maximum rate (one full rotation per second)
//!   and `cycles` is the loop length — rotations per loop for moving effects,
//!   seconds for stationary ones.
//!
//! A **vector** interpolates from its point to the next. In `swift` mode it
//! blends the two endpoint colours directly (so non-adjacent colours can jump);
//! otherwise it walks the straight path on the colour board (interpolating hue
//! and saturation), passing through the colours that lie on the way.
//!
//! The strip is treated as a **ring** of `n` LEDs; `field(phase)` yields the
//! colour of the pattern at a cycle position in `0..1`.

use crate::protocol::Rgb;
use serde::{Deserialize, Serialize};

/// Largest LED count we will render for (mirrors the protocol limit).
pub const MAX_ANIM_LEDS: usize = 254;

/// The movement modes, in the order they are offered in the UI.
pub const MOVEMENT_IDS: &[&str] = &[
    "stationary",
    "rotate_right",
    "rotate_left",
    "march_right",
    "march_left",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Movement {
    Stationary,
    RotateRight,
    RotateLeft,
    MarchRight,
    MarchLeft,
}

impl Movement {
    pub fn from_id(id: &str) -> Movement {
        match id {
            "stationary" => Movement::Stationary,
            "rotate_left" => Movement::RotateLeft,
            "march_right" => Movement::MarchRight,
            "march_left" => Movement::MarchLeft,
            _ => Movement::RotateRight,
        }
    }

    pub fn is_moving(&self) -> bool {
        !matches!(self, Movement::Stationary)
    }
}

/// One point on the colour board plus the vector leading to the next point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Point {
    /// Hue on the colour board, `0..1`.
    pub hue: f32,
    /// Saturation on the colour board, `0..1`.
    pub sat: f32,
    /// Brightness (value), `0..1`.
    pub brightness: f32,
    /// Relative duration of the vector leaving this point (any positive weight;
    /// all weights are normalised against their sum).
    pub duration: f32,
    /// `true` = blend the endpoint colours directly (jump); `false` = walk the
    /// colour-board path.
    pub swift: bool,
}

impl Default for Point {
    fn default() -> Self {
        Point {
            hue: 0.08,
            sat: 0.85,
            brightness: 1.0,
            duration: 1.0,
            swift: false,
        }
    }
}

/// A complete, user-facing animation definition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Animation {
    /// Stable identifier (used for built-in/user overrides).
    pub id: String,
    pub name: String,
    /// Movement id (see [`MOVEMENT_IDS`]).
    pub movement: String,
    /// Fraction of the maximum rate (`0..1`; max = one rotation per second).
    pub speed: f32,
    /// Loop length: rotations per loop when moving, seconds when stationary.
    pub cycles: f32,
    /// Master brightness, `0..1`.
    pub brightness: f32,
    /// Target update rate towards the bar.
    pub fps: u32,
    /// The pattern: a closed chain of points.
    pub points: Vec<Point>,
}

impl Default for Animation {
    fn default() -> Self {
        Animation {
            id: String::new(),
            name: String::new(),
            movement: "rotate_right".to_string(),
            speed: 0.25,
            cycles: 1.0,
            brightness: 0.9,
            fps: 30,
            points: Vec::new(),
        }
    }
}

impl Animation {
    pub fn valid(&self) -> bool {
        !self.id.trim().is_empty() && !self.name.trim().is_empty() && !self.points.is_empty()
    }

    pub fn movement(&self) -> Movement {
        Movement::from_id(&self.movement)
    }
}

/// Renders [`Animation`]s over time.
pub struct Animator {
    time: f32,
}

impl Default for Animator {
    fn default() -> Self {
        Self::new()
    }
}

impl Animator {
    pub fn new() -> Self {
        Animator { time: 0.0 }
    }

    /// Advance by `dt` seconds and render `n` LED colours.
    pub fn render(&mut self, dt: f32, n: usize, a: &Animation) -> Vec<Rgb> {
        let n = n.min(MAX_ANIM_LEDS);
        if n == 0 {
            return Vec::new();
        }
        let dt = dt.clamp(0.0, 0.1);
        self.time += dt;

        let movement = a.movement();
        let phase = if movement.is_moving() {
            // One rotation per second at speed 1.
            self.time * a.speed.clamp(0.0, 4.0)
        } else {
            self.time / a.cycles.max(0.1)
        };
        let ph = phase.rem_euclid(1.0);

        if !a.valid() {
            return vec![[0, 0, 0]; n];
        }

        (0..n)
            .map(|i| {
                let x = i as f32 / n as f32;
                match movement {
                    Movement::Stationary => field(a, ph),
                    Movement::RotateRight => field(a, (x - ph).rem_euclid(1.0)),
                    Movement::RotateLeft => field(a, (x + ph).rem_euclid(1.0)),
                    Movement::MarchRight => {
                        let p = x - ph;
                        if (0.0..1.0).contains(&p) { field(a, p) } else { [0, 0, 0] }
                    }
                    Movement::MarchLeft => {
                        let p = x + ph;
                        if (0.0..1.0).contains(&p) { field(a, p) } else { [0, 0, 0] }
                    }
                }
            })
            .collect()
    }
}

/// Evaluate the pattern at cycle position `phase` (`0..1`).
pub fn field(a: &Animation, phase: f32) -> Rgb {
    let pts = &a.points;
    if pts.is_empty() {
        return [0, 0, 0];
    }
    if pts.len() == 1 {
        return scale(point_rgb(&pts[0]), a.brightness);
    }
    let total: f32 = pts.iter().map(|p| p.duration.max(0.0)).sum();
    if total <= f32::EPSILON {
        return scale(point_rgb(&pts[0]), a.brightness);
    }
    let mut ph = phase.rem_euclid(1.0) * total;
    let last = pts.len() - 1;
    for (i, p) in pts.iter().enumerate() {
        let d = p.duration.max(0.0);
        if ph < d || i == last {
            let t = if d > 0.0 { (ph / d).clamp(0.0, 1.0) } else { 0.0 };
            let next = &pts[(i + 1) % pts.len()];
            return scale(mix_points(p, next, t, p.swift), a.brightness);
        }
        ph -= d;
    }
    scale(point_rgb(&pts[last]), a.brightness)
}

/// The colour of a single point (brightness applied).
fn point_rgb(p: &Point) -> Rgb {
    hsv(p.hue, p.sat.clamp(0.0, 1.0), p.brightness.clamp(0.0, 1.0))
}

/// Interpolate between two points. `swift` blends the endpoint colours in RGB
/// directly; otherwise hue and saturation are interpolated along the board.
fn mix_points(a: &Point, b: &Point, t: f32, swift: bool) -> Rgb {
    if swift {
        mix(point_rgb(a), point_rgb(b), t)
    } else {
        hsv(
            lerp(a.hue, b.hue, t),
            lerp(a.sat, b.sat, t).clamp(0.0, 1.0),
            lerp(a.brightness, b.brightness, t).clamp(0.0, 1.0),
        )
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn scale(c: Rgb, v: f32) -> Rgb {
    let v = v.clamp(0.0, 1.0);
    [
        (c[0] as f32 * v).round().clamp(0.0, 255.0) as u8,
        (c[1] as f32 * v).round().clamp(0.0, 255.0) as u8,
        (c[2] as f32 * v).round().clamp(0.0, 255.0) as u8,
    ]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
    ]
}

/// HSV -> RGB. `h`, `s`, `v` are in `0..=1` (`h` wraps).
pub fn hsv(h: f32, s: f32, v: f32) -> Rgb {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(movement: &str, points: Vec<Point>) -> Animation {
        Animation {
            id: "t".into(),
            name: "t".into(),
            movement: movement.into(),
            points,
            ..Default::default()
        }
    }

    fn pt(hue: f32, brightness: f32, duration: f32) -> Point {
        Point { hue, sat: 1.0, brightness, duration, swift: false }
    }

    #[test]
    fn movement_ids_are_valid_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for id in MOVEMENT_IDS {
            assert!(seen.insert(*id));
        }
        assert!(!Movement::from_id("stationary").is_moving());
        for id in MOVEMENT_IDS.iter().filter(|i| **i != "stationary") {
            assert!(Movement::from_id(id).is_moving(), "{id}");
        }
    }

    #[test]
    fn animation_needs_points_to_be_valid() {
        let mut a = Animation::default();
        assert!(!a.valid());
        a.id = "x".into();
        a.name = "x".into();
        a.points.push(Point::default());
        assert!(a.valid());
    }

    #[test]
    fn every_movement_renders_requested_count() {
        for m in MOVEMENT_IDS {
            let mut runner = Animator::new();
            let a = anim(m, vec![pt(0.0, 1.0, 1.0), pt(0.5, 0.5, 1.0)]);
            assert_eq!(runner.render(0.033, 54, &a).len(), 54, "{m}");
        }
    }

    #[test]
    fn empty_or_invalid_is_rendered_black() {
        let mut runner = Animator::new();
        let mut a = anim("rotate_right", vec![]);
        assert!(runner.render(0.033, 4, &a).iter().all(|c| *c == [0, 0, 0]));
        a.points = vec![pt(0.3, 1.0, 1.0)];
        a.name = String::new(); // invalid
        assert!(runner.render(0.033, 4, &a).iter().all(|c| *c == [0, 0, 0]));
    }

    #[test]
    fn zero_leds_yields_empty() {
        let mut runner = Animator::new();
        let a = anim("rotate_right", vec![pt(0.0, 1.0, 1.0)]);
        assert!(runner.render(0.033, 0, &a).is_empty());
    }

    #[test]
    fn led_count_is_capped() {
        let mut runner = Animator::new();
        let a = anim("stationary", vec![pt(0.0, 1.0, 1.0)]);
        assert_eq!(runner.render(0.033, 1000, &a).len(), MAX_ANIM_LEDS);
    }

    #[test]
    fn single_point_is_scaled_colour() {
        let p = Point { hue: 0.0, sat: 1.0, brightness: 1.0, duration: 1.0, swift: false };
        let a = Animation { brightness: 0.5, ..anim("stationary", vec![p]) };
        let cols = Animator::new().render(0.016, 4, &a);
        assert!(cols.iter().all(|c| *c == [128, 0, 0]));
    }

    #[test]
    fn master_brightness_zero_is_black() {
        let a = Animation { brightness: 0.0, ..anim("rotate_right", vec![pt(0.0, 1.0, 1.0)]) };
        let mut runner = Animator::new();
        assert!(runner.render(0.033, 8, &a).iter().all(|c| *c == [0, 0, 0]));
    }

    #[test]
    fn stationary_is_uniform() {
        let a = anim("stationary", vec![pt(0.1, 1.0, 1.0), pt(0.6, 0.4, 1.0)]);
        let cols = Animator::new().render(0.033, 16, &a);
        assert!(cols.iter().all(|c| *c == cols[0]));
    }

    #[test]
    fn moving_effects_change_over_time() {
        for m in ["rotate_right", "rotate_left", "march_right", "march_left"] {
            let a = anim(m, vec![pt(0.0, 1.0, 1.0), pt(0.5, 0.2, 1.0), pt(0.8, 1.0, 1.0)]);
            let mut runner = Animator::new();
            let first = runner.render(0.033, 40, &a);
            let mut later = first.clone();
            for _ in 0..20 {
                later = runner.render(0.033, 40, &a);
            }
            assert_ne!(first, later, "{m} is static");
        }
    }

    #[test]
    fn swift_and_normal_reach_different_midcolours() {
        // Red (hue 0) -> blue (hue 2/3). Normal walks through green; swift
        // blends directly and stays between red and blue.
        let red = Point { hue: 0.0, sat: 1.0, brightness: 1.0, duration: 1.0, swift: false };
        let blue = Point { hue: 2.0 / 3.0, sat: 1.0, brightness: 1.0, duration: 1.0, swift: false };
        let normal = mix_points(&red, &blue, 0.5, false);
        let swift = mix_points(&red, &blue, 0.5, true);
        assert!(normal[1] > 150, "normal should pass through green: {normal:?}");
        assert!(swift[1] < 100, "swift should not be green: {swift:?}");
        assert!(swift[0] > 100 && swift[2] > 100, "swift blend is red/blue: {swift:?}");
    }

    #[test]
    fn durations_are_normalised() {
        let a = anim("stationary", vec![pt(0.0, 1.0, 1.0), pt(1.0 / 3.0, 1.0, 3.0)]);
        let b = anim("stationary", vec![pt(0.0, 1.0, 10.0), pt(1.0 / 3.0, 1.0, 30.0)]);
        let fa = Animator::new().render(0.0, 1, &a);
        let fb = Animator::new().render(0.0, 1, &b);
        assert_eq!(fa, fb);
    }

    #[test]
    fn march_has_a_dark_gap() {
        let a = anim("march_right", vec![pt(0.0, 1.0, 1.0)]);
        let mut runner = Animator::new();
        // Advance until the pattern has (partly) left the strip.
        let mut seen_gap = false;
        for _ in 0..30 {
            let cols = runner.render(0.05, 20, &a);
            if cols.iter().any(|c| *c == [0, 0, 0]) {
                seen_gap = true;
                break;
            }
        }
        assert!(seen_gap);
    }

    #[test]
    fn hsv_matches_primary_colours() {
        assert_eq!(hsv(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(hsv(1.0 / 3.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(hsv(2.0 / 3.0, 1.0, 1.0), [0, 0, 255]);
        assert_eq!(hsv(0.0, 0.0, 0.0), [0, 0, 0]);
    }

    #[test]
    fn round_trips_json_with_defaults() {
        let a = Animation {
            id: "x".into(),
            name: "Test".into(),
            movement: "march_left".into(),
            points: vec![
                Point { hue: 0.2, sat: 0.5, brightness: 0.8, duration: 2.0, swift: true },
                Point::default(),
            ],
            ..Default::default()
        };
        let json = serde_json::to_string(&a).unwrap();
        let back: Animation = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);

        let partial: Animation = serde_json::from_str(r#"{"id":"a","name":"A"}"#).unwrap();
        assert_eq!(partial.speed, 0.25);
        assert_eq!(partial.movement, "rotate_right");
        assert!(partial.points.is_empty());
    }
}