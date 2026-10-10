//! Non-command runtime logic: capture/bar/cinema lifecycle, autostart and the
//! background threads (hotplug watcher, setup watchdog).

use crate::config::{read_config, update_config, BarConfig, Config, Ident};
use crate::logging::log;
use crate::monitors;
use crate::state::{AppState, BarRuntime, CinemaRuntime};
use bloqsync::capture::StreamInfo;
use bloqsync::cinema::{Cinema, CinemaParams};
use bloqsync::device::{enumerate, find_by_identity, Device, DeviceInfo};
use bloqsync::engine::{spawn, SyncConfig};
use bloqsync::filters::FilterKind;
use bloqsync::sampling::Layout;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

/// Sync parameters shared by every bar of one update; grouped so the numerous
/// `Config` fields are mapped to engine arguments in exactly one place.
pub(crate) struct BarSettings {
    pub(crate) fps: u32,
    pub(crate) smooth: f32,
    pub(crate) layout: Layout,
    pub(crate) brightness: Option<u8>,
    pub(crate) filter: FilterKind,
    pub(crate) filter_strength: u8,
    pub(crate) max_frames: usize,
}

impl BarSettings {
    pub(crate) fn from_config(cfg: &Config) -> Self {
        BarSettings {
            fps: cfg.fps,
            smooth: cfg.smooth,
            layout: cfg.layout(),
            brightness: Some(cfg.brightness),
            filter: parse_filter(Some(&cfg.filter)),
            filter_strength: cfg.filter_strength,
            max_frames: cfg.max_frames as usize,
        }
    }
}

pub(crate) fn parse_filter(name: Option<&str>) -> FilterKind {
    match name {
        Some("deadband") => FilterKind::Deadband,
        Some("quantize") => FilterKind::Quantize,
        Some("quantize_smooth") => FilterKind::QuantizeSmooth,
        Some("smooth") => FilterKind::Smooth,
        Some("median3") => FilterKind::Median3,
        Some("median5") => FilterKind::Median5,
        Some("mean4") => FilterKind::Mean4,
        Some("test") => FilterKind::Test,
        _ => FilterKind::None,
    }
}

pub(crate) fn open_capture_inner(state: &AppState) -> Result<Vec<StreamInfo>, String> {
    open_capture_with(state, None)
}

/// Open the ScreenCast session (multiple monitors). Uses the stored restore
/// token if present, so after the first selection no dialog is shown.
pub(crate) fn open_capture_with(
    state: &AppState,
    token_override: Option<String>,
) -> Result<Vec<StreamInfo>, String> {
    let mut cap = state.capture.lock().unwrap();
    if cap.is_none() {
        let sig = monitors::signature();
        let token = token_override.or_else(|| {
            let cfg = read_config();
            let mut t = cfg.active_profile(&sig).and_then(|p| p.restore_token.clone());
            if t.is_none() {
                t = cfg.restore_token.clone();
            }
            t
        });
        let c = bloqsync::capture::start(true, token).map_err(|e| e.to_string())?;
        log(&format!(
            "capture opened: {} stream(s) {:?}, token={}, setup={}",
            c.streams.len(),
            c.streams.iter().map(|s| (s.index, s.size)).collect::<Vec<_>>(),
            c.restore_token.is_some(),
            sig
        ));
        if let Some(tok) = &c.restore_token {
            update_config(|cfg| {
                cfg.upsert_profile_token(&sig, tok);
                cfg.restore_token = Some(tok.clone());
            });
        }
        *state.capture_signature.lock().unwrap() = Some(sig);
        *cap = Some(Arc::new(c));
    }
    Ok(cap.as_ref().unwrap().streams.clone())
}

pub(crate) fn stop_all_inner(state: &AppState) {
    let mut bars = state.bars.lock().unwrap();
    for (_, b) in bars.drain() {
        b.sync.stop();
        let _ = b.device.set_persistent_color([255, 200, 100]);
    }
}

/// Stop a running identify/blink loop and wait for it to finish. The identify
/// thread deliberately does not restore the previous sync when stopped this
/// way, so the caller can start the desired mode instead.
pub(crate) fn stop_identify_inner(state: &AppState) {
    if let Some(rt) = state.identify.lock().unwrap().take() {
        rt.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = rt.thread.join();
    }
}

