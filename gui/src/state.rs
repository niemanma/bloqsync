//! Shared application state held by Tauri and mutated by the background threads.

use crate::config::Ident;
use bloqsync::audio::AudioHandle;
use bloqsync::capture::Capture;
use bloqsync::device::Device;
use bloqsync::engine::SyncHandle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub(crate) struct BarRuntime {
    pub(crate) device: Arc<Device>,
    pub(crate) sync: SyncHandle,
}

pub(crate) struct IdentifyRuntime {
    pub(crate) running: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) thread: std::thread::JoinHandle<()>,
}

pub(crate) struct CinemaRuntime {
    pub(crate) audio: AudioHandle,
    pub(crate) running: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) devices: Vec<Arc<Device>>,
    pub(crate) thread: std::thread::JoinHandle<()>,
}

impl CinemaRuntime {
    pub(crate) fn stop(self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.audio.stop();
        let _ = self.thread.join();
        for d in &self.devices {
            let _ = d.set_persistent_color([255, 200, 100]);
        }
    }
}

/// A running LED animation (screen sync is paused while it plays).
pub(crate) struct AnimationRuntime {
    pub(crate) running: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) devices: Vec<Arc<Device>>,
    pub(crate) thread: std::thread::JoinHandle<()>,
}

impl AnimationRuntime {
    pub(crate) fn stop(self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = self.thread.join();
        for d in &self.devices {
            let _ = d.set_persistent_color([255, 200, 100]);
        }
    }
}

#[derive(Default)]
pub(crate) struct AppState {
    pub(crate) capture: Mutex<Option<Arc<Capture>>>,
    pub(crate) bars: Mutex<HashMap<String, BarRuntime>>,
    pub(crate) capture_signature: Mutex<Option<String>>,
    pub(crate) ident_cache: Mutex<HashMap<String, Ident>>,
    /// True while the user shows a static colour / cinema (screen sync paused).
    pub(crate) sync_paused: Mutex<bool>,
    pub(crate) cinema: Mutex<Option<CinemaRuntime>>,
    pub(crate) animation: Mutex<Option<AnimationRuntime>>,
    pub(crate) identify: Mutex<Option<IdentifyRuntime>>,
}