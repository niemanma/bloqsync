//! Audio-reactive analysis for the "cinema" mode (DRM/Netflix etc.).
//!
//! Captures the default sink's monitor via PulseAudio/PipeWire and derives a
//! rich feature set from the audio only: per-band energy, overall energy,
//! spectral centroid, spectral flatness, onset/transient pulses and stereo
//! balance. These feed the colour mapping in the cinema engine.

use anyhow::{Context, Result};
use libpulse_binding as pulse;
use libpulse_simple_binding::Simple;
use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const SAMPLE_RATE: usize = 48_000;
pub const CHANNELS: u8 = 2;
const FFT_SIZE: usize = 1024;
const HOP: usize = 256;
pub const NBANDS: usize = 6;
const BAND_EDGES: [f32; NBANDS + 1] = [0.0, 60.0, 150.0, 400.0, 1200.0, 4000.0, 16_000.0];

#[derive(Clone, Copy, Debug, Default)]
pub struct Spectrum {
    pub bands: [f32; NBANDS],
    pub energy: f32,
    /// Slow (scene-level) energy, ~seconds.
    pub energy_slow: f32,
    /// Contrast: short-term vs long-term energy, -1..1 (swells/drops).
    pub contrast: f32,
    pub rms: f32,
    pub centroid: f32,
    pub flatness: f32,
    pub onset: f32,
    pub stereo: f32,
    pub active: bool,
}

pub type SpectrumSlot = Arc<Mutex<Option<Spectrum>>>;

pub struct AudioHandle {
    running: Arc<AtomicBool>,
    pub spectrum: SpectrumSlot,
    pub source: String,
    _thread: std::thread::JoinHandle<()>,
}