/// Find, open and remember the device for `bar_path`, updating the identity
/// cache so the UI keeps a stable key for it.
fn open_bar_device(state: &AppState, bar_path: &str) -> Result<Arc<Device>, String> {
    let info = find_by_identity(bar_path).ok_or_else(|| "Leiste nicht gefunden".to_string())?;
    let device = Arc::new(Device::open(&info).map_err(|e| e.to_string())?);
    state.ident_cache.lock().unwrap().insert(
        device.info.id.clone(),
        Ident {
            uuid: device.uuid.clone(),
            leds: device.led_count,
            firmware: device.firmware.clone(),
        },
    );
    persist_identities(state);
    Ok(device)
}

/// Remember a bar → monitor mapping for the current monitor setup.
fn remember_mapping(bar_path: &str, stream_index: usize, reverse: bool, model_id: Option<String>) {
    let sig = monitors::signature();
    update_config(|c| {
        c.upsert_profile_bar(
            &sig,
            BarConfig {
                bar_path: bar_path.to_string(),
                stream_index,
                reverse,
                model_id,
            },
        );
    });
}

/// Start (or restart) screen sync for one bar on `stream_index`.
pub(crate) fn start_bar_inner(
    state: &AppState,
    bar_path: &str,
    stream_index: usize,
    reverse: bool,
    model_id: Option<String>,
    settings: &BarSettings,
) -> Result<(), String> {
    let capture = state
        .capture
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Aufnahme nicht gestartet".to_string())?;
    let slot = capture
        .slots
        .get(stream_index)
        .cloned()
        .ok_or_else(|| format!("Stream {stream_index} existiert nicht"))?;

    if let Some(prev) = state.bars.lock().unwrap().remove(bar_path) {
        prev.sync.stop();
    }

    let device = open_bar_device(state, bar_path)?;
    if let Some(b) = settings.brightness {
        let _ = device.set_brightness(b);
    }

    let cfg = SyncConfig {
        fps: settings.fps,
        smoothing: settings.smooth,
        reverse,
        filter: settings.filter,
        filter_strength: settings.filter_strength,
        max_frames: settings.max_frames,
        layout: settings.layout,
        ..Default::default()
    };
    let sync = spawn(device.clone(), slot, cfg).map_err(|e| e.to_string())?;
    state
        .bars
        .lock()
        .unwrap()
        .insert(bar_path.to_string(), BarRuntime { device, sync });
    remember_mapping(bar_path, stream_index, reverse, model_id);
    Ok(())
}

pub(crate) fn cinema_stop_inner(state: &AppState) {
    if let Some(c) = state.cinema.lock().unwrap().take() {
        c.stop();
    }
}

/// Audio-reactive "cinema" light for DRM content (Netflix etc.).
pub(crate) fn cinema_start_inner(
    state: &AppState,
    sensitivity: f32,
    brightness: f32,
    base: [u8; 3],
    onset_gain: f32,
    floor: f32,
    smooth: f32,
    contrast: f32,
) -> Result<(), String> {
    cinema_stop_inner(state);
    *state.sync_paused.lock().unwrap() = true;
    stop_all_inner(state);
    // Keep the screen capture (and therefore the monitor assignment) alive so
    // switching back to screen sync does not require reconnecting monitors.

    let audio = bloqsync::audio::start(None).map_err(|e| e.to_string())?;
    let spectrum = audio.spectrum.clone();
    let mut devices = Vec::new();
    for info in enumerate() {
        if let Ok(d) = Device::open(&info) {
            devices.push(Arc::new(d));
        }
    }
    if devices.is_empty() {
        return Err("keine Leiste gefunden".into());
    }
    log(&format!("cinema started on {} bar(s), source {}", devices.len(), audio.source));
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let running_t = running.clone();
    let params = CinemaParams {
        base,
        sensitivity,
        master: brightness,
        onset_gain,
        floor,
        smooth_secs: smooth,
        contrast_gain: contrast,
        ..Default::default()
    };
    let devices_thread = devices.clone();
    let thread = std::thread::Builder::new()
        .name("bloqsync-cinema".into())
        .spawn(move || {
            let mut states: Vec<Cinema> = (0..devices_thread.len()).map(|_| Cinema::new()).collect();
            let period = Duration::from_millis(20);
            while running_t.load(std::sync::atomic::Ordering::Relaxed) {
                let t = Instant::now();
                let s = spectrum.lock().unwrap().unwrap_or_default();
                for (i, d) in devices_thread.iter().enumerate() {
                    let cols = states[i].render(&s, d.led_count, &params);
                    let _ = d.send_colors_paced(&cols, Duration::from_millis(3), 8);
                }
                let e = t.elapsed();
                if e < period {
                    std::thread::sleep(period - e);
                }
            }
        })
        .map_err(|e| e.to_string())?;
    *state.cinema.lock().unwrap() = Some(CinemaRuntime {
        audio,
        running,
        devices,
        thread,
    });
    Ok(())
}

