//! Optional temporal filters to reduce flicker. These are meant as a tuning
//! toolbox: pick one in the UI and compare. `None` = raw (best-so-far
//! behaviour).

use crate::protocol::Rgb;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FilterKind {
    /// No filtering (raw).
    None,
    /// Per-channel hysteresis: keep the last value unless it differs by more
    /// than `strength`.
    Deadband,
    /// Round each channel to a multiple of `strength`.
    Quantize,
    /// Quantize (stable, flicker-free) then smoothly follow it over time, so
    /// brightness transitions fade instead of stepping.
    QuantizeSmooth,
    /// Hysteresis + ramp: below the threshold nothing changes (no flicker);
    /// for real changes the colour glides smoothly towards the target.
    Smooth,
    /// Per-channel median over the last 3 frames.
    Median3,
    /// Per-channel median over the last 5 frames.
    Median5,
    /// Per-channel mean over the last 4 frames.
    Mean4,
    /// Diagnostic: invert all colours (unmistakably shows the filter runs).
    Test,
}

impl Default for FilterKind {
    fn default() -> Self {
        FilterKind::None
    }
}

impl FilterKind {
    pub fn label(self) -> &'static str {
        match self {
            FilterKind::None => "keine",
            FilterKind::Deadband => "Deadband (Hysterese)",
            FilterKind::Quantize => "Quantisieren",
            FilterKind::QuantizeSmooth => "Quantisieren + weich",
            FilterKind::Smooth => "Weich (Hysterese+Ramp)",
            FilterKind::Median3 => "Median 3",
            FilterKind::Median5 => "Median 5",
            FilterKind::Mean4 => "Mittelwert 4",
            FilterKind::Test => "Test (Negativ)",
        }
    }
}

/// Holds the temporal state for stateful filters.
#[derive(Default)]
pub struct FilterState {
    history: VecDeque<Vec<Rgb>>,
    ema: Vec<Rgb>,
    disp: Vec<Rgb>,
}

impl FilterState {
    pub fn reset(&mut self) {
        self.history.clear();
        self.ema.clear();
        self.disp.clear();
    }

    /// Apply the filter to `input`. `last_sent` is the previously displayed
    /// colour (needed by the Deadband filter). Returns the colour to display.
    pub fn process(
        &mut self,
        input: &[Rgb],
        kind: FilterKind,
        strength: u8,
        last_sent: &[Rgb],
    ) -> Vec<Rgb> {
        match kind {
            FilterKind::None => input.to_vec(),
            FilterKind::Deadband => {
                if last_sent.len() != input.len() {
                    return input.to_vec();
                }
                let thr = strength.max(1);
                input
                    .iter()
                    .zip(last_sent.iter())
                    .map(|(c, s)| {
                        [
                            snap(c[0], s[0], thr),
                            snap(c[1], s[1], thr),
                            snap(c[2], s[2], thr),
                        ]
                    })
                    .collect()
            }
            FilterKind::Quantize => {
                let q = strength.max(2);
                input
                    .iter()
                    .map(|c| [quant(c[0], q), quant(c[1], q), quant(c[2], q)])
                    .collect()
            }
            FilterKind::QuantizeSmooth => {
                let q = strength.max(2);
                let target: Vec<Rgb> = input
                    .iter()
                    .map(|c| [quant(c[0], q), quant(c[1], q), quant(c[2], q)])
                    .collect();
                if self.ema.len() != target.len() {
                    self.ema = target;
                } else {
                    let a = 0.35f32;
                    for (e, t) in self.ema.iter_mut().zip(target.iter()) {
                        for k in 0..3 {
                            let v = e[k] as f32 + (t[k] as f32 - e[k] as f32) * a;
                            e[k] = v.round().clamp(0.0, 255.0) as u8;
                        }
                    }
                }
                self.ema.clone()
            }
            FilterKind::Smooth => {
                let db = strength.max(1);
                let alpha = 0.30f32;
                if self.disp.len() != input.len() {
                    self.disp = input.to_vec();
                } else {
                    for (d, v) in self.disp.iter_mut().zip(input.iter()) {
                        for k in 0..3 {
                            let diff = v[k] as i16 - d[k] as i16;
                            if diff.unsigned_abs() as u8 > db {
                                let nv = d[k] as f32 + diff as f32 * alpha;
                                d[k] = nv.round().clamp(0.0, 255.0) as u8;
                            }
                        }
                    }
                }
                self.disp.clone()
            }
            FilterKind::Median3 => self.window(input, 3, WindowOp::Median),
            FilterKind::Median5 => self.window(input, 5, WindowOp::Median),
            FilterKind::Mean4 => self.window(input, 4, WindowOp::Mean),
            FilterKind::Test => input
                .iter()
                .map(|c| [255 - c[0], 255 - c[1], 255 - c[2]])
                .collect(),
        }
    }

    fn window(&mut self, input: &[Rgb], n: usize, op: WindowOp) -> Vec<Rgb> {
        self.history.push_back(input.to_vec());
        while self.history.len() > n {
            self.history.pop_front();
        }
        let len = input.len();
        let mut out = vec![[0u8; 3]; len];
        for i in 0..len {
            for k in 0..3 {
                let mut vals: Vec<u8> = self.history.iter().map(|f| f[i][k]).collect();
                match op {
                    WindowOp::Median => {
                        vals.sort_unstable();
                        out[i][k] = vals[vals.len() / 2];
                    }
                    WindowOp::Mean => {
                        let sum: u32 = vals.iter().map(|&v| v as u32).sum();
                        out[i][k] = (sum / vals.len() as u32) as u8;
                    }
                }
            }
        }
        out
    }
}

#[derive(Clone, Copy)]
enum WindowOp {
    Median,
    Mean,
}

#[inline]
fn snap(v: u8, reference: u8, thr: u8) -> u8 {
    if (v as i16 - reference as i16).unsigned_abs() as u8 <= thr {
        reference
    } else {
        v
    }
}

#[inline]
fn quant(v: u8, q: u8) -> u8 {
    let q = q as u16;
    let r = ((v as u16 + q / 2) / q) * q;
    r.min(255) as u8
}
