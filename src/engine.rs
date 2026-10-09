//! The real-time sync engine: latest captured frame -> border sampling ->
//! smoothing -> `setSyncScreen` frames on the bar.

use crate::capture::FrameSlot;
use crate::device::Device;
use crate::protocol::Rgb;
use crate::filters::{FilterKind, FilterState};
use crate::sampling::{sample_border, smooth, Layout};
use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct SyncConfig {
    pub layout: Layout,
    /// Target update rate towards the bar.
    pub fps: u32,
    /// Exponential smoothing factor (1.0 = raw, lower = smoother).
    pub smoothing: f32,
    /// Pause between section-frames of one update. The firmware rolls/flickers
    /// if frames arrive back-to-back; ~3ms is stable.
    pub inter_frame_gap: Duration,
    /// Mirror the strip: reverse LED order (fixes left/right if mounted
    /// the other way round).
    pub reverse: bool,
    /// Only push a new frame when a channel differs by more than this
    /// (0 = send every new frame). Saves USB traffic and CPU; imperceptible.
    pub change_threshold: u8,
    /// Maximum HID reports per update. 1 = single atomic report; 0 = unlimited
    /// (raw multi-frame, best colour detail).
    pub max_frames: usize,
    /// Optional temporal filter to reduce flicker.
    pub filter: FilterKind,
    /// Strength/parameter of the selected filter.
    pub filter_strength: u8,
    /// Re-send the last frame at least this often (keeps the bar from idling).
    pub min_refresh: Duration,
}

impl Default for SyncConfig {
    fn default() -> Self {
        SyncConfig {
            layout: Layout::default(),
            fps: 24,
            smoothing: 0.22,
            inter_frame_gap: Duration::from_millis(3),
            reverse: false,
            change_threshold: 0,
            max_frames: 0,
            filter: FilterKind::None,
            filter_strength: 8,
            min_refresh: Duration::from_millis(500),
        }
    }
}

pub struct SyncHandle {
    running: Arc<AtomicBool>,
    pub sent: Arc<AtomicU64>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl SyncHandle {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.handle.lock().unwrap().take() {
            let _ = h.join();
        }
    }
}

impl Drop for SyncHandle {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// True if any channel differs by more than `threshold` (or lengths differ).
#[inline]
fn changed(prev: &[Rgb], next: &[Rgb], threshold: u8) -> bool {
    if prev.len() != next.len() {
        return true;
    }
    if threshold == 0 {
        return prev != next;
    }
    for (a, b) in prev.iter().zip(next.iter()) {
        for i in 0..3 {
            if (a[i] as i16 - b[i] as i16).unsigned_abs() as u8 > threshold {
                return true;
            }
        }
    }
    false
}

/// Start syncing one monitor (`slot`) to one bar (`device`).
pub fn spawn(device: Arc<Device>, slot: FrameSlot, cfg: SyncConfig) -> Result<SyncHandle> {
    let running = Arc::new(AtomicBool::new(true));
    let sent = Arc::new(AtomicU64::new(0));
    let running_thread = running.clone();
    let sent_thread = sent.clone();

    let handle = std::thread::Builder::new()
        .name("bloqsync-engine".into())
        .spawn(move || {
            let period = Duration::from_secs_f64(1.0 / cfg.fps.max(1) as f64);
            let mut prev: Vec<Rgb> = Vec::new();
            let mut last_sent: Vec<Rgb> = Vec::new();
            let mut filter_state = FilterState::default();
            let mut last_seq: u64 = u64::MAX;
            let mut last_send = Instant::now();
            let mut errs: u32 = 0;

            while running_thread.load(Ordering::Relaxed) {
                let tick = Instant::now();

                let frame = slot.frame.lock().ok().and_then(|g| g.clone());
                if let Some(frame) = frame {
                    // Mark as consumed so the capture thread may produce again.
                    slot.consumed.store(frame.seq, Ordering::Relaxed);
                    let new_frame = frame.seq != last_seq;
                    if new_frame {
                        last_seq = frame.seq;
                        let mut cols = sample_border(&frame, &cfg.layout);
                        if cfg.reverse {
                            cols.reverse();
                        }
                        let out = if prev.len() == cols.len() && cfg.smoothing < 1.0 {
                            smooth(&prev, &cols, cfg.smoothing)
                        } else {
                            cols
                        };
                        // Optional temporal filter (flicker tool-box). The
                        // smoothing state (`prev`) keeps following the input.
                        let disp = filter_state.process(
                            &out,
                            cfg.filter,
                            cfg.filter_strength,
                            &last_sent,
                        );
                        if changed(&last_sent, &disp, cfg.change_threshold) {
                            match device.send_colors_fit(&disp, cfg.inter_frame_gap, cfg.max_frames)
                            {
                                Ok(()) => {
                                    sent_thread.fetch_add(1, Ordering::Relaxed);
                                    last_send = Instant::now();
                                    last_sent = disp.clone();
                                    errs = 0;
                                }
                                Err(e) => {
                                    errs += 1;
                                    if errs <= 2 {
                                        eprintln!("bloqsync: send failed ({errs}): {e:#}");
                                    }
                                    // Device likely unplugged: stop so a
                                    // watchdog can reconnect cleanly.
                                    if errs >= 10 {
                                        eprintln!("bloqsync: device lost, stopping engine");
                                        running_thread.store(false, Ordering::Relaxed);
                                        break;
                                    }
                                    std::thread::sleep(Duration::from_millis(50));
                                }
                            }
                        }
                        prev = out;
                    } else if !last_sent.is_empty() && last_send.elapsed() >= cfg.min_refresh {
                        // Safety re-send for an idle screen (the device holds
                        // state, so this is only a safety net).
                        if device.send_colors_fit(&last_sent, cfg.inter_frame_gap, cfg.max_frames).is_ok() {
                            last_send = Instant::now();
                        }
                    }
                }

                let elapsed = tick.elapsed();
                if elapsed < period {
                    std::thread::sleep(period - elapsed);
                }
            }
        })
        .map_err(|e| anyhow::anyhow!("spawn engine: {e}"))?;

    Ok(SyncHandle {
        running,
        sent,
        handle: Mutex::new(Some(handle)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_frames_are_unchanged() {
        let a = [[10, 20, 30], [40, 50, 60]];
        assert!(!changed(&a, &a, 0));
        assert!(!changed(&a, &a, 5));
    }

    #[test]
    fn length_change_is_always_significant() {
        let a = [[0, 0, 0]];
        let b = [[0, 0, 0], [0, 0, 0]];
        assert!(changed(&a, &b, 0));
        assert!(changed(&a, &b, 255));
    }

    #[test]
    fn zero_threshold_detects_any_difference() {
        assert!(changed(&[[0, 0, 0]], &[[0, 0, 1]], 0));
    }

    #[test]
    fn threshold_ignores_small_differences() {
        assert!(!changed(&[[100, 100, 100]], &[[105, 100, 100]], 5));
        assert!(!changed(&[[100, 100, 100]], &[[100, 96, 100]], 5));
    }

    #[test]
    fn threshold_detects_larger_differences() {
        assert!(changed(&[[100, 100, 100]], &[[106, 100, 100]], 5));
        assert!(changed(&[[100, 100, 100]], &[[100, 90, 100]], 5));
    }
}