/// Persist the in-memory device identities into the config.
pub(crate) fn persist_identities(state: &AppState) {
    let snap = state.ident_cache.lock().unwrap().clone();
    update_config(|cfg| cfg.persist_uuids(&snap));
}

/// Auto-start the configured sync (used at launch and on hotplug).
pub(crate) fn autostart_run_inner(state: &AppState) -> Result<(), String> {
    let cfg = read_config();
    let sig = monitors::signature();
    let prof = match cfg.active_profile(&sig) {
        Some(p) => p.clone(),
        None => {
            log(&format!("autostart: no profile for setup {sig} - waiting"));
            return Ok(());
        }
    };
    let (token, bars) = (prof.restore_token.clone(), prof.bars.clone());
    if bars.is_empty() {
        return Ok(());
    }
    log(&format!("autostart: setup={sig} bars={}", bars.len()));
    // Ensure capture is open (uses profile/top-level restore token).
    open_capture_with(state, token)?;
    let settings = BarSettings::from_config(&cfg);
    for b in &bars {
        let _ = start_bar_inner(
            state,
            &b.bar_path,
            b.stream_index,
            b.reverse,
            b.model_id.clone(),
            &settings,
        );
    }
    Ok(())
}

fn autostart_file() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/autostart/bloqsync.desktop")
}

pub(crate) fn autostart_enabled() -> bool {
    autostart_file().exists()
}

pub(crate) fn set_autostart(enabled: bool) -> Result<(), String> {
    let file = autostart_file();
    if enabled {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let contents = format!(
            "[Desktop Entry]\nType=Application\nName=bloqsync\nComment=Ambient light sync\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        std::fs::write(&file, contents).map_err(|e| e.to_string())?;
    } else if file.exists() {
        std::fs::remove_file(&file).map_err(|e| e.to_string())?;
    }
    update_config(|cfg| cfg.autostart = enabled);
    Ok(())
}

/// Seed the device-identity cache from the config so the UI keeps stable keys
/// even before/without a successful identify.
pub(crate) fn seed_ident_cache(app: &tauri::AppHandle) {
    let st = app.state::<AppState>();
    let cfg = read_config();
    let mut cache = st.ident_cache.lock().unwrap();
    for (id, ident) in cfg.bar_uuids {
        cache.insert(id, ident);
    }
}

/// Auto-start sync once at launch (done in Rust, not via the UI).
pub(crate) fn spawn_autostart(app: tauri::AppHandle) {
    if !read_config().autostart_sync {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(2000));
        let state = app.state::<AppState>();
        match autostart_run_inner(&state) {
            Ok(()) => log("bloqsync: autostart ok"),
            Err(e) => log(&format!("bloqsync: autostart failed: {e}")),
        }
        let _ = app.emit("autostart", ());
    });
}

/// Hotplug: watch for bar add/remove.
pub(crate) fn spawn_hotplug(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let ids = |v: &[DeviceInfo]| v.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        let mut last = ids(&enumerate());
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let now = ids(&enumerate());
            if now != last {
                last = now.clone();
                let _ = app.emit("devices-changed", &now);
            }
        }
    });
}

