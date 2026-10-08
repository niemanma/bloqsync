//! Cinema colour mapping: turn an audio [`Spectrum`] into LED colours.
//!
//! Concept: the user picks a **base colour** and a master brightness. The sound
//! only drives the *brightness* (loudness envelope + transient pulses) and a
//! subtle stereo balance — no hue changes, which keeps it cinematic instead of
//! "disco".

use crate::audio::Spectrum;
use crate::protocol::Rgb;

#[derive(Clone, Copy, Debug)]
pub struct CinemaParams {
    /// Fixed base colour.
    pub base: Rgb,
    /// Energy sensitivity (1.0 = neutral).
    pub sensitivity: f32,
    /// Master brightness 0..1.
    pub master: f32,
    /// Minimum brightness (so the light never goes fully dark).
    pub floor: f32,
    /// How much transient/onset pulses add on top (0 = none; no beat flicker).
    pub onset_gain: f32,
    /// Stereo spatial effect 0..1 (0 = flat).
    pub stereo_amount: f32,
    /// Smoothing time constant in seconds (higher = slower, more flowing).
    pub smooth_secs: f32,
    /// How strongly scene contrast (swells/drops) modulates brightness.
    pub contrast_gain: f32,
}

impl Default for CinemaParams {
    fn default() -> Self {
        CinemaParams {
            base: [255, 160, 60], // warm amber
            sensitivity: 1.0,
            master: 0.7,
            floor: 0.35,
            onset_gain: 0.0,
            stereo_amount: 0.0,
            smooth_secs: 0.6,
            contrast_gain: 0.5,
        }
    }
}

pub struct Cinema {
    env: f32,
    onset: f32,
}

impl Default for Cinema {
    fn default() -> Self {
        Self::new()
    }
}

impl Cinema {
    pub fn new() -> Self {
        Cinema { env: 0.0, onset: 0.0 }
    }

    pub fn render(&mut self, s: &Spectrum, n: usize, p: &CinemaParams) -> Vec<Rgb> {
        // Scene-level loudness as the base; contrast adds the dynamics
        // (swells/drops) so it flows with the scene, not the beat.
        let slow = (s.energy_slow * p.sensitivity).clamp(0.0, 1.0);
        let ct = (s.contrast * p.contrast_gain).clamp(-0.6, 0.6);
        let target = (p.floor + (1.0 - p.floor) * slow.powf(0.8) + ct).clamp(0.0, 1.0);
        let dt = 0.02f32; // render period (~50 fps)
        let tau = if target > self.env {
            p.smooth_secs.max(0.2)
        } else {
            (p.smooth_secs * 1.6).max(0.3)
        };
        let alpha = (1.0 - (-dt / tau).exp()).clamp(0.0, 1.0);
        self.env += alpha * (target - self.env);
        // Optional subtle accent (default off).
        self.onset = (self.onset * 0.8).max((s.onset * p.onset_gain).min(1.0));
        let v = (self.env + self.onset).clamp(0.0, 1.0) * p.master;

        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let pos = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.5 };
            let w = (1.0 + p.stereo_amount * s.stereo * (2.0 * pos - 1.0)).clamp(0.3, 1.5);
            let vv = (v * w).clamp(0.0, 1.0);
            out.push([
                (p.base[0] as f32 * vv) as u8,
                (p.base[1] as f32 * vv) as u8,
                (p.base[2] as f32 * vv) as u8,
            ]);
        }
        out
    }
}