impl AudioHandle {
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for AudioHandle {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// Resolve `<default_sink>.monitor` via libpulse.
pub fn default_monitor_source() -> Option<String> {
    let mut ml = pulse::mainloop::standard::Mainloop::new()?;
    let mut ctx = pulse::context::Context::new(&ml, "bloqsync")?;
    ctx.connect(None, pulse::context::FlagSet::NOFLAGS, None)
        .ok()?;
    loop {
        match ctx.get_state() {
            pulse::context::State::Ready => break,
            pulse::context::State::Failed | pulse::context::State::Terminated => return None,
            _ => {
                ml.iterate(false);
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
    let sink: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    {
        let sink2 = sink.clone();
        let op = ctx.introspect().get_server_info(move |info| {
            if let Some(name) = info.default_sink_name.as_ref() {
                *sink2.lock().unwrap() = Some(name.to_string());
            }
        });
        while op.get_state() == pulse::operation::State::Running {
            ml.iterate(false);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    let name = sink.lock().unwrap().clone()?;
    Some(format!("{name}.monitor"))
}

/// Start analyzing the given source (or the default monitor).
pub fn start(source: Option<String>) -> Result<AudioHandle> {
    let src = match source {
        Some(s) if !s.is_empty() => s,
        _ => default_monitor_source().context("no default sink monitor found")?,
    };
    let running = Arc::new(AtomicBool::new(true));
    let spectrum: SpectrumSlot = Arc::new(Mutex::new(None));
    let running_t = running.clone();
    let spectrum_t = spectrum.clone();
    let src_t = src.clone();
    let handle = std::thread::Builder::new()
        .name("bloqsync-audio".into())
        .spawn(move || {
            if let Err(e) = run(&src_t, &running_t, &spectrum_t) {
                eprintln!("bloqsync: audio capture stopped: {e:#}");
            }
            running_t.store(false, Ordering::Relaxed);
        })
        .context("spawn audio thread")?;
    Ok(AudioHandle {
        running,
        spectrum,
        source: src,
        _thread: handle,
    })
}

fn run(source: &str, running: &AtomicBool, slot: &SpectrumSlot) -> Result<()> {
    let spec = pulse::sample::Spec {
        format: pulse::sample::Format::S16le,
        channels: CHANNELS,
        rate: SAMPLE_RATE as u32,
    };
    // Small buffers -> low latency.
    let fragsize = (HOP * CHANNELS as usize * 2) as u32;
    let attr = pulse::def::BufferAttr {
        maxlength: fragsize * 4,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize,
    };
    let simple = Simple::new(
        None,
        "bloqsync",
        pulse::stream::Direction::Record,
        Some(source),
        "bloqsync cinema",
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| anyhow::anyhow!("pa_simple: {e}"))?;

    let mut analyzer = Analyzer::new();
    let mut raw = vec![0u8; HOP * CHANNELS as usize * 2];
    while running.load(Ordering::Relaxed) {
        if let Err(e) = simple.read(&mut raw) {
            return Err(anyhow::anyhow!("read: {e}"));
        }
        for frame in raw.chunks_exact(CHANNELS as usize * 2) {
            let l = i16::from_le_bytes([frame[0], frame[1]]) as f32 / 32768.0;
            let r = i16::from_le_bytes([frame[2], frame[3]]) as f32 / 32768.0;
            if let Some(s) = analyzer.push(l, r) {
                *slot.lock().unwrap() = Some(s);
            }
        }
    }
    Ok(())
}

struct Analyzer {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    window: Vec<f32>,
    mono: Vec<f32>,
    left: Vec<f32>,
    right: Vec<f32>,
    fft_buf: Vec<Complex<f32>>,
    prev_mag: Vec<f32>,
    band_max: [f32; NBANDS],
    rms_max: f32,
    energy_fast: f32,
    energy_slow: f32,
    flux_avg: f32,
    onset_pulse: f32,
    smooth: Spectrum,
}

impl Analyzer {
    fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                let x = std::f32::consts::PI * i as f32 / (FFT_SIZE as f32 - 1.0);
                x.sin().powi(2)
            })
            .collect();
        Analyzer {
            fft,
            window,
            mono: Vec::with_capacity(FFT_SIZE),
            left: Vec::with_capacity(FFT_SIZE),
            right: Vec::with_capacity(FFT_SIZE),
            fft_buf: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            prev_mag: vec![0.0; FFT_SIZE / 2],
            band_max: [1e-6; NBANDS],
            rms_max: 1e-6,
            energy_fast: 0.0,
            energy_slow: 0.0,
            flux_avg: 0.0,
            onset_pulse: 0.0,
            smooth: Spectrum::default(),
        }
    }

    fn push(&mut self, l: f32, r: f32) -> Option<Spectrum> {
        self.mono.push((l + r) * 0.5);
        self.left.push(l);
        self.right.push(r);
        if self.mono.len() < FFT_SIZE {
            return None;
        }
        let spec = self.analyze();
        self.mono.drain(0..HOP);
        self.left.drain(0..HOP);
        self.right.drain(0..HOP);
        Some(spec)
    }

    fn analyze(&mut self) -> Spectrum {
        // Windowed FFT of the mono mix.
        for i in 0..FFT_SIZE {
            self.fft_buf[i] = Complex::new(self.mono[i] * self.window[i], 0.0);
        }
        self.fft.process(&mut self.fft_buf);
        let half = FFT_SIZE / 2;
        let mut mag = [0.0f32; FFT_SIZE / 2];
        for i in 0..half {
            mag[i] = self.fft_buf[i].norm();
        }
        let bin_hz = SAMPLE_RATE as f32 / FFT_SIZE as f32;
        let mut bands = [0.0f32; NBANDS];
        for b in 0..NBANDS {
            let lo = (BAND_EDGES[b] / bin_hz) as usize;
            let hi = ((BAND_EDGES[b + 1] / bin_hz) as usize).min(half);
            let mut e = 0.0;
            for i in lo..hi.max(lo + 1) {
                e += mag[i] * mag[i];
            }
            bands[b] = (e / (hi - lo).max(1) as f32).sqrt();
        }
        // Spectral centroid
        let (mut num, mut den) = (0.0f32, 0.0f32);
        for i in 1..half {
            num += i as f32 * mag[i];
            den += mag[i];
        }
        let centroid = if den > 1e-9 {
            (num / den) / half as f32
        } else {
            self.smooth.centroid
        };
        // Spectral flatness
        let (mut log_sum, mut arith) = (0.0f32, 0.0f32);
        for i in 1..half {
            let p = mag[i] * mag[i] + 1e-12;
            log_sum += p.ln();
            arith += p;
        }
        let m = (half - 1) as f32;
        let geo = (log_sum / m).exp();
        let flatness = (geo / (arith / m + 1e-12)).clamp(0.0, 1.0);
        // Spectral flux -> onset
        let mut flux = 0.0;
        for i in 0..half {
            let d = mag[i] - self.prev_mag[i];
            if d > 0.0 {
                flux += d;
            }
            self.prev_mag[i] = mag[i];
        }
        self.flux_avg = 0.98 * self.flux_avg + 0.02 * flux;
        let raw_on = ((flux - self.flux_avg * 1.4) / (self.flux_avg + 1e-6)).clamp(0.0, 1.0);
        let decay = (-(HOP as f32) / (SAMPLE_RATE as f32) / 0.15).exp();
        self.onset_pulse = (self.onset_pulse * decay).max(raw_on);
        // Stereo balance
        let el: f32 = self.left.iter().map(|x| x * x).sum();
        let er: f32 = self.right.iter().map(|x| x * x).sum();
        let stereo = ((el - er) / (el + er + 1e-9)).clamp(-1.0, 1.0);
        // RMS
        let rms = (self.mono.iter().map(|x| x * x).sum::<f32>() / FFT_SIZE as f32).sqrt();
        // Adaptive normalisation
        for b in 0..NBANDS {
            self.band_max[b] = (self.band_max[b] * 0.999).max(bands[b]);
            bands[b] = (bands[b] / (self.band_max[b] + 1e-9)).clamp(0.0, 1.0);
        }
        // Energy = loudness (RMS) with adaptive gain, independent of timbre.
        self.rms_max = (self.rms_max * 0.9997).max(rms);
        let energy = (rms / (self.rms_max + 1e-9)).clamp(0.0, 1.0);
        // Context: fast (~0.15 s) vs slow (~3 s) loudness -> contrast/dynamics.
        let dt = HOP as f32 / SAMPLE_RATE as f32;
        let a_fast = 1.0 - (-dt / 0.15).exp();
        let a_slow = 1.0 - (-dt / 3.0).exp();
        self.energy_fast += a_fast * (energy - self.energy_fast);
        self.energy_slow += a_slow * (energy - self.energy_slow);
        let contrast = ((self.energy_fast - self.energy_slow) / (self.energy_slow + 0.08))
            .clamp(-1.0, 1.0);
        let active = rms > 1e-4 && energy > 0.02;
        // Smoothing (fast attack, slow release)
        let a = 0.5;
        self.smooth.bands = std::array::from_fn(|i| {
            let prev = self.smooth.bands[i];
            prev + a * (bands[i] - prev)
        });
        self.smooth.energy = self.energy_fast;
        self.smooth.energy_slow += 0.1 * (self.energy_slow - self.smooth.energy_slow);
        self.smooth.contrast += 0.15 * (contrast - self.smooth.contrast);
        self.smooth.centroid += 0.12 * (centroid - self.smooth.centroid);
        self.smooth.flatness += 0.2 * (flatness - self.smooth.flatness);
        self.smooth.onset = self.onset_pulse;
        self.smooth.rms = rms;
        self.smooth.stereo += 0.3 * (stereo - self.smooth.stereo);
        self.smooth.active = active;
        self.smooth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(i: usize, hz: f32, amp: f32) -> f32 {
        (std::f32::consts::TAU * hz * i as f32 / SAMPLE_RATE as f32).sin() * amp
    }

    fn feed(a: &mut Analyzer, n: usize, f: impl Fn(usize) -> (f32, f32)) -> Option<Spectrum> {
        let mut last = None;
        for i in 0..n {
            let (l, r) = f(i);
            if let Some(s) = a.push(l, r) {
                last = Some(s);
            }
        }
        last
    }

    #[test]
    fn first_spectrum_needs_a_full_window() {
        let mut a = Analyzer::new();
        for _ in 0..FFT_SIZE - 1 {
            assert!(a.push(0.0, 0.0).is_none());
        }
        assert!(a.push(0.0, 0.0).is_some());
    }

    #[test]
    fn spectra_advance_by_one_hop() {
        let mut a = Analyzer::new();
        for _ in 0..FFT_SIZE {
            a.push(0.0, 0.0);
        }
        for _ in 0..HOP - 1 {
            assert!(a.push(0.0, 0.0).is_none());
        }
        assert!(a.push(0.0, 0.0).is_some());
    }

    #[test]
    fn silence_stays_inactive() {
        let mut a = Analyzer::new();
        let s = feed(&mut a, FFT_SIZE, |_| (0.0, 0.0)).unwrap();
        assert!(!s.active);
        assert_eq!(s.rms, 0.0);
        assert_eq!(s.energy, 0.0);
        assert_eq!(s.bands, [0.0; NBANDS]);
    }

    #[test]
    fn loud_tone_is_active_and_bounded() {
        let mut a = Analyzer::new();
        let s = feed(&mut a, FFT_SIZE * 2, |i| {
            let v = tone(i, 1000.0, 0.5);
            (v, v)
        })
        .unwrap();
        assert!(s.active);
        assert!(s.energy > 0.0 && s.energy <= 1.0);
        assert!((-1.0..=1.0).contains(&s.contrast));
        assert!((-1.0..=1.0).contains(&s.stereo));
        assert!((0.0..=1.0).contains(&s.flatness));
        assert!(s.centroid >= 0.0);
        for b in s.bands {
            assert!((0.0..=1.0).contains(&b), "band out of range: {b}");
        }
    }

    #[test]
    fn stereo_balance_sign_follows_the_louder_channel() {
        let mut left = Analyzer::new();
        let s = feed(&mut left, FFT_SIZE * 2, |i| (tone(i, 1000.0, 0.5), 0.0)).unwrap();
        assert!(s.stereo > 0.0);

        let mut right = Analyzer::new();
        let s = feed(&mut right, FFT_SIZE * 2, |i| (0.0, tone(i, 1000.0, 0.5))).unwrap();
        assert!(s.stereo < 0.0);
    }

    #[test]
    fn extreme_input_does_not_panic() {
        let mut a = Analyzer::new();
        let s = feed(&mut a, FFT_SIZE * 3, |i| {
            if i % 2 == 0 {
                (1.0, -1.0)
            } else {
                (-1.0, 1.0)
            }
        });
        assert!(s.is_some());
    }
}