/// Watchdog: keep the capture in sync with the current monitor setup and the
/// configured bars running (profile switching + reconnect).
pub(crate) fn spawn_watchdog(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        watchdog_tick(&app);
    });
}

/// True if the open capture no longer matches the current monitor setup.
fn capture_is_stale(state: &AppState, sig: &str) -> bool {
    let cap_open = state.capture.lock().unwrap().is_some();
    let cap_sig = state.capture_signature.lock().unwrap().clone();
    cap_open && cap_sig.as_deref() != Some(sig)
}

/// True if `bar_path` is configured but not currently running.
fn bar_needs_start(state: &AppState, bar_path: &str) -> bool {
    let bars = state.bars.lock().unwrap();
    match bars.get(bar_path) {
        Some(rt) => !rt.sync.is_running(),
        None => true,
    }
}

fn watchdog_tick(app: &tauri::AppHandle) {
    let cfg = read_config();
    if !cfg.autostart_sync {
        return;
    }
    let state = app.state::<AppState>();
    if *state.sync_paused.lock().unwrap() {
        return;
    }
    let sig = monitors::signature();
    if sig.is_empty() {
        return;
    }
    if capture_is_stale(&state, &sig) {
        log(&format!("setup changed -> {sig}: restarting capture"));
        stop_all_inner(&state);
        *state.capture.lock().unwrap() = None;
        *state.capture_signature.lock().unwrap() = None;
    }
    // Only act on setups we have a profile for (no dialog surprises).
    let prof = match cfg.active_profile(&sig).cloned() {
        Some(p) => p,
        None => return,
    };
    if state.capture.lock().unwrap().is_none() {
        if let Err(e) = open_capture_with(&state, prof.restore_token.clone()) {
            log(&format!("bloqsync: watchdog capture: {e}"));
            return;
        }
    }
    let settings = BarSettings::from_config(&cfg);
    for b in &prof.bars {
        if bar_needs_start(&state, &b.bar_path) {
            match start_bar_inner(
                &state,
                &b.bar_path,
                b.stream_index,
                b.reverse,
                b.model_id.clone(),
                &settings,
            ) {
                Ok(()) => log(&format!("bloqsync: watchdog started {}", b.bar_path)),
                Err(e) => log(&format!("bloqsync: watchdog {}: {e}", b.bar_path)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_filter_maps_known_names() {
        assert_eq!(parse_filter(Some("deadband")), FilterKind::Deadband);
        assert_eq!(parse_filter(Some("quantize")), FilterKind::Quantize);
        assert_eq!(parse_filter(Some("quantize_smooth")), FilterKind::QuantizeSmooth);
        assert_eq!(parse_filter(Some("smooth")), FilterKind::Smooth);
        assert_eq!(parse_filter(Some("median3")), FilterKind::Median3);
        assert_eq!(parse_filter(Some("median5")), FilterKind::Median5);
        assert_eq!(parse_filter(Some("mean4")), FilterKind::Mean4);
        assert_eq!(parse_filter(Some("test")), FilterKind::Test);
    }

    #[test]
    fn parse_filter_falls_back_to_none() {
        assert_eq!(parse_filter(None), FilterKind::None);
        assert_eq!(parse_filter(Some("nonsense")), FilterKind::None);
    }

    #[test]
    fn bar_settings_maps_config() {
        let mut cfg = Config::default();
        (cfg.fps, cfg.smooth) = (30, 0.4);
        (cfg.left, cfg.top, cfg.right, cfg.bottom) = (1, 2, 3, 4);
        cfg.brightness = 180;
        cfg.filter = "median3".into();
        cfg.filter_strength = 12;
        cfg.max_frames = 3;

        let s = BarSettings::from_config(&cfg);
        assert_eq!(s.fps, 30);
        assert_eq!(s.smooth, 0.4);
        assert_eq!((s.layout.left, s.layout.top, s.layout.right, s.layout.bottom), (1, 2, 3, 4));
        assert_eq!(s.brightness, Some(180));
        assert_eq!(s.filter, FilterKind::Median3);
        assert_eq!(s.filter_strength, 12);
        assert_eq!(s.max_frames, 3);
    }
}